//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health,/live — жив ли сервер (без авторизации)
//!   GET /ready        — принимает ли сервер новый трафик (без авторизации)
//!   GET /pool         — статус пула (util/cooling, без секретов)
//!   *             — форвардинг на api.anthropic.com (см. forward::forward)

use crate::admin;
use axum::extract::{ConnectInfo, FromRef, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{any, get, post};
use axum::Router;
use forward::{
    authed, client_key, control_authed, forward, openai_chat_completions, openai_model,
    openai_models, openai_responses, readonly_authed, AppState, Metrics,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Unified self-contained administration UI. Data is routed by the admin Caddy vhost.
const ADMIN_PANEL_HTML: &str = include_str!("admin-panel.html");
/// Caddy removes this internal marker on the Claude hostname and sets it only for
/// `openai.api.apitoken.sale`. Provider selection must never depend on client auth headers.
const API_PLANE_HEADER: &str = "x-apitoken-api-plane";
const OPENAI_API_PLANE: &[u8] = b"openai";

/// TTL-кэш дашборд-эндпоинтов: панель поллит /overview и /capacity, а они делают O(n)-скан
/// (capacity() под пул-локом + billing_totals full-scan). Кэш на 2с → при N поллерах пересчёт
/// максимум раз в 2с, не крадём hot-path лок у боевых запросов. Свежесть суб-секунд дашборду не нужна.
const DASH_TTL: std::time::Duration = std::time::Duration::from_secs(2);
type DashCache =
    std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>;
static OVERVIEW_CACHE: DashCache = std::sync::OnceLock::new();
/// /spend-stats сканирует usage_events за 30 дней — TTL длиннее дашбордного.
const SPEND_TTL: std::time::Duration = std::time::Duration::from_secs(30);
static SPEND_CACHE: DashCache = std::sync::OnceLock::new();
static CAPACITY_CACHE: DashCache = std::sync::OnceLock::new();
static SUBS_CACHE: DashCache = std::sync::OnceLock::new();
/// Планируемый срок жизни подписки — ровно N дней от добавления токена (`added_ts`). Это НЕ срок
/// самого OAuth-токена (opaque, недоступен), а наш горизонт планирования замены.
const SUB_LIFETIME_DAYS: i64 = 30;
fn cache_get(cell: &DashCache) -> Option<serde_json::Value> {
    cache_get_ttl(cell, DASH_TTL)
}
fn cache_get_ttl(cell: &DashCache, ttl: std::time::Duration) -> Option<serde_json::Value> {
    cell.get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(t, _)| t.elapsed() < ttl)
        .map(|(_, v)| v.clone())
}
fn cache_put(cell: &DashCache, v: &serde_json::Value) {
    *cell
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap() = Some((std::time::Instant::now(), v.clone()));
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

macro_rules! define_admin_routes {
    ($(($route_fn:ident, $method:ident, $pattern:literal, $sample:literal, $handler:path)),+ $(,)?) => {
        #[cfg(test)]
        const ADMIN_ROUTE_CASES: &[(axum::http::Method, &str)] = &[
            $((axum::http::Method::$method, $sample),)+
        ];

        fn admin_router(app: &AppState) -> Router<HttpState> {
            Router::new()
                $(.route($pattern, $route_fn($handler)))+
                .route_layer(middleware::from_fn_with_state(
                    app.clone(),
                    require_control_auth,
                ))
        }
    };
}

define_admin_routes!(
    (
        post,
        POST,
        "/admin/account",
        "/admin/account",
        admin::create_account
    ),
    (
        get,
        GET,
        "/admin/account/{id}",
        "/admin/account/test-account",
        admin::get_account
    ),
    (
        post,
        POST,
        "/admin/account/{id}/credit",
        "/admin/account/test-account/credit",
        admin::credit_account
    ),
    (
        post,
        POST,
        "/admin/account/{id}/status",
        "/admin/account/test-account/status",
        admin::account_status
    ),
    (
        post,
        POST,
        "/admin/account/{id}/pricing",
        "/admin/account/test-account/pricing",
        admin::account_pricing
    ),
    (
        get,
        GET,
        "/admin/account/{id}/keys",
        "/admin/account/test-account/keys",
        admin::list_keys
    ),
    (
        get,
        GET,
        "/admin/account/{id}/ledger",
        "/admin/account/test-account/ledger",
        admin::list_ledger
    ),
    (
        post,
        POST,
        "/admin/account/{id}/ledger/ack",
        "/admin/account/test-account/ledger/ack",
        admin::ack_ledger
    ),
    (
        get,
        GET,
        "/admin/account/{id}/usage",
        "/admin/account/test-account/usage",
        admin::list_usage
    ),
    (post, POST, "/admin/key", "/admin/key", admin::issue_key),
    (
        post,
        POST,
        "/admin/key-id/{key_id}/status",
        "/admin/key-id/test-key/status",
        admin::key_status_by_id
    ),
    (
        post,
        POST,
        "/admin/key-id/{key_id}/label",
        "/admin/key-id/test-key/label",
        admin::key_label_by_id
    ),
    (
        post,
        POST,
        "/admin/account/{account_id}/key-id/{key_id}/policy",
        "/admin/account/test-account/key-id/test-key/policy",
        admin::key_policy_by_id
    ),
);

async fn require_control_auth(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if control_authed(&app, request.headers(), &peer) {
        return next.run(request).await;
    }
    Metrics::inc(&app.metrics.auth_failures);
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

pub fn router(app: AppState, accepting: Arc<AtomicBool>) -> Router {
    let admin = admin_router(&app);
    Router::new()
        .route("/health", get(health))
        .route("/live", get(health))
        .route("/ready", get(ready))
        .route("/pool", get(pool_status))
        .route("/balance", get(balance))
        .route("/capacity", get(capacity))
        .route("/overview", get(overview))
        .route("/spend-stats", get(spend_stats))
        .route("/subs", get(subs))
        .route("/metrics", get(metrics))
        .route("/admin-panel", get(admin_panel))
        // Control-плоскость (`/admin/*`) — управление аккаунтами/ключами/балансом за control-ключом.
        // Контракт для будущей КОММЕРЦИИ (отдельный сервис). Движок — авторитет живого баланса.
        .merge(admin)
        // OpenAI-compatible text surface backed by the official Codex app-server. Caddy marks
        // requests from openai.api.apitoken.sale; every unmarked request preserves the exact
        // legacy Claude fallback, regardless of its authentication headers.
        .route("/v1/responses", post(responses_dispatch))
        .route(
            "/v1/responses/{*rest}",
            any(unsupported_openai_subroute_dispatch),
        )
        .route("/v1/chat/completions", post(chat_completions_dispatch))
        .route(
            "/v1/chat/completions/{*rest}",
            any(unsupported_openai_subroute_dispatch),
        )
        .route("/v1/models", get(models_dispatch))
        .route("/v1/models/{model_id}", get(model_dispatch))
        .fallback(provider_fallback_dispatch)
        .with_state(HttpState { app, accepting })
}

fn is_openai_plane(headers: &HeaderMap) -> bool {
    headers
        .get(API_PLANE_HEADER)
        .is_some_and(|value| value.as_bytes() == OPENAI_API_PLANE)
}

async fn responses_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_responses(State(app), ConnectInfo(peer), request).await
}

async fn chat_completions_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_chat_completions(State(app), ConnectInfo(peer), request).await
}

async fn models_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_models(State(app), ConnectInfo(peer), request.headers().clone()).await
}

