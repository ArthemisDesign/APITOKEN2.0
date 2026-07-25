//! Тарификация ответа: tee-стрим поверх тела апстрима.
//!
//! Инвариант прозрачности не нарушается: клиент получает байты апстрима БАЙТ-В-БАЙТ и без
//! задержки — мы лишь параллельно копим копию тела. Когда стрим завершился (или оборвался),
//! парсим `usage` (SSE — из накопленного текста, не-стрим — из полного JSON), считаем стоимость
//! через `metering` и списываем с баланса ключа. Метерим ТОЛЬКО успешный ответ (см. proxy.rs).

use crate::billing::AsyncBilling;
use bytes::Bytes;
use futures_util::Stream;
use pool::Pool;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Опциональное списание с АККАУНТА клиента (только для метерных ключей). Баланс общий на аккаунт;
/// `key` — для атрибуции расхода по ключу; `request_id` — в ledger как ссылка на запрос.
pub struct BillCtx {
    pub billing: Arc<AsyncBilling>,
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub hold: i64, // зарезервированный при допуске потолок — закрываем его фактической стоимостью
    /// Internal, generated before reservation; the exactly-once money identity.
    pub request_id: String,
    /// Upstream Anthropic request-id retained only as audit metadata.
    pub reference: Option<String>,
}

/// Что нужно, чтобы обработать один успешный ответ на завершении стрима. Делаем ВСЕГДА
/// (для калибровки ёмкости окна нужен расход ЛЮБОГО запроса, включая админский), а списание
/// с баланса — опционально (`bill`), только для метерных ключей.
pub struct MeterCtx {
    pub pool: Arc<Pool>,
    pub email: String, // подписка, которая обслужила запрос (для record_spend/калибровки)
    pub model: String,
    pub is_sse: bool,
    pub bill: Option<BillCtx>,
    /// Durable capacity lease transferred from the attempt guard to the response stream.
    pub capacity: Option<(Arc<AsyncBilling>, String)>,
}

/// Non-SSE usage is a trailing field of one JSON document, so retain at most the public response
/// limit. SSE is filtered incrementally: only message_start/message_delta/error events are kept;
/// content deltas pass through byte-for-byte without a second in-memory copy.
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
    lease_heartbeat: Option<tokio::task::JoinHandle<()>>,
    ctx: Option<MeterCtx>, // берётся ровно один раз (finalize идемпотентен)
}

