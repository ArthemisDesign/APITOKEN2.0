//! Тарификация ответа: tee-стрим поверх тела апстрима.
//!
//! Инвариант прозрачности не нарушается: клиент получает байты апстрима БАЙТ-В-БАЙТ и без
//! задержки — мы лишь параллельно копим копию тела. Когда стрим завершился (или оборвался),
//! парсим `usage` (SSE — из накопленного текста, не-стрим — из полного JSON), считаем стоимость
//! через `metering` и списываем с баланса ключа. Метерим ТОЛЬКО успешный ответ (см. proxy.rs).

use crate::billing::AnthropicQuotaSnapshot;
use crate::billing::AsyncBilling;
use crate::execution::RequestLifecycleClock;
use bytes::Bytes;
use futures_util::Stream;
use pool::Pool;
use registry::request_facts::{DeliveryState, ProviderTerminalClass, RequestFactTerminalEvidence};
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Checked count of actual provider HTTP submissions for one Anthropic billing request. The value
/// becomes unknown on arithmetic overflow or when cancellation/panic prevents an exhaustive terminal
/// observation; it contains no request, credential, profile, or response data.
pub(crate) struct AnthropicAttemptTracker {
    count: Option<usize>,
}

impl Default for AnthropicAttemptTracker {
    fn default() -> Self {
        Self { count: Some(0) }
    }
}

impl AnthropicAttemptTracker {
    pub(crate) fn record_send(&mut self) {
        self.count = self.count.and_then(|count| count.checked_add(1));
    }

    pub(crate) fn exhaustive_i32(&self) -> Option<i32> {
        self.count.and_then(|count| i32::try_from(count).ok())
    }
}

impl fmt::Debug for AnthropicAttemptTracker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AnthropicAttemptTracker(<redacted>)")
    }
}

/// Privacy-bounded lifecycle state admitted atomically with an Anthropic reservation. It carries
/// only typed clock/counter evidence and bounded terminal metadata; raw JSON, headers, key secrets,
/// upstream errors, and subscription identities are structurally absent.
pub(crate) struct AnthropicBillableFactContext {
    pub(crate) admitted_at: i64,
    pub(crate) lifecycle_clock: RequestLifecycleClock,
    pub(crate) attempts: AnthropicAttemptTracker,
    pub(crate) upstream_request_id: Option<String>,
    pub(crate) downstream_disconnect: Option<bool>,
}

impl AnthropicBillableFactContext {
    pub(crate) fn terminal_evidence(
        &self,
        http_status_code: Option<i32>,
        provider_terminal_class: ProviderTerminalClass,
        delivery_state: DeliveryState,
        tool_calls_in_output: Option<bool>,
        attempts_exhaustive: bool,
    ) -> RequestFactTerminalEvidence {
        let terminal_at = pool::now().max(self.admitted_at);
        RequestFactTerminalEvidence {
            terminal_at,
            http_status_code,
            provider_terminal_class,
            delivery_state,
            downstream_disconnect: self.downstream_disconnect,
            upstream_request_id: self.upstream_request_id.clone(),
            first_public_byte_at: self
                .lifecycle_clock
                .seal_first_public_byte_for_terminal(self.admitted_at, terminal_at),
            internal_attempt_count: attempts_exhaustive
                .then(|| self.attempts.exhaustive_i32())
                .flatten(),
            failure_class: None,
            tool_calls_in_output,
        }
    }
}

impl fmt::Debug for AnthropicBillableFactContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AnthropicBillableFactContext(<redacted>)")
    }
}

/// Какой pricing-authority выдал admission-hold — выбирает контракт округления при settlement.
/// Опциональное списание с АККАУНТА клиента (только для метерных ключей). Баланс общий на аккаунт;
/// `key` — для атрибуции расхода по ключу; `request_id` — в ledger как ссылка на запрос.
pub struct BillCtx {
    pub billing: Arc<AsyncBilling>,
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub hold: i64, // зарезервированный при допуске потолок — закрываем его фактической стоимостью
    /// Strict policy settlement reuses the exact tariff timestamp pinned at admission. Legacy
    /// scalar requests keep `None` and preserve their existing completion-time tariff lookup.
    pub tariff_priced_ts: Option<i64>,
    /// The hot tariff override version (`<family>/v<N>`, N >= 2) admission priced the hold with.
    /// Settlement replays exactly this version through the tariff book; `None` is the compiled
    /// constants, byte-identical to the pre-override behaviour.
    pub tariff_pin: Option<crate::pricing::tariff_book::PinnedTariff>,
    pub policy_fast: Option<bool>,
    pub policy_us_inference: Option<bool>,
    /// Internal, generated before reservation; the exactly-once money identity.
    pub request_id: String,
    /// Exact successful provider response status observed before body delivery.
    pub(crate) http_status_code: i32,
    /// Upstream Anthropic request-id retained only as audit metadata.
    pub reference: Option<String>,
    /// Present only for scoped billable Anthropic native Messages or universal OpenAI Chat/Responses.
    /// Admin, SQLite and legacy paths keep this absent and retain their existing money semantics.
    pub(crate) request_fact: Option<AnthropicBillableFactContext>,
}

/// Provider-capacity evidence is independent of customer billing, so admin traffic carries this
/// context even when `BillCtx` is absent.
pub struct CalibrationCtx {
    pub billing: Arc<AsyncBilling>,
    pub request_id: String,
    pub plan: String,
    pub quota_snapshots: Vec<AnthropicQuotaSnapshot>,
    /// Wake the backend quota poller after durable turn evidence has been queued. Response headers
    /// are captured before the streamed body reaches authoritative usage, so without a post-turn
    /// probe busy subscriptions can keep refreshing `polled_ts` while never pairing their newly
    /// completed spend with a later quota fraction.
    pub probe_poke: Option<Arc<tokio::sync::Notify>>,
}

/// Attribution that exists only when a local Claude subscription served the successful attempt.
/// An external emergency transport deliberately omits it so its usage cannot contaminate local
/// subscription spend, quota calibration, health, or capacity leases.
pub struct SubscriptionMeterCtx {
    pub pool: Arc<Pool>,
    pub email: String, // подписка, которая обслужила запрос (для record_spend/калибровки)
    pub calibration: Option<CalibrationCtx>,
    /// Durable capacity lease transferred from the attempt guard to the response stream.
    pub capacity: Option<(Arc<AsyncBilling>, String)>,
}

/// Что нужно, чтобы обработать один успешный ответ на завершении стрима. Списание с клиентского
/// баланса опционально (`bill`), а local-subscription attribution существует только для попыток,
/// действительно обслуженных локальным пулом.
pub struct MeterCtx {
    pub model: String,
    pub is_sse: bool,
    pub bill: Option<BillCtx>,
    pub subscription: Option<SubscriptionMeterCtx>,
}