async fn model_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(model_id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_model(
        State(app),
        ConnectInfo(peer),
        request.headers().clone(),
        Path(model_id),
    )
    .await
}

async fn unsupported_openai_subroute_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    (
        StatusCode::NOT_FOUND,
        Json(unsupported_openai_endpoint_error()),
    )
        .into_response()
}

async fn provider_fallback_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if is_openai_plane(request.headers()) {
        return (
            StatusCode::NOT_FOUND,
            Json(unsupported_openai_endpoint_error()),
        )
            .into_response();
    }
    forward(State(app), ConnectInfo(peer), request).await
}

fn unsupported_openai_endpoint_error() -> serde_json::Value {
    json!({
        "error": {
            "message": "The requested endpoint is not supported.",
            "type": "invalid_request_error",
            "param": Value::Null,
            "code": Value::Null
        }
    })
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
    let affinity = app.affinity.stats();
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
         # TYPE claude_api_affinity_local_hits_total counter\nclaude_api_affinity_local_hits_total {}\n\
         # TYPE claude_api_affinity_redis_hits_total counter\nclaude_api_affinity_redis_hits_total {}\n\
         # TYPE claude_api_affinity_misses_total counter\nclaude_api_affinity_misses_total {}\n\
         # TYPE claude_api_affinity_redis_errors_total counter\nclaude_api_affinity_redis_errors_total {}\n\
         # TYPE claude_api_affinity_native_hits_total counter\nclaude_api_affinity_native_hits_total {}\n\
         # TYPE claude_api_affinity_transcript_hits_total counter\nclaude_api_affinity_transcript_hits_total {}\n\
         # TYPE claude_api_affinity_cache_root_hits_total counter\nclaude_api_affinity_cache_root_hits_total {}\n\
         # TYPE claude_api_affinity_cache_root_writes_total counter\nclaude_api_affinity_cache_root_writes_total {}\n\
         # TYPE claude_api_affinity_cache_root_warm_placements_total counter\nclaude_api_affinity_cache_root_warm_placements_total {}\n\
         # TYPE claude_api_affinity_cache_root_cold_placements_total counter\nclaude_api_affinity_cache_root_cold_placements_total {}\n\
         # TYPE claude_api_affinity_claims_total counter\nclaude_api_affinity_claims_total {}\n\
         # TYPE claude_api_affinity_rebinds_total counter\nclaude_api_affinity_rebinds_total {}\n\
         # TYPE claude_api_inflight gauge\nclaude_api_inflight {}\n\
         # TYPE claude_api_subs gauge\nclaude_api_subs {}\n\
         # TYPE claude_api_cooling gauge\nclaude_api_cooling {}\n\
         # TYPE claude_api_breaker_open gauge\nclaude_api_breaker_open {}\n",
        g(&m.requests),
        g(&m.upstream_429),
        g(&m.upstream_auth),
        g(&m.upstream_5xx),
        g(&m.breaker_rejects),
        g(&m.exhausted),
        g(&m.key_throttled),
        g(&m.auth_failures),
        rs.pin,
        rs.spill,
        rs.place,
        affinity.local_hits,
        affinity.redis_hits,
        affinity.misses,
        affinity.redis_errors,
        affinity.native_hits,
        affinity.transcript_hits,
        affinity.cache_root_hits,
        affinity.cache_root_writes,
        affinity.cache_root_warm_placements,
        affinity.cache_root_cold_placements,
        affinity.claims,
        affinity.rebinds,
        inflight,
        app.pool.len(),
        cooling,
        breaker_open,
    );
    // Наблюдаемость трат: агрегаты по клиентским ключам (USD) — только когда биллинг включён И вызов
    // авторизован CONTROL-ключом. Выручка/остатки клиентов — коммерческая тайна: панельному (read-only)
    // ключу их НЕ отдаём (он видит лишь операционные метрики inflight/breaker/429).
    let mut body = match &app.billing {
        Some(b) if control_authed(&app, &headers, &peer) => match b.totals().await {
            Ok(t) => {
                let usd = |n: i64| n as f64 / 1e9;
                format!(
                "{body}# TYPE claude_api_billing_authority_up gauge\nclaude_api_billing_authority_up 1\n\
                 # TYPE claude_api_client_balance_usd gauge\nclaude_api_client_balance_usd {:.6}\n\
                 # TYPE claude_api_billed_usd_total counter\nclaude_api_billed_usd_total {:.6}\n\
                 # TYPE claude_api_reserved_usd gauge\nclaude_api_reserved_usd {:.6}\n\
                 # TYPE claude_api_metered_accounts gauge\nclaude_api_metered_accounts {}\n",
                usd(t.balance_nano), usd(t.spent_nano), usd(t.reserved_nano), t.active_accounts,
                    )
            }
            Err(error) => {
                eprintln!("billing metrics query failed: {error:#}");
                format!("{body}# TYPE claude_api_billing_authority_up gauge\nclaude_api_billing_authority_up 0\n")
            }
        },
        _ => body, // биллинг выключен ИЛИ вызов не control → только операционные метрики
    };
    use std::fmt::Write as _;
    let _ = writeln!(
        body,
        "# TYPE claude_api_codex_enabled gauge\nclaude_api_codex_enabled {}",
        u8::from(app.codex.is_some())
    );
    let codex_status = if let Some(codex) = &app.codex {
        Some(codex.operational_status().await)
    } else {
        None
    };
    let _ = writeln!(
        body,
        "# TYPE claude_api_codex_process_live gauge\nclaude_api_codex_process_live {}",
        u8::from(
            codex_status
                .as_ref()
                .is_some_and(|status| status.process_live)
        )
    );
    let limits = codex_status.as_ref().and_then(|status| status.rate_limits.clone());
    let _ = writeln!(
        body,
        "# TYPE claude_api_codex_rate_limit_snapshot_available gauge\n\
         claude_api_codex_rate_limit_snapshot_available {}\n\
         # TYPE claude_api_codex_rate_limit_reached gauge\n\
         claude_api_codex_rate_limit_reached {}",
        u8::from(limits.is_some()),
        u8::from(limits.as_ref().is_some_and(|limits| limits.reached))
    );
    if let Some(limits) = limits {
        let _ = writeln!(
            body,
            "# TYPE claude_api_codex_rate_limit_snapshot_timestamp_seconds gauge\n\
             claude_api_codex_rate_limit_snapshot_timestamp_seconds {}\n\
             # TYPE claude_api_codex_rate_limit_used_percent gauge\n\
             # TYPE claude_api_codex_rate_limit_window_minutes gauge\n\
             # TYPE claude_api_codex_rate_limit_resets_at_seconds gauge",
            limits.observed_at
        );
        for (window_name, window) in [
            ("primary", limits.primary.as_ref()),
            ("secondary", limits.secondary.as_ref()),
        ] {
            let Some(window) = window else {
                continue;
            };
            let _ = writeln!(
                body,
                "claude_api_codex_rate_limit_used_percent{{window=\"{window_name}\"}} {}",
                window.used_percent
            );
            if let Some(duration) = window.window_duration_mins {
                let _ = writeln!(
                    body,
                    "claude_api_codex_rate_limit_window_minutes{{window=\"{window_name}\"}} {duration}"
                );
            }
            if let Some(resets_at) = window.resets_at {
                let _ = writeln!(
                    body,
                    "claude_api_codex_rate_limit_resets_at_seconds{{window=\"{window_name}\"}} {resets_at}"
                );
            }
        }
    }
    // Pool health. Homes are labelled by their configured index only: a path or account identity
    // must never reach a metric label.
    if let Some(status) = &codex_status {
        let _ = writeln!(
            body,
            "# TYPE claude_api_codex_homes gauge\nclaude_api_codex_homes {}\n\
             # TYPE claude_api_codex_homes_available gauge\nclaude_api_codex_homes_available {}\n\
             # TYPE claude_api_codex_homes_authenticated gauge\nclaude_api_codex_homes_authenticated {}",
            status.homes.len(),
            status.available,
            status.homes.iter().filter(|home| home.auth_ok).count(),
        );
        if let Some(ready_at) = status.soonest_ready {
            let _ = writeln!(
                body,
                "# TYPE claude_api_codex_soonest_ready_seconds gauge\n\
                 claude_api_codex_soonest_ready_seconds {ready_at}"
            );
        }
        let _ = writeln!(
            body,
            "# TYPE claude_api_codex_home_process_live gauge\n\
             # TYPE claude_api_codex_home_authenticated gauge\n\
             # TYPE claude_api_codex_home_cooling_until_seconds gauge\n\
             # TYPE claude_api_codex_home_inflight_turns gauge\n\
             # TYPE claude_api_codex_home_rate_limit_used_percent gauge"
        );
        for home in &status.homes {
            let index = &home.id;
            let _ = writeln!(
                body,
                "claude_api_codex_home_process_live{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_authenticated{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_cooling_until_seconds{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_inflight_turns{{home=\"{index}\"}} {}",
                u8::from(home.process_live),
                u8::from(home.auth_ok),
                home.cooling_until,
                home.inflight,
            );
            if let Some(used) = home
                .rate_limits
                .as_ref()
                .and_then(|limits| limits.max_used_percent())
            {
                let _ = writeln!(
                    body,
                    "claude_api_codex_home_rate_limit_used_percent{{home=\"{index}\"}} {used}"
                );
            }
        }
    }
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
        .into_response()
}

