//! claude-router — единый stateless вход для всех provider-плоскостей
//! (этап 1b docs/engine/UNIFIED_ROUTER.md).
//!
//! Bounded context ВНЕ слоёв registry ← pool ← forward ← server: крейт не
//! импортирует их и общается с плоскостями только по HTTP через stable
//! loopback origins (8790/8792/8794). Router не резервирует и не списывает
//! деньги (инвариант 1), не ретраит неоднозначные исходы (инвариант 2), не
//! имеет execution-очередей, semaphore и breaker (инвариант 3); fail-fast
//! 64 MiB budget ограничивает только universal request bodies. SSE не
//! буферизуется (инвариант 4).
//! Universal fallback выключен по умолчанию и разрешён только по fencing-
//! сигналам из docs/engine/ROUTING_FENCING.md.

mod auth;
mod bounded;
mod catalog;
mod chat;
mod config;
mod error;
mod messages;
mod metrics;
mod policy;
mod presets;
mod pricing;
mod proxy;
mod responses;
mod routing;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use reqwest::Client;
use tokio::sync::Semaphore;

use catalog::{Catalog, PlaneOrigins};
use config::Config;
use error::Lane;
use metrics::{PricingFailure, RouterMetrics};

/// Состояние процесса: HTTP-клиент, кэш каталога и fail-fast 64 MiB budget для universal
/// request bodies. Денег, ключей и execution-очередей здесь нет.
pub struct AppState {
    cfg: Config,
    client: Client,
    catalog: Catalog,
    metrics: Arc<RouterMetrics>,
    /// Raw request-body admission only. Weighted units grow with observed chunked bytes and remain
    /// held through buffering until the outbound upload completes; provider TTFT/response streams
    /// and native lanes never hold them.
    body_admission: Arc<Semaphore>,
}

/// HTTP-клиент плоскостей. Только loopback HTTP: TLS не нужен. Redirect не
/// следуем — нативный ответ плоскости отдаётся клиенту как есть. Таймаут
/// на клиенте только connect: data-plane ожидание response headers и body
/// ограничивают вызывающий клиент и плоскость, а не router.
fn build_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(64)
        .build()?)
}

/// Таблица маршрутизации публичного контракта (UNIFIED_ROUTER.md,
/// «Публичный контракт»). Native lanes выбирают плоскость формой пути;
/// `/v1/chat/completions`, `/v1/responses` и `/v1/messages{,/count_tokens}` — universal
/// lanes с model-based dispatch (chat::proxy_chat, этап 3.1;
/// responses::proxy_responses, этап 4.1; messages::proxy_messages, этап 5.1).
/// Stored responses endpoints остаются native OpenAI lane (решение 5);
/// `/v1/images/generations` и `/v1/images/edits` — тоже native OpenAI lane
/// (байт-в-байт прокси на OpenAI-плоскость, биллинг и admission — в плоскости).
/// Собственные поверхности router'а — агрегированный каталог /v1/models и
/// dispatch universal-запросов.
pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(proxy::health))
        .route("/live", get(proxy::health))
        .route("/ready", get(proxy::ready))
        .route("/startup", get(startup))
        .route("/metrics", get(router_metrics))
        .route("/balance", get(proxy_balance))
        .route("/v1/messages", post(messages::proxy_messages))
        .route("/v1/messages/count_tokens", post(messages::proxy_messages))
        .route("/v1/models", get(list_models))
        .route("/v1/models/{*id}", get(get_model))
        .route("/v1/responses", post(responses::proxy_responses))
        .route("/v1/responses/input_tokens", post(proxy_openai))
        .route("/v1/responses/{id}", get(proxy_openai).delete(proxy_openai))
        .route("/v1/responses/{id}/input_items", get(proxy_openai))
        .route("/v1/images/generations", post(proxy_openai))
        .route("/v1/images/edits", post(proxy_openai))
        .route("/v1/chat/completions", post(chat::proxy_chat))
        .route("/v1beta/{*rest}", any(proxy_gemini))
        .fallback(error_fallback)
        .method_not_allowed_fallback(method_not_allowed_fallback)
        .with_state(state)
}

/// Loopback-only Prometheus endpoint. The router has no metrics credential because the listener is
/// already constrained to loopback and Prometheus scrapes it directly on the host.
async fn router_metrics(State(state): State<Arc<AppState>>) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(),
    )
        .into_response()
}

