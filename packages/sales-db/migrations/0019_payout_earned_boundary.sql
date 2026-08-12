-- Preserve the exact commission cutoff used to prepare an on-chain payout batch. A later
-- consumer release will replay its signed-balance proof at this same boundary immediately before
-- transfer, so newer still-locked commission cannot make an older stale row appear payable.
ALTER TABLE "payout_batches" ADD COLUMN "earned_before" timestamp with time zone;
