//! Фоновые задачи — СОБЫТИЙНЫЕ, не фиксированно-периодические:
//! (1) перечитывание реестра из БД (подхват добавленных/убранных подписок),
//! (2) free count-tokens quota probe для простаивающих подписок и post-turn калибровки.
//!
//! Ключевая идея: источник истины лимитов — заголовки боевых ответов (пассивный сбор в `forward`
//! обновляет `polled_ts`). Момент сброса окна вычисляется локально (`reset` — абсолютный timestamp),
//! поэтому его НЕ надо «ловить» опросом. Активный probe впервые узнаёт лимиты новой подписки,
//! редко проверяет liveness простаивающей и после authoritative turn связывает exact spend с
//! post-turn quota fraction. Никакого
//! фиксированного скана подписок: спим ровно до ближайшего due-времени или до сигнала об изменении
//! флота/завершении turn. Billing recovery и retention остаются отдельными maintenance-циклами.

use forward::{detect_plan, persona_ua, poll_sub, AppState, PlanDetect};
use registry::Sub;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinSet;

/// Run bounded retention continuously enough to keep pace with request-volume tables.
const LEDGER_PRUNE_INTERVAL: u64 = 60;

/// Recovery must outlive startup: reservations deliberately carry leases, so an immediate restart
/// cannot recover them until a later pass. A short leader lease permits blue/green overlap without
/// duplicate scanning; request-level advisory locks keep settlement idempotent as a second fence.
const BILLING_RECOVERY_INTERVAL: u64 = 5;
const BILLING_RECOVERY_LEADER_TTL: i64 = 15;
const BILLING_RECOVERY_BATCH: usize = 10_000;
const CODEX_HEALTH_MIN_INTERVAL_SECS: u64 = 10;

fn codex_health_interval_secs(configured: u64) -> u64 {
    configured.max(CODEX_HEALTH_MIN_INTERVAL_SECS)
}

pub async fn billing_recovery_loop(
    authority: registry::authority::AuthorityConfig,
    owner: Option<registry::pg::Owner>,
    authority_ready: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;

    loop {
        if authority_ready.load(Ordering::Acquire) {
            let config = authority.clone();
            let current = owner.clone();
            let recovered = tokio::task::spawn_blocking(move || {
                let mut authority = config.connect()?;
                if let Some(current) = current.as_ref() {
                    let pg = authority.postgres()?;
                    if !pg.acquire_leader(
                        current,
                        "billing-recovery",
                        BILLING_RECOVERY_LEADER_TTL,
                    )? {
                        return Ok::<_, anyhow::Error>(None);
                    }
                }
                Ok(Some(authority.reconcile_expired(
                    BILLING_RECOVERY_BATCH,
                    forward::settlement_policy::charge_hold_on_unknown_usage(),
                )?))
            })
            .await;

            match recovered {
                Ok(Ok(Some(report)))
                    if report.canceled_before_delivery > 0
                        || report.charged_after_delivery > 0
                        || report.processed_outbox > 0 =>
                {
                    elog::info(
                        "server",
                        format!(
                            "billing recovery: canceled={}, charged={}, outbox={}",
                            report.canceled_before_delivery,
                            report.charged_after_delivery,
                            report.processed_outbox,
                        ),
                    );
                }
                Ok(Ok(_)) => {}
                Ok(Err(err)) => elog::error("server", format!("billing recovery pass failed: {err:#}")),
                Err(err) => elog::error("server", format!("billing recovery task failed: {err}")),
            }
        }

        tokio::time::sleep(Duration::from_secs(BILLING_RECOVERY_INTERVAL)).await;
    }
}

