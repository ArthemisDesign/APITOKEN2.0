-- Durable account-level health for one Codex home.
--
-- Only the ACCOUNT axis is persisted. Transport health belongs to a single app-server generation
-- and dies with it, so carrying it across a restart would be a lie: the new process has a new
-- bridge and deserves a fresh verdict. The account does not care which generation talked to it.
--
-- Without this table every gateway restart — and blue-green makes those routine — reset a
-- corroborated dead or quota-limited verdict back to healthy and immediately re-admitted the home.
-- That is the same durability argument the Claude pool's `subs.auth_state` was introduced for,
-- after its ephemeral predecessor was lost on every deploy.

CREATE TABLE IF NOT EXISTS codex_home_health(
    home_id text PRIMARY KEY,
    account_state text NOT NULL DEFAULT 'healthy'
        CHECK (account_state IN ('healthy', 'suspect', 'dead')),
    auth_fail_streak bigint NOT NULL DEFAULT 0 CHECK (auth_fail_streak >= 0),
    first_auth_fail_ts bigint NOT NULL DEFAULT 0 CHECK (first_auth_fail_ts >= 0),
    cooling_until bigint NOT NULL DEFAULT 0 CHECK (cooling_until >= 0),
    updated_ts bigint NOT NULL
);

INSERT INTO engine_schema_migrations(version) VALUES (12)
ON CONFLICT (version) DO NOTHING;
