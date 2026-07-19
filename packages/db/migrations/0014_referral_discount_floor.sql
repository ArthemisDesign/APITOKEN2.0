-- Реф от сейлза остаётся b2c и идёт по обычным тир-правилам; скидка сейлза — это «пол»:
-- эффективный mult = min(тир-mult, 10000 - referral_floor_bps). floor=0 → no-op (тир как есть).
-- Скидка ≤ 90% (floor ≤ 9000). Разграничение b2c/b2b сохраняется (b2b business-invite не трогаем).
ALTER TABLE "customer_profiles" ADD COLUMN "referral_floor_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD CONSTRAINT "customer_profiles_referral_floor_check" CHECK ("referral_floor_bps" BETWEEN 0 AND 9000);
