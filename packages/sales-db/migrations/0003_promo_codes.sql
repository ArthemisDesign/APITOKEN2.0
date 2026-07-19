-- Промокоды партнёров: право на создание (по умолчанию выключено) + лимиты, и таблица кодов.
CREATE TYPE "public"."promo_code_status" AS ENUM('active', 'redeemed', 'disabled');--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "promo_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "promo_max_value_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "promo_max_count" integer DEFAULT 0 NOT NULL;--> statement-breakpoint
CREATE TABLE "promo_codes" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"code" text NOT NULL,
	"value_nano" bigint NOT NULL,
	"status" "promo_code_status" DEFAULT 'active' NOT NULL,
	"redeemed_by_commerce_user_id" uuid,
	"redeemed_at" timestamp with time zone,
	"redemption_ref" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "promo_codes_value_check" CHECK ("promo_codes"."value_nano" > 0)
);
--> statement-breakpoint
ALTER TABLE "promo_codes" ADD CONSTRAINT "promo_codes_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "promo_codes_code_uidx" ON "promo_codes" USING btree (upper("code"));--> statement-breakpoint
CREATE UNIQUE INDEX "promo_codes_redemption_ref_uidx" ON "promo_codes" USING btree ("redemption_ref") WHERE "promo_codes"."redemption_ref" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "promo_codes_redeemed_by_uidx" ON "promo_codes" USING btree ("redeemed_by_commerce_user_id") WHERE "promo_codes"."redeemed_by_commerce_user_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "promo_codes_partner_idx" ON "promo_codes" USING btree ("partner_id","created_at");
