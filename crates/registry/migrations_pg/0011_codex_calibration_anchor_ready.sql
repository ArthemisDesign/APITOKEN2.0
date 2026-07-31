-- Persist whether the first, potentially partial percentage transition of the concrete provider
-- window has already been crossed. Older binaries ignore this additive column; estimator v3 uses
-- it to resume interval collection correctly after restarts and blue/green handoffs.

ALTER TABLE codex_window_calibrations
    ADD COLUMN IF NOT EXISTS anchor_ready boolean NOT NULL DEFAULT false;

INSERT INTO engine_schema_migrations(version) VALUES (11)
ON CONFLICT (version) DO NOTHING;
