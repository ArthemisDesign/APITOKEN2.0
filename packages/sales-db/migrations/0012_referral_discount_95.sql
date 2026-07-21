-- Потолок скидки сейлза 90%→95% (9500 bps) для партнёров, инвайтов и одноразовых скидочных ссылок.
-- Только расширение диапазона CHECK — backward-compatible. Expand-only.
ALTER TABLE "partners" DROP CONSTRAINT IF EXISTS "partners_referral_discount_check";--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_referral_discount_check" CHECK ("referral_discount_bps" BETWEEN 0 AND 9500);--> statement-breakpoint
ALTER TABLE "partner_invites" DROP CONSTRAINT IF EXISTS "partner_invites_referral_discount_check";--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_referral_discount_check" CHECK ("referral_discount_bps" BETWEEN 0 AND 9500);--> statement-breakpoint
ALTER TABLE "partner_discount_links" DROP CONSTRAINT IF EXISTS "partner_discount_links_discount_check";--> statement-breakpoint
ALTER TABLE "partner_discount_links" ADD CONSTRAINT "partner_discount_links_discount_check" CHECK ("discount_bps" BETWEEN 0 AND 9500);
