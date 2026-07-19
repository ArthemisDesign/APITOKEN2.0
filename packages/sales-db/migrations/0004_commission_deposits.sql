-- Реферальная комиссия начисляется ТОЛЬКО с реальных депозитов (referred_topups = оплаченные
-- платежи commerce). Бесплатные деньги (welcome-бонус $4, промокоды, любые будущие бонусы) не
-- создают строку payments → в фид topups не попадают → комиссию не генерируют НИКОГДА.
-- commission_entries перепривязывается со списаний (usage_event) на депозит (topup).
ALTER TABLE "commission_entries" ADD COLUMN "topup_id" bigint REFERENCES "referred_topups"("id") ON DELETE RESTRICT;--> statement-breakpoint
ALTER TABLE "commission_entries" ALTER COLUMN "usage_event_id" DROP NOT NULL;--> statement-breakpoint
-- Строгий инвариант: ровно один источник у строки комиссии. Путь списаний отключён в коде;
-- CHECK гарантирует, что новая строка не может внезапно начислиться с траты.
ALTER TABLE "commission_entries" ADD CONSTRAINT "commission_entries_one_source_check"
  CHECK ((("usage_event_id" IS NOT NULL)::int + ("topup_id" IS NOT NULL)::int) = 1);--> statement-breakpoint
DROP INDEX IF EXISTS "commission_entries_event_partner_uidx";--> statement-breakpoint
CREATE UNIQUE INDEX "commission_entries_usage_partner_uidx" ON "commission_entries" ("usage_event_id","partner_id") WHERE "usage_event_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "commission_entries_topup_partner_uidx" ON "commission_entries" ("topup_id","partner_id") WHERE "topup_id" IS NOT NULL;--> statement-breakpoint
-- Онбординг сейлза задаёт прямо на инвайте: доступ к промокодам (кол-во и макс номинал нашего
-- баланса в nano) и скидку рефа (B2B), которую сейлз даёт своим пользователям. Скидка ≤ 90%.
ALTER TABLE "partner_invites" ADD COLUMN "promo_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "promo_max_value_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "promo_max_count" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "referral_discount_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_referral_discount_check" CHECK ("referral_discount_bps" BETWEEN 0 AND 9000);--> statement-breakpoint
-- Скидка рефа хранится на партнёре (авторитет): все его рефы получают B2B-цену с этой скидкой.
ALTER TABLE "partners" ADD COLUMN "referral_discount_bps" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_referral_discount_check" CHECK ("referral_discount_bps" BETWEEN 0 AND 9000);
