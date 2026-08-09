-- Strict funding check sums the ACTIVE GENERATION's lots (expand-only).
--
-- Migration 0041 re-sourced the strict funding check to funding_lots_v2 but filtered
-- status='active'. The funding-v2 model retires a lot to 'exhausted' with a nonzero balance
-- (e.g. a paid residual that went negative as the account spent down: welcome +$4 active plus
-- paid -$4 exhausted nets the exact zero aggregate), so the status filter breaks the sum for
-- exactly those accounts — 48 strict activations were stuck retrying "strict funding buckets do
-- not match account aggregates" while their active-generation lots reconciled to the nano
-- (verified live over the full stuck cohort: active-generation lot sums equal the account
-- aggregates for all 48, including negative-balance and adjusted accounts).
--
-- This migration replaces ONLY the row selection inside assert_strict_funding_account's
-- normalized branch: lots of the account's ACTIVE funding generation regardless of lot status
-- (the generation head is the authority boundary; retired generations never enter the sum).
-- The pre-normalization funding_buckets branch, the invariant, its scoping and the exception
-- text are unchanged; accounts already passing keep passing. The stalled strict deliveries
-- resume with no code change.

CREATE OR REPLACE FUNCTION assert_strict_funding_account(p_account_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    account_balance bigint;
    account_reserved bigint;
    bucket_balance numeric;
    bucket_reserved numeric;
    strict_funding boolean;
    normalized boolean;
BEGIN
    SELECT
        a.balance_nano,
        a.reserved_nano,
        COALESCE(b.funding_enforcement = 'strict', false)
    INTO account_balance, account_reserved, strict_funding
    FROM accounts a
    LEFT JOIN account_policy_bindings b ON b.account_id = a.id
    WHERE a.id = p_account_id;

    IF NOT FOUND OR NOT strict_funding THEN
        RETURN;
    END IF;

    SELECT EXISTS (
        SELECT 1 FROM account_funding_head_v2 WHERE account_id = p_account_id
    ) INTO normalized;

    IF normalized THEN
        SELECT COALESCE(sum(balance_nano), 0), COALESCE(sum(reserved_nano), 0)
        INTO bucket_balance, bucket_reserved
        FROM funding_lots_v2
        WHERE account_id = p_account_id
          AND funding_generation = (
              SELECT active_generation FROM account_funding_head_v2
              WHERE account_id = p_account_id
          );
    ELSE
        SELECT COALESCE(sum(balance_nano), 0), COALESCE(sum(reserved_nano), 0)
        INTO bucket_balance, bucket_reserved
        FROM funding_buckets
        WHERE account_id = p_account_id;
    END IF;

    IF bucket_balance <> account_balance OR bucket_reserved <> account_reserved THEN
        RAISE EXCEPTION 'strict funding buckets do not match account aggregates'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

INSERT INTO engine_schema_migrations(version) VALUES (42)
ON CONFLICT (version) DO NOTHING;
