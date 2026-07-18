CREATE TYPE "public"."partner_status" AS ENUM('active', 'suspended', 'pending');--> statement-breakpoint
CREATE TYPE "public"."partner_auth_token_purpose" AS ENUM('verify_email', 'reset_password');--> statement-breakpoint
CREATE TYPE "public"."partner_email_status" AS ENUM('pending', 'sending', 'sent', 'failed');--> statement-breakpoint
CREATE TYPE "public"."payout_status" AS ENUM('requested', 'approved', 'paid', 'rejected');--> statement-breakpoint
CREATE TABLE "partners" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"email" text NOT NULL,
	"display_name" text,
	"password_hash" text NOT NULL,
	"status" "partner_status" DEFAULT 'pending' NOT NULL,
	"email_verified" boolean DEFAULT false NOT NULL,
	"referral_code" text NOT NULL,
	"parent_partner_id" uuid,
	"commission_bps" integer DEFAULT 1000 NOT NULL,
	"sub_commission_bps" integer DEFAULT 1000 NOT NULL,
	"payout_method" text,
	"payout_details" jsonb,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "partners_commission_bps_check" CHECK ("partners"."commission_bps" BETWEEN 0 AND 10000),
	CONSTRAINT "partners_sub_commission_bps_check" CHECK ("partners"."sub_commission_bps" BETWEEN 0 AND 10000)
);
--> statement-breakpoint
CREATE TABLE "partner_sessions" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"token_hash" text NOT NULL,
	"expires_at" timestamp with time zone NOT NULL,
	"last_seen_at" timestamp with time zone DEFAULT now() NOT NULL,
	"revoked_at" timestamp with time zone,
	"user_agent" text,
	"ip_address" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "partner_auth_tokens" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"purpose" "partner_auth_token_purpose" NOT NULL,
	"token_hash" text NOT NULL,
	"expires_at" timestamp with time zone NOT NULL,
	"used_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "partner_rate_limits" (
	"key_hash" text PRIMARY KEY NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"window_started_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "partner_email_outbox" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"recipient" text NOT NULL,
	"template" text NOT NULL,
	"payload" jsonb DEFAULT '{}' NOT NULL,
	"status" "partner_email_status" DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"sent_at" timestamp with time zone,
	"last_error" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "partner_invites" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"code" text NOT NULL,
	"commission_bps" integer,
	"expires_at" timestamp with time zone,
	"consumed_at" timestamp with time zone,
	"consumed_by_partner_id" uuid,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "partner_invites_commission_bps_check" CHECK ("partner_invites"."commission_bps" IS NULL OR "partner_invites"."commission_bps" BETWEEN 0 AND 10000)
);
--> statement-breakpoint
CREATE TABLE "referred_users" (
	"commerce_user_id" uuid PRIMARY KEY NOT NULL,
	"partner_id" uuid NOT NULL,
	"referral_code" text,
	"attributed_at" timestamp with time zone DEFAULT now() NOT NULL,
	"source_attribution_id" bigint
);
--> statement-breakpoint
CREATE TABLE "sync_cursors" (
	"feed" text PRIMARY KEY NOT NULL,
	"last_id" bigint DEFAULT 0 NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "sync_cursors_feed_check" CHECK ("sync_cursors"."feed" IN ('attributions', 'usage_events', 'topups'))
);
--> statement-breakpoint
CREATE TABLE "partner_usage_events" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"commerce_event_id" bigint NOT NULL,
	"commerce_user_id" uuid NOT NULL,
	"partner_id" uuid NOT NULL,
	"amount_nano" bigint NOT NULL,
	"occurred_at" timestamp with time zone NOT NULL,
	"imported_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "partner_usage_events_amount_check" CHECK ("partner_usage_events"."amount_nano" > 0)
);
--> statement-breakpoint
CREATE TABLE "referred_topups" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"commerce_payment_id" text NOT NULL,
	"commerce_user_id" uuid NOT NULL,
	"partner_id" uuid NOT NULL,
	"amount_nano" bigint NOT NULL,
	"paid_at" timestamp with time zone NOT NULL,
	CONSTRAINT "referred_topups_amount_check" CHECK ("referred_topups"."amount_nano" > 0)
);
--> statement-breakpoint
CREATE TABLE "commission_entries" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"usage_event_id" bigint NOT NULL,
	"partner_id" uuid NOT NULL,
	"level" integer NOT NULL,
	"applied_bps" integer NOT NULL,
	"amount_nano" bigint NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "commission_entries_level_check" CHECK ("commission_entries"."level" BETWEEN 0 AND 10),
	CONSTRAINT "commission_entries_applied_bps_check" CHECK ("commission_entries"."applied_bps" BETWEEN 0 AND 10000),
	CONSTRAINT "commission_entries_amount_check" CHECK ("commission_entries"."amount_nano" > 0)
);
--> statement-breakpoint
CREATE TABLE "payouts" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"partner_id" uuid NOT NULL,
	"amount_nano" bigint NOT NULL,
	"status" "payout_status" DEFAULT 'requested' NOT NULL,
	"method" text NOT NULL,
	"details" jsonb DEFAULT '{}' NOT NULL,
	"requested_at" timestamp with time zone DEFAULT now() NOT NULL,
	"decided_at" timestamp with time zone,
	"paid_at" timestamp with time zone,
	"admin_note" text,
	CONSTRAINT "payouts_amount_check" CHECK ("payouts"."amount_nano" > 0)
);
--> statement-breakpoint
CREATE TABLE "sales_audit_log" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"actor_type" text NOT NULL,
	"actor_id" text,
	"action" text NOT NULL,
	"target_type" text NOT NULL,
	"target_id" text NOT NULL,
	"metadata" jsonb DEFAULT '{}' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_parent_partner_id_partners_id_fk" FOREIGN KEY ("parent_partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_sessions" ADD CONSTRAINT "partner_sessions_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_auth_tokens" ADD CONSTRAINT "partner_auth_tokens_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_email_outbox" ADD CONSTRAINT "partner_email_outbox_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_consumed_by_partner_id_partners_id_fk" FOREIGN KEY ("consumed_by_partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "referred_users" ADD CONSTRAINT "referred_users_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD CONSTRAINT "partner_usage_events_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "referred_topups" ADD CONSTRAINT "referred_topups_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "commission_entries" ADD CONSTRAINT "commission_entries_usage_event_id_partner_usage_events_id_fk" FOREIGN KEY ("usage_event_id") REFERENCES "public"."partner_usage_events"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "commission_entries" ADD CONSTRAINT "commission_entries_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "payouts" ADD CONSTRAINT "payouts_partner_id_partners_id_fk" FOREIGN KEY ("partner_id") REFERENCES "public"."partners"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "partners_email_lower_uidx" ON "partners" USING btree (lower("email"));--> statement-breakpoint
