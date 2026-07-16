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

/// Копим до 32 МиБ тела для парсинга usage. Реальный ответ (даже 128k output) — сильно меньше;
/// потолок лишь страхует от аномально большого потока (тогда возможен недосчёт хвоста — не крэш).
const ACC_CAP: usize = 32 * 1024 * 1024;

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

fn apply_us_inference_premium(real: i128, usage: &metering::Usage) -> i128 {
    // Data-residency premium applies to token categories, not fixed-price web-search requests.
    let web_search = (usage.web_search_requests as i128) * metering::WEB_SEARCH_NANO;
    metering::apply_multiplier(real - web_search, 11_000) + web_search
}

/// Стрим-обёртка: пропускает чанки клиенту и копит их копию; на конце — списывает.
pub struct TeeMeter {
    inner: ByteStream,
    acc: Vec<u8>,
    ctx: Option<MeterCtx>, // берётся ровно один раз (finalize идемпотентен)
}

impl TeeMeter {
    pub fn new(inner: ByteStream, ctx: MeterCtx) -> Self {
        TeeMeter { inner, acc: Vec::new(), ctx: Some(ctx) }
    }

    fn finalize(&mut self) {
        let ctx = match self.ctx.take() { Some(c) => c, None => return };
        // стрим завершён/оборван → освободить слот конкуррентности персоны (парен с mark_used).
        // Делаем ПЕРВЫМ и безусловно, даже если usage пустой (иначе in-flight подтёк бы).
        ctx.pool.end_stream(&ctx.email);
        if let Some((billing, lease_id)) = &ctx.capacity {
            billing.release_capacity(lease_id);
        }
        let (usage, served_model, incomplete_non_sse, us_inference) = if ctx.is_sse {
            let s = String::from_utf8_lossy(&self.acc);
            // ошибка ВНУТРИ стрима после 200 (overloaded посреди генерации) — HTTP-код её не отражал,
            // ротация уже невозможна; логируем, чтобы не была «тихой» (клиент получил её байт-в-байт).
            if metering::sse_has_error(&s) {
                eprintln!("⚠ SSE-error после 200 на {} — стрим нёс error-евент", ctx.email);
            }
            (
                metering::usage_from_sse(&s),
                metering::model_from_sse(&s),
                false,
                sse_has_us_inference(&s),
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
        let base_real = if usage.is_zero() {
            0
        } else {
            metering::cost_nanodollars(&usage, &metering::model_prices_at(price_model, now_unix))
        };
        let real = if us_inference && base_real > 0 {
            apply_us_inference_premium(base_real, &usage)
        } else {
            base_real
        };

        // расход в пул только когда он реально был (0 калибровку не двигает)
        if real > 0 {
            ctx.pool.record_spend(&ctx.email, real);
        }

        // Резерв метерного ключа закрываем ВСЕГДА: actual = charge (0 при usage=0 → полный возврат
        // hold). settle возвращает hold и списывает actual → итог по паре reserve→settle = −actual.
        if let Some(b) = ctx.bill {
            // Неполный non-SSE JSON означает, что клиент оборвал чтение до хвостового usage. Возврат hold
            // превратил бы уже полученный контент в бесплатный; локально безопаснее сохранить весь резерв.
            // AUDIT-TODO(C8): продолжать дренировать upstream после downstream-disconnect и settle по usage.
            let computed_charge = if incomplete_non_sse {
                eprintln!("⚠ неполный non-SSE ответ: сохраняем полный billing hold");
                b.hold.max(0) as i128
            } else if real > 0 {
                metering::apply_multiplier(real, b.mult_bp)
            } else {
                0
            };
            // Никогда не списываем больше атомарно зарезервированного потолка: иначе параллельные запросы
            // могут совместно увести общий баланс аккаунта ниже нуля из-за неучтённого preflight-overhead.
            // AUDIT-TODO(C53): резервировать по count_tokens точного post-injection запроса с tool overhead.
            let hold_cap = b.hold.max(0) as i128;
            if computed_charge > hold_cap {
                eprintln!("⚠ billing charge превысил hold: charge_nano={computed_charge} hold_nano={hold_cap}; clamp");
            }
            let charge_i64 = computed_charge.clamp(0, hold_cap) as i64;
            // AUDIT-TODO(C55): учитывать inference_geo premium и в preflight-резерве, чтобы hold был верхней границей.
            // Разбивка токенов/модели для клиентского дашборда — пишется рядом с charge (аналитика).
            // Только при авторитетном usage; C8-preserved hold не изображаем как токеновое событие.
            let usage_event = if charge_i64 > 0 && real > 0 {
                Some(registry::UsageEventInput {
                    model: price_model.to_string(),
                    input_tokens: usage.input_tokens as i64,
                    output_tokens: usage.output_tokens as i64,
                    cache_read_tokens: usage.cache_read_tokens as i64,
                    cache_write_5m_tokens: usage.cache_write_5m_tokens as i64,
                    cache_write_1h_tokens: usage.cache_write_1h_tokens as i64,
                    web_search_requests: usage.web_search_requests as i64,
                    real_nano: real.clamp(0, i64::MAX as i128) as i64,
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
                    if price_model.is_empty() { "?" } else { price_model }
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
                if me.acc.len().saturating_add(chunk.len()) <= ACC_CAP {
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
        self.finalize();
    }
}
