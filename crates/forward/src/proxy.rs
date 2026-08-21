//! Прозрачный форвардинг (Шаг B): для клиента — обычный api.anthropic.com.
//!
//! Клиент шлёт стандартный Anthropic-запрос (напр. Anthropic SDK с base_url=наш сервер и
//! api_key=наш ключ). Мы:
//!   1) авторизуем клиента по нашему ключу (x-api-key / Bearer);
//!   2) под капотом инжектим Claude Code identity в system + oauth-заголовки (иначе токен
//!      подписки не пускают на /v1/messages) — протокол для клиента при этом НЕ меняется;
//!   3) выбираем наименее загруженную подписку пула, шлём запрос с её Bearer через её прокси;
//!   4) при 429/5xx/протухшем токене — cooling и ротация на следующую подписку;
//!   5) ответ (включая SSE-стрим) отдаём клиенту байт-в-байт.

use crate::execution::{ClientAttribution, LogicalRequestId, RequestLifecycleClock};
use crate::meter::{
    AnthropicAttemptTracker, AnthropicBillableFactContext, BillCtx, CalibrationCtx, MeterCtx,
    SubscriptionMeterCtx, TeeMeter,
};
use crate::metrics::Metrics;
use crate::pricing::tariff_book;
use crate::request_classification::{classify_anthropic_messages, RequestClassification};
use crate::state::AppState;
use crate::upstream::{limits_from_headers, Limits};
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use futures_util::{Stream, StreamExt};
use registry::request_facts::{
    DeliveryState, ProviderTerminalClass, RequestFactAdmission, RequestFactTerminalEvidence,
    TerminalRequestFact, MAX_REQUEST_FACT_MODEL_LEN, MAX_REQUEST_FACT_UPSTREAM_ID_LEN,
};
use serde_json::Value;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::Arc;

const CACHE_ROOT_MIN_WARM_HOMES: usize = 2;
const CACHE_ROOT_MIN_CAPACITY_RATIO: f64 = 0.70;

/// Гард слота конкуррентности персоны. На ЛЮБОМ не-стриминговом исходе попытки (ошибка/ротация/4xx)
/// и — главное — при ОТМЕНЕ запроса (клиент отключился, future хендлера дропнут на await) декрементит
/// in-flight ровно один раз. Разоружается на успехе: слот переходит стриму и снимается в `end_stream`.
/// Без этого гарда `mark_used(+1)` при отмене НЕ откатывался бы → inflight персоны копится до рестарта.
struct InflightGuard {
    pool: Arc<pool::Pool>,
    billing: Option<Arc<crate::billing::AsyncBilling>>,
    capacity_lease_id: Option<String>,
    email: String,
    armed: bool,
}
impl InflightGuard {
    fn new(
        pool: Arc<pool::Pool>,
        billing: Option<Arc<crate::billing::AsyncBilling>>,
        capacity_lease_id: Option<String>,
        email: String,
    ) -> Self {
        Self {
            pool,
            billing,
            capacity_lease_id,
            email,
            armed: true,
        }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for InflightGuard {
    fn drop(&mut self) {
        if self.armed {
            self.pool.mark_done(&self.email);
            if let (Some(billing), Some(lease_id)) = (&self.billing, &self.capacity_lease_id) {
                billing.release_capacity(lease_id);
            }
        }
    }
}

/// Гард резерва баланса. На любом не-успешном исходе И при отмене запроса возвращает удержанный
/// hold клиенту (`settle` с actual=0). Разоружается на успехе — там hold закрывает tee-метеринг
/// фактической стоимостью. Без гарда отмена запроса НАВСЕГДА списывала бы удержанное (деньги клиента).
pub(crate) struct HoldGuard {
    billing: Option<Arc<crate::billing::AsyncBilling>>,
    account_id: String,
    key: String,
    hold: i64,
    request_id: String,
    request_fact: Option<AnthropicBillableFactContext>,
    armed: bool,
}
impl HoldGuard {
    pub(crate) fn new(
        billing: Option<Arc<crate::billing::AsyncBilling>>,
        account_id: String,
        key: String,
        hold: i64,
        request_id: String,
    ) -> Self {
        Self {
            billing,
            account_id,
            key,
            hold,
            request_id,
            request_fact: None,
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }

    fn with_request_fact(mut self, request_fact: AnthropicBillableFactContext) -> Self {
        self.request_fact = Some(request_fact);
        self
    }

    fn record_send(&mut self) {
        if let Some(request_fact) = self.request_fact.as_mut() {
            request_fact.attempts.record_send();
        }
    }

    fn set_terminal_upstream(&mut self, upstream_request_id: Option<String>) {
        if let Some(request_fact) = self.request_fact.as_mut() {
            request_fact.upstream_request_id = upstream_request_id;
        }
    }

    fn take_request_fact(&mut self) -> Option<AnthropicBillableFactContext> {
        self.request_fact.take()
    }

    fn settle_terminal(
        &mut self,
        actual: i64,
        reference: Option<&str>,
        http_status_code: Option<i32>,
        provider_terminal_class: ProviderTerminalClass,
        delivery_state: DeliveryState,
        attempts_exhaustive: bool,
    ) {
        let Some(billing) = self.billing.as_ref() else {
            self.armed = false;
            return;
        };
        let Some(request_fact) = self.request_fact.take() else {
            return;
        };
        let evidence = request_fact.terminal_evidence(
            http_status_code,
            provider_terminal_class,
            delivery_state,
            None,
            attempts_exhaustive,
        );
        if let Err(error) = billing.settle_detached_with_request_fact(
            &self.request_id,
            &self.account_id,
            &self.key,
            self.hold,
            actual,
            reference,
            None,
            evidence,
        ) {
            // Fact admission was already durable. Invalid local terminal evidence must not fall back
            // to a fact-free cancellation; the reconciler owns this true invariant failure.
            elog::error(
                "forward",
                format!("Anthropic request-fact cancellation evidence rejected: {error:#}"),
            );
        }
        self.armed = false;
    }
}
impl Drop for HoldGuard {
    fn drop(&mut self) {
        if self.armed {
            // возврат резерва на аккаунт (actual=0 → ledger-charge не пишется). Drop синхронен —
            // шлём АСИНХРОННО через актор (settle_detached: mpsc::send не блокирует, не требует await).
            if self.request_fact.is_some() {
                // Cancellation/unwind cannot prove an exhaustive send count or public HTTP result.
                // The admitted fact is closed in the same money actor as the reservation refund.
                self.settle_terminal(
                    0,
                    None,
                    None,
                    ProviderTerminalClass::Unknown,
                    DeliveryState::Unknown,
                    false,
                );
            } else if let Some(b) = &self.billing {
                b.settle_detached(
                    &self.request_id,
                    &self.account_id,
                    &self.key,
                    self.hold,
                    0,
                    None,
                    None,
                );
            }
        }
    }
}

type ResponseByteStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>;

/// Privacy-minimal terminal fact for the one native Anthropic nonbillable route in scope. The guard
/// is created only after metered auth, body ownership/JSON admission and typed request context exist.
/// It owns no raw key, headers or request JSON. Explicit `finish` is the normal path; `Drop` is only
/// cancellation/panic safety and records an honest local/unknown terminal without waiting.
struct AnthropicCountTokensFactGuard {
    billing: Arc<crate::billing::AsyncBilling>,
    seed: Option<AnthropicCountTokensFactSeed>,
    internal_attempt_count: Option<usize>,
    send_started: bool,
}

struct AnthropicCountTokensFactSeed {
    logical_request_id: String,
    client_attribution: ClientAttribution,
    execution: registry::ExecutionAttempt,
    account_id: String,
    key_id: String,
    requested_model_candidate: Option<String>,
    classification_candidate: Option<RequestClassification>,
    admitted_at: i64,
    lifecycle_clock: RequestLifecycleClock,
}

#[derive(Clone, Copy)]
struct AnthropicCountTokensTerminalEvidence {
    provider_terminal_class: ProviderTerminalClass,
    delivery_state: DeliveryState,
    internal_attempts_exhaustive: bool,
}

impl AnthropicCountTokensTerminalEvidence {
    fn local(sent_any: bool) -> Self {
        Self {
            provider_terminal_class: ProviderTerminalClass::Unknown,
            delivery_state: if sent_any {
                DeliveryState::Unknown
            } else {
                DeliveryState::NotStarted
            },
            internal_attempts_exhaustive: true,
        }
    }

    fn upstream(status: StatusCode) -> Self {
        let provider_terminal_class = match status.as_u16() {
            200..=299 => ProviderTerminalClass::Success,
            401 | 403 => ProviderTerminalClass::Auth,
            408 => ProviderTerminalClass::Timeout,
            409 | 425 | 500..=599 => ProviderTerminalClass::UpstreamError,
            429 => ProviderTerminalClass::Quota,
            400..=499 => ProviderTerminalClass::ClientError,
            _ => ProviderTerminalClass::Unknown,
        };
        Self {
            provider_terminal_class,
            // Headers prove that the provider started a response, not that the public body was
            // consumed. Terminal submission deliberately does not depend on body polling.
            delivery_state: DeliveryState::Started,
            internal_attempts_exhaustive: true,
        }
    }
}

impl AnthropicCountTokensFactGuard {
    fn new(billing: Arc<crate::billing::AsyncBilling>, seed: AnthropicCountTokensFactSeed) -> Self {
        Self {
            billing,
            seed: Some(seed),
            internal_attempt_count: Some(0),
            send_started: false,
        }
    }

    fn record_send(&mut self) {
        self.send_started = true;
        self.internal_attempt_count = self
            .internal_attempt_count
            .and_then(|count| count.checked_add(1));
    }

    fn finish_local(mut self, response: Response) -> Response {
        let evidence = AnthropicCountTokensTerminalEvidence::local(self.send_started);
        self.submit(Some(response.status()), None, evidence, false);
        response
    }

    fn finish_upstream(
        mut self,
        status: StatusCode,
        upstream_request_id: Option<String>,
        response: Response,
    ) -> Response {
        self.submit(
            Some(status),
            upstream_request_id,
            AnthropicCountTokensTerminalEvidence::upstream(status),
            status.is_success(),
        );
        response
    }

    fn submit(
        &mut self,
        status: Option<StatusCode>,
        upstream_request_id: Option<String>,
        evidence: AnthropicCountTokensTerminalEvidence,
        publish_candidates: bool,
    ) {
        let Some(seed) = self.seed.take() else {
            return;
        };
        let terminal_at = pool::now().max(seed.admitted_at);
        let first_public_byte_at = seed
            .lifecycle_clock
            .seal_first_public_byte_for_terminal(seed.admitted_at, terminal_at);
        let internal_attempt_count = evidence
            .internal_attempts_exhaustive
            .then_some(self.internal_attempt_count)
            .flatten()
            .and_then(|count| i32::try_from(count).ok());
        // The native lane intentionally delegates shape validation to Anthropic. A successful
        // terminal status is the owning parser's exhaustive acceptance proof; on every other status
        // the content-free candidate is discarded rather than treating rejected JSON as validated.
        let classification = publish_candidates
            .then_some(seed.classification_candidate)
            .flatten();
        let fact = TerminalRequestFact {
            logical_request_id: seed.logical_request_id,
            billing_request_id: None,
            execution_group_id: seed.execution.group_id().map(str::to_owned),
            attempt: seed.execution.attempt(),
            account_id: seed.account_id,
            key_id: seed.key_id,
            client_kind: seed.client_attribution.kind(),
            client_source: seed.client_attribution.source(),
            client_version: seed.client_attribution.version().map(str::to_owned),
            provider_plane: "anthropic".into(),
            route_class: "native".into(),
            request_class: "count_tokens".into(),
            // The native lane delegates model validation to Anthropic. A bounded spelling is
            // still only a candidate until the provider accepts the request successfully.
            requested_model: publish_candidates
                .then_some(seed.requested_model_candidate)
                .flatten(),
            // Subscription rotation does not resolve a different executable model. The client
            // spelling is not promoted to execution proof merely because a request was sent.
            executable_model: None,
            stream_flag: false,
            tools_declared_count: classification
                .as_ref()
                .and_then(RequestClassification::tools_declared_count),
            tool_classes: classification
                .as_ref()
                .and_then(RequestClassification::tool_classes),
            tool_choice_mode: classification
                .as_ref()
                .and_then(RequestClassification::tool_choice_mode),
            parallel_tools_requested: classification
                .as_ref()
                .and_then(RequestClassification::parallel_tools_requested),
            tool_results_in_input: classification
                .as_ref()
                .and_then(RequestClassification::tool_results_in_input),
            structured_output_flag: classification
                .as_ref()
                .and_then(RequestClassification::structured_output_flag),
            reasoning_flag: classification
                .as_ref()
                .and_then(RequestClassification::reasoning_flag),
            service_tier: classification
                .as_ref()
                .and_then(RequestClassification::service_tier)
                .map(str::to_owned),
            input_modalities: classification
                .as_ref()
                .and_then(RequestClassification::input_modalities),
            output_modalities: classification
                .as_ref()
                .and_then(RequestClassification::output_modalities),
            admitted_at: seed.admitted_at,
            terminal: RequestFactTerminalEvidence {
                terminal_at,
                http_status_code: status.map(|status| i32::from(status.as_u16())),
                provider_terminal_class: evidence.provider_terminal_class,
                delivery_state: evidence.delivery_state,
                downstream_disconnect: None,
                upstream_request_id,
                first_public_byte_at,
                internal_attempt_count,
                failure_class: None,
                tool_calls_in_output: None,
            },
        };
        let _ = self.billing.try_submit_terminal_request_fact(fact);
    }
}

impl Drop for AnthropicCountTokensFactGuard {
    fn drop(&mut self) {
        if self.seed.is_some() {
            self.submit(
                None,
                None,
                AnthropicCountTokensTerminalEvidence {
                    provider_terminal_class: ProviderTerminalClass::Unknown,
                    // Drop means cancellation or unwind. Even before a send, no normal HTTP
                    // response exists, so preserve only the known transport boundary and never
                    // fabricate a public terminal status or completed delivery.
                    delivery_state: DeliveryState::Unknown,
                    internal_attempts_exhaustive: false,
                },
                false,
            );
        }
    }
}

fn bounded_request_fact_ascii(value: &str, max_len: usize) -> Option<String> {
    (!value.is_empty()
        && value.len() <= max_len
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control()))
    .then(|| value.to_owned())
}

fn bounded_anthropic_request_id(response: &wreq::Response) -> Option<String> {
    let mut values = response.headers().get_all("request-id").iter();
    match (values.next(), values.next()) {
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|value| bounded_request_fact_ascii(value, MAX_REQUEST_FACT_UPSTREAM_ID_LEN)),
        _ => None,
    }
}

/// Заголовок авторитетной семантики исполнения (docs/engine/ROUTING_FENCING.md §3):
/// `not_started` — запрос гарантированно не был исполнен: ни байта публичного ответа клиенту не
/// ушло, reserve по request_id НЕ будет тарифицирован (settle — только refund/cancel), статус не-2xx.
/// Router снимает заголовок с транзитных ответов; наружу его отдаёт только сам движок.
pub(crate) const EXECUTION_STATE_HEADER: &str = "x-apitoken-execution-state";
pub(crate) const EXECUTION_STATE_NOT_STARTED: &str = "not_started";

/// Выставляет `not_started` на ответ. Допустимо ТОЛЬКО при выполнении всех трёх условий
/// контракта (см. EXECUTION_STATE_HEADER): не-2xx, ни байта клиенту не отправлено, reserve
/// уйдёт в refund. На 2xx и после границы доставки — никогда.
pub(crate) fn with_not_started(mut response: Response) -> Response {
    response.headers_mut().insert(
        EXECUTION_STATE_HEADER,
        HeaderValue::from_static(EXECUTION_STATE_NOT_STARTED),
    );
    response
}

/// Снимает `not_started` с ответа. Страховка для веток, где ответ собран через точку,
/// выставляющую заголовок, но условия контракта не выполнены (возможен charge).
pub(crate) fn without_not_started(mut response: Response) -> Response {
    response.headers_mut().remove(EXECUTION_STATE_HEADER);
    response
}

/// True only for the exact public proof consumed by the router: one `not_started` value on a
/// non-success response. Server composition uses the same predicate for its exported counter, so
/// malformed or duplicated headers can never inflate telemetry for a proof the router rejects.
pub fn is_exact_not_started_response(response: &Response) -> bool {
    if response.status().is_success() {
        return false;
    }
    let mut values = response.headers().get_all(EXECUTION_STATE_HEADER).iter();
    matches!(
        (values.next(), values.next()),
        (Some(value), None) if value.as_bytes() == EXECUTION_STATE_NOT_STARTED.as_bytes()
    )
}

// Anthropic Messages принимает тела до 32 МиБ. Держим тот же публичный предел; точное различение
// overflow/read-error ниже сохраняет нативный 413-контракт вместо ложного generic 400.
const BODY_LIMIT: usize = api_limits::current::ANTHROPIC_TEXT_REQUEST.bytes() as usize;

pub(crate) struct MaterializedAnthropicBody {
    pub(crate) bytes: bytes::Bytes,
    pub(crate) _lease: bounded_body::StoredBodyLease,
}

pub(crate) struct MaterializedBoundedBody {
    pub(crate) bytes: bytes::Bytes,
    pub(crate) _lease: bounded_body::StoredBodyLease,
}

pub(crate) enum BodyReadError {
    TooLarge,
    Read,
}

pub(crate) enum BodyAdmitError {
    Storage(bounded_body::StorageError),
    ContentEncoding,
}

pub(crate) const UNSUPPORTED_CONTENT_ENCODING_MESSAGE: &str =
    "Request Content-Encoding is not supported.";

pub(crate) fn request_content_encoding_forbidden(headers: &HeaderMap) -> bool {
    headers
        .get_all(axum::http::header::CONTENT_ENCODING)
        .iter()
        .any(|value| {
            let Ok(value) = value.to_str() else {
                return true;
            };
            value.split(',').any(|part| {
                let token = part.trim();
                !token.is_empty() && !token.eq_ignore_ascii_case("identity")
            })
        })
}

pub(crate) async fn read_body_limited(
    body: Body,
    limit: usize,
) -> Result<bytes::Bytes, BodyReadError> {
    let mut stream = body.into_data_stream();
    let mut out = bytes::BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BodyReadError::Read)?;
        if out.len().saturating_add(chunk.len()) > limit {
            return Err(BodyReadError::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out.freeze())
}

pub(crate) async fn read_body_bounded(
    app: &AppState,
    headers: &HeaderMap,
    body: Body,
    request_limit: api_limits::ByteLimit,
) -> Result<MaterializedBoundedBody, BodyAdmitError> {
    if request_content_encoding_forbidden(headers) {
        crate::metrics::Metrics::inc(&app.metrics.body_admission_content_encoding);
        return Err(BodyAdmitError::ContentEncoding);
    }
    let initial = api_limits::ByteLimit::from_bytes(api_limits::MIB);
    let body_storage = app
        .body_storage()
        .map_err(BodyAdmitError::Storage)?;
    let storage = body_storage
        .storage
        .try_reserve(initial)
        .map_err(|_| {
            crate::metrics::Metrics::inc(&app.metrics.body_admission_overload);
            BodyAdmitError::Storage(bounded_body::StorageError::StorageExhausted)
        })?;
    let memory = body_storage
        .memory
        .try_reserve(initial)
        .map_err(|_| {
            crate::metrics::Metrics::inc(&app.metrics.body_admission_overload);
            BodyAdmitError::Storage(bounded_body::StorageError::MemoryExhausted)
        })?;
    let mut store = bounded_body::BodyStore::start(
        bounded_body::StorageConfig {
            request_limit,
            memory_threshold: body_storage.limits.memory_threshold.min(request_limit),
        },
        &body_storage.storage,
        &body_storage.memory,
        storage,
        memory,
        body_storage.spool.try_clone().map_err(BodyAdmitError::Storage)?,
    )
    .map_err(BodyAdmitError::Storage)?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| BodyAdmitError::Storage(bounded_body::StorageError::Io))?;
        store.push(&chunk).map_err(|error| {
            match error {
                bounded_body::StorageError::TooLarge
                | bounded_body::StorageError::ArithmeticOverflow => {
                    crate::metrics::Metrics::inc(&app.metrics.body_admission_oversized);
                }
                bounded_body::StorageError::StorageExhausted
                | bounded_body::StorageError::MemoryExhausted
                | bounded_body::StorageError::PrivateSpoolUnavailable => {
                    crate::metrics::Metrics::inc(&app.metrics.body_admission_overload);
                }
                _ => {}
            }
            BodyAdmitError::Storage(error)
        })?;
    }
    let stored = store.finish().map_err(BodyAdmitError::Storage)?;
    let (bytes, lease) = stored.into_bytes().map_err(|error| {
        crate::metrics::Metrics::inc(&app.metrics.body_admission_overload);
        BodyAdmitError::Storage(error)
    })?;
    Ok(MaterializedBoundedBody {
        bytes: bytes::Bytes::from(bytes),
        _lease: lease,
    })
}