CREATE UNIQUE INDEX "partners_referral_code_uidx" ON "partners" USING btree ("referral_code");--> statement-breakpoint
CREATE INDEX "partners_parent_idx" ON "partners" USING btree ("parent_partner_id");--> statement-breakpoint
CREATE UNIQUE INDEX "partner_sessions_token_hash_uidx" ON "partner_sessions" USING btree ("token_hash");--> statement-breakpoint
CREATE INDEX "partner_sessions_partner_idx" ON "partner_sessions" USING btree ("partner_id","created_at");--> statement-breakpoint
CREATE INDEX "partner_sessions_expiry_idx" ON "partner_sessions" USING btree ("expires_at");--> statement-breakpoint
CREATE UNIQUE INDEX "partner_auth_tokens_token_hash_uidx" ON "partner_auth_tokens" USING btree ("token_hash");--> statement-breakpoint
CREATE INDEX "partner_auth_tokens_partner_purpose_idx" ON "partner_auth_tokens" USING btree ("partner_id","purpose","created_at");--> statement-breakpoint
CREATE INDEX "partner_email_outbox_claim_idx" ON "partner_email_outbox" USING btree ("status","next_attempt_at");--> statement-breakpoint
CREATE UNIQUE INDEX "partner_invites_code_uidx" ON "partner_invites" USING btree ("code");--> statement-breakpoint
CREATE INDEX "partner_invites_partner_idx" ON "partner_invites" USING btree ("partner_id","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "referred_users_source_attribution_uidx" ON "referred_users" USING btree ("source_attribution_id") WHERE "referred_users"."source_attribution_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "referred_users_partner_idx" ON "referred_users" USING btree ("partner_id","attributed_at");--> statement-breakpoint
CREATE UNIQUE INDEX "partner_usage_events_commerce_event_uidx" ON "partner_usage_events" USING btree ("commerce_event_id");--> statement-breakpoint
CREATE INDEX "partner_usage_events_partner_time_idx" ON "partner_usage_events" USING btree ("partner_id","occurred_at");--> statement-breakpoint
CREATE INDEX "partner_usage_events_user_idx" ON "partner_usage_events" USING btree ("commerce_user_id");--> statement-breakpoint
CREATE UNIQUE INDEX "referred_topups_commerce_payment_uidx" ON "referred_topups" USING btree ("commerce_payment_id");--> statement-breakpoint
CREATE INDEX "referred_topups_partner_idx" ON "referred_topups" USING btree ("partner_id","paid_at");--> statement-breakpoint
CREATE UNIQUE INDEX "commission_entries_event_partner_uidx" ON "commission_entries" USING btree ("usage_event_id","partner_id");--> statement-breakpoint
CREATE INDEX "commission_entries_partner_time_idx" ON "commission_entries" USING btree ("partner_id","created_at");--> statement-breakpoint
CREATE INDEX "payouts_partner_idx" ON "payouts" USING btree ("partner_id","requested_at");--> statement-breakpoint
CREATE INDEX "payouts_status_idx" ON "payouts" USING btree ("status","requested_at");--> statement-breakpoint
CREATE INDEX "sales_audit_log_target_idx" ON "sales_audit_log" USING btree ("target_type","target_id","created_at");
