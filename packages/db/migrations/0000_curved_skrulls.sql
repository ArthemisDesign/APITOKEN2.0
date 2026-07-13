CREATE TYPE "public"."api_key_status" AS ENUM('active', 'disabled');--> statement-breakpoint
CREATE TYPE "public"."engine_account_status" AS ENUM('pending', 'active', 'error', 'disabled');--> statement-breakpoint
CREATE TYPE "public"."engine_credit_status" AS ENUM('pending', 'processing', 'retry', 'confirmed', 'dead');--> statement-breakpoint
CREATE TYPE "public"."payment_status" AS ENUM('pending', 'paid', 'failed', 'refunded', 'disputed');--> statement-breakpoint
CREATE TYPE "public"."user_status" AS ENUM('active', 'disabled');--> statement-breakpoint
CREATE TYPE "public"."webhook_status" AS ENUM('received', 'processed', 'ignored', 'failed');--> statement-breakpoint
CREATE TABLE "api_keys" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"engine_key_id" text,
	"label" text,
	"key_masked" text NOT NULL,
	"status" "api_key_status" DEFAULT 'active' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "audit_log" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"actor_type" text NOT NULL,
	"actor_id" text,
	"action" text NOT NULL,
	"target_type" text NOT NULL,
	"target_id" text NOT NULL,
	"metadata" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "engine_accounts" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text,
	"mult_bp" integer DEFAULT 2000 NOT NULL,
	"status" "engine_account_status" DEFAULT 'pending' NOT NULL,
	"last_error" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_accounts_mult_bp_check" CHECK ("engine_accounts"."mult_bp" >= 0)
);
--> statement-breakpoint
CREATE TABLE "engine_credits" (
	"id" uuid PRIMARY KEY NOT NULL,
	"payment_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"amount_nano" bigint NOT NULL,
	"idempotency_ref" text NOT NULL,
	"status" "engine_credit_status" DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"engine_balance_after_nano" bigint,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_credits_amount_check" CHECK ("engine_credits"."amount_nano" > 0)
);
--> statement-breakpoint
CREATE TABLE "payments" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"provider" text NOT NULL,
	"provider_payment_id" text NOT NULL,
	"amount_minor" bigint NOT NULL,
	"currency" text NOT NULL,
	"amount_nano" bigint NOT NULL,
	"status" "payment_status" DEFAULT 'pending' NOT NULL,
	"provider_state" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"paid_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "payments_amount_minor_check" CHECK ("payments"."amount_minor" > 0),
	CONSTRAINT "payments_amount_nano_check" CHECK ("payments"."amount_nano" > 0),
	CONSTRAINT "payments_currency_check" CHECK ("payments"."currency" = upper("payments"."currency") AND length("payments"."currency") = 3)
);
--> statement-breakpoint
CREATE TABLE "users" (
	"id" uuid PRIMARY KEY NOT NULL,
	"email" text NOT NULL,
	"email_verified" boolean DEFAULT false NOT NULL,
	"password_hash" text,
	"status" "user_status" DEFAULT 'active' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "webhook_events" (
	"id" uuid PRIMARY KEY NOT NULL,
	"provider" text NOT NULL,
	"provider_event_id" text NOT NULL,
	"event_type" text NOT NULL,
	"payload" jsonb NOT NULL,
	"status" "webhook_status" DEFAULT 'received' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"last_error" text,
	"received_at" timestamp with time zone DEFAULT now() NOT NULL,
	"processed_at" timestamp with time zone
);
--> statement-breakpoint
ALTER TABLE "api_keys" ADD CONSTRAINT "api_keys_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_accounts" ADD CONSTRAINT "engine_accounts_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_credits" ADD CONSTRAINT "engine_credits_payment_id_payments_id_fk" FOREIGN KEY ("payment_id") REFERENCES "public"."payments"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "payments" ADD CONSTRAINT "payments_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "api_keys_user_idx" ON "api_keys" USING btree ("user_id","created_at");--> statement-breakpoint
CREATE INDEX "audit_log_target_idx" ON "audit_log" USING btree ("target_type","target_id","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "engine_accounts_user_uidx" ON "engine_accounts" USING btree ("user_id");--> statement-breakpoint
CREATE UNIQUE INDEX "engine_accounts_engine_id_uidx" ON "engine_accounts" USING btree ("engine_account_id") WHERE "engine_accounts"."engine_account_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "engine_credits_payment_uidx" ON "engine_credits" USING btree ("payment_id");--> statement-breakpoint
CREATE UNIQUE INDEX "engine_credits_ref_uidx" ON "engine_credits" USING btree ("idempotency_ref");--> statement-breakpoint
CREATE INDEX "engine_credits_claim_idx" ON "engine_credits" USING btree ("status","next_attempt_at");--> statement-breakpoint
CREATE UNIQUE INDEX "payments_provider_payment_uidx" ON "payments" USING btree ("provider","provider_payment_id");--> statement-breakpoint
CREATE INDEX "payments_user_created_idx" ON "payments" USING btree ("user_id","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "users_email_lower_uidx" ON "users" USING btree (lower("email"));--> statement-breakpoint
CREATE UNIQUE INDEX "webhook_events_provider_event_uidx" ON "webhook_events" USING btree ("provider","provider_event_id");--> statement-breakpoint
CREATE INDEX "webhook_events_status_idx" ON "webhook_events" USING btree ("status","received_at");