pub(crate) async fn read_anthropic_body_bounded(
    app: &AppState,
    headers: &HeaderMap,
    body: Body,
) -> Result<MaterializedAnthropicBody, BodyAdmitError> {
    let body_storage = app
        .body_storage()
        .map_err(BodyAdmitError::Storage)?;
    let body = read_body_bounded(
        app,
        headers,
        body,
        body_storage
            .limits
            .request
            .min(api_limits::current::ANTHROPIC_TEXT_REQUEST),
    )
    .await?;
    Ok(MaterializedAnthropicBody {
        bytes: body.bytes,
        _lease: body._lease,
    })
}

// Заголовки клиента, которые НЕ пробрасываем апстриму (перезаписываем или служебные).
fn skip_req_header(name: &str) -> bool {
    // Отпечаток Claude-Code-клиента синтезируем МЫ (x-app, x-stainless-*) → клиентские НЕ пробрасываем:
    // иначе Python-SDK клиент дал бы `x-stainless-lang: python` при нашем claude-cli UA (противоречие).
    if name.starts_with("x-stainless") {
        return true;
    }
    matches!(
        name,
        "x-app" | "anthropic-dangerous-direct-browser-access" | "accept" |
        "host" | "content-length" | "connection" | "authorization" | "x-api-key"
        | "x-goog-api-key"
        | "anthropic-beta" | "anthropic-version" | "user-agent" | "accept-encoding"
        | "x-claude-code-session-id" | "x-conversation-id" | "x-session-id"
        | "x-apitoken-api-plane"
        | "x-apitoken-calibration-profile"
        | "x-apitoken-execution-group" | "x-apitoken-attempt"
        | "transfer-encoding" | "upgrade" | "proxy-connection" | "proxy-authorization"
        | "keep-alive" | "te" | "trailer"
        // Клиентские forwarding/hop-заголовки НЕ пробрасываем апстриму: они раскрыли бы цепочку прокси
        // и IP клиента (рассинхрон с IP нашего egress-прокси → подрыв антифингерпринта флота).
        | "x-forwarded-for" | "x-forwarded-host" | "x-forwarded-proto" | "forwarded"
        | "x-real-ip" | "via" | "cf-connecting-ip" | "true-client-ip"
    )
}

const CALIBRATION_PROFILE_HEADER: &str = "x-apitoken-calibration-profile";

/// Bounded operator identifier shared with the protected capacity report. Full subscription
/// identity never enters the request or report, and a collision is rejected by the pool.
fn calibration_profile_hint(email: &str) -> String {
    email
        .split('@')
        .next()
        .unwrap_or(email)
        .chars()
        .take(4)
        .collect()
}

/// Exact profile targeting is deliberately unavailable to metered/control/panel credentials.
/// Forwarding-admin traffic is already unmetered and trusted to exercise live subscriptions; this
/// header only makes a bounded calibration run attributable instead of changing its authority.
fn operator_calibration_target<'a>(authz: &Authz, headers: &'a HeaderMap) -> Option<&'a str> {
    if !matches!(authz, Authz::Admin { .. }) {
        return None;
    }
    headers
        .get(CALIBRATION_PROFILE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            (1..=4).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}
// Заголовки апстрима, которые НЕ отдаём клиенту: hop-by-hop + per-ПОДПИСОЧНЫЕ ratelimit/идентити.
// `anthropic-ratelimit-*` отражают состояние НАШЕЙ подписки (утилизация/reset пула) — отдавать их
// клиенту (а) раскрывает, что это пул, (б) даёт «прыгающий» несогласованный лимит при ротации.
// Аналогично режем org/account-идентифицирующие, различающиеся между подписками (корреляция аккаунта).
fn skip_resp_header(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "transfer-encoding"
            | "content-length"
            | "content-encoding"
            | "keep-alive"
            | "proxy-connection"
            | "upgrade"
            | "te"
            | "trailer"
    ) || name.starts_with("anthropic-ratelimit")
        || name.starts_with("anthropic-organization")
        || name == "anthropic-account-id"
}

/// Все клиентские credential из запроса. Claude Code может одновременно прислать
/// `x-api-key` из унаследованного `ANTHROPIC_API_KEY` и `Authorization: Bearer` из
/// `ANTHROPIC_AUTH_TOKEN`. Ни один заголовок не имеет приоритета: возвращаем уникальный
/// отсортированный набор, чтобы выбор валидной биллинговой identity не зависел от
/// типа или порядка заголовков. Публично используется `server` для `/balance` и audit.
pub fn client_keys(headers: &HeaderMap) -> Vec<String> {
    let mut keys = std::collections::BTreeSet::new();
    for name in ["x-api-key", "x-goog-api-key"] {
        for value in headers.get_all(name).iter() {
            if let Ok(value) = value.to_str() {
                let key = value.trim();
                if !key.is_empty() {
                    keys.insert(key.to_string());
                }
            }
        }
    }
    for value in headers.get_all("authorization").iter() {
        let Ok(value) = value.to_str() else { continue };
        let Some((scheme, token)) = value.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let token = token.trim();
        if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
            keys.insert(token.to_string());
        }
    }
    keys.into_iter().collect()
}

/// Админ-доступ: env-ключи `CLAUDE_API_KEYS`, либо loopback-пир ТОЛЬКО если `trust_loopback`
/// (сервер реально слушает loopback). За реверс-прокси (bind 0.0.0.0) trust_loopback=false →
/// пустые ключи означают «отклонять всё», а не «доверять любому 127.0.0.1». Без биллинга.
/// Сравнение в константное время (по длине совпадающих строк): не выходим рано на первом различии,
/// чтобы не давать timing-oracle на угадывание админ-ключа по байту. Длину строк не скрываем
/// (не секрет). Без внешних зависимостей.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if app.cfg.api_keys.is_empty() {
        return app.cfg.trust_loopback && peer.ip().is_loopback();
    }
    matching_key(headers, &app.cfg.api_keys).is_some()
}

/// Совпал ли клиентский ключ с любым из `allow` (constant-time по каждому). Пустой `allow` → false.
fn key_in(headers: &HeaderMap, allow: &[String]) -> bool {
    matching_key(headers, allow).is_some()
}

/// Детерминированно вернуть любой допущенный credential. Все пары всё равно
/// сравниваются: невалидный `x-api-key` не может затмить валидный Bearer и наоборот.
fn matching_key(headers: &HeaderMap, allow: &[String]) -> Option<String> {
    let mut matched = None;
    for candidate in client_keys(headers) {
        let candidate_matches = allow.iter().fold(false, |ok, allowed| {
            ok | ct_eq(allowed.as_bytes(), candidate.as_bytes())
        });
        if candidate_matches && matched.is_none() {
            matched = Some(candidate);
        }
    }
    matched
}

/// Control-плоскость (`/admin/*`): admin-ключ ИЛИ control-ключ ИЛИ (нет ни того ни другого →
/// loopback-админ). Отдельный класс, чтобы коммерция управляла аккаунтами своим секретом, не имея
/// прав неметеренного форвардинга.
pub fn control_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if authed(app, headers, peer) {
        return true;
    } // admin-ключ/loopback покрывает control
    if key_in(headers, &app.cfg.control_keys) {
        return true;
    }
    // Ни admin, ни control-ключей не задано → доверяем loopback (dev). Иначе — только по ключу.
    app.cfg.api_keys.is_empty()
        && app.cfg.control_keys.is_empty()
        && app.cfg.trust_loopback
        && peer.ip().is_loopback()
}

/// Read-only дашборды (`/capacity`, `/metrics`): admin ИЛИ control ИЛИ panel-ключ. Панель смотрит
/// ёмкость своим низкопривилегированным ключом — без прав /admin/* и без форвардинга.
pub fn readonly_authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    control_authed(app, headers, peer) || key_in(headers, &app.cfg.panel_keys)
}

/// Результат авторизации запроса.
pub(crate) enum Authz {
    /// Админ (env-ключ/localhost) — без тарификации.
    Admin { affinity_scope: String },
    /// Ключ клиента → АККАУНТ с балансом. Тарифицируем и списываем с БАЛАНСА АККАУНТА (общего на
    /// все ключи юзера); `key` остаётся raw secret для существующих reserve/settle calls, while
    /// `key_id` is its authoritative non-secret identity for future request facts. `mult_bp` —
    /// скидка аккаунта по умолчанию, `provider_mult_bp` — её переопределения по провайдерам (B2B).
    /// `balance_nano` несём из авторизации → резерв-блок НЕ перечитывает баланс из БД (−1 запрос).
    Metered {
        account_id: String,
        key: String,
        #[allow(dead_code)]
        key_id: String,
        mult_bp: i64,
        provider_mult_bp: Vec<(String, i64)>,
        available_nano: i64,
    },
    /// Ключ неизвестен/заблокирован (ключ или аккаунт).
    Unauthorized,
    /// Billing authority could not answer. This must stay distinct from an unknown key so a
    /// transient database outage never turns valid credentials into a misleading 401.
    Unavailable,
}

impl Authz {
    pub(crate) fn affinity_scope(&self) -> Option<&str> {
        match self {
            Authz::Admin { affinity_scope } => Some(affinity_scope),
            Authz::Metered { account_id, .. } => Some(account_id),
            Authz::Unauthorized | Authz::Unavailable => None,
        }
    }

    /// The discount that prices a request on `provider_id`: the account's per-provider override
    /// when it has one, otherwise its default. Every provider plane resolves it the same way, so
    /// a B2B customer can hold different terms per provider without a second pricing concept.
    pub(crate) fn mult_for(&self, provider_id: &str) -> i64 {
        match self {
            Authz::Metered {
                mult_bp,
                provider_mult_bp,
                ..
            } => provider_mult_bp
                .iter()
                .find(|(provider, _)| provider == provider_id)
                .map(|(_, provider_mult)| *provider_mult)
                .unwrap_or(*mult_bp),
            _ => 0,
        }
    }
}

