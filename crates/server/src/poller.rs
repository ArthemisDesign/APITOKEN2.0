//! Фоновые задачи — СОБЫТИЙНЫЕ, не фиксированно-периодические:
//! (1) перечитывание реестра из БД (подхват добавленных/убранных подписок),
//! (2) liveness-опрос лимитов ТОЛЬКО простаивающих подписок.
//!
//! Ключевая идея: источник истины лимитов — заголовки боевых ответов (пассивный сбор в `forward`
//! обновляет `polled_ts`). Момент сброса окна вычисляется локально (`reset` — абсолютный timestamp),
//! поэтому его НЕ надо «ловить» опросом. Активный probe нужен лишь чтобы (а) впервые узнать лимиты
//! новой подписки, (б) редко проверить, что токен простаивающей подписки ещё жив. Никакого
//! периодического скана: спим ровно до ближайшего due-времени или до сигнала об изменении флота.

use forward::{persona_ua, poll_sub, AppState};
use registry::Sub;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::task::JoinSet;

/// Простаивающую подписку пингуем не чаще этого (сек). Не для «поймать сброс» (он вычисляется), а
/// лишь для проверки живости токена. Под боевым трафиком `polled_ts` свеж → probe не срабатывает.
const LIVENESS_INTERVAL: i64 = 300;
/// Потолок одновременных probe (медленный прокси не блокирует остальных).
const PROBE_CONCURRENCY: usize = 16;
/// Потолок сна (сек) — верхняя граница ожидания, чтобы liveness не растягивался бесконечно.
const MAX_SLEEP: i64 = 300;

/// Когда подписка «созреет» для активного probe. `polled_ts==0` (никогда не опрошена) → сейчас.
fn next_probe_at(live: &pool::Live) -> i64 {
    if live.polled_ts == 0 { 0 } else { live.polled_ts + LIVENESS_INTERVAL }
}

/// Перечитывание реестра: подхватываем добавленные/убранные подписки. Спит 30с (локальная БД,
/// дёшево), но будит поллер `poke` ТОЛЬКО когда набор реально изменился (онбординг новой подписки
/// не ждёт liveness-таймера).
pub async fn reload_loop(app: AppState, db_path: String, fleet: Option<String>, poke: Arc<Notify>) {
    let mut prev: HashSet<String> = HashSet::new();
    loop {
        if let Ok(conn) = registry::open(&db_path) {
            if let Ok(subs) = registry::load_active(&conn, fleet.as_deref()) {
                let cur: HashSet<String> = subs.iter().map(|s| s.email.clone()).collect();
                let membership_changed = cur != prev;
                // ВСЕГДА заменяем: подхватываем смену token/proxy того же email (внешняя
                // перепровизия authbot/CLI на живом сервере) — иначе держали бы протухший до рестарта.
                // replace_subs сохраняет volatile-состояние существующих (retain по email).
                app.pool.replace_subs(subs);
                if membership_changed {
                    prev = cur;
                    poke.notify_one(); // состав изменился → поллер probe-нёт новых
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// Один liveness-probe: читаем лимиты подписки и применяем. Сетевой сбой всё равно фиксируем
/// (`set_util` c None двигает `polled_ts`) — иначе поллер спинил бы по мёртвому прокси.
async fn probe(app: &AppState, sub: &Sub) {
    let client = match app.clients.get(&sub.proxy) {
        Ok(c) => c,
        Err(_) => { app.pool.set_util(&sub.email, None, None, None, None, None); return; }
    };
    let ua = persona_ua(&app.cfg, &sub.email);
    match poll_sub(&client, &app.cfg, &sub.token, &ua).await {
        Some(r) => {
            app.pool.set_util(&sub.email, r.util5h, r.util7d, r.status.clone(), r.reset5h, r.reset7d);
            match r.http {
                429 => {
                    // студим до сброса ОКНА-виновника (недельное почти выбрано → до reset7d, дни).
                    let reset = if r.util7d.unwrap_or(0.0) >= 0.98 { r.reset7d.or(r.reset5h) }
                                else { r.reset5h.or(r.reset7d) };
                    let secs = reset.map(|t| (t - pool::now()).max(1)).unwrap_or(app.cfg.cool_secs);
                    app.pool.cool(&sub.email, secs);
                }
                401 | 403 => app.pool.cool(&sub.email, 900), // мёртвый токен, обнаружен на простое
                _ => {}
            }
        }
        None => app.pool.set_util(&sub.email, None, None, None, None, None), // записать попытку (backoff)
    }
}

/// Safety-flush персиста (сек): калибровка, изменившаяся БЕЗ cooling, всё же осядет.
const PERSIST_SAFETY: u64 = 120;
/// Коалесцируем всплеск cooling-событий перед записью (сек).
const PERSIST_DEBOUNCE: u64 = 1;

/// Персист состояния пула: **write-through по событию** cooling (`poke` из `pool.on_change`) —
/// бан переживает рестарт почти сразу; плюс редкий safety-flush для калибровки. Не фиксированный
/// снапшот «раз в N»: под тишиной (нет cooling) пишем лишь раз в `PERSIST_SAFETY`.
pub async fn persist_loop(app: AppState, db_path: String, poke: Arc<Notify>) {
    loop {
        tokio::select! {
            _ = poke.notified() => { tokio::time::sleep(Duration::from_secs(PERSIST_DEBOUNCE)).await; }
            _ = tokio::time::sleep(Duration::from_secs(PERSIST_SAFETY)) => {}
        }
        let rows = app.pool.export_state();
        if rows.is_empty() { continue; }
        if let Ok(conn) = registry::open(&db_path) {
            let _ = registry::save_pool_state(&conn, &rows);
        }
    }
}

/// Событийный liveness-поллер: probe-ит созревшие подписки конкурентно, затем спит РОВНО до
/// ближайшего due-времени (или до `poke` при изменении флота). Фиксированного тика нет.
pub async fn poll_loop(app: AppState, poke: Arc<Notify>) {
    loop {
        let now = pool::now();
        let snap = app.pool.snapshot();
        let due: Vec<Sub> = snap.iter()
            .filter(|(_, l)| next_probe_at(l) <= now)
            .map(|(s, _)| s.clone())
            .collect();

        if !due.is_empty() {
            let mut set: JoinSet<()> = JoinSet::new();
            for sub in due {
                if set.len() >= PROBE_CONCURRENCY { set.join_next().await; }
                let app = app.clone();
                set.spawn(async move { probe(&app, &sub).await; });
            }
            while set.join_next().await.is_some() {}
        }

        // спим до ближайшего due (событийно), но не дольше MAX_SLEEP; будимся раньше по `poke`.
        let next = app.pool.snapshot().iter().map(|(_, l)| next_probe_at(l)).min().unwrap_or(now + MAX_SLEEP);
        let sleep_s = (next - pool::now()).clamp(1, MAX_SLEEP);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(sleep_s as u64)) => {}
            _ = poke.notified() => {}
        }
    }
}
