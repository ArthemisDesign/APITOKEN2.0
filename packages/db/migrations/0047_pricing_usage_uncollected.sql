-- Preserve the engine's full billed amount while recording the pool-funded part that the
-- account-wide settlement floor could not collect. The default keeps every deployed commerce
-- writer compatible: old versions omit the field and therefore persist the historical zero.
ALTER TABLE "pricing_usage_events"
  ADD COLUMN "uncollected_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_events"
  ADD CONSTRAINT "pricing_usage_events_uncollected_check"
  CHECK ("uncollected_nano" >= 0 AND "uncollected_nano" <= "amount_nano");
