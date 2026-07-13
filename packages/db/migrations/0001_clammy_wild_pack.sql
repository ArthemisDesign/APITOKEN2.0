CREATE TYPE "public"."checkout_status" AS ENUM('creating', 'pending', 'paid', 'canceled', 'refunded', 'failed');--> statement-breakpoint
CREATE TABLE "checkout_sessions" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"provider" text NOT NULL,
	"amount_usd" bigint NOT NULL,
	"amount_nano" bigint NOT NULL,
	"provider_payment_id" text,
	"checkout_url" text,
	"status" "checkout_status" DEFAULT 'creating' NOT NULL,
	"provider_state" jsonb DEFAULT '{}'::jsonb NOT NULL,
	"expires_at" timestamp with time zone,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "checkout_sessions_amount_usd_check" CHECK ("checkout_sessions"."amount_usd" > 0),
	CONSTRAINT "checkout_sessions_amount_exact_check" CHECK ("checkout_sessions"."amount_nano" = "checkout_sessions"."amount_usd" * 1000000000)
);
--> statement-breakpoint
ALTER TABLE "payments" ADD COLUMN "checkout_id" uuid NOT NULL;--> statement-breakpoint
ALTER TABLE "checkout_sessions" ADD CONSTRAINT "checkout_sessions_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "checkout_sessions_user_created_idx" ON "checkout_sessions" USING btree ("user_id","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "checkout_sessions_provider_payment_uidx" ON "checkout_sessions" USING btree ("provider","provider_payment_id") WHERE "checkout_sessions"."provider_payment_id" IS NOT NULL;--> statement-breakpoint
ALTER TABLE "payments" ADD CONSTRAINT "payments_checkout_id_checkout_sessions_id_fk" FOREIGN KEY ("checkout_id") REFERENCES "public"."checkout_sessions"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "payments_checkout_uidx" ON "payments" USING btree ("checkout_id");