/// Порядок для МАСШТАБА: сначала админ (env-ключ/loopback) — проверка В ПАМЯТИ, без похода в БД
/// (админ-трафик не грузит биллинг-мьютекс). Только если не админ — клиентский ключ → аккаунт
/// (ОДНА DB-выборка, несёт баланс для резерва → без повторного чтения).
pub(crate) async fn authorize(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Authz {
    if let Some(credential) = matching_key(headers, &app.cfg.api_keys) {
        return Authz::Admin {
            affinity_scope: credential,
        };
    }
    if app.cfg.api_keys.is_empty() && app.cfg.trust_loopback && peer.ip().is_loopback() {
        return Authz::Admin {
            affinity_scope: "loopback-admin".to_string(),
        };
    }
    if let Some(billing) = &app.billing {
        match resolve_client_key(billing, headers).await {
            Ok(Some((k, a))) => {
                let key_remaining_nano = a.spend_limit_nano.map(|limit| {
                    limit
                        .saturating_sub(a.spent_nano)
                        .saturating_sub(a.reserved_nano)
                        .max(0)
                });
                let available_nano = key_remaining_nano
                    .map(|remaining| remaining.min(a.balance_nano))
                    .unwrap_or(a.balance_nano);
                return Authz::Metered {
                    account_id: a.account_id,
                    key: k,
                    key_id: a.key_id,
                    mult_bp: a.mult_bp,
                    provider_mult_bp: a.provider_mult_bp,
                    available_nano,
                };
            }
            Ok(None) => {}
            Err(error) => {
                elog::error(
                    "forward",
                    format!("billing key authorization failed: {error:#}"),
                );
                return Authz::Unavailable;
            }
        }
    }
    Authz::Unauthorized
}

/// Резолв любого валидного метерного credential. Порядок детерминирован самими
/// значениями, а не типом заголовка. Ошибка authority не затмит уже найденный валидный
/// ключ; но если ни один не подтверждён, транзиентный сбой остаётся 503, а не ложным 401.
pub async fn resolve_client_key(
    billing: &crate::billing::AsyncBilling,
    headers: &HeaderMap,
) -> anyhow::Result<Option<(String, registry::KeyAuth)>> {
    resolve_client_keys(billing, &client_keys(headers)).await
}

/// Тот же OR-резолв для middleware, который должен извлечь credential до передачи request дальше.
pub async fn resolve_client_keys(
    billing: &crate::billing::AsyncBilling,
    keys: &[String],
) -> anyhow::Result<Option<(String, registry::KeyAuth)>> {
    let mut first_error = None;
    for key in keys {
        match billing.key_auth(key).await {
            Ok(Some(auth)) if auth.active_at(pool::now()) => return Ok(Some((key.clone(), auth))),
            Ok(Some(_)) | Ok(None) => {}
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

/// Anthropic-подобная ошибка (чтобы SDK-клиент видел привычную форму).
fn err_response(code: StatusCode, kind: &str, msg: &str) -> Response {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": msg}});
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Ошибка с `Retry-After` — прозрачно для клиента (как настоящий Anthropic при перегрузе/лимите):
/// SDK сам откатится на указанные секунды вместо слепого ретрая.
fn err_retry(code: StatusCode, kind: &str, msg: &str, retry_after: i64) -> Response {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": msg}});
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .header("retry-after", retry_after.max(1).to_string())
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// HTTP-статус `overloaded_error` у НАСТОЯЩЕГО Anthropic — 529 (не 503). Держим точь-в-точь,
/// чтобы синтетический ответ был неотличим от api.anthropic.com (SDK Anthropic знает 529 как
/// retryable overloaded). Через `from_u16` — 529 вне «именованных» констант `http`.
fn http_overloaded() -> StatusCode {
    StatusCode::from_u16(529).expect("529 is a valid HTTP status code")
}

/// Внутренняя причина СИНТЕТИЧЕСКОЙ (не-upstream) ошибки движка. Единственная точка, где рождаются
/// НАШИ ответы клиенту. НИКОГДА не сериализуется как есть — только выбирает Anthropic-аутентичный
/// публичный триплет `{status, error.type, message}`. Смысл: клиент считает, что говорит с
/// api.anthropic.com, и НЕ должен видеть наши внутренности («subscription/pool/upstream/billing
/// authority/cooling»). Настоящую причину несут метрики и локальный лог (`eprintln`), не тело ответа.
///
/// Что клиент ЗАКОННО должен знать (контракт продукта, задокументирован в docs-portal) — остаётся:
/// `InvalidKey` (401), `LowBalance` (402 — состояние аккаунта, «пополни баланс»), `NotFound` (404),
/// `BodyTooLarge`/`BadRequest`/`BadBeta` (4xx валидация). Всё «наше» (нет ёмкости/пул пуст/брейкер/
/// authority/сбой апстрим-соединения) схлопывается в ретраибл `Overloaded`/`RateLimited`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LocalErr {
    /// Транзиентная нехватка ёмкости на НАШЕЙ стороне: пул пуст, breaker разомкнут (брауноут
    /// апстрима), authority недоступен/зафенсен, сбой соединения/сети с апстримом, серверная
    /// перегрузка. Клиенту — 529 `overloaded_error` «Overloaded» (retryable), опц. Retry-After.
    Overloaded,
    /// Весь флот за лимитом (все подписки cooling) ИЛИ клиент превысил свою конкурентность.
    /// Клиенту это выглядит как обычный рейт-лимит: 429 `rate_limit_error` + Retry-After.
    RateLimited,
    /// Неизвестный/заблокированный клиентский ключ. 401 `authentication_error`.
    InvalidKey,
    /// Недостаточно средств на балансе аккаунта или достигнут лимит расхода ключа. Это ЗАКОННАЯ
    /// ошибка состояния аккаунта (клиент — платящий пользователь с предоплаченным балансом; docs
    /// определяют 402 именно так). 402 `invalid_request_error`.
    LowBalance,
    /// Эндпоинт не поддерживается на квоте подписки. 404 `not_found_error`.
    NotFound,
    /// Тело запроса больше лимита. 413 `request_too_large`.
    BodyTooLarge,
    /// Compressed request bodies are forbidden on materializing text routes. 415.
    UnsupportedContentEncoding,
    /// Не удалось прочитать/разобрать тело запроса. 400 `invalid_request_error`.
    BadRequest,
    /// Некорректный `anthropic-beta` заголовок. 400 `invalid_request_error`.
    BadBeta,
    /// Внутренний сбой движка (сериализация тела/сборка ответа). 500 `api_error`.
    Internal,
}

/// Privacy-safe terminal classification carried in response extensions for the server audit
/// middleware. Values must remain static reason codes: never put keys, prompts, upstream text,
/// subscription identities, or other request-derived data here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalErrorReason(pub &'static str);

impl LocalErr {
    /// Публичный триплет `(status, error.type, message)`. ТОЛЬКО аутентичные Anthropic-тексты и типы;
    /// ни одного упоминания подписок/пула/upstream/authority/cooling.
    fn parts(self) -> (StatusCode, &'static str, &'static str) {
        match self {
            LocalErr::Overloaded => (http_overloaded(), "overloaded_error", "Overloaded"),
            LocalErr::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_error",
                "Number of requests has exceeded your rate limit. Please try again later.",
            ),
            LocalErr::InvalidKey => (
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "invalid x-api-key",
            ),
            LocalErr::LowBalance => (
                StatusCode::PAYMENT_REQUIRED,
                "invalid_request_error",
                "insufficient balance or key spending limit reached for this request",
            ),
            LocalErr::NotFound => (StatusCode::NOT_FOUND, "not_found_error", "Not Found"),
            LocalErr::BodyTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "Request exceeds the maximum allowed number of bytes.",
            ),
            LocalErr::UnsupportedContentEncoding => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "invalid_request_error",
                "Request Content-Encoding is not supported.",
            ),
            LocalErr::BadRequest => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "Could not parse request body.",
            ),
            LocalErr::BadBeta => (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "invalid anthropic-beta header",
            ),
            LocalErr::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                "Internal server error",
            ),
        }
    }

    fn reason(self) -> &'static str {
        match self {
            LocalErr::Overloaded => "overloaded",
            LocalErr::RateLimited => "rate_limited",
            LocalErr::InvalidKey => "invalid_key",
            LocalErr::LowBalance => "billing_limit",
            LocalErr::NotFound => "unsupported_endpoint",
            LocalErr::BodyTooLarge => "body_too_large",
            LocalErr::UnsupportedContentEncoding => "unsupported_content_encoding",
            LocalErr::BadRequest => "invalid_request_body",
            LocalErr::BadBeta => "invalid_beta_header",
            LocalErr::Internal => "internal_response_error",
        }
    }
}

/// ЕДИНЫЙ санитайзер синтетических ошибок: внутренняя причина → Anthropic-аутентичный публичный
/// ответ. `retry_after` (сек) добавляет `Retry-After` для retryable-причин. Настоящая причина
/// НЕ попадает в тело — она уже отражена в метриках/логах у места вызова.
fn local_err(reason: LocalErr, retry_after: Option<i64>) -> Response {
    local_err_for(reason, reason.reason(), retry_after)
}

pub(crate) fn local_err_for(
    reason: LocalErr,
    terminal_reason: &'static str,
    retry_after: Option<i64>,
) -> Response {
    let (code, kind, msg) = reason.parts();
    let mut response = match retry_after {
        Some(secs) => err_retry(code, kind, msg, secs),
        None => err_response(code, kind, msg),
    };
    response
        .extensions_mut()
        .insert(TerminalErrorReason(terminal_reason));
    // A synthetic error never carried an upstream `request-id`, so a customer reporting one of
    // these had nothing to quote and support nothing to search on. The audit event logs whatever id
    // the response carries, so setting it here makes the two match.
    if let Ok(value) = HeaderValue::from_str(&crate::fresh_request_id()) {
        response.headers_mut().insert("x-request-id", value);
    }
    // Все ответы local_err_for — синтетические не-2xx ДО границы доставки: reserve по request_id
    // (если был) закрывает armed HoldGuard refund'ом. Ветки после границы снимают заголовок через
    // without_not_started (delivery-marker-failed с full-hold charge, fallback сборки stream_back).
    with_not_started(response)
}

/// Точный баланс-лимит метерного ключа: сколько OUTPUT-токенов и какой hold клиент может позволить
/// остатком баланса `bal` (client-нанодоллары, т.е. с учётом наценки). Гарантии («ни на токен/цент
/// больше баланса»):
///   • `None` → баланса не хватает даже на ВХОД worst-case → запрос отклоняется (иначе вход мог бы
///     пробить баланс, ведь input тарифицируется всегда, даже при output=0);
///   • `hold ≤ bal` (по построению + кламп) → атомарный reserve не уводит баланс в минус;
///   • при возвращённом `eff_mt`: реальная стоимость (usage ≤ input_est токенов входа + `eff_mt`
///     output + web_buf) ≤ hold — т.к. affordable округляем ВНИЗ и вход оценён сверху (байты ≥ токены).
/// Anthropic, получив урезанный `max_tokens=eff_mt`, останавливает генерацию ровно на доступном
/// токене — это и есть «отруб посреди запроса» без mid-stream-хаков.
/// Дефолт лимита web-поисков, если web_search включён без явного `max_uses` — консервативная оценка
/// для резерва их стоимости под баланс (реальный `max_uses` из тела приоритетен).
const DEFAULT_WEB_USES: u64 = 20;

fn cap_to_balance(
    bal: i128,
    input_est: i128,
    web_buf: i128,
    p: &metering::Prices,
    mult_bp: i64,
    client_mt: u64,
) -> Option<(u64, i64)> {
    // Наценка ≤ 0 = бесплатный ключ (charge всегда 0) → не лимитируем, hold 0 (баланс не двигается).
    if mult_bp <= 0 {
        return Some((client_mt, 0));
    }
    // Овердрафт-буфер: доступное = balance + $1. Funded-юзера НЕ роняем 402 из-за гонки конкурентных
    // резервов — атомарный резерв (registry) всё равно держит пол баланса на −$1. За полом (bal ≤ −$1) → None.
    let ceil = bal + metering::OVERDRAFT_NANO;
    if ceil <= 0 {
        return None;
    }
    // Работаем в RAW-нано (до наценки). Максимальная RAW-стоимость `x_max`, чья КЛИЕНТСКАЯ цена
    // (apply_multiplier, round-half-up) гарантированно ≤ ceil (=bal+$1): X ≤ ⌊ceil·10000/m⌋ → X·m ≤
    // ceil·10000 → (X·m+5000)/10000 ≤ ceil. Так `hold=apply_multiplier(ceiling) ≤ bal+$1` по построению.
    let x_max = ceil.saturating_mul(10000) / (mult_bp as i128);
    let fixed_raw = input_est * p.cache_write_1h + web_buf; // вход worst-case + буфер поисков (RAW)
    if fixed_raw > x_max {
        return None;
    } // не тянет даже вход worst-case → 402
    let out = p.output.max(1);
    let affordable = ((x_max - fixed_raw) / out).max(0) as u64; // сколько output-токенов влезает в x_max
    if affordable == 0 {
        return None;
    }
    let eff_mt = client_mt.min(affordable);
    // ceiling_raw ≤ x_max (по построению) → hold = apply_multiplier(ceiling_raw) ≤ bal. А реальный
    // charge = apply_multiplier(real_raw) ≤ hold, т.к. real_raw ≤ ceiling_raw (реальных input-токенов
    // ≤ байт=input_est, ставка входа ≤ cw1h, output ≤ eff_mt). Итог: charge ≤ hold ≤ bal — жёстко.
    let ceiling_raw = fixed_raw + (eff_mt as i128) * p.output;
    // Кламп к i64::MAX: овердрафт-буфер (bal+$1) при абсурдном балансе мог бы толкнуть hold за i64,
    // и `as i64` обернул бы его в отрицательный. Реальные балансы недостижимы близко к i64::MAX.
    let hold = metering::apply_multiplier(ceiling_raw, mult_bp).min(i64::MAX as i128) as i64;
    Some((eff_mt, hold))
}

/// Инжект Claude Code identity первым system-блоком (если его там ещё нет).
/// Первый system-блок уже несёт Claude-Code-идентичность? (billing-header/identity — как шлёт
/// САМ Claude Code). Тогда повторно инжектить не надо — иначе получим двойную identity.
fn is_cc_marker(text: &str) -> bool {
    text.starts_with("x-anthropic-billing-header:")
        || text.starts_with("You are Claude Code")
        || text.starts_with("You are a Claude agent")
}

fn inject_identity(v: &mut Value, identity: &str) -> bool {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return false,
    };
    if !obj.contains_key("messages") {
        return false;
    } // не messages-запрос — не трогаем
      // identity-блок БЕЗ cache_control — ТОЧНО как реальный CC (снято с 2.1.195: system[identity] не
      // имеет cache_control; брейкпоинты CC ставит на БОЛЬШОЙ системный промпт, а не на identity).
      // cache_control на нашем identity был бы отпечатком («этот клиент кэш-контролит identity, а CC — нет»).
      // Экономику кэша это не рушит: если клиент сам ставит брейкпоинт ниже, наш identity попадёт в его
      // кэш-префикс (кэшируется всё ДО брейкпоинта включительно). session_key считаем отдельно (не зависит).
    let idblock = serde_json::json!({"type":"text","text":identity});
    match obj.get("system").cloned() {
        None => {
            obj.insert("system".into(), serde_json::json!([idblock]));
        }
        Some(Value::String(s)) => {
            if is_cc_marker(&s) {
                return false;
            } // клиент прислал identity строкой — не дублируем
            obj.insert(
                "system".into(),
                serde_json::json!([idblock, {"type":"text","text":s}]),
            );
        }
        Some(Value::Array(mut arr)) => {
            let first_cc = arr
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                .map(is_cc_marker)
                .unwrap_or(false);
            if first_cc {
                return false;
            } // уже Claude-Code-запрос (напр. сам Claude Code)
            arr.insert(0, idblock);
            obj.insert("system".into(), Value::Array(arr));
        }
        _ => return false,
    }
    true
}

/// Препендит/обновляет system[0] = billing-header блок ИДЕМПОТЕНТНО (на ротации заменяет текст,
/// не дублирует). Реальный CC шлёт его первым system-блоком, БЕЗ cache_control (снято с 2.1.195).
/// Работает только по массиву system (identity-инжект уже гарантирует массив на messages-запросе).
fn set_billing_block(v: &mut Value, text: &str) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    let block = serde_json::json!({"type":"text","text":text});
    if let Some(Value::Array(arr)) = obj.get_mut("system") {
        let first_billing = arr
            .first()
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .map(|t| t.starts_with("x-anthropic-billing-header:"))
            .unwrap_or(false);
        if first_billing {
            arr[0] = block;
        } else {
            arr.insert(0, block);
        }
    }
}

/// Сливает клиентские beta-флаги с МИНИМАЛЬНЫМИ identity-флагами OAuth/Claude Code.
/// Feature-беты из fleet-конфига не навязываем обычному SDK-клиенту: реальный Claude Code пришлёт
/// свой полный набор сам, а произвольный клиент сохранит ровно запрошенные capabilities.
fn merged_beta(headers: &HeaderMap, configured: &str) -> Result<String, ()> {
    fn push(raw: &str, identity_only: bool, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if identity_only && !(token.starts_with("oauth-") || token.starts_with("claude-code-"))
            {
                continue;
            }
            if seen.insert(token.to_string()) {
                out.push(token.to_string());
            }
        }
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in headers.get_all("anthropic-beta").iter() {
        push(value.to_str().map_err(|_| ())?, false, &mut seen, &mut out);
    }
    push(configured, true, &mut seen, &mut out);
    Ok(out.join(","))
}

