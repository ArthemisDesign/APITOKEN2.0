-- Право «давать скидку рефам» выдаётся партнёру в админке (по умолчанию нет, как промо). Только
-- при включённом праве referral_discount_bps применяется как «пол» цены рефа (иначе floor=0).
-- Право может каскадиться: партнёр с правом включает его суб-сейлзам, которых онбордит.
ALTER TABLE "partners" ADD COLUMN "referral_discount_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "referral_discount_enabled" boolean DEFAULT false NOT NULL;