/// Unified admin.apitoken.sale UI. Human and data authorization is enforced by Caddy.
async fn admin_panel() -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        ADMIN_PANEL_HTML,
    )
        .into_response()
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    if let Some(v) = cache_get(&CAPACITY_CACHE) {
        return Json(v).into_response();
    }
    let v = capacity_value(&app);
    cache_put(&CAPACITY_CACHE, &v);
    Json(v).into_response()
}

/// Cooling/dead subscriptions remain visible per-sub but must never inflate sellable supply.
fn routable_capacity_summary(caps: &[pool::Cap]) -> ([f64; 4], bool) {
    let mut available = [0.0; 4];
    let mut routable = 0usize;
    let mut all_calibrated = true;
    for cap in caps.iter().filter(|cap| cap.routable) {
        available[0] += cap.avail_1h_usd;
        available[1] += cap.avail_5h_usd;
        available[2] += cap.avail_1d_usd;
        available[3] += cap.avail_7d_usd;
        routable += 1;
        all_calibrated &= cap.calibrated;
    }
    (available, routable > 0 && all_calibrated)
}

/// Вычисление ёмкости пула (per-sub + агрегат) — для `/capacity` и TTL-кэша. Синхронно.
fn capacity_value(app: &AppState) -> serde_json::Value {
    let caps = app.pool.capacity();
    let round = |x: f64| (x * 100.0).round() / 100.0;
    // Маскируем email подписки: панельному (read-only) ключу не отдаём реальные адреса пула — это
    // операционно чувствительно (реселлимые Claude-аккаунты; полный email помогает корреляции/бану).
    let mask_email = |e: &str| -> String {
        let head: String = e.chars().take(4).collect();
        format!("{head}…")
    };
    let ([a1, a5, a1d, a7d], all_calibrated) = routable_capacity_summary(&caps);
    let subs: Vec<_> = caps
        .iter()
        .map(|c| {
            json!({
                "email": mask_email(&c.email),
                "calibrated": c.calibrated,
                "routable": c.routable,
                "util5h": round(c.util5h), "util7d": round(c.util7d),
                "reset5h_in": c.reset5h_in, "reset7d_in": c.reset7d_in,
                "cap5h_usd": round(c.cap5h_usd), "cap7d_usd": round(c.cap7d_usd),
                "rem5h_usd": round(c.rem5h_usd), "rem7d_usd": round(c.rem7d_usd),
                "avail_1h_usd": round(c.avail_1h_usd), "avail_5h_usd": round(c.avail_5h_usd),
                "avail_1d_usd": round(c.avail_1d_usd), "avail_7d_usd": round(c.avail_7d_usd),
                "status": c.status, "cooling": c.cooling,
                "dead": c.auth_dead,   // токен отвергнут Anthropic (корроборированно) → «мёртвая» подписка
                "auth_state": c.auth_state,       // "healthy" | "suspect" | "dead" (durable)
                "dead_reason": c.dead_reason,     // "authentication_error" (re-auth) | "permission_error" (banned)
                "dead_since": c.dead_since_ts,    // когда стала dead (0 = нет)
            })
        })
        .collect();
    let dead_count = caps.iter().filter(|c| c.auth_dead).count();
    let suspect_count = caps.iter().filter(|c| c.auth_state == "suspect").count();
    json!({
        "now": pool::now(),
        "subs": subs.len(),
        "dead": dead_count,             // >0 → есть мёртвые токены (401/403): вне ротации, нужна замена
        "suspect": suspect_count,       // auth падает, под наблюдением (ещё не приговор)
        "calibrated": all_calibrated,   // false → хотя бы одна подписка ещё на прайоре
        "available_usd": {              // суммарно по флоту, USD real-API-эквивалента
            "next_1h": round(a1), "next_5h": round(a5), "next_1d": round(a1d), "next_7d": round(a7d),
        },
        "per_sub": subs,
    })
}

