//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health,/live — жив ли сервер (без авторизации)
//!   GET /ready        — принимает ли сервер новый трафик (без авторизации)
//!   GET /pool         — статус пула (util/cooling, без секретов)
//!   *             — форвардинг на api.anthropic.com (см. forward::forward)

use crate::admin;
use axum::extract::{ConnectInfo, FromRef, Path, Query, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use forward::{
    authed, client_keys, control_authed, forward, gemini_api, openai_chat_completions,
    openai_delete_response, openai_get_response, openai_input_tokens, openai_model, openai_models,
    openai_response_input_items, openai_responses, readonly_authed, resolve_client_key,
    resolve_client_keys, AppState, Metrics, PricingBridgeFallbackReason,
    PricingShadowEnqueueResult, PricingShadowProcessingResult, TerminalErrorReason,
    PRICING_BRIDGE_LATENCY_BUCKETS_MS, PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS,
};
use registry::pricing::{
    PricingMode, PricingShadowComparison, PricingShadowReadErrorCode, PricingShadowRejectionCode,
    SnapshotProvider,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// One-release migration marker used only by the `Combined` bridge. Fixed provider routers never
/// inspect it, and Caddy strips/overwrites client input while the bridge is present.
const API_PLANE_HEADER: &str = "x-apitoken-api-plane";
const OPENAI_API_PLANE: &[u8] = b"openai";

/// TTL-кэш дашборд-эндпоинтов: панель поллит /overview и /capacity, а они делают O(n)-скан
/// (capacity() под пул-локом + billing_totals full-scan). Кэш на 2с → при N поллерах пересчёт
/// максимум раз в 2с, не крадём hot-path лок у боевых запросов. Свежесть суб-секунд дашборду не нужна.
const DASH_TTL: std::time::Duration = std::time::Duration::from_secs(2);
type DashCache =
    std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>;
static OVERVIEW_CACHE: DashCache = std::sync::OnceLock::new();
/// /spend-stats сканирует usage_events за 30 дней — TTL длиннее дашбордного. В кэше только
/// стандартные окна (d1/d7/d30): произвольный диапазон ?from&to считается на каждый запрос
/// мимо кэша, чтобы ответы с разными границами не смешивались.
const SPEND_TTL: std::time::Duration = std::time::Duration::from_secs(30);
static SPEND_CACHE: DashCache = std::sync::OnceLock::new();
/// Порог backlog для /settlement-health: несеттленая строка outbox старше 5 минут — уже аномалия
/// (штатный settle синхронен, ретраи идут с backoff в секундах).
const SETTLEMENT_BACKLOG_SECS: i64 = 300;
static SETTLEMENT_CACHE: DashCache = std::sync::OnceLock::new();
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
        post,
        POST,
        "/admin/accounts/query",
        "/admin/accounts/query",
        admin::query_accounts
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
    (
        post,
        POST,
        "/admin/pricing/catalog/prepare",
        "/admin/pricing/catalog/prepare",
        admin::prepare_pricing_catalog
    ),
    (
        get,
        GET,
        "/admin/pricing/catalog/{product_id}/active",
        "/admin/pricing/catalog/main/active",
        admin::active_pricing_catalog
    ),
    (
        get,
        GET,
        "/admin/pricing/catalog/{product_id}/version/{generation}",
        "/admin/pricing/catalog/main/version/1",
        admin::pricing_catalog_version
    ),
    (
        post,
        POST,
        "/admin/pricing/catalog/{product_id}/activate",
        "/admin/pricing/catalog/main/activate",
        admin::activate_pricing_catalog
    ),
    (
        post,
        POST,
        "/admin/pricing/switches/prepare",
        "/admin/pricing/switches/prepare",
        admin::prepare_provider_switches
    ),
    (
        get,
        GET,
        "/admin/pricing/switches/active",
        "/admin/pricing/switches/active",
        admin::active_provider_switches
    ),
    (
        get,
        GET,
        "/admin/pricing/switches/version/{generation}",
        "/admin/pricing/switches/version/1",
        admin::provider_switches_version
    ),
    (
        post,
        POST,
        "/admin/pricing/switches/activate",
        "/admin/pricing/switches/activate",
        admin::activate_provider_switches
    ),
    (
        post,
        POST,
        "/admin/pricing/policy/prepare",
        "/admin/pricing/policy/prepare",
        admin::prepare_account_policy
    ),
    (
        get,
        GET,
        "/admin/pricing/policy/{account_id}/active",
        "/admin/pricing/policy/test-account/active",
        admin::active_account_policy
    ),
    (
        get,
        GET,
        "/admin/pricing/policy/{account_id}/state",
        "/admin/pricing/policy/test-account/state",
        admin::account_policy_state
    ),
    (
        get,
        GET,
        "/admin/pricing/policy/{account_id}/version/{effective_version}",
        "/admin/pricing/policy/test-account/version/1",
        admin::account_policy_version
    ),
    (
        post,
        POST,
        "/admin/pricing/policy/{account_id}/activate",
        "/admin/pricing/policy/test-account/activate",
        admin::activate_account_policy
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
    let provider = app.provider;
    let state = HttpState { app, accepting };
    let common = Router::new()
        .route("/health", get(health))
        .route("/live", get(health))
        .route("/ready", get(ready))
        .route("/balance", get(balance))
        .route("/metrics", get(metrics));
    let router = match provider {
        forward::ProviderMode::Combined => common
            .route("/pool", get(pool_status))
            .route("/capacity", get(capacity))
            .route("/overview", get(overview))
            .route("/spend-stats", get(spend_stats))
            .route("/settlement-health", get(settlement_health))
            .route("/subs", get(subs))
            .route("/fleet-history", get(fleet_history))
            .route("/codex-subs", get(codex_subs))
            .merge(admin)
            // Migration bridge: existing Caddy marks the OpenAI hostname until provider-specific
            // services are installed. Fixed provider modes below never inspect this header.
            .route("/v1/responses", post(responses_dispatch))
            .route("/v1/responses/input_tokens", post(input_tokens_dispatch))
            .route(
                "/v1/responses/{response_id}",
                get(get_response_dispatch).delete(delete_response_dispatch),
            )
            .route(
                "/v1/responses/{response_id}/input_items",
                get(response_input_items_dispatch),
            )
            .route("/v1/chat/completions", post(chat_completions_dispatch))
            .route("/v1/models", get(models_dispatch))
            .route("/v1/models/{model_id}", get(model_dispatch))
            .fallback(provider_fallback_dispatch)
            .method_not_allowed_fallback(method_not_allowed_dispatch),
        forward::ProviderMode::Anthropic => common
            .route("/pool", get(pool_status))
            .route("/capacity", get(capacity))
            .route("/overview", get(overview))
            .route("/spend-stats", get(spend_stats))
            .route("/settlement-health", get(settlement_health))
            .route("/subs", get(subs))
            .route("/fleet-history", get(fleet_history))
            .route("/codex-subs", get(codex_subs))
            .merge(admin)
            .fallback(forward),
        forward::ProviderMode::OpenAi => common
            .route("/codex-subs", get(codex_subs))
            .route("/v1/responses", post(openai_responses))
            .route("/v1/responses/input_tokens", post(openai_input_tokens))
            .route(
                "/v1/responses/{response_id}",
                get(openai_get_response).delete(openai_delete_response),
            )
            .route(
                "/v1/responses/{response_id}/input_items",
                get(openai_response_input_items),
            )
            .route("/v1/chat/completions", post(openai_chat_completions))
            .route("/v1/models", get(openai_models))
            .route("/v1/models/{model_id}", get(openai_model))
            .fallback(fixed_openai_not_found)
            .method_not_allowed_fallback(fixed_openai_not_found),
        forward::ProviderMode::Gemini => common
            .route("/gemini-subs", get(gemini_subs))
            .fallback(gemini_api)
            .method_not_allowed_fallback(gemini_api),
    };
    router
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, audit_customer_error))
}

async fn fixed_openai_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(unsupported_openai_endpoint_error()),
    )
        .into_response()
}

/// One privacy-safe event for the terminal HTTP error actually returned to a metered customer.
/// Internal retries are invisible here, and successful requests do not pay the extra key lookup.
async fn audit_customer_error(
    State(state): State<HttpState>,
    request: Request,
    next: Next,
) -> Response {
    let keys = client_keys(request.headers());
    let method = request.method().clone();
    let uri = request.uri().clone();
    let response = next.run(request).await;
    if response.status().is_success() || !uri.path().starts_with("/v1/") {
        return response;
    }
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(forward::fresh_request_id);
    let retry_after_seconds = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let Some(billing) = &state.app.billing else {
        return response;
    };
    let Ok(Some((key, _))) = resolve_client_keys(billing, &keys).await else {
        return response;
    };
    let Ok(Some(key_row)) = billing.get(&key).await else {
        return response;
    };
    let Some(account_id) = key_row.account_id.as_deref() else {
        return response;
    };
    let account = billing.account(account_id).await.ok().flatten();
    let mut reason = response
        .extensions()
        .get::<TerminalErrorReason>()
        .map(|reason| reason.0)
        .unwrap_or_else(|| upstream_status_reason(response.status()));
    if reason == "billing_limit" {
        reason = billing_limit_reason(account.as_ref(), &key_row);
    }
    let event = customer_error_event(
        response.status(),
        reason,
        account_id,
        &key_row.key_id,
        &method,
        &uri,
        &request_id,
        retry_after_seconds,
        account.as_ref(),
        &key_row,
    );
    eprintln!("{event}");
    response
}

fn upstream_status_reason(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400..=499 => "upstream_client_error",
        500..=599 => "upstream_server_error",
        _ => "non_success_response",
    }
}

fn billing_limit_reason(
    account: Option<&registry::AccountRow>,
    key: &registry::KeyRow,
) -> &'static str {
    match (account, key.spend_limit_nano) {
        (Some(account), Some(limit)) => {
            let remaining = limit
                .saturating_sub(key.spent_nano)
                .saturating_sub(key.reserved_nano)
                .max(0);
            if remaining < account.balance_nano {
                "key_spend_limit"
            } else if account.balance_nano < remaining {
                "account_balance"
            } else {
                "account_and_key_limit"
            }
        }
        (Some(_), None) => "account_balance",
        (None, _) => "billing_limit",
    }
}

fn customer_error_event(
    status: StatusCode,
    reason: &'static str,
    account_id: &str,
    key_id: &str,
    method: &Method,
    uri: &Uri,
    request_id: &str,
    retry_after_seconds: Option<u64>,
    account: Option<&registry::AccountRow>,
    key: &registry::KeyRow,
) -> String {
    json!({
        "event": "customer_http_error",
        "status": status.as_u16(),
        "reason": reason,
        "account_id": account_id,
        "key_id": key_id,
        "method": method.as_str(),
        "path": audit_path(uri.path()),
        "request_id": request_id,
        "retry_after_seconds": retry_after_seconds,
        "account_balance_nano": account.map(|row| row.balance_nano),
        "account_reserved_nano": account.map(|row| row.reserved_nano),
        "key_spent_nano": key.spent_nano,
        "key_reserved_nano": key.reserved_nano,
        "key_spend_limit_nano": key.spend_limit_nano,
        "key_expires_ts": key.expires_ts,
        "account_status": account.map(|row| row.status.as_str()),
        "key_status": key.status,
    })
    .to_string()
}

fn audit_path(path: &str) -> &'static str {
    match path {
        "/v1/messages" => "/v1/messages",
        "/v1/messages/count_tokens" => "/v1/messages/count_tokens",
        "/v1/models" => "/v1/models",
        "/v1/responses" => "/v1/responses",
        "/v1/responses/input_tokens" => "/v1/responses/input_tokens",
        "/v1/chat/completions" => "/v1/chat/completions",
        _ if path.starts_with("/v1/responses/") && path.ends_with("/input_items") => {
            "/v1/responses/{id}/input_items"
        }
        _ if path.starts_with("/v1/responses/") => "/v1/responses/{id}",
        _ if path.starts_with("/v1/models/") => "/v1/models/{id}",
        _ => "/v1/{unsupported}",
    }
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

async fn get_response_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(response_id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_get_response(
        State(app),
        ConnectInfo(peer),
        request.headers().clone(),
        Path(response_id),
    )
    .await
}

async fn delete_response_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(response_id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_delete_response(
        State(app),
        ConnectInfo(peer),
        request.headers().clone(),
        Path(response_id),
    )
    .await
}

async fn response_input_items_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(response_id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_response_input_items(
        State(app),
        ConnectInfo(peer),
        request.headers().clone(),
        Path(response_id),
    )
    .await
}

async fn input_tokens_dispatch(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if !is_openai_plane(request.headers()) {
        return forward(State(app), ConnectInfo(peer), request).await;
    }
    openai_input_tokens(State(app), ConnectInfo(peer), request).await
}

