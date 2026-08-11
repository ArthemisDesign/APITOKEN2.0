-- Commerce mirrors the engine's payable-multiplier contract. Existing application writers already
-- emit only 0..10000, and production was verified to contain no out-of-range row before this
-- checkpoint. Keep the original non-negative constraint for compatibility and add the missing
-- upper fence under a new name so this remains an additive migration.
ALTER TABLE "engine_accounts"
  ADD CONSTRAINT "engine_accounts_mult_bp_upper_check" CHECK ("mult_bp" <= 10000);
