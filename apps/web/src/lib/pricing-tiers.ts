// Плоская B2C-модель: единая скидка 50% от официальных цен провайдера на каждый запрос.
// Тарифных тиров нет — ставка одинакова для всех аккаунтов и любой суммы пополнения.
export const B2C_DISCOUNT_PERCENT = 50;

/** Доля официальной стоимости, которую платит клиент (0.5 = половина официальной цены). */
export const B2C_PAYMENT_RATIO = (100 - B2C_DISCOUNT_PERCENT) / 100;

/** Множитель ценности баланса: $1 баланса покрывает $2 официального использования API. */
export const B2C_VALUE_MULTIPLIER = 1 / B2C_PAYMENT_RATIO;

/** Сколько официального использования API покрывает пополнение на `payUsd` долларов. */
export function officialUsageForTopup(payUsd: number): number {
  return payUsd * B2C_VALUE_MULTIPLIER;
}

// Алиас для дашборда (production-шелл импортирует старое имя константы).
export const FLAT_DISCOUNT_PERCENT = B2C_DISCOUNT_PERCENT;
