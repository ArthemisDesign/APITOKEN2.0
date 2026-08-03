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
    anthropic_chat_completions, anthropic_responses, authed, client_keys,
    codex_messages_count_tokens, codex_messages_skin, control_authed, forward, gemini_api,
    gemini_chat_completions, gemini_messages_count_tokens, gemini_messages_skin, gemini_responses,
    openai_chat_completions, openai_delete_response, openai_get_response, openai_input_tokens,
    openai_model, openai_models, openai_response_input_items, openai_responses, readonly_authed,
    resolve_client_key, resolve_client_keys, AppState, Metrics, PricingBridgeFallbackReason,
    PricingShadowEnqueueResult, PricingShadowProcessingResult, StrictPricingProvider,
    StrictPricingRejectionReason, TerminalErrorReason, PRICING_BRIDGE_LATENCY_BUCKETS_MS,
    PRICING_SHADOW_QUEUE_AGE_BUCKETS_SECS,
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
    (
        post,
        POST,
        "/admin/pricing/v2/policy/prepare",
        "/admin/pricing/v2/policy/prepare",
        admin::prepare_pricing_release_policy_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/policy/{policy_id}/version/{policy_version}",
        "/admin/pricing/v2/policy/test-policy/version/1",
        admin::pricing_release_policy_v2
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/release/prepare",
        "/admin/pricing/v2/release/prepare",
        admin::prepare_pricing_release_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/release/{generation}",
        "/admin/pricing/v2/release/1",
        admin::pricing_release_v2
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/recovery-link/prepare",
        "/admin/pricing/v2/recovery-link/prepare",
        admin::prepare_pricing_release_recovery_link_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/recovery-link/{target_generation}/{recovery_generation}",
        "/admin/pricing/v2/recovery-link/1/2",
        admin::pricing_release_recovery_link_v2
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/assignment-extension/prepare",
        "/admin/pricing/v2/assignment-extension/prepare",
        admin::prepare_pricing_release_assignment_extension_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/assignment-extension/{head_version}/{account_id}",
        "/admin/pricing/v2/assignment-extension/1/test-account",
        admin::pricing_release_assignment_extension_v2
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/stage8-evidence/capture",
        "/admin/pricing/v2/stage8-evidence/capture",
        admin::capture_stage8_engine_evidence
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/activate",
        "/admin/pricing/v2/activate",
        admin::activate_pricing_release_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/head",
        "/admin/pricing/v2/head",
        admin::pricing_release_head_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/provisioning-context",
        "/admin/pricing/v2/provisioning-context",
        admin::pricing_release_provisioning_context_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/inventory",
        "/admin/pricing/v2/inventory",
        admin::pricing_release_inventory_v2
    ),
    (
        get,
        GET,
        "/admin/pricing/v2/funding/{account_id}/normalization",
        "/admin/pricing/v2/funding/test-account/normalization",
        admin::funding_normalization_plan_v2
    ),
    (
        post,
        POST,
        "/admin/pricing/v2/funding/{account_id}/normalization",
        "/admin/pricing/v2/funding/test-account/normalization",
        admin::apply_funding_normalization_v2
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
        .route("/metrics", get(metrics))
        .route(
            "/internal/router/auth/preflight",
            post(crate::router_auth::preflight),
        )
        .route(
            "/internal/router/policy/preflight",
            post(crate::router_policy::preflight)
                .layer(axum::extract::DefaultBodyLimit::max(64 * 1024)),
        )
        .route(
            "/internal/router/catalog/pricing",
            post(crate::router_pricing::pricing)
                .layer(axum::extract::DefaultBodyLimit::max(256 * 1024)),
        );
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
            // Universal lane (этап 3.1 UNIFIED_ROUTER.md): chat→Messages адаптер.
            .route("/v1/chat/completions", post(anthropic_chat_completions))
            // Universal Responses (этап 4.1 UNIFIED_ROUTER.md): Responses→Messages
            // адаптер. Stored endpoints (/v1/responses/*) здесь НЕ регистрируются
            // и остаются openai-only (решение 5).
            .route("/v1/responses", post(anthropic_responses))
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
            // Anthropic Skin (этап 5.1 UNIFIED_ROUTER.md): Messages→Responses адаптер
            // на Codex-плоскости. Dispatch по модели (`openai/*` сюда, остальное на
            // Claude-плоскость) выполняет router; сюда попадают только openai-модели.
            .route("/v1/messages", post(codex_messages_skin))
            .route(
                "/v1/messages/count_tokens",
                post(codex_messages_count_tokens),
            )
            .route("/v1/models", get(openai_models))
            .route("/v1/models/{model_id}", get(openai_model))
            .fallback(fixed_openai_not_found)
            .method_not_allowed_fallback(fixed_openai_not_found),
        forward::ProviderMode::Gemini => common
            .route("/gemini-subs", get(gemini_subs))
            // Universal lane (этап 3.3 UNIFIED_ROUTER.md): chat→generateContent адаптер.
            .route("/v1/chat/completions", post(gemini_chat_completions))
            // Universal Responses (этап 4.3 UNIFIED_ROUTER.md): Responses→generateContent
            // адаптер. Stored endpoints (/v1/responses/*) здесь НЕ регистрируются
            // и остаются openai-only (решение 5).
            .route("/v1/responses", post(gemini_responses))
            // Anthropic Skin (этап 5.2 UNIFIED_ROUTER.md): Messages→generateContent адаптер
            // на Gemini-плоскости. Dispatch по модели (`google/*` и gemini-alias'ы сюда,
            // остальное на свои плоскости) выполняет router; сюда попадают только
            // google-модели. count_tokens — через нативный :countTokens (quota-free).
            .route("/v1/messages", post(gemini_messages_skin))
            .route(
                "/v1/messages/count_tokens",
                post(gemini_messages_count_tokens),
            )
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
    let execution_plane = match state.app.provider {
        forward::ProviderMode::Combined if is_openai_plane(request.headers()) => {
            forward::ProviderMode::OpenAi
        }
        forward::ProviderMode::Combined => forward::ProviderMode::Anthropic,
        fixed => fixed,
    };
    let method = request.method().clone();
    let uri = request.uri().clone();
    let response = next.run(request).await;
    if forward::is_exact_not_started_response(&response) {
        state.app.metrics.execution_not_started(execution_plane);
    }
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
    // Codex response history is the one shared-state surface whose loss is customer-visible: a
    // missing previous_response_id becomes a 400 that clients treat as permanent. It had no
    // counters at all, so eviction or a degraded write was invisible until a customer complained.
    let history = app.codex.as_ref().map(|codex| codex.history_stats());
    let g = |c| Metrics::get(c);
    let body = format!(
        "# TYPE claude_api_requests_total counter\nclaude_api_requests_total {}\n\
         # TYPE claude_api_upstream_429_total counter\nclaude_api_upstream_429_total {}\n\
         # TYPE claude_api_upstream_auth_total counter\nclaude_api_upstream_auth_total {}\n\
         # TYPE claude_api_upstream_5xx_total counter\nclaude_api_upstream_5xx_total {}\n\
         # TYPE claude_api_breaker_rejects_total counter\nclaude_api_breaker_rejects_total {}\n\
         # TYPE claude_api_exhausted_total counter\nclaude_api_exhausted_total {}\n\
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
         # TYPE claude_api_cooling_hint_skips_total counter\nclaude_api_cooling_hint_skips_total {}\n\
         # TYPE claude_api_cooling_hint_publishes_total counter\nclaude_api_cooling_hint_publishes_total {}\n\
         # TYPE claude_api_cooling_hint_lookup_errors_total counter\nclaude_api_cooling_hint_lookup_errors_total {}\n\
         # TYPE claude_api_cooling_hint_publish_errors_total counter\nclaude_api_cooling_hint_publish_errors_total {}\n\
         # TYPE claude_api_codex_history_local_hits_total counter\nclaude_api_codex_history_local_hits_total {}\n\
         # TYPE claude_api_codex_history_redis_hits_total counter\nclaude_api_codex_history_redis_hits_total {}\n\
         # TYPE claude_api_codex_history_misses_total counter\nclaude_api_codex_history_misses_total {}\n\
         # TYPE claude_api_codex_history_redis_errors_total counter\nclaude_api_codex_history_redis_errors_total {}\n\
         # TYPE claude_api_codex_history_writes_total counter\nclaude_api_codex_history_writes_total {}\n\
         # TYPE claude_api_codex_history_write_failures_total counter\nclaude_api_codex_history_write_failures_total {}\n\
         # TYPE claude_api_codex_history_wrong_tenant_total counter\nclaude_api_codex_history_wrong_tenant_total {}\n\
         # TYPE claude_api_codex_history_corrupt_total counter\nclaude_api_codex_history_corrupt_total {}\n\
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
        g(&m.cooling_hint_skips),
        affinity.cooling_hint_publishes,
        affinity.cooling_hint_lookup_errors,
        affinity.cooling_hint_publish_errors,
        history.map(|stats| stats.local_hits).unwrap_or(0),
        history.map(|stats| stats.redis_hits).unwrap_or(0),
        history.map(|stats| stats.misses).unwrap_or(0),
        history.map(|stats| stats.redis_errors).unwrap_or(0),
        history.map(|stats| stats.writes).unwrap_or(0),
        history.map(|stats| stats.write_failures).unwrap_or(0),
        history.map(|stats| stats.wrong_tenant).unwrap_or(0),
        history.map(|stats| stats.corrupt).unwrap_or(0),
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
    // The writer queue depth and PostgreSQL money-command latency are operational, not
    // commercial: they expose saturation of the single-writer hot path that every reserve,
    // settlement and capacity lease pays for synchronously, so the readonly panel key may see
    // them. Absent billing (or the SQLite fallback for the histogram) simply omits the series.
    if let Some(b) = &app.billing {
        let _ = writeln!(
            body,
            "# TYPE claude_api_billing_write_queue_depth gauge\n\
             claude_api_billing_write_queue_depth {}",
            b.write_queue_depth()
        );
        if let Some(pg) = b.pg_command_stats() {
            let _ = writeln!(
                body,
                "# TYPE claude_api_billing_pg_command_duration_seconds histogram"
            );
            for op in forward::PgCommandOp::ALL {
                let op_index = op as usize;
                let op_label = op.label();
                for (bucket_index, upper_ms) in
                    forward::PG_COMMAND_LATENCY_BUCKETS_MS.iter().enumerate()
                {
                    let _ = writeln!(
                        body,
                        "claude_api_billing_pg_command_duration_seconds_bucket{{op=\"{op_label}\",le=\"{}\"}} {}",
                        *upper_ms as f64 / 1_000.0,
                        pg.buckets[op_index * forward::PG_COMMAND_LATENCY_BUCKETS_MS.len()
                            + bucket_index],
                    );
                }
                let command_count = pg.count[op_index];
                let _ = writeln!(
                    body,
                    "claude_api_billing_pg_command_duration_seconds_bucket{{op=\"{op_label}\",le=\"+Inf\"}} {command_count}\n\
                     claude_api_billing_pg_command_duration_seconds_sum{{op=\"{op_label}\"}} {}\n\
                     claude_api_billing_pg_command_duration_seconds_count{{op=\"{op_label}\"}} {command_count}",
                    pg.sum_micros[op_index] as f64 / 1_000_000.0,
                );
            }
        }
    }
    let _ = writeln!(
        body,
        "# TYPE claude_api_execution_group_double_winner_total counter\n\
         claude_api_execution_group_double_winner_total {}",
        registry::execution_group_double_winner_total()
    );
    let _ = writeln!(
        body,
        "# HELP claude_api_execution_not_started_total Exact non-2xx not_started proofs returned by provider planes.\n\
         # TYPE claude_api_execution_not_started_total counter"
    );
    for plane in [
        forward::ProviderMode::Anthropic,
        forward::ProviderMode::OpenAi,
        forward::ProviderMode::Gemini,
    ] {
        let _ = writeln!(
            body,
            "claude_api_execution_not_started_total{{plane=\"{}\"}} {}",
            plane.as_str(),
            m.execution_not_started_count(plane),
        );
    }
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
        "# HELP claude_api_claudestore_fallback_attempts_total Last-resort external attempts after local Claude rotation became terminal.\n\
         # TYPE claude_api_claudestore_fallback_attempts_total counter\n\
         claude_api_claudestore_fallback_attempts_total {}\n\
         # HELP claude_api_claudestore_fallback_successes_total Successful last-resort external responses admitted to customer delivery.\n\
         # TYPE claude_api_claudestore_fallback_successes_total counter\n\
         claude_api_claudestore_fallback_successes_total {}\n\
         # HELP claude_api_claudestore_fallback_failures_total Failed external transport, HTTP, or delivery-marker attempts.\n\
         # TYPE claude_api_claudestore_fallback_failures_total counter\n\
         claude_api_claudestore_fallback_failures_total {}",
        g(&m.claudestore_fallback_attempts),
        g(&m.claudestore_fallback_successes),
        g(&m.claudestore_fallback_failures),
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
    for provider in [
        SnapshotProvider::Anthropic,
        SnapshotProvider::OpenAi,
        SnapshotProvider::Google,
    ] {
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
    let _ = writeln!(
        body,
        "# TYPE claude_api_strict_pricing_admitted_total counter\n\
         # TYPE claude_api_strict_pricing_rejected_total counter"
    );
    for provider in StrictPricingProvider::ALL {
        let provider_id = provider.as_str();
        for mode in [PricingMode::Track, PricingMode::Discount] {
            let mode_id = match mode {
                PricingMode::Track => "track",
                PricingMode::Discount => "discount",
            };
            for (model_scope, scope_id) in [(false, "provider"), (true, "model")] {
                let _ = writeln!(
                    body,
                    "claude_api_strict_pricing_admitted_total{{provider=\"{provider_id}\",mode=\"{mode_id}\",scope=\"{scope_id}\"}} {}",
                    m.strict_pricing_admitted_count(*provider, mode, model_scope),
                );
            }
        }
        for reason in StrictPricingRejectionReason::ALL {
            let _ = writeln!(
                body,
                "claude_api_strict_pricing_rejected_total{{provider=\"{provider_id}\",reason=\"{}\"}} {}",
                reason.code(),
                m.strict_pricing_rejected_count(*provider, *reason),
            );
        }
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
    for provider in [
        SnapshotProvider::Anthropic,
        SnapshotProvider::OpenAi,
        SnapshotProvider::Google,
    ] {
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
    let anthropic_delivery = app
        .billing
        .as_ref()
        .map(|billing| billing.anthropic_calibration_delivery_status());
    let anthropic_report = if let Some(billing) = &app.billing {
        billing.anthropic_calibration_report().await.ok()
    } else {
        None
    };
    let _ = writeln!(
        body,
        "# TYPE claude_api_anthropic_calibration_authority_available gauge\n\
         claude_api_anthropic_calibration_authority_available {}\n\
         # TYPE claude_api_anthropic_calibration_pending_events gauge\n\
         claude_api_anthropic_calibration_pending_events {}\n\
         # TYPE claude_api_anthropic_calibration_dropped_events_total counter\n\
         claude_api_anthropic_calibration_dropped_events_total {}\n\
         # TYPE claude_api_anthropic_calibration_persistence_ok gauge\n\
         claude_api_anthropic_calibration_persistence_ok {}",
        u8::from(anthropic_report.is_some()),
        anthropic_delivery.map_or(0, |status| status.pending_events),
        anthropic_delivery.map_or(0, |status| status.dropped_events),
        u8::from(
            anthropic_delivery.is_some_and(|status| status.persistence_ok)
                && anthropic_report.is_some()
        ),
    );
    if let Some(report) = anthropic_report.as_ref() {
        let capacity = capacity_value(
            &app.pool.capacity(),
            Some(report),
            anthropic_delivery,
            pool::now(),
        );
        let _ = writeln!(
            body,
            "# TYPE claude_api_anthropic_window_capacity_usd gauge\n\
             # TYPE claude_api_anthropic_window_remaining_usd gauge\n\
             # TYPE claude_api_anthropic_window_routable_subscriptions gauge\n\
             # TYPE claude_api_anthropic_window_calibrated_subscriptions gauge\n\
             # TYPE claude_api_anthropic_window_snapshot_subscriptions gauge\n\
             # TYPE claude_api_anthropic_window_confidence_ratio gauge\n\
             # TYPE claude_api_anthropic_window_estimate_available gauge"
        );
        if let Some(windows) = capacity["window_totals"].as_array() {
            for window in windows {
                let Some(duration) = window["window_minutes"].as_i64() else {
                    continue;
                };
                let estimate_available = window["capacity_nano"].is_string();
                let _ = writeln!(
                    body,
                    "claude_api_anthropic_window_routable_subscriptions{{window_minutes=\"{duration}\"}} {}\n\
                     claude_api_anthropic_window_calibrated_subscriptions{{window_minutes=\"{duration}\"}} {}\n\
                     claude_api_anthropic_window_snapshot_subscriptions{{window_minutes=\"{duration}\"}} {}\n\
                     claude_api_anthropic_window_estimate_available{{window_minutes=\"{duration}\"}} {}",
                    window["routable_subs"].as_u64().unwrap_or(0),
                    window["calibrated_subs"].as_u64().unwrap_or(0),
                    window["snapshot_subs"].as_u64().unwrap_or(0),
                    u8::from(estimate_available),
                );
                if let Some(value) = window["capacity_nano"]
                    .as_str()
                    .and_then(|value| value.parse::<i128>().ok())
                {
                    let _ = writeln!(
                        body,
                        "claude_api_anthropic_window_capacity_usd{{window_minutes=\"{duration}\"}} {:.9}",
                        value as f64 / 1e9,
                    );
                }
                if let Some(value) = window["remaining_nano"]
                    .as_str()
                    .and_then(|value| value.parse::<i128>().ok())
                {
                    let _ = writeln!(
                        body,
                        "claude_api_anthropic_window_remaining_usd{{window_minutes=\"{duration}\"}} {:.9}",
                        value as f64 / 1e9,
                    );
                }
                if let Some(value) = window["confidence_bp"].as_i64() {
                    let _ = writeln!(
                        body,
                        "claude_api_anthropic_window_confidence_ratio{{window_minutes=\"{duration}\"}} {:.4}",
                        value as f64 / 10_000.0,
                    );
                }
            }
        }
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
             # TYPE claude_api_codex_rate_limit_used_ratio gauge\n\
             # TYPE claude_api_codex_rate_limit_used_fraction_units gauge\n\
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
                "claude_api_codex_rate_limit_used_percent{{window=\"{window_name}\"}} {}\n\
                 claude_api_codex_rate_limit_used_ratio{{window=\"{window_name}\"}} {:.8}\n\
                 claude_api_codex_rate_limit_used_fraction_units{{window=\"{window_name}\"}} {}",
                window.used_percent,
                window.used_fraction(),
                window.used_fraction_units,
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
             # TYPE claude_api_codex_home_window_remaining_low_usd gauge\n\
             # TYPE claude_api_codex_home_window_remaining_high_usd gauge\n\
             # TYPE claude_api_codex_home_window_used_ratio gauge\n\
             # TYPE claude_api_codex_home_window_used_fraction_units gauge\n\
             # TYPE claude_api_codex_home_window_observed_spend_usd gauge\n\
             # TYPE claude_api_codex_home_window_observed_fraction_units gauge\n\
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
             # TYPE claude_api_codex_window_capacity_low_usd gauge\n\
             # TYPE claude_api_codex_window_capacity_high_usd gauge\n\
             # TYPE claude_api_codex_window_remaining_low_usd gauge\n\
             # TYPE claude_api_codex_window_remaining_high_usd gauge\n\
             # TYPE claude_api_codex_window_measured_homes gauge\n\
             # TYPE claude_api_codex_window_observed_homes gauge"
        );
        for (duration, total) in codex_window_totals(status) {
            let _ = writeln!(
                body,
                "claude_api_codex_window_measured_homes{{window_minutes=\"{duration}\"}} {}\n\
                 claude_api_codex_window_observed_homes{{window_minutes=\"{duration}\"}} {}",
                total.measured_homes, total.observed_homes,
            );
            if total.measured_homes > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_codex_window_capacity_usd{{window_minutes=\"{duration}\"}} {:.9}\n\
                     claude_api_codex_window_remaining_usd{{window_minutes=\"{duration}\"}} {:.9}",
                    total.capacity_nano as f64 / 1e9,
                    total.remaining_nano as f64 / 1e9,
                );
            }
            if total.low_homes == total.measured_homes && total.measured_homes > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_codex_window_capacity_low_usd{{window_minutes=\"{duration}\"}} {:.9}\n\
                     claude_api_codex_window_remaining_low_usd{{window_minutes=\"{duration}\"}} {:.9}",
                    total.low_nano as f64 / 1e9,
                    total.remaining_low_nano as f64 / 1e9,
                );
            }
            if total.high_homes == total.measured_homes && total.measured_homes > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_codex_window_capacity_high_usd{{window_minutes=\"{duration}\"}} {:.9}\n\
                     claude_api_codex_window_remaining_high_usd{{window_minutes=\"{duration}\"}} {:.9}",
                    total.high_nano as f64 / 1e9,
                    total.remaining_high_nano as f64 / 1e9,
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
        let delivery = app
            .billing
            .as_ref()
            .map(|billing| billing.gemini_calibration_delivery_status());
        let calibration_persistence_ok = gemini_calibration_persistence_ok(&status, delivery);
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
        let _ = writeln!(
            body,
            "# TYPE claude_api_gemini_calibration_pending_events gauge\n\
             claude_api_gemini_calibration_pending_events {}\n\
             # TYPE claude_api_gemini_calibration_dropped_events_total counter\n\
             claude_api_gemini_calibration_dropped_events_total {}\n\
             # TYPE claude_api_gemini_calibration_persistence_ok gauge\n\
             claude_api_gemini_calibration_persistence_ok {}",
            delivery.map_or(0, |status| status.pending_events),
            delivery.map_or(0, |status| status.dropped_events),
            u8::from(calibration_persistence_ok),
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
            write_gemini_profile_capacity_metrics(&mut body, profile, calibration_persistence_ok);
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
        for (duration, total) in gemini_window_totals(&status, pool::now()) {
            let _ = writeln!(
                body,
                "claude_api_gemini_window_measured_profiles{{window_minutes=\"{duration}\"}} {}\n\
                 claude_api_gemini_window_observed_profiles{{window_minutes=\"{duration}\"}} {}",
                if calibration_persistence_ok {
                    total.measured_profiles
                } else {
                    0
                },
                total.observed_profiles,
            );
            if calibration_persistence_ok && total.measured_profiles > 0 {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.capacity_nano as f64 / 1e9,
                    total.remaining_nano as f64 / 1e9,
                );
            }
            if calibration_persistence_ok
                && total.low_profiles == total.measured_profiles
                && total.measured_profiles > 0
            {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_low_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_low_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.low_nano as f64 / 1e9,
                    total.remaining_low_nano as f64 / 1e9,
                );
            }
            if calibration_persistence_ok
                && total.high_profiles == total.measured_profiles
                && total.measured_profiles > 0
            {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_window_capacity_high_usd{{window_minutes=\"{duration}\"}} {:.6}\n\
                     claude_api_gemini_window_remaining_high_usd{{window_minutes=\"{duration}\"}} {:.6}",
                    total.high_nano as f64 / 1e9,
                    total.remaining_high_nano as f64 / 1e9,
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
            "claude_api_codex_home_window_used_ratio{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.8}\n\
             claude_api_codex_home_window_used_fraction_units{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {}\n\
             claude_api_codex_home_window_estimate_available{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {}\n\
             claude_api_codex_home_window_confidence_ratio{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.4}\n\
             claude_api_codex_home_window_samples{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {}\n\
             claude_api_codex_home_window_observed_spend_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.9}\n\
             claude_api_codex_home_window_observed_fraction_units{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {}",
            capacity.used_fraction_units as f64 / 100_000_000.0,
            capacity.used_fraction_units,
            capacity.source,
            u8::from(capacity.capacity_nano.is_some()),
            capacity.confidence,
            capacity.samples,
            capacity.observed_spend_nano as f64 / 1e9,
            capacity.observed_fraction_units,
        );
        if let Some(age) = capacity.data_age_seconds {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_data_age_seconds{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {age}"
            );
        }
        if let (Some(capacity_nano), Some(remaining_nano)) =
            (capacity.capacity_nano, capacity.remaining_nano)
        {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {:.9}\n\
                 claude_api_codex_home_window_remaining_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\",source=\"{}\"}} {:.9}",
                capacity.source,
                capacity_nano as f64 / 1e9,
                capacity.source,
                remaining_nano as f64 / 1e9,
            );
        }
        if let Some(low_nano) = capacity.low_nano {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_low_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.9}",
                low_nano as f64 / 1e9,
            );
        }
        if let Some(high_nano) = capacity.high_nano {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_capacity_high_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.9}",
                high_nano as f64 / 1e9,
            );
        }
        if let Some(remaining_low_nano) = capacity.remaining_low_nano {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_remaining_low_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.9}",
                remaining_low_nano as f64 / 1e9,
            );
        }
        if let Some(remaining_high_nano) = capacity.remaining_high_nano {
            let _ = writeln!(
                body,
                "claude_api_codex_home_window_remaining_high_usd{{home=\"{index}\",slot=\"{slot}\",window_minutes=\"{duration}\"}} {:.9}",
                remaining_high_nano as f64 / 1e9,
            );
        }
    }
}

fn write_gemini_profile_capacity_metrics(
    body: &mut String,
    profile: &forward::GeminiProfileStatus,
    capacity_available: bool,
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
            if capacity_available {
                capacity.source
            } else {
                "unknown"
            },
            u8::from(capacity_available && capacity.cap_usd.is_some()),
            capacity.confidence,
            capacity.samples,
            capacity.observed_spend_nano as f64 / 1e9,
            capacity.observed_fraction_units,
        );
        // Unknown capacity has no dollar time series. Publishing a numeric zero before the first
        // complete interval would be indistinguishable from a genuinely measured zero-dollar cap.
        if capacity_available {
            if let (Some(cap), Some(remaining)) = (capacity.cap_usd, capacity.remaining_usd) {
                let _ = writeln!(
                    body,
                    "claude_api_gemini_profile_window_capacity_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\",source=\"{}\"}} {cap:.6}\n\
                     claude_api_gemini_profile_window_remaining_usd{{profile=\"{id}\",window=\"{window}\",window_minutes=\"{duration}\",source=\"{}\"}} {remaining:.6}",
                    capacity.source,
                    capacity.source,
                );
            }
        }
        if capacity_available {
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
}

/// Sum each real duration once per home. Slot names are presentation metadata, and duplicate slots
/// must never make one subscription look like two copies of the same dollar capacity.
#[derive(Default)]
struct CodexWindowTotal {
    capacity_nano: i128,
    remaining_nano: i128,
    low_nano: i128,
    high_nano: i128,
    remaining_low_nano: i128,
    remaining_high_nano: i128,
    observed_spend_nano: i128,
    observed_fraction_units: i128,
    measured_homes: usize,
    observed_homes: usize,
    low_homes: usize,
    high_homes: usize,
    capacity_nanocredits: i128,
    remaining_nanocredits: i128,
    low_nanocredits: i128,
    high_nanocredits: i128,
    remaining_low_nanocredits: i128,
    remaining_high_nanocredits: i128,
    observed_spend_nanocredits: i128,
    unattributed_fraction_units: i128,
    credit_measured_homes: usize,
    credit_observed_homes: usize,
    credit_low_homes: usize,
    credit_high_homes: usize,
}

/// Equal paid plans with the same provider window share one native-credit capacity parameter.
/// Individual home estimates remain available for audit, while this aggregate removes arbitrary
/// whole-percent rounding noise from the operator answer "how many credits does one plan hold?".
#[derive(Default)]
struct CodexPlanCohort {
    homes_total: usize,
    measured_homes: usize,
    observed_fraction_units: i128,
    observed_spend_nanocredits: i128,
    unused_fraction_units: i128,
    measurement_resolution_fraction_units: i64,
    low_nanocredits: Option<i64>,
    low_homes: usize,
    high_nanocredits: Option<i64>,
    high_homes: usize,
}

fn codex_window_totals(
    status: &forward::codex::CodexOperationalStatus,
) -> BTreeMap<i64, CodexWindowTotal> {
    let mut totals: BTreeMap<i64, CodexWindowTotal> = BTreeMap::new();
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
            total.observed_homes += 1;
            total.observed_spend_nano += i128::from(capacity.observed_spend_nano);
            total.observed_fraction_units += i128::from(capacity.observed_fraction_units);
            if let Some(observed) = capacity.observed_spend_nanocredits {
                total.observed_spend_nanocredits += i128::from(observed);
                total.credit_observed_homes += 1;
            }
            total.unattributed_fraction_units +=
                i128::from(capacity.unattributed_fraction_units.unwrap_or(0));
            if let (Some(cap), Some(remaining)) = (capacity.capacity_nano, capacity.remaining_nano)
            {
                total.capacity_nano += i128::from(cap);
                total.remaining_nano += i128::from(remaining);
                total.measured_homes += 1;
                if let (Some(low), Some(remaining_low)) =
                    (capacity.low_nano, capacity.remaining_low_nano)
                {
                    total.low_nano += i128::from(low);
                    total.remaining_low_nano += i128::from(remaining_low);
                    total.low_homes += 1;
                }
                if let (Some(high), Some(remaining_high)) =
                    (capacity.high_nano, capacity.remaining_high_nano)
                {
                    total.high_nano += i128::from(high);
                    total.remaining_high_nano += i128::from(remaining_high);
                    total.high_homes += 1;
                }
            }
            if let (Some(cap), Some(remaining)) = (
                capacity.capacity_nanocredits,
                capacity.remaining_nanocredits,
            ) {
                total.capacity_nanocredits += i128::from(cap);
                total.remaining_nanocredits += i128::from(remaining);
                total.credit_measured_homes += 1;
                if let (Some(low), Some(remaining_low)) =
                    (capacity.low_nanocredits, capacity.remaining_low_nanocredits)
                {
                    total.low_nanocredits += i128::from(low);
                    total.remaining_low_nanocredits += i128::from(remaining_low);
                    total.credit_low_homes += 1;
                }
                if let (Some(high), Some(remaining_high)) = (
                    capacity.high_nanocredits,
                    capacity.remaining_high_nanocredits,
                ) {
                    total.high_nanocredits += i128::from(high);
                    total.remaining_high_nanocredits += i128::from(remaining_high);
                    total.credit_high_homes += 1;
                }
            }
        }
    }
    totals
}

