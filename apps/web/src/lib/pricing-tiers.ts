// Плоская модель скидки: у КАЖДОГО клиента −50% от официальных ставок провайдеров на все
// модели, без тиров, порогов пополнений и удержаний. Тир-модель (Starter…Scale) удалена
// на web/v2 по решению продукта 2026-07-30; исторические места, где показывался прогресс
// тиров, теперь показывают только фиксированную скидку.
export const FLAT_DISCOUNT_PERCENT = 50;

/** Множитель к официальной цене: клиент платит эту долю ($0.50 за $1 официального расхода). */
export const FLAT_PRICE_MULTIPLIER = 1 - FLAT_DISCOUNT_PERCENT / 100;

export function formatWholeUsd(value: string): string {
  return `$${BigInt(value).toLocaleString("en-US")}`;
}
