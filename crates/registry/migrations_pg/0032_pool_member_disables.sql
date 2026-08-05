-- Operator authority over pool membership: which roster-backed profiles may be routed to.
--
-- Credentials for the Gemini/Codex/KIMI/GLM fleets are produced and owned by the Auth Bot, which
-- publishes them as a sealed, atomically-replaced roster. The engine reads that roster but must
-- never write it: any edit is erased by the next publication, and the roster layout is validated
-- on load, so a hand-patched file fails closed for the whole fleet.
--
-- Routability, however, is an engine decision, not a credential fact. An operator needs to pull a
-- specific profile out of rotation — a credential the provider revoked, an account under review,
-- one misbehaving home — and needs that decision to survive both a slot restart and the next
-- roster publication. This table is that decision, kept deliberately BESIDE the roster:
--
--   roster  -> "these credentials exist and are valid"        (Auth Bot authority)
--   here    -> "this member may/may not receive traffic"      (engine authority)
--
-- Presence of a row means disabled. Re-enabling deletes the row, so both directions are
-- idempotent and there is no third state to reconcile after a partial write.
--
-- Anthropic is deliberately NOT a provider here. Claude subscriptions are rows in this same
-- authority and already carry active|paused|disabled, which the pool honours when it loads them.
-- Adding them to this table would create a second, competing switch for the same subscription —
-- two sources of truth that can disagree about whether an account is routable. The provider CHECK
-- below is a closed set precisely so that mistake cannot be made later by accident.

CREATE TABLE IF NOT EXISTS pool_member_disables (
    -- Closed set: only fleets whose membership comes from a sealed roster the engine cannot edit.
    -- Anthropic is excluded by design (see above), not by omission. The vocabulary is the same one
    -- provider attribution already uses (PROVIDER_OPENAI/PROVIDER_GOOGLE/...), so the Codex fleet
    -- is 'openai' and the Gemini fleet is 'google' here too — one provider vocabulary in this
    -- authority, not a second one that drifts.
    provider text NOT NULL CHECK (provider IN ('google', 'openai', 'kimi', 'glm')),

    -- Roster-local identity of the member (e.g. a Gemini profile id, a Codex home id). Opaque to
    -- this table: it is compared verbatim against the ids the roster publishes.
    member_id text NOT NULL CHECK (member_id <> ''),

    -- Why it was pulled, for the operator reading this six weeks later. Free text, may be empty
    -- when the reason is obvious from context, but the column is NOT NULL so a row always carries
    -- an explicit (possibly empty) intent rather than an ambiguous NULL.
    reason text NOT NULL DEFAULT '',

    -- Who pulled it. Same NOT NULL reasoning as `reason`.
    actor text NOT NULL DEFAULT '',

    updated_ts bigint NOT NULL CHECK (updated_ts > 0),

    PRIMARY KEY (provider, member_id)
);

-- The pools read the whole disabled set for one provider on every roster load and on a short
-- refresh interval, so the provider prefix of the primary key is the access path. No secondary
-- index is warranted: this table is bounded by operator actions, not by traffic.

INSERT INTO engine_schema_migrations(version) VALUES (32)
ON CONFLICT (version) DO NOTHING;
