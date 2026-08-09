-- Strict funding check reads the funding-v2 authority (expand-only).
--
-- Migration 0016's assert_strict_funding_account validates, for a binding with
-- funding_enforcement='strict', that per-account funding rows sum exactly to the account
-- aggregates. It reads the v1 `funding_buckets` table — but accounts normalized during the
-- release era carry their truth only in `funding_lots_v2` (the authority every modern reserve
-- path maintains), so the strict activation of such an account fails with "strict funding
-- buckets do not match account aggregates" while its lots reconcile exactly. Verified live:
-- acct_14c48c722aff177c74d3d532 has balance = active lots = 2205180000 and zero bucket rows.
--
-- This migration replaces ONLY the source of the bucket sum inside the same function: when the
-- account is funding-v2 normalized (a row in account_funding_head_v2), the sum comes from its
-- active funding_lots_v2 rows — the authority every modern reserve path maintains; otherwise it
-- keeps reading funding_buckets exactly as before (legacy strict fixtures and any
-- pre-normalization account). The invariant, its strict-enforcement scoping, the exception and
-- every caller/trigger are unchanged. Accounts already passing keep passing (buckets and lots
-- reconcile for them — verified on the production set of strict-funded bindings), accounts with
-- lots-only history now pass, and a genuine mismatch still raises. The dependent strict cutover
-- retries resume with no code change.

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
          AND status = 'active';
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

INSERT INTO engine_schema_migrations(version) VALUES (41)
ON CONFLICT (version) DO NOTHING;