/// Control-room: агрегат СПРОС (балансы/резерв/траты) + ПРЕДЛОЖЕНИЕ (ёмкость/потребление/здоровье)
/// + headroom + рекомендация по числу подписок. Одно окно «мы под контролем?». Гейт — control-ключ
///
/// (деньги = коммерческая тайна). Работает от 1 до 1000 подписок — всё в агрегатах и отношениях.
async fn overview(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !control_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    if let Some(v) = cache_get(&OVERVIEW_CACHE) {
        return Json(v).into_response();
    }
    let mut v = match overview_value(&app).await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("billing overview query failed: {error:#}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response();
        }
    };
    // Разбивка спроса по engine-аккаунтам — только в HTTP-ответе панели; в историю
    // (poller::metrics_loop пишет overview_value) её не кладём, чтобы не раздувать metrics.db.
    if let Some(b) = &app.billing {
        let account_rows = match b.accounts().await {
            Ok(rows) => rows,
            Err(error) => {
                eprintln!("billing account overview failed: {error:#}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "billing authority unavailable"})),
                )
                    .into_response();
            }
        };
        let accounts: Vec<_> = account_rows
            .into_iter()
            .map(|a| {
                json!({
                    "account": a.id,
                    "handle": a.handle,
                    "balance_usd": (a.balance_nano as f64 / 1e9 * 100.0).round() / 100.0,
                    "spent_usd": (a.spent_nano as f64 / 1e9 * 100.0).round() / 100.0,
                    "mult": a.mult_bp as f64 / 10000.0,
                    "status": a.status,
                })
            })
            .collect();
        v["accounts"] = json!(accounts);
    }
    cache_put(&OVERVIEW_CACHE, &v);
    Json(v).into_response()
}

