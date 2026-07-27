CREATE TYPE "public"."openkeys_key_status" AS ENUM('active', 'disabled');--> statement-breakpoint
CREATE TABLE "openkeys_batches" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"label" text,
	"face_value_nano" bigint NOT NULL,
	"mult_bp" integer NOT NULL,
	"quantity" integer NOT NULL,
	"note" text,
	"created_by" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "openkeys_keys" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"batch_id" uuid NOT NULL,
	"view_token" text NOT NULL,
	"engine_account_id" text NOT NULL,
	"engine_key_id" text NOT NULL,
	"key_masked" text NOT NULL,
	"face_value_nano" bigint NOT NULL,
	"mult_bp" integer NOT NULL,
	"status" "openkeys_key_status" DEFAULT 'active' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"disabled_at" timestamp with time zone
);
--> statement-breakpoint
ALTER TABLE "openkeys_keys" ADD CONSTRAINT "openkeys_keys_batch_id_openkeys_batches_id_fk" FOREIGN KEY ("batch_id") REFERENCES "public"."openkeys_batches"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "openkeys_keys_view_token_key" ON "openkeys_keys" USING btree ("view_token");--> statement-breakpoint
CREATE UNIQUE INDEX "openkeys_keys_engine_key_id_key" ON "openkeys_keys" USING btree ("engine_key_id");--> statement-breakpoint
CREATE INDEX "openkeys_keys_batch_id_idx" ON "openkeys_keys" USING btree ("batch_id");
