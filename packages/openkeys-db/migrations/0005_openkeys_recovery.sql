-- 0003 and 0004 had timestamps older than 0002, so Drizzle skipped them on
-- databases that had already applied 0002. These statements are deliberately
-- replay-safe: a fresh database applies 0003/0004 first, while production gets
-- the missing runtime schema here.
ALTER TABLE "openkeys_keys" ADD COLUMN IF NOT EXISTS "removed_by" text;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN IF NOT EXISTS "removal_reason" text;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN IF NOT EXISTS "secret_version" integer DEFAULT 1 NOT NULL;--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD COLUMN IF NOT EXISTS "secret_key_id" text DEFAULT 'legacy' NOT NULL;--> statement-breakpoint
UPDATE "openkeys_keys" SET "status" = 'disabled', "disabled_at" = COALESCE("disabled_at", "removed_at"), "secret_ciphertext" = NULL, "secret_nonce" = NULL, "removed_by" = COALESCE("removed_by", 'legacy'), "removal_reason" = COALESCE("removal_reason", 'legacy removal') WHERE "removed_at" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX IF NOT EXISTS "openkeys_keys_engine_account_id_key" ON "openkeys_keys" USING btree ("engine_account_id");--> statement-breakpoint
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