fn codex_plan_cohorts(status: &forward::codex::CodexOperationalStatus) -> Vec<Value> {
    const FRACTION_SCALE: i128 = 100_000_000;

    let mut cohorts: BTreeMap<(String, i64), CodexPlanCohort> = BTreeMap::new();
    for home in &status.homes {
        let mut seen = BTreeSet::new();
        for capacity in &home.capacities {
            let Some(duration) = capacity.window_minutes else {
                continue;
            };
            if !seen.insert(duration) {
                continue;
            }
            let cohort = cohorts.entry((home.plan.clone(), duration)).or_default();
            cohort.homes_total += 1;
            cohort.unused_fraction_units += i128::from(
                100_000_000i64.saturating_sub(capacity.used_fraction_units.clamp(0, 100_000_000)),
            );
            cohort.measurement_resolution_fraction_units = cohort
                .measurement_resolution_fraction_units
                .max(capacity.measurement_resolution_fraction_units);

            let Some(observed_spend) = capacity.observed_spend_nanocredits else {
                continue;
            };
            if capacity.capacity_nanocredits.is_none()
                || observed_spend <= 0
                || capacity.observed_fraction_units <= 0
            {
                continue;
            }
            cohort.measured_homes += 1;
            cohort.observed_spend_nanocredits += i128::from(observed_spend);
            cohort.observed_fraction_units += i128::from(capacity.observed_fraction_units);
            if let Some(low) = capacity.low_nanocredits {
                cohort.low_nanocredits = Some(
                    cohort
                        .low_nanocredits
                        .map_or(low, |existing| existing.min(low)),
                );
                cohort.low_homes += 1;
            }
            if let Some(high) = capacity.high_nanocredits {
                cohort.high_nanocredits = Some(
                    cohort
                        .high_nanocredits
                        .map_or(high, |existing| existing.max(high)),
                );
                cohort.high_homes += 1;
            }
        }
    }

    let multiply = |value: Option<i128>, multiplier: i128| {
        value.and_then(|value| value.checked_mul(multiplier))
    };
    let remaining = |capacity: Option<i128>, unused_fraction_units: i128| {
        capacity
            .and_then(|value| value.checked_mul(unused_fraction_units))
            .and_then(|value| value.checked_div(FRACTION_SCALE))
    };
    cohorts
        .into_iter()
        .map(|((plan, duration), cohort)| {
            let capacity_per_home = if cohort.observed_fraction_units > 0 {
                cohort
                    .observed_spend_nanocredits
                    .checked_mul(FRACTION_SCALE)
                    .and_then(|numerator| {
                        numerator
                            .checked_add(cohort.observed_fraction_units / 2)
                            .and_then(|rounded| {
                                rounded.checked_div(cohort.observed_fraction_units)
                            })
                    })
            } else {
                None
            };
            let low_per_home = (cohort.measured_homes > 0
                && cohort.low_homes == cohort.measured_homes)
                .then(|| cohort.low_nanocredits.map(i128::from))
                .flatten();
            let high_per_home = (cohort.measured_homes > 0
                && cohort.high_homes == cohort.measured_homes)
                .then(|| cohort.high_nanocredits.map(i128::from))
                .flatten();
            let homes_total = i128::try_from(cohort.homes_total).expect("usize fits i128");
            let string_opt = |value: Option<i128>| value.map(|value| value.to_string());
            json!({
                "plan": plan,
                "window_minutes": duration,
                "homes_total": cohort.homes_total,
                "measured_homes": cohort.measured_homes,
                "observed_fraction_units": cohort.observed_fraction_units.to_string(),
                "observed_spend_nanocredits": cohort.observed_spend_nanocredits.to_string(),
                "measurement_resolution_fraction_units": cohort.measurement_resolution_fraction_units,
                "capacity_per_home_nanocredits": string_opt(capacity_per_home),
                "capacity_per_home_low_nanocredits": string_opt(low_per_home),
                "capacity_per_home_high_nanocredits": string_opt(high_per_home),
                "fleet_capacity_nanocredits": string_opt(multiply(capacity_per_home, homes_total)),
                "fleet_capacity_low_nanocredits": string_opt(multiply(low_per_home, homes_total)),
                "fleet_capacity_high_nanocredits": string_opt(multiply(high_per_home, homes_total)),
                "fleet_remaining_nanocredits": string_opt(remaining(
                    capacity_per_home,
                    cohort.unused_fraction_units,
                )),
                "fleet_remaining_low_nanocredits": string_opt(remaining(
                    low_per_home,
                    cohort.unused_fraction_units,
                )),
                "fleet_remaining_high_nanocredits": string_opt(remaining(
                    high_per_home,
                    cohort.unused_fraction_units,
                )),
                "source": if capacity_per_home.is_some() {
                    "plan_pooled_native_credits"
                } else {
                    "unknown"
                },
                "same_plan_capacity": true,
                "workload_dependent": false,
            })
        })
        .collect()
}

#[derive(Default)]
struct GeminiWindowTotal {
    capacity_nano: i128,
    remaining_nano: i128,
    low_nano: i128,
    high_nano: i128,
    remaining_low_nano: i128,
    remaining_high_nano: i128,
    measured_profiles: usize,
    observed_profiles: usize,
    low_profiles: usize,
    high_profiles: usize,
}

fn gemini_profile_routable(profile: &forward::GeminiProfileStatus, now: i64) -> bool {
    profile.authenticated
        && profile.cooling_until <= now
        && (profile.model_cooling.is_empty()
            || profile
                .model_cooling
                .iter()
                .any(|model| model.cooling_until <= now))
}

fn gemini_window_totals(
    status: &forward::GeminiOperationalStatus,
    now: i64,
) -> BTreeMap<i64, GeminiWindowTotal> {
    let mut totals: BTreeMap<i64, GeminiWindowTotal> = BTreeMap::new();
    for profile in status
        .profiles
        .iter()
        .filter(|profile| gemini_profile_routable(profile, now))
    {
        for capacity in &profile.capacities {
            let total = totals.entry(capacity.window_minutes).or_default();
            total.observed_profiles += 1;
            if let (Some(cap), Some(remaining)) = (capacity.capacity_nano, capacity.remaining_nano)
            {
                total.capacity_nano += i128::from(cap);
                total.remaining_nano += i128::from(remaining);
                total.measured_profiles += 1;
                if let (Some(low), Some(remaining_low)) =
                    (capacity.low_nano, capacity.remaining_low_nano)
                {
                    total.low_nano += i128::from(low);
                    total.remaining_low_nano += i128::from(remaining_low);
                    total.low_profiles += 1;
                }
                if let (Some(high), Some(remaining_high)) =
                    (capacity.high_nano, capacity.remaining_high_nano)
                {
                    total.high_nano += i128::from(high);
                    total.remaining_high_nano += i128::from(remaining_high);
                    total.high_profiles += 1;
                }
            }
        }
    }
    totals
}

fn mask_claude_email(email: &str) -> String {
    let local = email.split_once('@').map_or(email, |(local, _)| local);
    let head: String = local.chars().take(4).collect();
    format!("{head}…")
}

fn anthropic_conversion_models(now: i64) -> Vec<Value> {
    const MODELS: [(&str, &str); 7] = [
        ("claude-opus-5", "Claude Opus 5"),
        ("claude-fable-5", "Claude Fable 5"),
        ("claude-opus-4-8", "Claude Opus 4.8"),
        ("claude-opus-4-7", "Claude Opus 4.7"),
        ("claude-sonnet-5", "Claude Sonnet 5"),
        ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
        ("claude-haiku-4-5", "Claude Haiku 4.5"),
    ];

    MODELS
        .into_iter()
        .filter_map(|(id, display_name)| {
            let tier = |speed, name| {
                let capability = metering::anthropic_tariff_capability_at(
                    id,
                    now,
                    metering::AnthropicAdmissionModifiers {
                        speed,
                        inference_geo: metering::AnthropicInferenceGeo::Global,
                    },
                )
                .ok()?;
                let prices = capability.effective_reserve_prices;
                Some(json!({
                    "id": name,
                    "tariff_schedule_id": capability.tariff_schedule_id.as_str(),
                    "input_nanousd_per_token": prices.input.to_string(),
                    "cache_read_nanousd_per_token": prices.cache_read.to_string(),
                    "cache_write_5m_nanousd_per_token": prices.cache_write_5m.to_string(),
                    "cache_write_1h_nanousd_per_token": prices.cache_write_1h.to_string(),
                    "output_nanousd_per_token": prices.output.to_string(),
                }))
            };
            let standard = tier(metering::AnthropicSpeed::Standard, "standard")?;
            let tiers = if matches!(id, "claude-opus-5" | "claude-opus-4-8") {
                vec![standard, tier(metering::AnthropicSpeed::Fast, "fast")?]
            } else {
                vec![standard]
            };
            Some(json!({
                "id": id,
                "display_name": display_name,
                "alias_generation": metering::ANTHROPIC_ALIAS_GENERATION,
                "web_search_nanousd_per_request": metering::WEB_SEARCH_NANO.to_string(),
                "us_inference_basis_points": 11_000,
                "tiers": tiers,
            }))
        })
        .collect()
}

const CLAUDE_FRACTION_SCALE: i128 = 100_000_000;
const CLAUDE_WINDOWS: [(&str, i64); 2] = [("5h", 300), ("7d", 10_080)];
/// Idle healthy subscriptions are probed every 5–7.5 minutes. Twice that upper cadence is a
/// conservative boundary: capacity evidence remains valid, but current remaining supply does not.
const CLAUDE_SNAPSHOT_MAX_AGE_SECS: i64 = 900;

#[derive(Default)]
struct ClaudePlanWindowCohort {
    subs_total: usize,
    routable_subs: usize,
    snapshot_subs: usize,
    routable_snapshot_subs: usize,
    measured_subs: usize,
    observed_fraction_units: i128,
    observed_spend_nano: i128,
    samples: i128,
    unattributed_fraction_units: i128,
    unused_routable_fraction_units: i128,
    measurement_resolution_fraction_units: i64,
    low_nano: Option<i64>,
    low_subs: usize,
    high_nano: Option<i64>,
    high_subs: usize,
    confidence_bp: Option<i64>,
    last_observed_at: Option<i64>,
    last_measured_at: Option<i64>,
}

#[derive(Clone, Default)]
struct ClaudeCohortEstimate {
    capacity_per_sub_nano: Option<i128>,
    low_per_sub_nano: Option<i128>,
    high_per_sub_nano: Option<i128>,
    fleet_capacity_nano: Option<i128>,
    fleet_low_nano: Option<i128>,
    fleet_high_nano: Option<i128>,
    fleet_remaining_nano: Option<i128>,
    fleet_remaining_low_nano: Option<i128>,
    fleet_remaining_high_nano: Option<i128>,
}

#[derive(Clone, Copy)]
struct ClaudeCurrentQuota {
    used_fraction_units: i64,
    measurement_resolution_fraction_units: i64,
    observed_at: i64,
    resets_at: Option<i64>,
    source: &'static str,
}

fn claude_row<'a>(
    rows: &'a [registry::AnthropicCalibrationRow],
    email: &str,
    plan: &str,
    window_kind: &str,
) -> Option<&'a registry::AnthropicCalibrationRow> {
    rows.iter()
        .find(|row| row.subject_id == email && row.plan == plan && row.window_kind == window_kind)
}

fn claude_current_quota(
    cap: &pool::Cap,
    row: Option<&registry::AnthropicCalibrationRow>,
    window_kind: &str,
    now: i64,
) -> Option<ClaudeCurrentQuota> {
    let fresh = |observed_at: i64| {
        observed_at <= now.saturating_add(30)
            && now.saturating_sub(observed_at) <= CLAUDE_SNAPSHOT_MAX_AGE_SECS
    };
    let durable = row
        .filter(|row| fresh(row.observed_at))
        .map(|row| ClaudeCurrentQuota {
            used_fraction_units: row.used_fraction_units,
            measurement_resolution_fraction_units: row.measurement_resolution_fraction_units,
            observed_at: row.observed_at,
            resets_at: Some(row.resets_at),
            source: "durable_calibration_snapshot",
        });
    let runtime = match window_kind {
        "5h" => cap.quota5h,
        "7d" => cap.quota7d,
        _ => None,
    }
    .filter(|snapshot| fresh(snapshot.observed_at))
    .map(|snapshot| ClaudeCurrentQuota {
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        observed_at: snapshot.observed_at,
        resets_at: snapshot.resets_at,
        source: "runtime_quota_snapshot",
    });
    match (durable, runtime) {
        (Some(durable), Some(runtime)) if durable.observed_at > runtime.observed_at => {
            Some(durable)
        }
        (_, Some(runtime)) => Some(runtime),
        (Some(durable), None) => Some(durable),
        (None, None) => None,
    }
}

fn claude_last_known_quota(
    cap: &pool::Cap,
    row: Option<&registry::AnthropicCalibrationRow>,
    window_kind: &str,
    now: i64,
) -> Option<ClaudeCurrentQuota> {
    let not_from_future = |observed_at: i64| observed_at <= now.saturating_add(30);
    let durable = row
        .filter(|row| not_from_future(row.observed_at))
        .map(|row| ClaudeCurrentQuota {
            used_fraction_units: row.used_fraction_units,
            measurement_resolution_fraction_units: row.measurement_resolution_fraction_units,
            observed_at: row.observed_at,
            resets_at: Some(row.resets_at),
            source: "durable_calibration_snapshot",
        });
    let runtime = match window_kind {
        "5h" => cap.quota5h,
        "7d" => cap.quota7d,
        _ => None,
    }
    .filter(|snapshot| not_from_future(snapshot.observed_at))
    .map(|snapshot| ClaudeCurrentQuota {
        used_fraction_units: snapshot.used_fraction_units,
        measurement_resolution_fraction_units: snapshot.measurement_resolution_fraction_units,
        observed_at: snapshot.observed_at,
        resets_at: snapshot.resets_at,
        source: "runtime_quota_snapshot",
    });
    match (durable, runtime) {
        (Some(durable), Some(runtime)) if durable.observed_at > runtime.observed_at => {
            Some(durable)
        }
        (_, Some(runtime)) => Some(runtime),
        (Some(durable), None) => Some(durable),
        (None, None) => None,
    }
}

