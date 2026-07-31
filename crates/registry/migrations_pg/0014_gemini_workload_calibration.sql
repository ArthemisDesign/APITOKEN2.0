-- Exact cumulative official-API spend for Gemini workload-dependent calibration.
--
-- The original WLS sufficient statistics cannot recover SUM(delta spend), which is required for
-- a realized workload blend. This expand-only column is deliberately defaulted so the currently
-- serving estimator remains compatible until the application rollout rebuilds it from the
-- immutable observation log.

ALTER TABLE gemini_window_calibrations
    ADD COLUMN IF NOT EXISTS observed_spend_nano bigint NOT NULL DEFAULT 0
        CHECK (observed_spend_nano >= 0);

INSERT INTO engine_schema_migrations(version) VALUES (14)
ON CONFLICT (version) DO NOTHING;