/// Perform the single last-resort external attempt. A successful HTTP response is returned to the
/// caller for the ordinary delivery marker and exact usage meter; every failure is deliberately
/// collapsed back into the already-computed local terminal response so ClaudeStore credential,
/// balance and infrastructure details never cross the public boundary.
enum ClaudeStoreAttempt {
    Response(wreq::Response),
    BeforeSend,
    Transport,
    Http(StatusCode, Option<String>),
}

async fn attempt_claudestore_fallback(
    app: &AppState,
    config: &crate::config::ClaudeStoreFallbackConfig,
    body: bytes::Bytes,
    anthropic_version: &str,
    client_beta: &str,
    hold_guard: &mut HoldGuard,
) -> ClaudeStoreAttempt {
    Metrics::inc(&app.metrics.claudestore_fallback_attempts);
    // A dedicated cache identity creates a direct connection pool that is never shared with a
    // subscription proxy/TLS session. No local OAuth or persona header is attached below.
    let client = match app.clients.get("", "claudestore-fallback") {
        Ok(client) => client,
        Err(error) => {
            Metrics::inc(&app.metrics.claudestore_fallback_failures);
            elog::warn(
                "forward",
                format!("ClaudeStore fallback client unavailable: {error}"),
            );
            return ClaudeStoreAttempt::BeforeSend;
        }
    };
    let url = format!("{}/v1/messages", config.base_url().trim_end_matches('/'));
    let mut request = client
        .request(Method::POST, url)
        .redirect(wreq::redirect::Policy::none())
        .header("x-api-key", config.api_key())
        .header("anthropic-version", anthropic_version)
        .header("content-type", "application/json")
        .body(body);
    if !client_beta.is_empty() {
        request = request.header("anthropic-beta", client_beta);
    }
    hold_guard.record_send();
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            Metrics::inc(&app.metrics.claudestore_fallback_failures);
            elog::warn(
                "forward",
                format!("ClaudeStore fallback transport failed: {error}"),
            );
            return ClaudeStoreAttempt::Transport;
        }
    };
    if !response.status().is_success() {
        Metrics::inc(&app.metrics.claudestore_fallback_failures);
        let status = response.status();
        let request_id = bounded_anthropic_request_id(&response);
        elog::warn(
            "forward",
            format!(
                "ClaudeStore fallback returned terminal status {}",
                status.as_u16()
            ),
        );
        return ClaudeStoreAttempt::Http(status, request_id);
    }
    ClaudeStoreAttempt::Response(response)
}

/// Claude-Code persona нужна для OAuth-поведения, но документированную атрибуцию клиента нельзя
/// перезаписывать. Добавляем persona только когда metadata/user_id отсутствуют; malformed metadata
/// оставляем как есть, чтобы Anthropic вернул свою нативную validation error вместо panic шлюза.
fn set_persona_user_id_if_absent(v: &mut Value, user_id: String) {
    let Some(obj) = v.as_object_mut() else { return };
    if obj.get("metadata").map(|m| m.is_null()).unwrap_or(true) {
        obj.insert("metadata".into(), serde_json::json!({"user_id": user_id}));
    } else if let Some(Value::Object(metadata)) = obj.get_mut("metadata") {
        if !metadata.contains_key("user_id") {
            metadata.insert("user_id".into(), Value::String(user_id));
        }
    }
    // Existing non-object metadata is intentionally untouched so upstream returns its native 4xx.
    // AUDIT-TODO(C52): move internal persona attribution to a private upstream channel and stop adding public metadata.
}

/// Allowlist эндпоинтов Anthropic, работающих на квоте ПОДПИСКИ (что мы форвардим на пул):
/// `POST /v1/messages` (метерим), `POST /v1/messages/count_tokens` и `GET /v1/models[/{id}]` (проброс
/// без тарификации). Всё прочее (batches/files/agents/sessions/environments/skills/complete) на
/// подписочном OAuth-токене недоступно → чистый 404 на шлюзе. Управляющие роуты (`/health` и др.)
/// сюда НЕ доходят — их обслуживает `server` до fallback на `forward`.
fn is_supported_endpoint(method: &Method, path: &str) -> bool {
    // Модельный id — ровно ОДИН непустой raw-сегмент. `%` отклоняем целиком: model ids не требуют
    // percent-encoding, зато URL-парсер нормализует `%2e`, `%2f` и варианты регистра уже ПОСЛЕ allowlist.
    // Также запрещаем backslash (separator на части URL/proxy стеков) и literal dot/empty-сегменты.
    if path.contains('%')
        || path.contains('\\')
        || path.contains("//")
        || path.split('/').any(|seg| seg == "." || seg == "..")
    {
        return false;
    }
    match (method.as_str(), path) {
        ("POST", "/v1/messages") | ("POST", "/v1/messages/count_tokens") => true,
        ("GET", "/v1/models") => true,
        ("GET", p) => p
            .strip_prefix("/v1/models/")
            .map(|id| !id.is_empty() && !id.contains('/'))
            .unwrap_or(false),
        _ => false,
    }
}

/// Namespaced catalog id (`anthropic/<native id>`) → native id в теле запроса
/// native lane. Universal dispatch (этапы 3–5 UNIFIED_ROUTER) проксирует тело
/// байт-идентично, поэтому префикс доезжает до плоскости как есть; admission/
/// reserve и upstream Anthropic ждут нативный id (зеркало strip'а
/// chat-адаптера `anthropic.rs`). Возвращает итоговый id для локального
/// использования; при снятии префикса поле `model` тела переписывается.
fn strip_own_namespace(v: &mut Value) -> String {
    let model = v
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match model.strip_prefix("anthropic/") {
        Some(stripped) => {
            let stripped = stripped.to_string();
            if let Some(slot) = v.get_mut("model") {
                *slot = Value::String(stripped.clone());
            }
            stripped
        }
        None => model,
    }
}