/// A method mismatch on a known path is an unknown-endpoint situation for the caller, so the
/// OpenAI plane answers with the same OpenAI-shaped 404 as unsupported routes instead of
/// axum's bare 405. The Claude plane keeps its legacy 405 semantics.
async fn method_not_allowed_dispatch(
    State(app): State<AppState>,
    request: axum::extract::Request,
) -> Response {
    if is_openai_plane(request.headers()) {
        return (
            StatusCode::NOT_FOUND,
            Json(unsupported_openai_endpoint_error()),
        )
            .into_response();
    }
    let _ = app;
    StatusCode::METHOD_NOT_ALLOWED.into_response()
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
         # TYPE claude_api_route_rebind_total counter\nclaude_api_route_rebind_total {}\n\
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
        rs.rebind,
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
        "# TYPE claude_api_gemini_usage_metadata_missing_total counter\n\
         claude_api_gemini_usage_metadata_missing_total {}",
        g(&m.gemini_usage_missing)
    );
    let _ = writeln!(
        body,
        "# TYPE claude_api_gemini_transport_failures_total counter\n\
         claude_api_gemini_transport_failures_total {}\n\
         # TYPE claude_api_gemini_backend_failures_total counter\n\
         claude_api_gemini_backend_failures_total {}\n\
         # TYPE claude_api_gemini_malformed_responses_total counter\n\
         claude_api_gemini_malformed_responses_total {}\n\
         # TYPE claude_api_gemini_stream_start_failures_total counter\n\
         claude_api_gemini_stream_start_failures_total {}",
        g(&m.gemini_transport_failures),
        g(&m.gemini_backend_failures),
        g(&m.gemini_malformed_responses),
        g(&m.gemini_stream_start_failures),
    );
    let _ = writeln!(
        body,
        "# TYPE claude_api_pricing_bridge_selected_total counter\n\
         # TYPE claude_api_pricing_bridge_snapshot_inserted_total counter\n\
         # TYPE claude_api_pricing_bridge_snapshot_replayed_total counter\n\
         # TYPE claude_api_pricing_bridge_not_reserved_total counter\n\
         # TYPE claude_api_pricing_bridge_failure_total counter\n\
         # TYPE claude_api_pricing_bridge_conflict_total counter\n\
         # TYPE claude_api_pricing_bridge_fallback_total counter\n\
         # TYPE claude_api_pricing_bridge_atomic_reserve_duration_seconds histogram"
    );
    for provider in [SnapshotProvider::Anthropic, SnapshotProvider::OpenAi] {
        let provider_id = provider.as_str();
        let _ = writeln!(
            body,
            "claude_api_pricing_bridge_selected_total{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_snapshot_inserted_total{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_snapshot_replayed_total{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_not_reserved_total{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_failure_total{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_conflict_total{{provider=\"{provider_id}\"}} {}",
            m.pricing_bridge_selected_count(provider),
            m.pricing_bridge_inserted_count(provider),
            m.pricing_bridge_unchanged_count(provider),
            m.pricing_bridge_not_reserved_count(provider),
            m.pricing_bridge_failure_count(provider),
            m.pricing_bridge_conflict_count(provider),
        );
        for reason in [
            PricingBridgeFallbackReason::BridgeDisabled,
            PricingBridgeFallbackReason::NotSampled,
            PricingBridgeFallbackReason::UnsupportedModelIdentity,
            PricingBridgeFallbackReason::UnsupportedModifier,
            PricingBridgeFallbackReason::SnapshotIdentityOversized,
            PricingBridgeFallbackReason::OfficialHoldOutOfRange,
        ] {
            let _ = writeln!(
                body,
                "claude_api_pricing_bridge_fallback_total{{provider=\"{provider_id}\",reason=\"{}\"}} {}",
                reason.code(),
                m.pricing_bridge_fallback_count(provider, reason),
            );
        }
        for (bucket_index, upper_ms) in PRICING_BRIDGE_LATENCY_BUCKETS_MS.iter().enumerate() {
            let _ = writeln!(
                body,
                "claude_api_pricing_bridge_atomic_reserve_duration_seconds_bucket{{provider=\"{provider_id}\",le=\"{}\"}} {}",
                *upper_ms as f64 / 1_000.0,
                m.pricing_bridge_latency_bucket_count(provider, bucket_index),
            );
        }
        let latency_count = m.pricing_bridge_latency_count(provider);
        let _ = writeln!(
            body,
            "claude_api_pricing_bridge_atomic_reserve_duration_seconds_bucket{{provider=\"{provider_id}\",le=\"+Inf\"}} {latency_count}\n\
             claude_api_pricing_bridge_atomic_reserve_duration_seconds_sum{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_bridge_atomic_reserve_duration_seconds_count{{provider=\"{provider_id}\"}} {latency_count}",
            m.pricing_bridge_latency_sum_seconds(provider),
        );
    }
    let shadow = app.pricing_shadow.as_ref();
    let _ = writeln!(
        body,
        "# TYPE claude_api_pricing_shadow_enabled gauge\n\
         claude_api_pricing_shadow_enabled {}\n\
         # TYPE claude_api_pricing_shadow_sample_basis_points gauge\n\
         claude_api_pricing_shadow_sample_basis_points {}\n\
         # TYPE claude_api_pricing_shadow_queue_capacity gauge\n\
         claude_api_pricing_shadow_queue_capacity {}\n\
         # TYPE claude_api_pricing_shadow_worker_concurrency gauge\n\
         claude_api_pricing_shadow_worker_concurrency {}\n\
         # TYPE claude_api_pricing_shadow_timeout_milliseconds gauge\n\
         claude_api_pricing_shadow_timeout_milliseconds {}\n\
         # TYPE claude_api_pricing_shadow_max_queue_age_seconds gauge\n\
         claude_api_pricing_shadow_max_queue_age_seconds {}\n\
         # TYPE claude_api_pricing_shadow_max_field_bytes gauge\n\
         claude_api_pricing_shadow_max_field_bytes {}\n\
         # TYPE claude_api_pricing_shadow_max_item_bytes gauge\n\
         claude_api_pricing_shadow_max_item_bytes {}\n\
         # TYPE claude_api_pricing_shadow_rate_per_second gauge\n\
         claude_api_pricing_shadow_rate_per_second {}\n\
         # TYPE claude_api_pricing_shadow_rate_burst gauge\n\
         claude_api_pricing_shadow_rate_burst {}\n\
         # TYPE claude_api_pricing_shadow_db_read_connections gauge\n\
         claude_api_pricing_shadow_db_read_connections {}\n\
         # TYPE claude_api_pricing_shadow_queue_depth gauge\n\
         claude_api_pricing_shadow_queue_depth {}\n\
         # TYPE claude_api_pricing_shadow_queue_high_water gauge\n\
         claude_api_pricing_shadow_queue_high_water {}\n\
         # TYPE claude_api_pricing_shadow_enqueue_total counter\n\
         # TYPE claude_api_pricing_shadow_processing_total counter\n\
         # TYPE claude_api_pricing_shadow_rejected_total counter\n\
         # TYPE claude_api_pricing_shadow_read_error_total counter\n\
         # TYPE claude_api_pricing_shadow_resolved_total counter\n\
         # TYPE claude_api_pricing_shadow_queue_age_seconds histogram",
        u8::from(shadow.is_some_and(|runtime| runtime.config().enabled())),
        shadow.map_or(0, |runtime| runtime.config().sample_bp()),
        shadow.map_or(0, |runtime| runtime.config().queue_capacity()),
        shadow.map_or(0, |runtime| runtime.config().worker_concurrency()),
        shadow.map_or(0, |runtime| runtime.config().timeout_ms()),
        shadow.map_or(0, |runtime| runtime.config().max_queue_age_secs()),
        shadow.map_or(0, |runtime| runtime.config().max_field_bytes()),
        shadow.map_or(0, |runtime| runtime.config().max_item_bytes()),
        shadow.map_or(0, |runtime| runtime.config().rate_per_sec()),
        shadow.map_or(0, |runtime| runtime.config().rate_burst()),
        shadow.map_or(0, |runtime| runtime.config().db_read_connections()),
        m.pricing_shadow_queue_depth(),
        m.pricing_shadow_queue_high_water(),
    );
    if let Some(runtime) = shadow {
        let _ = writeln!(
            body,
            "# TYPE claude_api_pricing_shadow_runtime_manifest_info gauge\n\
             claude_api_pricing_shadow_runtime_manifest_info{{generation=\"{}\",digest=\"{}\"}} 1",
            runtime.manifest().manifest_generation(),
            runtime.manifest().manifest_digest(),
        );
    }
    for provider in [SnapshotProvider::Anthropic, SnapshotProvider::OpenAi] {
        let provider_id = provider.as_str();
        for result in PricingShadowEnqueueResult::ALL {
            let _ = writeln!(
                body,
                "claude_api_pricing_shadow_enqueue_total{{provider=\"{provider_id}\",result=\"{}\"}} {}",
                result.code(),
                m.pricing_shadow_enqueue_count(provider, result),
            );
        }
        for result in PricingShadowProcessingResult::ALL {
            let _ = writeln!(
                body,
                "claude_api_pricing_shadow_processing_total{{provider=\"{provider_id}\",result=\"{}\"}} {}",
                result.code(),
                m.pricing_shadow_processing_count(provider, result),
            );
        }
        for reason in PricingShadowRejectionCode::ALL {
            let _ = writeln!(
                body,
                "claude_api_pricing_shadow_rejected_total{{provider=\"{provider_id}\",reason=\"{}\"}} {}",
                reason.as_str(),
                m.pricing_shadow_rejection_count(provider, *reason),
            );
        }
        for reason in PricingShadowReadErrorCode::ALL {
            let _ = writeln!(
                body,
                "claude_api_pricing_shadow_read_error_total{{provider=\"{provider_id}\",reason=\"{}\"}} {}",
                reason.as_str(),
                m.pricing_shadow_read_error_count(provider, *reason),
            );
        }
        for mode in [PricingMode::Track, PricingMode::Discount] {
            let mode_id = match mode {
                PricingMode::Track => "track",
                PricingMode::Discount => "discount",
            };
            for (model_scope, scope_id) in [(false, "provider"), (true, "model")] {
                for comparison in [
                    PricingShadowComparison::Equal,
                    PricingShadowComparison::Different,
                ] {
                    let _ = writeln!(
                        body,
                        "claude_api_pricing_shadow_resolved_total{{provider=\"{provider_id}\",mode=\"{mode_id}\",scope=\"{scope_id}\",comparison=\"{}\"}} {}",
                        comparison.as_str(),
                        m.pricing_shadow_resolved_count(
                            provider,
                            mode,
                            model_scope,
                            comparison,
                        ),
                    );
                }
            }
        }
        for (bucket, upper) in PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS.iter().enumerate() {
            let _ = writeln!(
                body,
                "claude_api_pricing_shadow_queue_age_seconds_bucket{{provider=\"{provider_id}\",le=\"{upper}\"}} {}",
                m.pricing_shadow_queue_age_bucket_count(provider, bucket),
            );
        }
        let count = m.pricing_shadow_queue_age_count(provider);
        let _ = writeln!(
            body,
            "claude_api_pricing_shadow_queue_age_seconds_bucket{{provider=\"{provider_id}\",le=\"+Inf\"}} {count}\n\
             claude_api_pricing_shadow_queue_age_seconds_sum{{provider=\"{provider_id}\"}} {}\n\
             claude_api_pricing_shadow_queue_age_seconds_count{{provider=\"{provider_id}\"}} {count}",
            m.pricing_shadow_queue_age_sum_seconds(provider),
        );
    }
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
    let limits = codex_status
        .as_ref()
        .and_then(|status| status.rate_limits.clone());
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
             # TYPE claude_api_codex_home_rate_limit_used_percent gauge\n\
             # TYPE claude_api_codex_home_limit_reached gauge\n\
             # TYPE claude_api_codex_home_spend_usd_total gauge\n\
             # TYPE claude_api_codex_home_calibration_persistence_ok gauge\n\
             # TYPE claude_api_codex_home_window_capacity_usd gauge\n\
             # TYPE claude_api_codex_home_window_remaining_usd gauge\n\
             # TYPE claude_api_codex_home_window_capacity_low_usd gauge\n\
             # TYPE claude_api_codex_home_window_capacity_high_usd gauge\n\
             # TYPE claude_api_codex_home_window_confidence_ratio gauge\n\
             # TYPE claude_api_codex_home_window_data_age_seconds gauge\n\
             # TYPE claude_api_codex_home_window_estimate_available gauge\n\
             # TYPE claude_api_codex_home_window_samples gauge\n\
             # TYPE claude_api_codex_home_admitted gauge\n\
             # TYPE claude_api_codex_home_account_dead gauge\n\
             # TYPE claude_api_codex_home_account_suspect gauge\n\
             # TYPE claude_api_codex_home_transport_degraded gauge\n\
             # TYPE claude_api_codex_home_transport_wedged gauge\n\
             # TYPE claude_api_codex_home_snapshot_age_seconds gauge\n\
             # TYPE claude_api_codex_home_ready_published gauge"
        );
        for home in &status.homes {
            let index = &home.id;
            let _ = writeln!(
                body,
                "claude_api_codex_home_process_live{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_authenticated{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_cooling_until_seconds{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_inflight_turns{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_limit_reached{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_spend_usd_total{{home=\"{index}\"}} {:.4}\n\
                 claude_api_codex_home_calibration_persistence_ok{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_admitted{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_account_dead{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_account_suspect{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_transport_degraded{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_transport_wedged{{home=\"{index}\"}} {}\n\
                 claude_api_codex_home_ready_published{{home=\"{index}\"}} {}",
                u8::from(home.process_live),
                u8::from(home.auth_ok),
                home.cooling_until,
                home.inflight,
                u8::from(home.limit_reached),
                home.spend_usd_total,
                u8::from(home.calibration_persistence_ok),
                u8::from(home.admitted),
                u8::from(home.account_state == "dead"),
                u8::from(home.account_state == "suspect"),
                u8::from(home.transport_state == "degraded"),
                u8::from(home.transport_state == "wedged"),
                u8::from(home.ready_published),
            );
            // Snapshot age is the only signal that distinguishes "quota evidence says X" from
            // "quota evidence stopped arriving". A frozen snapshot keeps reporting its last value,
            // so without this gauge a broken refresh path is indistinguishable from a healthy one.
            if let Some(age) = home.snapshot_age_secs {
                let _ = writeln!(
                    body,
                    "claude_api_codex_home_snapshot_age_seconds{{home=\"{index}\"}} {age}"
                );
            }
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
            write_codex_home_capacity_metrics(&mut body, home);
        }
        // Pool totals are grouped by actual provider duration, not primary/secondary slot names.
        let _ = writeln!(
            body,
            "# TYPE claude_api_codex_window_capacity_usd gauge\n\
             # TYPE claude_api_codex_window_remaining_usd gauge\n\
             # TYPE claude_api_codex_window_measured_homes gauge\n\
             # TYPE claude_api_codex_window_observed_homes gauge"
        );
        for (duration, (cap_sum, remaining_sum, measured, observed)) in codex_window_totals(status)
        {
            let _ = writeln!(
                body,
                "claude_api_codex_window_measured_homes{{window_minutes=\"{duration}\"}} {measured}\n\
                 claude_api_codex_window_observed_homes{{window_minutes=\"{duration}\"}} {observed}"
            );
            if measured > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_codex_window_capacity_usd{{window_minutes=\"{duration}\"}} {cap_sum:.4}\n\
                     claude_api_codex_window_remaining_usd{{window_minutes=\"{duration}\"}} {remaining_sum:.4}"
                );
            }
        }
    }
    let _ = writeln!(
        body,
        "# TYPE claude_api_gemini_enabled gauge\nclaude_api_gemini_enabled {}",
        u8::from(app.gemini.is_some())
    );
    if let Some(gemini) = &app.gemini {
        let status = gemini.operational_status().await;
        let _ = writeln!(
            body,
            "# TYPE claude_api_gemini_profiles gauge\nclaude_api_gemini_profiles {}\n\
             # TYPE claude_api_gemini_profiles_available gauge\nclaude_api_gemini_profiles_available {}\n\
             # TYPE claude_api_gemini_profiles_authenticated gauge\nclaude_api_gemini_profiles_authenticated {}\n\
             # TYPE claude_api_gemini_model_profiles_available gauge\n\
             # TYPE claude_api_gemini_model_profiles_healthy gauge\n\
             # TYPE claude_api_gemini_model_profiles_degraded gauge\n\
             # TYPE claude_api_gemini_profile_authenticated gauge\n\
             # TYPE claude_api_gemini_profile_cooling_until_seconds gauge\n\
             # TYPE claude_api_gemini_profile_inflight_requests gauge\n\
             # TYPE claude_api_gemini_profile_last_probe_seconds gauge\n\
             # TYPE claude_api_gemini_profile_quota_updated_seconds gauge\n\
             # TYPE claude_api_gemini_profile_spend_usd_total gauge\n\
             # TYPE claude_api_gemini_profile_calibration_persistence_ok gauge\n\
             # TYPE claude_api_gemini_profile_window_remaining_ratio gauge\n\
             # TYPE claude_api_gemini_profile_window_resets_at_seconds gauge\n\
             # TYPE claude_api_gemini_profile_window_data_age_seconds gauge\n\
             # TYPE claude_api_gemini_profile_window_estimate_available gauge\n\
             # TYPE claude_api_gemini_profile_window_capacity_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_remaining_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_capacity_low_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_capacity_high_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_remaining_low_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_remaining_high_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_observed_spend_usd gauge\n\
             # TYPE claude_api_gemini_profile_window_observed_fraction_units gauge\n\
             # TYPE claude_api_gemini_profile_window_confidence_ratio gauge\n\
             # TYPE claude_api_gemini_profile_window_samples gauge\n\
             # TYPE claude_api_gemini_profile_model_cooling_until_seconds gauge\n\
             # TYPE claude_api_gemini_profile_model_failure_streak gauge\n\
             # TYPE claude_api_gemini_profile_model_last_success_seconds gauge\n\
             # TYPE claude_api_gemini_profile_model_last_failure_seconds gauge",
            status.profiles.len(),
            status.available,
            status.authenticated,
        );
        if let Some(ready_at) = status.soonest_ready {
            let _ = writeln!(
                body,
                "# TYPE claude_api_gemini_soonest_ready_seconds gauge\n\
                 claude_api_gemini_soonest_ready_seconds {ready_at}"
            );
        }
        for model in &status.models {
            let _ = writeln!(
                body,
                "claude_api_gemini_model_profiles_available{{model=\"{}\"}} {}\n\
                 claude_api_gemini_model_profiles_healthy{{model=\"{}\"}} {}\n\
                 claude_api_gemini_model_profiles_degraded{{model=\"{}\"}} {}",
                model.id, model.available, model.id, model.healthy, model.id, model.degraded,
            );
        }
        for profile in &status.profiles {
            let id = &profile.id;
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_authenticated{{profile=\"{id}\"}} {}\n\
                 claude_api_gemini_profile_cooling_until_seconds{{profile=\"{id}\"}} {}\n\
                 claude_api_gemini_profile_inflight_requests{{profile=\"{id}\"}} {}\n\
                 claude_api_gemini_profile_last_probe_seconds{{profile=\"{id}\"}} {}\n\
                 claude_api_gemini_profile_quota_updated_seconds{{profile=\"{id}\"}} {}\n\
                 claude_api_gemini_profile_spend_usd_total{{profile=\"{id}\"}} {:.6}\n\
                 claude_api_gemini_profile_calibration_persistence_ok{{profile=\"{id}\"}} {}",
                u8::from(profile.authenticated),
                profile.cooling_until,
                profile.inflight,
                profile.last_probe_at,
                profile.quota_updated_at,
                profile.spend_usd_total,
                u8::from(profile.calibration_persistence_ok),
            );
            write_gemini_profile_capacity_metrics(&mut body, profile);
            for cooling in &profile.model_cooling {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_profile_model_cooling_until_seconds{{profile=\"{id}\",model=\"{}\"}} {}\n\
                     claude_api_gemini_profile_model_failure_streak{{profile=\"{id}\",model=\"{}\"}} {}\n\
                     claude_api_gemini_profile_model_last_success_seconds{{profile=\"{id}\",model=\"{}\"}} {}\n\
                     claude_api_gemini_profile_model_last_failure_seconds{{profile=\"{id}\",model=\"{}\"}} {}",
                    cooling.model_id,
                    cooling.cooling_until,
                    cooling.model_id,
                    cooling.failure_streak,
                    cooling.model_id,
                    cooling.last_success_at,
                    cooling.model_id,
                    cooling.last_failure_at,
                );
            }
        }
        let _ = writeln!(
            body,
            "# TYPE claude_api_gemini_window_capacity_usd gauge\n\
             # TYPE claude_api_gemini_window_remaining_usd gauge\n\
             # TYPE claude_api_gemini_window_capacity_low_usd gauge\n\
             # TYPE claude_api_gemini_window_capacity_high_usd gauge\n\
             # TYPE claude_api_gemini_window_remaining_low_usd gauge\n\
             # TYPE claude_api_gemini_window_remaining_high_usd gauge\n\
             # TYPE claude_api_gemini_window_measured_profiles gauge\n\
             # TYPE claude_api_gemini_window_observed_profiles gauge"
        );
        for (duration, total) in gemini_window_totals(&status) {
            let _ = writeln!(
                body,
                "claude_api_gemini_window_measured_profiles{{window_minutes=\"{duration}\"}} {}\n\
                 claude_api_gemini_window_observed_profiles{{window_minutes=\"{duration}\"}} {}",
                total.measured_profiles, total.observed_profiles,
            );
            if total.measured_profiles > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.cap_usd,
                    total.remaining_usd,
                );
            }
            if total.low_profiles == total.measured_profiles && total.measured_profiles > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_low_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_low_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.low_usd,
                    total.remaining_low_usd,
                );
            }
            if total.high_profiles == total.measured_profiles && total.measured_profiles > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_high_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_high_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.high_usd,
                    total.remaining_high_usd,
                );
            }
        }
    } else {
        let _ = writeln!(
            body,
            "# TYPE claude_api_gemini_profiles gauge\nclaude_api_gemini_profiles 0\n\
             # TYPE claude_api_gemini_profiles_available gauge\nclaude_api_gemini_profiles_available 0\n\
             # TYPE claude_api_gemini_profiles_authenticated gauge\nclaude_api_gemini_profiles_authenticated 0"
        );
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

fn write_codex_home_capacity_metrics(body: &mut String, home: &forward::codex::CodexHomeStatus) {
    use std::fmt::Write as _;

    let index = &home.id;
    for capacity in &home.capacities {
        let slot = capacity.slot;
        let duration = capacity
            .window_minutes
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string());
        let _ = writeln!(
            body,
            "claude_api_codex_home_window_estimate_available{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {}\n\
             claude_api_codex_home_window_confidence_ratio{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.4}\n\
             claude_api_codex_home_window_samples{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {}",
            capacity.source,
            u8::from(capacity.cap_usd.is_some()),
            capacity.confidence,
            capacity.samples,
        );
        if let Some(age) = capacity.data_age_seconds {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_data_age_seconds{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {age}"
            );
        }
        if let (Some(cap_usd), Some(remaining_usd)) = (capacity.cap_usd, capacity.remaining_usd) {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {cap_usd:.4}\n\
                 claude_api_codex_home_window_remaining_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {remaining_usd:.4}",
                capacity.source,
                capacity.source,
            );
        }
        if let Some(low_usd) = capacity.low_usd {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_low_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {low_usd:.4}"
            );
        }
        if let Some(high_usd) = capacity.high_usd {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_high_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {high_usd:.4}"
            );
        }
    }
}