/// Фоновая обрезка bulk history и terminal request machinery.
///
/// Request lifecycle retention is deliberately independent from ledger/usage retention: it is the
/// backing store for the bounded pricing-snapshot idempotency contract and must not shrink merely
/// because an operator changes analytics retention later.
pub async fn ledger_prune_loop(
    authority: registry::authority::AuthorityConfig,
    ledger_retention_days: i64,
    request_lifecycle_retention_days: i64,
) {
    if ledger_retention_days <= 0 && request_lifecycle_retention_days <= 0 {
        return;
    }
    loop {
        tokio::time::sleep(Duration::from_secs(LEDGER_PRUNE_INTERVAL)).await;
        let now = pool::now();
        let ledger_cutoff = now - ledger_retention_days.max(0) * 86400;
        let lifecycle_cutoff = now - request_lifecycle_retention_days.max(0) * 86400;
        let db = authority.clone();
        let res = tokio::task::spawn_blocking(move || {
            db.connect().and_then(|mut c| {
                let mut led = 0usize;
                let mut usg = 0usize;
                let mut lifecycle = registry::pg::MaintenanceReport::default();
                for _ in 0..20 {
                    let ledger_batch = if ledger_retention_days > 0 {
                        c.ledger_prune(ledger_cutoff)?
                    } else {
                        0
                    };
                    let usage_batch = if ledger_retention_days > 0 {
                        c.usage_prune(ledger_cutoff)?
                    } else {
                        0
                    };
                    let life = if request_lifecycle_retention_days > 0 {
                        c.maintenance_prune(lifecycle_cutoff)?
                    } else {
                        registry::pg::MaintenanceReport::default()
                    };
                    led += ledger_batch;
                    usg += usage_batch;
                    lifecycle.outbox += life.outbox;
                    lifecycle.reservations += life.reservations;
                    lifecycle.capacity_leases += life.capacity_leases;
                    lifecycle.engine_instances += life.engine_instances;
                    let full = ledger_batch >= 5_000
                        || usage_batch >= 5_000
                        || life.outbox >= 5_000
                        || life.reservations >= 5_000
                        || life.capacity_leases >= 5_000;
                    if !full {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok((led, usg, lifecycle))
            })
        })
        .await;
        match res {
            Ok(Ok((led, usg, life))) if led > 0 || usg > 0 || life.outbox > 0
                || life.reservations > 0
                || life.capacity_leases > 0
                || life.engine_instances > 0 =>
                elog::info(
                    "server",
                    format!(
                        "retention: ledger={led}, usage={usg}, outbox={}, reservations={}, capacity={}, instances={} (history>{ledger_retention_days}d, request_lifecycle>{request_lifecycle_retention_days}d)",
                        life.outbox,
                        life.reservations,
                        life.capacity_leases,
                        life.engine_instances,
                    ),
                ),
            Ok(Err(e)) => elog::error("server", format!("ledger-обрезка не удалась: {e}")),
            Err(e) => elog::error("server", format!("ledger-обрезка: задача упала: {e}")),
            _ => {}
        }
    }
}

/// Коллектор истории: раз в минуту пишет снапшот агрегата (спрос/предложение/headroom/рекомендация)
/// в ОТДЕЛЬНУЮ metrics.db. Фундамент под capacity-planning и предсказательную модель — учиться она
/// будет на накопленной истории. Не мешает биллингу (своя БД, blocking-поток, редкая запись).
/// Periodic read-only health sweep over the Codex home pool.
///
/// A ChatGPT device login expires on its own schedule, with no request traffic to reveal it. Without
/// this loop a home would stay silently unusable until a customer request selected it — and a pool
/// of one would mean a total provider outage discovered by the customer. The sweep also refreshes
/// the rate-limit snapshot that the admission gate and the `/metrics` gauges read, and it lets a
/// re-authenticated home rejoin the rotation without an engine restart.
pub async fn codex_health_loop(
    gateway: Arc<forward::CodexGateway>,
    admin_changes: tokio::sync::broadcast::Sender<forward::AdminChange>,
) {
    let interval = Duration::from_secs(codex_health_interval_secs(
        gateway.config().health_probe_interval_secs,
    ));
    // Seed the last-good model snapshot without making startup or customer discovery wait on it.
    gateway.refresh_model_catalog().await;
    let mut status_fingerprint = codex_admin_fingerprint(&gateway).await;
    loop {
        // Race the steady cadence against a forced-probe request from the data path. A healthy pool
        // keeps its normal interval; a home that just failed a customer turn is re-checked at once,
        // instead of staying routable until the next scheduled sweep.
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = gateway.probe_requested() => {}
        }
        gateway.probe_health().await;
        let next = codex_admin_fingerprint(&gateway).await;
        if next != status_fingerprint {
            status_fingerprint = next;
            let _ = admin_changes.send(forward::AdminChange::engine(
                &["/codex-subs", "/capacity", "/overview"],
                "codex_health",
            ));
        }
    }
}

async fn codex_admin_fingerprint(gateway: &forward::CodexGateway) -> String {
    let status = gateway.operational_status().await;
    format!(
        "{}|{}|{:?}|{:?}",
        status.process_live,
        status.available,
        status.soonest_ready,
        status
            .homes
            .iter()
            .map(|home| {
                (
                    &home.id,
                    home.admitted,
                    home.reject_reason,
                    home.cooling_until,
                    home.limit_reached,
                    home.rate_limits.as_ref().map(|limits| {
                        (
                            limits.primary.as_ref().map(|window| {
                                (
                                    window.used_fraction_units,
                                    window.window_duration_mins,
                                    window.resets_at,
                                )
                            }),
                            limits.secondary.as_ref().map(|window| {
                                (
                                    window.used_fraction_units,
                                    window.window_duration_mins,
                                    window.resets_at,
                                )
                            }),
                            limits.reached,
                        )
                    }),
                )
            })
            .collect::<Vec<_>>()
    )
}

/// Periodically validates every paid Gemini project so an expired/disabled key is quarantined
/// before customer traffic needs that profile, and a repaired credential rejoins automatically.
pub async fn gemini_health_loop(
    gateway: Arc<forward::GeminiGateway>,
    admin_changes: tokio::sync::broadcast::Sender<forward::AdminChange>,
) {
    const PROFILE_DISCOVERY_SECS: u64 = 15;
    let health_interval = Duration::from_secs(gateway.config().health_probe_interval_secs.max(30));
    let mut last_health = tokio::time::Instant::now();
    let mut status_fingerprint = gemini_admin_fingerprint(&gateway).await;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(PROFILE_DISCOVERY_SECS)) => {
                if last_health.elapsed() >= health_interval {
                    gateway.probe_health().await;
                    last_health = tokio::time::Instant::now();
                } else {
                    gateway.refresh_profiles().await;
                }
                let next = gemini_admin_fingerprint(&gateway).await;
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/gemini-subs", "/capacity", "/overview"],
                        "gemini_profiles",
                    ));
                }
            }
            _ = gateway.probe_requested() => {
                gateway.probe_health().await;
                last_health = tokio::time::Instant::now();
                let next = gemini_admin_fingerprint(&gateway).await;
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/gemini-subs", "/capacity", "/overview"],
                        "gemini_health",
                    ));
                }
            }
        }
    }
}

