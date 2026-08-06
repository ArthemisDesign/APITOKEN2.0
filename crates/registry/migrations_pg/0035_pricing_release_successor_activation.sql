-- Stage 9 successor activation kind (expand-only).
--
-- Until now the global pricing release head could only appear once (cutover, from an absent head)
-- or advance to the paired recovery of the same prepared pair (recovery). A successor activation
-- advances an active head to a NEWER prepared target/recovery pair — the standard way a new pricing
-- generation (added models, changed per-account discounts, refreshed inventory) goes live. The
-- activation audit stores the exact previous head in from_generation/from_digest and the new
-- target in to_generation/to_digest, backed by the same fresh passed Stage 8 evidence as any other
-- activation. Existing cutover/recovery rows, checks and trigger arms are unchanged; the head-step
-- and head-audit triggers are kind-agnostic and already cover the new transition.

ALTER TABLE pricing_release_activations_v2
    DROP CONSTRAINT pricing_release_activations_v2_activation_kind_check;
ALTER TABLE pricing_release_activations_v2
    ADD CONSTRAINT pricing_release_activations_v2_activation_kind_check
    CHECK (activation_kind IN ('cutover', 'recovery', 'successor'));

CREATE OR REPLACE FUNCTION enforce_pricing_release_activation_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_stage8_evidence_v2 evidence
        WHERE evidence.evidence_digest = NEW.evidence_digest
          AND evidence.passed
          AND NEW.activated_ts >= evidence.observed_ts
          AND NEW.activated_ts <= evidence.valid_until_ts
          AND (
              (
                  NEW.activation_kind = 'cutover'
                  AND NEW.to_generation = evidence.target_generation
                  AND NEW.to_digest = evidence.target_digest
              )
              OR (
                  NEW.activation_kind = 'recovery'
                  AND NEW.to_generation = evidence.recovery_generation
                  AND NEW.to_digest = evidence.recovery_digest
              )
              OR (
                  NEW.activation_kind = 'successor'
                  AND NEW.from_generation IS NOT NULL
                  AND NEW.from_generation <> evidence.target_generation
                  AND NEW.to_generation = evidence.target_generation
                  AND NEW.to_digest = evidence.target_digest
              )
          )
    ) THEN
        RAISE EXCEPTION 'pricing v2 activation requires fresh passed evidence for its release'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

INSERT INTO engine_schema_migrations(version) VALUES (35)
ON CONFLICT (version) DO NOTHING;
