CREATE TABLE "referral_attributions" (
	"id" bigserial PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"code" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "payments" ADD COLUMN "feed_seq" bigserial NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_events" ADD COLUMN "feed_seq" bigserial NOT NULL;--> statement-breakpoint
ALTER TABLE "referral_attributions" ADD CONSTRAINT "referral_attributions_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "referral_attributions_user_uidx" ON "referral_attributions" USING btree ("user_id");--> statement-breakpoint
CREATE INDEX "referral_attributions_code_idx" ON "referral_attributions" USING btree ("code");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_usage_events_feed_seq_uidx" ON "pricing_usage_events" USING btree ("feed_seq");