/// Панель «кто тратит»: разбивка расхода по engine-аккаунтам за окна 24ч/7д/30д.
/// На каждую строку — списано клиенту (charge, с его множителем) И real-API эквивалент,
/// чтобы был виден эффект скидки. Гейт — control-ключ, как у /overview.
async fn spend_stats(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !control_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let Some(b) = &app.billing else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "billing authority unavailable"})),
        )
            .into_response();
    };
    if let Some(v) = cache_get_ttl(&SPEND_CACHE, SPEND_TTL) {
        return Json(v).into_response();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let r2 = |nano: i64| (nano as f64 / 1e9 * 100.0).round() / 100.0;
    let mut periods = serde_json::Map::new();
    for (key, secs) in [("d1", 86_400i64), ("d7", 7 * 86_400), ("d30", 30 * 86_400)] {
        let rows = match b.spend_by_account(now - secs, 50).await {
            Ok(rows) => rows,
            Err(error) => {
                eprintln!("billing spend stats query failed: {error:#}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "billing authority unavailable"})),
                )
                    .into_response();
            }
        };
        let (mut charge_total, mut real_total, mut requests_total) = (0i64, 0i64, 0i64);
        let accounts: Vec<_> = rows
            .iter()
            .map(|row| {
                charge_total += row.charge_nano;
                real_total += row.real_nano;
                requests_total += row.requests;
                json!({
                    "account": row.account_id,
                    "handle": row.handle,
                    "requests": row.requests,
                    "charge_usd": r2(row.charge_nano),
                    "real_usd": r2(row.real_nano),
                    "last_ts": row.last_ts,
                })
            })
            .collect();
        // Which upstream earned the window. Both providers settle into the same money tables, so
        // this split comes from the explicit provider column rather than from a model-name guess.
        let providers: Vec<_> = match b.spend_by_provider(now - secs).await {
            Ok(rows) => rows
                .into_iter()
                .map(|row| {
                    json!({
                        "provider": row.provider,
                        "requests": row.requests,
                        "charge_usd": r2(row.charge_nano),
                        "real_usd": r2(row.real_nano),
                    })
                })
                .collect(),
            Err(error) => {
                eprintln!("billing provider spend query failed: {error:#}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "billing authority unavailable"})),
                )
                    .into_response();
            }
        };
        periods.insert(
            key.into(),
            json!({
                "charge_usd": r2(charge_total),
                "real_usd": r2(real_total),
                "requests": requests_total,
                "accounts": accounts,
                "providers": providers,
            }),
        );
    }
    let v = json!({"now": now, "periods": periods});
    cache_put(&SPEND_CACHE, &v);
    Json(v).into_response()
}

