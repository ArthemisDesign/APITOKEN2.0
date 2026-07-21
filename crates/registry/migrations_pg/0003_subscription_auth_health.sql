-- Durable per-subscription auth-health state (expand-only).
-- Today a "dead/blocked" subscription is only an ephemeral in-memory bit (pool::Live.auth_dead)
-- that the leader poller sets, mark_healthy clears, and every restart drops. Claude bans
-- subscriptions, so operators need a DURABLE, authority-owned verdict that survives restarts and
-- blue/green overlap. These columns are that source of truth; the poller writes them from
-- corroborated clean probes. All columns are additive with safe defaults — existing rows stay
-- 'healthy' and every current SELECT (explicit column lists) is unaffected.
--
-- State machine (decided in `pool`/`server`, persisted here):
--   healthy --(clean probe 401/403)--> suspect --(streak >= N over >= T, no 2xx)--> dead
--      ^                                   |                                          |
--      +------------ any real 2xx ---------+---- token replaced / operator revive -----+
--
--   auth_state         'healthy' | 'suspect' | 'dead'
--   auth_fail_streak   consecutive clean-probe auth failures (reset to 0 by any 2xx)
--   first_auth_fail_ts epoch secs the current failing streak began
--   last_auth_fail_ts  epoch secs of the most recent auth failure
--   last_auth_http     last auth HTTP code observed (401 | 403)
--   dead_since_ts      epoch secs the sub became terminal ('dead')
--   dead_reason        'authentication_error' (token revoked/expired → re-auth)
--                      | 'permission_error' (account blocked/banned) | operator text
--   auth_token_fp      short fingerprint of the token this verdict is about; a changed token
--                      (authbot replaced it) auto-resets health back to 'healthy'

ALTER TABLE subs
    ADD COLUMN IF NOT EXISTS auth_state text NOT NULL DEFAULT 'healthy',
    ADD COLUMN IF NOT EXISTS auth_fail_streak integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS first_auth_fail_ts bigint,
    ADD COLUMN IF NOT EXISTS last_auth_fail_ts bigint,
    ADD COLUMN IF NOT EXISTS last_auth_http integer,
    ADD COLUMN IF NOT EXISTS dead_since_ts bigint,
    ADD COLUMN IF NOT EXISTS dead_reason text,
    ADD COLUMN IF NOT EXISTS auth_token_fp text;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'subs_auth_state_valid' AND conrelid = 'subs'::regclass
    ) THEN
        ALTER TABLE subs
            ADD CONSTRAINT subs_auth_state_valid
            CHECK (auth_state IN ('healthy', 'suspect', 'dead'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'subs_auth_fail_streak_nonnegative' AND conrelid = 'subs'::regclass
    ) THEN
        ALTER TABLE subs
            ADD CONSTRAINT subs_auth_fail_streak_nonnegative
            CHECK (auth_fail_streak >= 0);
    END IF;
END $$;

-- Small partial index: the panel/alerts only ever scan the non-healthy minority.
CREATE INDEX IF NOT EXISTS subs_auth_state_idx ON subs(auth_state) WHERE auth_state <> 'healthy';

INSERT INTO engine_schema_migrations(version) VALUES (3)
ON CONFLICT (version) DO NOTHING;
