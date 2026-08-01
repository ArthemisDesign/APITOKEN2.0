-- Fixed-point evidence for workload-aware OpenAI/Codex window calibration.
--
-- The provider sends `used_percent` as a JSON/HTTP decimal, while estimator v5 persisted only the
-- rounded whole percent.  These nullable expand columns preserve 10^-8 fraction units and the
-- exact cumulative legs required by a realized-workload estimator.  They intentionally have no
-- defaults: the currently serving binary may continue inserting/updating the legacy columns while
-- this migration is deployed, and the following application release can distinguish those rows
-- and reconstruct their fixed-point value from immutable legacy evidence.

ALTER TABLE codex_window_calibrations
    ADD COLUMN IF NOT EXISTS anchor_used_fraction_units bigint
        CHECK (anchor_used_fraction_units BETWEEN 0 AND 100000000),
    ADD COLUMN IF NOT EXISTS used_fraction_units bigint
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    ADD COLUMN IF NOT EXISTS observed_fraction_units bigint
        CHECK (observed_fraction_units >= 0),
    ADD COLUMN IF NOT EXISTS observed_spend_nano bigint
        CHECK (observed_spend_nano >= 0);

ALTER TABLE codex_window_observations
    ADD COLUMN IF NOT EXISTS used_fraction_units bigint
        CHECK (used_fraction_units BETWEEN 0 AND 100000000);

INSERT INTO engine_schema_migrations(version) VALUES (15)
ON CONFLICT (version) DO NOTHING;
