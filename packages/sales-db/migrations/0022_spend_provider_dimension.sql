-- Which provider served a referred spend event, kept as a pure reporting dimension.
--
-- The live feed emits the scalar usage form: commission is computed from the amount the producer
-- already narrowed to the customer's own money, and the retired per-request attribution tuple
-- (provider_id, account_class, pricing_mode, paid_funded_nano, commission_eligible, snapshot_digest)
-- arrives all-null. `partner_usage_events_multi_discount_check` binds the legacy provider_id to that
-- whole tuple, so the scalar path could not record its provider without re-opening a retired
-- authority. The producer does send it, and sales dropped it: every live row has provider_id NULL,
-- leaving partner earnings with no provider breakdown while the pool serves four providers.
--
-- A separate column keeps the two apart by construction: spend_provider_id NEVER participates in
-- any commission, eligibility or pricing decision — it only answers "where did this spend happen".
-- Nullable and unbackfilled: rows imported before this migration genuinely have no provider on
-- record, and readers must show them as unattributed rather than guess.
ALTER TABLE "partner_usage_events" ADD COLUMN "spend_provider_id" text;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD CONSTRAINT "partner_usage_events_spend_provider_check"
	CHECK ("spend_provider_id" IS NULL OR "spend_provider_id" <> '');--> statement-breakpoint
-- Reporting reads slice a partner's history by provider over a time range.
CREATE INDEX "partner_usage_events_partner_provider_idx"
	ON "partner_usage_events" ("partner_id", "spend_provider_id", "occurred_at");
--> statement-breakpoint
-- Spend that arrives before its user's attribution waits in the buffer and is replayed later.
-- Without the same column the dimension would be silently lost for exactly those events.
ALTER TABLE "pending_referral_events" ADD COLUMN "spend_provider_id" text;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD CONSTRAINT "pending_referral_events_spend_provider_check"
	CHECK ("spend_provider_id" IS NULL OR "spend_provider_id" <> '');