async fn gemini_admin_fingerprint(gateway: &forward::GeminiGateway) -> String {
    let status = gateway.operational_status().await;
    format!(
        "{}|{}|{:?}|{:?}",
        status.available,
        status.authenticated,
        status.soonest_ready,
        status
            .profiles
            .iter()
            .map(|profile| {
                (
                    &profile.id,
                    profile.authenticated,
                    profile.disabled,
                    profile.hidden,
                    profile.cooling_until,
                    &profile.quotas,
                )
            })
            .collect::<Vec<_>>()
    )
}

/// Discover atomic Auth Bot roster publications and poll free subscription quota independently.
/// The gateway owns profile-idle exclusion, turn-FIFO ordering and durable observation/CAS; this
/// server loop owns only cadence and never consumes customer concurrency permits.
pub async fn kimi_maintenance_loop(
    gateway: Arc<forward::KimiGateway>,
    admin_changes: tokio::sync::broadcast::Sender<forward::AdminChange>,
) {
    const PROFILE_DISCOVERY_SECS: u64 = 15;
    let mut discovery = tokio::time::interval(Duration::from_secs(PROFILE_DISCOVERY_SECS));
    discovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The startup preflight just authenticated the roster. Delay discovery's first immediate tick
    // while allowing quota to anchor immediately without waiting a whole configured interval.
    discovery.tick().await;
    let mut quota = tokio::time::interval(gateway.quota_poll_interval());
    quota.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut status_fingerprint = kimi_admin_fingerprint(&gateway);
    loop {
        tokio::select! {
            _ = discovery.tick() => {
                gateway.refresh_profiles().await;
                let next = kimi_admin_fingerprint(&gateway);
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/kimi-subs", "/capacity", "/overview"],
                        "kimi_profiles",
                    ));
                }
            }
            _ = quota.tick() => {
                gateway.poll_quotas().await;
                let next = kimi_admin_fingerprint(&gateway);
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/kimi-subs", "/capacity", "/overview"],
                        "kimi_quota",
                    ));
                }
            }
        }
    }
}