/// Вычисление control-room агрегата (без авторизации) — переиспользуется хендлером `/overview`
/// И фоновым коллектором истории (`poller::metrics_loop`). Считается на лету из пула + биллинга.
pub(crate) async fn overview_value(app: &AppState) -> anyhow::Result<serde_json::Value> {
    const TARGET_HEADROOM: f64 = 1.3; // держим 30% запас
    const REF_MULT: f64 = 0.20; // референсная наценка для «продаём клиентам» и coverage
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    let caps = app.pool.capacity();
    let n = caps.len();
    let usable = caps.iter().filter(|cap| cap.routable).count();
    let (mut a1, mut a5, mut a1d, mut a7d) = (0.0, 0.0, 0.0, 0.0);
    let (mut cap5, mut cap7, mut rem5, mut rem7) = (0.0, 0.0, 0.0, 0.0);
    let (mut u5, mut u7) = (0.0, 0.0);
    let (mut cooling, mut dead, mut suspect, mut all_cal) = (0usize, 0usize, 0usize, usable > 0);
    for c in &caps {
        if c.routable {
            a1 += c.avail_1h_usd;
            a5 += c.avail_5h_usd;
            a1d += c.avail_1d_usd;
            a7d += c.avail_7d_usd;
            cap5 += c.cap5h_usd;
            cap7 += c.cap7d_usd;
            rem5 += c.rem5h_usd;
            rem7 += c.rem7d_usd;
            u5 += c.util5h;
            u7 += c.util7d;
        }
        if c.auth_dead {
            dead += 1;
        } else if c.cooling {
            cooling += 1;
        } else if c.auth_state == "suspect" {
            suspect += 1;
        }
        if c.routable && !c.calibrated {
            all_cal = false;
        }
    }
    let cons5 = (cap5 - rem5).max(0.0); // real-API-$ потрачено в текущем 5h окне (по всему флоту)
    let cons7 = (cap7 - rem7).max(0.0);
    let head5 = if cons5 > 0.01 {
        a5 / cons5
    } else {
        f64::INFINITY
    }; // «во сколько раз ещё выдержим»
    let head7 = if cons7 > 0.01 {
        a7d / cons7
    } else {
        f64::INFINITY
    };
    // Рекомендация: сколько подписок держать под target_headroom относительно текущего потребления.
    let per_sub_5h = if usable > 0 {
        cap5 / usable as f64
    } else {
        0.0
    };
    let per_sub_7d = if usable > 0 {
        cap7 / usable as f64
    } else {
        0.0
    };
    let need5 = if per_sub_5h > 0.0 {
        (cons5 * TARGET_HEADROOM / per_sub_5h).ceil() as i64
    } else {
        0
    };
    let need7 = if per_sub_7d > 0.0 {
        (cons7 * TARGET_HEADROOM / per_sub_7d).ceil() as i64
    } else {
        0
    };
    let need = need5.max(need7).max(usable.min(1) as i64);
    let gap = need - usable as i64;
    // Спрос (деньги клиентов) — только при включённом биллинге.
    let (bal, res, spent, keys) = match &app.billing {
        Some(b) => {
            let t = b.totals().await?;
            (
                t.balance_nano as f64 / 1e9,
                t.reserved_nano as f64 / 1e9,
                t.spent_nano as f64 / 1e9,
                t.active_accounts,
            )
        }
        None => (0.0, 0.0, 0.0, 0),
    };
    let real_demand = if REF_MULT > 0.0 { bal / REF_MULT } else { 0.0 }; // потенц. real-API из балансов
    let coverage7 = if a7d > 0.01 { real_demand / a7d } else { 0.0 }; // >1 = потенциально перепродали
    let jinf = |x: f64| {
        if x.is_finite() {
            json!(r2(x))
        } else {
            json!(null)
        }
    }; // null = ∞ (нет спроса)
    Ok(json!({
        "now": pool::now(),
        "subs": n, "calibrated": all_cal, "ref_mult": REF_MULT, "target_headroom": TARGET_HEADROOM,
        "supply": {
            "avail_usd":    {"1h": r2(a1), "5h": r2(a5), "1d": r2(a1d), "7d": r2(a7d)},
            "cap_usd":      {"5h": r2(cap5), "7d": r2(cap7)},
            "consumed_usd": {"5h": r2(cons5), "7d": r2(cons7)},
            "util":         {"5h": r2(if usable>0 {u5/usable as f64} else {0.0}), "7d": r2(if usable>0 {u7/usable as f64} else {0.0})},
            "health":       {"healthy": caps.iter().filter(|c| c.routable && c.auth_state == "healthy").count(),
                               "suspect": suspect, "cooling": cooling, "dead": dead,
                               "usable": usable, "total": n},
        },
        "demand": {"balance_usd": r2(bal), "reserved_usd": r2(res), "spent_usd": r2(spent), "active_accounts": keys,
                   "potential_realapi_usd": r2(real_demand)},
        "headroom": {"5h": jinf(head5), "7d": jinf(head7)},
        "coverage": {"7d": r2(coverage7)},
        "recommend": {"subs_needed": need, "gap": gap},
    }))
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    if let Some(v) = cache_get(&SUBS_CACHE) {
        return Json(v).into_response();
    }
    let authority = app.authority.as_ref().clone();
    let db = app.data_db_path.as_ref().clone();
    let (rows, maxes) = tokio::task::spawn_blocking(move || {
        let rows = authority
            .connect()
            .and_then(|mut c| c.subs_admin())
            .unwrap_or_default();
        // пиковая ёмкость по подписке за всю дистанцию (durable sub_peaks в metrics.db)
        let mdir = std::path::Path::new(&db)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        let maxes = crate::metrics_store::open(&format!("{mdir}/metrics.db"))
            .and_then(|c| crate::metrics_store::sub_maxes(&c))
            .unwrap_or_default();
        (rows, maxes)
    })
    .await
    .unwrap_or_default();
    let peak: std::collections::HashMap<String, (f64, f64)> = maxes
        .into_iter()
        .map(|(e, m5, m7, _)| (e, (m5, m7)))
        .collect();
    let now = pool::now();
    let lifetime = SUB_LIFETIME_DAYS * 86400;
    let days = |secs: i64| ((secs as f64 / 86400.0) * 10.0).round() / 10.0;
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    let list: Vec<_> = rows
        .iter()
        .map(|s| {
            let head: String = s.email.chars().take(4).collect();
            let sub_expire = if s.added_ts > 0 {
                s.added_ts + lifetime
            } else {
                0
            };
            let (pk5, pk7) = peak.get(&s.email).copied().unwrap_or((0.0, 0.0));
            json!({
                "email": format!("{head}…"),
                "status": s.status, "fleet": s.fleet, "added": s.added, "has_token": s.has_token,
                "sub_expire_ts": sub_expire,
                "sub_days_left": if sub_expire > 0 { days(sub_expire - now) } else { 0.0 },
                "proxy_host": s.proxy_host,
                "proxy_expire": s.proxy_expire,   // из IPRoyal (пусто, пока authbot-loop не заполнил)
                "proxy_ok": s.proxy_ok,
                // Durable auth-health (авторитетно из БД, переживает рестарт) — панель показывает «токен
                // мёртв (бан/re-auth)» здесь, а не только по эфемерному /capacity-флагу.
                "auth_state": s.auth_state,       // "healthy" | "suspect" | "dead"
                "dead_reason": s.dead_reason,     // "authentication_error" (re-auth) | "permission_error" (banned)
                "dead_since_ts": s.dead_since_ts,
                "peak_cap5h_usd": r2(pk5),         // истинный ПИК ёмкости на дистанции (не EMA)
                "peak_cap7d_usd": r2(pk7),
            })
        })
        .collect();
    let dead = rows.iter().filter(|s| s.auth_state == "dead").count();
    let v = json!({"now": now, "lifetime_days": SUB_LIFETIME_DAYS, "dead": dead, "subs": list});
    cache_put(&SUBS_CACHE, &v);
    Json(v).into_response()
}