fn write_gemini_profile_capacity_metrics(
    body: &mut String,
    profile: &forward::GeminiProfileStatus,
) {
    use std::fmt::Write as _;

    for capacity in &profile.capacities {
        let id = &profile.id;
        let window = capacity.window_kind;
        let duration = capacity.window_minutes;
        let remaining_ratio = capacity.remaining_fraction_units as f64 / 100_000_000.0;
        let _ = writeln!(
            body,
            "claude_api_gemini_profile_window_remaining_ratio{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {remaining_ratio:.8}\n\
             claude_api_gemini_profile_window_resets_at_seconds{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {}\n\
             claude_api_gemini_profile_window_data_age_seconds{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {}\n\
             claude_api_gemini_profile_window_estimate_available{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\",source=\"{}\"}} {}\n\
             claude_api_gemini_profile_window_confidence_ratio{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {:.4}\n\
             claude_api_gemini_profile_window_samples{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {}\n\
             claude_api_gemini_profile_window_observed_spend_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {:.9}\n\
             claude_api_gemini_profile_window_observed_fraction_units{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {}",
            capacity.resets_at,
            capacity.data_age_seconds,
            capacity.source,
            u8::from(capacity.cap_usd.is_some()),
            capacity.confidence,
            capacity.samples,
            capacity.observed_spend_nano as f64 / 1e9,
            capacity.observed_fraction_units,
        );
        // Unknown capacity has no dollar time series. Publishing a numeric zero before the first
        // complete interval would be indistinguishable from a genuinely measured zero-dollar cap.
        if let (Some(cap), Some(remaining)) = (capacity.cap_usd, capacity.remaining_usd) {
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_window_capacity_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\",source=\"{}\"}} {cap:.6}\n\
                 claude_api_gemini_profile_window_remaining_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\",source=\"{}\"}} {remaining:.6}",
                capacity.source,
                capacity.source,
            );
        }
        if let Some(low) = capacity.low_usd {
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_window_capacity_low_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {low:.6}"
            );
        }
        if let Some(high) = capacity.high_usd {
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_window_capacity_high_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {high:.6}"
            );
        }
        if let Some(low) = capacity.remaining_low_usd {
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_window_remaining_low_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {low:.6}"
            );
        }
        if let Some(high) = capacity.remaining_high_usd {
            let _ = writeln!(
                body,
                "claude_api_gemini_profile_window_remaining_high_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\"}} {high:.6}"
            );
        }
    }
}

/// Sum each real duration once per home. Slot names are presentation metadata, and duplicate slots
/// must never make one subscription look like two copies of the same dollar capacity.
fn codex_window_totals(
    status: &forward::codex::CodexOperationalStatus,
) -> BTreeMap<i64, (f64, f64, usize, usize)> {
    let mut totals: BTreeMap<i64, (f64, f64, usize, usize)> = BTreeMap::new();
    for home in &status.homes {
        let mut seen = BTreeSet::new();
        for capacity in &home.capacities {
            let Some(duration) = capacity.window_minutes else {
                continue;
            };
            if !seen.insert(duration) {
                continue;
            }
            let total = totals.entry(duration).or_default();
            total.3 += 1;
            if let (Some(cap), Some(remaining)) = (capacity.cap_usd, capacity.remaining_usd) {
                total.0 += cap;
                total.1 += remaining;
                total.2 += 1;
            }
        }
    }
    totals
}

#[derive(Default)]
struct GeminiWindowTotal {
    cap_usd: f64,
    remaining_usd: f64,
    low_usd: f64,
    high_usd: f64,
    remaining_low_usd: f64,
    remaining_high_usd: f64,
    measured_profiles: usize,
    observed_profiles: usize,
    low_profiles: usize,
    high_profiles: usize,
}

