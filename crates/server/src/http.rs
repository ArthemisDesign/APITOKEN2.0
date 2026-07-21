//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health,/live — жив ли сервер (без авторизации)
//!   GET /ready        — принимает ли сервер новый трафик (без авторизации)
//!   GET /pool         — статус пула (util/cooling, без секретов)
//!   *             — форвардинг на api.anthropic.com (см. forward::forward)

use axum::extract::{ConnectInfo, FromRef, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use forward::{authed, client_key, control_authed, forward, readonly_authed, AppState, Metrics};
use crate::admin;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Unified self-contained administration UI. Data is routed by the admin Caddy vhost.
const ADMIN_PANEL_HTML: &str = include_str!("admin-panel.html");

/// TTL-кэш дашборд-эндпоинтов: панель поллит /overview и /capacity, а они делают O(n)-скан
/// (capacity() под пул-локом + billing_totals full-scan). Кэш на 2с → при N поллерах пересчёт
/// максимум раз в 2с, не крадём hot-path лок у боевых запросов. Свежесть суб-секунд дашборду не нужна.
const DASH_TTL: std::time::Duration = std::time::Duration::from_secs(2);
type DashCache = std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>;
static OVERVIEW_CACHE: DashCache = std::sync::OnceLock::new();
static CAPACITY_CACHE: DashCache = std::sync::OnceLock::new();
static SUBS_CACHE: DashCache = std::sync::OnceLock::new();
/// Планируемый срок жизни подписки — ровно N дней от добавления токена (`added_ts`). Это НЕ срок
/// самого OAuth-токена (opaque, недоступен), а наш горизонт планирования замены.
const SUB_LIFETIME_DAYS: i64 = 30;
fn cache_get(cell: &DashCache) -> Option<serde_json::Value> {
    cell.get_or_init(|| std::sync::Mutex::new(None)).lock().unwrap().as_ref()
        .filter(|(t, _)| t.elapsed() < DASH_TTL).map(|(_, v)| v.clone())
}
fn cache_put(cell: &DashCache, v: &serde_json::Value) {
    *cell.get_or_init(|| std::sync::Mutex::new(None)).lock().unwrap() =
        Some((std::time::Instant::now(), v.clone()));
}

#[derive(Clone)]
struct HttpState {
    app: AppState,
    accepting: Arc<AtomicBool>,
}

impl FromRef<HttpState> for AppState {
    fn from_ref(state: &HttpState) -> Self {
        state.app.clone()
    }
}

pub fn router(app: AppState, accepting: Arc<AtomicBool>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/live", get(health))
        .route("/ready", get(ready))
        .route("/pool", get(pool_status))
        .route("/balance", get(balance))
        .route("/capacity", get(capacity))
        .route("/overview", get(overview))
        .route("/subs", get(subs))
        .route("/metrics", get(metrics))
        .route("/admin-panel", get(admin_panel))
        // Control-плоскость (`/admin/*`) — управление аккаунтами/ключами/балансом за control-ключом.
        // Контракт для будущей КОММЕРЦИИ (отдельный сервис). Движок — авторитет живого баланса.
        .route("/admin/account", post(admin::create_account))
        .route("/admin/account/{id}", get(admin::get_account))
        .route("/admin/account/{id}/credit", post(admin::credit_account))
        .route("/admin/account/{id}/status", post(admin::account_status))
        .route("/admin/account/{id}/pricing", post(admin::account_pricing))
        .route("/admin/account/{id}/keys", get(admin::list_keys))
        .route("/admin/account/{id}/ledger", get(admin::list_ledger))
        .route("/admin/account/{id}/usage", get(admin::list_usage))
        .route("/admin/key", post(admin::issue_key))
        .route("/admin/key/{key}/status", post(admin::key_status))
        .route("/admin/key-id/{key_id}/status", post(admin::key_status_by_id))
        .route("/admin/key-id/{key_id}/label", post(admin::key_label_by_id))
        .route("/admin/account/{account_id}/key-id/{key_id}/policy", post(admin::key_policy_by_id))
        .fallback(forward)
        .with_state(HttpState { app, accepting })
}

/// Prometheus-метрики (admin-авторизация). Ключевое: `route_pin/place` = доля cache-hit,
/// `inflight` (растёт при простое → утечка слота), 429/auth/5xx, состояние circuit breaker.
async fn metrics(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !readonly_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let m = &app.metrics;
    let rs = app.pool.route_stats();
    let (inflight, cooling) = app.pool.gauges();
    let breaker_open = app.breaker.open_for(pool::now()).is_some() as u8;
    let g = |c| Metrics::get(c);
    let body = format!(
        "# TYPE claude_api_requests_total counter\nclaude_api_requests_total {}\n\
         # TYPE claude_api_upstream_429_total counter\nclaude_api_upstream_429_total {}\n\
         # TYPE claude_api_upstream_auth_total counter\nclaude_api_upstream_auth_total {}\n\
         # TYPE claude_api_upstream_5xx_total counter\nclaude_api_upstream_5xx_total {}\n\
         # TYPE claude_api_breaker_rejects_total counter\nclaude_api_breaker_rejects_total {}\n\
         # TYPE claude_api_exhausted_total counter\nclaude_api_exhausted_total {}\n\
         # TYPE claude_api_key_throttled_total counter\nclaude_api_key_throttled_total {}\n\
         # TYPE claude_api_auth_failures_total counter\nclaude_api_auth_failures_total {}\n\
         # TYPE claude_api_route_pin_total counter\nclaude_api_route_pin_total {}\n\
         # TYPE claude_api_route_spill_total counter\nclaude_api_route_spill_total {}\n\
         # TYPE claude_api_route_place_total counter\nclaude_api_route_place_total {}\n\
         # TYPE claude_api_inflight gauge\nclaude_api_inflight {}\n\
         # TYPE claude_api_subs gauge\nclaude_api_subs {}\n\
         # TYPE claude_api_cooling gauge\nclaude_api_cooling {}\n\
         # TYPE claude_api_breaker_open gauge\nclaude_api_breaker_open {}\n",
        g(&m.requests), g(&m.upstream_429), g(&m.upstream_auth), g(&m.upstream_5xx),
        g(&m.breaker_rejects), g(&m.exhausted), g(&m.key_throttled), g(&m.auth_failures),
        rs.pin, rs.spill, rs.place, inflight, app.pool.len(), cooling, breaker_open,
    );
    // Наблюдаемость трат: агрегаты по клиентским ключам (USD) — только когда биллинг включён И вызов
    // авторизован CONTROL-ключом. Выручка/остатки клиентов — коммерческая тайна: панельному (read-only)
    // ключу их НЕ отдаём (он видит лишь операционные метрики inflight/breaker/429).
    let body = match &app.billing {
        Some(b) if control_authed(&app, &headers, &peer) => {
            let t = b.totals().await;
            let usd = |n: i64| n as f64 / 1e9;
            format!(
                "{body}# TYPE claude_api_client_balance_usd gauge\nclaude_api_client_balance_usd {:.6}\n\
                 # TYPE claude_api_billed_usd_total counter\nclaude_api_billed_usd_total {:.6}\n\
                 # TYPE claude_api_reserved_usd gauge\nclaude_api_reserved_usd {:.6}\n\
                 # TYPE claude_api_metered_keys gauge\nclaude_api_metered_keys {}\n",
                usd(t.balance_nano), usd(t.spent_nano), usd(t.reserved_nano), t.active_keys,
            )
        }
        _ => body, // биллинг выключен ИЛИ вызов не control → только операционные метрики
    };
    ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response()
}

/// Unified admin.apitoken.sale UI. Human and data authorization is enforced by Caddy.
async fn admin_panel() -> Response {
    ([(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")], ADMIN_PANEL_HTML).into_response()
}

/// Доступная ёмкость пула в USD real-API-эквиваленте на горизонты 1ч/5ч/1д/7д.
/// По каждой подписке + суммарно по флоту. Считается на лету (без обращений к Anthropic).
async fn capacity(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !readonly_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    if let Some(v) = cache_get(&CAPACITY_CACHE) { return Json(v).into_response(); }
    let v = capacity_value(&app);
    cache_put(&CAPACITY_CACHE, &v);
    Json(v).into_response()
}

/// Вычисление ёмкости пула (per-sub + агрегат) — для `/capacity` и TTL-кэша. Синхронно.
fn capacity_value(app: &AppState) -> serde_json::Value {
    let caps = app.pool.capacity();
    let round = |x: f64| (x * 100.0).round() / 100.0; // 2 знака
    // Маскируем email подписки: панельному (read-only) ключу не отдаём реальные адреса пула — это
    // операционно чувствительно (реселлимые Claude-аккаунты; полный email помогает корреляции/бану).
    let mask_email = |e: &str| -> String {
        let head: String = e.chars().take(4).collect();
        format!("{head}…")
    };
    let (mut a1, mut a5, mut a1d, mut a7d) = (0.0, 0.0, 0.0, 0.0);
    let mut all_calibrated = true;
    let subs: Vec<_> = caps.iter().map(|c| {
        a1 += c.avail_1h_usd; a5 += c.avail_5h_usd; a1d += c.avail_1d_usd; a7d += c.avail_7d_usd;
        if !c.calibrated { all_calibrated = false; }
        json!({
            "email": mask_email(&c.email),
            "calibrated": c.calibrated,
            "util5h": round(c.util5h), "util7d": round(c.util7d),
            "reset5h_in": c.reset5h_in, "reset7d_in": c.reset7d_in,
            "cap5h_usd": round(c.cap5h_usd), "cap7d_usd": round(c.cap7d_usd),
            "rem5h_usd": round(c.rem5h_usd), "rem7d_usd": round(c.rem7d_usd),
            "avail_1h_usd": round(c.avail_1h_usd), "avail_5h_usd": round(c.avail_5h_usd),
            "avail_1d_usd": round(c.avail_1d_usd), "avail_7d_usd": round(c.avail_7d_usd),
            "status": c.status, "cooling": c.cooling,
        })
    }).collect();
    json!({
        "now": pool::now(),
        "subs": subs.len(),
        "calibrated": all_calibrated,   // false → хотя бы одна подписка ещё на прайоре
        "available_usd": {              // суммарно по флоту, USD real-API-эквивалента
            "next_1h": round(a1), "next_5h": round(a5), "next_1d": round(a1d), "next_7d": round(a7d),
        },
        "per_sub": subs,
    })
}

/// Control-room: агрегат СПРОС (балансы/резерв/траты) + ПРЕДЛОЖЕНИЕ (ёмкость/потребление/здоровье)
/// + headroom + рекомендация по числу подписок. Одно окно «мы под контролем?». Гейт — control-ключ
/// (деньги = коммерческая тайна). Работает от 1 до 1000 подписок — всё в агрегатах и отношениях.
async fn overview(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !control_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    if let Some(v) = cache_get(&OVERVIEW_CACHE) { return Json(v).into_response(); }
    let mut v = overview_value(&app).await;
    // Разбивка спроса по engine-аккаунтам — только в HTTP-ответе панели; в историю
    // (poller::metrics_loop пишет overview_value) её не кладём, чтобы не раздувать metrics.db.
    if let Some(b) = &app.billing {
        let accounts: Vec<_> = b.accounts().await.into_iter().map(|a| json!({
            "account": a.id,
            "handle": a.handle,
            "balance_usd": (a.balance_nano as f64 / 1e9 * 100.0).round() / 100.0,
            "spent_usd": (a.spent_nano as f64 / 1e9 * 100.0).round() / 100.0,
            "mult": a.mult_bp as f64 / 10000.0,
            "status": a.status,
        })).collect();
        v["accounts"] = json!(accounts);
    }
    cache_put(&OVERVIEW_CACHE, &v);
    Json(v).into_response()
}

/// Вычисление control-room агрегата (без авторизации) — переиспользуется хендлером `/overview`
/// И фоновым коллектором истории (`poller::metrics_loop`). Считается на лету из пула + биллинга.
pub(crate) async fn overview_value(app: &AppState) -> serde_json::Value {
    const TARGET_HEADROOM: f64 = 1.3; // держим 30% запас
    const REF_MULT: f64 = 0.20;       // референсная наценка для «продаём клиентам» и coverage
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    let caps = app.pool.capacity();
    let n = caps.len();
    let (mut a1, mut a5, mut a1d, mut a7d) = (0.0, 0.0, 0.0, 0.0);
    let (mut cap5, mut cap7, mut rem5, mut rem7) = (0.0, 0.0, 0.0, 0.0);
    let (mut u5, mut u7) = (0.0, 0.0);
    let (mut cooling, mut all_cal) = (0usize, true);
    for c in &caps {
        a1 += c.avail_1h_usd; a5 += c.avail_5h_usd; a1d += c.avail_1d_usd; a7d += c.avail_7d_usd;
        cap5 += c.cap5h_usd; cap7 += c.cap7d_usd; rem5 += c.rem5h_usd; rem7 += c.rem7d_usd;
        u5 += c.util5h; u7 += c.util7d;
        if c.cooling { cooling += 1; }
        if !c.calibrated { all_cal = false; }
    }
    let cons5 = (cap5 - rem5).max(0.0); // real-API-$ потрачено в текущем 5h окне (по всему флоту)
    let cons7 = (cap7 - rem7).max(0.0);
    let head5 = if cons5 > 0.01 { a5 / cons5 } else { f64::INFINITY };  // «во сколько раз ещё выдержим»
    let head7 = if cons7 > 0.01 { a7d / cons7 } else { f64::INFINITY };
    // Рекомендация: сколько подписок держать под target_headroom относительно текущего потребления.
    let per_sub_5h = if n > 0 { cap5 / n as f64 } else { 0.0 };
    let per_sub_7d = if n > 0 { cap7 / n as f64 } else { 0.0 };
    let need5 = if per_sub_5h > 0.0 { (cons5 * TARGET_HEADROOM / per_sub_5h).ceil() as i64 } else { 0 };
    let need7 = if per_sub_7d > 0.0 { (cons7 * TARGET_HEADROOM / per_sub_7d).ceil() as i64 } else { 0 };
    let need = need5.max(need7).max(n.min(1) as i64);
    let gap = need - n as i64;
    // Спрос (деньги клиентов) — только при включённом биллинге.
    let (bal, res, spent, keys) = match &app.billing {
        Some(b) => { let t = b.totals().await;
            (t.balance_nano as f64/1e9, t.reserved_nano as f64/1e9, t.spent_nano as f64/1e9, t.active_keys) }
        None => (0.0, 0.0, 0.0, 0),
    };
    let real_demand = if REF_MULT > 0.0 { bal / REF_MULT } else { 0.0 }; // потенц. real-API из балансов
    let coverage7 = if a7d > 0.01 { real_demand / a7d } else { 0.0 };    // >1 = потенциально перепродали
    let jinf = |x: f64| if x.is_finite() { json!(r2(x)) } else { json!(null) }; // null = ∞ (нет спроса)
    json!({
        "now": pool::now(),
        "subs": n, "calibrated": all_cal, "ref_mult": REF_MULT, "target_headroom": TARGET_HEADROOM,
        "supply": {
            "avail_usd":    {"1h": r2(a1), "5h": r2(a5), "1d": r2(a1d), "7d": r2(a7d)},
            "cap_usd":      {"5h": r2(cap5), "7d": r2(cap7)},
            "consumed_usd": {"5h": r2(cons5), "7d": r2(cons7)},
            "util":         {"5h": r2(if n>0 {u5/n as f64} else {0.0}), "7d": r2(if n>0 {u7/n as f64} else {0.0})},
            "health":       {"healthy": n.saturating_sub(cooling), "cooling": cooling, "total": n},
        },
        "demand": {"balance_usd": r2(bal), "reserved_usd": r2(res), "spent_usd": r2(spent), "active_keys": keys,
                   "potential_realapi_usd": r2(real_demand)},
        "headroom": {"5h": jinf(head5), "7d": jinf(head7)},
        "coverage": {"7d": r2(coverage7)},
        "recommend": {"subs_needed": need, "gap": gap},
    })
}

/// Lifecycle-обзор подписок (control-authed): срок жизни (added_ts + N дней = план замены) + прокси
/// (маска host:port + срок из IPRoyal). Живое здоровье (cooling/util) — в /capacity; тут — планирование.
/// Чистое чтение реестра (read-only, TTL-кэш) — money-путь не трогаем.
async fn subs(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !control_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    if let Some(v) = cache_get(&SUBS_CACHE) { return Json(v).into_response(); }
    let authority = app.authority.as_ref().clone();
    let db = app.data_db_path.as_ref().clone();
    let (rows, maxes) = tokio::task::spawn_blocking(move || {
        let rows = authority.connect().and_then(|mut c| c.subs_admin()).unwrap_or_default();
        // пиковая ёмкость по подписке за всю дистанцию (durable sub_peaks в metrics.db)
        let mdir = std::path::Path::new(&db).parent()
            .map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        let maxes = crate::metrics_store::open(&format!("{mdir}/metrics.db"))
            .and_then(|c| crate::metrics_store::sub_maxes(&c)).unwrap_or_default();
        (rows, maxes)
    }).await.unwrap_or_default();
    let peak: std::collections::HashMap<String, (f64, f64)> =
        maxes.into_iter().map(|(e, m5, m7, _)| (e, (m5, m7))).collect();
    let now = pool::now();
    let lifetime = SUB_LIFETIME_DAYS * 86400;
    let days = |secs: i64| ((secs as f64 / 86400.0) * 10.0).round() / 10.0;
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    let list: Vec<_> = rows.iter().map(|s| {
        let head: String = s.email.chars().take(4).collect();
        let sub_expire = if s.added_ts > 0 { s.added_ts + lifetime } else { 0 };
        let (pk5, pk7) = peak.get(&s.email).copied().unwrap_or((0.0, 0.0));
        json!({
            "email": format!("{head}…"),
            "status": s.status, "fleet": s.fleet, "added": s.added, "has_token": s.has_token,
            "sub_expire_ts": sub_expire,
            "sub_days_left": if sub_expire > 0 { days(sub_expire - now) } else { 0.0 },
            "proxy_host": s.proxy_host,
            "proxy_expire": s.proxy_expire,   // из IPRoyal (пусто, пока authbot-loop не заполнил)
            "proxy_ok": s.proxy_ok,
            "peak_cap5h_usd": r2(pk5),         // истинный ПИК ёмкости на дистанции (не EMA)
            "peak_cap7d_usd": r2(pk7),
        })
    }).collect();
    let v = json!({"now": now, "lifetime_days": SUB_LIFETIME_DAYS, "subs": list});
    cache_put(&SUBS_CACHE, &v);
    Json(v).into_response()
}

async fn health() -> Json<serde_json::Value> {
    // Минимум без авторизации: голый liveness-пинг. Размер пула / upstream / статус биллинга —
    // раскрытие backend (у api.anthropic.com нет /health, N подписок = фингерпринт) → только на
    // авторизованных /pool и /metrics.
    Json(json!({ "ok": true }))
}

fn readiness_snapshot(accepting: &AtomicBool, authority_ready: &AtomicBool) -> (StatusCode, serde_json::Value) {
    if accepting.load(Ordering::Acquire) && authority_ready.load(Ordering::Acquire) {
        (StatusCode::OK, json!({ "ready": true }))
    } else {
        let reason = if !accepting.load(Ordering::Acquire) { "draining" } else { "authority_unavailable" };
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({ "ready": false, "reason": reason }),
        )
    }
}

async fn ready(State(state): State<HttpState>) -> Response {
    let (status, body) = readiness_snapshot(&state.accepting, &state.app.authority_ready);
    (status, Json(body)).into_response()
}

/// Баланс по своему ключу: клиент шлёт свой x-api-key/Bearer → видит остаток в USD.
async fn balance(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let billing = match &app.billing {
        Some(b) => b,
        None => return (StatusCode::NOT_FOUND, Json(json!({"error": "billing disabled"}))).into_response(),
    };
    let key = match client_key(&headers) {
        Some(k) => k,
        None => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "no api key"}))).into_response(),
    };
    // ключ → аккаунт → баланс аккаунта (общий на все ключи юзера) + расход именно этого ключа
    let krow = billing.get(&key).await;
    let acct = match billing.key_auth(&key).await {
        Some(a) if a.active_at(pool::now()) => billing.account(&a.account_id).await,
        None => None,
        Some(_) => None,
    };
    match (krow, acct) {
        (Some(k), Some(a)) => Json(json!({
            "account": a.id,
            "balance": metering::nano_to_usd_string(a.balance_nano as i128),
            "spent": metering::nano_to_usd_string(a.spent_nano as i128),
            "balance_nano": a.balance_nano,
            "spent_nano": a.spent_nano,
            "reserved_nano": a.reserved_nano,
            "multiplier": a.mult_bp as f64 / 10000.0,
            "status": a.status,
            "key_label": k.label,
            "key_spent_nano": k.spent_nano,
        })).into_response(),
        _ => (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response(),
    }
}

