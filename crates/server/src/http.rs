//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health   — жив ли сервер (без авторизации)
//!   GET /pool     — статус пула (util/cooling, без секретов)
//!   *             — форвардинг на api.anthropic.com (см. forward::forward)

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use forward::{authed, client_key, control_authed, forward, readonly_authed, AppState, Metrics};
use crate::admin;
use serde_json::json;
use std::net::SocketAddr;

/// Живой дашборд ёмкости (self-contained HTML; данные тянет с /capacity тем же origin).
const PANEL_HTML: &str = include_str!("panel.html");

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/pool", get(pool_status))
        .route("/balance", get(balance))
        .route("/capacity", get(capacity))
        .route("/metrics", get(metrics))
        .route("/panel", get(panel))
        // Control-плоскость (`/admin/*`) — управление аккаунтами/ключами/балансом за control-ключом.
        // Контракт для будущей КОММЕРЦИИ (отдельный сервис). Движок — авторитет живого баланса.
        .route("/admin/account", post(admin::create_account))
        .route("/admin/account/{id}", get(admin::get_account))
        .route("/admin/account/{id}/credit", post(admin::credit_account))
        .route("/admin/account/{id}/status", post(admin::account_status))
        .route("/admin/account/{id}/keys", get(admin::list_keys))
        .route("/admin/account/{id}/ledger", get(admin::list_ledger))
        .route("/admin/key", post(admin::issue_key))
        .route("/admin/key/{key}/status", post(admin::key_status))
        .fallback(forward)
        .with_state(app)
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

/// Отдать HTML-панель (без авторизации — это только UI; данные /capacity требуют ключ,
/// который панель спрашивает у пользователя и хранит в его браузере).
async fn panel() -> Response {
    ([(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")], PANEL_HTML).into_response()
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
    let caps = app.pool.capacity();
    let round = |x: f64| (x * 100.0).round() / 100.0; // 2 знака
    // Маскируем email подписки: панельному (read-only) ключу не отдаём реальные адреса пула — это
    // операционно чувствительно (реселлимые Claude-аккаунты; полный email помогает корреляции/бану).
    // Оставляем стабильный различитель: первые 4 символа + «…». Оператору хватает отличить подписки.
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
    Json(json!({
        "now": pool::now(),
        "subs": subs.len(),
        "calibrated": all_calibrated,   // false → хотя бы одна подписка ещё на прайоре
        "available_usd": {              // суммарно по флоту, USD real-API-эквивалента
            "next_1h": round(a1), "next_5h": round(a5), "next_1d": round(a1d), "next_7d": round(a7d),
        },
        "per_sub": subs,
    })).into_response()
}

async fn health() -> Json<serde_json::Value> {
    // Минимум без авторизации: голый liveness-пинг. Размер пула / upstream / статус биллинга —
    // раскрытие backend (у api.anthropic.com нет /health, N подписок = фингерпринт) → только на
    // авторизованных /pool и /metrics.
    Json(json!({ "ok": true }))
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
        Some(a) => billing.account(&a.account_id).await,
        None => None,
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