async fn proxy_balance(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_balance(
        &state.client,
        [
            (&state.cfg.anthropic_origin, Lane::Anthropic),
            (&state.cfg.openai_origin, Lane::OpenAi),
            (&state.cfg.gemini_origin, Lane::Gemini),
        ],
        req,
        &state.metrics,
    )
    .await
}

/// Loopback-only deployment probe. Caddy's public allowlist omits `/startup`; blue-green admission
/// calls the direct candidate slot and requires an exact provider unauthenticated wire contract.
async fn startup(State(state): State<Arc<AppState>>) -> Response {
    if auth::startup_probe(&state.client, &origins(&state)).await {
        (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({"startup": true})),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"startup": false})),
        )
            .into_response()
    }
}

async fn proxy_openai(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_request(
        &state.client,
        &state.cfg.openai_origin,
        Lane::OpenAi,
        req,
        &state.metrics,
    )
    .await
}

async fn proxy_gemini(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_request(
        &state.client,
        &state.cfg.gemini_origin,
        Lane::Gemini,
        req,
        &state.metrics,
    )
    .await
}

/// 404 вне контракта. OpenAI-совместимый конверт: универсальные клиенты —
/// основная аудитория путей, не совпадающих с native lanes.
async fn error_fallback() -> Response {
    error::unsupported_endpoint()
}

/// 405 в форме плоскости, выбранной по пути.
async fn method_not_allowed_fallback(req: Request) -> Response {
    error::method_not_allowed(req.uri().path())
}

pub(crate) fn origins(state: &AppState) -> PlaneOrigins<'_> {
    PlaneOrigins {
        anthropic: &state.cfg.anthropic_origin,
        openai: &state.cfg.openai_origin,
        gemini: &state.cfg.gemini_origin,
    }
}

/// `GET /v1/models` — единый каталог. Ответ OpenAI-совместим (`object: list`),
/// ID namespaced; `anthropic/claude-*` принимается discovery Claude Code
/// (он игнорирует ID вне префиксов claude/anthropic — см. документ).
async fn list_models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let codex_envelope = requests_codex_models_envelope(&headers);
    let auth = proxy::auth_passthrough(&headers);
    let aggregate = state
        .catalog
        .aggregate(&state.client, &origins(&state), &auth, &state.metrics)
        .await;
    if aggregate.auth_rejected {
        return private_catalog_response(auth_rejected_response());
    }
    let entries = catalog::dedup(aggregate.entries);
    if entries.is_empty() {
        elog::warn("router", "models catalog empty");
        return private_catalog_response(error::catalog_unavailable());
    }
    let pricing = match catalog_pricing(&state, &auth, &entries).await {
        Ok(pricing) => pricing,
        Err(response) => return private_catalog_response(response),
    };
    let eligible: Vec<_> = entries
        .iter()
        .filter(|(_, entry)| pricing.entry(&entry.id).is_some())
        .cloned()
        .collect();
    let mut data: Vec<_> = eligible
        .iter()
        .map(|(namespace, entry)| model_json_with_pricing(namespace, entry, &pricing))
        .collect();
    data.extend(presets::active_catalog_entries(&eligible));

    let mut response = if codex_envelope {
        // Codex gets its backend-native empty overlay, but only after the uncached pricing call
        // authenticated this exact request even when the shared capability catalog was fresh.
        axum::Json(serde_json::json!({"models": []})).into_response()
    } else {
        axum::Json(serde_json::json!({"object": "list", "data": data})).into_response()
    };
    if !aggregate.degraded.is_empty() {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(catalog::DEGRADED_HEADER),
            axum::http::HeaderValue::from_str(&aggregate.degraded.join(","))
                .expect("namespace list is header-safe"),
        );
    }
    private_catalog_response(response)
}

fn requests_codex_models_envelope(headers: &HeaderMap) -> bool {
    ["originator", "user-agent"].iter().any(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("codex"))
    })
}