async fn health() -> Json<serde_json::Value> {
    // Минимум без авторизации: голый liveness-пинг. Размер пула / upstream / статус биллинга —
    // раскрытие backend (у api.anthropic.com нет /health, N подписок = фингерпринт) → только на
    // авторизованных /pool и /metrics.
    Json(json!({ "ok": true }))
}

fn readiness_snapshot(
    accepting: &AtomicBool,
    authority_ready: &AtomicBool,
) -> (StatusCode, serde_json::Value) {
    if accepting.load(Ordering::Acquire) && authority_ready.load(Ordering::Acquire) {
        (StatusCode::OK, json!({ "ready": true }))
    } else {
        let reason = if !accepting.load(Ordering::Acquire) {
            "draining"
        } else {
            "authority_unavailable"
        };
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
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "billing disabled"})),
            )
                .into_response()
        }
    };
    let key = match client_key(&headers) {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "no api key"})),
            )
                .into_response()
        }
    };
    // ключ → аккаунт → баланс аккаунта (общий на все ключи юзера) + расход именно этого ключа
    let krow = match billing.get(&key).await {
        Ok(row) => row,
        Err(error) => {
            eprintln!("billing key lookup failed: {error:#}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response();
        }
    };
    let acct = match billing.key_auth(&key).await {
        Ok(Some(a)) if a.active_at(pool::now()) => match billing.account(&a.account_id).await {
            Ok(account) => account,
            Err(error) => {
                eprintln!("billing account lookup failed: {error:#}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": "billing authority unavailable"})),
                )
                    .into_response();
            }
        },
        Ok(None) | Ok(Some(_)) => None,
        Err(error) => {
            eprintln!("billing authorization lookup failed: {error:#}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response();
        }
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
        }))
        .into_response(),
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
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let now = pool::now();
    let list: Vec<_> = app
        .pool
        .snapshot()
        .into_iter()
        .map(|(s, l)| {
            json!({
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
            })
        })
        .collect();
    Json(json!({"pool": list, "cap": app.cfg.util_cap, "poller": app.cfg.poll})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    fn admin_auth_test_app() -> AppState {
        let mut cfg = crate::config::Settings::from_env().proxy;
        cfg.api_keys = vec!["admin-key".to_string()];
        cfg.control_keys = vec!["control-key".to_string()];
        cfg.panel_keys = vec!["panel-key".to_string()];
        cfg.trust_loopback = false;

        let clients = Arc::new(forward::Clients::new(&cfg));
        AppState {
            cfg: Arc::new(cfg),
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                ":memory:".to_string(),
                None,
            )),
            data_db_path: Arc::new(":memory:".to_string()),
            pool: Arc::new(pool::Pool::new(
                Vec::new(),
                pool::Reserve::new(0.1, 0.03, 0.02),
                0.0,
                0.0,
            )),
            affinity: Arc::new(forward::AffinityStore::new(None, None, 3_600, 300, 35).unwrap()),
            clients,
            codex: None,
            billing: None,
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(forward::Breaker::new(0)),
            metrics: Arc::new(Metrics::new()),
            key_limiter: Arc::new(forward::KeyLimiter::new()),
            concurrency: Arc::new(tokio::sync::Semaphore::new(16)),
            probe_poke: None,
        }
    }

    #[tokio::test]
    async fn every_admin_route_enforces_the_control_key_lattice() {
        assert_eq!(ADMIN_ROUTE_CASES.len(), 13);
        let service = router(admin_auth_test_app(), Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

        for (method, path) in ADMIN_ROUTE_CASES {
            for (credential, expect_unauthorized) in [
                (None, true),
                (Some("panel-key"), true),
                (Some("control-key"), false),
                (Some("admin-key"), false),
            ] {
                let mut request = Request::builder()
                    .method(method.clone())
                    .uri(*path)
                    .body(Body::empty())
                    .unwrap();
                request.extensions_mut().insert(peer);
                if let Some(key) = credential {
                    request
                        .headers_mut()
                        .insert("x-api-key", key.parse().unwrap());
                }

                let status = service.clone().oneshot(request).await.unwrap().status();
                if expect_unauthorized {
                    assert_eq!(
                        status,
                        StatusCode::UNAUTHORIZED,
                        "{method} {path} accepted credential {credential:?}"
                    );
                } else {
                    assert_ne!(
                        status,
                        StatusCode::UNAUTHORIZED,
                        "{method} {path} rejected credential {credential:?}"
                    );
                }
            }
        }
    }

    fn capacity(email: &str, available: f64, routable: bool, calibrated: bool) -> pool::Cap {
        pool::Cap {
            email: email.to_string(),
            calibrated,
            util5h: 0.0,
            util7d: 0.0,
            reset5h_in: 0,
            reset7d_in: 0,
            cap5h_usd: available,
            cap7d_usd: available,
            rem5h_usd: available,
            rem7d_usd: available,
            avail_1h_usd: available,
            avail_5h_usd: available,
            avail_1d_usd: available,
            avail_7d_usd: available,
            status: String::new(),
            cooling: !routable,
            routable,
            auth_dead: false,
            auth_state: "healthy".to_string(),
            dead_reason: String::new(),
            dead_since_ts: 0,
        }
    }

    #[test]
    fn capacity_aggregate_excludes_unroutable_supply() {
        let caps = vec![
            capacity("healthy", 12.0, true, true),
            capacity("cooling", 50.0, false, true),
            capacity("uncalibrated", 3.0, true, false),
        ];
        assert_eq!(routable_capacity_summary(&caps), ([15.0; 4], false));
        assert_eq!(
            routable_capacity_summary(&[capacity("dead", 50.0, false, true)]),
            ([0.0; 4], false)
        );
    }

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
    fn api_plane_is_hostname_selected_and_auth_header_agnostic() {
        let mut headers = HeaderMap::new();
        assert!(!is_openai_plane(&headers));

        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer customer-key".parse().unwrap(),
        );
        assert!(!is_openai_plane(&headers));

        headers.insert("x-api-key", "customer-key".parse().unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("anthropic-beta", "test-feature".parse().unwrap());
        assert!(!is_openai_plane(&headers));

        headers.insert(API_PLANE_HEADER, "anthropic".parse().unwrap());
        assert!(!is_openai_plane(&headers));

        headers.insert(API_PLANE_HEADER, "openai".parse().unwrap());
        assert!(is_openai_plane(&headers));
    }

    #[test]
    fn unsupported_openai_subroutes_use_generic_openai_error_shape() {
        let error = unsupported_openai_endpoint_error();
        assert_eq!(error["error"]["type"], "invalid_request_error");
        assert_eq!(
            error["error"]["message"],
            "The requested endpoint is not supported."
        );
        let serialized = error.to_string();
        assert!(!serialized.contains("Codex"));
        assert!(!serialized.contains("app-server"));
        assert!(!serialized.contains("ChatGPT"));
        assert!(!serialized.contains("Anthropic"));
    }

    #[test]
    fn embedded_admin_panel_exposes_all_operational_workflows_without_secrets() {
        assert!(ADMIN_PANEL_HTML.contains("data-admin-panel-version=\"9\""));
        // The spend breakdown must separate the Claude fleet from the Codex pool: both settle into
        // the same money tables, so an unattributed total hides which upstream earned it.
        assert!(ADMIN_PANEL_HTML.contains("period.providers"));
        assert!(ADMIN_PANEL_HTML.contains("OpenAI (Codex)"));
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
            assert!(
                ADMIN_PANEL_HTML.contains(route),
                "missing admin workflow route {route}"
            );
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
            assert!(
                ADMIN_PANEL_HTML.contains(legacy_capability),
                "missing migrated legacy capability {legacy_capability}"
            );
        }
        assert!(!ADMIN_PANEL_HTML.contains("COMMERCIAL_ADMIN_KEY"));
        assert!(!ADMIN_PANEL_HTML.contains("CONTROL_KEY"));
        assert!(!ADMIN_PANEL_HTML.contains("SALES_ADMIN_KEY"));
        assert!(ADMIN_PANEL_HTML.contains("sessionStorage.setItem(pendingKey"));
        assert!(ADMIN_PANEL_HTML.contains("idempotency_key:idempotencyKey"));
        assert!(ADMIN_PANEL_HTML.contains("Пароль (минимум 8)"));
        assert!(ADMIN_PANEL_HTML.contains("minlength=\"8\""));
        assert!(ADMIN_PANEL_HTML.contains("(values.first||'').length<8"));
    }
}