pub async fn forward(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let typed_logical_request_id = req.extensions().get::<LogicalRequestId>().cloned();
    let typed_client_attribution = req.extensions().get::<ClientAttribution>().cloned();
    let typed_lifecycle_clock = req.extensions().get::<RequestLifecycleClock>().cloned();
    let synthesized_messages_origin = req
        .extensions()
        .get::<crate::execution::SynthesizedMessagesOrigin>()
        .cloned();
    if !app.authority_ready.load(AtomicOrdering::Acquire) {
        // Инстанс зафенсен/authority недоступен — НАША внутренняя причина. Клиенту — транзиентный
        // retryable overload (ретрай, вероятно, попадёт на здоровый инстанс), без слова «authority».
        return local_err_for(
            LocalErr::Overloaded,
            "billing_authority_unavailable",
            Some(2),
        );
    }
    let (parts, body) = req.into_parts();
    let execution = match crate::execution::parse_execution_attempt(&parts.headers) {
        Ok(execution) => execution,
        Err(error) => {
            elog::error(
                "forward",
                format!(
                    "Anthropic execution identity rejected class={}",
                    error.as_str()
                ),
            );
            return with_not_started(local_err_for(
                LocalErr::Overloaded,
                "invalid_execution_identity",
                Some(2),
            ));
        }
    };
    let billable = parts.method == Method::POST && parts.uri.path() == "/v1/messages";
    let authz = authorize(&app, &parts.headers, &peer).await;
    match &authz {
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return local_err(LocalErr::InvalidKey, None);
        }
        Authz::Unavailable => {
            return local_err_for(
                LocalErr::Overloaded,
                "billing_authority_unavailable",
                Some(2),
            )
        }
        Authz::Admin { .. } | Authz::Metered { .. } => {}
    }
    let operator_target = operator_calibration_target(&authz, &parts.headers).map(str::to_owned);
    // ALLOWLIST эндпоинтов: форвардим на пул ТОЛЬКО то, что доступно на квоте ПОДПИСКИ Claude Max
    // (messages/count_tokens/models). Batches/Files/Agents/Sessions требуют scope OAuth-токена
    // (user:batch/developer), которого у подписки НЕТ → на них Anthropic отдаёт 403/401/404. Не роутим
    // их на подписку (иначе застудили бы её и слили бы backend-scope в ошибке), а отдаём чистый 404.
    if !is_supported_endpoint(&parts.method, parts.uri.path()) {
        return local_err(LocalErr::NotFound, None);
    }
    let native_count_tokens = app.provider.serves_anthropic()
        && parts.method == Method::POST
        && parts.uri.path() == "/v1/messages/count_tokens"
        && matches!(authz, Authz::Metered { .. });
    // Model-specific release resolution below decides balance vs service meter_only. Rejecting a
    // zero balance here would incorrectly block service accounts before their release assignment.
    Metrics::inc(&app.metrics.requests);
    let method: Method = parts.method.clone();
    let pq = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    // Реальный CC шлёт `/v1/messages?beta=true` (снято с живого claude 2.1.195). Добавляем query
    // `beta=true` ровно на POST /v1/messages, если его там ещё нет (мержим с существующим query,
    // не ломая клиентские параметры). Остальные пути — байт-в-байт как пришли.
    let pq_owned;
    let pq: &str = if method == Method::POST
        && parts.uri.path() == "/v1/messages"
        && !parts
            .uri
            .query()
            .map(|q| q.split('&').any(|kv| kv == "beta=true"))
            .unwrap_or(false)
    {
        let sep = if parts.uri.query().is_some() {
            '&'
        } else {
            '?'
        };
        pq_owned = format!("{pq}{sep}beta=true");
        &pq_owned
    } else {
        pq
    };
    let url = format!("{}{}", app.cfg.upstream.trim_end_matches('/'), pq);

    // Universal adapters already admitted and retain the original customer body. Avoid charging a
    // second full raw-body reservation for their synthesized internal Messages request; the
    // translated Bytes still remain owned by this handler through rotation.
    let bounded_native_messages =
        billable && app.provider.serves_anthropic() && synthesized_messages_origin.is_none();
    let (raw, _body_lease) = if bounded_native_messages {
        let bounded = match read_anthropic_body_bounded(&app, &parts.headers, body).await {
            Ok(body) => body,
            Err(BodyAdmitError::ContentEncoding) => {
                return local_err(LocalErr::UnsupportedContentEncoding, None)
            }
            Err(BodyAdmitError::Storage(bounded_body::StorageError::TooLarge))
            | Err(BodyAdmitError::Storage(bounded_body::StorageError::ArithmeticOverflow)) => {
                return local_err(LocalErr::BodyTooLarge, None)
            }
            Err(BodyAdmitError::Storage(bounded_body::StorageError::Io)) => {
                return local_err(LocalErr::BadRequest, None)
            }
            Err(BodyAdmitError::Storage(_)) => {
                return with_not_started(local_err_for(
                    LocalErr::Overloaded,
                    "body_storage_unavailable",
                    Some(2),
                ))
            }
        };
        (bounded.bytes, Some(bounded._lease))
    } else {
        let raw = match read_body_limited(body, BODY_LIMIT).await {
            Ok(body) => body,
            Err(BodyReadError::TooLarge) => return local_err(LocalErr::BodyTooLarge, None),
            Err(BodyReadError::Read) => return local_err(LocalErr::BadRequest, None),
        };
        (raw, None)
    };

    // тело: один парс — вытаскиваем модель + max_tokens (для тарификации/резерва) и инжектим
    // identity (иначе токен подписки не пустят на /v1/messages). Держим `Bytes` (не Vec): clone на
    // каждую попытку ротации тогда O(1) refcount, а не копия до BODY_LIMIT (анти-амплификация памяти).
    let body_bytes: bytes::Bytes = raw.clone();
    let mut model = String::new();
    let mut max_tokens: u64 = 0;
    let mut requested_fast = false;
    let mut requested_us_inference = false;
    let mut affinity_input = None;
    let mut parsed = serde_json::from_slice::<Value>(&raw).ok();
    // Capture native client intent before namespace removal, max_tokens balance mutation, persona,
    // identity or billing injection. Unknown/unvalidated sub-shapes remain nullable classifier fields.
    let native_messages_fact_candidate = (billable
        && app.provider.serves_anthropic()
        && synthesized_messages_origin.is_none()
        && matches!(authz, Authz::Metered { .. }))
    .then(|| parsed.as_ref())
    .flatten()
    .filter(|original| original.is_object())
    .map(|original| {
        let requested_model = original
            .get("model")
            .and_then(Value::as_str)
            .and_then(|model| bounded_request_fact_ascii(model, MAX_REQUEST_FACT_MODEL_LEN));
        let stream_flag = original
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        (
            requested_model,
            stream_flag,
            classify_anthropic_messages(original),
        )
    });
    let mut count_tokens_fact = if native_count_tokens {
        match (
            app.billing.as_ref(),
            typed_logical_request_id.as_ref(),
            typed_lifecycle_clock.as_ref(),
            parsed.as_ref(),
            &authz,
        ) {
            (
                Some(billing),
                Some(logical_request_id),
                Some(lifecycle_clock),
                Some(original),
                Authz::Metered {
                    account_id, key_id, ..
                },
            ) if original.is_object() => Some(AnthropicCountTokensFactGuard::new(
                Arc::clone(billing),
                AnthropicCountTokensFactSeed {
                    logical_request_id: logical_request_id.as_str().to_owned(),
                    client_attribution: typed_client_attribution
                        .clone()
                        .unwrap_or_else(ClientAttribution::unknown_for_internal_use),
                    execution: execution.clone(),
                    account_id: account_id.clone(),
                    key_id: key_id.clone(),
                    requested_model_candidate: original
                        .get("model")
                        .and_then(Value::as_str)
                        .and_then(|model| {
                            bounded_request_fact_ascii(model, MAX_REQUEST_FACT_MODEL_LEN)
                        }),
                    // This private classifier result has no arbitrary strings/content and remains a
                    // candidate until a successful upstream status proves native shape acceptance.
                    // Internal KIMI/GLM aliases are not native Anthropic count-token shapes, so
                    // even an unexpected accepting response cannot publish structural evidence.
                    classification_candidate: (!original
                        .get("model")
                        .and_then(Value::as_str)
                        .is_some_and(|model| {
                            crate::kimi::KimiGateway::resolve_public_model(model).is_some()
                                || crate::glm::GlmGateway::model_is_glm(model)
                        }))
                    .then(|| classify_anthropic_messages(original)),
                    admitted_at: pool::now(),
                    lifecycle_clock: lifecycle_clock.clone(),
                },
            )),
            _ => None,
        }
    } else {
        None
    };
    macro_rules! return_local {
        ($response:expr) => {{
            let response = $response;
            return match count_tokens_fact.take() {
                Some(fact) => fact.finish_local(response),
                None => response,
            };
        }};
    }
    // KIMI is an internal backend of the Anthropic Messages plane, not a public provider mode.
    // Dispatch only exact reviewed subscription aliases, after shared authorization/body bounds
    // and before Claude-specific identity injection, breaker, pricing policy and pool selection.
    // Every other model follows the byte-stable Claude path below.
    let kimi_model = parsed
        .as_ref()
        .and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .and_then(crate::kimi::KimiGateway::resolve_public_model);
    if !billable && kimi_model.is_some() {
        // A KIMI alias exists only on POST /v1/messages: the subscription route has no
        // count_tokens sibling. Refuse here rather than forward the internal alias to the
        // Claude upstream, whose unknown-model error would name neither the alias nor the cause.
        return_local!(local_err(LocalErr::NotFound, None));
    }
    if billable {
        if let Some(kimi_model) = kimi_model {
            let Some(gateway) = app.kimi.as_ref() else {
                // An exact internal alias must never escape to the Claude upstream when the KIMI
                // plane is disabled or failed before composition.
                return local_err_for(LocalErr::Overloaded, "kimi_gateway_unavailable", Some(2));
            };
            let mut kimi_body = parsed.take().expect("KIMI model came from parsed body");
            // The client may have addressed the model through the router's `kimi/` namespace.
            // Normalize to the bare alias once, here, so nothing downstream — pricing, the
            // gateway's own alias→wire rewrite, the durable turn event — has to know both
            // spellings.
            kimi_body["model"] = Value::String(kimi_model.to_string());
            // Admin-only exact calibration targeting, mirroring the Gemini admission contract:
            // both headers together, validated, never forwarded upstream (send_generation passes
            // through only the two named Anthropic headers). A customer carrying them, a
            // half-present pair or a malformed value are all refused fail-closed.
            let calibration = match crate::kimi::parse_kimi_calibration_headers(
                &parts.headers,
                matches!(authz, Authz::Admin { .. }),
            ) {
                Ok(calibration) => calibration,
                Err(()) => return local_err(LocalErr::BadRequest, None),
            };
            let kimi_affinity = authz
                .affinity_scope()
                .and_then(|scope| app.affinity.infer(scope, &parts.headers, &kimi_body));
            let billing = match &authz {
                Authz::Admin { .. } => None,
                Authz::Metered {
                    account_id,
                    key,
                    available_nano,
                    ..
                } => Some(crate::kimi::KimiBillingInput {
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: authz.mult_for(registry::PROVIDER_KIMI),
                    available_nano: *available_nano,
                }),
                Authz::Unauthorized | Authz::Unavailable => {
                    unreachable!("KIMI dispatch runs only after shared authorization")
                }
            };
            let response = gateway
                .handle(crate::kimi::KimiRequest {
                    headers: parts.headers.clone(),
                    body: kimi_body,
                    raw_body_len: raw.len(),
                    model: kimi_model.to_string(),
                    execution,
                    billing,
                    affinity: kimi_affinity,
                    affinity_store: app.affinity.clone(),
                    calibration,
                })
                .await;
            // One instrumentation point for the whole plane: everything a caller can receive
            // passes through here and already carries its terminal reason. Counting inside the
            // gateway would mean touching every exit and would still miss one.
            Metrics::inc(&app.metrics.kimi_requests);
            if !response.status().is_success() {
                Metrics::inc(&app.metrics.kimi_failures);
                if response
                    .extensions()
                    .get::<TerminalErrorReason>()
                    .is_some_and(|reason| reason.0 == "kimi_capacity_exhausted")
                {
                    Metrics::inc(&app.metrics.kimi_capacity_exhausted);
                }
            }
            return response;
        }
    }
    // GLM is the second internal backend of the Anthropic Messages plane (static API key,
    // dual-ledger calibration). Same dispatch contract as KIMI: exact reviewed aliases only,
    // after shared authorization/body bounds, before Claude-specific identity/pricing/pool —
    // and never a silent fall-through to the Claude upstream when the plane is unavailable.
    let glm_model = parsed
        .as_ref()
        .and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .filter(|model| crate::glm::GlmGateway::model_is_glm(model))
        .map(str::to_string);
    if billable {
        if let Some(glm_model) = glm_model {
            let Some(gateway) = app.glm.as_ref() else {
                // A disabled plane, a corrupted initial roster and a cold roster all produce
                // this fail-closed GLM answer — never a fallback into the Claude pool.
                return local_err_for(LocalErr::Overloaded, "glm_gateway_unavailable", Some(2));
            };
            let glm_body = parsed.take().expect("GLM model came from parsed body");
            let glm_affinity = authz
                .affinity_scope()
                .and_then(|scope| app.affinity.infer(scope, &parts.headers, &glm_body));
            let billing = match &authz {
                Authz::Admin { .. } => None,
                Authz::Metered {
                    account_id,
                    key,
                    available_nano,
                    ..
                } => Some(crate::glm::GlmBillingInput {
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: authz.mult_for(registry::PROVIDER_GLM),
                    available_nano: *available_nano,
                }),
                Authz::Unauthorized | Authz::Unavailable => {
                    unreachable!("GLM dispatch runs only after shared authorization")
                }
            };
            return gateway
                .handle(crate::glm::GlmRequest {
                    body: glm_body,
                    raw_body_len: raw.len(),
                    model: glm_model,
                    execution,
                    billing,
                    affinity: glm_affinity,
                    affinity_store: app.affinity.clone(),
                })
                .await;
        }
    }
    if app.provider == crate::ProviderMode::Kimi {
        // The dedicated KIMI plane serves exact reviewed KIMI aliases only (dispatched above
        // through the same gateway entry). Any other model — Claude included — fails closed with
        // a bounded static 404: this process deliberately operates no Claude pool to fall into.
        return local_err(LocalErr::NotFound, None);
    }
    let fallback_preeligible = billable
        && matches!(authz, Authz::Metered { .. })
        && operator_target.is_none()
        && app.cfg.claudestore_fallback.is_some();
    let mut fallback_parsed = None;
    // `parsed` (если тело — JSON) держим как ФИНАЛИЗИРУЕМЫЙ шаблон: инжектим identity/cap max_tokens
    // здесь, а per-sub metadata.user_id — в цикле per-подписка, и сериализуем тело per-attempt. `body_bytes`
    // (=raw) — фолбэк для не-JSON тела. В общем случае (1 попытка) это 1 сериализация, как и раньше.
    let mut web_uses: u64 = 0; // суммарный лимит web-поисков (для резерва их стоимости под баланс)
    if let Some(v) = parsed.as_mut() {
        model = strip_own_namespace(v);
        max_tokens = v.get("max_tokens").and_then(Value::as_u64).unwrap_or(0);
        requested_fast = v
            .get("speed")
            .and_then(Value::as_str)
            .is_some_and(|speed| speed.eq_ignore_ascii_case("fast"));
        requested_us_inference = v
            .get("inference_geo")
            .and_then(Value::as_str)
            .is_some_and(|geo| geo.eq_ignore_ascii_case("us"));
        // Резервируем стоимость web_search по РЕАЛЬНОМУ max_uses каждого инструмента (не фикс-буфер):
        // иначе клиент с max_uses>buf пробил бы hold. Без max_uses — консервативный дефолт.
        if let Some(tools) = v.get("tools").and_then(Value::as_array) {
            for t in tools {
                let is_web = t
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|s| s.contains("web_search"))
                    .unwrap_or(false);
                if is_web {
                    // saturating + кламп: crafted max_uses≈u64::MAX не должен обернуть web_uses (release-wrap
                    // → мизерный web_buf → недорезерв). Потолок 1000 с запасом покрывает любой реальный кейс.
                    web_uses = web_uses
                        .saturating_add(
                            t.get("max_uses")
                                .and_then(Value::as_u64)
                                .unwrap_or(DEFAULT_WEB_USES),
                        )
                        .min(1000);
                }
            }
        }
        // Infer from the untouched client request. Native harness IDs win; ordinary API clients are
        // linked by canonical transcript prefixes. The account (not individual API key) is the tenant.
        if let Some(scope) = authz.affinity_scope() {
            affinity_input = app.affinity.infer(scope, &parts.headers, v);
        }
        if fallback_preeligible {
            // Clone after namespace normalization but before any local OAuth identity, persona
            // metadata or billing block can be injected into the subscription-bound body.
            fallback_parsed = Some(v.clone());
        }
        if app.cfg.inject_identity {
            inject_identity(v, &app.cfg.identity);
        }
    }
    let mut affinity_resolution = match affinity_input.as_ref() {
        Some(input) => app.affinity.resolve(input).await,
        None => None,
    };
    let affinity_started_new = affinity_input.is_some() && affinity_resolution.is_none();
    let affinity_warm_homes = match (affinity_input.as_ref(), affinity_resolution.as_ref()) {
        (Some(input), None) => app.affinity.warm_homes(input).await,
        _ => Vec::new(),
    };
    let mut persona_session = affinity_resolution
        .as_ref()
        .map(|resolution| pool::stable_hash64(resolution.session_id.as_bytes()));

    // Circuit breaker разомкнут (брауноут апстрима) → быстрый отбой ДО резерва: в аутейдж не делаем
    // лишних DB-записей (reserve+возврат) на каждый запрос thundering-herd. Резерва ещё нет — возвращать нечего.
    if let Some(retry) = app.breaker.open_for(pool::now()) {
        Metrics::inc(&app.metrics.breaker_rejects);
        if fallback_parsed.is_none() {
            return_local!(local_err_for(
                LocalErr::Overloaded,
                "upstream_circuit_breaker",
                Some(retry),
            ));
        }
    }

    // БАЛАНС-ЛИМИТ метерного ключа (точный контроль: клиент не получит ни токена/цента сверх баланса).
    // Идея: output ограничиваем ЗАРАНЕЕ — урезаем `max_tokens` под остаток баланса, и Anthropic сам
    // отрубает генерацию ровно на доступном токене (stop_reason: max_tokens). Вход считаем по ВЕРХНЕЙ
    // оценке (полные байты × cache_write_1h — токенов ≤ байт при любой корзине). Затем атомарно
    // резервируем потолок при УРЕЗАННОМ max_tokens (≤ баланса), фактику закрываем settle в finalize.
    // One stable internal ID spans reservation, all upstream attempts, settlement, and capacity leases.
    // It is generated before any money mutation and is never replaced by an upstream audit header.
    let engine_request_id = crate::upstream::fresh_request_id();
    // Build the immutable fact before reserve. Native intent comes from the original Messages JSON;
    // a universal adapter supplies the accepted original OpenAI intent through its typed carrier.
    // Missing typed logical/lifecycle context, admin, SQLite (whose fact-aware method deliberately
    // ignores analytics), and internal KIMI/GLM leaves preserve the legacy money path.
    let request_fact_admitted_at = pool::now();
    let request_fact_source = native_messages_fact_candidate
        .as_ref()
        .map(|(requested_model, stream_flag, classification)| {
            (
                "native",
                "messages",
                requested_model,
                *stream_flag,
                classification,
            )
        })
        .or_else(|| {
            synthesized_messages_origin.as_ref().map(|origin| {
                (
                    origin.route_class(),
                    origin.request_class(),
                    origin.requested_model(),
                    origin.stream_flag(),
                    origin.classification(),
                )
            })
        });
    let request_fact_admission = match (
        request_fact_source,
        typed_logical_request_id.as_ref(),
        typed_lifecycle_clock.as_ref(),
        &authz,
    ) {
        (
            Some((route_class, request_class, requested_model, stream_flag, classification)),
            Some(logical_request_id),
            Some(_),
            Authz::Metered {
                account_id, key_id, ..
            },
        ) if kimi_model.is_none() && glm_model.is_none() => Some(RequestFactAdmission {
            logical_request_id: logical_request_id.as_str().to_owned(),
            billing_request_id: engine_request_id.clone(),
            execution_group_id: execution.group_id().map(str::to_owned),
            attempt: execution.attempt(),
            account_id: account_id.clone(),
            key_id: key_id.clone(),
            client_kind: typed_client_attribution
                .as_ref()
                .cloned()
                .unwrap_or_else(ClientAttribution::unknown_for_internal_use)
                .kind(),
            client_source: typed_client_attribution
                .as_ref()
                .cloned()
                .unwrap_or_else(ClientAttribution::unknown_for_internal_use)
                .source(),
            client_version: typed_client_attribution
                .as_ref()
                .and_then(|client| client.version().map(str::to_owned)),
            provider_plane: "anthropic".into(),
            route_class: route_class.into(),
            request_class: request_class.into(),
            requested_model: requested_model.clone(),
            // Only a locally resolved/accepted executable id is admissible. Anthropic model
            // resolution happens upstream, after immutable admission, so this remains unknown.
            executable_model: None,
            stream_flag,
            tools_declared_count: classification.tools_declared_count(),
            tool_classes: classification.tool_classes(),
            tool_choice_mode: classification.tool_choice_mode(),
            parallel_tools_requested: classification.parallel_tools_requested(),
            tool_results_in_input: classification.tool_results_in_input(),
            structured_output_flag: classification.structured_output_flag(),
            reasoning_flag: classification.reasoning_flag(),
            service_tier: classification.service_tier().map(str::to_owned),
            input_modalities: classification.input_modalities(),
            output_modalities: classification.output_modalities(),
            admitted_at: request_fact_admitted_at,
        }),
        _ => None,
    };

    // Tuple: request/account/key/hold plus the payable multiplier, the optional strict tariff pin
    // and the hot tariff override version pinned at admission (None = compiled constants).
    let mut reserved: Option<(
        String,
        String,
        String,
        i64,
        i64,
        Option<i64>,
        Option<tariff_book::PinnedTariff>,
    )> = None;
    // Резервируем ТОЛЬКО под POST /v1/messages — единственный биллинговый эндпоинт. `count_tokens` и
    // `GET /v1/models` бесплатны у Anthropic; резерв мог бы ошибочно 402-ить их при нулевом балансе.
    if let (
        true,
        Authz::Metered {
            account_id,
            key,
            available_nano,
            ..
        },
        Some(billing),
    ) = (billable, &authz, &app.billing)
    {
        // Одна скидка на запрос: переопределение аккаунта по `anthropic`, иначе его дефолт.
        let mult_bp = &authz.mult_for(registry::PROVIDER_ANTHROPIC);
        // баланс несём из authorize (свежая выборка) — без повторного чтения. Гонку с параллельными
        // запросами всё равно ловит АТОМАРНЫЙ reserve (WHERE balance>=hold): stale-баланс лишь мог бы
        // дать чуть больший cap, но reserve тогда честно откажет (402), в минус не уводя.
        let bal = *available_nano as i128;
        // РЕЗЕРВ по model_prices_RESERVE: распознанная модель → её цена; нераспознанный алиас →
        // MAX-тариф. Иначе резерв по дешёвому дефолту, а списание по (дорогой) модели ОТВЕТА пробили
        // бы hold → баланс в минус до −2×. Списание (finalize) остаётся по реальной модели ответа.
        let price_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        let matched_tariff =
            metering::anthropic_matched_tariff_at(&model, price_ts, requested_fast);
        let compiled_base = matched_tariff.map(|(_, prices)| prices).unwrap_or_else(|| {
            metering::model_prices_reserve_for_speed_at(&model, price_ts, requested_fast)
        });
        let resolved_tariff = match matched_tariff {
            Some((family, _)) => tariff_book::reserve_base(
                &tariff_book::snapshot(),
                family,
                price_ts,
                compiled_base,
                tariff_book::as_anthropic,
            ),
            None => tariff_book::ReserveBase {
                prices: compiled_base,
                pin: None,
            },
        };
        let mut p = resolved_tariff.prices;
        let tariff_pin = resolved_tariff.pin;
        if requested_us_inference {
            p = metering::premium_prices_ceil(p, 11_000);
        }
        let input_est = ((raw.len() + app.cfg.identity.len()) as i128).max(1);
        // web-буфер = число разрешённых поисков × ставка (0 без web_search-инструмента → малобалансовым
        // не блокирует обычные запросы; при включённом — покрывает ровно заявленный max_uses).
        let web_buf = (web_uses as i128) * metering::WEB_SEARCH_NANO;
        let client_mt = if max_tokens > 0 {
            max_tokens.min(2_000_000)
        } else {
            4096
        };
        // РЕЗЕРВ по АККАУНТУ с ПЕРЕ-РЕЗЕРВОМ под свежий баланс: funded-юзера НЕ роняем 402 из-за гонки
        // конкурентных резервов. `bal` из authorize оптимистичен; если атомарный reserve отказал (соседние
        // holds увели баланс за пол −$1, или per-key лимит), перечитываем АКТУАЛЬНЫЙ баланс, дорезаем
        // output под него+буфер и повторяем. None-путь reserve строки НЕ создаёт → тот же request_id
        // безопасно повторить с меньшим hold (идемпотентность не триггерится). Ограничено 4 попытками:
        // строго убывающий hold сходится быстро; иначе честный 402 (реально за полом даже с буфером).
        let mut reserved_pair: Option<(u64, i64)> = None;
        let settlement_mult_bp = *mult_bp;
        let settlement_priced_ts: Option<i64> = None;
        if *mult_bp > 0 && bal <= 0 {
            return local_err(LocalErr::LowBalance, None);
        }
        let mut cur = cap_to_balance(bal, input_est, web_buf, &p, *mult_bp, client_mt);
        for _ in 0..4 {
            let (eff_mt, hold) = match cur {
                Some(x) => x,
                None => break,
            };
            let reserve_result = match request_fact_admission.as_ref() {
                Some(request_fact) => {
                    billing
                        .reserve_priced_request_for_execution_with_fact(
                            &engine_request_id,
                            account_id,
                            key,
                            hold,
                            execution.clone(),
                            registry::PROVIDER_ANTHROPIC,
                            *mult_bp,
                            request_fact.clone(),
                        )
                        .await
                }
                None => {
                    billing
                        .reserve_priced_request_for_execution(
                            &engine_request_id,
                            account_id,
                            key,
                            hold,
                            execution.clone(),
                            registry::PROVIDER_ANTHROPIC,
                            *mult_bp,
                        )
                        .await
                }
            };
            match reserve_result {
                Ok(Some(_)) => {
                    reserved_pair = Some((eff_mt, hold));
                    break;
                }
                Ok(None) => {}
                Err(error) => {
                    elog::error("forward", format!("billing reservation failed: {error:#}"));
                    return local_err_for(
                        LocalErr::Overloaded,
                        "billing_reservation_unavailable",
                        Some(2),
                    );
                }
            }
            let fresh = match billing.account(account_id).await {
                Ok(Some(account)) => account.balance_nano as i128,
                Ok(None) => 0,
                Err(error) => {
                    elog::error(
                        "forward",
                        format!("billing balance refresh failed: {error:#}"),
                    );
                    return local_err_for(
                        LocalErr::Overloaded,
                        "billing_balance_refresh_unavailable",
                        Some(2),
                    );
                }
            };
            match cap_to_balance(fresh, input_est, web_buf, &p, *mult_bp, client_mt) {
                Some((e, h)) if h < hold => cur = Some((e, h)),
                _ => break,
            }
        }
        let (eff_mt, hold) = match reserved_pair {
            Some(x) => x,
            None => {
                return local_err_for(LocalErr::LowBalance, "billing_reservation_rejected", None)
            }
        };
        // урезали под баланс → правим max_tokens в теле ПОСЛЕ финального eff_mt (мог уменьшиться на
        // ретрае): Anthropic остановит генерацию ровно тут.
        if eff_mt < max_tokens {
            if let Some(v) = parsed.as_mut() {
                v["max_tokens"] = serde_json::json!(eff_mt);
            }
            if let Some(v) = fallback_parsed.as_mut() {
                v["max_tokens"] = serde_json::json!(eff_mt);
            }
        }
        reserved = Some((
            engine_request_id.clone(),
            account_id.clone(),
            key.clone(),
            hold,
            settlement_mult_bp,
            settlement_priced_ts,
            tariff_pin,
        ));
    }
    // Гард резерва: на любом не-успешном исходе И при отмене запроса вернёт hold клиенту. Создаём
    // ДО пересборки тела — если она упадёт и мы вернёмся, Drop гарда вернёт hold (без утечки).
    // Разоружим на успехе — там hold закрывает tee-метеринг. Снимает утечку денег при disconnect.
    let mut hold_guard = reserved.as_ref().map(|(request_id, acct, k, h, _, _, _)| {
        let guard = HoldGuard {
            billing: app.billing.clone(),
            account_id: acct.clone(),
            key: k.clone(),
            hold: *h,
            request_id: request_id.clone(),
            request_fact: None,
            armed: true,
        };
        match (
            request_fact_admission.as_ref(),
            typed_lifecycle_clock.as_ref(),
        ) {
            (Some(admission), Some(lifecycle_clock)) => {
                guard.with_request_fact(AnthropicBillableFactContext {
                    admitted_at: admission.admitted_at,
                    lifecycle_clock: lifecycle_clock.clone(),
                    attempts: AnthropicAttemptTracker::default(),
                    upstream_request_id: None,
                    downstream_disconnect: None,
                })
            }
            _ => guard,
        }
    });
    macro_rules! return_after_reserve_local {
        ($response:expr) => {{
            let response = $response;
            if let Some(guard) = hold_guard.as_mut() {
                guard.settle_terminal(
                    0,
                    None,
                    Some(i32::from(response.status().as_u16())),
                    ProviderTerminalClass::Unknown,
                    DeliveryState::NotStarted,
                    true,
                );
            }
            return match count_tokens_fact.take() {
                Some(fact) => fact.finish_local(response),
                None => response,
            };
        }};
    }
    let version = parts
        .headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&app.cfg.anthropic_version)
        .to_string();
    let beta = match merged_beta(&parts.headers, &app.cfg.default_beta) {
        Ok(v) => v,
        Err(()) => return_after_reserve_local!(local_err(LocalErr::BadBeta, None)),
    };
    let fallback_beta = match merged_beta(&parts.headers, "") {
        Ok(v) => v,
        Err(()) => return_after_reserve_local!(local_err(LocalErr::BadBeta, None)),
    };
    let fallback_body = if reserved.is_some() {
        fallback_parsed
            .and_then(|body| serde_json::to_vec(&body).ok())
            .map(bytes::Bytes::from)
    } else {
        None
    };

    let mut affinity_newly_claimed = false;

    // Гладкий UX: транзиентную нехватку ёмкости (все подписки cooling/за util_cap / breaker /
    // upstream-429) НЕ отдаём клиенту сразу — тихо ждём до бюджета и повторяем весь раунд ротации.
    // Решение принимается ДО начала стрима, поэтому для клиента это лишь чуть больший TTFB, не ошибка.
    let smooth_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(app.cfg.smooth_wait_ms);
    'smooth: loop {
        let mut tried: HashSet<String> = HashSet::new();
        // Some(secs) → раунд уперся в транзиент → ждём+ретрай.
        let mut transient_hint: Option<i64> = None;
        // Один запрос вносит в breaker максимум ОДИН backend-фейл (не `max_tries`): иначе poison-запрос
        // (500 на каждой подписке) в одиночку размыкал бы глобальный breaker и клал сервис всем. Реальный
        // аутейдж тилит breaker числом РАЗНЫХ запросов, а не веером одного.
        let mut backend_fail_recorded = false;
        // Дефолт терминала, если ни одной подписки не выбрали и нет реального upstream-ответа (пул пуст).
        // Клиенту — обезличенный retryable overload, без слова «pool/subscriptions».
        let mut last_local = local_err_for(LocalErr::Overloaded, "pool_unavailable", None);
        // Последний РЕАЛЬНЫЙ ответ Anthropic удерживаем до конца ротации. Если успеха не будет, именно он
        // уходит клиенту со своим body/request-id/retry headers; synthetic допустим только без response.
        let mut last_upstream: Option<(StatusCode, wreq::Response)> = None;

        // Бюджет: ошибки ПОДПИСКИ (429/401/403 — бан/лимит конкретного аккаунта) НЕ тратят попытки,
        // крутимся дальше по флоту (клиенту такая ошибка идти не должна, пока есть здоровые подписки).
        // Бюджет `max_tries` тратят только BACKEND-фейлы (5xx/сеть — вероятный аутейдж апстрима). Верхний
        // предел итераций = «весь флот + запас» (пул сам исключает уже cooling/tried → быстро сходится).
        let hard_cap = if operator_target.is_some() {
            1
        } else {
            app.pool.len().max(1) + 2
        };
        let mut attempt = 0usize;
        let mut backend_tries = 0usize;
        let mut auth_tries = 0usize; // 401/403: возможно вина запроса клиента, а не токена — см. ниже
        while attempt < hard_cap {
            // Уже допущенные запросы тоже обязаны увидеть breaker, открытый конкурентным фейлом, ДО
            // следующей ротации. Если у нас есть реальный upstream response — возвращаем его прозрачно.
            if let Some(retry) = app.breaker.open_for(pool::now()) {
                Metrics::inc(&app.metrics.breaker_rejects);
                // Брейкер разомкнут (брауноут апстрима) — транзиент. Тихо ждём+ретраим до бюджета
                // (breaker сам закроется по record_ok), клиенту 503 отдаём лишь при исчерпании бюджета.
                transient_hint = Some(retry);
                break;
            }
            // First attempt uses shared cache affinity. Redis only proposes an opaque home; the pool
            // revalidates live health/capacity and reserves the local slot atomically. Retries remain
            // load-based and PostgreSQL below is still the authoritative distributed capacity gate.
            let affinity_sub = if attempt == 0 && operator_target.is_none() {
                if let Some(input) = affinity_input.as_ref() {
                    if affinity_resolution.is_none() {
                        if let Some(proposed) = app.pool.peek_affinity_home_with_warm(
                            &affinity_warm_homes,
                            CACHE_ROOT_MIN_WARM_HOMES,
                            CACHE_ROOT_MIN_CAPACITY_RATIO,
                            |email| app.affinity.home_id(email),
                        ) {
                            let proposed_home = app.affinity.home_id(&proposed.email);
                            affinity_resolution =
                                Some(app.affinity.claim(input, &proposed_home).await);
                            affinity_newly_claimed = true;
                            persona_session = affinity_resolution.as_ref().map(|resolution| {
                                pool::stable_hash64(resolution.session_id.as_bytes())
                            });
                        }
                    }

                    let mut selected = None;
                    if let Some(resolution) = affinity_resolution.as_mut() {
                        match app.pool.route_affinity(
                            &resolution.home,
                            affinity_newly_claimed,
                            |email| app.affinity.home_id(email),
                        ) {
                            pool::AffinityRoute::Selected { sub, disposition } => {
                                if disposition == pool::AffinityDisposition::Rebound {
                                    let new_home = app.affinity.home_id(&sub.email);
                                    app.affinity.rebind(resolution, &new_home).await;
                                }
                                app.affinity.remember(input, resolution).await;
                                affinity_newly_claimed = false;
                                selected = Some((sub, disposition));
                            }
                            pool::AffinityRoute::Exhausted => {}
                        }
                    }
                    selected
                } else {
                    None
                }
            } else {
                None
            };
            let (sub, affinity_disposition) = if let Some(target) = operator_target.as_deref() {
                match app
                    .pool
                    .route_operator_target(target, calibration_profile_hint)
                {
                    Some(sub) => (sub, Some(pool::AffinityDisposition::Pinned)),
                    None => {
                        transient_hint = Some(1);
                        break;
                    }
                }
            } else {
                match affinity_sub {
                    Some((sub, disposition)) => (sub, Some(disposition)),
                    None => match app.pool.pick(&tried, false) {
                        Some(sub) => (sub, None),
                        None => break,
                    },
                }
            };
            // route/pick отдали cooling-персону → значит НЕТ ни одной не-cooling (весь оставшийся флот за
            // лимитом). НЕ шлём живой клиентский трафик на отлимиченный аккаунт: это гарантированный свежий
            // 429 (ban-signal + «автоматный» кластер, ровно то, чего избегаем). Быстрый прозрачный отбой
            // клиенту с точным Retry-After = soonest_ready (он откатится сам). hold вернёт hold_guard на return.
            if app.pool.is_cooling(&sub.email) {
                app.pool.mark_done(&sub.email);
                // Не-cooling подписок не осталось (весь флот за лимитом/в кулдауне). НЕ шлём живой трафик
                // на отлимиченный аккаунт. Вместо мгновенного 429 — помечаем транзиент; хвост раунда
                // тихо ждёт до soonest_ready и ретраит (подписка успеет освободиться), 429 — лишь по бюджету.
                Metrics::inc(&app.metrics.exhausted);
                transient_hint = Some(app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs));
                break;
            }
            // Advisory cross-slot hint: a sibling slot's fresh 429 has not reached pool_state yet
            // (persist debounce), so the authority below would still grant a lease on the
            // just-limited subscription. The hint can only cost one rotation to the next
            // candidate; a stale or lost hint is harmless because acquire_capacity performs the
            // authoritative cooldown validation regardless. Errors fail open inside the store.
            if let Ok(Some(until)) = app.affinity.cooling_hint(&sub.email).await {
                if until > pool::now() {
                    Metrics::inc(&app.metrics.cooling_hint_skips);
                    app.pool.mark_done(&sub.email);
                    tried.insert(sub.email.clone());
                    continue;
                }
            }
            attempt += 1;
            tried.insert(sub.email.clone());
            // The in-memory choice is only a candidate. PostgreSQL performs the authoritative atomic
            // cooldown/utilization/inflight validation and increments capacity in the same transaction.
            let capacity_lease_id = format!("{}:{attempt}", engine_request_id);
            let capacity_lease = if let Some(billing) = &app.billing {
                let authority_util_cap =
                    if affinity_disposition == Some(pool::AffinityDisposition::Pinned) {
                        1.0
                    } else {
                        app.cfg.util_cap
                    };
                match billing
                    .acquire_capacity(
                        &capacity_lease_id,
                        &engine_request_id,
                        &sub.email,
                        3600,
                        authority_util_cap,
                    )
                    .await
                {
                    Ok(lease) => lease,
                    Err(error) => {
                        app.pool.mark_done(&sub.email);
                        elog::error("forward", format!("capacity authority failed: {error:#}"));
                        return_after_reserve_local!(local_err_for(
                            LocalErr::Overloaded,
                            "capacity_authority_unavailable",
                            Some(2),
                        ));
                    }
                }
            } else {
                None
            };
            if app.billing.is_some() && capacity_lease.is_none() {
                app.pool.mark_done(&sub.email);
                continue;
            }
            // Гард слота: закроет in-flight на ЛЮБОМ выходе из итерации (continue/return/ОТМЕНА запроса).
            // Разоружим только на успехе (слот перейдёт стриму). mark_cooling/mark_healthy in-flight не трогают.
            let mut guard = InflightGuard::new(
                app.pool.clone(),
                app.billing.clone(),
                capacity_lease.map(|l| l.lease_id),
                sub.email.clone(),
            );

            let client = match app.clients.get(&sub.proxy, &sub.email) {
                Ok(c) => c,
                Err(e) => {
                    app.pool.mark_cooling(&sub.email, 10); // битый прокси → cooling (слот закроет guard)
                    app.affinity
                        .publish_cooling_hint(&sub.email, pool::now() + 10);
                    elog::warn("forward", format!("прокси {}: {e}", sub.email)); // детали ТОЛЬКО в лог (не клиенту)
                    last_local =
                        local_err_for(LocalErr::Overloaded, "subscription_proxy_unavailable", None);
                    continue;
                }
            };

            // per-persona UA: стабильный для подписки, но различный между подписками (антифингерпринт
            // флота). Клиентский user-agent НЕ пробрасываем (см. skip_req_header) — отпечаток наш.
            let ua = crate::upstream::persona_ua(&app.cfg, &sub.email);
            // Клиент, не просивший стрим, ждёт единственный JSON и до его готовности апстрим не
            // шлёт ни байта. Тишина там штатна, поэтому стриминговая граница простоя её убивала бы
            // на длинном ответе; живость на этом пути держит TCP keep-alive. Неразобранное тело
            // трактуем как не-стрим: ошибиться в сторону терпения безопаснее, чем оборвать ответ.
            let client_streams = parsed
                .as_ref()
                .and_then(|body| body.get("stream"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut rb = client
                .request(method.clone(), &url)
                .redirect(wreq::redirect::Policy::none())
                .header("authorization", format!("Bearer {}", sub.token))
                .header("anthropic-version", &version)
                .header("user-agent", &ua)
                // X-Claude-Code-Session-Id — per-диалог UUID (тот же, что в metadata.user_id.session_id).
                // Реальный CC шлёт его на каждый запрос; синтезируем от session-ключа.
                .header(
                    "x-claude-code-session-id",
                    crate::upstream::persona_session_id(&sub.email, persona_session),
                )
                // x-client-request-id — случайный per-request uuid (реальный CC шлёт на каждый запрос).
                .header("x-client-request-id", &engine_request_id);
            // Стрим ловит зависшее соединение по паузе между чтениями: Anthropic шлёт SSE-ping,
            // поэтому живой поток её не встречает. Не-стриминговый запрос молчит до конца
            // генерации штатно, и никакая пауза не отличит там «думает» от «умер» — границы нет,
            // живость держат TCP keep-alive и отмена при уходе клиента.
            let idle = if client_streams {
                app.clients.stream_read_timeout()
            } else {
                app.clients.nonstream_read_timeout()
            };
            if let Some(idle) = idle {
                rb = rb.read_timeout(idle);
            }
            if !beta.is_empty() {
                rb = rb.header("anthropic-beta", &beta);
            }
            rb = crate::upstream::apply_persona_headers(rb, &app.cfg); // x-app + x-stainless-* + accept + browser-access
            for (name, value) in parts.headers.iter() {
                let n = name.as_str();
                if !skip_req_header(n) {
                    rb = rb.header(n, value.as_bytes());
                }
            }
            // per-sub тело: persona metadata добавляем только когда клиент её НЕ задал; malformed metadata
            // не мутируем (Anthropic сам вернёт native 4xx). Сериализация здесь — в общем случае 1 раз;
            // лишняя только на редкой ротации. Ошибка сериализации → отказ (не форвардим stale-тело с
            // большим max_tokens при малом hold — пробой баланса; hold вернёт hold_guard).
            let body_this = match parsed.as_mut() {
                Some(v) => {
                    // metadata допустима только у /v1/messages; count_tokens её не принимает.
                    if billable {
                        set_persona_user_id_if_absent(
                            v,
                            crate::upstream::persona_user_id(&sub.email, persona_session),
                        );
                        // billing-header первым system-блоком (как реальный CC): cc_version флот-константна,
                        // cch стабилен per-подписка. Идемпотентно — на ротации заменяет, не дублирует.
                        if app.cfg.inject_billing {
                            // cc_version = <base>.<build> где build стабилен per-подписка (см. persona_ccbuild).
                            let txt = format!("x-anthropic-billing-header: cc_version={}.{}; cc_entrypoint={}; cch={};",
                            app.cfg.cc_version, crate::upstream::persona_ccbuild(&sub.email),
                            app.cfg.cc_entrypoint, crate::upstream::persona_cch(&sub.email));
                            set_billing_block(v, &txt);
                        }
                    }
                    match serde_json::to_vec(v) {
                        Ok(b) => bytes::Bytes::from(b),
                        Err(_) => return_after_reserve_local!(local_err(LocalErr::Internal, None)),
                    }
                }
                None => body_bytes.clone(), // не-JSON тело → как есть
            };
            rb = rb.body(body_this);

            // Breaker мог открыться, пока мы выбирали persona/строили тело. Не делаем ещё один send после
            // этого момента; предыдущий upstream response (если был) приоритетнее локальной синтетики.
            if let Some(retry) = app.breaker.open_for(pool::now()) {
                Metrics::inc(&app.metrics.breaker_rejects);
                // Брейкер разомкнут (брауноут апстрима) — транзиент. Тихо ждём+ретраим до бюджета
                // (breaker сам закроется по record_ok), клиенту 503 отдаём лишь при исчерпании бюджета.
                transient_hint = Some(retry);
                break;
            }

            if let Some(fact) = count_tokens_fact.as_mut() {
                fact.record_send();
            }
            if let Some(guard) = hold_guard.as_mut() {
                guard.record_send();
            }
            let resp = match rb.send().await {
                Ok(r) => r,
                Err(e) => {
                    // Сетевой сбой остаётся per-subscription сигналом: короткий cooling и ротация.
                    // Не кормим global breaker — один нестабильный прокси не доказывает outage провайдера.
                    app.pool.mark_cooling(&sub.email, 15);
                    app.affinity
                        .publish_cooling_hint(&sub.email, pool::now() + 15);
                    elog::warn("forward", format!("upstream {}: {e}", sub.email)); // детали (email/сеть) ТОЛЬКО в лог
                    last_local =
                        local_err_for(LocalErr::Overloaded, "upstream_connection_error", None);
                    backend_tries += 1;
                    if backend_tries >= app.cfg.max_tries.max(1) {
                        break;
                    }
                    continue;
                }
            };

            let st = resp.status();
            let code = st.as_u16();

            // ПАССИВНЫЙ сбор лимитов из боевого ответа: свежий util/reset без лишних запросов
            // (обновляет polled_ts → активный поллер сам перестаёт трогать «живые» подписки).
            let lim = limits_from_headers(resp.headers());
            let now = pool::now();
            if lim.has_util() {
                app.pool.set_util(
                    &sub.email,
                    lim.util5h,
                    lim.util7d,
                    lim.status.clone(),
                    lim.reset5h,
                    lim.reset7d,
                );
                app.pool.set_quota_snapshots(
                    &sub.email,
                    lim.quota5h.map(|quota| pool::QuotaSnapshot {
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                        resets_at: lim.reset5h,
                    }),
                    lim.quota7d.map(|quota| pool::QuotaSnapshot {
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                        resets_at: lim.reset7d,
                    }),
                );
            }

            // Классификация вины (важно: НЕ студить подписку за чужую вину):
            if code == 429 {
                // квота подписки: студим до сброса окна-виновника (см. cool_secs_429)
                Metrics::inc(&app.metrics.upstream_429);
                let secs = cool_secs_429(&resp, &lim, now);
                app.pool.mark_cooling(&sub.email, secs);
                app.affinity.publish_cooling_hint(&sub.email, now + secs);
                elog::warn(
                    "forward",
                    format!("ротация: {} вернул 429 — cooling {}s", sub.email, secs),
                );
                last_upstream = Some((st, resp));
                continue;
            }
            if code == 401 || code == 403 {
                // 401/403 может быть виной ЗАПРОСА клиента (недоступная модель/бета/путь), а НЕ мёртвого
                // токена. НЕ студим подписку здесь: иначе один crafted-запрос выстудил бы весь пул на
                // AUTH_QUARANTINE (DoS). Дохлый токен ловит ПОЛЛЕР чистым probe (там карантин). Ротируем
                // ограниченно, чтобы исключить единичный дохлый токен; если повторилось на другой подписке
                // — детерминировано → это запрос клиента → отдаём НАСТОЯЩИЙ ответ апстрима прозрачно.
                Metrics::inc(&app.metrics.upstream_auth);
                auth_tries += 1;
                elog::warn(
                    "forward",
                    format!(
                        "auth {} на {} (попытка {}) — НЕ студим (возможно вина запроса)",
                        code, sub.email, auth_tries
                    ),
                );
                // Пробуем ДРУГУЮ подписку ТОЛЬКО если она реально есть (вдруг дохлый токен этой). Если
                // другой нет (напр. пул из одной) или повтор — это вина запроса (scope/модель/путь) → отдаём
                // РЕАЛЬНЫЙ 401/403 Anthropic прозрачно, а НЕ маскируем в 429-исчерпание (был баг с 1 подпиской).
                if auth_tries < 2 && attempt < hard_cap {
                    // Уходим на ДРУГУЮ подписку → эта, возможно, с мёртвым токеном. Просим поллер проверить
                    // её чистым probe СРАЗУ (не через LIVENESS_INTERVAL): revoked-токен перестанет быть
                    // placement-магнитом за ~1 цикл. Без cooling здесь (crafted-запрос иначе студил бы флот).
                    app.pool.request_probe(&sub.email);
                    if let Some(p) = &app.probe_poke {
                        p.notify_one();
                    }
                    last_upstream = Some((st, resp));
                    continue;
                }
                // Повтор/нет альтернативы → отдаём РЕАЛЬНЫЙ 401/403 клиенту (может быть вина запроса), но
                // НЕ штампуем токен «здоровым» (старый баг: маскировал реально мёртвый токен, когда он
                // последний/единственный). Вердикт о живости выносит ТОЛЬКО поллер по чистым probe — просим
                // его проверить эту подписку (dead-детект durable в pool::record_probe).
                app.pool.request_probe(&sub.email);
                if let Some(p) = &app.probe_poke {
                    p.notify_one();
                }
                // Запрос-детерминированный 401/403 возвращаем БАЙТ-В-БАЙТ: body/request-id/error type
                // принадлежат Anthropic и нужны SDK-диагностике; секретные auth headers уже фильтруются.
                let upstream_request_id = bounded_anthropic_request_id(&resp);
                if let Some(hold_guard) = hold_guard.as_mut() {
                    hold_guard.set_terminal_upstream(upstream_request_id.clone());
                    hold_guard.settle_terminal(
                        0,
                        upstream_request_id.as_deref(),
                        Some(i32::from(st.as_u16())),
                        AnthropicCountTokensTerminalEvidence::upstream(st).provider_terminal_class,
                        DeliveryState::Started,
                        true,
                    );
                }
                let response = stream_back(st, resp, None, app.metrics.clone());
                return match count_tokens_fact.take() {
                    Some(fact) => fact.finish_upstream(st, upstream_request_id, response),
                    None => response,
                };
            }
            if st.is_server_error() || code == 408 || code == 409 || code == 425 {
                // вина АПСТРИМА, не подписки: НЕ студим подписку (слот закроет guard), кормим breaker
                // максимум раз на запрос (анти-DoS от poison-запроса).
                Metrics::inc(&app.metrics.upstream_5xx);
                if !backend_fail_recorded {
                    app.breaker.record_fail(now, &sub.email);
                    backend_fail_recorded = true;
                }
                elog::warn(
                    "forward",
                    format!(
                        "ротация: {} вернул {} — backend-fault (breaker+)",
                        sub.email, code
                    ),
                );
                last_upstream = Some((st, resp));
                backend_tries += 1; // backend тратит бюджет (аутейдж)
                if backend_tries >= app.cfg.max_tries.max(1) {
                    break;
                }
                continue;
            }
            // 2xx/клиентская 4xx → апстрим здоров: сбрасываем окно фейлов брейкера
            app.breaker.record_ok(now);

            // успех или клиентская ошибка запроса (одинакова на любой подписке) → отдаём как есть.
            // На УСПЕХЕ всегда меряем ответ: расход подписки → калибровка пула; для метерного ключа
            // finalize закрывает резерв фактической стоимостью; и там же `end_stream` снимает слот
            // конкуррентности. 4xx не меряем — резерв возвращаем, слот освобождаем сразу.
            let terminal_upstream_request_id = bounded_anthropic_request_id(&resp);
            let meter = if st.is_success() {
                app.pool.mark_healthy(&sub.email);
                // count_tokens is successful but does not populate Anthropic's prompt cache.
                if let (true, Some(input)) = (billable, affinity_input.as_ref()) {
                    let home = app.affinity.home_id(&sub.email);
                    if affinity_started_new {
                        app.affinity.record_cache_root_placement(
                            input,
                            affinity_warm_homes.iter().any(|warm| warm == &home),
                        );
                    }
                    app.affinity.mark_cache_warm(input, &home);
                }
                if let (Some((request_id, account_id, key, hold, _, priced_ts, _)), Some(billing)) =
                    (reserved.as_ref(), app.billing.as_ref())
                {
                    let delivery = if request_fact_admission.is_some() {
                        billing
                            .mark_delivering_with_request_fact(request_id, 3600)
                            .await
                    } else {
                        billing.mark_delivering(request_id, 3600).await
                    };
                    if !matches!(delivery, Ok(true)) {
                        // The provider accepted the request, but the durable delivery marker was
                        // fenced. Nothing was measured, so the customer is not billed the admission
                        // ceiling for it; the request still fails closed rather than streaming
                        // untracked usage.
                        let actual = if priced_ts.is_some() {
                            0
                        } else {
                            crate::settlement_policy::unknown_usage_charge(*hold)
                        };
                        if let Some(g) = hold_guard.as_mut() {
                            if g.request_fact.is_some() {
                                g.settle_terminal(
                                    actual,
                                    Some("delivery-marker-failed"),
                                    Some(i32::from(StatusCode::SERVICE_UNAVAILABLE.as_u16())),
                                    ProviderTerminalClass::Unknown,
                                    DeliveryState::Unknown,
                                    true,
                                );
                            } else {
                                billing.settle_detached(
                                    request_id,
                                    account_id,
                                    key,
                                    *hold,
                                    actual,
                                    Some("delivery-marker-failed"),
                                    None,
                                );
                                g.disarm();
                            }
                        }
                        // Заголовок not_started снимаем: в legacy-scalar ветке выше settle
                        // закрыл hold ПОЛНЫМ списанием (actual=hold), значит условие «reserve не
                        // будет тарифицирован» не выполнено — отсутствие заголовка безопаснее.
                        return without_not_started(local_err_for(
                            LocalErr::Overloaded,
                            "billing_delivery_marker_unavailable",
                            Some(2),
                        ));
                    }
                }
                let capacity = match (&guard.billing, &guard.capacity_lease_id) {
                    (Some(billing), Some(lease_id)) => Some((billing.clone(), lease_id.clone())),
                    _ => None,
                };
                guard.disarm(); // слот переходит стриму (end_stream)
                if let Some(g) = hold_guard.as_mut() {
                    g.disarm();
                } // hold закроет tee-метеринг фактикой
                let bill = match (&authz, reserved.take()) {
                    (
                        Authz::Metered { .. },
                        Some((
                            request_id,
                            acct,
                            key,
                            hold,
                            payable_multiplier_bp,
                            priced_ts,
                            tariff_pin,
                        )),
                    ) => app.billing.clone().map(|billing| BillCtx {
                        billing,
                        account_id: acct,
                        key,
                        mult_bp: payable_multiplier_bp,
                        hold,
                        tariff_priced_ts: priced_ts,
                        tariff_pin,
                        policy_fast: priced_ts.map(|_| requested_fast),
                        policy_us_inference: priced_ts.map(|_| requested_us_inference),
                        request_id,
                        http_status_code: i32::from(st.as_u16()),
                        reference: request_id_of(&resp),
                        request_fact: hold_guard
                            .as_mut()
                            .and_then(HoldGuard::take_request_fact)
                            .map(|mut request_fact| {
                                request_fact.upstream_request_id =
                                    terminal_upstream_request_id.clone();
                                request_fact
                            }),
                    }),
                    _ => None,
                };
                let mut quota_snapshots = Vec::with_capacity(2);
                if let (Some(quota), Some(resets_at)) = (lim.quota5h, lim.reset5h) {
                    quota_snapshots.push(crate::billing::AnthropicQuotaSnapshot {
                        window_kind: "5h".to_owned(),
                        window_duration_mins: 300,
                        resets_at,
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                    });
                }
                if let (Some(quota), Some(resets_at)) = (lim.quota7d, lim.reset7d) {
                    quota_snapshots.push(crate::billing::AnthropicQuotaSnapshot {
                        window_kind: "7d".to_owned(),
                        window_duration_mins: 10_080,
                        resets_at,
                        used_fraction_units: quota.used_fraction_units,
                        measurement_resolution_fraction_units: quota
                            .measurement_resolution_fraction_units,
                        observed_at: now,
                    });
                }
                let calibration = app.billing.clone().map(|billing| CalibrationCtx {
                    billing,
                    request_id: engine_request_id.clone(),
                    plan: if sub.plan.trim().is_empty() {
                        "unknown".to_owned()
                    } else {
                        sub.plan.clone()
                    },
                    quota_snapshots,
                    probe_poke: app.probe_poke.clone(),
                });
                Some(MeterCtx {
                    model: model.clone(),
                    is_sse: is_event_stream(&resp),
                    bill,
                    subscription: Some(SubscriptionMeterCtx {
                        pool: app.pool.clone(),
                        email: sub.email.clone(),
                        calibration,
                        capacity,
                    }),
                })
            } else {
                // клиентская 4xx: подписка ни при чём. Слот закроет guard, резерв — hold_guard (на return).
                app.pool.mark_healthy(&sub.email);
                if let Some(hold_guard) = hold_guard.as_mut() {
                    hold_guard.set_terminal_upstream(terminal_upstream_request_id.clone());
                    hold_guard.settle_terminal(
                        0,
                        terminal_upstream_request_id.as_deref(),
                        Some(i32::from(st.as_u16())),
                        AnthropicCountTokensTerminalEvidence::upstream(st).provider_terminal_class,
                        DeliveryState::Started,
                        true,
                    );
                }
                None
            };
            let upstream_request_id = terminal_upstream_request_id;
            let response = stream_back(st, resp, meter, app.metrics.clone());
            return match count_tokens_fact.take() {
                Some(fact) => fact.finish_upstream(st, upstream_request_id, response),
                None => response,
            };
        }
        // Итог раунда. Транзиентную нехватку (нет реального upstream-ответа И backend-бюджет цел) тихо
        // ждём и ретраим до smooth_deadline; всё остальное — отдаём клиенту. Резерв держит hold_guard,
        // слоты закрыты guard'ами последней попытки — на continue 'smooth утечки нет (RAII).
        let backend_exhausted = backend_tries >= app.cfg.max_tries.max(1);
        // Транзиент = не хард-аутейдж апстрима, и был реальный шанс: явная подсказка (все cooling/breaker),
        // синтетическое исчерпание (нет last_upstream, но подписки пробовались), либо upstream отдал 429.
        let is_transient = !backend_exhausted
            && (transient_hint.is_some()
                || (last_upstream.is_none() && !tried.is_empty())
                || last_upstream
                    .as_ref()
                    .map(|(st, _)| st.as_u16() == 429)
                    .unwrap_or(false));
        if is_transient {
            let remaining = smooth_deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis();
            let hint = transient_hint
                .or_else(|| app.pool.soonest_ready())
                .unwrap_or(app.cfg.cool_secs);
            if let Some(step) = smooth_step(hint, remaining) {
                tokio::time::sleep(step).await;
                continue 'smooth;
            }
        }
        // Only a metered request with an existing durable reservation can reach this branch. The
        // local rotation and smooth-wait budget are terminal, no public byte has been emitted, and
        // this is the sole external attempt. A failed external call leaves the hold guard armed and
        // falls through to the original local terminal response/refund path.
        let mut fallback_terminal = None;
        if let (Some(config), Some(body)) =
            (app.cfg.claudestore_fallback.as_ref(), fallback_body.clone())
        {
            let fallback_attempt = attempt_claudestore_fallback(
                &app,
                config,
                body,
                &version,
                &fallback_beta,
                hold_guard
                    .as_mut()
                    .expect("fallback requires a durable metered reservation"),
            )
            .await;
            if let ClaudeStoreAttempt::Response(resp) = fallback_attempt {
                let fallback_upstream_request_id = bounded_anthropic_request_id(&resp);
                if let Some(guard) = hold_guard.as_mut() {
                    guard.set_terminal_upstream(fallback_upstream_request_id.clone());
                }
                let delivery_marked = match (reserved.as_ref(), app.billing.as_ref()) {
                    (Some((request_id, ..)), Some(billing)) => {
                        let delivery = if request_fact_admission.is_some() {
                            billing
                                .mark_delivering_with_request_fact(request_id, 3600)
                                .await
                        } else {
                            billing.mark_delivering(request_id, 3600).await
                        };
                        matches!(delivery, Ok(true))
                    }
                    _ => false,
                };
                if !delivery_marked {
                    Metrics::inc(&app.metrics.claudestore_fallback_failures);
                    if let Some(guard) = hold_guard.as_mut() {
                        guard.settle_terminal(
                            crate::settlement_policy::unknown_usage_charge(guard.hold),
                            Some("delivery-marker-failed"),
                            Some(i32::from(StatusCode::SERVICE_UNAVAILABLE.as_u16())),
                            ProviderTerminalClass::Unknown,
                            DeliveryState::Unknown,
                            true,
                        );
                    }
                    return without_not_started(local_err_for(
                        LocalErr::Overloaded,
                        "billing_delivery_marker_unavailable",
                        Some(2),
                    ));
                }
                if let Some(guard) = hold_guard.as_mut() {
                    guard.disarm();
                }
                let bill = match reserved.take() {
                    Some((
                        request_id,
                        account_id,
                        key,
                        hold,
                        payable_multiplier_bp,
                        priced_ts,
                        tariff_pin,
                    )) => app.billing.clone().map(|billing| BillCtx {
                        billing,
                        account_id,
                        key,
                        mult_bp: payable_multiplier_bp,
                        hold,
                        tariff_priced_ts: priced_ts,
                        tariff_pin,
                        policy_fast: priced_ts.map(|_| requested_fast),
                        policy_us_inference: priced_ts.map(|_| requested_us_inference),
                        request_id,
                        http_status_code: i32::from(resp.status().as_u16()),
                        reference: request_id_of(&resp),
                        request_fact: hold_guard
                            .as_mut()
                            .and_then(HoldGuard::take_request_fact)
                            .map(|mut request_fact| {
                                request_fact.upstream_request_id =
                                    fallback_upstream_request_id.clone();
                                request_fact
                            }),
                    }),
                    None => None,
                };
                Metrics::inc(&app.metrics.claudestore_fallback_successes);
                let status = resp.status();
                let meter = MeterCtx {
                    model: model.clone(),
                    is_sse: is_event_stream(&resp),
                    bill,
                    subscription: None,
                };
                return stream_back(status, resp, Some(meter), app.metrics.clone());
            }
            fallback_terminal = match fallback_attempt {
                ClaudeStoreAttempt::BeforeSend => None,
                ClaudeStoreAttempt::Transport => {
                    Some((None, None, ProviderTerminalClass::Transport))
                }
                ClaudeStoreAttempt::Http(status, request_id) => Some((
                    Some(status),
                    request_id,
                    AnthropicCountTokensTerminalEvidence::upstream(status).provider_terminal_class,
                )),
                ClaudeStoreAttempt::Response(_) => unreachable!("returned above"),
            };
        }
        // Терминал: реальный Anthropic-ответ приоритетнее локальной классификации; иначе backend-аутейдж
        // (или пул пуст) → last_local; иначе синтетический 429 с readiness всего пула.
        let (terminal, upstream_terminal) = if let Some((st, resp)) = last_upstream {
            let upstream_request_id = bounded_anthropic_request_id(&resp);
            (
                stream_back(st, resp, None, app.metrics.clone()),
                Some((st, upstream_request_id)),
            )
        } else if backend_exhausted || tried.is_empty() {
            (last_local, None)
        } else {
            Metrics::inc(&app.metrics.exhausted);
            let retry = app.pool.soonest_ready().unwrap_or(app.cfg.cool_secs);
            (
                local_err_for(
                    LocalErr::RateLimited,
                    "subscription_pool_exhausted",
                    Some(retry),
                ),
                None,
            )
        };
        // Once an external transport attempt begins, classify its failure conservatively: the
        // provider might have accepted it before the response was lost. Preserve the local terminal
        // status and refund, but never sign the stronger proof that execution definitely did not start.
        let terminal = if fallback_terminal.is_some() {
            without_not_started(terminal)
        } else {
            terminal
        };
        if let Some(hold_guard) = hold_guard.as_mut() {
            if let Some((status, upstream_request_id, terminal_class)) = fallback_terminal.as_ref()
            {
                hold_guard.set_terminal_upstream(upstream_request_id.clone());
                hold_guard.settle_terminal(
                    0,
                    upstream_request_id.as_deref(),
                    status.map(|status| i32::from(status.as_u16())),
                    *terminal_class,
                    DeliveryState::Unknown,
                    true,
                );
            } else {
                match upstream_terminal.as_ref() {
                    Some((status, upstream_request_id)) => {
                        hold_guard.set_terminal_upstream(upstream_request_id.clone());
                        hold_guard.settle_terminal(
                            0,
                            upstream_request_id.as_deref(),
                            Some(i32::from(status.as_u16())),
                            AnthropicCountTokensTerminalEvidence::upstream(*status)
                                .provider_terminal_class,
                            DeliveryState::Started,
                            true,
                        );
                    }
                    None => hold_guard.settle_terminal(
                        0,
                        None,
                        Some(i32::from(terminal.status().as_u16())),
                        ProviderTerminalClass::Unknown,
                        DeliveryState::NotStarted,
                        true,
                    ),
                }
            }
        }
        return match (count_tokens_fact.take(), upstream_terminal) {
            (Some(fact), Some((status, upstream_request_id))) => {
                fact.finish_upstream(status, upstream_request_id, terminal)
            }
            (Some(fact), None) => fact.finish_local(terminal),
            (None, _) => terminal,
        };
    } // 'smooth: loop
}

