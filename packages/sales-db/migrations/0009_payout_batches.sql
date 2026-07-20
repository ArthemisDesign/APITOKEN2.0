-- Пакетные on-chain выплаты (USDT BEP-20). Двухфазно: prepare → (админ смотрит) → send.
-- payout_batches — один прогон; payouts обогащаются batch_id + on-chain полями (хеш/статус/ошибка).
CREATE TABLE "payout_batches" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"status" text DEFAULT 'prepared' NOT NULL,
	"hot_wallet_address" text,
	"total_nano" bigint DEFAULT 0 NOT NULL,
	"recipient_count" integer DEFAULT 0 NOT NULL,
	"gas_price_gwei" text,
	"min_nano" bigint DEFAULT 0 NOT NULL,
	"note" text,
	"created_by" text,
	"error" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"prepared_at" timestamp with time zone,
	"sent_at" timestamp with time zone,
	"completed_at" timestamp with time zone
);
--> statement-breakpoint
ALTER TABLE "payouts" ADD COLUMN "batch_id" uuid;--> statement-breakpoint
ALTER TABLE "payouts" ADD COLUMN "wallet_address" text;--> statement-breakpoint
ALTER TABLE "payouts" ADD COLUMN "tx_hash" text;--> statement-breakpoint
ALTER TABLE "payouts" ADD COLUMN "chain_status" text;--> statement-breakpoint
ALTER TABLE "payouts" ADD COLUMN "chain_error" text;--> statement-breakpoint
ALTER TABLE "payouts" ADD CONSTRAINT "payouts_batch_fk" FOREIGN KEY ("batch_id") REFERENCES "payout_batches"("id") ON DELETE set null;--> statement-breakpoint
CREATE INDEX "payouts_batch_idx" ON "payouts" USING btree ("batch_id");--> statement-breakpoint
-- Один on-chain перевод на строку выплаты: хеш уникален там, где он есть (partial unique).
CREATE UNIQUE INDEX "payouts_tx_hash_uidx" ON "payouts" USING btree ("tx_hash") WHERE "tx_hash" IS NOT NULL;
