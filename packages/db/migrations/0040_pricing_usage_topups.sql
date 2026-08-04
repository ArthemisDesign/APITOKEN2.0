CREATE TABLE "pricing_usage_topups" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"ledger_entry_id" bigint NOT NULL,
	"ref" text,
	"source" text NOT NULL,
	"amount_nano" bigint NOT NULL,
	"occurred_at" timestamp with time zone NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_usage_topups_amount_check" CHECK ("pricing_usage_topups"."amount_nano" > 0),
	CONSTRAINT "pricing_usage_topups_source_check" CHECK ("pricing_usage_topups"."source" IN ('payment', 'bonus', 'manual'))
);
--> statement-breakpoint
ALTER TABLE "pricing_usage_cursors" ADD COLUMN "topups_scanned_through_ledger_id" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_topups" ADD CONSTRAINT "pricing_usage_topups_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_usage_topups_engine_ledger_uidx" ON "pricing_usage_topups" USING btree ("engine_account_id","ledger_entry_id");--> statement-breakpoint
CREATE INDEX "pricing_usage_topups_user_time_idx" ON "pricing_usage_topups" USING btree ("user_id","occurred_at");