/// Ответ — SSE-стрим? (по content-type). Определяет способ парсинга usage.
fn is_event_stream(resp: &wreq::Response) -> bool {
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains("text/event-stream"))
        .unwrap_or(false)
}

/// request-id ответа Anthropic — кладём в ledger как `ref` списания (аудит-трейл «за что списано»).
fn request_id_of(resp: &wreq::Response) -> Option<String> {
    resp.headers()
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Явный заголовок `Retry-After` (самый авторитетный хинт — Anthropic сам говорит, когда можно).
fn retry_after_header(resp: &wreq::Response) -> Option<i64> {
    let v = resp.headers().get("retry-after")?.to_str().ok()?;
    v.trim().parse::<i64>().ok().map(|s| s.max(1))
}

/// Короткий cooldown при небурстовом/транзиентном 429 (лимит запросов-в-минуту чистится быстро).
const BURST_COOL_SECS: i64 = 20;

/// Секунды до сброса ОКНА-виновника — ТОЛЬКО если это реально квотный 429 (окно у потолка):
/// недельное почти выбрано (util7d≥0.95) → до reset7d (могут быть дни, и это правильно), либо
/// 5h у потолка → до reset5h. Если НИ ОДНО окно не близко к потолку — это НЕ квотный 429
/// (burst/иной лимит), студить до reset нельзя (можно на часы зря) → None → короткий дефолт.
fn window_cool(lim: &Limits, now: i64) -> Option<i64> {
    let fut = |t: Option<i64>| t.filter(|x| *x > now).map(|x| (x - now).max(1));
    let (u5, u7) = (lim.util5h.unwrap_or(0.0), lim.util7d.unwrap_or(0.0));
    // АВТОРИТЕТНО: Anthropic сам указал связывающее окно (`representative-claim`). Студим до его reset,
    // но только если это окно реально у потолка (≥0.9) — иначе 429 = burst по rate (запросов/мин), а
    // не исчерпание квоты, и на часы студить нельзя (упадём в burst-дефолт). "seven_day"/"five_hour".
    if let Some(c) = lim.claim.as_deref() {
        if c.contains("day") && u7 >= 0.9 {
            return fut(lim.reset7d).or_else(|| fut(lim.reset5h));
        }
        if c.contains("hour") && u5 >= 0.9 {
            return fut(lim.reset5h).or_else(|| fut(lim.reset7d));
        }
    }
    // Фолбэк-эвристика, если заголовка claim нет: окно у потолка → студим до его reset.
    if u7 >= 0.95 {
        return fut(lim.reset7d).or_else(|| fut(lim.reset5h));
    }
    if u5 >= 0.95 {
        return fut(lim.reset5h).or_else(|| fut(lim.reset7d));
    }
    None
}

/// Сколько студить при 429: Retry-After (авторитетно) → окно-виновник (если квота выбита) →
/// короткий burst-дефолт (транзиентный лимит запросов/мин).
/// Верхний потолок cooling: недельное окно 7d + сутки запаса. Кламп защищает от враждебного/битого
/// upstream-ответа (или скомпрометированного прокси), который прислал бы Retry-After/reset в далёкое
/// будущее и запарковал бы здоровую подписку на месяцы. Дольше 8 суток остывать нечему (все окна ≤7d).
const MAX_COOL_SECS: i64 = 8 * 24 * 3600;

fn cool_secs_429(resp: &wreq::Response, lim: &Limits, now: i64) -> i64 {
    retry_after_header(resp)
        .or_else(|| window_cool(lim, now))
        .unwrap_or(BURST_COOL_SECS)
        .clamp(0, MAX_COOL_SECS)
}

/// Гладкий UX: сколько ТИХО ждать перед следующим раундом ротации. `hint_secs` — подсказка готовности
/// (soonest_ready / Retry-After), `remaining_ms` — остаток бюджета. `None` → бюджет исчерпан (пора
/// отдать клиенту ошибку). Шаг зажат в [250мс, 2с] и не больше остатка: так пул перечитывается часто
/// (подписка могла освободиться раньше hint), а один длинный сон не «проглатывает» весь бюджет.
pub(crate) fn smooth_step(hint_secs: i64, remaining_ms: u128) -> Option<std::time::Duration> {
    if remaining_ms == 0 {
        return None;
    }
    let hint_ms = (hint_secs.max(0) as u128).saturating_mul(1000);
    let step = hint_ms.clamp(250, 2000).min(remaining_ms);
    Some(std::time::Duration::from_millis(step as u64))
}

/// Отдать ответ апстрима клиенту байт-в-байт (стримом — работает и для SSE).
/// Если задан `meter` — оборачиваем тело в tee-метеринг: клиент получает те же байты,
/// а на завершении стрима списываем стоимость с ключа (тело клиенту НЕ задерживается).
/// Terminate a broken SSE body with the error event the protocol defines for it.
///
/// A transport failure after the first byte cannot be retried — another attempt would replay or
/// interleave output the client has already seen. What it must not do is end the body silently:
/// an SDK reading `text/event-stream` sees a stream that simply stops, which is indistinguishable
/// from a completed response until a parse fails somewhere further up, and many clients hang on it.
///
/// `event: error` is part of the Anthropic stream protocol, so emitting one is not a deviation from
/// byte-for-byte transparency — it restores it. A real upstream that failed mid-stream would send
/// exactly this frame, and the client's existing error path handles it. The payload carries the
/// same anonymised overload wording the local error path uses: the cause belongs in our metrics and
/// logs, never in a customer's response body.
struct SseErrorTail {
    inner: ResponseByteStream,
    /// Set once the inner stream has failed, so the tail frame is emitted exactly once.
    failed: bool,
    done: bool,
    /// Kept so the cut can be counted where it happens: the stream outlives the handler that
    /// built it, and by then no request context is left to attribute it to.
    metrics: Arc<Metrics>,
}

impl SseErrorTail {
    fn new(inner: ResponseByteStream, metrics: Arc<Metrics>) -> Self {
        Self {
            inner,
            failed: false,
            done: false,
            metrics,
        }
    }

    fn frame() -> bytes::Bytes {
        bytes::Bytes::from_static(
            b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        )
    }
}

impl Stream for SseErrorTail {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if self.done {
            return Poll::Ready(None);
        }
        if self.failed {
            self.done = true;
            return Poll::Ready(Some(Ok(Self::frame())));
        }
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Err(e))) => {
                // Swallow the transport error and end the body with a protocol frame instead.
                // Propagating it would abort the response, which is precisely the silent truncation
                // this exists to remove.
                //
                // The log alone cannot answer "how often does this happen": a customer reporting
                // dropped connections needs a rate, and the response already carried `200`, so the
                // terminal-error audit never sees it. Count it, and split the two causes the
                // customer cannot tell apart — an upstream that went quiet past the read timeout,
                // and a tunnel that died mid-answer — because the remedies are different.
                let timed_out = matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                );
                if timed_out {
                    Metrics::inc(&self.metrics.stream_cut_timeout);
                } else {
                    Metrics::inc(&self.metrics.stream_cut_transport);
                }
                elog::error("forward", format!("mid-stream transport failure: {e}"));
                self.failed = true;
                self.done = true;
                Poll::Ready(Some(Ok(Self::frame())))
            }
            other => other,
        }
    }
}

