//! Тарификация ответа: tee-стрим поверх тела апстрима.
//!
//! Инвариант прозрачности не нарушается: клиент получает байты апстрима БАЙТ-В-БАЙТ и без
//! задержки — мы лишь параллельно копим копию тела. Когда стрим завершился (или оборвался),
//! парсим `usage` (SSE — из накопленного текста, не-стрим — из полного JSON), считаем стоимость
//! через `metering` и списываем с баланса ключа. Метерим ТОЛЬКО успешный ответ (см. proxy.rs).

use bytes::Bytes;
use futures_util::Stream;
use registry::Billing;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Всё, что нужно, чтобы старифицировать один ответ на завершении стрима.
pub struct MeterCtx {
    pub billing: Arc<Billing>,
    pub key: String,
    pub model: String,
    pub mult_bp: i64,
    pub is_sse: bool,
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
            return; // нечего тарифицировать (напр. не-messages ответ)
        }
        let charge = metering::cost_with_multiplier(&usage, &ctx.model, ctx.mult_bp);
        let charge_i64 = charge.clamp(0, i64::MAX as i128) as i64;
        let newbal = ctx.billing.deduct(&ctx.key, charge_i64);
        // лог без утечки ключа: только последние 4 символа
        let tail: String = {
            let s = ctx.key.as_str();
            s[s.len().saturating_sub(4)..].to_string()
        };
        eprintln!(
            "💵 ключ …{tail}: −{} [{}] → баланс {}",
            metering::nano_to_usd_string(charge),
            if ctx.model.is_empty() { "?" } else { &ctx.model },
            newbal.map(|b| metering::nano_to_usd_string(b as i128)).unwrap_or_else(|| "?".into()),
        );
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
