-- Персистим подписанный raw для идемпотентного ре-бродкаста поллером (без переподписи, устойчиво к
-- смене газа/конфига). Плюс индекс по nonce для расчёта следующего свободного nonce.
ALTER TABLE "payouts" ADD COLUMN "raw_tx" text;--> statement-breakpoint
CREATE INDEX "payouts_nonce_idx" ON "payouts" USING btree ("nonce") WHERE "chain_status" = 'broadcast';