fn gemini_window_totals(
    status: &forward::GeminiOperationalStatus,
) -> BTreeMap<i64, GeminiWindowTotal> {
    let mut totals: BTreeMap<i64, GeminiWindowTotal> = BTreeMap::new();
    for profile in &status.profiles {
        for capacity in &profile.capacities {
            let total = totals.entry(capacity.window_minutes).or_default();
            total.observed_profiles += 1;
            if let (Some(cap), Some(remaining)) = (capacity.cap_usd, capacity.remaining_usd) {
                total.cap_usd += cap;
                total.remaining_usd += remaining;
                total.measured_profiles += 1;
                if let (Some(low), Some(remaining_low)) =
                    (capacity.low_usd, capacity.remaining_low_usd)
                {
                    total.low_usd += low;
                    total.remaining_low_usd += remaining_low;
                    total.low_profiles += 1;
                }
                if let (Some(high), Some(remaining_high)) =
                    (capacity.high_usd, capacity.remaining_high_usd)
                {
                    total.high_usd += high;
                    total.remaining_high_usd += remaining_high;
                    total.high_profiles += 1;
                }
            }
        }
    }
    totals
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
///
/// Опциональный произвольный диапазон `?from&to` (epoch-секунды, обязательны вместе): ответ
/// дополняется блоком `custom` той же формы, что окно periods, плюс эхо from/to. Окно
/// полуоткрытое [from, to) — стыкующиеся диапазоны не задваивают события; ширина ≤ 92 дней.
/// `to` в будущем зажимается до now+1 (без +1 «по сейчас» теряло бы события текущей секунды);
/// диапазон целиком в будущем, from ≥ to и нечисловой мусор — 400. Блок custom считается на
/// каждый запрос и НЕ кладётся в TTL-кэш (кэш хранит только стандартные окна), поэтому ответы
/// с разными from/to не смешиваются; periods-часть custom-запрос переиспользует из кэша.
async fn spend_stats(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(params): Query<BTreeMap<String, String>>,
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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let custom = match spend_custom_range(&params, now) {
        Ok(range) => range,
        Err(response) => return response,
    };
    let mut v = if let Some(cached) = cache_get_ttl(&SPEND_CACHE, SPEND_TTL) {
        cached
    } else {
        let mut periods = serde_json::Map::new();
        for (key, secs) in [("d1", 86_400i64), ("d7", 7 * 86_400), ("d30", 30 * 86_400)] {
            let window = match spend_window_json(b, now - secs, i64::MAX).await {
                Ok(window) => window,
                Err(response) => return response,
            };
            periods.insert(key.into(), window);
        }
        let built = json!({"now": now, "periods": periods});
        cache_put(&SPEND_CACHE, &built);
        built
    };
    if let Some((from, to)) = custom {
        let mut window = match spend_window_json(b, from, to).await {
            Ok(window) => window,
            Err(response) => return response,
        };
        window["from"] = json!(from);
        window["to"] = json!(to);
        v["custom"] = window;
    }
    Json(v).into_response()
}

/// Разбор query `from`/`to` для /spend-stats. Оба параметра — epoch-секунды и обязательны вместе;
/// без обоих — Ok(None), отвечаем только стандартными окнами. Валидация: from ≥ 0, from < to
/// (после зажатия to до now+1 — см. хендлер), ширина ≤ 92 дней; нарушение — 400 с текстом причины.
fn spend_custom_range(
    params: &BTreeMap<String, String>,
    now: i64,
) -> Result<Option<(i64, i64)>, Response> {
    /// Ручной разбор — до квартала; стандартные окна (≤30д) этим лимитом не ограничены.
    const MAX_CUSTOM_RANGE_SECS: i64 = 92 * 86_400;
    let bad =
        |message: &str| (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response();
    let from_raw = params.get("from");
    let to_raw = params.get("to");
    if from_raw.is_none() && to_raw.is_none() {
        return Ok(None);
    }
    let (Some(from_raw), Some(to_raw)) = (from_raw, to_raw) else {
        return Err(bad("from and to must be given together (epoch seconds)"));
    };
    let Ok(from) = from_raw.parse::<i64>() else {
        return Err(bad("from must be epoch seconds"));
    };
    let Ok(to) = to_raw.parse::<i64>() else {
        return Err(bad("to must be epoch seconds"));
    };
    let to = to.min(now + 1);
    if from < 0 || from >= to {
        return Err(bad(
            "from must be non-negative and less than to (a range fully in the future is rejected)",
        ));
    }
    if to - from > MAX_CUSTOM_RANGE_SECS {
        return Err(bad("custom range is too wide (max 92 days)"));
    }
    Ok(Some((from, to)))
}

/// Одна usage_events-агрегация «кто тратит» за полуоткрытое окно [since_ts, until_ts):
/// топ-50 аккаунтов по charge + провайдеры + топ-20 моделей; суммарные charge/real/requests
/// считаются по строкам топ-50 (семантика стандартных окон, custom её наследует). Ошибка любого
/// запроса → 503 «billing authority unavailable», как у соседних денежных ручек.
async fn spend_window_json(
    b: &forward::AsyncBilling,
    since_ts: i64,
    until_ts: i64,
) -> Result<serde_json::Value, Response> {
    let r2 = |nano: i64| (nano as f64 / 1e9 * 100.0).round() / 100.0;
    let rows = match b.spend_by_account_range(since_ts, until_ts, 50).await {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("billing spend stats query failed: {error:#}");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response());
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
    let providers: Vec<_> = match b.spend_by_provider_range(since_ts, until_ts).await {
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
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response());
        }
    };
    // Top-20 моделей по charge из того же usage_events, что и providers[]. `model` там —
    // served id из ответа апстрима, по которому реально посчитан charge (фолбэк — модель
    // запроса): разбивка отражает фактический прайсинг, а не клиентский алиас/`-latest`.
    // Группировка по (model, provider): один model ID могут обслуживать разные апстримы.
    let models: Vec<_> = match b.spend_by_model_range(since_ts, until_ts, 20).await {
        Ok(rows) => rows
            .into_iter()
            .map(|row| {
                json!({
                    "model": row.model,
                    "provider": row.provider,
                    "requests": row.requests,
                    "charge_usd": r2(row.charge_nano),
                    "real_usd": r2(row.real_nano),
                })
            })
            .collect(),
        Err(error) => {
            eprintln!("billing model spend query failed: {error:#}");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response());
        }
    };
    Ok(json!({
        "charge_usd": r2(charge_total),
        "real_usd": r2(real_total),
        "requests": requests_total,
        "accounts": accounts,
        "providers": providers,
        "models": models,
    }))
}

/// Здоровье settlement pipeline — «тихие деньги»: зависшие/битые settle и лаг pricing-воркера
/// коммерции раньше были видны только в stderr. Гейт — control-ключ (денежная диагностика,
/// как /overview и /spend-stats). Ответ:
///   outbox.pending/processing/done/failed — counts по state (реально пишутся pending/done/failed;
///     'processing' объявлен в CHECK миграции 0001/0004, но ни один writer его пока не ставит);
///   outbox.failed_24h — failed с updated_ts за последние 24ч;
///   outbox.pending_with_error — ретраи в полёте (state='pending' с last_error): на PostgreSQL это
///     transient-ошибки до исчерпания классификатора, permanent уходят в 'failed';
///   outbox.backlog — несеттленые (pending|processing) старше SETTLEMENT_BACKLOG_SECS;
///   outbox.oldest_unsettled_ts/_age_secs — возраст старейшей несеттленой строки (null, если нет);
///   outbox.recent_failed — последние ≤10 failed: request_id, actual_usd, attempts, last_error,
///     updated_ts. last_error обрезан до 200 символов в registry: там только внутренние
///     invariant/SQLSTATE детали (request_id, суммы, имена constraint'ов) — токенов подписок и
///     API-ключей в settle-пути нет, обрезка страхует от раздутого PG-trace;
///   pricing_consumer — лаг durable-консьюмера ledger'а: ledger_max_id против watermark'ов
///     ledger_consumer_checkpoints (consumer='pricing', тот же, что ack-ит коммерция через
///     /admin/account/{id}/ledger/ack), unacked — строки ledger с id > watermark'а своего аккаунта,
///     oldest_unacked_ts/_age_secs — возраст старейшей из них. Растущий unacked = коммерция не
///     дочитывает списания, а ledger_prune стоит (не удаляет неподтверждённое).
async fn settlement_health(
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
    if let Some(v) = cache_get(&SETTLEMENT_CACHE) {
        return Json(v).into_response();
    }
    let h = match b
        .settlement_health(SETTLEMENT_BACKLOG_SECS, "pricing")
        .await
    {
        Ok(h) => h,
        Err(error) => {
            eprintln!("billing settlement health query failed: {error:#}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response();
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let r2 = |nano: i64| (nano as f64 / 1e9 * 100.0).round() / 100.0;
    let age = |ts: i64| {
        if ts > 0 {
            json!((now - ts).max(0))
        } else {
            json!(null)
        }
    };
    let lag = &h.ledger_consumer;
    let v = json!({
        "now": now,
        "backlog_threshold_secs": SETTLEMENT_BACKLOG_SECS,
        "outbox": {
            "pending": h.pending,
            "processing": h.processing,
            "done": h.done,
            "failed": h.failed,
            "failed_24h": h.failed_24h,
            "pending_with_error": h.pending_with_error,
            "backlog": h.backlog,
            "oldest_unsettled_ts": h.oldest_unsettled_ts,
            "oldest_unsettled_age_secs": age(h.oldest_unsettled_ts),
            "recent_failed": h.recent_failed.iter().map(|f| json!({
                "request_id": f.request_id,
                "actual_usd": r2(f.actual_nano),
                "attempts": f.attempts,
                "last_error": f.last_error,
                "updated_ts": f.updated_ts,
            })).collect::<Vec<_>>(),
        },
        "pricing_consumer": {
            "consumer": lag.consumer,
            "ledger_max_id": lag.ledger_max_id,
            "checkpoints": lag.checkpoints,
            "checkpoint_min": lag.checkpoint_min,
            "unacked": lag.unacked,
            "oldest_unacked_ts": lag.oldest_unacked_ts,
            "oldest_unacked_age_secs": age(lag.oldest_unacked_ts),
        },
    });
    cache_put(&SETTLEMENT_CACHE, &v);
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

/// История флота из metrics.db (control-room тренды): минутные снапшоты `poller::metrics_loop`,
/// прочитанные за окно `window=24h|7d|30d|90d` (дефолт 7d) и сбакетированные до ≤ ~500 точек
/// (`metrics_store::window_bucket`). Гейт — control-ключ: в флот-ряду те же денежные агрегаты
/// (balance/spent), что и в /overview. Опциональный `sub=<masked email>` (маска «abcd…», как у
/// /subs и /capacity) переключает ответ на per-sub ряд cap/util по префиксу email. Чтение — один
/// indexed SELECT по ts + бакетирование в памяти на blocking-потоке, без TTL-кэша: дёшево, а
/// свежесть минутная и так. Писателю (metrics_loop) WAL-читатель не мешает.
#[derive(serde::Deserialize)]
struct FleetHistoryQuery {
    window: Option<String>,
    sub: Option<String>,
}

async fn fleet_history(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<FleetHistoryQuery>,
) -> Response {
    if !control_authed(&app, &headers, &peer) {
        Metrics::inc(&app.metrics.auth_failures);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let window = q.window.unwrap_or_else(|| "7d".into());
    let Some((window_secs, bucket_secs)) = crate::metrics_store::window_bucket(&window) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "unknown window", "allowed": ["24h", "7d", "30d", "90d"]})),
        )
            .into_response();
    };
    // Маска «abcd…» → префикс до «…» (голый префикс тоже принимаем). Только email-алфавит:
    // дальше префикс уходит в GLOB-паттерн metrics_store::sub_history.
    let sub = q.sub.map(|s| s.trim_end_matches('…').to_string());
    if let Some(prefix) = &sub {
        let valid = !prefix.is_empty()
            && prefix.len() <= 64
            && prefix.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '@')
            });
        if !valid {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "bad sub"}))).into_response();
        }
    }
    let since = pool::now() - window_secs;
    let db = app.data_db_path.as_ref().clone();
    let read = tokio::task::spawn_blocking(move || -> rusqlite::Result<Value> {
        // metrics.db лежит рядом с основной data.db — тот же каталог, что у poller::metrics_loop.
        let mdir = std::path::Path::new(&db)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        let c = crate::metrics_store::open(&format!("{mdir}/metrics.db"))?;
        let r2 = |x: Option<f64>| x.map(|v| (v * 100.0).round() / 100.0);
        if let Some(prefix) = sub {
            let points = crate::metrics_store::sub_history(&c, &prefix, since, bucket_secs)?;
            return Ok(json!({
                "now": pool::now(), "window": window, "bucket_secs": bucket_secs,
                "sub": format!("{prefix}…"),
                "series": points.iter().map(|p| json!({
                    "ts": p.ts,
                    "cap5h": r2(p.cap5h), "cap7d": r2(p.cap7d),
                    "util5h": r2(p.util5h), "util7d": r2(p.util7d),
                })).collect::<Vec<_>>(),
            }));
        }
        let points = crate::metrics_store::fleet_history(&c, since, bucket_secs)?;
        Ok(json!({
            "now": pool::now(), "window": window, "bucket_secs": bucket_secs,
            "series": points.iter().map(|p| json!({
                "ts": p.ts,
                "avail_1h": r2(p.avail_1h), "avail_5h": r2(p.avail_5h),
                "avail_1d": r2(p.avail_1d), "avail_7d": r2(p.avail_7d),
                "util5h": r2(p.util5h), "util7d": r2(p.util7d),
                "cap5h": r2(p.cap5h), "cap7d": r2(p.cap7d),
                "cons5h": r2(p.cons5h), "cons7d": r2(p.cons7d),
                "healthy": p.healthy, "cooling": p.cooling, "subs": p.subs,
                "balance_usd": r2(p.balance_usd), "reserved_usd": r2(p.reserved_usd),
                "spent_usd": r2(p.spent_usd), "potential_realapi": r2(p.potential_realapi),
                "coverage7d": r2(p.coverage7d),
                "headroom5h": r2(p.headroom5h), "headroom7d": r2(p.headroom7d),
                "subs_needed": p.subs_needed, "gap": p.gap,
            })).collect::<Vec<_>>(),
        }))
    })
    .await;
    match read {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(error)) => {
            eprintln!("fleet history query failed: {error:#}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "metrics store unavailable"})),
            )
                .into_response()
        }
        Err(error) => {
            eprintln!("fleet history task failed: {error:#}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "metrics store unavailable"})),
            )
                .into_response()
        }
    }
}

/// GPT (OpenAI Codex) подписки: per-home операционный статус codex gateway для вкладки
/// «Подписки» панели. Как и /subs — control-authed; деньги тут только official-price оценки
/// ёмкости (float для UI), money-путь биллинга не трогаем. На Anthropic-процессе codex не
/// настроен → enabled:false; панель читает GPT-данные через OpenAI-origin в Caddy.
async fn codex_subs(
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
    let Some(codex) = &app.codex else {
        return Json(json!({"now": pool::now(), "enabled": false, "homes": []})).into_response();
    };
    let status = codex.operational_status().await;
    Json(codex_subs_value(&status, pool::now())).into_response()
}