async fn pool_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    let now = pool::now();
    let list: Vec<_> = app.pool.snapshot().into_iter().map(|(s, l)| json!({
        "email": s.email,
        "fleet": s.fleet,
        "proxy": !s.proxy.is_empty(),
        "util5h": l.util5h,
        "util7d": l.util7d,
        "status": l.status,
        "cooling": l.cooling_until > now,
        "cooling_left": (l.cooling_until - now).max(0),
        "last_used": l.last_used,
        "polled_ts": l.polled_ts,
    })).collect();
    Json(json!({"pool": list, "cap": app.cfg.util_cap, "poller": app.cfg.poll})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_flag_flips_before_drain() {
        let accepting = AtomicBool::new(true);
        let authority_ready = AtomicBool::new(true);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready),
            (StatusCode::OK, json!({"ready": true}))
        );

        accepting.store(false, Ordering::Release);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"ready": false, "reason": "draining"}),
            )
        );
        accepting.store(true, Ordering::Release);
        authority_ready.store(false, Ordering::Release);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"ready": false, "reason": "authority_unavailable"}),
            )
        );
    }

    #[test]
    fn embedded_admin_panel_exposes_all_operational_workflows_without_secrets() {
        assert!(ADMIN_PANEL_HTML.contains("data-admin-panel-version=\"4\""));
        for route in [
            "/admin/dashboard",
            "/admin/users",
            "/admin/topups",
            "/admin/business-invites",
            "/admin/audit",
            "/admin/admin-accounts",
            "/balance-adjustments",
            "/sessions/revoke",
            "/totp/reset",
            "/partner-admin/partner-analytics",
            "/overview",
            "/capacity",
            "/subs",
        ] {
            assert!(ADMIN_PANEL_HTML.contains(route), "missing admin workflow route {route}");
        }
        for legacy_capability in [
            "systemVerdict",
            "target_headroom",
            "coverage['7d']",
            "reset5h_in",
            "peak_cap5h_usd",
            "Аккаунты движка",
            "crm-parsing",
        ] {
            assert!(ADMIN_PANEL_HTML.contains(legacy_capability),
                "missing migrated legacy capability {legacy_capability}");
        }
        assert!(!ADMIN_PANEL_HTML.contains("COMMERCIAL_ADMIN_KEY"));
        assert!(!ADMIN_PANEL_HTML.contains("CONTROL_KEY"));
        assert!(!ADMIN_PANEL_HTML.contains("SALES_ADMIN_KEY"));
        assert!(ADMIN_PANEL_HTML.contains("sessionStorage.setItem(pendingKey"));
        assert!(ADMIN_PANEL_HTML.contains("idempotency_key:idempotencyKey"));
        assert!(ADMIN_PANEL_HTML.contains("Пароль (минимум 8)"));
        assert!(ADMIN_PANEL_HTML.contains("minlength=\"8\""));
        assert!(ADMIN_PANEL_HTML.contains("first.length<8"));
    }
}