fn kimi_admin_fingerprint(gateway: &forward::KimiGateway) -> String {
    let status = gateway.operational_status();
    format!(
        "{}|{}|{}|{}|{}|{:?}",
        status.total_profiles,
        status.live_profiles,
        status.available_profiles,
        status.auth_quarantined_profiles,
        status.quota_cooling_profiles,
        status
            .profiles
            .iter()
            .map(|profile| (
                &profile.id,
                profile.live,
                profile.auth_quarantined_until,
                profile.transport_cool_until,
                profile.quota_cool_until,
                profile
                    .quota_windows
                    .iter()
                    .map(|window| {
                        (
                            window.duration_secs,
                            window.used_units,
                            window.limit_units,
                            window.resets_at,
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
            .collect::<Vec<_>>()
    )
}

/// GLM mirror of the KIMI maintenance loop (`docs/engine/GLM_PROVIDER.md` §5.4): discover
/// atomic Auth Bot roster publications on the independent 15-second tick and run the free
/// quota sweep on `CLAUDE_API_GLM_QUOTA_POLL_SECS`. The gateway owns profile-idle exclusion,
/// turn-FIFO turn-before-quota ordering and durable observation/CAS; this server loop owns only
/// cadence and never consumes customer concurrency permits.
pub async fn glm_maintenance_loop(
    gateway: Arc<forward::glm::GlmGateway>,
    admin_changes: tokio::sync::broadcast::Sender<forward::AdminChange>,
) {
    const PROFILE_DISCOVERY_SECS: u64 = 15;
    let mut discovery = tokio::time::interval(Duration::from_secs(PROFILE_DISCOVERY_SECS));
    discovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The startup preflight just authenticated the roster. Delay discovery's first immediate tick
    // while allowing quota to anchor immediately without waiting a whole configured interval.
    discovery.tick().await;
    let mut quota = tokio::time::interval(gateway.quota_poll_interval());
    quota.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut status_fingerprint = glm_admin_fingerprint(&gateway);
    loop {
        tokio::select! {
            _ = discovery.tick() => {
                gateway.refresh_profiles().await;
                let next = glm_admin_fingerprint(&gateway);
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/glm-subs", "/capacity", "/overview"],
                        "glm_profiles",
                    ));
                }
            }
            _ = quota.tick() => {
                gateway.poll_quotas().await;
                let next = glm_admin_fingerprint(&gateway);
                if next != status_fingerprint {
                    status_fingerprint = next;
                    let _ = admin_changes.send(forward::AdminChange::engine(
                        &["/glm-subs", "/capacity", "/overview"],
                        "glm_quota",
                    ));
                }
            }
        }
    }
}

fn glm_admin_fingerprint(gateway: &forward::glm::GlmGateway) -> String {
    let status = gateway.operational_status();
    format!(
        "{}|{}|{}|{}|{}|{}|{:?}",
        status.total_profiles,
        status.live_profiles,
        status.available_profiles,
        status.account_dead_profiles,
        status.account_suspect_profiles,
        status.quota_cooling_profiles,
        status
            .profiles
            .iter()
            .map(|profile| (
                &profile.id,
                profile.live,
                profile.account_dead,
                profile.account_suspect,
                profile.transport_cool_until,
                profile.quota_cool_until,
                profile
                    .quota_windows
                    .iter()
                    .map(|window| {
                        (
                            window.duration_secs,
                            window.used_units,
                            window.limit_units,
                            window.resets_at,
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
            .collect::<Vec<_>>()
    )
}

pub async fn metrics_loop(app: AppState, metrics_db: String, retention_days: i64) {
    const SNAP_SECS: u64 = 60;
    let mut ticks: u64 = 0;
    loop {
        let snap = match crate::http::overview_value(&app).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                elog::warn(
                    "server",
                    format!("metrics snapshot skipped: billing authority unavailable: {error:#}"),
                );
                tokio::time::sleep(Duration::from_secs(SNAP_SECS)).await;
                continue;
            }
        }; // тот же агрегат, что и /overview
        let now = pool::now();
        // Per-sub history must use the same exact authority as `/capacity` and `/overview`. The old
        // pool::Cap EMA/prior floats are routing hints and must never create a second peak history.
        // Full email remains local to metrics.db; Control API values are still bounded masks.
        let caps = app.pool.capacity();
        let subs: Vec<(String, f64, f64, f64, f64)> = match &app.billing {
            Some(billing) => match billing.anthropic_calibration_report().await {
                Ok(report) => {
                    let exact = crate::http::capacity_value(
                        &caps,
                        Some(&report),
                        Some(billing.anthropic_calibration_delivery_status()),
                        now,
                    );
                    let rows = exact["per_sub"].as_array().cloned().unwrap_or_default();
                    caps.iter()
                        .zip(rows)
                        .filter_map(|(cap, row)| {
                            let dollars = |field: &str| {
                                row[field]
                                    .as_str()
                                    .and_then(|value| value.parse::<i128>().ok())
                                    .map(|value| value as f64 / 1e9)
                            };
                            Some((
                                cap.email.clone(),
                                dollars("cap5h_nano")?,
                                dollars("cap7d_nano")?,
                                row["util5h"].as_f64()?,
                                row["util7d"].as_f64()?,
                            ))
                        })
                        .collect()
                }
                Err(error) => {
                    elog::warn(
                        "server",
                        format!("exact per-sub metrics snapshot skipped: {error:#}"),
                    );
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let db = metrics_db.clone();
        let cutoff = if retention_days > 0 {
            now - retention_days * 86400
        } else {
            0
        };
        // Примерно раз в час.
        let do_prune = retention_days > 0 && ticks.is_multiple_of(60);
        // Запись в SQLite — на blocking-потоке (не блокируем async-воркер). Открываем per-write:
        // снапшоты редки (60с), проще, чем таскать не-Sync Connection через .await.
        let stored = tokio::task::spawn_blocking(move || {
            let c = crate::metrics_store::open(&db)?;
            crate::metrics_store::insert_snapshot(&c, &snap)?;
            crate::metrics_store::insert_sub_snapshots(&c, now, &subs)?;
            if do_prune {
                crate::metrics_store::prune(&c, cutoff)?;
            }
            Ok::<_, rusqlite::Error>(())
        })
        .await;
        if matches!(stored, Ok(Ok(()))) {
            let _ = app.admin_changes.send(forward::AdminChange::engine(
                &["/fleet-history"],
                "metrics_snapshot",
            ));
        }
        ticks = ticks.wrapping_add(1);
        tokio::time::sleep(Duration::from_secs(SNAP_SECS)).await;
    }
}

/// Простаивающую подписку пингуем не чаще этого (сек). Не для «поймать сброс» (он вычисляется), а
/// лишь для проверки живости токена. Authoritative turn отдельно форсит bounded post-turn probe.
const LIVENESS_INTERVAL: i64 = 300;
/// Suspect (auth падает, но ещё не приговор): probe чаще и НЕЗАВИСИМО от cooling — иначе `cool(900)`
/// после 401/403-probe отложил бы следующий probe. 300с → второй probe приходит на ~5-й минуте, и
/// (при DEAD_STREAK=2, DEAD_MIN_SECS=300) подписка становится DEAD ровно на 2-м вызове в 5 минут.
const SUSPECT_INTERVAL: i64 = 300;
/// Dead (токен корроборированно мёртв): медленный resurrection-probe — авто-ревайв, если токен снова
/// заработал (заменён/разбанен), не долбя забаненный аккаунт часто. Тоже cooling-независимо.
const DEAD_RESURRECT_INTERVAL: i64 = 3600;
/// Потолок одновременных probe (медленный прокси не блокирует остальных).
const PROBE_CONCURRENCY: usize = 16;
/// Потолок сна (сек) — верхняя граница ожидания, чтобы liveness не растягивался бесконечно.
const MAX_SLEEP: i64 = 300;

/// Когда подписка «созреет» для активного probe. `polled_ts==0` (никогда не опрошена) → сейчас.
/// - **Dead/Suspect** — probe по своему интервалу и НЕЗАВИСИМО от cooling: dead уже вне ротации
///   (`is_dead`), suspect обычно cooled после 401/403, но корроборацию/ресуррекцию надо продолжать.
/// - **Healthy** — прежняя логика: пока подписка в COOLING, её состояние АВТОРИТЕТНО (reset вычислен,
///   quarantine активен), probe не нужен и ВРЕДЕН (амплификация бан-сигнала) → откладываем до конца
///   cooling. `max` держит инвариант единой точки (due-фильтр == сон).
fn next_probe_at(email: &str, live: &pool::Live) -> i64 {
    match live.auth_state {
        pool::AuthState::Dead => live.polled_ts + DEAD_RESURRECT_INTERVAL,
        pool::AuthState::Suspect => live.polled_ts + SUSPECT_INTERVAL,
        pool::AuthState::Healthy => {
            // Per-persona джиттер (0..LIVENESS_INTERVAL/2, стабилен по email): без него весь idle-флот
            // созрел бы РОВНО в T+300 → синхронный залп идентичных probe (временна́я корреляция поверх
            // фингерпринта). Джиттер разводит их. Cooling НЕ джиттерим — он авторитетен.
            let base = if live.polled_ts == 0 {
                0
            } else {
                let jitter =
                    (pool::stable_hash64(email.as_bytes()) % (LIVENESS_INTERVAL as u64 / 2)) as i64;
                live.polled_ts + LIVENESS_INTERVAL + jitter
            };
            base.max(live.cooling_until)
        }
    }
}

/// Перечитывание реестра: подхватываем добавленные/убранные подписки. Спит 30с (локальная БД,
/// дёшево), но будит поллер `poke` ТОЛЬКО когда набор реально изменился (онбординг новой подписки
/// не ждёт liveness-таймера).
pub async fn reload_loop(
    app: AppState,
    authority: registry::authority::AuthorityConfig,
    fleet: Option<String>,
    poke: Arc<Notify>,
) {
    let mut prev: HashSet<String> = HashSet::new();
    loop {
        // синхронный SQLite → на blocking-пул, чтобы не держать async-воркер (пусть и на 30с редко)
        let (db, fl) = (authority.clone(), fleet.clone());
        let loaded = tokio::task::spawn_blocking(move || {
            db.connect().and_then(|mut c| c.load_active(fl.as_deref()))
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("reload task panicked: {e}")));
        match loaded {
            Err(e) => elog::warn("server", format!("reload реестра не удался (держим прежний список): {e}")),
            Ok(subs) => {
                let cur: HashSet<String> = subs.iter().map(|s| s.email.clone()).collect();
                let membership_changed = cur != prev;
                app.breaker.set_fleet_size(subs.len());
                // ВСЕГДА заменяем: подхватываем смену token/proxy того же email (внешняя
                // перепровизия authbot/CLI на живом сервере) — иначе держали бы протухший до рестарта.
                // replace_subs сохраняет volatile-состояние существующих (retain по email).
                let roster_changed = app.pool.replace_subs(subs);
                if roster_changed {
                    let _ = app.admin_changes.send(forward::AdminChange::engine(
                        &["/subs", "/capacity", "/overview"],
                        "anthropic_roster",
                    ));
                }
                if membership_changed {
                    prev = cur;
                    poke.notify_one(); // состав изменился → поллер probe-нёт новых
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// Верхняя граница cooling для probe: все известные окна не длиннее 7d; сутки — запас.
const MAX_PROBE_COOL_SECS: i64 = 8 * 24 * 3600;
/// Без authoritative claim используем тот же консервативный порог, что fallback в `forward`.
const PROBE_QUOTA_UTIL: f64 = 0.95;

/// Локальный claim-less fallback до переноса единой классификации 429 в `forward`.
/// Принимаем только будущие reset и ограничиваем даже враждебный timestamp восемью сутками.
fn probe_cool_secs_429(
    util5h: Option<f64>,
    util7d: Option<f64>,
    reset5h: Option<i64>,
    reset7d: Option<i64>,
    now: i64,
    fallback_secs: i64,
) -> i64 {
    let future = |reset: Option<i64>| {
        reset
            .filter(|t| *t > now)
            .map(|t| t.saturating_sub(now).max(1))
    };
    let (u5, u7) = (util5h.unwrap_or(0.0), util7d.unwrap_or(0.0));
    let reset = if u7 >= PROBE_QUOTA_UTIL {
        future(reset7d).or_else(|| future(reset5h))
    } else if u5 >= PROBE_QUOTA_UTIL {
        future(reset5h).or_else(|| future(reset7d))
    } else {
        None
    };
    reset.unwrap_or(fallback_secs).clamp(1, MAX_PROBE_COOL_SECS)
}

/// Один liveness-probe: читаем лимиты подписки и применяем. Сетевой сбой всё равно фиксируем
/// (`set_util` c None двигает `polled_ts`) — иначе поллер спинил бы по мёртвому прокси. Результат
/// probe (401/403/2xx) кормит durable-машину auth-health; изменившийся вердикт персистится (fenced).
async fn probe(
    app: &AppState,
    authority: &registry::authority::AuthorityConfig,
    owner: Option<&registry::pg::Owner>,
    sub: &Sub,
) {
    let client = match app.clients.get(&sub.proxy, &sub.email) {
        Ok(c) => c,
        Err(_) => {
            app.pool.set_util(&sub.email, None, None, None, None, None);
            return;
        }
    };
    let ua = persona_ua(&app.cfg, &sub.email);
    let mut calibration_plan = sub.plan.trim().to_owned();
    if calibration_plan.is_empty() {
        let detected = match detect_plan(&client, &app.cfg, &sub.token, &ua).await {
            PlanDetect::Plan(plan) => Some(plan),
            PlanDetect::NoScope => match app.pool.consensus_paid_plan() {
                Some(plan) => {
                    elog::info(
                        "server",
                        "Anthropic plan inferred from unanimous paid cohort for inference-only token",
                    );
                    Some(plan)
                }
                None => {
                    elog::warn(
                        "server",
                        format!(
                            "Anthropic plan remains unknown for {}: OAuth token has no profile scope and fleet cohort is not unanimous",
                            sub.email
                        ),
                    );
                    None
                }
            },
            PlanDetect::Err(error) => {
                elog::error("server", format!("Anthropic plan detection failed for {}: {error}", sub.email));
                None
            }
        };
        if let Some(plan) = detected.filter(|plan| !plan.is_empty()) {
            let config = authority.clone();
            let email = sub.email.clone();
            let stored_plan = plan.clone();
            let persisted = tokio::task::spawn_blocking(move || {
                let mut registry = config.connect()?;
                let updated = registry.set_plan(&email, &stored_plan)?;
                anyhow::ensure!(
                    updated == 1,
                    "subscription disappeared during plan detection"
                );
                Ok::<_, anyhow::Error>(())
            })
            .await;
            match persisted {
                Ok(Ok(())) => {
                    app.pool.set_plan(&sub.email, &plan);
                    calibration_plan = plan;
                }
                Ok(Err(error)) => elog::error(
                    "server",
                    format!("Anthropic plan persistence failed for {}: {error:#}", sub.email),
                ),
                Err(error) => elog::error(
                    "server",
                    format!("Anthropic plan persistence task failed for {}: {error}", sub.email),
                ),
            }
        }
    }
    match poll_sub(&client, &app.cfg, &sub.token, &ua, &sub.email).await {
        Some(r) => {
            let before = anthropic_admin_fingerprint(&app.pool, &sub.email);
            let observed_at = pool::now();
            app.pool.set_util(
                &sub.email,
                r.util5h,
                r.util7d,
                r.status.clone(),
                r.reset5h,
                r.reset7d,
            );
            app.pool.set_quota_snapshots(
                &sub.email,
                r.quota5h.map(|quota| pool::QuotaSnapshot {
                    used_fraction_units: quota.used_fraction_units,
                    measurement_resolution_fraction_units: quota
                        .measurement_resolution_fraction_units,
                    observed_at,
                    resets_at: r.reset5h,
                }),
                r.quota7d.map(|quota| pool::QuotaSnapshot {
                    used_fraction_units: quota.used_fraction_units,
                    measurement_resolution_fraction_units: quota
                        .measurement_resolution_fraction_units,
                    observed_at,
                    resets_at: r.reset7d,
                }),
            );
            if let Some(billing) = &app.billing {
                let plan = if calibration_plan.is_empty() {
                    "unknown"
                } else {
                    &calibration_plan
                };
                for (kind, duration, quota, resets_at) in [
                    ("5h", 300, r.quota5h, r.reset5h),
                    ("7d", 10_080, r.quota7d, r.reset7d),
                ] {
                    let (Some(quota), Some(resets_at)) = (quota, resets_at) else {
                        continue;
                    };
                    if let Err(error) = billing
                        .observe_anthropic_window(
                            &sub.email,
                            plan,
                            kind,
                            duration,
                            resets_at,
                            quota.used_fraction_units,
                            quota.measurement_resolution_fraction_units,
                            observed_at,
                        )
                        .await
                    {
                        elog::error(
                            "server",
                            format!("Anthropic poll calibration failed for {} {kind}: {error:#}", sub.email),
                        );
                    }
                }
            }
            match r.http {
                429 => {
                    // AUDIT-TODO(C59): expose claim/Retry-After and one public 429-cooling helper from
                    // `forward`, then delete this claim-less fallback from the composition crate.
                    let secs = probe_cool_secs_429(
                        r.util5h,
                        r.util7d,
                        r.reset5h,
                        r.reset7d,
                        pool::now(),
                        app.cfg.cool_secs,
                    );
                    app.pool.cool(&sub.email, secs);
                }
                401 | 403 => {
                    // Убираем из ротации на время наблюдения (suspect не должен быть placement-магнитом),
                    // но next_probe_at для suspect/dead cooling-независим → корроборация продолжается.
                    app.pool.cool(&sub.email, 900);
                }
                _ => {}
            }
            // Durable auth-health: один 401/403 НЕ приговор; `record_probe` копит серию и ставит Dead
            // только по корроборации (DEAD_STREAK за ≥DEAD_MIN_SECS). Вердикт-изменение персистим
            // (leader/owner-fenced) → переживает рестарт/blue-green, панель показывает авторитетно.
            if let Some(health) =
                app.pool
                    .record_probe(&sub.email, r.http, &pool::token_fp(&sub.token))
            {
                persist_health(authority, owner, health).await;
            }
            let after = anthropic_admin_fingerprint(&app.pool, &sub.email);
            if after != before {
                let _ = app.admin_changes.send(forward::AdminChange::engine(
                    &["/subs", "/capacity", "/overview"],
                    "anthropic_probe",
                ));
            }
        }
        None => app.pool.set_util(&sub.email, None, None, None, None, None), // записать попытку (backoff)
    }
}

fn anthropic_admin_fingerprint(pool: &pool::Pool, email: &str) -> String {
    let live = pool
        .snapshot()
        .into_iter()
        .find(|(sub, _)| sub.email == email)
        .map(|(_, live)| live)
        .unwrap_or_default();
    format!(
        "{:.6}|{:.6}|{}|{}|{:?}|{:?}|{:?}|{:?}",
        live.util5h,
        live.util7d,
        live.status,
        live.cooling_until,
        live.auth_state,
        live.quota5h,
        live.quota7d,
        live.dead_reason,
    )
}

/// Персист durable-вердикта auth-health одной подписки (owner-fenced в PostgreSQL). Синхронный
/// клиент → blocking-пул, как persist_state_cas/ledger_prune.
async fn persist_health(
    authority: &registry::authority::AuthorityConfig,
    owner: Option<&registry::pg::Owner>,
    health: registry::SubHealth,
) {
    let config = authority.clone();
    let owner = owner.cloned();
    let email = health.email.clone();
    let state = health.auth_state.clone();
    let res = tokio::task::spawn_blocking(move || {
        let mut conn = config.connect()?;
        conn.save_sub_health(owner.as_ref(), &health)?;
        Ok::<_, anyhow::Error>(())
    })
    .await;
    match res {
        Ok(Ok(())) => {
            if state == "dead" {
                elog::error("server", format!("подписка {email} помечена DEAD (токен отвергнут Anthropic)"));
            }
        }
        Ok(Err(e)) => elog::error("server", format!("персист auth-health {email} не удался: {e}")),
        Err(e) => elog::error("server", format!("персист auth-health {email}: задача упала: {e}")),
    }
}

/// Safety-flush персиста (сек): калибровка, изменившаяся БЕЗ cooling, всё же осядет.
const PERSIST_SAFETY: u64 = 120;
/// Коалесцируем всплеск cooling-событий перед записью (сек).
const PERSIST_DEBOUNCE: u64 = 1;

/// Persist with bounded CAS rebasing so overlapping blue/green engines converge instead of one
/// writer either overwriting the other or retrying forever from a stale version.
pub async fn persist_state_cas(
    app: &AppState,
    authority: &registry::authority::AuthorityConfig,
    owner: Option<&registry::pg::Owner>,
    attempts: usize,
) -> anyhow::Result<()> {
    let attempts = attempts.max(1);
    let mut last_error = String::from("pool-state persistence did not run");
    for attempt in 0..attempts {
        let rows = app.pool.export_state();
        if rows.is_empty() {
            return Ok(());
        }
        let config = authority.clone();
        let write_owner = owner.cloned();
        let saved = tokio::task::spawn_blocking(move || {
            let mut conn = config.connect()?;
            let versions = conn.save_pool_state(write_owner.as_ref(), &rows)?;
            let acknowledged: Vec<(String, i64, f64)> = versions
                .into_iter()
                .map(|(email, version)| {
                    let delta = rows
                        .iter()
                        .find(|row| row.email == email)
                        .map(|row| row.spent_delta_usd)
                        .unwrap_or(0.0);
                    (email, version, delta)
                })
                .collect();
            let _ = conn.wal_checkpoint();
            Ok::<_, anyhow::Error>(acknowledged)
        })
        .await;
        match saved {
            Ok(Ok(acknowledged)) => {
                app.pool.accept_persist_versions(&acknowledged);
                return Ok(());
            }
            Ok(Err(err)) => last_error = err.to_string(),
            Err(err) => last_error = format!("pool-state writer task failed: {err}"),
        }

        if attempt + 1 == attempts {
            break;
        }
        let config = authority.clone();
        match tokio::task::spawn_blocking(move || {
            config.connect().and_then(|mut conn| conn.load_pool_state())
        })
        .await
        {
            Ok(Ok(rows)) => app.pool.merge_persisted_state(rows),
            Ok(Err(err)) => last_error = format!("{last_error}; CAS rebase failed: {err}"),
            Err(err) => last_error = format!("{last_error}; CAS rebase task failed: {err}"),
        }
        tokio::time::sleep(Duration::from_millis(50 * (attempt as u64 + 1))).await;
    }
    Err(anyhow::anyhow!(last_error))
}

/// Персист состояния пула: **write-through по событию** cooling (`poke` из `pool.on_change`) —
/// бан переживает рестарт почти сразу; плюс редкий safety-flush для калибровки. Не фиксированный
/// снапшот «раз в N»: под тишиной (нет cooling) пишем лишь раз в `PERSIST_SAFETY`.
pub async fn persist_loop(
    app: AppState,
    authority: registry::authority::AuthorityConfig,
    owner: Option<registry::pg::Owner>,
    poke: Arc<Notify>,
) {
    loop {
        tokio::select! {
            _ = poke.notified() => { tokio::time::sleep(Duration::from_secs(PERSIST_DEBOUNCE)).await; }
            _ = tokio::time::sleep(Duration::from_secs(PERSIST_SAFETY)) => {}
        }
        // Durability is real only after a successful fenced write. PostgreSQL overlap may produce
        // a legitimate CAS conflict, so reload/merge/retry immediately instead of losing a cooling.
        if let Err(e) = persist_state_cas(&app, &authority, owner.as_ref(), 5).await {
            elog::error("server", format!("персист состояния пула не удался после CAS retry: {e}"));
        }
    }
}

/// Событийный backend-поллер: probe-ит созревшие подписки конкурентно, затем спит РОВНО до
/// ближайшего due-времени (или до `poke` при изменении флота/post-turn evidence). Фиксированного
/// тика нет.
pub async fn poll_loop(
    app: AppState,
    authority: registry::authority::AuthorityConfig,
    owner: Option<registry::pg::Owner>,
    poke: Arc<Notify>,
) {
    loop {
        // PostgreSQL lease-epoch elects exactly one active poller. No Redlock and no best-effort race.
        if let Some(owner) = owner.as_ref() {
            let config = authority.clone();
            let owner = owner.clone();
            let leader = tokio::task::spawn_blocking(move || {
                let mut db = config.connect()?;
                db.postgres()?
                    .acquire_leader(&owner, "subscription-poller", 30)
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("poller leader task failed: {e}")));
            match leader {
                Ok(true) => {}
                Ok(false) => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                        _ = poke.notified() => {}
                    }
                    continue;
                }
                Err(err) => {
                    elog::error("server", format!("poller leader lease unavailable: {err}"));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
        }
        let now = pool::now();
        let snap = app.pool.snapshot();
        let due: Vec<Sub> = snap
            .iter()
            .filter(|(s, l)| next_probe_at(&s.email, l) <= now)
            .map(|(s, _)| s.clone())
            .collect();

        if !due.is_empty() {
            let mut set: JoinSet<()> = JoinSet::new();
            let mut due = due.into_iter();
            for sub in due.by_ref().take(PROBE_CONCURRENCY) {
                let app = app.clone();
                let authority = authority.clone();
                let owner = owner.clone();
                set.spawn(async move {
                    probe(&app, &authority, owner.as_ref(), &sub).await;
                });
            }
            let mut renew = tokio::time::interval(Duration::from_secs(10));
            renew.tick().await;
            while !set.is_empty() {
                tokio::select! {
                    joined = set.join_next() => {
                        if joined.is_some() {
                            if let Some(sub) = due.next() {
                                let app = app.clone();
                                let authority = authority.clone();
                                let owner = owner.clone();
                                set.spawn(async move { probe(&app, &authority, owner.as_ref(), &sub).await; });
                            }
                        }
                    }
                    _ = renew.tick(), if owner.is_some() => {
                        let config = authority.clone();
                        let current = owner.clone().expect("guarded by owner.is_some()");
                        let renewed = tokio::task::spawn_blocking(move || {
                            let mut db = config.connect()?;
                            db.postgres()?.acquire_leader(&current, "subscription-poller", 30)
                        }).await;
                        if !matches!(renewed, Ok(Ok(true))) {
                            elog::error("server", "poller leader renewal lost; aborting outstanding probes");
                            set.abort_all();
                            while set.join_next().await.is_some() {}
                            break;
                        }
                    }
                }
            }
        }

        // спим до ближайшего due (событийно), но не дольше MAX_SLEEP; будимся раньше по `poke`.
        let next = app
            .pool
            .snapshot()
            .iter()
            .map(|(s, l)| next_probe_at(&s.email, l))
            .min()
            .unwrap_or(now + MAX_SLEEP);
        let max_sleep = if owner.is_some() { 10 } else { MAX_SLEEP };
        let sleep_s = (next - pool::now()).clamp(1, max_sleep);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(sleep_s as u64)) => {}
            _ = poke.notified() => {}
        }
    }
}

/// Renew the monotonic instance epoch. A fenced/stale process flips readiness before it can accept
/// further work; PostgreSQL transactions independently reject its money/capacity mutations.
pub async fn owner_heartbeat_loop(
    authority: registry::authority::AuthorityConfig,
    owner: registry::pg::Owner,
    authority_ready: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        let config = authority.clone();
        let current = owner.clone();
        let renewed = tokio::task::spawn_blocking(move || {
            let mut db = config.connect()?;
            db.heartbeat_instance(&current, 30)
        })
        .await;
        match renewed {
            Ok(Ok(true)) => authority_ready.store(true, Ordering::Release),
            Ok(Ok(false)) => {
                elog::error("server", "engine owner epoch fenced; readiness removed");
                authority_ready.store(false, Ordering::Release);
                return;
            }
            Ok(Err(err)) => {
                authority_ready.store(false, Ordering::Release);
                elog::error(
                    "server",
                    format!("engine owner heartbeat failed; readiness removed until recovery: {err}"),
                );
            }
            Err(err) => {
                authority_ready.store(false, Ordering::Release);
                elog::error("server", format!("engine owner heartbeat task failed; readiness removed: {err}"));
            }
        }
    }
}

#[cfg(test)]
mod tests;