/// Non-SSE usage is a trailing field of one JSON document, so retain at most the public response
/// limit. SSE is filtered incrementally: only message_start/message_delta/error events are kept;
/// content deltas pass through byte-for-byte without a second in-memory copy. Their delivered UTF-8
/// byte count and observed web-search calls are tracked separately for a conservative truncated-
/// stream charge when the terminal message_delta never arrives.
const JSON_ACC_CAP: usize = 32 * 1024 * 1024;
const SSE_USAGE_CAP: usize = 1024 * 1024;
const SSE_LINE_CAP: usize = 1024 * 1024;
const STREAM_LEASE_SECS: i64 = 3_600;
const STREAM_LEASE_RENEW_SECS: u64 = 300;

fn usage_has_us_inference(usage: &serde_json::Value) -> bool {
    usage
        .get("inference_geo")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|geo| geo.eq_ignore_ascii_case("us"))
}

fn sse_has_us_inference(sse: &str) -> bool {
    for raw in sse.lines() {
        let json = match raw.trim_start().strip_prefix("data:") {
            Some(raw) => raw.trim(),
            None => continue,
        };
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let usage = match value.get("type").and_then(serde_json::Value::as_str) {
            Some("message_start") => value
                .get("message")
                .and_then(|message| message.get("usage")),
            Some("message_delta") => value.get("usage"),
            _ => None,
        };
        if usage.is_some_and(usage_has_us_inference) {
            return true;
        }
    }
    false
}

/// Стрим-обёртка: пропускает чанки клиенту и копит их копию; на конце — списывает.
pub struct TeeMeter {
    inner: ByteStream,
    acc: Vec<u8>,
    sse_line: Vec<u8>,
    sse_drop_line: bool,
    sse_delta_bytes: u64,
    sse_web_search_requests: u64,
    sse_output_tokens: Option<u64>,
    sse_protocol_valid: bool,
    sse_saw_message_start: bool,
    sse_saw_message_delta: bool,
    sse_saw_message_stop: bool,
    sse_saw_error: bool,
    output_tool_calls: bool,
    stream_error: Option<ProviderTerminalClass>,
    lease_heartbeat: Option<tokio::task::JoinHandle<()>>,
    ctx: Option<MeterCtx>, // берётся ровно один раз (finalize идемпотентен)
}

