-- Expand-only persistence foundation for cross-model execution-group fencing.
--
-- The currently serving engine does not supply group identity. `group_id` therefore stays nullable:
-- NULL means that the reservation's own `request_id` is its effective one-attempt group. The
-- dependent runtime rollout will supply explicit group IDs for router fallback attempts and use
-- COALESCE(group_id, request_id) while old and new reservations coexist.

ALTER TABLE reservations
    ADD COLUMN IF NOT EXISTS group_id text,
    ADD COLUMN IF NOT EXISTS attempt integer NOT NULL DEFAULT 1;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_group_id_nonempty'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_group_id_nonempty
            CHECK (group_id IS NULL OR group_id <> '');
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_attempt_positive'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_attempt_positive
            CHECK (attempt > 0);
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS execution_group_winner (
    group_id text PRIMARY KEY CHECK (group_id <> ''),
    winner_request_id text NOT NULL CHECK (winner_request_id <> ''),
    decided_at bigint NOT NULL
);

COMMENT ON COLUMN reservations.group_id IS
    'Explicit router execution group; NULL means the effective group is request_id.';
COMMENT ON COLUMN reservations.attempt IS
    'One-based serial attempt within the effective execution group.';
COMMENT ON TABLE execution_group_winner IS
    'Insert-first-wins fence allowing at most one nonzero settlement per execution group.';

INSERT INTO engine_schema_migrations(version) VALUES (21)
ON CONFLICT (version) DO NOTHING;
