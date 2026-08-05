-- Presentation state for an already-disabled pool member: keep it out of the operator's list.
--
-- A fleet accumulates credentials that are permanently dead — a revoked OAuth grant, a banned
-- account — and once an operator has pulled one out of rotation there is nothing left to decide
-- about it. Leaving it in the board forever is noise that makes the live rows harder to read.
--
-- Hiding is deliberately a column on `pool_member_disables` rather than its own table, because
-- that makes the safety rule structural instead of a check someone can forget: a row exists only
-- for a DISABLED member, so a hidden member is always a disabled one. Hiding a profile that still
-- receives traffic would remove live capacity from the operator's view while it keeps serving —
-- the one outcome this feature must never allow. Re-enabling deletes the row, which drops the
-- hidden flag with it: a member returning to rotation is always visible again.
--
-- The engine keeps REPORTING hidden members (with the flag) rather than omitting them from
-- `/gemini-subs`. Presentation belongs to the panel, and an operator has to be able to reveal and
-- restore one; an endpoint that silently dropped rows would make hiding irreversible from the UI.

ALTER TABLE pool_member_disables
    ADD COLUMN IF NOT EXISTS hidden boolean NOT NULL DEFAULT false;

INSERT INTO engine_schema_migrations(version) VALUES (33)
ON CONFLICT (version) DO NOTHING;