impl TeeMeter {
    pub fn new(inner: ByteStream, ctx: MeterCtx) -> Self {
        let request_id = ctx.bill.as_ref().map(|bill| bill.request_id.clone());
        let capacity_lease_id = ctx
            .subscription
            .as_ref()
            .and_then(|subscription| subscription.capacity.as_ref())
            .map(|(_, lease_id)| lease_id.clone());
        let billing = ctx
            .bill
            .as_ref()
            .map(|bill| bill.billing.clone())
            .or_else(|| {
                ctx.subscription
                    .as_ref()
                    .and_then(|subscription| subscription.capacity.as_ref())
                    .map(|(billing, _)| billing.clone())
            });
        let lease_heartbeat = billing.map(|billing| {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(STREAM_LEASE_RENEW_SECS))
                        .await;
                    if !matches!(
                        billing
                            .renew_stream_leases(
                                request_id.as_deref(),
                                capacity_lease_id.as_deref(),
                                STREAM_LEASE_SECS,
                            )
                            .await,
                        Ok(true)
                    ) {
                        elog::error(
                            "meter",
                            "stream lease renewal failed; live response remains fail-closed",
                        );
                        return;
                    }
                }
            })
        });
        TeeMeter {
            inner,
            acc: Vec::new(),
            sse_line: Vec::new(),
            sse_drop_line: false,
            sse_delta_bytes: 0,
            sse_web_search_requests: 0,
            sse_output_tokens: None,
            sse_protocol_valid: true,
            sse_saw_message_start: false,
            sse_saw_message_delta: false,
            sse_saw_message_stop: false,
            sse_saw_error: false,
            output_tool_calls: false,
            stream_error: None,
            lease_heartbeat,
            ctx: Some(ctx),
        }
    }

    fn retain_sse_usage(&mut self, chunk: &[u8]) {
        for &byte in chunk {
            if self.sse_drop_line {
                if byte == b'\n' {
                    self.sse_drop_line = false;
                }
                continue;
            }
            if byte != b'\n' {
                if self.sse_line.len() < SSE_LINE_CAP {
                    self.sse_line.push(byte);
                } else {
                    self.sse_line.clear();
                    self.sse_drop_line = true;
                    self.sse_protocol_valid = false;
                }
                continue;
            }

            let line = std::mem::take(&mut self.sse_line);
            let Ok(line) = std::str::from_utf8(&line) else {
                self.sse_protocol_valid = false;
                continue;
            };
            let Some(json) = line.trim_start().strip_prefix("data:").map(str::trim) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
                self.sse_protocol_valid = false;
                continue;
            };
            let event_type = value.get("type").and_then(serde_json::Value::as_str);
            match event_type {
                Some("message_start") => self.sse_saw_message_start = true,
                Some("content_block_start") => {
                    let block = value.get("content_block");
                    if block
                        .and_then(|block| block.get("type"))
                        .and_then(serde_json::Value::as_str)
                        .is_none()
                    {
                        self.sse_protocol_valid = false;
                    }
                    if block
                        .and_then(|block| block.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("tool_use")
                    {
                        self.output_tool_calls = true;
                    }
                    if block
                        .and_then(|block| block.get("type"))
                        .and_then(serde_json::Value::as_str)
                        == Some("server_tool_use")
                        && block
                            .and_then(|block| block.get("name"))
                            .and_then(serde_json::Value::as_str)
                            == Some("web_search")
                    {
                        self.sse_web_search_requests =
                            self.sse_web_search_requests.saturating_add(1);
                    }
                }
                Some("content_block_delta") => {
                    if let Some(delta) = value.get("delta") {
                        for field in ["text", "thinking", "partial_json"] {
                            if let Some(text) = delta.get(field).and_then(serde_json::Value::as_str)
                            {
                                self.sse_delta_bytes =
                                    self.sse_delta_bytes.saturating_add(text.len() as u64);
                            }
                        }
                    }
                }
                Some("message_delta") => {
                    self.sse_saw_message_delta = true;
                    if let Some(output_tokens) = value
                        .get("usage")
                        .and_then(|usage| usage.get("output_tokens"))
                        .and_then(serde_json::Value::as_u64)
                    {
                        self.sse_output_tokens = Some(output_tokens);
                    }
                }
                Some("message_stop") => self.sse_saw_message_stop = true,
                Some("error") => self.sse_saw_error = true,
                _ => {}
            }
            let relevant = matches!(
                event_type,
                Some("message_start" | "message_delta" | "error")
            );
            if relevant && self.acc.len().saturating_add(line.len() + 8) <= SSE_USAGE_CAP {
                self.acc.extend_from_slice(b"data: ");
                self.acc.extend_from_slice(json.as_bytes());
                self.acc.extend_from_slice(b"\n\n");
            }
        }
    }

    fn nonstream_terminal_evidence(&self) -> (ProviderTerminalClass, DeliveryState, Option<bool>) {
        if let Some(provider_terminal_class) = self.stream_error {
            return (provider_terminal_class, DeliveryState::Interrupted, None);
        }
        let Ok(response) = serde_json::from_slice::<serde_json::Value>(&self.acc) else {
            return (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            );
        };
        let Some(object) = response.as_object() else {
            return (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            );
        };
        let valid = object.get("type").and_then(serde_json::Value::as_str) == Some("message")
            && object
                .get("content")
                .is_some_and(serde_json::Value::is_array)
            && object
                .get("usage")
                .is_some_and(serde_json::Value::is_object)
            && object
                .get("stop_reason")
                .and_then(serde_json::Value::as_str)
                .is_some();
        if !valid {
            return (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            );
        }
        let blocks = object
            .get("content")
            .and_then(serde_json::Value::as_array)
            .expect("validated Anthropic content array");
        if !blocks.iter().all(|block| {
            block
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some()
        }) {
            return (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            );
        }
        let tool_calls = blocks
            .iter()
            .any(|block| block.get("type").and_then(serde_json::Value::as_str) == Some("tool_use"));
        (
            ProviderTerminalClass::Success,
            DeliveryState::Completed,
            Some(tool_calls),
        )
    }

    fn sse_terminal_evidence(&self) -> (ProviderTerminalClass, DeliveryState, Option<bool>) {
        if let Some(provider_terminal_class) = self.stream_error {
            return (provider_terminal_class, DeliveryState::Interrupted, None);
        }
        if self.sse_saw_error {
            return (
                ProviderTerminalClass::UpstreamError,
                DeliveryState::Interrupted,
                None,
            );
        }
        let exhaustive = self.sse_protocol_valid
            && !self.sse_drop_line
            && self.sse_line.is_empty()
            && self.sse_saw_message_start
            && self.sse_saw_message_delta
            && self.sse_saw_message_stop
            && self.sse_output_tokens.is_some();
        if !exhaustive {
            return (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            );
        }
        (
            ProviderTerminalClass::Success,
            DeliveryState::Completed,
            Some(self.output_tool_calls),
        )
    }

    fn finalize(&mut self) {
        let terminal_result = if self.ctx.as_ref().is_some_and(|ctx| ctx.is_sse) {
            self.sse_terminal_evidence()
        } else {
            self.nonstream_terminal_evidence()
        };
        let ctx = match self.ctx.take() {
            Some(c) => c,
            None => return,
        };
        if let Some(heartbeat) = self.lease_heartbeat.take() {
            heartbeat.abort();
        }
        // Локальная попытка передала слоты стриму — освобождаем их ПЕРВЫМИ даже при пустом usage.
        // External fallback не имеет subscription attribution и сюда не попадает.
        if let Some(subscription) = &ctx.subscription {
            subscription.pool.end_stream(&subscription.email);
            if let Some((billing, lease_id)) = &subscription.capacity {
                billing.release_capacity(lease_id);
            }
        }
        let (usage, served_model, incomplete_non_sse, authoritative_usage, us_inference, speed) =
            if ctx.is_sse {
                let s = String::from_utf8_lossy(&self.acc);
                let mut usage = metering::usage_from_sse(&s);
                usage.web_search_requests =
                    usage.web_search_requests.max(self.sse_web_search_requests);
                if let Some(output_tokens) = self.sse_output_tokens {
                    usage.output_tokens = output_tokens;
                } else {
                    usage.output_tokens = usage.output_tokens.max(self.sse_delta_bytes);
                }
                // ошибка ВНУТРИ стрима после 200 (overloaded посреди генерации) — HTTP-код её не отражал,
                // ротация уже невозможна; логируем, чтобы не была «тихой» (клиент получил её байт-в-байт).
                if metering::sse_has_error(&s) {
                    let source = ctx
                        .subscription
                        .as_ref()
                        .map(|subscription| subscription.email.as_str())
                        .unwrap_or("external-fallback");
                    elog::warn(
                        "meter",
                        format!("SSE-error после 200 на {source} — стрим нёс error-евент"),
                    );
                }
                (
                    usage,
                    metering::model_from_sse(&s),
                    false,
                    self.sse_output_tokens.is_some(),
                    sse_has_us_inference(&s),
                    metering::speed_from_sse(&s),
                )
            } else {
                let response = serde_json::from_slice::<serde_json::Value>(&self.acc).ok();
                let us_inference = response
                    .as_ref()
                    .and_then(|value| value.get("usage"))
                    .is_some_and(usage_has_us_inference);
                (
                    metering::usage_from_response_json(&self.acc),
                    metering::model_from_response_json(&self.acc),
                    response.is_none(),
                    response
                        .as_ref()
                        .and_then(|value| value.get("usage"))
                        .is_some(),
                    us_inference,
                    metering::speed_from_response_json(&self.acc),
                )
            };
        // Тарифицируем по МОДЕЛИ ИЗ ОТВЕТА (авторитетный сервёный id): клиент мог прислать алиас или
        // `-latest`, апстрим резолвит в конкретную датированную модель — считать надо по НЕЙ. Фолбэк —
        // модель запроса (ctx.model), если ответ модель не отдал.
        let price_model = served_model
            .as_deref()
            .filter(|m| !m.is_empty())
            .unwrap_or(&ctx.model);
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // Реальная стоимость (×1.0, до наценки). 0, если usage нет — count_tokens/models/любой 200
        // без usage/обрыв до message_start. ВАЖНО: даже при 0 нельзя просто выйти — иначе hold висел
        // бы в reserved_nano до рестарта (тихая утечка баланса клиента на штатном count_tokens).
        let fast = ctx
            .bill
            .as_ref()
            .and_then(|bill| bill.policy_fast)
            .unwrap_or_else(|| {
                speed
                    .as_deref()
                    .is_some_and(|speed| speed.eq_ignore_ascii_case("fast"))
            });
        let us_inference = ctx
            .bill
            .as_ref()
            .and_then(|bill| bill.policy_us_inference)
            .unwrap_or(us_inference);
        let priced_ts = ctx
            .bill
            .as_ref()
            .and_then(|bill| bill.tariff_priced_ts)
            .unwrap_or(now_unix);
        let compiled_prices = metering::model_prices_for_speed_at(price_model, priced_ts, fast);
        // Hot tariff override: settlement replays the exact version pinned at admission; a
        // cross-family serve reprices by the served family at the pinned priced timestamp; an
        // empty book is byte-identical to the compiled constants. A pinned version missing from
        // the book is an integrity error (the table is append-only), never a silent reprice at
        // compiled: the customer settle below is skipped and left to the reconciler. This sync
        // finalize cannot await the bounded refresh retry the async planes perform; the miss is
        // structurally unreachable because the pinning reserve read the row from this process.
        let served_family = metering::anthropic_matched_tariff_at(price_model, priced_ts, fast)
            .map(|(family, _)| family);
        let pinned_tariff = ctx.bill.as_ref().and_then(|bill| bill.tariff_pin.as_ref());
        let (prices, override_schedule_id, pinned_missing) =
            match crate::pricing::tariff_book::charge_base(
                &crate::pricing::tariff_book::snapshot(),
                pinned_tariff,
                served_family,
                priced_ts,
                compiled_prices,
                crate::pricing::tariff_book::as_anthropic,
            ) {
                crate::pricing::tariff_book::ChargeBase::Compiled(prices) => (prices, None, false),
                crate::pricing::tariff_book::ChargeBase::Override(prices, schedule_id) => {
                    (prices, Some(schedule_id), false)
                }
                crate::pricing::tariff_book::ChargeBase::MissingPinned => {
                    (compiled_prices, None, true)
                }
            };
        if pinned_missing {
            elog::error(
                "meter",
                format!(
                    "pinned tariff {} is absent from the override book; settlement left to recovery",
                    pinned_tariff
                        .map(|pin| pin.schedule_id.as_str())
                        .unwrap_or("<unknown>")
                ),
            );
        }
        let base_breakdown = if usage.is_zero() {
            metering::CostBreakdown::default()
        } else {
            metering::cost_breakdown(&usage, &prices)
        };
        let breakdown = if us_inference {
            base_breakdown.apply_token_multiplier(11_000)
        } else {
            base_breakdown
        };
        let real = breakdown.total();

        // расход в пул только когда он реально был (0 калибровку не двигает)
        if real > 0 {
            if let Some(subscription) = &ctx.subscription {
                subscription.pool.record_spend(&subscription.email, real);
            }
        }

        // Capacity calibration records the official-price workload before applying any customer
        // markup. Response quota snapshots travel in the same ordered writer command, so they can
        // only observe cumulative spend after the immutable turn insert wins.
        if authoritative_usage && real > 0 {
            if let Some((subscription, calibration)) = ctx
                .subscription
                .as_ref()
                .and_then(|s| s.calibration.as_ref().map(|calibration| (s, calibration)))
            {
                let to_i64 = |value: u64| i64::try_from(value).ok();
                let cost_i64 = |value: i128| i64::try_from(value).ok();
                let modifiers = metering::AnthropicAdmissionModifiers {
                    speed: if fast {
                        metering::AnthropicSpeed::Fast
                    } else {
                        metering::AnthropicSpeed::Standard
                    },
                    inference_geo: if us_inference {
                        metering::AnthropicInferenceGeo::Us
                    } else {
                        metering::AnthropicInferenceGeo::Global
                    },
                };
                let tariff_schedule_id = override_schedule_id.clone().unwrap_or_else(|| {
                    metering::anthropic_tariff_capability_at(price_model, priced_ts, modifiers)
                        .map(|identity| identity.tariff_schedule_id.as_str().to_owned())
                        .unwrap_or_else(|_| {
                            // Legacy dated model ids use the same audited metering table but are outside
                            // the narrow strict-pricing identity allowlist. Persist the exact resolved
                            // base rates so this event still has an immutable, unambiguous tariff identity.
                            format!(
                                "anthropic/calibration-rates/v1/i{}-cr{}-cw5{}-cw1{}-o{}-ws{}",
                                prices.input,
                                prices.cache_read,
                                prices.cache_write_5m,
                                prices.cache_write_1h,
                                prices.output,
                                metering::WEB_SEARCH_NANO,
                            )
                        })
                });
                let event = match (
                    to_i64(usage.input_tokens),
                    to_i64(usage.cache_read_tokens),
                    to_i64(usage.cache_write_5m_tokens),
                    to_i64(usage.cache_write_1h_tokens),
                    to_i64(usage.output_tokens),
                    to_i64(usage.web_search_requests),
                    cost_i64(breakdown.input),
                    cost_i64(breakdown.cache_read),
                    cost_i64(breakdown.cache_write_5m),
                    cost_i64(breakdown.cache_write_1h),
                    cost_i64(breakdown.output),
                    cost_i64(breakdown.web_search),
                    cost_i64(real),
                ) {
                    (
                        Some(input_tokens),
                        Some(cache_read_tokens),
                        Some(cache_write_5m_tokens),
                        Some(cache_write_1h_tokens),
                        Some(output_tokens),
                        Some(search_queries),
                        Some(api_input_nanousd),
                        Some(api_cache_read_nanousd),
                        Some(api_cache_write_5m_nanousd),
                        Some(api_cache_write_1h_nanousd),
                        Some(api_output_nanousd),
                        Some(api_search_nanousd),
                        Some(api_total_nanousd),
                    ) => Some(registry::ProviderTurnCalibrationEvent {
                        provider: registry::PROVIDER_ANTHROPIC.to_owned(),
                        request_id: calibration.request_id.clone(),
                        subject_id: subscription.email.clone(),
                        model_id: price_model.to_owned(),
                        service_tier: if fast { "fast" } else { "standard" }.to_owned(),
                        inference_geo: if us_inference { "us" } else { "global" }.to_owned(),
                        tariff_schedule_id,
                        priced_ts,
                        completed_at: now_unix,
                        input_tokens,
                        audio_input_tokens: 0,
                        cache_read_tokens,
                        cached_audio_input_tokens: 0,
                        cache_write_5m_tokens,
                        cache_write_1h_tokens,
                        output_tokens,
                        thinking_output_tokens: 0,
                        image_output_tokens: 0,
                        tool_prompt_tokens: 0,
                        search_queries,
                        grounded_search_prompts: 0,
                        api_input_nanousd,
                        api_audio_input_nanousd: 0,
                        api_cache_read_nanousd,
                        api_cached_audio_input_nanousd: 0,
                        api_cache_write_5m_nanousd,
                        api_cache_write_1h_nanousd,
                        api_output_nanousd,
                        api_image_output_nanousd: 0,
                        api_search_nanousd,
                        api_total_nanousd,
                    }),
                    _ => None,
                };
                if let Some(event) = event {
                    let queued = calibration.billing.record_anthropic_turn_detached(
                        event,
                        &calibration.plan,
                        calibration.quota_snapshots.clone(),
                    );
                    if queued {
                        // Queue the immutable spend evidence before waking the quota probe
                        // (messages with max_tokens=0). `AnthropicObserveWindow` also flushes the
                        // pending turn FIFO first, so writer backpressure cannot invert spend and
                        // quota observations.
                        subscription.pool.request_probe(&subscription.email);
                        if let Some(poke) = &calibration.probe_poke {
                            poke.notify_one();
                        }
                    }
                } else {
                    elog::error(
                        "meter",
                        "Anthropic calibration event exceeds durable bigint bounds",
                    );
                }
            }
        }

        // Резерв метерного ключа закрываем ВСЕГДА: actual = charge (0 при usage=0 → полный возврат
        // hold). settle возвращает hold и списывает actual → итог по паре reserve→settle = −actual.
        if let Some(b) = ctx.bill {
            // TeeMeter::drop продолжает дренировать upstream после downstream-disconnect, поэтому сюда
            // обычно приходит authoritative usage. Истинно оборванный upstream без usage списывает 0 и
            // оставляет явный диагностический сигнал вместо бездоказательного списания всего hold.
            let computed_charge = if incomplete_non_sse {
                // A missing authoritative usage object is not proof that the provider consumed the
                // maximum reservation. Downstream cancellation is drained asynchronously in Drop;
                // a genuine upstream truncation is settled at zero and surfaced for reconciliation.
                elog::warn(
                    "meter",
                    "incomplete non-SSE response without authoritative usage; charge=0",
                );
                0
            } else if real > 0 {
                metering::apply_multiplier(real, b.mult_bp)
            } else {
                0
            };
            let hold_cap = b.hold.max(0) as i128;
            if computed_charge > hold_cap {
                // The hold is admission evidence, not a second price ceiling. Registry serializes
                // collection on the account row, leaves the balance at the shared floor and records
                // the full remainder as uncollected; clamping here would silently erase pool loss.
                elog::warn(
                    "meter",
                    format!(
                        "billing charge превысил hold: charge_nano={computed_charge} hold_nano={hold_cap}; \
                         full actual retained for account-floor settlement | model={price_model} \
                         us_geo={us_inference} real_nano={real} \
                         in={} out={} cr={} cw5={} cw1={} web={}",
                        usage.input_tokens, usage.output_tokens, usage.cache_read_tokens,
                        usage.cache_write_5m_tokens, usage.cache_write_1h_tokens, usage.web_search_requests,
                    ),
                );
            }
            let charge_i64 = computed_charge.clamp(0, i64::MAX as i128) as i64;
            // Разбивка токенов/модели для клиентского дашборда — пишется рядом с charge (аналитика).
            // Только при авторитетном usage; C8-preserved hold не изображаем как токеновое событие.
            // A zero multiplier is the free-but-metered account: it charges exactly zero and must
            // still record the turn, otherwise the usage of an internal service would be invisible.
            let meter_only = b.mult_bp == 0;
            let usage_event = if (charge_i64 > 0 || meter_only) && real > 0 {
                Some(registry::UsageEventInput {
                    model: price_model.to_string(),
                    provider: registry::PROVIDER_ANTHROPIC.to_string(),
                    input_tokens: usage.input_tokens as i64,
                    output_tokens: usage.output_tokens as i64,
                    cache_read_tokens: usage.cache_read_tokens as i64,
                    cache_write_5m_tokens: usage.cache_write_5m_tokens as i64,
                    cache_write_1h_tokens: usage.cache_write_1h_tokens as i64,
                    web_search_requests: usage.web_search_requests as i64,
                    real_nano: real.clamp(0, i64::MAX as i128) as i64,
                    charge_basis_nano: real.clamp(0, i64::MAX as i128) as i64,
                    speed: if fast { "fast" } else { "standard" }.to_string(),
                    inference_geo: if us_inference {
                        "us".to_string()
                    } else {
                        String::new()
                    },
                    input_nano: breakdown.input.clamp(0, i64::MAX as i128) as i64,
                    output_nano: breakdown.output.clamp(0, i64::MAX as i128) as i64,
                    cache_read_nano: breakdown.cache_read.clamp(0, i64::MAX as i128) as i64,
                    cache_write_5m_nano: breakdown.cache_write_5m.clamp(0, i64::MAX as i128) as i64,
                    cache_write_1h_nano: breakdown.cache_write_1h.clamp(0, i64::MAX as i128) as i64,
                    web_search_nano: breakdown.web_search.clamp(0, i64::MAX as i128) as i64,
                    priced_ts,
                })
            } else {
                None
            };
            // finalize СИНХРОНЕН (Stream::poll / Drop) → шлём списание АСИНХРОННО через актор
            // (settle_detached не блокирует). Гарантия: осиротевшее при краше вернёт reconcile.
            // A missing pinned override version never settles at compiled prices: the reservation
            // is left to the reconciler instead of this settle.
            if !pinned_missing {
                let settlement = match b.request_fact.as_ref() {
                    Some(request_fact) => {
                        let (provider_terminal_class, delivery_state, tool_calls_in_output) =
                            terminal_result;
                        let evidence = request_fact.terminal_evidence(
                            Some(b.http_status_code),
                            provider_terminal_class,
                            delivery_state,
                            tool_calls_in_output,
                            true,
                        );
                        b.billing.settle_detached_with_request_fact(
                            &b.request_id,
                            &b.account_id,
                            &b.key,
                            b.hold,
                            charge_i64,
                            b.reference.as_deref(),
                            usage_event,
                            evidence,
                        )
                    }
                    None => {
                        b.billing.settle_detached(
                            &b.request_id,
                            &b.account_id,
                            &b.key,
                            b.hold,
                            charge_i64,
                            b.reference.as_deref(),
                            usage_event,
                        );
                        Ok(())
                    }
                };
                if let Err(error) = settlement {
                    // The reservation/fact remain for the existing reconciler rather than issuing a
                    // late fact-free settlement after typed evidence validation failed.
                    elog::error(
                        "meter",
                        format!("Anthropic request-fact terminal evidence rejected: {error:#}"),
                    );
                }
                if charge_i64 > 0 {
                    // хвост ключа для лога — по символам (не байтами: срез не на границе char паникует)
                    let tail: String = {
                        let mut t: Vec<char> = b.key.chars().rev().take(4).collect();
                        t.reverse();
                        t.into_iter().collect()
                    };
                    elog::info(
                        "meter",
                        format!(
                            "ключ …{tail}: −{} [{}]",
                            metering::nano_to_usd_string(charge_i64 as i128),
                            if price_model.is_empty() {
                                "?"
                            } else {
                                price_model
                            }
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use pool::Reserve;
    use registry::Sub;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    const ACCOUNT_ID: &str = "acct";
    const KEY: &str = "sk-pool-meter-test";
    const EMAIL: &str = "subscription@example.test";
    const TOPUP_NANO: i64 = 1_000_000_000;
    const HOLD_NANO: i64 = 500_000_000;
    static BILL_DB_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct BilledSse {
        delivered: Vec<u8>,
        balance_nano: i64,
        reserved_nano: i64,
        usage: registry::UsageModelAgg,
    }

    async fn bill_sse(sse: &[u8], drop_after_first_chunk: bool) -> BilledSse {
        bill_sse_with_pin(sse, drop_after_first_chunk, None).await
    }

    async fn bill_sse_with_pin(
        sse: &[u8],
        drop_after_first_chunk: bool,
        tariff_pin: Option<crate::pricing::tariff_book::PinnedTariff>,
    ) -> BilledSse {
        let unique_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique_sequence = BILL_DB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-tee-meter-{}-{unique_time}-{unique_sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start test billing"),
        );
        billing
            .create_account(ACCOUNT_ID, None, 10_000)
            .await
            .unwrap();
        billing
            .topup(ACCOUNT_ID, TOPUP_NANO, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT_ID, None, None, None)
            .await
            .unwrap();
        assert_eq!(
            billing
                .reserve_request("request", ACCOUNT_ID, KEY, HOLD_NANO)
                .await
                .unwrap(),
            Some(TOPUP_NANO - HOLD_NANO)
        );

        let pool = Arc::new(Pool::new(
            vec![Sub {
                email: EMAIL.into(),
                token: "secret".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            }],
            Reserve::FULL,
            50.0,
            1_500.0,
        ));
        pool.mark_used(EMAIL);

        // Seventeen-byte chunks split both SSE lines and multi-byte UTF-8 code points. The exact
        // upstream bytes must still be returned unchanged while the side-channel meter observes
        // complete logical events.
        let frames = sse
            .chunks(17)
            .map(Bytes::copy_from_slice)
            .map(Ok::<_, std::io::Error>)
            .collect::<Vec<_>>();
        let inner: ByteStream = Box::pin(futures_util::stream::iter(frames));
        let mut meter = TeeMeter::new(
            inner,
            MeterCtx {
                model: "claude-sonnet-4-6".into(),
                is_sse: true,
                bill: Some(BillCtx {
                    billing: Arc::clone(&billing),
                    account_id: ACCOUNT_ID.into(),
                    key: KEY.into(),
                    mult_bp: 10_000,
                    hold: HOLD_NANO,
                    tariff_priced_ts: None,
                    tariff_pin,
                    policy_fast: None,
                    policy_us_inference: None,
                    request_id: "request".into(),
                    http_status_code: 200,
                    reference: None,
                    request_fact: None,
                }),
                subscription: Some(SubscriptionMeterCtx {
                    pool,
                    email: EMAIL.into(),
                    calibration: None,
                    capacity: None,
                }),
            },
        );
        let mut delivered = Vec::new();
        if drop_after_first_chunk {
            delivered.extend_from_slice(&meter.next().await.unwrap().unwrap());
            drop(meter);
        } else {
            while let Some(frame) = meter.next().await {
                delivered.extend_from_slice(&frame.unwrap());
            }
        }

        let account = loop {
            billing.flush().await.unwrap();
            let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
            if account.reserved_nano == 0 {
                break account;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        };
        let mut usage = billing.usage_by_model(ACCOUNT_ID, 0).await.unwrap();
        assert_eq!(usage.len(), 1);
        let outcome = BilledSse {
            delivered,
            balance_nano: account.balance_nano,
            reserved_nano: account.reserved_nano,
            usage: usage.remove(0),
        };
        drop(billing);
        let _ = std::fs::remove_file(path);
        outcome
    }

    fn message_start() -> String {
        format!(
            "data: {}\n\n",
            serde_json::json!({
                "type": "message_start",
                "message": {
                    "model": "claude-sonnet-4-6",
                    "usage": {"input_tokens": 10, "output_tokens": 1}
                }
            })
        )
    }

    #[tokio::test]
    async fn authoritative_turn_queues_evidence_and_wakes_backend_quota_probe() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-tee-calibration-probe-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start test billing"),
        );
        let pool = Arc::new(Pool::new(
            vec![Sub {
                email: EMAIL.into(),
                token: "secret".into(),
                proxy: String::new(),
                fleet: "test".into(),
                plan: "max20".into(),
            }],
            Reserve::FULL,
            50.0,
            1_500.0,
        ));
        pool.set_util(
            EMAIL,
            Some(0.10),
            Some(0.20),
            None,
            Some(2_000_000_000),
            Some(2_000_500_000),
        );
        assert!(pool.snapshot()[0].1.polled_ts > 0);
        pool.mark_used(EMAIL);

        let probe_poke = Arc::new(tokio::sync::Notify::new());
        let sse = format!(
            "{}data: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}
            }),
        );
        let inner: ByteStream =
            Box::pin(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
                Bytes::from(sse),
            )]));
        let mut meter = TeeMeter::new(
            inner,
            MeterCtx {
                model: "claude-sonnet-4-6".into(),
                is_sse: true,
                bill: None,
                subscription: Some(SubscriptionMeterCtx {
                    pool: Arc::clone(&pool),
                    email: EMAIL.into(),
                    calibration: Some(CalibrationCtx {
                        billing: Arc::clone(&billing),
                        request_id: "calibration-probe-request".into(),
                        plan: "max20".into(),
                        quota_snapshots: Vec::new(),
                        probe_poke: Some(Arc::clone(&probe_poke)),
                    }),
                    capacity: None,
                }),
            },
        );
        while meter.next().await.is_some() {}

        tokio::time::timeout(std::time::Duration::from_millis(100), probe_poke.notified())
            .await
            .expect("successful turn must wake the backend quota poller");
        assert_eq!(
            pool.snapshot()[0].1.polled_ts,
            0,
            "post-turn probe must stay due even though response headers were fresh",
        );

        billing.flush().await.unwrap();
        let (_, evidence, recent_turns) = billing.anthropic_calibration_report().await.unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].turns, 1);
        assert_eq!(evidence[0].subject_id, EMAIL);
        assert_eq!(recent_turns.len(), 1);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn truncated_sse_charges_delivered_utf8_bytes_and_web_search() {
        let text = "🙂".repeat(1_000);
        assert_eq!(text.len(), 4_000);
        let sse = format!(
            "{}data: {}\n\ndata: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "server_tool_use",
                    "id": "srvtoolu_1",
                    "name": "web_search"
                }
            }),
            serde_json::json!({
                "type": "content_block_delta",
                "index": 1,
                "delta": {"type": "text_delta", "text": text}
            }),
        );

        let outcome = bill_sse(sse.as_bytes(), false).await;
        assert_eq!(outcome.delivered, sse.as_bytes());
        assert_eq!(outcome.reserved_nano, 0);
        // Sonnet: 10 input * 3,000 + 4,000 output * 15,000 + one $0.01 web search.
        assert_eq!(outcome.balance_nano, TOPUP_NANO - 70_030_000);
        assert_eq!(outcome.usage.input_tokens, 10);
        assert_eq!(outcome.usage.output_tokens, 4_000);
        assert_eq!(outcome.usage.web_search_requests, 1);
        assert_eq!(outcome.usage.charge_nano, 70_030_000);
    }

    /// Settlement replays the exact override version pinned at admission: the same turn the
    /// compiled card prices at 60_000 nano settles at the override card.
    #[tokio::test]
    async fn settlement_replays_the_pinned_override_version() {
        let _lock = crate::pricing::tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // effective_from = i64::MAX keeps the row invisible to every timestamped resolve — and so
        // to every concurrently running test; only the exact pinned-version lookup sees it.
        crate::pricing::tariff_book::install_global_rows_for_test(vec![
            crate::pricing::tariff_book::test_row(
                "anthropic/standard/sonnet-current",
                2,
                i64::MAX,
                serde_json::json!({
                    "input": "6000",
                    "output": "30000",
                    "cache_read": "1000",
                    "cache_write_5m": "12500",
                    "cache_write_1h": "20000"
                }),
            ),
        ]);
        let sse = format!(
            "{}data: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}
            }),
        );
        let outcome = bill_sse_with_pin(
            sse.as_bytes(),
            false,
            Some(crate::pricing::tariff_book::PinnedTariff {
                family: "anthropic/standard/sonnet-current".to_owned(),
                version: 2,
                schedule_id: "anthropic/standard/sonnet-current/v2".to_owned(),
            }),
        )
        .await;
        crate::pricing::tariff_book::clear_global_book_for_test();
        // 10 input × 6_000 + 2 output × 30_000 at the override card — compiled would be 60_000.
        assert_eq!(outcome.balance_nano, TOPUP_NANO - 120_000);
        assert_eq!(outcome.usage.charge_nano, 120_000);
    }

    /// A pinned version the book cannot produce is an integrity error: nothing is settled at
    /// compiled prices and the reservation is left for durable recovery.
    #[tokio::test]
    async fn a_missing_pinned_override_version_never_settles_at_compiled() {
        let _lock = crate::pricing::tariff_book::GLOBAL_BOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        crate::pricing::tariff_book::clear_global_book_for_test();
        let unique_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique_sequence = BILL_DB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-tee-meter-missing-pin-{}-{unique_time}-{unique_sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start test billing"),
        );
        billing
            .create_account(ACCOUNT_ID, None, 10_000)
            .await
            .unwrap();
        billing
            .topup(ACCOUNT_ID, TOPUP_NANO, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT_ID, None, None, None)
            .await
            .unwrap();
        billing
            .reserve_request("missing-pin-request", ACCOUNT_ID, KEY, HOLD_NANO)
            .await
            .unwrap();

        let sse = format!(
            "{}data: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}
            }),
        );
        let inner: ByteStream =
            Box::pin(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(
                Bytes::from(sse),
            )]));
        let mut meter = TeeMeter::new(
            inner,
            MeterCtx {
                model: "claude-sonnet-4-6".into(),
                is_sse: true,
                bill: Some(BillCtx {
                    billing: Arc::clone(&billing),
                    account_id: ACCOUNT_ID.into(),
                    key: KEY.into(),
                    mult_bp: 10_000,
                    hold: HOLD_NANO,
                    tariff_priced_ts: None,
                    tariff_pin: Some(crate::pricing::tariff_book::PinnedTariff {
                        family: "anthropic/standard/sonnet-current".to_owned(),
                        version: 9,
                        schedule_id: "anthropic/standard/sonnet-current/v9".to_owned(),
                    }),
                    policy_fast: None,
                    policy_us_inference: None,
                    request_id: "missing-pin-request".into(),
                    http_status_code: 200,
                    reference: None,
                    request_fact: None,
                }),
                subscription: None,
            },
        );
        while meter.next().await.is_some() {}
        // Give the (never dispatched) settle a chance to prove it does not exist.
        billing.flush().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
        drop(billing);
        let _ = std::fs::remove_file(path);
        crate::pricing::tariff_book::clear_global_book_for_test();
        // No settle happened: the hold stays reserved for the reconciler, nothing is spent.
        assert_eq!(account.reserved_nano, HOLD_NANO);
        assert_eq!(account.balance_nano, TOPUP_NANO - HOLD_NANO);
    }

    #[tokio::test]
    async fn terminal_sse_usage_overrides_observed_delta_bytes() {
        let text = "🙂".repeat(1_000);
        let sse = format!(
            "{}data: {}\n\ndata: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text}
            }),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": 2}
            }),
        );

        let outcome = bill_sse(sse.as_bytes(), false).await;
        assert_eq!(outcome.delivered, sse.as_bytes());
        assert_eq!(outcome.reserved_nano, 0);
        assert_eq!(outcome.balance_nano, TOPUP_NANO - 60_000);
        assert_eq!(outcome.usage.input_tokens, 10);
        assert_eq!(outcome.usage.output_tokens, 2);
        assert_eq!(outcome.usage.web_search_requests, 0);
        assert_eq!(outcome.usage.charge_nano, 60_000);
    }

    #[test]
    fn attempt_tracker_is_exact_and_terminal_clock_seals_once() {
        let mut attempts = AnthropicAttemptTracker::default();
        assert_eq!(attempts.exhaustive_i32(), Some(0));
        attempts.record_send();
        attempts.record_send();
        assert_eq!(attempts.exhaustive_i32(), Some(2));

        let clock = RequestLifecycleClock::default();
        let context = AnthropicBillableFactContext {
            admitted_at: pool::now(),
            lifecycle_clock: clock.clone(),
            attempts,
            upstream_request_id: Some("req-bounded".into()),
            downstream_disconnect: Some(false),
        };
        let evidence = context.terminal_evidence(
            Some(200),
            ProviderTerminalClass::Success,
            DeliveryState::Completed,
            Some(false),
            true,
        );
        assert_eq!(evidence.internal_attempt_count, Some(2));
        assert_eq!(evidence.downstream_disconnect, Some(false));
        assert_eq!(evidence.tool_calls_in_output, Some(false));
        assert_eq!(evidence.first_public_byte_at, None);
        clock.observe_first_public_byte();
        assert_eq!(clock.first_public_byte_at(), None);
    }

    #[test]
    fn terminal_output_tool_call_evidence_requires_exhaustive_native_output() {
        let ctx = MeterCtx {
            model: "claude-test".into(),
            is_sse: false,
            bill: None,
            subscription: None,
        };
        let mut meter = TeeMeter::new(Box::pin(futures_util::stream::empty()), ctx);
        meter.acc = br#"{"type":"message","content":[{"type":"tool_use","id":"toolu_private","name":"never-store","input":{"secret":true}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}}"#.to_vec();
        assert_eq!(
            meter.nonstream_terminal_evidence(),
            (
                ProviderTerminalClass::Success,
                DeliveryState::Completed,
                Some(true)
            )
        );

        meter.acc = br#"{"type":"message","content":[{"type":"text","text":"PRIVATE"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#.to_vec();
        assert_eq!(
            meter.nonstream_terminal_evidence(),
            (
                ProviderTerminalClass::Success,
                DeliveryState::Completed,
                Some(false)
            )
        );

        meter.acc = br#"{"type":"message","content":[{"type":"tool_use"}]"#.to_vec();
        assert_eq!(
            meter.nonstream_terminal_evidence(),
            (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None
            )
        );

        meter.acc = br#"{"type":"message","content":[{"type":"text","text":"PRIVATE"},{"payload":"malformed"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#.to_vec();
        assert_eq!(
            meter.nonstream_terminal_evidence(),
            (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None
            )
        );
    }

    #[test]
    fn sse_terminal_truth_requires_valid_stop_and_never_infers_disconnect_from_error() {
        let ctx = MeterCtx {
            model: "claude-test".into(),
            is_sse: true,
            bill: None,
            subscription: None,
        };
        let mut meter = TeeMeter::new(Box::pin(futures_util::stream::empty()), ctx);
        meter.retain_sse_usage(
            b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n\
              data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"tool_use\",\"name\":\"PRIVATE\"}}\n\n\
              data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":1}}\n\n\
              data: {\"type\":\"message_stop\"}\n\n",
        );
        assert_eq!(
            meter.sse_terminal_evidence(),
            (
                ProviderTerminalClass::Success,
                DeliveryState::Completed,
                Some(true)
            )
        );

        meter.sse_saw_error = true;
        assert_eq!(
            meter.sse_terminal_evidence(),
            (
                ProviderTerminalClass::UpstreamError,
                DeliveryState::Interrupted,
                None
            )
        );
        assert!(meter
            .ctx
            .as_ref()
            .and_then(|ctx| ctx.bill.as_ref())
            .is_none());
    }

    #[tokio::test]
    async fn downstream_abort_drains_truncated_upstream_before_billing() {
        let text = "🙂".repeat(1_000);
        let sse = format!(
            "{}data: {}\n\n",
            message_start(),
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text}
            }),
        );

        let outcome = bill_sse(sse.as_bytes(), true).await;
        assert_eq!(outcome.delivered, &sse.as_bytes()[..17]);
        assert_eq!(outcome.reserved_nano, 0);
        assert_eq!(outcome.balance_nano, TOPUP_NANO - 60_030_000);
        assert_eq!(outcome.usage.output_tokens, 4_000);
        assert_eq!(outcome.usage.charge_nano, 60_030_000);
    }
}