impl TeeMeter {
    pub fn new(inner: ByteStream, ctx: MeterCtx) -> Self {
        let request_id = ctx.bill.as_ref().map(|bill| bill.request_id.clone());
        let capacity_lease_id = ctx.capacity.as_ref().map(|(_, lease_id)| lease_id.clone());
        let billing = ctx
            .bill
            .as_ref()
            .map(|bill| bill.billing.clone())
            .or_else(|| ctx.capacity.as_ref().map(|(billing, _)| billing.clone()));
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
                        eprintln!("stream lease renewal failed; live response remains fail-closed");
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
                }
                continue;
            }

            let line = std::mem::take(&mut self.sse_line);
            let Ok(line) = std::str::from_utf8(&line) else {
                continue;
            };
            let Some(json) = line.trim_start().strip_prefix("data:").map(str::trim) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
                continue;
            };
            let relevant = matches!(
                value.get("type").and_then(serde_json::Value::as_str),
                Some("message_start" | "message_delta" | "error")
            );
            if relevant && self.acc.len().saturating_add(line.len() + 8) <= SSE_USAGE_CAP {
                self.acc.extend_from_slice(b"data: ");
                self.acc.extend_from_slice(json.as_bytes());
                self.acc.extend_from_slice(b"\n\n");
            }
        }
    }

    fn finalize(&mut self) {
        let ctx = match self.ctx.take() {
            Some(c) => c,
            None => return,
        };
        if let Some(heartbeat) = self.lease_heartbeat.take() {
            heartbeat.abort();
        }
        // стрим завершён/оборван → освободить слот конкуррентности персоны (парен с mark_used).
        // Делаем ПЕРВЫМ и безусловно, даже если usage пустой (иначе in-flight подтёк бы).
        ctx.pool.end_stream(&ctx.email);
        if let Some((billing, lease_id)) = &ctx.capacity {
            billing.release_capacity(lease_id);
        }
        let (usage, served_model, incomplete_non_sse, us_inference, speed) = if ctx.is_sse {
            let s = String::from_utf8_lossy(&self.acc);
            // ошибка ВНУТРИ стрима после 200 (overloaded посреди генерации) — HTTP-код её не отражал,
            // ротация уже невозможна; логируем, чтобы не была «тихой» (клиент получил её байт-в-байт).
            if metering::sse_has_error(&s) {
                eprintln!(
                    "⚠ SSE-error после 200 на {} — стрим нёс error-евент",
                    ctx.email
                );
            }
            (
                metering::usage_from_sse(&s),
                metering::model_from_sse(&s),
                false,
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
        let fast = speed
            .as_deref()
            .is_some_and(|speed| speed.eq_ignore_ascii_case("fast"));
        let prices = metering::model_prices_for_speed_at(price_model, now_unix, fast);
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
            ctx.pool.record_spend(&ctx.email, real);
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
                eprintln!("⚠ incomplete non-SSE response without authoritative usage; charge=0");
                0
            } else if real > 0 {
                metering::apply_multiplier(real, b.mult_bp)
            } else {
                0
            };
            // Никогда не списываем больше атомарно зарезервированного потолка: иначе параллельные запросы
            // могут совместно увести общий баланс аккаунта ниже нуля из-за неучтённого preflight-overhead.
            // AUDIT-TODO(C53): резервировать по count_tokens точного post-injection запроса с tool overhead.
            let hold_cap = b.hold.max(0) as i128;
            // Списываем РЕАЛЬНОЕ, а не clamp до hold: деньги на аккаунте есть, а clamp молча терял разницу
            // (недобор). registry settle умеет actual>hold — списывает из остатка баланса (допустимый
            // овердрафт). Кап на hold+$1 бьётся с овердрафт-буфером резерва: один запрос не спишет больше
            // hold+буфер даже при аномальном usage (пол убытка ограничен).
            let charge_ceiling = hold_cap + metering::OVERDRAFT_NANO;
            if computed_charge > hold_cap {
                // Разбивка usage: реальный расход пробил (порезанный балансом) hold — обычно cache_read
                // либо аномалия. Списываем реальное до hold+$1; разбивка нужна для точного preflight-резерва.
                eprintln!(
                    "⚠ billing charge превысил hold: charge_nano={computed_charge} hold_nano={hold_cap}; \
                     charge real до hold+$1 | model={price_model} us_geo={us_inference} real_nano={real} \
                     in={} out={} cr={} cw5={} cw1={} web={}",
                    usage.input_tokens, usage.output_tokens, usage.cache_read_tokens,
                    usage.cache_write_5m_tokens, usage.cache_write_1h_tokens, usage.web_search_requests,
                );
            }
            let charge_i64 = computed_charge.clamp(0, charge_ceiling) as i64;
            // Разбивка токенов/модели для клиентского дашборда — пишется рядом с charge (аналитика).
            // Только при авторитетном usage; C8-preserved hold не изображаем как токеновое событие.
            let usage_event = if charge_i64 > 0 && real > 0 {
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
                    speed: speed.unwrap_or_else(|| "standard".to_string()),
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
                    priced_ts: now_unix,
                })
            } else {
                None
            };
            // finalize СИНХРОНЕН (Stream::poll / Drop) → шлём списание АСИНХРОННО через актор
            // (settle_detached не блокирует). Гарантия: осиротевшее при краше вернёт reconcile.
            b.billing.settle_detached(
                &b.request_id,
                &b.account_id,
                &b.key,
                b.hold,
                charge_i64,
                b.reference.as_deref(),
                usage_event,
            );
            if charge_i64 > 0 {
                // хвост ключа для лога — по символам (не байтами: срез не на границе char паникует)
                let tail: String = {
                    let mut t: Vec<char> = b.key.chars().rev().take(4).collect();
                    t.reverse();
                    t.into_iter().collect()
                };
                eprintln!(
                    "💵 ключ …{tail}: −{} [{}]",
                    metering::nano_to_usd_string(charge_i64 as i128),
                    if price_model.is_empty() {
                        "?"
                    } else {
                        price_model
                    }
                );
            }
        }
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
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
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
            let inner = std::mem::replace(&mut self.inner, Box::pin(futures_util::stream::empty()));
            let ctx = self.ctx.take();
            let acc = std::mem::take(&mut self.acc);
            let sse_line = std::mem::take(&mut self.sse_line);
            let sse_drop_line = self.sse_drop_line;
            let lease_heartbeat = self.lease_heartbeat.take();
            handle.spawn(async move {
                use futures_util::StreamExt;
                let mut meter = TeeMeter {
                    inner,
                    acc,
                    sse_line,
                    sse_drop_line,
                    lease_heartbeat,
                    ctx,
                };
                while let Some(frame) = meter.inner.next().await {
                    let Ok(chunk) = frame else { break };
                    let is_sse = meter.ctx.as_ref().is_some_and(|ctx| ctx.is_sse);
                    if is_sse {
                        meter.retain_sse_usage(&chunk);
                    } else if meter.acc.len().saturating_add(chunk.len()) <= JSON_ACC_CAP {
                        meter.acc.extend_from_slice(&chunk);
                    }
                }
                meter.finalize();
            });
        } else {
            // Only possible outside the HTTP runtime (for example during abnormal teardown).
            self.finalize();
        }
    }
}
