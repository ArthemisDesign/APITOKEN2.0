-- Last measured cost of a delivery, so an abandoned turn is settled from fact rather than ceiling.
--
-- A reservation moves to `delivering` before the first byte and is settled when the answer ends. If
-- the owning process never gets to settle — a kill, an OOM, a host loss — the reconciler charges the
-- full preflight hold. That hold is a byte-conservative estimate plus the model's entire output
-- limit: on this fleet it runs 19x-250x the real cost of a turn, so the customer pays a double-digit
-- multiple for an answer nobody can describe.
--
-- The measurement exists long before the settlement does: providers parse cumulative usage on every
-- streamed frame. Writing it here as it arrives means the durable record already knows what the turn
-- cost, and the reconciler never has to guess. What remains uncertain is only the tokens between the
-- last checkpoint and the death — bounded by the checkpoint cadence, and always in the customer's
-- favour.
--
-- Deliberately a COLUMN on `reservations` rather than a checkpoint table. The lifetime question
-- ("when may this be deleted?") then has no independent answer to get wrong: the value is born with
-- the reservation, updated in place — so a long stream overwrites rather than accumulates — and
-- removed by the existing `maintenance_prune` that already deletes settled reservations with their
-- children. No new rows, no new retention policy, no way for these to outlive what they describe.

ALTER TABLE reservations
    ADD COLUMN IF NOT EXISTS measured_nano bigint;

INSERT INTO engine_schema_migrations(version) VALUES (38)
ON CONFLICT (version) DO NOTHING;
