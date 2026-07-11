//! Фоновые задачи: (1) перечитывание реестра из БД, (2) опрос лимитов подписок.

use crate::state::AppState;
use crate::upstream::poll_sub;
use crate::{db, pool};
use std::time::Duration;

/// Адаптивный интервал опроса: горячие подписки опрашиваем чаще.
fn poll_interval(l: &pool::Live) -> i64 {
    let u = l.util5h.max(l.util7d);
    if u >= 0.7 { 12 } else if u >= 0.3 { 30 } else { 60 }
}

/// Раз в reload_secs перечитываем subs из БД (подхватываем добавленные/убранные подписки).
pub async fn reload_loop(app: AppState) {
    loop {
        if let Ok(conn) = db::open(&app.cfg.db_path) {
            if let Ok(subs) = db::load_active(&conn, app.cfg.fleet.as_deref()) {
                app.pool.replace_subs(subs);
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// Фоновый поллер лимитов: для каждой подписки, чей интервал подошёл, читаем ratelimit.
pub async fn poll_loop(app: AppState) {
    loop {
        let now = pool::now();
        let snap = app.pool.snapshot();
        for (sub, live) in snap {
            if now - live.polled_ts < poll_interval(&live) { continue; }
            let client = match app.clients.get(&sub.proxy) { Ok(c) => c, Err(_) => continue };
            if let Some(r) = poll_sub(&client, &app.cfg, &sub.token).await {
                app.pool.set_util(&sub.email, r.util5h, r.util7d, r.status.clone(), r.reset5h, r.reset7d);
                if r.http == 429 {
                    let secs = r.reset5h.map(|t| (t - now).max(1)).unwrap_or(app.cfg.cool_secs);
                    app.pool.mark_cooling(&sub.email, secs);
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
    }
}