impl Stream for TeeMeter {
    type Item = Result<Bytes, std::io::Error>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.get_mut();
        match me.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                let is_sse = me.ctx.as_ref().is_some_and(|ctx| ctx.is_sse);
                if is_sse {
                    me.retain_sse_usage(&chunk);
                } else if me.acc.len().saturating_add(chunk.len()) <= JSON_ACC_CAP {
                    me.acc.extend_from_slice(&chunk);
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                me.stream_error = Some(
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) {
                        ProviderTerminalClass::Timeout
                    } else {
                        ProviderTerminalClass::Transport
                    },
                );
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                if let Some(request_fact) = me
                    .ctx
                    .as_mut()
                    .and_then(|ctx| ctx.bill.as_mut())
                    .and_then(|bill| bill.request_fact.as_mut())
                {
                    request_fact.downstream_disconnect = Some(false);
                }
                me.finalize();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

// Обрыв соединения клиентом: стрим дропается, не дойдя до конца. Тарифицируем то, что успели
// (частичный usage — обычно недосчёт, но лучше, чем ноль). При нормальном завершении ctx уже
// взят в poll_next → здесь no-op.
impl Drop for TeeMeter {
    fn drop(&mut self) {
        if self.ctx.is_none() {
            return;
        }

        // A downstream disconnect must not turn an unknown partial JSON body into a maximum charge.
        // Keep reading the already-started upstream response in a bounded background task so its
        // authoritative final usage can settle the request. The stream carries the capacity/global
        // guards, so those remain held until the drain completes.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if self.stream_error.is_none() {
                if let Some(request_fact) = self
                    .ctx
                    .as_mut()
                    .and_then(|ctx| ctx.bill.as_mut())
                    .and_then(|bill| bill.request_fact.as_mut())
                {
                    request_fact.downstream_disconnect = Some(true);
                }
            }
            let detached_work = self.ctx.as_ref().and_then(|ctx| {
                ctx.bill
                    .as_ref()
                    .map(|bill| bill.billing.track_detached_work())
                    .or_else(|| {
                        ctx.subscription
                            .as_ref()
                            .and_then(|subscription| subscription.capacity.as_ref())
                            .map(|(billing, _)| billing.track_detached_work())
                    })
            });
            let inner = std::mem::replace(&mut self.inner, Box::pin(futures_util::stream::empty()));
            let ctx = self.ctx.take();
            let acc = std::mem::take(&mut self.acc);
            let sse_line = std::mem::take(&mut self.sse_line);
            let sse_drop_line = self.sse_drop_line;
            let sse_delta_bytes = self.sse_delta_bytes;
            let sse_web_search_requests = self.sse_web_search_requests;
            let sse_output_tokens = self.sse_output_tokens;
            let sse_protocol_valid = self.sse_protocol_valid;
            let sse_saw_message_start = self.sse_saw_message_start;
            let sse_saw_message_delta = self.sse_saw_message_delta;
            let sse_saw_message_stop = self.sse_saw_message_stop;
            let sse_saw_error = self.sse_saw_error;
            let output_tool_calls = self.output_tool_calls;
            let stream_error = self.stream_error;
            let lease_heartbeat = self.lease_heartbeat.take();
            handle.spawn(async move {
                use futures_util::StreamExt;
                let mut meter = TeeMeter {
                    inner,
                    acc,
                    sse_line,
                    sse_drop_line,
                    sse_delta_bytes,
                    sse_web_search_requests,
                    sse_output_tokens,
                    sse_protocol_valid,
                    sse_saw_message_start,
                    sse_saw_message_delta,
                    sse_saw_message_stop,
                    sse_saw_error,
                    output_tool_calls,
                    stream_error,
                    lease_heartbeat,
                    ctx,
                };
                while let Some(frame) = meter.inner.next().await {
                    let chunk = match frame {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            meter.stream_error = Some(
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                                ) {
                                    ProviderTerminalClass::Timeout
                                } else {
                                    ProviderTerminalClass::Transport
                                },
                            );
                            break;
                        }
                    };
                    let is_sse = meter.ctx.as_ref().is_some_and(|ctx| ctx.is_sse);
                    if is_sse {
                        meter.retain_sse_usage(&chunk);
                    } else if meter.acc.len().saturating_add(chunk.len()) <= JSON_ACC_CAP {
                        meter.acc.extend_from_slice(&chunk);
                    }
                }
                meter.finalize();
                drop(detached_work);
            });
        } else {
            // Only possible outside the HTTP runtime (for example during abnormal teardown).
            self.finalize();
        }
    }
}
