//! Тарификация ответа: tee-стрим поверх тела апстрима.
//!
//! Инвариант прозрачности не нарушается: клиент получает байты апстрима БАЙТ-В-БАЙТ и без
//! задержки — мы лишь параллельно копим копию тела. Когда стрим завершился (или оборвался),
//! парсим `usage` (SSE — из накопленного текста, не-стрим — из полного JSON), считаем стоимость
//! через `metering` и списываем с баланса ключа. Метерим ТОЛЬКО успешный ответ (см. proxy.rs).

use bytes::Bytes;
use futures_util::Stream;
use pool::Pool;
use registry::Billing;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Опциональное списание с ключа клиента (только для метерных ключей).
pub struct BillCtx {
    pub billing: Arc<Billing>,
    pub key: String,
    pub mult_bp: i64,
    pub hold: i64, // зарезервированный при допуске потолок — закрываем его фактической стоимостью
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
        let usage = if ctx.is_sse {
            metering::usage_from_sse(&String::from_utf8_lossy(&self.acc))
        } else {
            metering::usage_from_response_json(&self.acc)
        };
        if usage.is_zero() {
            return; // нечего учитывать (напр. не-messages ответ)
        }
        // реальная стоимость (×1.0, до наценки) — один раз, для калибровки И для списания
        let real = metering::cost_nanodollars(&usage, &metering::model_prices(&ctx.model));

        // 1) ВСЕГДА: расход подписки → пул (калибровка ёмкости окна + живая утилизация)
        ctx.pool.record_spend(&ctx.email, real);

        // 2) ОПЦИОНАЛЬНО: закрыть резерв метерного ключа фактической стоимостью (real × наценка).
        // settle возвращает hold и списывает actual — итог по паре reserve→settle = −actual.
        if let Some(b) = ctx.bill {
            let charge = metering::apply_multiplier(real, b.mult_bp);
            let charge_i64 = charge.clamp(0, i64::MAX as i128) as i64;
            let newbal = b.billing.settle(&b.key, b.hold, charge_i64);
            // хвост ключа для лога — по символам (не байтами: срез не на границе char паникует)
            let tail: String = {
                let mut t: Vec<char> = b.key.chars().rev().take(4).collect();
                t.reverse();
                t.into_iter().collect()
            };
            eprintln!(
                "💵 ключ …{tail}: −{} [{}] → баланс {}",
                metering::nano_to_usd_string(charge),
                if ctx.model.is_empty() { "?" } else { &ctx.model },
                newbal.map(|b| metering::nano_to_usd_string(b as i128)).unwrap_or_else(|| "?".into()),
            );
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
