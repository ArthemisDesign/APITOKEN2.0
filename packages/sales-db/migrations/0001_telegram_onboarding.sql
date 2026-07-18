-- Telegram-онбординг: identity партнёра = telegram_id; email/password становятся legacy.
-- Инвайт привязывается к telegram-юзернейму; NULL partner_id = корневой инвайт из админки.
ALTER TABLE "partners" ALTER COLUMN "email" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ALTER COLUMN "password_hash" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "telegram_id" bigint;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "telegram_username" text;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "telegram_photo_url" text;--> statement-breakpoint
DROP INDEX IF EXISTS "partners_email_lower_uidx";--> statement-breakpoint
CREATE UNIQUE INDEX "partners_email_lower_uidx" ON "partners" USING btree (lower("email")) WHERE "partners"."email" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "partners_telegram_id_uidx" ON "partners" USING btree ("telegram_id") WHERE "partners"."telegram_id" IS NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ALTER COLUMN "partner_id" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "telegram_username" text;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "sub_commission_bps" integer;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_sub_commission_bps_check" CHECK ("partner_invites"."sub_commission_bps" IS NULL OR "partner_invites"."sub_commission_bps" BETWEEN 0 AND 10000);
