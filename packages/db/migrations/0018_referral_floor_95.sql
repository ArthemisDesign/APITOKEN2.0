-- Расширяем потолок скидки сейлза 90%→95% (referral floor до 9500 bps). Только расширение диапазона —
-- backward-compatible (старые значения ≤9000 остаются валидны). Expand-only.
ALTER TABLE "customer_profiles" DROP CONSTRAINT IF EXISTS "customer_profiles_referral_floor_check";--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD CONSTRAINT "customer_profiles_referral_floor_check" CHECK ("referral_floor_bps" BETWEEN 0 AND 9500);
