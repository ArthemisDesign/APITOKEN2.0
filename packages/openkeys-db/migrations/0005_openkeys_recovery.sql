-- Recovery migration: 0003 and 0004 were assigned timestamps older than 0002 in
-- meta/_journal.json, so Drizzle legitimately skipped them on databases that had
-- already applied 0002. Keep this append-only migration newer than every prior entry.
DO $recovery$
BEGIN
IF to_regclass('public.openkeys_issuance_jobs') IS NULL THEN
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_face_value_positive" CHECK ("face_value_nano" > 0) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_mult_bp_range" CHECK ("mult_bp" BETWEEN 1 AND 10000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_quantity_range" CHECK ("quantity" BETWEEN 1 AND 100) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_label_length" CHECK ("label" IS NULL OR char_length("label") <= 200) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_note_length" CHECK ("note" IS NULL OR char_length("note") <= 2000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" ADD CONSTRAINT "openkeys_batches_created_by_length" CHECK (char_length("created_by") BETWEEN 1 AND 128) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_face_value_positive" CHECK ("face_value_nano" > 0) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_mult_bp_range" CHECK ("mult_bp" BETWEEN 1 AND 10000) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_view_token_shape" CHECK ("view_token" ~ '^[A-Za-z0-9_-]{22}$') NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_secret_pair" CHECK (("secret_ciphertext" IS NULL) = ("secret_nonce" IS NULL)) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_delivered_secret_cleared" CHECK ("delivered_at" IS NULL OR ("secret_ciphertext" IS NULL AND "secret_nonce" IS NULL)) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_disabled_timestamp" CHECK ("status" <> 'disabled' OR "disabled_at" IS NOT NULL) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "removed_by" text;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "removal_reason" text;--> statement-breakpoint
UPDATE "openkeys_keys" SET "status" = 'disabled', "disabled_at" = COALESCE("disabled_at", "removed_at"), "secret_ciphertext" = NULL, "secret_nonce" = NULL, "removed_by" = COALESCE("removed_by", 'legacy'), "removal_reason" = COALESCE("removal_reason", 'legacy removal') WHERE "removed_at" IS NOT NULL;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_removed_state" CHECK ("removed_at" IS NULL OR ("status" = 'disabled' AND "disabled_at" IS NOT NULL AND "secret_ciphertext" IS NULL AND "secret_nonce" IS NULL AND "removed_by" IS NOT NULL)) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "secret_version" integer DEFAULT 1 NOT NULL;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN "secret_key_id" text DEFAULT 'legacy' NOT NULL;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_secret_version" CHECK ("secret_version" BETWEEN 1 AND 2) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_secret_key_id" CHECK (char_length("secret_key_id") BETWEEN 1 AND 32) NOT VALID;--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_face_value_positive";--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_mult_bp_range";--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_quantity_range";--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_label_length";--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_note_length";--> statement-breakpoint
ALTER TABLE "openkeys_batches" VALIDATE CONSTRAINT "openkeys_batches_created_by_length";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_face_value_positive";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_mult_bp_range";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_view_token_shape";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_secret_pair";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_secret_version";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_secret_key_id";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_delivered_secret_cleared";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_disabled_timestamp";--> statement-breakpoint
ALTER TABLE "openkeys_keys" VALIDATE CONSTRAINT "openkeys_keys_removed_state";--> statement-breakpoint
CREATE UNIQUE INDEX "openkeys_keys_engine_account_id_key" ON "openkeys_keys" USING btree ("engine_account_id");--> statement-breakpoint
END IF;
END
$recovery$;--> statement-breakpoint
DO $recovery$
BEGIN
IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'openkeys_issuance_status') THEN
  CREATE TYPE "public"."openkeys_issuance_status" AS ENUM('pending', 'account_created', 'credited', 'key_issued', 'completed', 'compensated');
END IF;
END
$recovery$;--> statement-breakpoint
CREATE TABLE IF NOT EXISTS "openkeys_issuance_jobs" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"batch_id" uuid NOT NULL,
	"item_index" integer NOT NULL,
	"status" "openkeys_issuance_status" DEFAULT 'pending' NOT NULL,
	"engine_account_id" text,
	"engine_key_id" text,
	"last_error" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "openkeys_issuance_jobs_item_index" CHECK ("openkeys_issuance_jobs"."item_index" BETWEEN 0 AND 99)
);--> statement-breakpoint
DO $recovery$
BEGIN
IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'openkeys_issuance_jobs_batch_id_openkeys_batches_id_fk') THEN
  ALTER TABLE "openkeys_issuance_jobs" ADD CONSTRAINT "openkeys_issuance_jobs_batch_id_openkeys_batches_id_fk" FOREIGN KEY ("batch_id") REFERENCES "public"."openkeys_batches"("id") ON DELETE cascade ON UPDATE no action;
END IF;
END
$recovery$;--> statement-breakpoint
CREATE UNIQUE INDEX IF NOT EXISTS "openkeys_issuance_jobs_batch_item_key" ON "openkeys_issuance_jobs" USING btree ("batch_id","item_index");--> statement-breakpoint
CREATE INDEX IF NOT EXISTS "openkeys_issuance_jobs_status_updated_idx" ON "openkeys_issuance_jobs" USING btree ("status","updated_at");
