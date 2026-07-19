-- Заявки в партнёрскую программу: вход через Telegram без аккаунта/инвайта → pending-заявка,
-- админ approve (создаёт партнёра) или reject.
CREATE TYPE "public"."partner_application_status" AS ENUM('pending', 'approved', 'rejected');--> statement-breakpoint
CREATE TABLE "partner_applications" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"telegram_id" bigint NOT NULL,
	"telegram_username" text,
	"display_name" text,
	"telegram_photo_url" text,
	"note" text,
	"status" "partner_application_status" DEFAULT 'pending' NOT NULL,
	"admin_note" text,
	"created_partner_id" uuid,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"decided_at" timestamp with time zone
);
--> statement-breakpoint
ALTER TABLE "partner_applications" ADD CONSTRAINT "partner_applications_created_partner_id_partners_id_fk" FOREIGN KEY ("created_partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "partner_applications_pending_tg_uidx" ON "partner_applications" USING btree ("telegram_id") WHERE "partner_applications"."status" = 'pending';--> statement-breakpoint
CREATE INDEX "partner_applications_status_idx" ON "partner_applications" USING btree ("status","created_at");