/// `GET /v1/models/{id}` — namespaced ID или нативный alias.
async fn get_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let auth = proxy::auth_passthrough(&headers);
    let aggregate = state
        .catalog
        .aggregate(&state.client, &origins(&state), &auth, &state.metrics)
        .await;
    if aggregate.auth_rejected {
        return private_catalog_response(auth_rejected_response());
    }
    let entries = catalog::dedup(aggregate.entries);
    if entries.is_empty() {
        elog::warn("router", "models catalog empty");
        return private_catalog_response(error::catalog_unavailable());
    }
    let pricing = match catalog_pricing(&state, &auth, &entries).await {
        Ok(pricing) => pricing,
        Err(response) => return private_catalog_response(response),
    };
    let eligible: Vec<_> = entries
        .iter()
        .filter(|(_, entry)| pricing.entry(&entry.id).is_some())
        .cloned()
        .collect();
    let model = presets::active_catalog_entry(&id, &eligible).or_else(|| {
        catalog::find(&eligible, &id)
            .map(|(namespace, entry)| model_json_with_pricing(namespace, entry, &pricing))
    });
    match model {
        Some(model) => {
            let mut response = axum::Json(model).into_response();
            if !aggregate.degraded.is_empty() {
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static(catalog::DEGRADED_HEADER),
                    axum::http::HeaderValue::from_str(&aggregate.degraded.join(","))
                        .expect("namespace list is header-safe"),
                );
            }
            private_catalog_response(response)
        }
        None => private_catalog_response(error::model_not_found(&id)),
    }
}

async fn catalog_pricing(
    state: &AppState,
    auth: &HeaderMap,
    entries: &[(String, catalog::CatalogEntry)],
) -> Result<pricing::PricingOverlay, Response> {
    let candidates: Vec<_> = entries
        .iter()
        .map(|(provider_id, entry)| pricing::PricingCandidate {
            id: &entry.id,
            provider_id,
            model_id: &entry.native_id,
        })
        .collect();
    pricing::fetch(&state.client, &origins(state), auth, &candidates)
        .await
        .map_err(|error| match error {
            pricing::PricingError::Unauthorized => {
                state.metrics.pricing_failure(PricingFailure::Unauthorized);
                auth_rejected_response()
            }
            pricing::PricingError::Unavailable => {
                state.metrics.pricing_failure(PricingFailure::Unavailable);
                elog::warn("router-pricing", "pricing error for catalog");
                error::pricing_unavailable()
            }
        })
}

fn model_json_with_pricing(
    namespace: &str,
    entry: &catalog::CatalogEntry,
    pricing: &pricing::PricingOverlay,
) -> serde_json::Value {
    let mut model = entry.to_json(namespace);
    let rate = pricing
        .entry(&entry.id)
        .expect("eligible catalog entry has pricing");
    let apitoken = model
        .as_object_mut()
        .expect("catalog model is an object")
        .entry("apitoken")
        .or_insert_with(|| serde_json::json!({}));
    apitoken["pricing"] = rate.public_json();
    model
}

fn private_catalog_response(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
}

/// Единый 401 каталога: ключ проверяет общий billing authority плоскостей,
/// поэтому отказ любой из них однозначен. Конверт OpenAI-совместим, как и
/// сам каталог.
pub(crate) fn auth_rejected_response() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::Json(
            serde_json::json!({"error": {"message": "Invalid or missing API key.",
            "type": "invalid_request_error", "code": "invalid_api_key"}}),
        ),
    )
        .into_response()
}

async fn shutdown_signal() {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    presets::validate_at_startup()?;
    let cfg = Config::from_env()?;
    let state = Arc::new(AppState {
        client: build_client()?,
        catalog: Catalog::new(),
        metrics: Arc::new(RouterMetrics::new()),
        body_admission: Arc::new(Semaphore::new(routing::BODY_ADMISSION_UNITS)),
        cfg,
    });
    let addr: SocketAddr = format!("{}:{}", state.cfg.host, state.cfg.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    elog::info(
        "router",
        format!(
            "claude-router listening on {addr} (anthropic={}, openai={}, gemini={}, fallback_enabled={})",
            state.cfg.anthropic_origin,
            state.cfg.openai_origin,
            state.cfg.gemini_origin,
            state.cfg.fallback_enabled,
        ),
    );
    // Graceful shutdown: после атомарного Caddy cutover SIGTERM прекращает приём новых
    // соединений на старом slot; живые SSE-стримы добиваются до TimeoutStopSec юнита
    // (см. systemd/claude-router@.service и deploy/router-bluegreen.sh).
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