fn stream_back(
    st: StatusCode,
    resp: wreq::Response,
    meter: Option<MeterCtx>,
    metrics: Arc<Metrics>,
) -> Response {
    // Не-2xx passthrough без метеринга: hold по request_id вернёт armed HoldGuard (refund),
    // а ни одного байта клиенту ещё не отправлено — контракт not_started выполнен. На 2xx
    // (meter = Some) заголовок недопустим никогда, включая SseErrorTail внутри 200.
    let not_started = meter.is_none() && !st.is_success();
    let mut builder = Response::builder().status(st);
    for (name, value) in resp.headers().iter() {
        if !skip_resp_header(name.as_str()) {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
    }
    if not_started {
        builder = builder.header(EXECUTION_STATE_HEADER, EXECUTION_STATE_NOT_STARTED);
    }
    let event_stream = is_event_stream(&resp);
    let stream = resp
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let stream: ResponseByteStream = Box::pin(stream);
    let mut stream: ResponseByteStream = match meter {
        Some(ctx) => Box::pin(TeeMeter::new(stream, ctx)),
        None => stream,
    };
    // Outermost, so metering never observes the synthetic frame: usage belongs to the upstream that
    // produced it, and a failure tail is not usage.
    if event_stream {
        stream = Box::pin(SseErrorTail::new(stream, metrics));
    }
    let body = Body::from_stream(stream);
    match builder.body(body) {
        Ok(response) => response,
        Err(_) => {
            // Сборка ответа не удалась. На 2xx-пути дропнутый TeeMeter при drain может списать
            // фактику → not_started недопустим; на не-2xx armed HoldGuard вернёт hold — заголовок
            // из local_err корректен и сохраняется.
            let response = local_err(LocalErr::Internal, None);
            if not_started {
                response
            } else {
                without_not_started(response)
            }
        }
    }
}

#[cfg(test)]
mod sse_tail_tests;

#[cfg(test)]
mod tests;