fn claude_last_known_reset(
    cap: &pool::Cap,
    row: Option<&registry::AnthropicCalibrationRow>,
    window_kind: &str,
) -> Option<i64> {
    let durable = row.map(|row| (row.observed_at, row.resets_at));
    let runtime = match window_kind {
        "5h" => cap.quota5h,
        "7d" => cap.quota7d,
        _ => None,
    }
    .and_then(|snapshot| {
        snapshot
            .resets_at
            .map(|resets_at| (snapshot.observed_at, resets_at))
    });
    match (durable, runtime) {
        (Some(durable), Some(runtime)) if durable.0 > runtime.0 => Some(durable.1),
        (_, Some(runtime)) => Some(runtime.1),
        (Some(durable), None) => Some(durable.1),
        (None, None) => None,
    }
}

fn claude_plan_window_cohorts(
    caps: &[pool::Cap],
    rows: &[registry::AnthropicCalibrationRow],
    now: i64,
) -> BTreeMap<(String, i64), ClaudePlanWindowCohort> {
    let mut cohorts = BTreeMap::new();
    for cap in caps {
        for (window_kind, duration) in CLAUDE_WINDOWS {
            let cohort = cohorts
                .entry((cap.plan.clone(), duration))
                .or_insert_with(ClaudePlanWindowCohort::default);
            cohort.subs_total += 1;
            cohort.routable_subs += usize::from(cap.routable);

            let row = claude_row(rows, &cap.email, &cap.plan, window_kind);
            if let Some(current) = claude_current_quota(cap, row, window_kind, now) {
                cohort.snapshot_subs += 1;
                if cap.routable {
                    cohort.routable_snapshot_subs += 1;
                    cohort.unused_routable_fraction_units += i128::from(
                        100_000_000i64
                            .saturating_sub(current.used_fraction_units.clamp(0, 100_000_000)),
                    );
                }
            }
            let Some(row) = row else {
                continue;
            };
            cohort.last_observed_at = Some(
                cohort
                    .last_observed_at
                    .map_or(row.observed_at, |value| value.max(row.observed_at)),
            );
            cohort.measurement_resolution_fraction_units = cohort
                .measurement_resolution_fraction_units
                .max(row.measurement_resolution_fraction_units);
            cohort.unattributed_fraction_units += i128::from(row.unattributed_fraction_units);
            if row.current_capacity_nano.is_none()
                || row.samples <= 0
                || row.observed_fraction_units <= 0
                || row.observed_spend_nano <= 0
            {
                continue;
            }
            cohort.measured_subs += 1;
            cohort.observed_fraction_units += i128::from(row.observed_fraction_units);
            cohort.observed_spend_nano += i128::from(row.observed_spend_nano);
            cohort.samples += i128::from(row.samples);
            cohort.confidence_bp = Some(
                cohort
                    .confidence_bp
                    .map_or(row.current_confidence_bp, |value| {
                        value.min(row.current_confidence_bp)
                    }),
            );
            cohort.last_measured_at = row.last_measured_at.map(|measured_at| {
                cohort
                    .last_measured_at
                    .map_or(measured_at, |value| value.max(measured_at))
            });
            if let Some(low) = row.current_low_nano {
                cohort.low_nano = Some(cohort.low_nano.map_or(low, |value| value.min(low)));
                cohort.low_subs += 1;
            }
            if let Some(high) = row.current_high_nano {
                cohort.high_nano = Some(cohort.high_nano.map_or(high, |value| value.max(high)));
                cohort.high_subs += 1;
            }
        }
    }
    cohorts
}

fn claude_cohort_estimate(cohort: &ClaudePlanWindowCohort) -> ClaudeCohortEstimate {
    let capacity = (cohort.observed_fraction_units > 0 && cohort.observed_spend_nano > 0)
        .then(|| {
            cohort
                .observed_spend_nano
                .checked_mul(CLAUDE_FRACTION_SCALE)
                .and_then(|numerator| {
                    numerator
                        .checked_add(cohort.observed_fraction_units / 2)
                        .and_then(|rounded| rounded.checked_div(cohort.observed_fraction_units))
                })
        })
        .flatten();
    let low = (cohort.measured_subs > 0 && cohort.low_subs == cohort.measured_subs)
        .then(|| cohort.low_nano.map(i128::from))
        .flatten();
    let high = (cohort.measured_subs > 0 && cohort.high_subs == cohort.measured_subs)
        .then(|| cohort.high_nano.map(i128::from))
        .flatten();
    let multiplier = i128::try_from(cohort.routable_subs).expect("usize fits i128");
    let fleet = |value: Option<i128>| {
        (cohort.routable_subs > 0)
            .then(|| value.and_then(|value| value.checked_mul(multiplier)))
            .flatten()
    };
    let remaining = |value: Option<i128>| {
        (cohort.routable_subs > 0 && cohort.routable_snapshot_subs == cohort.routable_subs)
            .then(|| {
                value
                    .and_then(|value| value.checked_mul(cohort.unused_routable_fraction_units))
                    .and_then(|numerator| {
                        numerator
                            .checked_add(CLAUDE_FRACTION_SCALE / 2)
                            .and_then(|rounded| rounded.checked_div(CLAUDE_FRACTION_SCALE))
                    })
            })
            .flatten()
    };
    ClaudeCohortEstimate {
        capacity_per_sub_nano: capacity,
        low_per_sub_nano: low,
        high_per_sub_nano: high,
        fleet_capacity_nano: fleet(capacity),
        fleet_low_nano: fleet(low),
        fleet_high_nano: fleet(high),
        fleet_remaining_nano: remaining(capacity),
        fleet_remaining_low_nano: remaining(low),
        fleet_remaining_high_nano: remaining(high),
    }
}

fn nano_string(value: Option<i128>) -> Option<String> {
    value.map(|value| value.to_string())
}

fn claude_plan_cohort_values(
    cohorts: &BTreeMap<(String, i64), ClaudePlanWindowCohort>,
    authority_available: bool,
    delivery_missing_reason: Option<&str>,
) -> Vec<Value> {
    cohorts
        .iter()
        .map(|((plan, duration), cohort)| {
            let estimate = claude_cohort_estimate(cohort);
            let remaining_allowed = authority_available && delivery_missing_reason.is_none();
            let missing_reason = if !authority_available {
                Some("calibration_authority_unavailable")
            } else if estimate.capacity_per_sub_nano.is_none() {
                Some("awaiting_positive_spend_fraction_interval")
            } else if let Some(reason) = delivery_missing_reason {
                Some(reason)
            } else if cohort.routable_subs > cohort.routable_snapshot_subs {
                Some("missing_current_quota_snapshot")
            } else {
                None
            };
            json!({
                "plan": plan,
                "window_kind": if *duration == 300 { "5h" } else { "7d" },
                "window_minutes": duration,
                "subs_total": cohort.subs_total,
                "routable_subs": cohort.routable_subs,
                "snapshot_subs": cohort.snapshot_subs,
                "routable_snapshot_subs": cohort.routable_snapshot_subs,
                "measured_subs": cohort.measured_subs,
                "observed_fraction_units": cohort.observed_fraction_units.to_string(),
                "observed_spend_nano": cohort.observed_spend_nano.to_string(),
                "samples": cohort.samples.to_string(),
                "unattributed_fraction_units": cohort.unattributed_fraction_units.to_string(),
                "measurement_resolution_fraction_units": cohort.measurement_resolution_fraction_units,
                "confidence_bp": cohort.confidence_bp,
                "last_observed_at": cohort.last_observed_at,
                "last_measured_at": cohort.last_measured_at,
                "capacity_per_sub_nano": nano_string(estimate.capacity_per_sub_nano),
                "low_per_sub_nano": nano_string(estimate.low_per_sub_nano),
                "high_per_sub_nano": nano_string(estimate.high_per_sub_nano),
                "fleet_capacity_nano": nano_string(estimate.fleet_capacity_nano),
                "fleet_low_nano": nano_string(estimate.fleet_low_nano),
                "fleet_high_nano": nano_string(estimate.fleet_high_nano),
                "fleet_remaining_nano": nano_string(remaining_allowed.then_some(estimate.fleet_remaining_nano).flatten()),
                "fleet_remaining_low_nano": nano_string(remaining_allowed.then_some(estimate.fleet_remaining_low_nano).flatten()),
                "fleet_remaining_high_nano": nano_string(remaining_allowed.then_some(estimate.fleet_remaining_high_nano).flatten()),
                "source": if estimate.capacity_per_sub_nano.is_some() {
                    "plan_pooled_workload_api_equivalent"
                } else {
                    "unknown"
                },
                "same_plan_capacity": true,
                "workload_dependent": true,
                "missing_reason": missing_reason,
            })
        })
        .collect()
}

fn claude_window_total_values(
    cohorts: &BTreeMap<(String, i64), ClaudePlanWindowCohort>,
    authority_available: bool,
    delivery_missing_reason: Option<&str>,
) -> Vec<Value> {
    CLAUDE_WINDOWS
        .iter()
        .map(|(window_kind, duration)| {
            let selected = cohorts
                .iter()
                .filter(|((_, cohort_duration), cohort)| {
                    cohort_duration == duration && cohort.routable_subs > 0
                })
                .collect::<Vec<_>>();
            let routable_subs = selected
                .iter()
                .map(|(_, cohort)| cohort.routable_subs)
                .sum::<usize>();
            let calibrated_subs = selected
                .iter()
                .filter(|(_, cohort)| claude_cohort_estimate(cohort).capacity_per_sub_nano.is_some())
                .map(|(_, cohort)| cohort.routable_subs)
                .sum::<usize>();
            let plans_total = selected.len();
            let calibrated_plans = selected
                .iter()
                .filter(|(_, cohort)| claude_cohort_estimate(cohort).capacity_per_sub_nano.is_some())
                .count();
            let snapshot_subs = selected
                .iter()
                .map(|(_, cohort)| cohort.routable_snapshot_subs)
                .sum::<usize>();
            let complete_capacity = authority_available
                && routable_subs > 0
                && calibrated_plans == plans_total;
            let complete_remaining = complete_capacity
                && delivery_missing_reason.is_none()
                && snapshot_subs == routable_subs;
            let complete_low = complete_capacity
                && selected
                    .iter()
                    .all(|(_, cohort)| claude_cohort_estimate(cohort).fleet_low_nano.is_some());
            let complete_high = complete_capacity
                && selected
                    .iter()
                    .all(|(_, cohort)| claude_cohort_estimate(cohort).fleet_high_nano.is_some());
            let sum = |pick: fn(&ClaudeCohortEstimate) -> Option<i128>| {
                selected.iter().try_fold(0i128, |total, (_, cohort)| {
                    pick(&claude_cohort_estimate(cohort)).and_then(|value| total.checked_add(value))
                })
            };
            let capacity = complete_capacity
                .then(|| sum(|estimate| estimate.fleet_capacity_nano))
                .flatten();
            let remaining = complete_remaining
                .then(|| sum(|estimate| estimate.fleet_remaining_nano))
                .flatten();
            let low = complete_low
                .then(|| sum(|estimate| estimate.fleet_low_nano))
                .flatten();
            let high = complete_high
                .then(|| sum(|estimate| estimate.fleet_high_nano))
                .flatten();
            let remaining_low = (complete_low && complete_remaining)
                .then(|| sum(|estimate| estimate.fleet_remaining_low_nano))
                .flatten();
            let remaining_high = (complete_high && complete_remaining)
                .then(|| sum(|estimate| estimate.fleet_remaining_high_nano))
                .flatten();
            let missing_reason = if !authority_available {
                Some("calibration_authority_unavailable")
            } else if routable_subs == 0 {
                Some("no_routable_subscriptions")
            } else if !complete_capacity {
                Some("missing_plan_evidence")
            } else if let Some(reason) = delivery_missing_reason {
                Some(reason)
            } else if !complete_remaining {
                Some("missing_current_quota_snapshot")
            } else {
                None
            };
            let confidence_bp = selected
                .iter()
                .filter_map(|(_, cohort)| cohort.confidence_bp)
                .min();
            json!({
                "window_kind": window_kind,
                "window_minutes": duration,
                "capacity_nano": nano_string(capacity),
                "remaining_nano": nano_string(remaining),
                "low_nano": nano_string(low),
                "high_nano": nano_string(high),
                "remaining_low_nano": nano_string(remaining_low),
                "remaining_high_nano": nano_string(remaining_high),
                "routable_subs": routable_subs,
                "calibrated_subs": calibrated_subs,
                "snapshot_subs": snapshot_subs,
                "plans_total": plans_total,
                "calibrated_plans": calibrated_plans,
                "confidence_bp": confidence_bp,
                "samples": selected.iter().map(|(_, cohort)| cohort.samples).sum::<i128>().to_string(),
                "observed_fraction_units": selected.iter().map(|(_, cohort)| cohort.observed_fraction_units).sum::<i128>().to_string(),
                "observed_spend_nano": selected.iter().map(|(_, cohort)| cohort.observed_spend_nano).sum::<i128>().to_string(),
                "unattributed_fraction_units": selected.iter().map(|(_, cohort)| cohort.unattributed_fraction_units).sum::<i128>().to_string(),
                "source": if capacity.is_some() {
                    "plan_pooled_workload_api_equivalent"
                } else {
                    "unknown"
                },
                "workload_dependent": true,
                "fail_closed": true,
                "missing_reason": missing_reason,
            })
        })
        .collect()
}

fn claude_reset_count(now: i64, reset_at: i64, window_secs: i64, horizon_secs: i64) -> i128 {
    let next = if reset_at > now {
        reset_at
    } else {
        reset_at.saturating_add(
            ((now.saturating_sub(reset_at)) / window_secs + 1).saturating_mul(window_secs),
        )
    };
    if next > now.saturating_add(horizon_secs) {
        0
    } else {
        i128::from(1 + (now.saturating_add(horizon_secs) - next) / window_secs)
    }
}

fn claude_sub_horizon_available(
    cap: &pool::Cap,
    rows: &[registry::AnthropicCalibrationRow],
    cohorts: &BTreeMap<(String, i64), ClaudePlanWindowCohort>,
    now: i64,
    horizon_secs: i64,
) -> Option<i128> {
    if !cap.routable {
        return Some(0);
    }
    let availability = CLAUDE_WINDOWS
        .iter()
        .map(|(window_kind, duration)| {
            let row = claude_row(rows, &cap.email, &cap.plan, window_kind);
            let current = claude_current_quota(cap, row, window_kind, now)?;
            let resets_at = current.resets_at?;
            let cohort = cohorts.get(&(cap.plan.clone(), *duration))?;
            let capacity = claude_cohort_estimate(cohort).capacity_per_sub_nano?;
            let unused = i128::from(
                100_000_000i64.saturating_sub(current.used_fraction_units.clamp(0, 100_000_000)),
            );
            let remaining = capacity
                .checked_mul(unused)?
                .checked_add(CLAUDE_FRACTION_SCALE / 2)?
                .checked_div(CLAUDE_FRACTION_SCALE)?;
            let reset_capacity = capacity.checked_mul(claude_reset_count(
                now,
                resets_at,
                duration.saturating_mul(60),
                horizon_secs,
            ))?;
            remaining.checked_add(reset_capacity)
        })
        .collect::<Option<Vec<_>>>()?;
    availability.into_iter().min()
}

fn anthropic_calibration_aggregate_value(
    row: &registry::ProviderTurnCalibrationAggregate,
) -> Value {
    json!({
        "email": mask_claude_email(&row.subject_id),
        "model": row.model_id,
        "service_tier": row.service_tier,
        "inference_geo": row.inference_geo,
        "tariff_schedule_id": row.tariff_schedule_id,
        "turns": row.turns,
        "first_completed_at": row.first_completed_at,
        "last_completed_at": row.last_completed_at,
        "input_tokens": row.input_tokens.to_string(),
        "audio_input_tokens": row.audio_input_tokens.to_string(),
        "cache_read_tokens": row.cache_read_tokens.to_string(),
        "cached_audio_input_tokens": row.cached_audio_input_tokens.to_string(),
        "cache_write_5m_tokens": row.cache_write_5m_tokens.to_string(),
        "cache_write_1h_tokens": row.cache_write_1h_tokens.to_string(),
        "output_tokens": row.output_tokens.to_string(),
        "thinking_output_tokens": row.thinking_output_tokens.to_string(),
        "image_output_tokens": row.image_output_tokens.to_string(),
        "tool_prompt_tokens": row.tool_prompt_tokens.to_string(),
        "search_queries": row.search_queries.to_string(),
        "grounded_search_prompts": row.grounded_search_prompts.to_string(),
        "api_input_nanousd": row.api_input_nanousd.to_string(),
        "api_audio_input_nanousd": row.api_audio_input_nanousd.to_string(),
        "api_cache_read_nanousd": row.api_cache_read_nanousd.to_string(),
        "api_cached_audio_input_nanousd": row.api_cached_audio_input_nanousd.to_string(),
        "api_cache_write_5m_nanousd": row.api_cache_write_5m_nanousd.to_string(),
        "api_cache_write_1h_nanousd": row.api_cache_write_1h_nanousd.to_string(),
        "api_output_nanousd": row.api_output_nanousd.to_string(),
        "api_image_output_nanousd": row.api_image_output_nanousd.to_string(),
        "api_search_nanousd": row.api_search_nanousd.to_string(),
        "api_total_nanousd": row.api_total_nanousd.to_string(),
    })
}

fn anthropic_calibration_event_value(row: &registry::ProviderTurnCalibrationEvent) -> Value {
    json!({
        "request_id": row.request_id,
        "email": mask_claude_email(&row.subject_id),
        "model": row.model_id,
        "service_tier": row.service_tier,
        "inference_geo": row.inference_geo,
        "tariff_schedule_id": row.tariff_schedule_id,
        "priced_ts": row.priced_ts,
        "completed_at": row.completed_at,
        "input_tokens": row.input_tokens.to_string(),
        "audio_input_tokens": row.audio_input_tokens.to_string(),
        "cache_read_tokens": row.cache_read_tokens.to_string(),
        "cached_audio_input_tokens": row.cached_audio_input_tokens.to_string(),
        "cache_write_5m_tokens": row.cache_write_5m_tokens.to_string(),
        "cache_write_1h_tokens": row.cache_write_1h_tokens.to_string(),
        "output_tokens": row.output_tokens.to_string(),
        "thinking_output_tokens": row.thinking_output_tokens.to_string(),
        "image_output_tokens": row.image_output_tokens.to_string(),
        "tool_prompt_tokens": row.tool_prompt_tokens.to_string(),
        "search_queries": row.search_queries.to_string(),
        "grounded_search_prompts": row.grounded_search_prompts.to_string(),
        "api_input_nanousd": row.api_input_nanousd.to_string(),
        "api_audio_input_nanousd": row.api_audio_input_nanousd.to_string(),
        "api_cache_read_nanousd": row.api_cache_read_nanousd.to_string(),
        "api_cached_audio_input_nanousd": row.api_cached_audio_input_nanousd.to_string(),
        "api_cache_write_5m_nanousd": row.api_cache_write_5m_nanousd.to_string(),
        "api_cache_write_1h_nanousd": row.api_cache_write_1h_nanousd.to_string(),
        "api_output_nanousd": row.api_output_nanousd.to_string(),
        "api_image_output_nanousd": row.api_image_output_nanousd.to_string(),
        "api_search_nanousd": row.api_search_nanousd.to_string(),
        "api_total_nanousd": row.api_total_nanousd.to_string(),
    })
}

/// Exact Claude capacity from durable request spend and fixed-point provider quota observations.
/// No old pool prior, EMA or floating-point calibration is allowed into this response.
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
    let report = match &app.billing {
        Some(billing) => match billing.anthropic_calibration_report().await {
            Ok(report) => Some(report),
            Err(error) => {
                eprintln!("Anthropic calibration report unavailable: {error:#}");
                None
            }
        },
        None => None,
    };
    let delivery = app
        .billing
        .as_ref()
        .map(|billing| billing.anthropic_calibration_delivery_status());
    let caps = app.pool.capacity();
    let v = capacity_value(&caps, report.as_ref(), delivery, pool::now());
    cache_put(&CAPACITY_CACHE, &v);
    Json(v).into_response()
}

