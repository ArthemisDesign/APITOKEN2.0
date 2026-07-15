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
    pub request_id: Option<String>,
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
}

/// Копим до 32 МиБ тела для парсинга usage. Реальный ответ (даже 128k output) — сильно меньше;
/// потолок лишь страхует от аномально большого потока (тогда возможен недосчёт хвоста — не крэш).
const ACC_CAP: usize = 32 * 1024 * 1024;

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
        let (usage, served_model) = if ctx.is_sse {
            let s = String::from_utf8_lossy(&self.acc);
            // ошибка ВНУТРИ стрима после 200 (overloaded посреди генерации) — HTTP-код её не отражал,
            // ротация уже невозможна; логируем, чтобы не была «тихой» (клиент получил её байт-в-байт).
            if metering::sse_has_error(&s) {
                eprintln!("⚠ SSE-error после 200 на {} — стрим нёс error-евент", ctx.email);
            }
            (metering::usage_from_sse(&s), metering::model_from_sse(&s))
        } else {
            (metering::usage_from_response_json(&self.acc), metering::model_from_response_json(&self.acc))
        };
        // Тарифицируем по МОДЕЛИ ИЗ ОТВЕТА (авторитетный сервёный id): клиент мог прислать алиас или
        // `-latest`, апстрим резолвит в конкретную датированную модель — считать надо по НЕЙ. Фолбэк —
        // модель запроса (ctx.model), если ответ модель не отдал.
        let price_model = served_model.as_deref().filter(|m| !m.is_empty()).unwrap_or(&ctx.model);
        // Реальная стоимость (×1.0, до наценки). 0, если usage нет — count_tokens/models/любой 200
        // без usage/обрыв до message_start. ВАЖНО: даже при 0 нельзя просто выйти — иначе hold висел
        // бы в reserved_nano до рестарта (тихая утечка баланса клиента на штатном count_tokens).
        let real = if usage.is_zero() { 0 } else {
            metering::cost_nanodollars(&usage, &metering::model_prices(price_model))
        };

        // расход в пул только когда он реально был (0 калибровку не двигает)
        if real > 0 { ctx.pool.record_spend(&ctx.email, real); }

        // Резерв метерного ключа закрываем ВСЕГДА: actual = charge (0 при usage=0 → полный возврат
        // hold). settle возвращает hold и списывает actual → итог по паре reserve→settle = −actual.
        if let Some(b) = ctx.bill {
            let charge = if real > 0 { metering::apply_multiplier(real, b.mult_bp) } else { 0 };
            let charge_i64 = charge.clamp(0, i64::MAX as i128) as i64;
            // Разбивка токенов/модели для клиентского дашборда — пишется рядом с charge (аналитика).
            // Только при реальном списании; `real` = стоимость по официальным ценам (×1.0).
            let usage_event = if charge_i64 > 0 { Some(registry::UsageEventInput {
                model: price_model.to_string(),
                input_tokens: usage.input_tokens as i64,
                output_tokens: usage.output_tokens as i64,
                cache_read_tokens: usage.cache_read_tokens as i64,
                cache_write_5m_tokens: usage.cache_write_5m_tokens as i64,
                cache_write_1h_tokens: usage.cache_write_1h_tokens as i64,
                web_search_requests: usage.web_search_requests as i64,
                real_nano: real.clamp(0, i64::MAX as i128) as i64,
            }) } else { None };
            // finalize СИНХРОНЕН (Stream::poll / Drop) → шлём списание АСИНХРОННО через актор
            // (settle_detached не блокирует). Гарантия: осиротевшее при краше вернёт reconcile.
            b.billing.settle_detached(&b.account_id, &b.key, b.hold, charge_i64, b.request_id.as_deref(), usage_event);
            if charge_i64 > 0 {
                // хвост ключа для лога — по символам (не байтами: срез не на границе char паникует)
                let tail: String = {
                    let mut t: Vec<char> = b.key.chars().rev().take(4).collect();
                    t.reverse();
                    t.into_iter().collect()
                };
                eprintln!("💵 ключ …{tail}: −{} [{}]",
                    metering::nano_to_usd_string(charge),
                    if price_model.is_empty() { "?" } else { price_model });
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