fn codex_subs_value(status: &forward::codex::CodexOperationalStatus, now: i64) -> Value {
    let round = |x: f64| (x * 100.0).round() / 100.0;
    let round_opt = |x: Option<f64>| x.map(|value| round(value));
    let window = |w: &forward::codex::CodexRateLimitWindow| {
        json!({
            "used_percent": w.used_percent,
            "window_duration_mins": w.window_duration_mins,
            "resets_at": w.resets_at,
        })
    };
    let homes: Vec<_> = status
        .homes
        .iter()
        .map(|h| {
            json!({
                "id": h.id,
                "process_live": h.process_live,
                "auth_ok": h.auth_ok,
                "account_state": h.account_state,
                "transport_state": h.transport_state,
                "admitted": h.admitted,
                "reject_reason": h.reject_reason,
                "snapshot_age_secs": h.snapshot_age_secs,
                "cooling_until": h.cooling_until,
                "inflight": h.inflight,
                "limit_reached": h.limit_reached,
                "spend_usd_total": round(h.spend_usd_total),
                "calibration_persistence_ok": h.calibration_persistence_ok,
                "rate_limits": h.rate_limits.as_ref().map(|rl| json!({
                    "reached": rl.reached,
                    "observed_at": rl.observed_at,
                    "primary": rl.primary.as_ref().map(window),
                    "secondary": rl.secondary.as_ref().map(window),
                })),
                "windows": h.capacities.iter().map(|c| json!({
                    "slot": c.slot,
                    "window_minutes": c.window_minutes,
                    "resets_at": c.resets_at,
                    "observed_at": c.observed_at,
                    "data_age_seconds": c.data_age_seconds,
                    "used_percent": c.used_percent,
                    "cap_usd": round_opt(c.cap_usd),
                    "remaining_usd": round_opt(c.remaining_usd),
                    "low_usd": round_opt(c.low_usd),
                    "high_usd": round_opt(c.high_usd),
                    "source": c.source,
                    "confidence": c.confidence,
                    "samples": c.samples,
                })).collect::<Vec<_>>(),
                "fast_tiers": h.fast_tiers.iter().map(|tier| json!({
                    "model": tier.model,
                    "catalog_available": tier.catalog_available,
                    "catalog_fast_supported": tier.catalog_fast_supported,
                    "served_tier": tier.served_tier,
                    "provider_reported_tier": tier.provider_reported_tier,
                    "observed_at": tier.observed_at,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    // Fleet totals use the real duration identity. Unknown estimates remain null per-home and do
    // not silently contribute zero or a configured prior to the measured aggregate.
    let totals: Vec<_> = codex_window_totals(status)
        .into_iter()
        .map(|(duration, (cap, remaining, measured, observed))| {
            json!({
                "window_minutes": duration,
                "cap_usd": (measured > 0).then(|| round(cap)),
                "remaining_usd": (measured > 0).then(|| round(remaining)),
                "measured_homes": measured,
                "observed_homes": observed,
            })
        })
        .collect();
    json!({
        "now": now,
        "enabled": true,
        "process_live": status.process_live,
        "available": status.available,
        "homes_total": status.homes.len(),
        "soonest_ready": status.soonest_ready,
        "window_totals": totals,
        "homes": homes,
    })
}

/// Gemini paid-subscription fleet status for the unified panel. This route exists only on the
/// fixed Gemini runtime and contains opaque profile ids plus sanitized quota/transport metadata;
/// Google subject, email, project, OAuth and proxy values never enter the response.
async fn gemini_subs(
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
    let Some(gemini) = &app.gemini else {
        return Json(json!({"now": pool::now(), "enabled": false, "profiles": []})).into_response();
    };
    let now = pool::now();
    let status = gemini.operational_status().await;
    let affinity = app.affinity.stats();
    let profiles = gemini_profile_values(&status);
    let models = status
        .models
        .iter()
        .map(|model| {
            json!({
                "id": model.id,
                "available": model.available,
                "healthy": model.healthy,
                "degraded": model.degraded,
                "unknown": model.unknown,
                "soonest_ready": model.soonest_ready,
            })
        })
        .collect::<Vec<_>>();
    let window_totals = gemini_window_total_values(&status);
    Json(json!({
        "now": now,
        "enabled": true,
        "profiles_total": status.profiles.len(),
        "authenticated": status.authenticated,
        "available": status.available,
        "inflight": status.profiles.iter().map(|profile| profile.inflight).sum::<usize>(),
        "soonest_ready": status.soonest_ready,
        "capacity_semantics": {
            "kind": "realized_workload_api_equivalent",
            "fixed_subscription_nominal": false,
            "source": "workload_blend",
        },
        "window_totals": window_totals,
        "models": models,
        "profiles": profiles,
        "transport": {
            "antigravity_version": gemini.config().antigravity_version,
            "profile": forward::GEMINI_NODE_TRANSPORT_PROFILE,
            "node_version": gemini.config().node_version,
            "node_sha256": gemini.config().node_sha256,
            "http_version": "HTTP/1.1",
            "expected_ja3": forward::GEMINI_NODE_EXPECTED_JA3,
            "expected_ja4": forward::GEMINI_NODE_EXPECTED_JA4,
            "userinfo_profile": forward::GEMINI_NODE_FETCH_TRANSPORT_PROFILE,
            "userinfo_http_version": "HTTP/1.1",
            "userinfo_expected_ja3": forward::GEMINI_NODE_FETCH_EXPECTED_JA3,
            "userinfo_expected_ja4": forward::GEMINI_NODE_FETCH_EXPECTED_JA4,
        },
        "affinity": {
            "local_hits": affinity.local_hits,
            "redis_hits": affinity.redis_hits,
            "misses": affinity.misses,
            "redis_errors": affinity.redis_errors,
            "native_hits": affinity.native_hits,
            "transcript_hits": affinity.transcript_hits,
            "cache_root_hits": affinity.cache_root_hits,
            "cache_root_writes": affinity.cache_root_writes,
            "cache_root_warm_placements": affinity.cache_root_warm_placements,
            "cache_root_cold_placements": affinity.cache_root_cold_placements,
            "claims": affinity.claims,
            "rebinds": affinity.rebinds,
        },
        "usage_metadata_missing": Metrics::get(&app.metrics.gemini_usage_missing),
        "failures": {
            "transport": Metrics::get(&app.metrics.gemini_transport_failures),
            "backend": Metrics::get(&app.metrics.gemini_backend_failures),
            "malformed": Metrics::get(&app.metrics.gemini_malformed_responses),
            "stream_start": Metrics::get(&app.metrics.gemini_stream_start_failures),
        },
    }))
    .into_response()
}

fn gemini_profile_values(status: &forward::GeminiOperationalStatus) -> Vec<Value> {
    let round = |x: f64| (x * 1_000_000.0).round() / 1_000_000.0;
    let round_opt = |x: Option<f64>| x.map(round);
    status
        .profiles
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "authenticated": profile.authenticated,
                "cooling_until": profile.cooling_until,
                "inflight": profile.inflight,
                "last_probe_at": profile.last_probe_at,
                "quota_updated_at": profile.quota_updated_at,
                "spend_usd_total": round(profile.spend_usd_total),
                "calibration_persistence_ok": profile.calibration_persistence_ok,
                "windows": profile.capacities.iter().map(|window| json!({
                    "bucket_id": window.bucket_id,
                    "window_kind": window.window_kind,
                    "window_minutes": window.window_minutes,
                    "resets_at": window.resets_at,
                    "observed_at": window.observed_at,
                    "data_age_seconds": window.data_age_seconds,
                    "remaining_fraction_units": window.remaining_fraction_units,
                    "used_fraction_units": window.used_fraction_units,
                    "remaining_fraction": window.remaining_fraction_units as f64 / 100_000_000.0,
                    "used_fraction": window.used_fraction_units as f64 / 100_000_000.0,
                    "cap_usd": round_opt(window.cap_usd),
                    "remaining_usd": round_opt(window.remaining_usd),
                    "low_usd": round_opt(window.low_usd),
                    "high_usd": round_opt(window.high_usd),
                    "remaining_low_usd": round_opt(window.remaining_low_usd),
                    "remaining_high_usd": round_opt(window.remaining_high_usd),
                    "observed_spend_nano": window.observed_spend_nano.to_string(),
                    "observed_spend_usd": round(window.observed_spend_nano as f64 / 1e9),
                    "observed_fraction_units": window.observed_fraction_units,
                    "workload_dependent": true,
                    "source": window.source,
                    "confidence": window.confidence,
                    "samples": window.samples,
                })).collect::<Vec<_>>(),
                "model_cooling": profile.model_cooling.iter().map(|cooling| json!({
                    "model_id": cooling.model_id,
                    "cooling_until": cooling.cooling_until,
                    "failure_streak": cooling.failure_streak,
                    "last_success_at": cooling.last_success_at,
                    "last_failure_at": cooling.last_failure_at,
                    "last_failure_class": cooling.last_failure_class,
                })).collect::<Vec<_>>(),
                "quotas": profile.quotas.iter().map(|quota| json!({
                    "model_id": quota.model_id,
                    "remaining_amount": quota.remaining_amount,
                    "remaining_fraction": quota.remaining_fraction,
                    "reset_time": quota.reset_time,
                    "token_type": quota.token_type,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn gemini_window_total_values(status: &forward::GeminiOperationalStatus) -> Vec<Value> {
    let round = |x: f64| (x * 1_000_000.0).round() / 1_000_000.0;
    gemini_window_totals(status)
        .into_iter()
        .map(|(duration, total)| {
            let measured = total.measured_profiles;
            json!({
                "window_minutes": duration,
                "cap_usd": (measured > 0).then(|| round(total.cap_usd)),
                "remaining_usd": (measured > 0).then(|| round(total.remaining_usd)),
                "low_usd": (measured > 0 && total.low_profiles == measured)
                    .then(|| round(total.low_usd)),
                "high_usd": (measured > 0 && total.high_profiles == measured)
                    .then(|| round(total.high_usd)),
                "remaining_low_usd": (measured > 0 && total.low_profiles == measured)
                    .then(|| round(total.remaining_low_usd)),
                "remaining_high_usd": (measured > 0 && total.high_profiles == measured)
                    .then(|| round(total.remaining_high_usd)),
                "measured_profiles": measured,
                "observed_profiles": total.observed_profiles,
                "source": if measured > 0 { "workload_blend" } else { "unknown" },
                "workload_dependent": true,
            })
        })
        .collect()
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
    provider_ready: Option<bool>,
) -> (StatusCode, serde_json::Value) {
    if accepting.load(Ordering::Acquire)
        && authority_ready.load(Ordering::Acquire)
        && provider_ready.unwrap_or(true)
    {
        (StatusCode::OK, json!({ "ready": true }))
    } else {
        let reason = if !accepting.load(Ordering::Acquire) {
            "draining"
        } else if !authority_ready.load(Ordering::Acquire) {
            "authority_unavailable"
        } else {
            "provider_unavailable"
        };
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({ "ready": false, "reason": reason }),
        )
    }
}

fn codex_provider_ready(live_authenticated_homes: usize) -> bool {
    // One working subscription is real capacity: a single authenticated home keeps the service
    // floor, exactly like the Claude pool. Blue-green safety comes from old/candidate cohort
    // parity while both generations overlap, not from a fixed fleet-size threshold.
    live_authenticated_homes >= 1
}

async fn ready(State(state): State<HttpState>) -> Response {
    // Only the dedicated OpenAI slots use provider liveness for load-balancer admission. A home
    // counts once its sealed credential opened and this generation proved the profile works
    // (`ready_published`). `codex=None` deliberately stays ready so the provider kill switch can
    // serve its stable OpenAI-shaped disabled envelope.
    let provider_ready = if state.app.provider == forward::ProviderMode::OpenAi {
        match &state.app.codex {
            Some(codex) => {
                let status = codex.operational_status().await;
                let ready = status
                    .homes
                    .iter()
                    .filter(|home| home.process_live && home.auth_ok)
                    .count();
                Some(codex_provider_ready(ready))
            }
            None => None,
        }
    } else {
        None
    };
    let (status, body) =
        readiness_snapshot(&state.accepting, &state.app.authority_ready, provider_ready);
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
    if client_keys(&headers).is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "no api key"})),
        )
            .into_response();
    }
    // Любой валидный credential → аккаунт → общий баланс; невалидный соседний заголовок не затмевает его.
    let (key, auth) = match resolve_client_key(billing, &headers).await {
        Ok(Some(resolved)) => resolved,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response()
        }
        Err(error) => {
            eprintln!("billing authorization lookup failed: {error:#}");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "billing authority unavailable"})),
            )
                .into_response();
        }
    };
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
    let acct = match billing.account(&auth.account_id).await {
        Ok(account) => account,
        Err(error) => {
            eprintln!("billing account lookup failed: {error:#}");
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
    use axum::body::{to_bytes, Body};
    use tower::ServiceExt;

    fn unknown_codex_status() -> forward::codex::CodexOperationalStatus {
        forward::codex::CodexOperationalStatus {
            process_live: true,
            rate_limits: None,
            homes: vec![forward::codex::CodexHomeStatus {
                id: "home-1".to_string(),
                process_live: true,
                auth_ok: true,
                account_state: "healthy",
                transport_state: "responsive",
                admitted: true,
                ready_published: true,
                reject_reason: None,
                snapshot_age_secs: Some(5),
                cooling_until: 0,
                inflight: 0,
                rate_limits: None,
                limit_reached: false,
                spend_usd_total: 12.5,
                calibration_persistence_ok: true,
                capacities: vec![forward::codex::CodexWindowCapacityReport {
                    slot: "primary",
                    window_minutes: Some(300),
                    resets_at: Some(2_000_000_000),
                    observed_at: 100,
                    data_age_seconds: Some(5),
                    used_percent: 40,
                    cap_usd: None,
                    remaining_usd: None,
                    low_usd: None,
                    high_usd: None,
                    source: "unknown",
                    confidence: 0.0,
                    samples: 0,
                }],
                fast_tiers: vec![forward::codex::CodexFastTierStatus {
                    model: "gpt-5.6-sol".to_string(),
                    catalog_available: Some(true),
                    catalog_fast_supported: Some(true),
                    served_tier: Some("priority"),
                    provider_reported_tier: Some("default"),
                    observed_at: Some(101),
                }],
            }],
            available: 1,
            soonest_ready: None,
        }
    }

    fn unknown_gemini_status() -> forward::GeminiOperationalStatus {
        let window = |bucket_id, window_kind, window_minutes, remaining_fraction_units| {
            forward::GeminiWindowCapacityReport {
                bucket_id,
                window_kind,
                window_minutes,
                resets_at: 2_000_000_000,
                observed_at: 100,
                data_age_seconds: 5,
                remaining_fraction_units,
                used_fraction_units: 100_000_000 - remaining_fraction_units,
                cap_usd: None,
                remaining_usd: None,
                low_usd: None,
                high_usd: None,
                remaining_low_usd: None,
                remaining_high_usd: None,
                observed_spend_nano: 0,
                observed_fraction_units: 0,
                source: "unknown",
                confidence: 0.0,
                samples: 0,
            }
        };
        forward::GeminiOperationalStatus {
            profiles: vec![forward::GeminiProfileStatus {
                id: "profile-opaque".to_string(),
                authenticated: true,
                cooling_until: 0,
                inflight: 0,
                last_probe_at: 100,
                quota_updated_at: 100,
                quotas: Vec::new(),
                model_cooling: Vec::new(),
                spend_usd_total: 0.019404,
                calibration_persistence_ok: true,
                capacities: vec![
                    window("gemini-5h", "5h", 300, 75_000_000),
                    window("gemini-weekly", "weekly", 10_080, 60_000_000),
                ],
            }],
            models: Vec::new(),
            available: 1,
            authenticated: 1,
            soonest_ready: None,
        }
    }

    #[test]
    fn codex_subscription_contract_publishes_the_admission_verdict() {
        let mut status = unknown_codex_status();
        let value = codex_subs_value(&status, 105);
        assert_eq!(value["homes"][0]["limit_reached"], false);

        // A home the gateway refuses to route to must never read as active on an operator surface.
        status.homes[0].limit_reached = true;
        status.homes[0].capacities[0].used_percent = 100;
        status.available = 0;
        let value = codex_subs_value(&status, 105);
        assert_eq!(value["homes"][0]["limit_reached"], true);
        assert_eq!(value["available"], 0);
        assert_eq!(
            value["homes"][0]["windows"][0]["used_percent"], 100,
            "the exhausted window stays visible next to the verdict"
        );
    }

    #[test]
    fn codex_subscription_contract_separates_effective_fast_from_provider_report() {
        let value = codex_subs_value(&unknown_codex_status(), 105);
        let tier = &value["homes"][0]["fast_tiers"][0];
        assert_eq!(tier["model"], "gpt-5.6-sol");
        assert_eq!(tier["catalog_fast_supported"], true);
        assert_eq!(tier["served_tier"], "priority");
        assert_eq!(tier["provider_reported_tier"], "default");
        assert_eq!(tier["observed_at"], 101);
    }

    #[test]
    fn codex_subscription_contract_keeps_unmeasured_capacity_null() {
        let mut status = unknown_codex_status();
        let mut duplicate_slot = status.homes[0].capacities[0].clone();
        duplicate_slot.slot = "secondary";
        status.homes[0].capacities.push(duplicate_slot);
        let value = codex_subs_value(&status, 105);
        let window = &value["homes"][0]["windows"][0];
        assert_eq!(window["window_minutes"], 300);
        assert_eq!(window["source"], "unknown");
        assert!(window["cap_usd"].is_null());
        assert!(window["remaining_usd"].is_null());

        let total = &value["window_totals"][0];
        assert_eq!(total["window_minutes"], 300);
        assert_eq!(
            total["observed_homes"], 1,
            "one home must not be counted twice"
        );
        assert_eq!(total["measured_homes"], 0);
        assert!(total["cap_usd"].is_null());
        assert!(total["remaining_usd"].is_null());
    }

    #[test]
    fn codex_subscription_contract_publishes_cumulative_capacity_and_remaining() {
        let mut status = unknown_codex_status();
        let capacity = &mut status.homes[0].capacities[0];
        capacity.cap_usd = Some(2_450.04188);
        capacity.remaining_usd = Some(1_911.0326664);
        capacity.source = "measured_cumulative";
        capacity.confidence = 0.8333;
        capacity.samples = 10;

        let value = codex_subs_value(&status, 105);
        let window = &value["homes"][0]["windows"][0];
        assert_eq!(window["cap_usd"], 2_450.04);
        assert_eq!(window["remaining_usd"], 1_911.03);
        assert_eq!(window["source"], "measured_cumulative");
        assert_eq!(window["samples"], 10);
        assert_eq!(value["window_totals"][0]["measured_homes"], 1);
    }

    #[test]
    fn prometheus_omits_unmeasured_codex_dollar_series() {
        let home = &unknown_codex_status().homes[0];
        let mut body = String::new();
        write_codex_home_capacity_metrics(&mut body, home);
        assert!(body.contains(
            "claude_api_codex_home_window_estimate_available{home=\"home-1\",slot=\"primary\",window_minutes=\"300\",source=\"unknown\"} 0"
        ));
        assert!(body.contains(
            "claude_api_codex_home_window_data_age_seconds{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 5"
        ));
        assert!(!body.contains("claude_api_codex_home_window_capacity_usd{"));
        assert!(!body.contains("claude_api_codex_home_window_remaining_usd{"));
    }

    #[test]
    fn gemini_subscription_contract_keeps_five_hour_and_weekly_unknown_independently() {
        let mut status = unknown_gemini_status();
        let profiles = gemini_profile_values(&status);
        assert_eq!(profiles[0]["windows"][0]["bucket_id"], "gemini-5h");
        assert_eq!(profiles[0]["windows"][1]["bucket_id"], "gemini-weekly");
        assert!(profiles[0]["windows"][0]["cap_usd"].is_null());
        assert!(profiles[0]["windows"][1]["cap_usd"].is_null());

        let totals = gemini_window_total_values(&status);
        assert_eq!(totals[0]["window_minutes"], 300);
        assert_eq!(totals[0]["observed_profiles"], 1);
        assert_eq!(totals[0]["measured_profiles"], 0);
        assert!(totals[0]["cap_usd"].is_null());
        assert_eq!(totals[1]["window_minutes"], 10_080);
        assert_eq!(totals[1]["measured_profiles"], 0);

        let five_hour = &mut status.profiles[0].capacities[0];
        five_hour.cap_usd = Some(36.515628714);
        five_hour.remaining_usd = Some(27.386721535);
        five_hour.low_usd = Some(20.000244158);
        five_hour.high_usd = Some(81.204166575);
        five_hour.remaining_low_usd = Some(15.000183118);
        five_hour.remaining_high_usd = Some(60.903124931);
        five_hour.observed_spend_nano = 61_448_500;
        five_hour.observed_fraction_units = 168_280;
        five_hour.source = "workload_blend";
        five_hour.confidence = 0.123;
        five_hour.samples = 2;
        let profiles = gemini_profile_values(&status);
        assert_eq!(profiles[0]["windows"][0]["cap_usd"], 36.515629);
        assert_eq!(profiles[0]["windows"][0]["remaining_usd"], 27.386722);
        assert_eq!(profiles[0]["windows"][0]["low_usd"], 20.000244);
        assert_eq!(profiles[0]["windows"][0]["high_usd"], 81.204167);
        assert_eq!(profiles[0]["windows"][0]["observed_spend_nano"], "61448500");
        assert_eq!(profiles[0]["windows"][0]["source"], "workload_blend");
        assert_eq!(profiles[0]["windows"][0]["workload_dependent"], true);
        assert!(profiles[0]["windows"][1]["cap_usd"].is_null());
        let totals = gemini_window_total_values(&status);
        assert_eq!(totals[0]["measured_profiles"], 1);
        assert_eq!(totals[0]["low_usd"], 20.000244);
        assert_eq!(totals[0]["high_usd"], 81.204167);
        assert_eq!(totals[0]["source"], "workload_blend");
        assert_eq!(totals[1]["measured_profiles"], 0);
    }

    #[test]
    fn prometheus_omits_unmeasured_gemini_dollar_series() {
        let profile = &unknown_gemini_status().profiles[0];
        let mut body = String::new();
        write_gemini_profile_capacity_metrics(&mut body, profile);
        assert!(body.contains(
            "claude_api_gemini_profile_window_estimate_available{profile=\"profile-opaque\",window=\"5h\",window_minutes=\"300\",source=\"unknown\"} 0"
        ));
        assert!(body.contains(
            "claude_api_gemini_profile_window_estimate_available{profile=\"profile-opaque\",window=\"weekly\",window_minutes=\"10080\",source=\"unknown\"} 0"
        ));
        assert!(!body.contains("claude_api_gemini_profile_window_capacity_usd{"));
        assert!(!body.contains("claude_api_gemini_profile_window_remaining_usd{"));
    }

    fn admin_auth_test_app() -> AppState {
        let mut cfg = crate::config::Settings::from_env().proxy;
        cfg.api_keys = vec!["admin-key".to_string()];
        cfg.control_keys = vec!["control-key".to_string()];
        cfg.panel_keys = vec!["panel-key".to_string()];
        cfg.trust_loopback = false;

        let clients = Arc::new(forward::Clients::new(&cfg));
        AppState {
            provider: forward::ProviderMode::Combined,
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
            gemini: None,
            billing: None,
            pricing_shadow: None,
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(forward::Breaker::new(0)),
            metrics: Arc::new(Metrics::new()),
            key_limiter: Arc::new(forward::KeyLimiter::new()),
            concurrency: Arc::new(tokio::sync::Semaphore::new(16)),
            probe_poke: None,
        }
    }

    fn provider_test_app(provider: forward::ProviderMode) -> AppState {
        let mut app = admin_auth_test_app();
        app.provider = provider;
        app
    }

    #[tokio::test]
    async fn every_admin_route_enforces_the_control_key_lattice() {
        assert_eq!(ADMIN_ROUTE_CASES.len(), 27);
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

    async fn control_json_request(
        service: &Router,
        method: Method,
        path: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-api-key", "control-key")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
        let response = service.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
        let value = serde_json::from_slice(&body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()));
        (status, value)
    }

    #[tokio::test]
    async fn pricing_control_api_preserves_version_identity_cas_and_dual_lineage_state() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-pricing-control-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let billing =
            Arc::new(forward::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap());
        billing
            .create_account("acct_pricing_control", Some("pricing-control"), 5_000)
            .await
            .unwrap();

        let mut app = admin_auth_test_app();
        app.billing = Some(billing);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let catalog = json!({
            "product_id": "main",
            "generation": 1,
            "schema_version": 1,
            "capability_generation": 1,
            "capability_digest": "capability-v1",
            "content_digest": "catalog-v1",
            "entries": [{
                "provider_id": "anthropic",
                "canonical_model_id": "claude-sonnet-4-6",
                "enabled": true
            }]
        });
        let (status, prepared) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/prepare",
            catalog.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(prepared["result"], "stored");
        assert_eq!(prepared["identity"]["catalog"], catalog);
        let (_, replayed) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/prepare",
            catalog.clone(),
        )
        .await;
        assert_eq!(replayed["result"], "unchanged");
        let mut catalog_with_unknown_field = catalog.clone();
        catalog_with_unknown_field["implicit_activation"] = json!(true);
        let (status, _) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/prepare",
            catalog_with_unknown_field,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        let mut conflicting_catalog = catalog.clone();
        conflicting_catalog["capability_digest"] = json!("different-capability");
        let (status, conflict) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/main/activate",
            json!({
                "catalog": conflicting_catalog,
                "expectation": "absent"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(conflict["code"], "version_conflict");

        let mut missing_catalog = catalog.clone();
        missing_catalog["generation"] = json!(99);
        missing_catalog["content_digest"] = json!("catalog-missing");
        let (status, missing) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/main/activate",
            json!({
                "catalog": missing_catalog,
                "expectation": "absent"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(missing["code"], "missing_dependency");

        let activate_catalog = json!({
            "catalog": catalog.clone(),
            "expectation": "absent"
        });
        let (status, applied) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/main/activate",
            activate_catalog.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied["result"], "applied");
        assert_eq!(applied["identity"]["catalog"], catalog);
        let (_, replayed) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/main/activate",
            activate_catalog,
        )
        .await;
        assert_eq!(replayed["result"], "unchanged");

        let switches = json!({
            "generation": 1,
            "schema_version": 1,
            "capability_generation": 1,
            "capability_digest": "capability-v1",
            "content_digest": "switches-v1",
            "entries": [
                {
                    "provider_id": "anthropic",
                    "scope": "master",
                    "catalog_generation": null,
                    "enabled": true
                },
                {
                    "provider_id": "anthropic",
                    "scope": {"segment": {"product_id": "main", "segment": "b2c"}},
                    "catalog_generation": 1,
                    "enabled": true
                }
            ]
        });
        let (status, prepared) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/switches/prepare",
            switches.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(prepared["result"], "stored");
        let (status, applied) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/switches/activate",
            json!({
                "switches": switches.clone(),
                "expectation": "absent"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied["result"], "applied");
        assert_eq!(applied["identity"]["switches"], switches);

        let policy = json!({
            "account_id": "acct_pricing_control",
            "effective_version": 1,
            "policy_id": "global-b2c-v1",
            "policy_version": 1,
            "source_policy_digest": "commerce-global-b2c-v1",
            "owner_type": "global_b2c",
            "owner_id": "global-b2c",
            "account_class": "b2c",
            "product_id": "main",
            "schema_version": 1,
            "catalog_generation": 1,
            "switch_generation": 1,
            "content_digest": "account-policy-v1",
            "replacement_locked": false,
            "rules": [{
                "rule_id": "anthropic-track",
                "rule_digest": "anthropic-track-v1",
                "scope": {"provider": {"provider_id": "anthropic"}},
                "pricing_mode": "track",
                "rule_origin": "managed",
                "discount_bps": null,
                "payable_multiplier_bp": 5_000,
                "track_eligible": true,
                "retention_eligible": true,
                "commission_eligible": true
            }]
        });
        let (status, prepared) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/policy/prepare",
            policy.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(prepared["result"], "stored");
        let binding = json!({
            "policy_enforcement": "shadow",
            "funding_enforcement": "legacy_single",
            "reconciliation_state": "pending"
        });
        let mut redirected_policy = policy.clone();
        redirected_policy["account_id"] = json!("another-account");
        let (status, rejected) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/policy/acct_pricing_control/activate",
            json!({
                "policy": redirected_policy,
                "binding": binding.clone(),
                "expectation": "unbound"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(rejected["code"], "invalid");

        let (status, applied) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/policy/acct_pricing_control/activate",
            json!({
                "policy": policy.clone(),
                "binding": binding.clone(),
                "expectation": "unbound"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(applied["result"], "applied");
        assert_eq!(applied["identity"]["policy"], policy);
        assert_eq!(applied["identity"]["activation"]["binding"], binding);

        let (status, state) = control_json_request(
            &service,
            Method::GET,
            "/admin/pricing/policy/acct_pricing_control/state",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(state["state"]["account_id"], "acct_pricing_control");
        assert_eq!(
            state["state"]["policy"]["active"]["policy"]["content_digest"],
            "account-policy-v1"
        );
        assert_eq!(
            state["state"]["policy_catalog"]["content_digest"],
            "catalog-v1"
        );
        assert_eq!(
            state["state"]["admission_switches"]["content_digest"],
            "switches-v1"
        );

        let catalog_v2 = json!({
            "product_id": "main",
            "generation": 2,
            "schema_version": 1,
            "capability_generation": 1,
            "capability_digest": "capability-v1",
            "content_digest": "catalog-v2",
            "entries": [{
                "provider_id": "anthropic",
                "canonical_model_id": "claude-sonnet-4-6",
                "enabled": true
            }]
        });
        let _ = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/prepare",
            catalog_v2.clone(),
        )
        .await;
        let (status, conflict) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/catalog/main/activate",
            json!({
                "catalog": catalog_v2,
                "expectation": "absent"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(conflict["code"], "cas_mismatch");
        assert_eq!(
            conflict["rejection"]["cas_mismatch"]["actual"]["version"],
            1
        );

        drop(service);
        let _ = std::fs::remove_file(path);
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
            readiness_snapshot(&accepting, &authority_ready, None),
            (StatusCode::OK, json!({"ready": true}))
        );

        accepting.store(false, Ordering::Release);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready, Some(true)),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"ready": false, "reason": "draining"}),
            )
        );
        accepting.store(true, Ordering::Release);
        authority_ready.store(false, Ordering::Release);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready, Some(true)),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"ready": false, "reason": "authority_unavailable"}),
            )
        );
        authority_ready.store(true, Ordering::Release);
        assert_eq!(
            readiness_snapshot(&accepting, &authority_ready, Some(false)),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"ready": false, "reason": "provider_unavailable"}),
            )
        );
    }

    #[test]
    fn openai_readiness_preserves_a_single_working_home() {
        assert!(codex_provider_ready(1));
        assert!(codex_provider_ready(7));
        assert!(!codex_provider_ready(0));
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

    #[tokio::test]
    async fn fixed_provider_routers_ignore_the_legacy_plane_header() {
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

        let mut anthropic_request = Request::builder()
            .uri("/pool")
            .header("x-api-key", "admin-key")
            .header(API_PLANE_HEADER, "openai")
            .body(Body::empty())
            .unwrap();
        anthropic_request.extensions_mut().insert(peer);
        let anthropic = router(
            provider_test_app(forward::ProviderMode::Anthropic),
            Arc::new(AtomicBool::new(true)),
        )
        .oneshot(anthropic_request)
        .await
        .unwrap();
        assert_eq!(anthropic.status(), StatusCode::OK);

        let mut openai_request = Request::builder()
            .uri("/pool")
            .header("x-api-key", "admin-key")
            .header(API_PLANE_HEADER, "anthropic")
            .body(Body::empty())
            .unwrap();
        openai_request.extensions_mut().insert(peer);
        let openai = router(
            provider_test_app(forward::ProviderMode::OpenAi),
            Arc::new(AtomicBool::new(true)),
        )
        .oneshot(openai_request)
        .await
        .unwrap();
        assert_eq!(openai.status(), StatusCode::NOT_FOUND);

        let mut gemini_request = Request::builder()
            .method(Method::POST)
            .uri("/v1beta/models/gemini-2.5-flash:generateContent")
            .header("x-api-key", "admin-key")
            .header(API_PLANE_HEADER, "openai")
            .body(Body::from(r#"{"contents":[]}"#))
            .unwrap();
        gemini_request.extensions_mut().insert(peer);
        let gemini = router(
            provider_test_app(forward::ProviderMode::Gemini),
            Arc::new(AtomicBool::new(true)),
        )
        .oneshot(gemini_request)
        .await
        .unwrap();
        assert_eq!(gemini.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(gemini.into_body(), 1024).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["error"]["code"], 404);
        assert_eq!(body["error"]["status"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn pricing_shadow_metrics_expose_only_bounded_labels_and_default_off_config() {
        let mut app = admin_auth_test_app();
        let metrics = Arc::new(Metrics::new());
        let manifest = registry::pricing::PricingRuntimeManifestEvidence::new(
            1,
            vec![registry::pricing::PricingRuntimeCapabilityEvidence::new(
                registry::pricing::PRICING_SCHEMA_VERSION,
                1,
                "metrics-capability-v1",
            )
            .unwrap()],
        )
        .unwrap();
        let manifest_digest = manifest.manifest_digest().to_owned();
        app.pricing_shadow = Some(
            forward::PricingShadowRuntime::start(
                forward::PricingShadowConfig::default(),
                manifest,
                None,
                Arc::clone(&metrics),
            )
            .unwrap(),
        );
        app.metrics = metrics;

        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        let mut request = Request::builder()
            .uri("/metrics")
            .header("x-api-key", "panel-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = router(app, Arc::new(AtomicBool::new(true)))
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();

        for sample in [
            "claude_api_pricing_shadow_enabled 0",
            "claude_api_pricing_shadow_sample_basis_points 0",
            "claude_api_pricing_shadow_queue_capacity 256",
            "claude_api_pricing_shadow_worker_concurrency 2",
            "claude_api_pricing_shadow_timeout_milliseconds 750",
            "claude_api_pricing_shadow_max_queue_age_seconds 300",
            "claude_api_pricing_shadow_max_field_bytes 512",
            "claude_api_pricing_shadow_max_item_bytes 16384",
            "claude_api_pricing_shadow_rate_per_second 20",
            "claude_api_pricing_shadow_rate_burst 40",
            "claude_api_pricing_shadow_db_read_connections 2",
        ] {
            assert!(body.contains(sample), "missing shadow metric {sample}");
        }
        assert!(body.contains(&format!(
            "claude_api_pricing_shadow_runtime_manifest_info{{generation=\"1\",digest=\"{manifest_digest}\"}} 1"
        )));

        let shadow_samples = body
            .lines()
            .filter(|line| line.starts_with("claude_api_pricing_shadow_") && !line.starts_with('#'))
            .collect::<Vec<_>>();
        for forbidden_label in [
            "account=",
            "account_id=",
            "key=",
            "key_id=",
            "request=",
            "request_id=",
            "model=",
            "model_id=",
        ] {
            assert!(
                shadow_samples
                    .iter()
                    .all(|sample| !sample.contains(forbidden_label)),
                "pricing shadow metrics leaked unbounded label {forbidden_label}"
            );
        }
        for provider in ["anthropic", "openai"] {
            assert!(body.contains(&format!(
                "claude_api_pricing_shadow_enqueue_total{{provider=\"{provider}\",result=\"accepted\"}} 0"
            )));
            assert!(body.contains(&format!(
                "claude_api_pricing_shadow_processing_total{{provider=\"{provider}\",result=\"cancelled\"}} 0"
            )));
            assert!(body.contains(&format!(
                "claude_api_pricing_shadow_rejected_total{{provider=\"{provider}\",reason=\"missing_rule\"}} 0"
            )));
            assert!(body.contains(&format!(
                "claude_api_pricing_shadow_read_error_total{{provider=\"{provider}\",reason=\"evaluation_timeout\"}} 0"
            )));
            assert!(body.contains(&format!(
                "claude_api_pricing_shadow_resolved_total{{provider=\"{provider}\",mode=\"discount\",scope=\"model\",comparison=\"different\"}} 0"
            )));
        }
    }

    #[tokio::test]
    async fn gemini_fleet_status_is_readonly_key_protected_and_runtime_scoped() {
        let service = router(
            provider_test_app(forward::ProviderMode::Gemini),
            Arc::new(AtomicBool::new(true)),
        );
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        for (credential, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("panel-key"), StatusCode::OK),
            (Some("control-key"), StatusCode::OK),
            (Some("admin-key"), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .uri("/gemini-subs")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected, "credential {credential:?}");
            if expected == StatusCode::OK {
                let body = to_bytes(response.into_body(), 4_096).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["enabled"], false);
                assert_eq!(body["profiles"], json!([]));
                let wire = body.to_string();
                for forbidden in [
                    "subject",
                    "email",
                    "project_id",
                    "refresh_token",
                    "access_token",
                    "client_secret",
                    "proxy",
                ] {
                    assert!(!wire.contains(forbidden), "leaked field {forbidden}");
                }
            }
        }
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
    fn customer_error_event_is_structured_and_redacts_request_data() {
        let uri: Uri = "/v1/responses/secret-response/input_items?api_key=raw-secret"
            .parse()
            .unwrap();
        let event = customer_error_event(
            StatusCode::PAYMENT_REQUIRED,
            "billing_limit",
            "acct_safe",
            "key_safe",
            &Method::POST,
            &uri,
            "request_safe",
            Some(60),
            Some(&registry::AccountRow {
                id: "acct_safe".to_string(),
                handle: None,
                balance_nano: 999,
                spent_nano: 1,
                reserved_nano: 2,
                mult_bp: 500,
                status: "active".to_string(),
            }),
            &registry::KeyRow {
                key: "raw-key-must-not-appear".to_string(),
                key_id: "key_safe".to_string(),
                account_id: Some("acct_safe".to_string()),
                label: Some("private label must not appear".to_string()),
                spent_nano: 3,
                reserved_nano: 4,
                spend_limit_nano: None,
                expires_ts: None,
                created_ts: 0,
                last_used_ts: None,
                status: "active".to_string(),
            },
        );
        let value: Value = serde_json::from_str(&event).unwrap();
        assert_eq!(
            value,
            json!({
                "event": "customer_http_error",
                "status": 402,
                "reason": "billing_limit",
                "account_id": "acct_safe",
                "key_id": "key_safe",
                "method": "POST",
                "path": "/v1/responses/{id}/input_items",
                "request_id": "request_safe",
                "retry_after_seconds": 60,
                "account_balance_nano": 999,
                "account_reserved_nano": 2,
                "key_spent_nano": 3,
                "key_reserved_nano": 4,
                "key_spend_limit_nano": null,
                "key_expires_ts": null,
                "account_status": "active",
                "key_status": "active",
            })
        );
        assert!(!event.contains("secret-response"));
        assert!(!event.contains("raw-secret"));
        assert!(!event.contains("api_key"));
        assert!(!event.contains("raw-key-must-not-appear"));
        assert!(!event.contains("private label must not appear"));
    }

    #[test]
    fn audit_path_allows_only_fixed_route_templates() {
        assert_eq!(audit_path("/v1/messages"), "/v1/messages");
        assert_eq!(
            audit_path("/v1/models/client-controlled"),
            "/v1/models/{id}"
        );
        assert_eq!(
            audit_path("/v1/client-secret/unsupported"),
            "/v1/{unsupported}"
        );
    }

    #[test]
    fn billing_limit_reason_identifies_the_binding_budget() {
        let account = registry::AccountRow {
            id: "acct_safe".to_string(),
            handle: None,
            balance_nano: 1_000,
            spent_nano: 0,
            reserved_nano: 0,
            mult_bp: 500,
            status: "active".to_string(),
        };
        let mut key = registry::KeyRow {
            key: "secret".to_string(),
            key_id: "key_safe".to_string(),
            account_id: Some(account.id.clone()),
            label: None,
            spent_nano: 300,
            reserved_nano: 200,
            spend_limit_nano: None,
            expires_ts: None,
            created_ts: 0,
            last_used_ts: None,
            status: "active".to_string(),
        };
        assert_eq!(
            billing_limit_reason(Some(&account), &key),
            "account_balance"
        );
        key.spend_limit_nano = Some(700);
        assert_eq!(
            billing_limit_reason(Some(&account), &key),
            "key_spend_limit"
        );
        key.spend_limit_nano = Some(2_000);
        assert_eq!(
            billing_limit_reason(Some(&account), &key),
            "account_balance"
        );
        key.spend_limit_nano = Some(1_500);
        assert_eq!(
            billing_limit_reason(Some(&account), &key),
            "account_and_key_limit"
        );
    }

    fn fleet_history_test_app(tag: &str) -> (AppState, std::path::PathBuf) {
        let mut app = admin_auth_test_app();
        // metrics.db открывается рядом с data_db_path — направляем каталог в tempdir, чтобы
        // тест не оставлял файл в рабочем дереве крейта.
        let dir = std::env::temp_dir().join(format!("fh_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        app.data_db_path = Arc::new(dir.join("data.db").to_string_lossy().into_owned());
        (app, dir)
    }

    #[tokio::test]
    async fn fleet_history_enforces_control_key_and_validates_window() {
        let (app, dir) = fleet_history_test_app("gate");
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        // В ряду денежные агрегаты (balance/spent) → гейт control, read-only panel-ключ не подходит.
        for (credential, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("panel-key"), StatusCode::UNAUTHORIZED),
            (Some("control-key"), StatusCode::OK),
            (Some("admin-key"), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .uri("/fleet-history")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected, "credential {credential:?}");
            if expected == StatusCode::OK {
                let body = to_bytes(response.into_body(), 65_536).await.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["window"], "7d", "дефолтное окно — 7d");
                assert_eq!(body["bucket_secs"], 1_800);
                assert_eq!(body["series"], json!([]), "истории ещё нет — пустой ряд");
                assert!(body["now"].as_i64().unwrap() > 0);
            }
        }
        for uri in [
            "/fleet-history?window=3d",
            "/fleet-history?window=",
            "/fleet-history?sub=%0A",
        ] {
            let mut request = Request::builder()
                .uri(uri)
                .header("x-api-key", "control-key")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fleet_history_returns_bucketed_fleet_and_per_sub_series() {
        let (app, dir) = fleet_history_test_app("series");
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        // Сеем три минутных снапшота (как poller::metrics_loop) + per-sub ряд одной подписки.
        let now = pool::now();
        let c = crate::metrics_store::open(dir.join("metrics.db").to_str().unwrap()).unwrap();
        for mins_ago in [0i64, 1, 2] {
            let ts = now - mins_ago * 60;
            crate::metrics_store::insert_snapshot(
                &c,
                &serde_json::json!({
                    "now": ts, "subs": 3, "calibrated": true,
                    "supply": {"avail_usd": {"1h": 10.0, "5h": 20.0, "1d": 30.0, "7d": 40.0},
                               "cap_usd": {"5h": 20.0, "7d": 100.0},
                               "consumed_usd": {"5h": 1.0, "7d": 5.0},
                               "util": {"5h": 0.05, "7d": 0.5}, "health": {"healthy": 2, "cooling": 1}},
                    "demand": {"balance_usd": 500.0, "reserved_usd": 1.0, "spent_usd": 9.0,
                               "active_accounts": 4, "potential_realapi_usd": 2500.0},
                    "headroom": {"5h": null, "7d": 8.0}, "coverage": {"7d": 62.5},
                    "recommend": {"subs_needed": 1, "gap": -2}
                }),
            )
            .unwrap();
            crate::metrics_store::insert_sub_snapshots(
                &c,
                ts,
                &[("alpha@example.com".to_string(), 10.0, 100.0, 0.2, 0.4)],
            )
            .unwrap();
        }
        drop(c);
        // Флот-ряд: все поля контракта на месте, значения из снапшотов.
        let mut request = Request::builder()
            .uri("/fleet-history?window=24h")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["window"], "24h");
        assert_eq!(body["bucket_secs"], 300);
        let series = body["series"].as_array().unwrap();
        assert!(!series.is_empty(), "сеяные снапшоты должны попасть в ряд");
        let point = &series[0];
        for field in [
            "ts",
            "avail_1h",
            "avail_5h",
            "avail_1d",
            "avail_7d",
            "util5h",
            "util7d",
            "cap5h",
            "cap7d",
            "cons5h",
            "cons7d",
            "healthy",
            "cooling",
            "subs",
            "balance_usd",
            "reserved_usd",
            "spent_usd",
            "potential_realapi",
            "coverage7d",
            "headroom5h",
            "headroom7d",
            "subs_needed",
            "gap",
        ] {
            assert!(point.get(field).is_some(), "нет поля {field}");
        }
        assert_eq!(point["avail_5h"], 20.0);
        assert_eq!(point["balance_usd"], 500.0);
        assert_eq!(point["gap"], -2);
        assert_eq!(point["subs"], 3);
        assert!(
            point["headroom5h"].is_null(),
            "headroom5h=∞ хранится как NULL → null"
        );
        assert_eq!(point["headroom7d"], 8.0);
        // Per-sub ряд по маске «alph…» (URL-encoded «…»), как его шлёт панель.
        let mut request = Request::builder()
            .uri("/fleet-history?window=24h&sub=alph%E2%80%A6")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["sub"], "alph…");
        let series = body["series"].as_array().unwrap();
        assert!(!series.is_empty());
        assert_eq!(series[0]["cap7d"], 100.0);
        assert_eq!(series[0]["cap5h"], 10.0);
        assert_eq!(series[0]["util7d"], 0.4);
        // Чужая маска → пустой ряд, полные email в ответе не светятся.
        let mut request = Request::builder()
            .uri("/fleet-history?window=24h&sub=zzzz")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["series"], json!([]));
        assert!(!body.to_string().contains("alpha@example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AppState с настоящим SQLite-биллингом в tempdir — для /spend-stats и /settlement-health
    /// (AsyncBilling::start сам поднимает writer + reader на том же файле, WAL делит чтения).
    fn billing_test_app(tag: &str) -> (AppState, std::path::PathBuf) {
        let mut app = admin_auth_test_app();
        let dir = std::env::temp_dir().join(format!("billing_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("data.db");
        let billing = forward::AsyncBilling::start(db.to_string_lossy().into_owned(), 1).unwrap();
        app.billing = Some(Arc::new(billing));
        (app, dir)
    }

    /// /spend-stats кэширует periods в процессном static, а cargo test гоняет тесты параллельно:
    /// без сериализации соседний тест получил бы periods чужого tempdir-биллинга. Гард держится
    /// до конца теста и на захвате сбрасывает кэш.
    fn spend_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap();
        if let Some(cell) = SPEND_CACHE.get() {
            *cell.lock().unwrap() = None;
        }
        guard
    }

    #[tokio::test]
    async fn settlement_health_enforces_control_key_and_reports_pipeline() {
        let (app, dir) = billing_test_app("settlement");
        // Сеем outbox напрямую: свежий failed с длинным last_error + старый pending (backlog).
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        let ts = pool::now();
        conn.execute(
            "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
             next_attempt_ts,last_error,created_ts,updated_ts) \
             VALUES('r-failed',1500000000,'failed',5,0,?1,?2,?3)",
            rusqlite::params!["x".repeat(500), ts - 7200, ts - 30],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO billing_settlement_outbox(request_id,actual_nano,state,attempts, \
             next_attempt_ts,last_error,created_ts,updated_ts) \
             VALUES('r-stuck',1000,'pending',3,0,'transient pg error',?1,?2)",
            rusqlite::params![ts - 3600, ts - 60],
        )
        .unwrap();
        drop(conn);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        // Денежная диагностика → гейт control, read-only panel-ключ не подходит.
        for (credential, expected) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("panel-key"), StatusCode::UNAUTHORIZED),
            (Some("control-key"), StatusCode::OK),
            (Some("admin-key"), StatusCode::OK),
        ] {
            let mut request = Request::builder()
                .uri("/settlement-health")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            if let Some(key) = credential {
                request
                    .headers_mut()
                    .insert("x-api-key", key.parse().unwrap());
            }
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected, "credential {credential:?}");
        }
        let mut request = Request::builder()
            .uri("/settlement-health")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body["now"].as_i64().unwrap() > 0);
        assert_eq!(body["backlog_threshold_secs"], 300);
        let outbox = &body["outbox"];
        assert_eq!(outbox["failed"], 1);
        assert_eq!(outbox["failed_24h"], 1);
        assert_eq!(outbox["pending"], 1);
        assert_eq!(outbox["pending_with_error"], 1);
        assert_eq!(outbox["backlog"], 1, "старый pending старше порога");
        assert!(outbox["oldest_unsettled_age_secs"].as_i64().unwrap() >= 3000);
        let failed = outbox["recent_failed"].as_array().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["request_id"], "r-failed");
        assert_eq!(failed[0]["actual_usd"], 1.5);
        assert_eq!(failed[0]["attempts"], 5);
        assert_eq!(
            failed[0]["last_error"].as_str().unwrap().chars().count(),
            200,
            "last_error урезан до 200 символов"
        );
        let consumer = &body["pricing_consumer"];
        assert_eq!(consumer["consumer"], "pricing");
        for field in [
            "ledger_max_id",
            "checkpoints",
            "checkpoint_min",
            "unacked",
            "oldest_unacked_ts",
        ] {
            assert!(consumer.get(field).is_some(), "нет поля {field}");
        }
        assert_eq!(consumer["checkpoints"], 0, "консьюмер ещё не ack-ал");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spend_stats_includes_served_model_breakdown() {
        let (app, dir) = billing_test_app("spend");
        let _lock = spend_test_lock();
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::account_create(&conn, "acct", None, 2000).unwrap();
        let usage = registry::UsageEventInput {
            model: "claude-opus-5".into(),
            real_nano: 100_000_000,
            ..Default::default()
        };
        registry::usage_event_add(&conn, "acct", None, &usage, 50_000_000, Some("r1")).unwrap();
        registry::usage_event_add(&conn, "acct", None, &usage, 25_000_000, Some("r2")).unwrap();
        drop(conn);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        let mut request = Request::builder()
            .uri("/spend-stats")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let d1 = &body["periods"]["d1"];
        let models = d1["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["model"], "claude-opus-5");
        assert_eq!(models[0]["provider"], "anthropic");
        assert_eq!(models[0]["requests"], 2);
        assert_eq!(models[0]["charge_usd"], 0.08); // (50M+25M) nano → $0.075 → 0.08
        assert_eq!(models[0]["real_usd"], 0.2);
        // accounts/providers не потерялись рядом с новой разбивкой.
        assert_eq!(d1["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(d1["providers"].as_array().unwrap().len(), 1);
        // Без ?from&to произвольного диапазона в ответе нет.
        assert!(body.get("custom").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spend_stats_validates_custom_range() {
        let (app, dir) = billing_test_app("spend_range_bad");
        let _lock = spend_test_lock();
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        let now = pool::now();
        let mut bad = vec![
            "/spend-stats?from=abc&to=123".to_string(),
            "/spend-stats?from=100&to=xyz".to_string(),
            "/spend-stats?from=100".to_string(),
            "/spend-stats?to=100".to_string(),
            "/spend-stats?from=200&to=100".to_string(),
            "/spend-stats?from=-5&to=100".to_string(),
            // шире 92 дней даже после зажатия to до now
            "/spend-stats?from=0&to=99999999999".to_string(),
            // диапазон целиком в будущем
            format!("/spend-stats?from={}&to={}", now + 3_600, now + 7_200),
        ];
        // Гейт идёт первым: без ключа даже мусорные параметры отвечают 401, а не 400.
        let mut request = Request::builder()
            .uri(bad[0].as_str())
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        for uri in bad.drain(..) {
            let mut request = Request::builder()
                .uri(uri.as_str())
                .header("x-api-key", "control-key")
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(peer);
            let response = service.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "uri {uri}");
            let body = to_bytes(response.into_body(), 65_536).await.unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert!(body["error"].as_str().unwrap().len() > 10, "uri {uri}");
        }
        // Валидный диапазон на пустых данных: custom присутствует с нулевыми суммами,
        // to из будущего зажимается (внутренний код кладёт now+1 — проверяем только границы).
        let uri = format!("/spend-stats?from={}&to={}", now - 3_600, now + 3_600);
        let mut request = Request::builder()
            .uri(uri.as_str())
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let custom = &body["custom"];
        assert_eq!(custom["from"], now - 3_600);
        assert_eq!(custom["requests"], 0);
        assert_eq!(custom["accounts"], json!([]));
        assert_eq!(custom["providers"], json!([]));
        assert_eq!(custom["models"], json!([]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spend_stats_custom_range_aggregates_window() {
        let (app, dir) = billing_test_app("spend_custom");
        let _lock = spend_test_lock();
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::account_create(&conn, "acct", None, 2000).unwrap();
        let usage = registry::UsageEventInput {
            model: "claude-opus-5".into(),
            real_nano: 100_000_000,
            ..Default::default()
        };
        registry::usage_event_add(&conn, "acct", None, &usage, 50_000_000, Some("r1")).unwrap();
        registry::usage_event_add(&conn, "acct", None, &usage, 25_000_000, Some("r2")).unwrap();
        // Старое событие вне диапазона, но внутри стандартного окна d30.
        registry::usage_event_add(&conn, "acct", None, &usage, 70_000_000, Some("r3")).unwrap();
        conn.execute(
            "UPDATE usage_events SET ts=?1 WHERE ref='r3'",
            rusqlite::params![pool::now() - 10 * 86_400],
        )
        .unwrap();
        drop(conn);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));
        let now = pool::now();
        let uri = format!("/spend-stats?from={}&to={}", now - 3_600, now + 3_600);
        let mut request = Request::builder()
            .uri(uri.as_str())
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let custom = &body["custom"];
        assert_eq!(custom["from"], now - 3_600);
        assert!(custom["to"].as_i64().unwrap() <= now + 1);
        // В диапазон попали только r1+r2: 75M nano charge → $0.08, 200M real → $0.2.
        assert_eq!(custom["requests"], 2);
        assert_eq!(custom["charge_usd"], 0.08);
        assert_eq!(custom["real_usd"], 0.2);
        let accounts = custom["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0]["account"], "acct");
        assert_eq!(accounts[0]["requests"], 2);
        assert!(accounts[0]["last_ts"].as_i64().unwrap() >= now - 3_600);
        let providers = custom["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["provider"], "anthropic");
        let models = custom["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["model"], "claude-opus-5");
        // Стандартное окно d30 рядом видит и старое событие.
        assert_eq!(body["periods"]["d30"]["requests"], 3);
        // Кэш не загрязнён custom: повторный запрос без параметров отдаёт чистые periods.
        let mut request = Request::builder()
            .uri("/spend-stats")
            .header("x-api-key", "control-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert!(body.get("custom").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
