-- Pricing release opt-out marker (expand-only).
--
-- Head 55 is the final pricing release. The retirement of the release-v2 resolver happens in
-- phases: first the engine learns to serve an opted-out account from the direct strict-policy /
-- legacy paths while the release head keeps serving everyone else (the dual path), then commerce
-- migrates every account, and only then the release code is deleted. This migration adds the
-- per-account marker that drives the dual path: a NULL `pricing_release_opt_out_ts` means the
-- account is priced by the active release exactly as today; a non-NULL timestamp means the
-- account opted out of the release path and its requests fall through to the strict-policy /
-- legacy reserve paths.
--
-- The column is nullable and has no default and no constraint: every existing account reads as
-- NULL, so this SHA changes no behavior. The dependent dual-path resolver ships in a separate SHA
-- after a green migration/watchdog of this checkpoint.

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS pricing_release_opt_out_ts bigint;

INSERT INTO engine_schema_migrations(version) VALUES (39)
ON CONFLICT (version) DO NOTHING;