pub(crate) fn capacity_value(
    caps: &[pool::Cap],
    report: Option<&(
        Vec<registry::AnthropicCalibrationRow>,
        Vec<registry::ProviderTurnCalibrationAggregate>,
        Vec<registry::ProviderTurnCalibrationEvent>,
    )>,
    delivery: Option<forward::AnthropicCalibrationDeliveryStatus>,
    now: i64,
) -> serde_json::Value {
    let rows = report.map_or(&[][..], |(rows, _, _)| rows.as_slice());
    let delivery_missing_reason = match delivery {
        Some(status) if status.pending_events > 0 => Some("calibration_delivery_pending"),
        Some(status) if !status.persistence_ok => Some("calibration_delivery_degraded"),
        Some(_) => None,
        None => Some("calibration_delivery_unavailable"),
    };
    let delivery_ready = delivery_missing_reason.is_none();
    let cohorts = claude_plan_window_cohorts(caps, rows, now);
    let horizons = [3_600, 18_000, 86_400, 604_800];
    let available = horizons.map(|horizon| {
        let routable = caps.iter().filter(|cap| cap.routable).collect::<Vec<_>>();
        if report.is_none() || !delivery_ready || routable.is_empty() {
            return None;
        }
        routable.iter().try_fold(0i128, |total, cap| {
            claude_sub_horizon_available(cap, rows, &cohorts, now, horizon)
                .and_then(|value| total.checked_add(value))
        })
    });
    let subs = caps
        .iter()
        .map(|cap| {
            let window = |window_kind: &str, duration: i64| {
                let row = claude_row(rows, &cap.email, &cap.plan, window_kind);
                let current = delivery_ready
                    .then(|| claude_current_quota(cap, row, window_kind, now))
                    .flatten();
                let last_known = delivery_ready
                    .then(|| claude_last_known_quota(cap, row, window_kind, now))
                    .flatten();
                let retained_reset = current.is_none().then(|| {
                    claude_last_known_reset(cap, row, window_kind)
                        .filter(|resets_at| *resets_at > now)
                }).flatten();
                let retained = current
                    .is_none()
                    .then_some(last_known)
                    .flatten()
                    .filter(|_| retained_reset.is_some());
                let displayed = current.or(retained);
                let cohort = cohorts.get(&(cap.plan.clone(), duration));
                let estimate = cohort.map(claude_cohort_estimate).unwrap_or_default();
                let remaining = current.and_then(|current| {
                    estimate.capacity_per_sub_nano.and_then(|capacity| {
                        capacity
                            .checked_mul(i128::from(100_000_000i64.saturating_sub(
                                current.used_fraction_units.clamp(0, 100_000_000),
                            )))
                            .and_then(|value| value.checked_add(CLAUDE_FRACTION_SCALE / 2))
                            .and_then(|value| value.checked_div(CLAUDE_FRACTION_SCALE))
                    })
                });
                let remaining_bound = |bound: Option<i128>| {
                    current.and_then(|current| {
                        bound.and_then(|capacity| {
                            capacity
                                .checked_mul(i128::from(100_000_000i64.saturating_sub(
                                    current.used_fraction_units.clamp(0, 100_000_000),
                                )))
                                .and_then(|value| value.checked_add(CLAUDE_FRACTION_SCALE / 2))
                                .and_then(|value| value.checked_div(CLAUDE_FRACTION_SCALE))
                        })
                    })
                };
                let last_known_remaining = retained.and_then(|last_known| {
                    estimate.capacity_per_sub_nano.and_then(|capacity| {
                        capacity
                            .checked_mul(i128::from(100_000_000i64.saturating_sub(
                                last_known.used_fraction_units.clamp(0, 100_000_000),
                            )))
                            .and_then(|value| value.checked_add(CLAUDE_FRACTION_SCALE / 2))
                            .and_then(|value| value.checked_div(CLAUDE_FRACTION_SCALE))
                    })
                });
                let missing_reason = if report.is_none() {
                    Some("calibration_authority_unavailable")
                } else if estimate.capacity_per_sub_nano.is_none() {
                    Some("awaiting_plan_evidence")
                } else if let Some(reason) = delivery_missing_reason {
                    Some(reason)
                } else if row.is_none() {
                    if current.is_none() {
                        Some("missing_current_quota_snapshot")
                    } else {
                        None
                    }
                } else if current.is_none() {
                    Some("stale_current_quota_snapshot")
                } else {
                    None
                };
                json!({
                    "window_kind": window_kind,
                    "window_minutes": duration,
                    "resets_at": current.and_then(|current| current.resets_at).or(retained_reset),
                    "observed_at": displayed.map(|quota| quota.observed_at).or_else(|| row.map(|row| row.observed_at)),
                    "data_age_seconds": displayed
                        .map(|quota| now.saturating_sub(quota.observed_at).max(0))
                        .or_else(|| row.map(|row| now.saturating_sub(row.observed_at).max(0))),
                    "snapshot_fresh": current.is_some(),
                    "used_fraction_units": displayed.map(|quota| quota.used_fraction_units),
                    "measurement_resolution_fraction_units": displayed.map(|quota| quota.measurement_resolution_fraction_units),
                    "current_quota_source": current.map(|current| current.source),
                    "last_known_quota_source": retained.map(|quota| quota.source),
                    "capacity_nano": nano_string(estimate.capacity_per_sub_nano),
                    "remaining_nano": nano_string(remaining),
                    "last_known_remaining_nano": nano_string(last_known_remaining),
                    "low_nano": nano_string(estimate.low_per_sub_nano),
                    "high_nano": nano_string(estimate.high_per_sub_nano),
                    "remaining_low_nano": nano_string(remaining_bound(estimate.low_per_sub_nano)),
                    "remaining_high_nano": nano_string(remaining_bound(estimate.high_per_sub_nano)),
                    "confidence_bp": cohort.and_then(|cohort| cohort.confidence_bp),
                    "cohort_samples": cohort.map(|cohort| cohort.samples.to_string()),
                    "cohort_observed_fraction_units": cohort.map(|cohort| cohort.observed_fraction_units.to_string()),
                    "cohort_observed_spend_nano": cohort.map(|cohort| cohort.observed_spend_nano.to_string()),
                    "account_samples": row.map(|row| row.samples),
                    "account_observed_fraction_units": row.map(|row| row.observed_fraction_units),
                    "account_observed_spend_nano": row.map(|row| row.observed_spend_nano.to_string()),
                    "unattributed_fraction_units": row.map(|row| row.unattributed_fraction_units),
                    "source": if estimate.capacity_per_sub_nano.is_some() {
                        "plan_pooled_workload_api_equivalent"
                    } else {
                        "unknown"
                    },
                    "same_plan_capacity": true,
                    "missing_reason": missing_reason,
                })
            };
            let five = window("5h", 300);
            let weekly = window("7d", 10_080);
            let capacity_nano = |value: &Value| value["capacity_nano"].as_str().map(str::to_owned);
            let remaining_nano = |value: &Value| value["remaining_nano"].as_str().map(str::to_owned);
            let five_capacity = capacity_nano(&five);
            let weekly_capacity = capacity_nano(&weekly);
            let five_remaining = remaining_nano(&five);
            let weekly_remaining = remaining_nano(&weekly);
            let fraction = |value: &Value, fallback: f64| {
                value["used_fraction_units"]
                    .as_i64()
                    .map_or(fallback, |units| units as f64 / 100_000_000.0)
            };
            let reset_in = |value: &Value| {
                value["resets_at"]
                    .as_i64()
                    .map(|resets_at| resets_at.saturating_sub(now).max(0))
            };
            let sub_available = horizons.map(|horizon| {
                claude_sub_horizon_available(cap, rows, &cohorts, now, horizon)
            });
            json!({
                "email": mask_claude_email(&cap.email),
                "plan": cap.plan,
                "calibrated": five_capacity.is_some() && weekly_capacity.is_some(),
                "calibration_source": "durable_plan_pooled_workload",
                "routable": cap.routable,
                "util5h": fraction(&five, cap.util5h),
                "util7d": fraction(&weekly, cap.util7d),
                "reset5h_in": reset_in(&five),
                "reset7d_in": reset_in(&weekly),
                "cap5h_nano": five_capacity,
                "cap7d_nano": weekly_capacity,
                "rem5h_nano": five_remaining,
                "rem7d_nano": weekly_remaining,
                "available_nano": {
                    "next_1h": nano_string(sub_available[0]),
                    "next_5h": nano_string(sub_available[1]),
                    "next_1d": nano_string(sub_available[2]),
                    "next_7d": nano_string(sub_available[3]),
                },
                "windows": [five, weekly],
                "status": cap.status,
                "cooling": cap.cooling,
                "dead": cap.auth_dead,
                "auth_state": cap.auth_state,
                "dead_reason": cap.dead_reason,
                "dead_since": cap.dead_since_ts,
            })
        })
        .collect::<Vec<_>>();
    let window_totals =
        claude_window_total_values(&cohorts, report.is_some(), delivery_missing_reason);
    let fully_calibrated = window_totals
        .iter()
        .all(|window| window["capacity_nano"].is_string() && window["remaining_nano"].is_string());
    json!({
        "now": now,
        "subs": subs.len(),
        "dead": caps.iter().filter(|cap| cap.auth_dead).count(),
        "suspect": caps.iter().filter(|cap| cap.auth_state == "suspect").count(),
        "calibrated": fully_calibrated,
        "calibration_authority_available": report.is_some(),
        "calibration_delivery": delivery.map(|status| json!({
            "pending_events": status.pending_events,
            "dropped_events": status.dropped_events,
            "persistence_ok": status.persistence_ok,
            "queue_limit": status.queue_limit,
        })),
        "available_nano": {
            "next_1h": nano_string(available[0]),
            "next_5h": nano_string(available[1]),
            "next_1d": nano_string(available[2]),
            "next_7d": nano_string(available[3]),
        },
        "capacity_semantics": {
            "kind": "plan_pooled_realized_workload_api_equivalent",
            "formula": "100000000 * sum(observed_spend_nano) / sum(observed_fraction_units)",
            "fixed_subscription_nominal": false,
            "legacy_pool_prior_authoritative": false,
            "same_plan_capacity": true,
            "fleet_totals_fail_closed": true,
            "source": "durable_provider_turns_and_exact_quota_fractions",
        },
        "plan_cohorts": claude_plan_cohort_values(
            &cohorts,
            report.is_some(),
            delivery_missing_reason,
        ),
        "window_totals": window_totals,
        "conversion_models": anthropic_conversion_models(now),
        "calibration_evidence": report.map_or_else(Vec::new, |(_, evidence, _)| {
            evidence.iter().map(anthropic_calibration_aggregate_value).collect::<Vec<_>>()
        }),
        "calibration_recent_turn_limit": registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
        "calibration_recent_turns": report.map_or_else(Vec::new, |(_, _, events)| {
            events.iter().map(anthropic_calibration_event_value).collect::<Vec<_>>()
        }),
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
    let (mut cooling, mut dead, mut suspect) = (0usize, 0usize, 0usize);
    for c in &caps {
        if c.auth_dead {
            dead += 1;
        } else if c.cooling {
            cooling += 1;
        } else if c.auth_state == "suspect" {
            suspect += 1;
        }
    }

    // `/overview` used to sum pool::Cap EMA/prior floats. It now derives every capacity-facing
    // field from the exact `/capacity` authority; the old pool values remain routing hints only.
    let (calibration_report, delivery) = match &app.billing {
        Some(billing) => (
            match billing.anthropic_calibration_report().await {
                Ok(report) => Some(report),
                Err(error) => {
                    eprintln!("Anthropic overview calibration report unavailable: {error:#}");
                    None
                }
            },
            Some(billing.anthropic_calibration_delivery_status()),
        ),
        None => (None, None),
    };
    let exact = capacity_value(&caps, calibration_report.as_ref(), delivery, pool::now());
    let nano = |pointer: &str| {
        exact
            .pointer(pointer)
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i128>().ok())
    };
    let a1 = nano("/available_nano/next_1h");
    let a5 = nano("/available_nano/next_5h");
    let a1d = nano("/available_nano/next_1d");
    let a7d = nano("/available_nano/next_7d");
    let cap5 = nano("/window_totals/0/capacity_nano");
    let rem5 = nano("/window_totals/0/remaining_nano");
    let cap7 = nano("/window_totals/1/capacity_nano");
    let rem7 = nano("/window_totals/1/remaining_nano");
    let consumed = |capacity: Option<i128>, remaining: Option<i128>| {
        capacity
            .zip(remaining)
            .and_then(|(capacity, remaining)| capacity.checked_sub(remaining))
            .map(|value| value.max(0))
    };
    let cons5 = consumed(cap5, rem5);
    let cons7 = consumed(cap7, rem7);
    let usd = |value: Option<i128>| value.map(|value| value as f64 / 1e9);
    let rounded_usd = |value: Option<i128>| usd(value).map(r2);
    let nano_json = |value: Option<i128>| value.map(|value| value.to_string());
    let utilization = |capacity: Option<i128>, remaining: Option<i128>| {
        capacity.zip(remaining).and_then(|(capacity, remaining)| {
            (capacity > 0).then(|| r2((capacity - remaining).max(0) as f64 / capacity as f64))
        })
    };
    let headroom = |available: Option<i128>, consumed: Option<i128>| {
        available.zip(consumed).and_then(|(available, consumed)| {
            (consumed > 10_000_000).then(|| r2(available as f64 / consumed as f64))
        })
    };
    let need_for_window = |capacity: Option<i128>, consumed: Option<i128>| {
        capacity.zip(consumed).and_then(|(capacity, consumed)| {
            (usable > 0 && capacity > 0).then(|| {
                let per_sub = capacity as f64 / usable as f64;
                (consumed as f64 * TARGET_HEADROOM / per_sub).ceil() as i64
            })
        })
    };
    let need = need_for_window(cap5, cons5)
        .zip(need_for_window(cap7, cons7))
        .map(|(five, weekly)| five.max(weekly).max(usable.min(1) as i64));
    let gap = need.map(|need| need - usable as i64);
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
    let coverage7 =
        usd(a7d).and_then(|available| (available > 0.01).then(|| r2(real_demand / available))); // >1 = потенциально перепродали
    Ok(json!({
        "now": exact["now"],
        "subs": n, "calibrated": exact["calibrated"], "ref_mult": REF_MULT, "target_headroom": TARGET_HEADROOM,
        "supply": {
            "authority": "exact_provider_turns_and_quota_fractions",
            "legacy_pool_prior_authoritative": false,
            "calibration_delivery": exact["calibration_delivery"],
            "avail_nano":   {"1h": nano_json(a1), "5h": nano_json(a5), "1d": nano_json(a1d), "7d": nano_json(a7d)},
            "cap_nano":     {"5h": nano_json(cap5), "7d": nano_json(cap7)},
            "consumed_nano":{"5h": nano_json(cons5), "7d": nano_json(cons7)},
            "avail_usd":    {"1h": rounded_usd(a1), "5h": rounded_usd(a5), "1d": rounded_usd(a1d), "7d": rounded_usd(a7d)},
            "cap_usd":      {"5h": rounded_usd(cap5), "7d": rounded_usd(cap7)},
            "consumed_usd": {"5h": rounded_usd(cons5), "7d": rounded_usd(cons7)},
            "util":         {"5h": utilization(cap5, rem5), "7d": utilization(cap7, rem7)},
            "health":       {"healthy": caps.iter().filter(|c| c.routable && c.auth_state == "healthy").count(),
                               "suspect": suspect, "cooling": cooling, "dead": dead,
                               "usable": usable, "total": n},
        },
        "demand": {"balance_usd": r2(bal), "reserved_usd": r2(res), "spent_usd": r2(spent), "active_accounts": keys,
                   "potential_realapi_usd": r2(real_demand)},
        "headroom": {"5h": headroom(a5, cons5), "7d": headroom(a7d, cons7)},
        "coverage": {"7d": coverage7},
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
            let sub_expire = if s.added_ts > 0 {
                s.added_ts + lifetime
            } else {
                0
            };
            let (pk5, pk7) = peak.get(&s.email).copied().unwrap_or((0.0, 0.0));
            json!({
                "email": mask_claude_email(&s.email),
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
    let now = pool::now();
    let report = match &app.billing {
        Some(billing) => match billing.codex_calibration_report().await {
            Ok(report) => Some(report),
            Err(error) => {
                eprintln!("Codex calibration report unavailable: {error:#}");
                None
            }
        },
        None => None,
    };
    let mut value = codex_subs_value_with_report(&status, now, report.as_deref());
    value["conversion_models"] = Value::Array(codex_conversion_models(&codex.config().models, now));
    Json(value).into_response()
}

fn codex_window_value(c: &forward::codex::CodexWindowCapacityReport) -> Value {
    let round = |x: f64| (x * 100.0).round() / 100.0;
    let round_opt = |x: Option<f64>| x.map(round);
    let nano_opt = |x: Option<i64>| x.map(|value| value.to_string());
    json!({
        "slot": c.slot,
        "window_minutes": c.window_minutes,
        "resets_at": c.resets_at,
        "observed_at": c.observed_at,
        "data_age_seconds": c.data_age_seconds,
        "used_percent": c.used_percent,
        "used_fraction_units": c.used_fraction_units,
        "used_fraction": c.used_fraction_units as f64 / 100_000_000.0,
        "measurement_resolution_fraction_units": c.measurement_resolution_fraction_units,
        "capacity_nano": nano_opt(c.capacity_nano),
        "remaining_nano": nano_opt(c.remaining_nano),
        "low_nano": nano_opt(c.low_nano),
        "high_nano": nano_opt(c.high_nano),
        "remaining_low_nano": nano_opt(c.remaining_low_nano),
        "remaining_high_nano": nano_opt(c.remaining_high_nano),
        "cap_usd": round_opt(c.cap_usd),
        "remaining_usd": round_opt(c.remaining_usd),
        "low_usd": round_opt(c.low_usd),
        "high_usd": round_opt(c.high_usd),
        "remaining_low_usd": round_opt(c.remaining_low_usd),
        "remaining_high_usd": round_opt(c.remaining_high_usd),
        "capacity_nanocredits": nano_opt(c.capacity_nanocredits),
        "remaining_nanocredits": nano_opt(c.remaining_nanocredits),
        "low_nanocredits": nano_opt(c.low_nanocredits),
        "high_nanocredits": nano_opt(c.high_nanocredits),
        "remaining_low_nanocredits": nano_opt(c.remaining_low_nanocredits),
        "remaining_high_nanocredits": nano_opt(c.remaining_high_nanocredits),
        "observed_spend_nanocredits": nano_opt(c.observed_spend_nanocredits),
        "credit_samples": c.credit_samples,
        "unattributed_fraction_units": c.unattributed_fraction_units,
        "observed_spend_nano": c.observed_spend_nano.to_string(),
        "observed_fraction_units": c.observed_fraction_units,
        "workload_dependent": true,
        "source": c.source,
        "confidence": c.confidence,
        "samples": c.samples,
    })
}

fn codex_calibration_aggregate_value(row: &registry::CodexTurnCalibrationAggregate) -> Value {
    json!({
        "model": row.model_id,
        "service_tier": row.service_tier,
        "provider_reported_tier": row.provider_reported_tier,
        "api_tariff_schedule_id": row.api_tariff_schedule_id,
        "credit_schedule_id": row.credit_schedule_id,
        "turns": row.turns,
        "first_completed_at": row.first_completed_at,
        "last_completed_at": row.last_completed_at,
        "input_tokens": row.input_tokens.to_string(),
        "cached_input_tokens": row.cached_input_tokens.to_string(),
        "cache_write_input_tokens": row.cache_write_input_tokens.to_string(),
        "output_tokens": row.output_tokens.to_string(),
        "reasoning_output_tokens": row.reasoning_output_tokens.to_string(),
        "api_input_nanousd": row.api_input_nanousd.to_string(),
        "api_cached_input_nanousd": row.api_cached_input_nanousd.to_string(),
        "api_cache_write_nanousd": row.api_cache_write_nanousd.to_string(),
        "api_output_nanousd": row.api_output_nanousd.to_string(),
        "api_total_nanousd": row.api_total_nanousd.to_string(),
        "chatgpt_input_nanocredits": row.chatgpt_input_nanocredits.to_string(),
        "chatgpt_cached_input_nanocredits": row.chatgpt_cached_input_nanocredits.to_string(),
        "chatgpt_output_nanocredits": row.chatgpt_output_nanocredits.to_string(),
        "chatgpt_total_nanocredits": row.chatgpt_total_nanocredits.to_string(),
    })
}

fn codex_conversion_models(models: &[forward::codex::CodexModel], now: i64) -> Vec<Value> {
    models
        .iter()
        .filter_map(|model| {
            let tariff = metering::codex_tariff_capability_at(
                &model.id,
                now,
                metering::CodexServiceTier::Standard,
                0,
            )
            .ok()?;
            let credits = metering::codex_credit_rates(&model.id)?;
            let prices = tariff.prices;
            Some(json!({
                "id": model.id,
                "upstream": model.upstream,
                "api_tariff_schedule_id": tariff.tariff_schedule_id.as_str(),
                "credit_schedule_id": metering::CODEX_CREDIT_SCHEDULE_ID,
                "api": {
                    "input_nanousd_per_token": prices.input.to_string(),
                    "cached_input_nanousd_per_token": prices.cached_input.to_string(),
                    "cache_write_nanousd_per_token": prices.cache_write_input.to_string(),
                    "output_nanousd_per_token": prices.output.to_string(),
                    "fast_multiplier_basis_points": prices.api_fast_multiplier_basis_points,
                    "long_context_threshold": prices.long_context_threshold.to_string(),
                    "long_input_multiplier_basis_points": prices.long_input_basis_points,
                    "long_output_multiplier_basis_points": prices.long_output_basis_points,
                },
                "chatgpt_credits": {
                    "input_nanocredits_per_token": credits.input.to_string(),
                    "cached_input_nanocredits_per_token": credits.cached_input.to_string(),
                    "output_nanocredits_per_token": credits.output.to_string(),
                    "fast_multiplier_basis_points": model.fast_multiplier_basis_points,
                },
            }))
        })
        .collect()
}

#[cfg(test)]
fn codex_subs_value(status: &forward::codex::CodexOperationalStatus, now: i64) -> Value {
    codex_subs_value_with_report(status, now, None)
}

fn codex_subs_value_with_report(
    status: &forward::codex::CodexOperationalStatus,
    now: i64,
    report: Option<&[registry::CodexTurnCalibrationAggregate]>,
) -> Value {
    let round = |x: f64| (x * 100.0).round() / 100.0;
    let window = |w: &forward::codex::CodexRateLimitWindow| {
        json!({
            "used_percent": w.used_percent,
            "used_fraction_units": w.used_fraction_units,
            "used_fraction": w.used_fraction(),
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
                "email": h.masked_email,
                "plan": h.plan,
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
                "spend_nanocredits_total": h.spend_nanocredits_total.map(|value| value.to_string()),
                "credit_tracking_started_ts": h.credit_tracking_started_ts,
                "calibration_pending_events": h.calibration_pending_events,
                "calibration_dropped_events": h.calibration_dropped_events,
                "calibration_persistence_ok": h.calibration_persistence_ok,
                "rate_limits": h.rate_limits.as_ref().map(|rl| json!({
                    "reached": rl.reached,
                    "observed_at": rl.observed_at,
                    "primary": rl.primary.as_ref().map(window),
                    "secondary": rl.secondary.as_ref().map(window),
                })),
                "spend_nano_total": h.spend_nano_total.to_string(),
                "windows": h.capacities.iter().map(codex_window_value).collect::<Vec<_>>(),
                "fast_tiers": h.fast_tiers.iter().map(|tier| json!({
                    "model": tier.model,
                    "catalog_available": tier.catalog_available,
                    "catalog_fast_supported": tier.catalog_fast_supported,
                    "served_tier": tier.served_tier,
                    "provider_reported_tier": tier.provider_reported_tier,
                    "observed_at": tier.observed_at,
                })).collect::<Vec<_>>(),
                "calibration_evidence": report.unwrap_or_default().iter()
                    .filter(|row| row.home_id == h.id)
                    .map(codex_calibration_aggregate_value)
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    // Fleet totals use the real duration identity. Unknown estimates remain null per-home and do
    // not silently contribute zero or a configured prior to the measured aggregate.
    let totals: Vec<_> = codex_window_totals(status)
        .into_iter()
        .map(|(duration, total)| {
            let measured = total.measured_homes;
            let low_complete = measured > 0 && total.low_homes == measured;
            let high_complete = measured > 0 && total.high_homes == measured;
            let credit_measured = total.credit_measured_homes;
            let credit_low_complete = credit_measured > 0
                && total.credit_low_homes == credit_measured;
            let credit_high_complete = credit_measured > 0
                && total.credit_high_homes == credit_measured;
            json!({
                "window_minutes": duration,
                "capacity_nano": (measured > 0).then(|| total.capacity_nano.to_string()),
                "remaining_nano": (measured > 0).then(|| total.remaining_nano.to_string()),
                "low_nano": low_complete.then(|| total.low_nano.to_string()),
                "high_nano": high_complete.then(|| total.high_nano.to_string()),
                "remaining_low_nano": low_complete
                    .then(|| total.remaining_low_nano.to_string()),
                "remaining_high_nano": high_complete
                    .then(|| total.remaining_high_nano.to_string()),
                "cap_usd": (measured > 0)
                    .then(|| round(total.capacity_nano as f64 / 1e9)),
                "remaining_usd": (measured > 0)
                    .then(|| round(total.remaining_nano as f64 / 1e9)),
                "low_usd": low_complete.then(|| round(total.low_nano as f64 / 1e9)),
                "high_usd": high_complete.then(|| round(total.high_nano as f64 / 1e9)),
                "remaining_low_usd": low_complete
                    .then(|| round(total.remaining_low_nano as f64 / 1e9)),
                "remaining_high_usd": high_complete
                    .then(|| round(total.remaining_high_nano as f64 / 1e9)),
                "capacity_nanocredits": (credit_measured > 0)
                    .then(|| total.capacity_nanocredits.to_string()),
                "remaining_nanocredits": (credit_measured > 0)
                    .then(|| total.remaining_nanocredits.to_string()),
                "low_nanocredits": credit_low_complete.then(|| total.low_nanocredits.to_string()),
                "high_nanocredits": credit_high_complete.then(|| total.high_nanocredits.to_string()),
                "remaining_low_nanocredits": credit_low_complete
                    .then(|| total.remaining_low_nanocredits.to_string()),
                "remaining_high_nanocredits": credit_high_complete
                    .then(|| total.remaining_high_nanocredits.to_string()),
                "observed_spend_nanocredits": total.observed_spend_nanocredits.to_string(),
                "credit_measured_homes": credit_measured,
                "credit_observed_homes": total.credit_observed_homes,
                "unattributed_fraction_units": total.unattributed_fraction_units.to_string(),
                "observed_spend_nano": total.observed_spend_nano.to_string(),
                "observed_fraction_units": total.observed_fraction_units.to_string(),
                "measured_homes": measured,
                "observed_homes": total.observed_homes,
                "source": if measured > 0 { "workload_blend" } else { "unknown" },
                "workload_dependent": true,
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
        "calibration_evidence_available": report.is_some(),
        "credit_schedule_id": metering::CODEX_CREDIT_SCHEDULE_ID,
        "plan_cohorts": codex_plan_cohorts(status),
        "window_totals": totals,
        "homes": homes,
    })
}

fn gemini_conversion_models(models: &[forward::GeminiModel], now: i64) -> Vec<Value> {
    models
        .iter()
        .filter_map(|model| {
            let prices = metering::gemini_prices_at(&model.id, now)?;
            let (search_unit, search_nano) = match prices.search {
                metering::GeminiSearchBilling::PerQuery { nano } => ("query", nano),
                metering::GeminiSearchBilling::PerGroundedPrompt { nano } => {
                    ("grounded_prompt", nano)
                }
            };
            Some(json!({
                "id": model.id,
                "display_name": model.display_name,
                "tariff_schedule_id": metering::gemini::TARIFF_SCHEDULE_ID,
                "input_token_limit": model.input_token_limit.to_string(),
                "output_token_limit": model.output_token_limit.to_string(),
                "quota_model_ids": model.quota_model_ids(),
                "rates": {
                    "input_nanousd_per_token": prices.input.to_string(),
                    "audio_input_nanousd_per_token": prices.audio_input.to_string(),
                    "cached_input_nanousd_per_token": prices.cached_input.to_string(),
                    "cached_audio_input_nanousd_per_token": prices.cached_audio_input.to_string(),
                    "output_nanousd_per_token": prices.output.to_string(),
                    "image_output_nanousd_per_token": prices.image_output.to_string(),
                    "long_context_threshold": prices.long_context_threshold.to_string(),
                    "long_input_nanousd_per_token": prices.long_input.to_string(),
                    "long_audio_input_nanousd_per_token": prices.long_audio_input.to_string(),
                    "long_cached_input_nanousd_per_token": prices.long_cached_input.to_string(),
                    "long_cached_audio_input_nanousd_per_token": prices.long_cached_audio_input.to_string(),
                    "long_output_nanousd_per_token": prices.long_output.to_string(),
                },
                "search": {
                    "billing_unit": search_unit,
                    "nanousd_per_unit": search_nano.to_string(),
                },
            }))
        })
        .collect()
}

fn gemini_exact_calibration_value(row: &registry::GeminiExactCalibrationRow) -> Value {
    json!({
        "profile_id": row.profile_id,
        "plan": row.plan,
        "bucket_id": row.bucket_id,
        "window_kind": row.window_kind,
        "window_minutes": row.window_duration_mins,
        "resets_at": row.resets_at,
        "used_fraction_units": row.used_fraction_units,
        "measurement_resolution_fraction_units": row.measurement_resolution_fraction_units,
        "observed_at": row.observed_at,
        "observed_fraction_units": row.observed_fraction_units.to_string(),
        "observed_spend_nanousd": row.observed_spend_nano.to_string(),
        "samples": row.samples,
        "unattributed_fraction_units": row.unattributed_fraction_units.to_string(),
        "capacity_nanousd": row.current_capacity_nano.map(|value| value.to_string()),
        "low_nanousd": row.current_low_nano.map(|value| value.to_string()),
        "high_nanousd": row.current_high_nano.map(|value| value.to_string()),
        "confidence_bp": row.current_confidence_bp,
        "last_measured_at": row.last_measured_at,
        "estimator_version": row.estimator_version,
        "version": row.version,
        "updated_ts": row.updated_ts,
    })
}

fn gemini_calibration_aggregate_value(row: &registry::ProviderTurnCalibrationAggregate) -> Value {
    json!({
        "profile_id": row.subject_id,
        "model": row.model_id,
        "service_tier": row.service_tier,
        "inference_geo": row.inference_geo,
        "tariff_schedule_id": row.tariff_schedule_id,
        "turns": row.turns,
        "first_completed_at": row.first_completed_at,
        "last_completed_at": row.last_completed_at,
        "input_tokens": row.input_tokens.to_string(),
        "audio_input_tokens": row.audio_input_tokens.to_string(),
        "cache_read_tokens": row.cache_read_tokens.to_string(),
        "cached_audio_input_tokens": row.cached_audio_input_tokens.to_string(),
        "cache_write_5m_tokens": row.cache_write_5m_tokens.to_string(),
        "cache_write_1h_tokens": row.cache_write_1h_tokens.to_string(),
        "output_tokens": row.output_tokens.to_string(),
        "thinking_output_tokens": row.thinking_output_tokens.to_string(),
        "image_output_tokens": row.image_output_tokens.to_string(),
        "tool_prompt_tokens": row.tool_prompt_tokens.to_string(),
        "search_queries": row.search_queries.to_string(),
        "grounded_search_prompts": row.grounded_search_prompts.to_string(),
        "api_input_nanousd": row.api_input_nanousd.to_string(),
        "api_audio_input_nanousd": row.api_audio_input_nanousd.to_string(),
        "api_cache_read_nanousd": row.api_cache_read_nanousd.to_string(),
        "api_cached_audio_input_nanousd": row.api_cached_audio_input_nanousd.to_string(),
        "api_cache_write_5m_nanousd": row.api_cache_write_5m_nanousd.to_string(),
        "api_cache_write_1h_nanousd": row.api_cache_write_1h_nanousd.to_string(),
        "api_output_nanousd": row.api_output_nanousd.to_string(),
        "api_image_output_nanousd": row.api_image_output_nanousd.to_string(),
        "api_search_nanousd": row.api_search_nanousd.to_string(),
        "api_total_nanousd": row.api_total_nanousd.to_string(),
    })
}

fn gemini_calibration_event_value(row: &registry::ProviderTurnCalibrationEvent) -> Value {
    let mut value =
        gemini_calibration_aggregate_value(&registry::ProviderTurnCalibrationAggregate {
            provider: row.provider.clone(),
            subject_id: row.subject_id.clone(),
            model_id: row.model_id.clone(),
            service_tier: row.service_tier.clone(),
            inference_geo: row.inference_geo.clone(),
            tariff_schedule_id: row.tariff_schedule_id.clone(),
            turns: 1,
            first_completed_at: row.completed_at,
            last_completed_at: row.completed_at,
            input_tokens: row.input_tokens,
            audio_input_tokens: row.audio_input_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cached_audio_input_tokens: row.cached_audio_input_tokens,
            cache_write_5m_tokens: row.cache_write_5m_tokens,
            cache_write_1h_tokens: row.cache_write_1h_tokens,
            output_tokens: row.output_tokens,
            thinking_output_tokens: row.thinking_output_tokens,
            image_output_tokens: row.image_output_tokens,
            tool_prompt_tokens: row.tool_prompt_tokens,
            search_queries: row.search_queries,
            grounded_search_prompts: row.grounded_search_prompts,
            api_input_nanousd: row.api_input_nanousd,
            api_audio_input_nanousd: row.api_audio_input_nanousd,
            api_cache_read_nanousd: row.api_cache_read_nanousd,
            api_cached_audio_input_nanousd: row.api_cached_audio_input_nanousd,
            api_cache_write_5m_nanousd: row.api_cache_write_5m_nanousd,
            api_cache_write_1h_nanousd: row.api_cache_write_1h_nanousd,
            api_output_nanousd: row.api_output_nanousd,
            api_image_output_nanousd: row.api_image_output_nanousd,
            api_search_nanousd: row.api_search_nanousd,
            api_total_nanousd: row.api_total_nanousd,
        });
    let object = value
        .as_object_mut()
        .expect("Gemini calibration aggregate serializer returns object");
    object.insert("request_id".to_owned(), json!(row.request_id));
    object.insert("priced_ts".to_owned(), json!(row.priced_ts));
    object.insert("completed_at".to_owned(), json!(row.completed_at));
    object.remove("turns");
    object.remove("first_completed_at");
    object.remove("last_completed_at");
    value
}

/// Gemini paid-subscription fleet status for the unified panel. This route exists only on the
/// fixed Gemini runtime and contains opaque profile ids plus sanitized quota/transport metadata;
/// Google subject, full email, project, OAuth and proxy values never enter the response. A bounded
/// four-character local-part hint is included for operator matching, like `/codex-subs`.
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
    let calibration_report = match &app.billing {
        Some(billing) => match billing.gemini_calibration_report().await {
            Ok(report) => Some(report),
            Err(error) => {
                eprintln!("Gemini calibration report unavailable: {error:#}");
                None
            }
        },
        None => None,
    };
    let calibration_delivery = app
        .billing
        .as_ref()
        .map(|billing| billing.gemini_calibration_delivery_status());
    let status = gemini.operational_status().await;
    let calibration_persistence_ok =
        gemini_calibration_persistence_ok(&status, calibration_delivery);
    let capacity_available = calibration_report.is_some() && calibration_persistence_ok;
    let affinity = app.affinity.stats();
    let profiles = gemini_profile_values(&status, capacity_available, now);
    let models = status
        .models
        .iter()
        .map(|model| {
            let quota_model_ids = gemini
                .config()
                .models
                .iter()
                .find(|configured| configured.id == model.id)
                .map(|configured| configured.quota_model_ids())
                .unwrap_or_default();
            json!({
                "id": model.id,
                "quota_model_ids": quota_model_ids,
                "available": model.available,
                "healthy": model.healthy,
                "degraded": model.degraded,
                "unknown": model.unknown,
                "soonest_ready": model.soonest_ready,
            })
        })
        .collect::<Vec<_>>();
    let window_totals = gemini_window_total_values(&status, capacity_available, now);
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
        "calibration_authority_available": calibration_report.is_some(),
        "calibration_delivery": calibration_delivery.map(|status| json!({
            "pending_events": status.pending_events,
            "dropped_events": status.dropped_events,
            "persistence_ok": calibration_persistence_ok,
            "queue_limit": status.queue_limit,
        })),
        "calibration_windows": calibration_report.as_ref().map_or_else(Vec::new, |(rows, _, _)| {
            rows.iter().map(gemini_exact_calibration_value).collect::<Vec<_>>()
        }),
        "calibration_evidence": calibration_report.as_ref().map_or_else(Vec::new, |(_, rows, _)| {
            rows.iter().map(gemini_calibration_aggregate_value).collect::<Vec<_>>()
        }),
        "calibration_recent_turn_limit": registry::MAX_RECENT_PROVIDER_TURN_CALIBRATION_EVENTS,
        "calibration_recent_turns": calibration_report.as_ref().map_or_else(Vec::new, |(_, _, rows)| {
            rows.iter().map(gemini_calibration_event_value).collect::<Vec<_>>()
        }),
        "window_totals": window_totals,
        "conversion_models": gemini_conversion_models(&gemini.config().models, now),
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

fn gemini_calibration_persistence_ok(
    status: &forward::GeminiOperationalStatus,
    delivery: Option<forward::GeminiCalibrationDeliveryStatus>,
) -> bool {
    delivery.is_some_and(|delivery| {
        delivery.persistence_ok && delivery.pending_events == 0 && delivery.dropped_events == 0
    }) && status
        .profiles
        .iter()
        .all(|profile| profile.calibration_persistence_ok)
}

fn gemini_profile_values(
    status: &forward::GeminiOperationalStatus,
    capacity_available: bool,
    now: i64,
) -> Vec<Value> {
    let round = |x: f64| (x * 1_000_000.0).round() / 1_000_000.0;
    let round_opt = |x: Option<f64>| x.map(round);
    status
        .profiles
        .iter()
        .map(|profile| {
            let profile_capacity_available =
                capacity_available && gemini_profile_routable(profile, now);
            json!({
                "id": profile.id,
                "email": profile.masked_email,
                "plan": profile.plan,
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
                    "capacity_nano": window.capacity_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "remaining_nano": window.remaining_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "low_nano": window.low_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "high_nano": window.high_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "remaining_low_nano": window.remaining_low_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "remaining_high_nano": window.remaining_high_nano.filter(|_| profile_capacity_available).map(|value| value.to_string()),
                    "cap_usd": round_opt(window.cap_usd.filter(|_| profile_capacity_available)),
                    "remaining_usd": round_opt(window.remaining_usd.filter(|_| profile_capacity_available)),
                    "low_usd": round_opt(window.low_usd.filter(|_| profile_capacity_available)),
                    "high_usd": round_opt(window.high_usd.filter(|_| profile_capacity_available)),
                    "remaining_low_usd": round_opt(window.remaining_low_usd.filter(|_| profile_capacity_available)),
                    "remaining_high_usd": round_opt(window.remaining_high_usd.filter(|_| profile_capacity_available)),
                    "observed_spend_nano": window.observed_spend_nano.to_string(),
                    "observed_spend_usd": round(window.observed_spend_nano as f64 / 1e9),
                    "observed_fraction_units": window.observed_fraction_units,
                    "workload_dependent": true,
                    "source": if profile_capacity_available { window.source } else { "unknown" },
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
                    "remaining_amount": quota.remaining_amount.map(|value| value.to_string()),
                    "remaining_fraction": quota.remaining_fraction,
                    "reset_time": quota.reset_time,
                    "token_type": quota.token_type,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn gemini_window_total_values(
    status: &forward::GeminiOperationalStatus,
    capacity_available: bool,
    now: i64,
) -> Vec<Value> {
    let round = |x: f64| (x * 1_000_000.0).round() / 1_000_000.0;
    let usd = |nano: i128| round(nano as f64 / 1e9);
    gemini_window_totals(status, now)
        .into_iter()
        .map(|(duration, total)| {
            let measured = if capacity_available {
                total.measured_profiles
            } else {
                0
            };
            let low_complete = measured > 0 && total.low_profiles == measured;
            let high_complete = measured > 0 && total.high_profiles == measured;
            json!({
                "window_minutes": duration,
                "capacity_nano": (measured > 0).then(|| total.capacity_nano.to_string()),
                "remaining_nano": (measured > 0).then(|| total.remaining_nano.to_string()),
                "low_nano": low_complete.then(|| total.low_nano.to_string()),
                "high_nano": high_complete.then(|| total.high_nano.to_string()),
                "remaining_low_nano": low_complete
                    .then(|| total.remaining_low_nano.to_string()),
                "remaining_high_nano": high_complete
                    .then(|| total.remaining_high_nano.to_string()),
                "cap_usd": (measured > 0).then(|| usd(total.capacity_nano)),
                "remaining_usd": (measured > 0).then(|| usd(total.remaining_nano)),
                "low_usd": low_complete.then(|| usd(total.low_nano)),
                "high_usd": high_complete.then(|| usd(total.high_nano)),
                "remaining_low_usd": low_complete.then(|| usd(total.remaining_low_nano)),
                "remaining_high_usd": high_complete.then(|| usd(total.remaining_high_nano)),
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
                masked_email: "owne…".to_string(),
                plan: "chatgpt_pro".to_string(),
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
                spend_nano_total: 12_500_000_000,
                spend_usd_total: 12.5,
                spend_nanocredits_total: Some(1_250_000_000),
                credit_tracking_started_ts: Some(90),
                calibration_pending_events: 0,
                calibration_dropped_events: 0,
                calibration_persistence_ok: true,
                capacities: vec![forward::codex::CodexWindowCapacityReport {
                    slot: "primary",
                    window_minutes: Some(300),
                    resets_at: Some(2_000_000_000),
                    observed_at: 100,
                    data_age_seconds: Some(5),
                    used_fraction_units: 40_000_000,
                    used_percent: 40,
                    measurement_resolution_fraction_units: 1_000_000,
                    capacity_nano: None,
                    remaining_nano: None,
                    low_nano: None,
                    high_nano: None,
                    remaining_low_nano: None,
                    remaining_high_nano: None,
                    cap_usd: None,
                    remaining_usd: None,
                    low_usd: None,
                    high_usd: None,
                    remaining_low_usd: None,
                    remaining_high_usd: None,
                    capacity_nanocredits: None,
                    remaining_nanocredits: None,
                    low_nanocredits: None,
                    high_nanocredits: None,
                    remaining_low_nanocredits: None,
                    remaining_high_nanocredits: None,
                    observed_spend_nanocredits: Some(0),
                    credit_samples: Some(0),
                    unattributed_fraction_units: Some(0),
                    observed_spend_nano: 0,
                    observed_fraction_units: 0,
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
                capacity_nano: None,
                remaining_nano: None,
                low_nano: None,
                high_nano: None,
                remaining_low_nano: None,
                remaining_high_nano: None,
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
                masked_email: "owne…".to_string(),
                plan: "google_ai_pro".to_string(),
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
        assert_eq!(value["homes"][0]["email"], "owne…");
        assert_eq!(value["homes"][0]["plan"], "chatgpt_pro");
        assert_eq!(value["homes"][0]["limit_reached"], false);
        assert_eq!(value["homes"][0]["spend_nanocredits_total"], "1250000000");
        assert_eq!(value["homes"][0]["credit_tracking_started_ts"], 90);
        assert_eq!(value["homes"][0]["calibration_pending_events"], 0);
        assert_eq!(value["homes"][0]["calibration_dropped_events"], 0);
        assert_eq!(value["calibration_evidence_available"], false);

        // A home the gateway refuses to route to must never read as active on an operator surface.
        status.homes[0].limit_reached = true;
        status.homes[0].capacities[0].used_percent = 100;
        status.homes[0].capacities[0].used_fraction_units = 100_000_000;
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
        assert_eq!(window["used_fraction_units"], 40_000_000);
        assert_eq!(window["used_fraction"], 0.4);
        assert_eq!(window["measurement_resolution_fraction_units"], 1_000_000);
        assert_eq!(window["workload_dependent"], true);
        assert!(window["capacity_nano"].is_null());
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
        let cohort = &value["plan_cohorts"][0];
        assert_eq!(cohort["plan"], "chatgpt_pro");
        assert_eq!(cohort["homes_total"], 1);
        assert_eq!(cohort["measured_homes"], 0);
        assert!(cohort["capacity_per_home_nanocredits"].is_null());
        assert_eq!(cohort["source"], "unknown");
    }

    #[test]
    fn codex_plan_cohort_pools_equal_plans_without_overwriting_home_evidence() {
        let mut status = unknown_codex_status();
        let first = &mut status.homes[0].capacities[0];
        first.capacity_nanocredits = Some(45_000_000_000_000);
        first.remaining_nanocredits = Some(27_000_000_000_000);
        first.low_nanocredits = Some(30_000_000_000_000);
        first.high_nanocredits = Some(90_000_000_000_000);
        first.remaining_low_nanocredits = Some(18_000_000_000_000);
        first.remaining_high_nanocredits = Some(54_000_000_000_000);
        first.observed_spend_nanocredits = Some(900_000_000_000);
        first.observed_fraction_units = 2_000_000;
        first.credit_samples = Some(1);

        let mut second_home = status.homes[0].clone();
        second_home.id = "home-2".into();
        second_home.masked_email = "seco…".into();
        let second = &mut second_home.capacities[0];
        second.used_fraction_units = 14_000_000;
        second.used_percent = 14;
        second.capacity_nanocredits = Some(60_000_000_000_000);
        second.remaining_nanocredits = Some(51_600_000_000_000);
        second.low_nanocredits = Some(40_000_000_000_000);
        second.high_nanocredits = Some(120_000_000_000_000);
        second.remaining_low_nanocredits = Some(34_400_000_000_000);
        second.remaining_high_nanocredits = Some(103_200_000_000_000);
        second.observed_spend_nanocredits = Some(1_800_000_000_000);
        second.observed_fraction_units = 3_000_000;
        second.credit_samples = Some(1);

        let mut unmeasured_home = status.homes[0].clone();
        unmeasured_home.id = "home-3".into();
        unmeasured_home.masked_email = "thir…".into();
        unmeasured_home.capacities[0].used_fraction_units = 5_000_000;
        unmeasured_home.capacities[0].used_percent = 5;
        unmeasured_home.capacities[0].capacity_nanocredits = None;
        unmeasured_home.capacities[0].remaining_nanocredits = None;
        unmeasured_home.capacities[0].low_nanocredits = None;
        unmeasured_home.capacities[0].high_nanocredits = None;
        unmeasured_home.capacities[0].remaining_low_nanocredits = None;
        unmeasured_home.capacities[0].remaining_high_nanocredits = None;
        unmeasured_home.capacities[0].observed_spend_nanocredits = Some(0);
        unmeasured_home.capacities[0].observed_fraction_units = 0;
        unmeasured_home.capacities[0].credit_samples = Some(0);
        let mut duplicate_slot = unmeasured_home.capacities[0].clone();
        duplicate_slot.slot = "secondary";
        unmeasured_home.capacities.push(duplicate_slot);

        let mut other_plan = status.homes[0].clone();
        other_plan.id = "home-plus".into();
        other_plan.plan = "chatgpt_plus".into();
        let plus = &mut other_plan.capacities[0];
        plus.capacity_nanocredits = Some(10_000_000_000_000);
        plus.remaining_nanocredits = Some(6_000_000_000_000);
        plus.low_nanocredits = Some(8_000_000_000_000);
        plus.high_nanocredits = Some(12_000_000_000_000);
        plus.remaining_low_nanocredits = Some(4_800_000_000_000);
        plus.remaining_high_nanocredits = Some(7_200_000_000_000);
        plus.observed_spend_nanocredits = Some(100_000_000_000);
        plus.observed_fraction_units = 1_000_000;
        plus.credit_samples = Some(1);

        status
            .homes
            .extend([second_home, unmeasured_home, other_plan]);
        let value = codex_subs_value(&status, 105);
        let cohorts = value["plan_cohorts"].as_array().unwrap();
        let pro = cohorts
            .iter()
            .find(|cohort| cohort["plan"] == "chatgpt_pro")
            .unwrap();
        assert_eq!(pro["window_minutes"], 300);
        assert_eq!(pro["homes_total"], 3);
        assert_eq!(pro["measured_homes"], 2);
        assert_eq!(pro["observed_fraction_units"], "5000000");
        assert_eq!(pro["observed_spend_nanocredits"], "2700000000000");
        assert_eq!(pro["capacity_per_home_nanocredits"], "54000000000000");
        assert_eq!(pro["capacity_per_home_low_nanocredits"], "30000000000000");
        assert_eq!(pro["capacity_per_home_high_nanocredits"], "120000000000000");
        assert_eq!(pro["fleet_capacity_nanocredits"], "162000000000000");
        assert_eq!(pro["fleet_remaining_nanocredits"], "130140000000000");
        assert_eq!(pro["source"], "plan_pooled_native_credits");
        assert_eq!(pro["same_plan_capacity"], true);
        assert_eq!(pro["workload_dependent"], false);

        assert_eq!(
            value["homes"][0]["windows"][0]["capacity_nanocredits"], "45000000000000",
            "the pooled plan answer must not overwrite immutable per-home evidence"
        );
        assert_eq!(
            value["homes"][1]["windows"][0]["capacity_nanocredits"],
            "60000000000000"
        );
        let plus = cohorts
            .iter()
            .find(|cohort| cohort["plan"] == "chatgpt_plus")
            .unwrap();
        assert_eq!(plus["homes_total"], 1);
        assert_eq!(plus["capacity_per_home_nanocredits"], "10000000000000");
    }

    #[test]
    fn codex_plan_cohort_keeps_upper_bound_unknown_if_any_evidence_is_one_sided() {
        let mut status = unknown_codex_status();
        let first = &mut status.homes[0].capacities[0];
        first.capacity_nanocredits = Some(45_000_000_000_000);
        first.remaining_nanocredits = Some(27_000_000_000_000);
        first.low_nanocredits = Some(30_000_000_000_000);
        first.high_nanocredits = None;
        first.observed_spend_nanocredits = Some(900_000_000_000);
        first.observed_fraction_units = 1_000_000;
        first.credit_samples = Some(1);

        let value = codex_subs_value(&status, 105);
        let cohort = &value["plan_cohorts"][0];
        assert_eq!(cohort["capacity_per_home_nanocredits"], "90000000000000");
        assert_eq!(
            cohort["capacity_per_home_low_nanocredits"],
            "30000000000000"
        );
        assert!(cohort["capacity_per_home_high_nanocredits"].is_null());
        assert!(cohort["fleet_remaining_high_nanocredits"].is_null());
    }

    #[test]
    fn codex_subscription_contract_publishes_exact_workload_capacity_and_remaining() {
        let mut status = unknown_codex_status();
        let capacity = &mut status.homes[0].capacities[0];
        capacity.capacity_nano = Some(2_450_041_880_000);
        capacity.remaining_nano = Some(1_470_025_128_000);
        capacity.low_nano = Some(2_449_980_630_000);
        capacity.high_nano = Some(2_450_103_133_000);
        capacity.remaining_low_nano = Some(1_469_988_378_000);
        capacity.remaining_high_nano = Some(1_470_061_880_000);
        capacity.cap_usd = Some(2_450.04188);
        capacity.remaining_usd = Some(1_470.025128);
        capacity.low_usd = Some(2_449.98063);
        capacity.high_usd = Some(2_450.103133);
        capacity.remaining_low_usd = Some(1_469.988378);
        capacity.remaining_high_usd = Some(1_470.06188);
        capacity.observed_spend_nano = 980_016_752_000;
        capacity.observed_fraction_units = 40_000_000;
        capacity.capacity_nanocredits = Some(2_000_000_000_000);
        capacity.remaining_nanocredits = Some(1_200_000_000_000);
        capacity.low_nanocredits = Some(1_900_000_000_000);
        capacity.high_nanocredits = Some(2_100_000_000_000);
        capacity.remaining_low_nanocredits = Some(1_140_000_000_000);
        capacity.remaining_high_nanocredits = Some(1_260_000_000_000);
        capacity.observed_spend_nanocredits = Some(800_000_000_000);
        capacity.credit_samples = Some(4);
        capacity.unattributed_fraction_units = Some(250_000);
        capacity.source = "workload_blend";
        capacity.confidence = 0.8333;
        capacity.samples = 10;

        let value = codex_subs_value(&status, 105);
        let window = &value["homes"][0]["windows"][0];
        assert_eq!(value["homes"][0]["spend_nano_total"], "12500000000");
        assert_eq!(window["capacity_nano"], "2450041880000");
        assert_eq!(window["remaining_nano"], "1470025128000");
        assert_eq!(window["remaining_low_nano"], "1469988378000");
        assert_eq!(window["remaining_high_nano"], "1470061880000");
        assert_eq!(window["observed_spend_nano"], "980016752000");
        assert_eq!(window["capacity_nanocredits"], "2000000000000");
        assert_eq!(window["remaining_nanocredits"], "1200000000000");
        assert_eq!(window["observed_spend_nanocredits"], "800000000000");
        assert_eq!(window["credit_samples"], 4);
        assert_eq!(window["unattributed_fraction_units"], 250_000);
        assert_eq!(window["observed_fraction_units"], 40_000_000);
        assert_eq!(window["workload_dependent"], true);
        assert_eq!(window["cap_usd"], 2_450.04);
        assert_eq!(window["remaining_usd"], 1_470.03);
        assert_eq!(window["source"], "workload_blend");
        assert_eq!(window["samples"], 10);
        assert_eq!(value["window_totals"][0]["capacity_nano"], "2450041880000");
        assert_eq!(
            value["window_totals"][0]["capacity_nanocredits"],
            "2000000000000"
        );
        assert_eq!(
            value["window_totals"][0]["remaining_nanocredits"],
            "1200000000000"
        );
        assert_eq!(
            value["window_totals"][0]["observed_spend_nanocredits"],
            "800000000000"
        );
        assert_eq!(value["window_totals"][0]["credit_measured_homes"], 1);
        assert_eq!(value["window_totals"][0]["credit_observed_homes"], 1);
        assert_eq!(
            value["window_totals"][0]["unattributed_fraction_units"],
            "250000"
        );
        assert_eq!(value["window_totals"][0]["source"], "workload_blend");
        assert_eq!(value["window_totals"][0]["measured_homes"], 1);

        let mut metrics = String::new();
        write_codex_home_capacity_metrics(&mut metrics, &status.homes[0]);
        assert!(metrics.contains(
            "claude_api_codex_home_window_used_ratio{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 0.40000000"
        ));
        assert!(metrics.contains(
            "claude_api_codex_home_window_observed_spend_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 980.016752000"
        ));
        assert!(metrics.contains(
            "claude_api_codex_home_window_capacity_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\",source=\"workload_blend\"} 2450.041880000"
        ));
        assert!(metrics.contains(
            "claude_api_codex_home_window_remaining_low_usd{home=\"home-1\",slot=\"primary\",window_minutes=\"300\"} 1469.988378000"
        ));
    }

    #[test]
    fn codex_subscription_contract_publishes_immutable_turn_evidence() {
        let report = vec![registry::CodexTurnCalibrationAggregate {
            home_id: "home-1".into(),
            model_id: "gpt-5.6-sol".into(),
            service_tier: "fast".into(),
            provider_reported_tier: Some("priority".into()),
            api_tariff_schedule_id: "openai/gpt-5.6-sol/2026-07-30/v2".into(),
            credit_schedule_id: metering::CODEX_CREDIT_SCHEDULE_ID.into(),
            turns: 3,
            first_completed_at: 100,
            last_completed_at: 120,
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 100,
            reasoning_output_tokens: 80,
            api_input_nanousd: 5_000_000,
            api_cached_input_nanousd: 400_000,
            api_cache_write_nanousd: 1_250_000,
            api_output_nanousd: 6_000_000,
            api_total_nanousd: 12_650_000,
            chatgpt_input_nanocredits: 187_500_000,
            chatgpt_cached_input_nanocredits: 12_500_000,
            chatgpt_output_nanocredits: 187_500_000,
            chatgpt_total_nanocredits: 387_500_000,
        }];
        let value = codex_subs_value_with_report(&unknown_codex_status(), 105, Some(&report));
        assert_eq!(value["calibration_evidence_available"], true);
        assert_eq!(
            value["credit_schedule_id"],
            metering::CODEX_CREDIT_SCHEDULE_ID
        );
        let evidence = &value["homes"][0]["calibration_evidence"][0];
        assert_eq!(evidence["model"], "gpt-5.6-sol");
        assert_eq!(evidence["service_tier"], "fast");
        assert_eq!(evidence["turns"], 3);
        assert_eq!(evidence["input_tokens"], "1000");
        assert_eq!(evidence["api_total_nanousd"], "12650000");
        assert_eq!(evidence["chatgpt_total_nanocredits"], "387500000");
    }

    #[test]
    fn codex_conversion_catalogue_keeps_api_and_subscription_fast_independent() {
        let spec = metering::codex_catalog_at(1_785_369_601)
            .into_iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .unwrap();
        let model = forward::codex::CodexModel {
            id: spec.id.into(),
            upstream: spec.upstream.into(),
            created: 0,
            owned_by: "test".into(),
            max_output_tokens: spec.max_output_tokens,
            reasoning_efforts: spec
                .reasoning_efforts
                .iter()
                .map(|value| (*value).into())
                .collect(),
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: spec.subscription_fast_multiplier_basis_points,
            prices: spec.prices,
        };
        let values = codex_conversion_models(&[model], 1_785_369_601);
        assert_eq!(values.len(), 1);
        assert_eq!(
            values[0]["api_tariff_schedule_id"],
            "openai/gpt-5.6-sol/2026-07-30/v2"
        );
        assert_eq!(values[0]["api"]["input_nanousd_per_token"], "5000");
        assert_eq!(values[0]["api"]["fast_multiplier_basis_points"], 20_000);
        assert_eq!(
            values[0]["chatgpt_credits"]["input_nanocredits_per_token"],
            "125000"
        );
        assert_eq!(
            values[0]["chatgpt_credits"]["fast_multiplier_basis_points"],
            25_000
        );
    }

    #[test]
    fn claude_conversion_catalogue_publishes_every_metered_token_bucket() {
        let values = anthropic_conversion_models(1_785_369_601);
        assert_eq!(values.len(), 7);
        let opus = values
            .iter()
            .find(|value| value["id"] == "claude-opus-4-8")
            .unwrap();
        assert_eq!(opus["tiers"][0]["id"], "standard");
        assert_eq!(opus["tiers"][0]["input_nanousd_per_token"], "5000");
        assert_eq!(
            opus["tiers"][0]["cache_write_1h_nanousd_per_token"],
            "10000"
        );
        assert_eq!(opus["tiers"][0]["output_nanousd_per_token"], "25000");
        assert_eq!(opus["tiers"][1]["id"], "fast");
        assert_eq!(opus["tiers"][1]["output_nanousd_per_token"], "50000");
        assert_eq!(opus["web_search_nanousd_per_request"], "10000000");

        let opus5 = values
            .iter()
            .find(|value| value["id"] == "claude-opus-5")
            .unwrap();
        assert_eq!(opus5["tiers"][1]["id"], "fast");
        let fable = values
            .iter()
            .find(|value| value["id"] == "claude-fable-5")
            .unwrap();
        assert_eq!(fable["tiers"].as_array().unwrap().len(), 1);
        assert_eq!(fable["tiers"][0]["input_nanousd_per_token"], "10000");
        assert_eq!(fable["tiers"][0]["output_nanousd_per_token"], "50000");
        let opus47 = values
            .iter()
            .find(|value| value["id"] == "claude-opus-4-7")
            .unwrap();
        assert_eq!(opus47["tiers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn claude_email_hint_never_includes_the_domain_for_a_short_local_part() {
        assert_eq!(mask_claude_email("a@example.com"), "a…");
        assert_eq!(mask_claude_email("owner.account@example.com"), "owne…");
    }

    #[test]
    fn gemini_conversion_catalogue_keeps_long_context_media_and_quota_aliases() {
        let spec = metering::gemini_catalog_at(1_785_369_601)
            .into_iter()
            .find(|model| model.id == "gemini-3.1-pro-preview")
            .unwrap();
        let model = forward::GeminiModel {
            id: spec.id.into(),
            display_name: spec.display_name.into(),
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        };
        let values = gemini_conversion_models(&[model], 1_785_369_601);
        assert_eq!(values.len(), 1);
        assert_eq!(
            values[0]["tariff_schedule_id"],
            metering::gemini::TARIFF_SCHEDULE_ID
        );
        assert_eq!(values[0]["rates"]["input_nanousd_per_token"], "2000");
        assert_eq!(values[0]["rates"]["long_output_nanousd_per_token"], "18000");
        assert_eq!(values[0]["search"]["billing_unit"], "query");
        assert_eq!(
            values[0]["quota_model_ids"],
            json!(["gemini-3.1-pro-low", "gemini-pro-agent"])
        );

        let preview = metering::gemini_catalog_at(1_785_369_601)
            .into_iter()
            .find(|model| model.id == "gemini-3-flash-preview")
            .unwrap();
        let preview = forward::GeminiModel {
            id: preview.id.into(),
            display_name: preview.display_name.into(),
            input_token_limit: preview.input_token_limit,
            output_token_limit: preview.output_token_limit,
            prices: preview.prices,
        };
        let values = gemini_conversion_models(&[preview], 1_785_369_601);
        assert_eq!(values[0]["rates"]["input_nanousd_per_token"], "500");
        assert_eq!(values[0]["rates"]["audio_input_nanousd_per_token"], "1000");
        assert_eq!(values[0]["rates"]["output_nanousd_per_token"], "3000");
        assert_eq!(
            values[0]["quota_model_ids"],
            json!(["gemini-3-flash", "gemini-3-flash-agent"])
        );
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
        let profiles = gemini_profile_values(&status, true, 105);
        assert_eq!(profiles[0]["email"], "owne…");
        assert_eq!(profiles[0]["plan"], "google_ai_pro");
        assert_eq!(profiles[0]["windows"][0]["bucket_id"], "gemini-5h");
        assert_eq!(profiles[0]["windows"][1]["bucket_id"], "gemini-weekly");
        assert!(profiles[0]["windows"][0]["cap_usd"].is_null());
        assert!(profiles[0]["windows"][1]["cap_usd"].is_null());

        let totals = gemini_window_total_values(&status, true, 105);
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
        five_hour.capacity_nano = Some(36_515_628_714);
        five_hour.remaining_nano = Some(27_386_721_535);
        five_hour.low_nano = Some(20_000_244_158);
        five_hour.high_nano = Some(81_204_166_575);
        five_hour.remaining_low_nano = Some(15_000_183_118);
        five_hour.remaining_high_nano = Some(60_903_124_931);
        five_hour.observed_spend_nano = 61_448_500;
        five_hour.observed_fraction_units = 168_280;
        five_hour.source = "workload_blend";
        five_hour.confidence = 0.123;
        five_hour.samples = 2;
        let profiles = gemini_profile_values(&status, true, 105);
        assert_eq!(profiles[0]["windows"][0]["cap_usd"], 36.515629);
        assert_eq!(profiles[0]["windows"][0]["capacity_nano"], "36515628714");
        assert_eq!(profiles[0]["windows"][0]["remaining_usd"], 27.386722);
        assert_eq!(profiles[0]["windows"][0]["low_usd"], 20.000244);
        assert_eq!(profiles[0]["windows"][0]["high_usd"], 81.204167);
        assert_eq!(profiles[0]["windows"][0]["observed_spend_nano"], "61448500");
        assert_eq!(profiles[0]["windows"][0]["source"], "workload_blend");
        assert_eq!(profiles[0]["windows"][0]["workload_dependent"], true);
        assert!(profiles[0]["windows"][1]["cap_usd"].is_null());
        let totals = gemini_window_total_values(&status, true, 105);
        assert_eq!(totals[0]["measured_profiles"], 1);
        assert_eq!(totals[0]["capacity_nano"], "36515628714");
        assert_eq!(totals[0]["remaining_nano"], "27386721535");
        assert_eq!(totals[0]["low_usd"], 20.000244);
        assert_eq!(totals[0]["high_usd"], 81.204167);
        assert_eq!(totals[0]["source"], "workload_blend");
        assert_eq!(totals[1]["measured_profiles"], 0);
    }

    #[test]
    fn gemini_profile_persistence_failure_hides_stale_dollar_capacity_everywhere() {
        let mut status = unknown_gemini_status();
        let five_hour = &mut status.profiles[0].capacities[0];
        five_hour.capacity_nano = Some(50_000_000_000);
        five_hour.remaining_nano = Some(30_000_000_000);
        five_hour.low_nano = Some(40_000_000_000);
        five_hour.high_nano = Some(60_000_000_000);
        five_hour.remaining_low_nano = Some(24_000_000_000);
        five_hour.remaining_high_nano = Some(36_000_000_000);
        five_hour.cap_usd = Some(50.0);
        five_hour.remaining_usd = Some(30.0);
        five_hour.low_usd = Some(40.0);
        five_hour.high_usd = Some(60.0);
        five_hour.remaining_low_usd = Some(24.0);
        five_hour.remaining_high_usd = Some(36.0);
        five_hour.source = "workload_blend";
        status.profiles[0].calibration_persistence_ok = false;
        let delivery = forward::GeminiCalibrationDeliveryStatus {
            pending_events: 0,
            dropped_events: 0,
            persistence_ok: true,
            queue_limit: 4_096,
        };

        assert!(!gemini_calibration_persistence_ok(&status, Some(delivery)));

        let profiles = gemini_profile_values(&status, false, 105);
        assert!(profiles[0]["windows"][0]["capacity_nano"].is_null());
        assert!(profiles[0]["windows"][0]["remaining_nano"].is_null());
        assert!(profiles[0]["windows"][0]["cap_usd"].is_null());
        assert_eq!(profiles[0]["windows"][0]["source"], "unknown");
        assert_eq!(profiles[0]["windows"][0]["used_fraction_units"], 25_000_000);

        let totals = gemini_window_total_values(&status, false, 105);
        assert_eq!(totals[0]["observed_profiles"], 1);
        assert_eq!(totals[0]["measured_profiles"], 0);
        assert!(totals[0]["capacity_nano"].is_null());
        assert!(totals[0]["remaining_nano"].is_null());

        let mut body = String::new();
        write_gemini_profile_capacity_metrics(&mut body, &status.profiles[0], false);
        assert!(body.contains("source=\"unknown\"} 0"));
        assert!(!body.contains("claude_api_gemini_profile_window_capacity_usd{"));
        assert!(!body.contains("claude_api_gemini_profile_window_remaining_usd{"));
    }

    #[test]
    fn gemini_non_routable_profile_keeps_quota_but_is_excluded_from_saleable_capacity() {
        let mut status = unknown_gemini_status();
        let five_hour = &mut status.profiles[0].capacities[0];
        five_hour.capacity_nano = Some(50_000_000_000);
        five_hour.remaining_nano = Some(30_000_000_000);
        five_hour.cap_usd = Some(50.0);
        five_hour.remaining_usd = Some(30.0);
        five_hour.source = "workload_blend";
        status.profiles[0].authenticated = false;
        status.available = 0;

        let profiles = gemini_profile_values(&status, true, 105);
        assert_eq!(profiles[0]["windows"][0]["used_fraction_units"], 25_000_000);
        assert!(profiles[0]["windows"][0]["capacity_nano"].is_null());
        assert!(profiles[0]["windows"][0]["remaining_nano"].is_null());
        assert_eq!(profiles[0]["windows"][0]["source"], "unknown");

        let totals = gemini_window_total_values(&status, true, 105);
        assert!(totals.is_empty());
    }

    #[test]
    fn prometheus_omits_unmeasured_gemini_dollar_series() {
        let profile = &unknown_gemini_status().profiles[0];
        let mut body = String::new();
        write_gemini_profile_capacity_metrics(&mut body, profile, true);
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
            pricing_manifest: Arc::new(forward::builtin_pricing_runtime_manifest()),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(forward::Breaker::new(0)),
            metrics: Arc::new(Metrics::new()),
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
        assert_eq!(ADMIN_ROUTE_CASES.len(), 42);
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

    #[tokio::test]
    async fn stage8_capture_separates_malformed_input_from_unavailable_authority() {
        let (app, dir) = billing_test_app("stage8_capture_validation");
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let valid = json!({
            "target_generation": 41,
            "recovery_generation": 42,
            "window_start_ts": 1,
            "window_end_ts": 2,
            "min_samples_per_provider": 1,
            "financial_sample_size": 1,
            "gemini_client_admissions": 0
        });

        let mut unknown_field = valid.clone();
        unknown_field["runtime_manifest"] = json!({"caller_controlled": true});
        let (status, _) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/v2/stage8-evidence/capture",
            unknown_field,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        let mut invalid_window = valid.clone();
        invalid_window["window_end_ts"] = json!(1);
        let (status, body) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/v2/stage8-evidence/capture",
            invalid_window,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["error"],
            "Stage 8 evidence window must be a positive non-empty half-open interval"
        );

        let (status, body) = control_json_request(
            &service,
            Method::POST,
            "/admin/pricing/v2/stage8-evidence/capture",
            valid,
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"], "billing authority unavailable");

        drop(service);
        let _ = std::fs::remove_dir_all(dir);
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
    async fn account_and_ledger_control_reads_preserve_funding_and_attribution() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-read-surfaces-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let connection = registry::open(path.to_string_lossy().as_ref()).unwrap();
        connection
            .execute_batch(
                "INSERT INTO accounts(
                     id,handle,balance_nano,spent_nano,reserved_nano,mult_bp,status,
                     created_ts,created
                 ) VALUES('acct_read_surface','read-user',900,300,40,5000,'active',1,'');
                 INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES(
                     'acct_read_surface','main','b2c',NULL,'shadow','shadow','verified',1
                 );
                 INSERT INTO funding_buckets(
                     bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                     reserved_nano,spent_nano,version,status,created_ts,updated_ts
                 ) VALUES
                     ('read-paid','acct_read_surface','paid','payment:read','any',700,40,0,2,
                      'active',1,2),
                     ('read-bonus','acct_read_surface','welcome_track_bonus','welcome','track',
                      200,0,300,2,'active',1,2);
                 INSERT INTO ledger(
                     account_id,key,kind,request_id,amount_nano,ref,balance_after_nano,ts,model,
                     provider,official_nano,attribution_schema_version,snapshot_kind,product_id,
                     account_class,requested_model_id,canonical_model_id,served_model_id,
                     served_canonical_model_id,alias_generation,rule_id,rule_digest,rule_scope,
                     pricing_mode,rule_origin,payable_multiplier_bp,policy_id,policy_version,
                     effective_policy_version,policy_digest,catalog_generation,switch_generation,
                     tariff_schedule_id,tariff_priced_ts,official_cost_json,paid_funded_nano,
                     bonus_funded_nano,other_funded_nano,funding_allocation_json,track_eligible,
                     retention_eligible,commission_eligible,snapshot_digest,source_policy_digest,
                     admission_catalog_generation,admission_catalog_digest,
                     admission_switch_generation,admission_switch_digest,
                     runtime_manifest_generation,runtime_manifest_digest
                 ) VALUES(
                     'acct_read_surface','read-key','charge','read-request',300,'provider:read',900,
                     2,'claude-read','anthropic',600,1,'policy_v1','main','b2c','claude-read',
                     'claude-read','claude-read','claude-read',1,'read-rule','read-rule-digest',
                     'provider','track','managed',5000,'read-policy',1,1,'read-policy-digest',1,1,
                     'read-tariff',2,
                     '{\"schema_version\":1,\"provider\":\"anthropic\",\"official_nano\":600}',
                     0,300,0,
                     '[{\"bucket_id\":\"read-bonus\",\"source_type\":\"welcome_track_bonus\",\"bucket_version\":1,\"reserved_nano\":300,\"charged_nano\":300,\"released_nano\":0,\"allocation_order\":1}]',
                     1,1,1,'read-snapshot','read-source-policy',1,'read-catalog',1,
                     'read-switch',1,'read-runtime'
                 );
                 INSERT INTO ledger_funding_allocations(
                     ledger_id,account_id,bucket_id,bucket_source_type,bucket_version,
                     direction,amount_nano
                 ) SELECT id,'acct_read_surface','read-bonus','welcome_track_bonus',1,'debit',300
                     FROM ledger WHERE request_id='read-request';",
            )
            .unwrap();
        drop(connection);

        let billing =
            Arc::new(forward::AsyncBilling::start(path.to_string_lossy().into_owned(), 1).unwrap());
        let mut app = admin_auth_test_app();
        app.billing = Some(billing);
        let service = router(app, Arc::new(AtomicBool::new(true)));

        let (status, account) = control_json_request(
            &service,
            Method::GET,
            "/admin/account/acct_read_surface",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(account["funding"]["account_class"], "b2c");
        assert_eq!(account["funding"]["funding_enforcement"], "shadow");
        assert_eq!(account["funding"]["bucket_count"], 2);
        assert_eq!(account["funding"]["paid_balance_nano"], 700);
        assert_eq!(account["funding"]["bonus_balance_nano"], 200);
        assert_eq!(account["funding"]["unattributed_balance_nano"], 0);
        assert_eq!(account["funding"]["paid_reserved_nano"], 40);
        assert_eq!(account["funding"]["bonus_spent_nano"], 300);

        let (status, ledger) = control_json_request(
            &service,
            Method::GET,
            "/admin/account/acct_read_surface/ledger?after_id=0&limit=10",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let entry = &ledger["entries"][0];
        assert_eq!(entry["request_id"], "read-request");
        assert_eq!(entry["provider"], "anthropic");
        assert_eq!(entry["official_nano"], 600);
        assert_eq!(entry["attribution"]["snapshot_kind"], "policy_v1");
        assert_eq!(
            entry["attribution"]["source_policy_digest"],
            "read-source-policy"
        );
        assert_eq!(
            entry["attribution"]["runtime_manifest_digest"],
            "read-runtime"
        );
        assert_eq!(
            entry["attribution"]["official_cost_json"]["official_nano"],
            600
        );
        assert_eq!(entry["funding_allocations"][0]["bucket_id"], "read-bonus");
        assert_eq!(entry["funding_allocations"][0]["source_ref"], "welcome");
        assert_eq!(entry["funding_allocations"][0]["amount_nano"], 300);

        let _ = std::fs::remove_file(path);
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
            plan: "max20".to_string(),
            calibrated,
            util5h: 0.0,
            util7d: 0.0,
            quota5h: None,
            quota7d: None,
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

    fn claude_calibration(
        email: &str,
        window_kind: &str,
        used_fraction_units: i64,
        observed_fraction_units: i64,
        observed_spend_nano: i64,
    ) -> registry::AnthropicCalibrationRow {
        let duration = if window_kind == "5h" { 300 } else { 10_080 };
        let capacity = i64::try_from(
            i128::from(observed_spend_nano) * CLAUDE_FRACTION_SCALE
                / i128::from(observed_fraction_units),
        )
        .unwrap();
        registry::AnthropicCalibrationRow {
            subject_id: email.to_owned(),
            plan: "max20".to_owned(),
            window_kind: window_kind.to_owned(),
            window_duration_mins: duration,
            resets_at: 2_000_000_000 + duration * 60,
            anchor_used_fraction_units: used_fraction_units,
            anchor_resolution_fraction_units: 100_000,
            anchor_spend_nano: observed_spend_nano,
            used_fraction_units,
            measurement_resolution_fraction_units: 100_000,
            observed_at: 100,
            observed_fraction_units,
            observed_spend_nano,
            samples: 2,
            unattributed_fraction_units: 0,
            current_capacity_nano: Some(capacity),
            current_low_nano: Some(capacity - 1),
            current_high_nano: Some(capacity + 1),
            current_confidence_bp: 8_000,
            last_measured_at: Some(100),
            estimator_version: 1,
            version: 1,
            updated_ts: 100,
        }
    }

    fn claude_delivery(
        pending_events: usize,
    ) -> Option<forward::AnthropicCalibrationDeliveryStatus> {
        Some(forward::AnthropicCalibrationDeliveryStatus {
            pending_events,
            dropped_events: 0,
            persistence_ok: pending_events == 0,
            queue_limit: 4_096,
        })
    }

    #[test]
    fn claude_recent_turn_contract_masks_subject_and_preserves_exact_vector() {
        let event = registry::ProviderTurnCalibrationEvent {
            provider: registry::PROVIDER_ANTHROPIC.to_owned(),
            request_id: "cal-request-1".to_owned(),
            subject_id: "operator@example.test".to_owned(),
            model_id: "claude-opus-4-8".to_owned(),
            service_tier: "fast".to_owned(),
            inference_geo: "global".to_owned(),
            tariff_schedule_id: "anthropic/test/v1".to_owned(),
            priced_ts: 99,
            completed_at: 100,
            input_tokens: 11,
            audio_input_tokens: 0,
            cache_read_tokens: 12,
            cached_audio_input_tokens: 0,
            cache_write_5m_tokens: 13,
            cache_write_1h_tokens: 14,
            output_tokens: 15,
            thinking_output_tokens: 0,
            image_output_tokens: 0,
            tool_prompt_tokens: 0,
            search_queries: 1,
            grounded_search_prompts: 0,
            api_input_nanousd: 110,
            api_audio_input_nanousd: 0,
            api_cache_read_nanousd: 12,
            api_cached_audio_input_nanousd: 0,
            api_cache_write_5m_nanousd: 130,
            api_cache_write_1h_nanousd: 280,
            api_output_nanousd: 750,
            api_image_output_nanousd: 0,
            api_search_nanousd: 10_000_000,
            api_total_nanousd: 10_001_282,
        };

        let value = anthropic_calibration_event_value(&event);
        assert_eq!(value["request_id"], "cal-request-1");
        assert_eq!(value["email"], "oper…");
        assert!(value.get("subject_id").is_none());
        assert_eq!(value["cache_write_1h_tokens"], "14");
        assert_eq!(value["api_total_nanousd"], "10001282");
    }

    #[test]
    fn gemini_recent_turn_contract_preserves_every_runner_vector_field() {
        let event = registry::ProviderTurnCalibrationEvent {
            provider: registry::PROVIDER_GOOGLE.to_owned(),
            request_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            subject_id: "profile-opaque".to_owned(),
            model_id: "gemini-3.6-flash".to_owned(),
            service_tier: "standard".to_owned(),
            inference_geo: "global".to_owned(),
            tariff_schedule_id: "google/test/v1".to_owned(),
            priced_ts: 99,
            completed_at: 100,
            input_tokens: 11,
            audio_input_tokens: 12,
            cache_read_tokens: 13,
            cached_audio_input_tokens: 3,
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: 0,
            output_tokens: 15,
            thinking_output_tokens: 5,
            image_output_tokens: 16,
            tool_prompt_tokens: 4,
            search_queries: 2,
            grounded_search_prompts: 1,
            api_input_nanousd: 110,
            api_audio_input_nanousd: 120,
            api_cache_read_nanousd: 13,
            api_cached_audio_input_nanousd: 3,
            api_cache_write_5m_nanousd: 0,
            api_cache_write_1h_nanousd: 0,
            api_output_nanousd: 750,
            api_image_output_nanousd: 960,
            api_search_nanousd: 28_000_000,
            api_total_nanousd: 28_001_956,
        };

        let value = gemini_calibration_event_value(&event);
        assert_eq!(value["request_id"], event.request_id);
        assert_eq!(value["profile_id"], "profile-opaque");
        assert!(value.get("subject_id").is_none());
        assert_eq!(value["cache_write_5m_tokens"], "0");
        assert_eq!(value["cache_write_1h_tokens"], "0");
        assert_eq!(value["api_cache_write_5m_nanousd"], "0");
        assert_eq!(value["api_cache_write_1h_nanousd"], "0");
        assert_eq!(value["thinking_output_tokens"], "5");
        assert_eq!(value["tool_prompt_tokens"], "4");
        assert_eq!(value["api_total_nanousd"], "28001956");
    }

    #[test]
    fn claude_same_plan_capacity_is_pooled_and_unroutable_supply_is_excluded() {
        let caps = vec![
            capacity("first@example.test", 1.0, true, false),
            capacity("second@example.test", 999.0, true, true),
            capacity("cooling@example.test", 50_000.0, false, true),
        ];
        let rows = vec![
            claude_calibration(
                "first@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            ),
            claude_calibration(
                "second@example.test",
                "5h",
                50_000_000,
                20_000_000,
                4_000_000_000,
            ),
            claude_calibration(
                "first@example.test",
                "7d",
                20_000_000,
                10_000_000,
                10_000_000_000,
            ),
            claude_calibration(
                "second@example.test",
                "7d",
                30_000_000,
                20_000_000,
                20_000_000_000,
            ),
        ];
        let report = (rows, Vec::new(), Vec::new());
        let value = capacity_value(&caps, Some(&report), claude_delivery(0), 100);

        assert_eq!(value["per_sub"][0]["cap5h_nano"], "16666666667");
        assert_eq!(value["per_sub"][1]["cap5h_nano"], "16666666667");
        assert_eq!(value["per_sub"][2]["cap5h_nano"], "16666666667");
        assert_eq!(value["window_totals"][0]["capacity_nano"], "33333333334");
        assert_eq!(value["window_totals"][0]["remaining_nano"], "23333333334");
        assert_eq!(value["window_totals"][0]["routable_subs"], 2);
        assert_eq!(value["window_totals"][0]["calibrated_subs"], 2);
        assert_eq!(value["window_totals"][1]["capacity_nano"], "200000000000");
        assert_eq!(value["window_totals"][1]["remaining_nano"], "150000000000");
        assert_eq!(value["plan_cohorts"][0]["same_plan_capacity"], true);
        assert_eq!(
            value["capacity_semantics"]["legacy_pool_prior_authoritative"],
            false
        );
    }

    #[test]
    fn claude_fresh_runtime_quota_without_reset_publishes_current_dollars_only() {
        let now = 2_000;
        let mut cap = capacity("runtime@example.test", 999.0, true, true);
        cap.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 25_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - 50,
            resets_at: None,
        });
        cap.quota7d = Some(pool::QuotaSnapshot {
            used_fraction_units: 75_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - 50,
            resets_at: None,
        });
        let report = (
            vec![
                claude_calibration(
                    "runtime@example.test",
                    "5h",
                    10_000_000,
                    10_000_000,
                    1_000_000_000,
                ),
                claude_calibration(
                    "runtime@example.test",
                    "7d",
                    10_000_000,
                    10_000_000,
                    10_000_000_000,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );

        let value = capacity_value(&[cap], Some(&report), claude_delivery(0), now);
        let five = &value["per_sub"][0]["windows"][0];
        assert_eq!(five["remaining_nano"], "7500000000");
        assert_eq!(five["snapshot_fresh"], true);
        assert_eq!(five["current_quota_source"], "runtime_quota_snapshot");
        assert!(five["resets_at"].is_null());
        assert!(value["per_sub"][0]["reset5h_in"].is_null());
        assert_eq!(value["per_sub"][0]["rem5h_nano"], "7500000000");
        assert_eq!(value["per_sub"][0]["rem7d_nano"], "25000000000");
        assert_eq!(value["window_totals"][0]["remaining_nano"], "7500000000");
        assert_eq!(value["window_totals"][1]["remaining_nano"], "25000000000");
        assert!(value["available_nano"]["next_5h"].is_null());
    }

    #[test]
    fn claude_stale_runtime_quota_does_not_reopen_current_supply() {
        let now = 2_000;
        let mut cap = capacity("stale@example.test", 999.0, true, true);
        cap.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 25_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
            resets_at: Some(now + 1_800),
        });
        let report = (
            vec![claude_calibration(
                "stale@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            )],
            Vec::new(),
            Vec::new(),
        );

        let value = capacity_value(
            std::slice::from_ref(&cap),
            Some(&report),
            claude_delivery(0),
            now,
        );
        let five = &value["per_sub"][0]["windows"][0];
        assert!(five["remaining_nano"].is_null());
        assert_eq!(five["snapshot_fresh"], false);
        assert_eq!(five["used_fraction_units"], 25_000_000);
        assert_eq!(five["resets_at"], now + 1_800);
        assert_eq!(five["last_known_remaining_nano"], "7500000000");
        assert_eq!(five["last_known_quota_source"], "runtime_quota_snapshot");
        assert!(five["current_quota_source"].is_null());
        assert_eq!(five["missing_reason"], "stale_current_quota_snapshot");
        assert_eq!(value["per_sub"][0]["reset5h_in"], 1_800);
        assert!(value["window_totals"][0]["remaining_nano"].is_null());

        let after_reset = capacity_value(
            std::slice::from_ref(&cap),
            Some(&report),
            claude_delivery(0),
            now + 1_801,
        );
        let expired = &after_reset["per_sub"][0]["windows"][0];
        assert!(expired["used_fraction_units"].is_null());
        assert!(expired["resets_at"].is_null());
        assert!(expired["last_known_remaining_nano"].is_null());
        assert!(expired["last_known_quota_source"].is_null());
        assert!(after_reset["per_sub"][0]["reset5h_in"].is_null());
    }

    #[test]
    fn claude_cooling_preserves_provider_reset_after_quota_snapshot_stales() {
        let now = 2_000;
        let mut cap = capacity("cooling@example.test", 999.0, false, true);
        cap.util5h = 1.0;
        cap.util7d = 0.42;
        cap.reset5h_in = 1_800;
        cap.reset7d_in = 86_400;
        cap.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 100_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
            resets_at: Some(now + 1_800),
        });
        cap.quota7d = Some(pool::QuotaSnapshot {
            used_fraction_units: 42_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
            resets_at: Some(now + 86_400),
        });
        let report = (
            vec![claude_calibration(
                "cooling@example.test",
                "5h",
                100_000_000,
                10_000_000,
                1_000_000_000,
            )],
            Vec::new(),
            Vec::new(),
        );

        let value = capacity_value(&[cap], Some(&report), claude_delivery(0), now);
        let five = &value["per_sub"][0]["windows"][0];
        assert_eq!(five["snapshot_fresh"], false);
        assert!(five["remaining_nano"].is_null());
        assert_eq!(five["resets_at"], now + 1_800);
        assert_eq!(five["last_known_remaining_nano"], "0");
        assert_eq!(five["last_known_quota_source"], "runtime_quota_snapshot");
        assert_eq!(value["per_sub"][0]["cooling"], true);
        assert_eq!(value["per_sub"][0]["routable"], false);
        assert_eq!(value["per_sub"][0]["reset5h_in"], 1_800);
        assert_eq!(value["per_sub"][0]["reset7d_in"], 86_400);
        assert!(value["window_totals"][0]["remaining_nano"].is_null());

        let mut unknown_reset = capacity("unknown-reset@example.test", 999.0, false, true);
        unknown_reset.util5h = 1.0;
        unknown_reset.reset5h_in = 18_000;
        unknown_reset.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 100_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now - CLAUDE_SNAPSHOT_MAX_AGE_SECS - 1,
            resets_at: None,
        });
        let empty_report = (Vec::new(), Vec::new(), Vec::new());
        let unknown = capacity_value(
            &[unknown_reset],
            Some(&empty_report),
            claude_delivery(0),
            now,
        );
        assert!(unknown["per_sub"][0]["reset5h_in"].is_null());
    }

    #[test]
    fn claude_runtime_quota_uses_same_plan_capacity_without_own_durable_row() {
        let now = 100;
        let evidence = capacity("evidence@example.test", 999.0, true, true);
        let mut runtime = capacity("runtime@example.test", 999.0, true, false);
        runtime.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 40_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now,
            resets_at: None,
        });
        runtime.quota7d = Some(pool::QuotaSnapshot {
            used_fraction_units: 60_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now,
            resets_at: None,
        });
        let report = (
            vec![
                claude_calibration(
                    "evidence@example.test",
                    "5h",
                    10_000_000,
                    10_000_000,
                    1_000_000_000,
                ),
                claude_calibration(
                    "evidence@example.test",
                    "7d",
                    20_000_000,
                    10_000_000,
                    10_000_000_000,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );

        let value = capacity_value(&[evidence, runtime], Some(&report), claude_delivery(0), now);
        let runtime_five = &value["per_sub"][1]["windows"][0];
        assert_eq!(runtime_five["capacity_nano"], "10000000000");
        assert_eq!(runtime_five["remaining_nano"], "6000000000");
        assert_eq!(
            runtime_five["current_quota_source"],
            "runtime_quota_snapshot"
        );
        assert!(runtime_five["missing_reason"].is_null());
        assert_eq!(value["window_totals"][0]["remaining_nano"], "15000000000");
    }

    #[test]
    fn claude_delivery_degradation_remains_fail_closed_with_runtime_quota() {
        let now = 100;
        let mut cap = capacity("runtime@example.test", 999.0, true, true);
        cap.quota5h = Some(pool::QuotaSnapshot {
            used_fraction_units: 25_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: now,
            resets_at: None,
        });
        let report = (
            vec![claude_calibration(
                "runtime@example.test",
                "5h",
                10_000_000,
                10_000_000,
                1_000_000_000,
            )],
            Vec::new(),
            Vec::new(),
        );

        let value = capacity_value(&[cap], Some(&report), claude_delivery(1), now);
        let five = &value["per_sub"][0]["windows"][0];
        assert!(five["remaining_nano"].is_null());
        assert!(five["current_quota_source"].is_null());
        assert_eq!(five["missing_reason"], "calibration_delivery_pending");
        assert!(value["window_totals"][0]["remaining_nano"].is_null());
        assert!(value["plan_cohorts"][0]["fleet_remaining_nano"].is_null());
        assert_eq!(
            value["plan_cohorts"][0]["missing_reason"],
            "calibration_delivery_pending"
        );
    }

    #[test]
    fn claude_fleet_totals_fail_closed_for_missing_plan_or_authority_evidence() {
        let mut caps = vec![capacity("first@example.test", 999.0, true, true)];
        let report = (
            vec![
                claude_calibration(
                    "first@example.test",
                    "5h",
                    10_000_000,
                    10_000_000,
                    1_000_000_000,
                ),
                claude_calibration(
                    "first@example.test",
                    "7d",
                    20_000_000,
                    10_000_000,
                    10_000_000_000,
                ),
            ],
            Vec::new(),
            Vec::new(),
        );
        let measured = capacity_value(&caps, Some(&report), claude_delivery(0), 100);
        assert!(measured["window_totals"][0]["capacity_nano"].is_string());

        let stale = capacity_value(&caps, Some(&report), claude_delivery(0), 2_000);
        assert!(stale["window_totals"][0]["capacity_nano"].is_string());
        assert!(stale["window_totals"][0]["remaining_nano"].is_null());
        assert_eq!(
            stale["per_sub"][0]["windows"][0]["missing_reason"],
            "stale_current_quota_snapshot"
        );

        let mut uncovered = capacity("other@example.test", 50_000.0, true, true);
        uncovered.plan = "max5".to_owned();
        caps.push(uncovered);
        let missing_plan = capacity_value(&caps, Some(&report), claude_delivery(0), 100);
        assert!(missing_plan["window_totals"][0]["capacity_nano"].is_null());
        assert_eq!(
            missing_plan["window_totals"][0]["missing_reason"],
            "missing_plan_evidence"
        );

        let missing_authority = capacity_value(&caps[..1], None, None, 100);
        assert!(missing_authority["window_totals"][0]["capacity_nano"].is_null());
        assert!(missing_authority["available_nano"]["next_5h"].is_null());
        assert_eq!(missing_authority["per_sub"][0]["cap5h_nano"], Value::Null);
        assert_eq!(
            missing_authority["window_totals"][0]["missing_reason"],
            "calibration_authority_unavailable"
        );

        let pending_delivery = capacity_value(&caps[..1], Some(&report), claude_delivery(2), 100);
        assert!(pending_delivery["window_totals"][0]["capacity_nano"].is_string());
        assert!(pending_delivery["window_totals"][0]["remaining_nano"].is_null());
        assert!(pending_delivery["available_nano"]["next_5h"].is_null());
        assert_eq!(
            pending_delivery["per_sub"][0]["windows"][0]["missing_reason"],
            "calibration_delivery_pending"
        );
        assert_eq!(
            pending_delivery["calibration_delivery"]["pending_events"],
            2
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
    async fn exact_not_started_responses_increment_only_the_serving_plane_counter() {
        let app = provider_test_app(forward::ProviderMode::Anthropic);
        let metrics = Arc::clone(&app.metrics);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let peer = ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424)));

        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/v1/chat/completions")
            .header("x-api-key", "admin-key")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(forward::is_exact_not_started_response(&response));
        assert_eq!(
            metrics.execution_not_started_count(forward::ProviderMode::Anthropic),
            1
        );
        assert_eq!(
            metrics.execution_not_started_count(forward::ProviderMode::OpenAi),
            0
        );
        assert_eq!(
            metrics.execution_not_started_count(forward::ProviderMode::Gemini),
            0
        );

        let mut request = Request::builder()
            .uri("/metrics")
            .header("x-api-key", "panel-key")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(peer);
        let response = service.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = String::from_utf8(
            to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("claude_api_execution_not_started_total{plane=\"anthropic\"} 1"));
        assert!(body.contains("claude_api_execution_not_started_total{plane=\"openai\"} 0"));
        assert!(body.contains("claude_api_execution_not_started_total{plane=\"gemini\"} 0"));
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
            "claude_api_anthropic_calibration_authority_available 0",
            "claude_api_anthropic_calibration_pending_events 0",
            "claude_api_anthropic_calibration_dropped_events_total 0",
            "claude_api_anthropic_calibration_persistence_ok 0",
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
        for provider in ["anthropic", "openai", "google"] {
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

    async fn router_auth_request(
        service: &Router,
        credential: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/internal/router/auth/preflight")
            .body(Body::empty())
            .unwrap();
        if let Some(credential) = credential {
            request
                .headers_mut()
                .insert("x-api-key", credential.parse().unwrap());
        }
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
        let response = service.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn router_policy_request(
        service: &Router,
        credential: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/internal/router/policy/preflight")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        if let Some(credential) = credential {
            request
                .headers_mut()
                .insert("x-api-key", credential.parse().unwrap());
        }
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
        let response = service.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn router_pricing_request(
        service: &Router,
        credential: Option<&str>,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/internal/router/catalog/pricing")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        if let Some(credential) = credential {
            request
                .headers_mut()
                .insert("x-api-key", credential.parse().unwrap());
        }
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 42_424))));
        let response = service.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    fn pricing_candidates() -> Value {
        json!({
            "schema_version": 1,
            "candidates": [
                {
                    "id": "anthropic/claude-sonnet-4-6",
                    "provider_id": "anthropic",
                    "model_id": "claude-sonnet-4-6"
                },
                {
                    "id": "openai/gpt-5.6-sol",
                    "provider_id": "openai",
                    "model_id": "gpt-5.6-sol"
                },
                {
                    "id": "google/gemini-3.6-flash",
                    "provider_id": "google",
                    "model_id": "gemini-3.6-flash"
                }
            ]
        })
    }

    fn policy_candidates() -> Value {
        json!({
            "schema_version": 1,
            "candidates": [
                {
                    "id": "anthropic/claude-sonnet-5",
                    "provider_id": "anthropic",
                    "canonical_model_id": "claude-sonnet-5"
                },
                {
                    "id": "openai/gpt-5.6-sol",
                    "provider_id": "openai",
                    "canonical_model_id": "gpt-5.6-sol"
                },
                {
                    "id": "google/gemini-3.6-flash",
                    "provider_id": "google",
                    "canonical_model_id": "gemini-3.6-flash"
                }
            ]
        })
    }

    #[tokio::test]
    async fn router_auth_preflight_is_bodyless_read_only_and_present_on_every_plane() {
        for provider in [
            forward::ProviderMode::Combined,
            forward::ProviderMode::Anthropic,
            forward::ProviderMode::OpenAi,
            forward::ProviderMode::Gemini,
        ] {
            let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
            let (status, body) = router_auth_request(&service, Some("admin-key")).await;
            assert_eq!(status, StatusCode::OK, "provider={provider:?}");
            assert_eq!(body, json!({"schema_version": 1, "authenticated": true}));
        }

        let service = router(
            provider_test_app(forward::ProviderMode::Anthropic),
            Arc::new(AtomicBool::new(true)),
        );
        let (status, body) = router_auth_request(&service, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");

        let (app, dir) = billing_test_app("router_auth_preflight");
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::account_create(&conn, "router-auth-account", None, 10_000).unwrap();
        registry::key_issue(
            &conn,
            "router-auth-key",
            "router-auth-account",
            Some("router auth"),
        )
        .unwrap();
        drop(conn);

        let service = router(app, Arc::new(AtomicBool::new(true)));
        let (status, body) = router_auth_request(&service, Some("router-auth-key")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"schema_version": 1, "authenticated": true}));
        assert!(!body.to_string().contains("router-auth-account"));

        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::key_set_status(&conn, "router-auth-key", "inactive").unwrap();
        drop(conn);
        let (status, body) = router_auth_request(&service, Some("router-auth-key")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");

        let (status, body) = router_auth_request(&service, Some("unknown-key")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn router_policy_preflight_is_present_on_every_plane_and_admin_is_unrestricted() {
        for provider in [
            forward::ProviderMode::Combined,
            forward::ProviderMode::Anthropic,
            forward::ProviderMode::OpenAi,
            forward::ProviderMode::Gemini,
        ] {
            let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
            let (status, body) =
                router_policy_request(&service, Some("admin-key"), policy_candidates()).await;
            assert_eq!(status, StatusCode::OK, "provider={provider:?}");
            assert_eq!(body["schema_version"], 1);
            assert_eq!(body["mode"], "unrestricted");
            assert_eq!(
                body["allowed"],
                json!([
                    "anthropic/claude-sonnet-5",
                    "openai/gpt-5.6-sol",
                    "google/gemini-3.6-flash"
                ])
            );
        }

        let service = router(
            provider_test_app(forward::ProviderMode::Anthropic),
            Arc::new(AtomicBool::new(true)),
        );
        let (status, body) = router_policy_request(&service, None, policy_candidates()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");

        let mut malformed = policy_candidates();
        malformed["authority_override"] = json!(true);
        let (status, body) = router_policy_request(&service, Some("admin-key"), malformed).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_request");
    }

    #[tokio::test]
    async fn router_catalog_pricing_is_key_scoped_integer_only_and_present_on_every_plane() {
        for provider in [
            forward::ProviderMode::Combined,
            forward::ProviderMode::Anthropic,
            forward::ProviderMode::OpenAi,
            forward::ProviderMode::Gemini,
        ] {
            let service = router(provider_test_app(provider), Arc::new(AtomicBool::new(true)));
            let (status, body) =
                router_pricing_request(&service, Some("admin-key"), pricing_candidates()).await;
            assert_eq!(status, StatusCode::OK, "provider={provider:?}");
            assert_eq!(body["schema_version"], 1);
            assert_eq!(body["unit"], "nano_usd_per_million_tokens");
            assert_eq!(body["mode"], "admin");
            assert_eq!(body["entries"].as_array().unwrap().len(), 3);
            assert_eq!(body["entries"][0]["standard"]["input"], "3000000000");
            assert_eq!(body["entries"][1]["priority"]["output"], "60000000000");
            assert!(body["entries"][0]["standard"]["input"].is_string());
        }

        let (app, dir) = billing_test_app("router_catalog_pricing");
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::account_create(&conn, "discount-account", None, 5_000).unwrap();
        registry::key_issue(
            &conn,
            "discount-key",
            "discount-account",
            Some("router pricing"),
        )
        .unwrap();
        drop(conn);

        let service = router(app, Arc::new(AtomicBool::new(true)));
        let (status, body) =
            router_pricing_request(&service, Some("discount-key"), pricing_candidates()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["mode"], "legacy");
        assert_eq!(body["entries"][0]["standard"]["input"], "1500000000");
        assert_eq!(body["entries"][1]["standard"]["output"], "15000000000");
        assert_eq!(body["entries"][1]["priority"]["output"], "30000000000");
        assert_eq!(body["entries"][2]["standard"]["input"], "750000000");
        assert!(!body.to_string().contains("discount-account"));

        let (status, body) =
            router_pricing_request(&service, Some("unknown-key"), pricing_candidates()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["code"], "unauthorized");
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn router_policy_preflight_filters_one_coherent_strict_policy_before_execution() {
        use registry::pricing::{
            AccountClass, AccountPolicyActivationSpec, AccountPolicyBindingSpec,
            AccountPolicyRuleSpec, AccountPolicySpec, ActiveExpectation, FundingEnforcement,
            PolicyActiveExpectation, PolicyEnforcement, PolicyOwnerType, PolicyRuleScope,
            PolicySegment, PricingCatalogEntrySpec, PricingCatalogSpec, PricingMode,
            PricingMutation, ProviderSwitchEntrySpec, ProviderSwitchScope, ProviderSwitchSpec,
            ReconciliationState, RuleOrigin,
        };

        let (mut app, dir) = billing_test_app("router_policy_preflight");
        let db = dir.join("data.db");
        let conn = registry::open(db.to_string_lossy().as_ref()).unwrap();
        registry::account_create(&conn, "strict-router-account", None, 10_000).unwrap();

        let runtime = forward::builtin_pricing_runtime_manifest();
        let capability = &runtime.capabilities()[0];
        let catalog = PricingCatalogSpec {
            product_id: "main".to_owned(),
            generation: 1,
            schema_version: capability.pricing_schema_version(),
            capability_generation: capability.capability_generation(),
            capability_digest: capability.capability_digest().to_owned(),
            content_digest: "router-policy-catalog-v1".to_owned(),
            entries: vec![
                PricingCatalogEntrySpec {
                    provider_id: "anthropic".to_owned(),
                    canonical_model_id: "claude-sonnet-5".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "openai".to_owned(),
                    canonical_model_id: "gpt-5.6-sol".to_owned(),
                    enabled: true,
                },
                PricingCatalogEntrySpec {
                    provider_id: "google".to_owned(),
                    canonical_model_id: "gemini-3.6-flash".to_owned(),
                    enabled: true,
                },
            ],
        };
        assert_eq!(
            registry::pricing::sqlite_prepare_pricing_catalog(&conn, &catalog).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            registry::pricing::sqlite_activate_pricing_catalog(
                &conn,
                "main",
                &catalog.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        let mut switch_entries = Vec::new();
        for provider_id in ["anthropic", "openai", "google"] {
            switch_entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.to_owned(),
                scope: ProviderSwitchScope::Master,
                catalog_generation: None,
                enabled: true,
            });
            switch_entries.push(ProviderSwitchEntrySpec {
                provider_id: provider_id.to_owned(),
                scope: ProviderSwitchScope::Segment {
                    product_id: "main".to_owned(),
                    segment: PolicySegment::B2c,
                },
                catalog_generation: Some(1),
                enabled: true,
            });
        }
        let switches = ProviderSwitchSpec {
            generation: 1,
            schema_version: capability.pricing_schema_version(),
            capability_generation: capability.capability_generation(),
            capability_digest: capability.capability_digest().to_owned(),
            content_digest: "router-policy-switches-v1".to_owned(),
            entries: switch_entries,
        };
        assert_eq!(
            registry::pricing::sqlite_prepare_provider_switches(&conn, &switches).unwrap(),
            PricingMutation::Stored
        );
        assert_eq!(
            registry::pricing::sqlite_activate_provider_switches(
                &conn,
                &switches.target(),
                &ActiveExpectation::Absent,
            )
            .unwrap(),
            PricingMutation::Applied
        );

        let rule = |rule_id: &str, scope: PolicyRuleScope, payable_multiplier_bp: i64| {
            AccountPolicyRuleSpec {
                rule_id: rule_id.to_owned(),
                rule_digest: format!("{rule_id}-digest"),
                scope,
                pricing_mode: PricingMode::Discount,
                rule_origin: RuleOrigin::Managed,
                discount_bps: Some(10_000 - payable_multiplier_bp),
                payable_multiplier_bp,
                track_eligible: false,
                retention_eligible: false,
                commission_eligible: false,
            }
        };
        let policy = AccountPolicySpec {
            account_id: "strict-router-account".to_owned(),
            effective_version: 1,
            policy_id: "router-policy".to_owned(),
            policy_version: 1,
            source_policy_digest: "router-policy-source-v1".to_owned(),
            owner_type: PolicyOwnerType::GlobalB2c,
            owner_id: "global".to_owned(),
            account_class: AccountClass::B2c,
            product_id: "main".to_owned(),
            schema_version: capability.pricing_schema_version(),
            catalog_generation: 1,
            switch_generation: 1,
            content_digest: "router-policy-v1".to_owned(),
            replacement_locked: false,
            rules: vec![
                rule(
                    "allow-anthropic-sonnet",
                    PolicyRuleScope::Model {
                        provider_id: "anthropic".to_owned(),
                        canonical_model_id: "claude-sonnet-5".to_owned(),
                    },
                    6_000,
                ),
                rule(
                    "allow-openai",
                    PolicyRuleScope::Provider {
                        provider_id: "openai".to_owned(),
                    },
                    8_000,
                ),
                rule(
                    "allow-google",
                    PolicyRuleScope::Provider {
                        provider_id: "google".to_owned(),
                    },
                    10_000,
                ),
            ],
        };
        assert_eq!(
            registry::pricing::sqlite_prepare_account_policy(&conn, &policy).unwrap(),
            PricingMutation::Stored
        );
        let binding = AccountPolicyBindingSpec {
            policy_enforcement: PolicyEnforcement::Strict,
            funding_enforcement: FundingEnforcement::Strict,
            reconciliation_state: ReconciliationState::Verified,
        };
        assert_eq!(
            registry::pricing::sqlite_activate_account_policy(
                &conn,
                &AccountPolicyActivationSpec {
                    account_id: policy.account_id.clone(),
                    effective_version: policy.effective_version,
                    content_digest: policy.content_digest.clone(),
                    binding,
                },
                &PolicyActiveExpectation::Unbound,
            )
            .unwrap(),
            PricingMutation::Applied
        );
        registry::key_issue_with_policy_ack(
            &conn,
            "strict-router-key",
            &policy.account_id,
            Some("router"),
            None,
            None,
            Some(&registry::KeyActivationPolicyAck {
                effective_policy_version: policy.effective_version,
                policy_digest: policy.content_digest.clone(),
            }),
        )
        .unwrap();
        drop(conn);

        app.pricing_manifest = Arc::new(runtime);
        let service = router(app, Arc::new(AtomicBool::new(true)));
        let (status, body) =
            router_policy_request(&service, Some("strict-router-key"), policy_candidates()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["mode"], "strict");
        assert_eq!(
            body["allowed"],
            json!(["anthropic/claude-sonnet-5", "openai/gpt-5.6-sol"])
        );
        assert!(
            !body.to_string().contains("strict-router-account")
                && !body.to_string().contains("router-policy-v1")
        );

        let (status, pricing) = router_pricing_request(
            &service,
            Some("strict-router-key"),
            json!({
                "schema_version": 1,
                "candidates": [
                    {
                        "id": "anthropic/claude-sonnet-5",
                        "provider_id": "anthropic",
                        "model_id": "claude-sonnet-5"
                    },
                    {
                        "id": "openai/gpt-5.6-sol",
                        "provider_id": "openai",
                        "model_id": "gpt-5.6-sol"
                    },
                    {
                        "id": "google/gemini-3.6-flash",
                        "provider_id": "google",
                        "model_id": "gemini-3.6-flash"
                    }
                ]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(pricing["mode"], "strict");
        assert_eq!(pricing["entries"].as_array().unwrap().len(), 2);
        assert_eq!(pricing["entries"][0]["id"], "anthropic/claude-sonnet-5");
        assert_eq!(pricing["entries"][1]["id"], "openai/gpt-5.6-sol");
        assert_eq!(pricing["entries"][0]["standard"]["input"], "1200000000");
        assert_eq!(pricing["entries"][1]["standard"]["input"], "4000000000");
        assert!(!pricing.to_string().contains("strict-router-account"));

        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn strict_key_issue_and_reactivation_require_the_exact_policy_ack() {
        let (app, dir) = billing_test_app("strict_key_policy_ack");
        let conn = registry::open(dir.join("data.db").to_str().unwrap()).unwrap();
        registry::account_create(&conn, "strict-http-account", None, 10_000).unwrap();
        conn.execute_batch(
            "INSERT INTO pricing_catalog_versions(
                 product_id,generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES('main',1,1,1,'capability','catalog-digest',1);
             INSERT INTO provider_switch_versions(
                 generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES(1,1,1,'capability','switch-digest',1);
             INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                 switch_generation,content_digest,replacement_locked,created_ts
             ) VALUES(
                 'strict-http-account',7,'b2c:global',3,'source-policy','global_b2c','global',
                 'b2c','main',1,1,1,'policy-digest-7',0,1
             );
             INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(
                 'strict-http-account','main','b2c',7,'strict','strict','verified',1
             );",
        )
        .unwrap();
        drop(conn);

        let service = router(app, Arc::new(AtomicBool::new(true)));
        let issue_body = |activation_policy_ack: Option<Value>| {
            let mut body = json!({"account_id": "strict-http-account"});
            if let Some(ack) = activation_policy_ack {
                body["activation_policy_ack"] = ack;
            }
            body
        };

        let (status, _) = control_json_request(
            &service,
            Method::POST,
            "/admin/key",
            issue_body(Some(json!({
                "effective_policy_version": 0,
                "policy_digest": "policy-digest-7"
            }))),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) =
            control_json_request(&service, Method::POST, "/admin/key", issue_body(None)).await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, _) = control_json_request(
            &service,
            Method::POST,
            "/admin/key",
            issue_body(Some(json!({
                "effective_policy_version": 7,
                "policy_digest": "wrong-digest"
            }))),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, issued) = control_json_request(
            &service,
            Method::POST,
            "/admin/key",
            issue_body(Some(json!({
                "effective_policy_version": 7,
                "policy_digest": "policy-digest-7"
            }))),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let key_id = issued["key_id"].as_str().unwrap();

        let status_path = format!("/admin/key-id/{key_id}/status");
        let (status, _) = control_json_request(
            &service,
            Method::POST,
            &status_path,
            json!({"status": "disabled"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = control_json_request(
            &service,
            Method::POST,
            &status_path,
            json!({"status": "active"}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, _) = control_json_request(
            &service,
            Method::POST,
            &status_path,
            json!({
                "status": "active",
                "activation_policy_ack": {
                    "effective_policy_version": 7,
                    "policy_digest": "policy-digest-7"
                }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
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
