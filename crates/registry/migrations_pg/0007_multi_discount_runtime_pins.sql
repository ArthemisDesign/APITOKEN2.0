-- Complete the durable target identity required before any multi-discount writer exists.
--
-- Migration 0006 intentionally shipped without writers. Commerce subsequently made the
-- capability, catalog, and switch generations part of its exact delivery/ACK contract. Persist
-- those pins in the engine before a Control API can claim that a policy or switch was durably
-- applied. Legacy scalar accounts, keys, balances, reservations, and ledger rows are untouched.

DO $migration$
DECLARE
    table_name text;
    has_rows boolean;
BEGIN
    IF EXISTS (
        SELECT 1 FROM public.engine_schema_migrations WHERE version = 7
    ) THEN
        RETURN;
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'provider_switch_versions',
        'provider_switch_entries',
        'provider_switch_head',
        'account_policy_versions',
        'account_policy_rules',
        'account_policy_bindings'
    ]
    LOOP
        EXECUTE format(
            'LOCK TABLE public.%I IN SHARE ROW EXCLUSIVE MODE NOWAIT',
            table_name
        );
        EXECUTE format(
            'SELECT EXISTS (SELECT 1 FROM public.%I LIMIT 1)',
            table_name
        )
        INTO has_rows;

        IF has_rows THEN
            RAISE EXCEPTION USING
                ERRCODE = '23514',
                CONSTRAINT = 'multi_discount_runtime_pins_empty_preflight',
                MESSAGE = format(
                    '0007 requires empty pre-writer table public.%I; manual audit required',
                    table_name
                );
        END IF;
    END LOOP;

    EXECUTE $ddl$
        ALTER TABLE public.provider_switch_versions
            ADD COLUMN IF NOT EXISTS capability_generation bigint,
            ADD COLUMN IF NOT EXISTS capability_digest text,
            ALTER COLUMN capability_generation SET NOT NULL,
            ALTER COLUMN capability_digest SET NOT NULL
    $ddl$;

    EXECUTE $ddl$
        ALTER TABLE public.provider_switch_entries
            ADD COLUMN IF NOT EXISTS catalog_generation bigint
    $ddl$;

    EXECUTE $ddl$
        ALTER TABLE public.account_policy_versions
            ADD COLUMN IF NOT EXISTS switch_generation bigint,
            ALTER COLUMN switch_generation SET NOT NULL
    $ddl$;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'provider_switch_versions_capability_identity'
          AND conrelid = 'public.provider_switch_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.provider_switch_versions
                ADD CONSTRAINT provider_switch_versions_capability_identity CHECK (
                    capability_generation > 0
                    AND capability_digest <> ''
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'provider_switch_versions_ack_identity'
          AND conrelid = 'public.provider_switch_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.provider_switch_versions
                ADD CONSTRAINT provider_switch_versions_ack_identity UNIQUE (
                    generation,
                    schema_version,
                    capability_generation,
                    capability_digest,
                    content_digest
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'provider_switch_entries_catalog_fk'
          AND conrelid = 'public.provider_switch_entries'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.provider_switch_entries
                ADD CONSTRAINT provider_switch_entries_catalog_fk
                    FOREIGN KEY (product_id, catalog_generation)
                    REFERENCES public.pricing_catalog_versions(product_id, generation)
                    ON DELETE RESTRICT
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'provider_switch_entries_catalog_scope'
          AND conrelid = 'public.provider_switch_entries'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.provider_switch_entries
                ADD CONSTRAINT provider_switch_entries_catalog_scope CHECK (
                    (
                        scope_type = 'master'
                        AND product_id = ''
                        AND segment = ''
                        AND catalog_generation IS NULL
                    )
                    OR (
                        scope_type = 'product'
                        AND product_id <> ''
                        AND segment = ''
                        AND catalog_generation IS NOT NULL
                        AND catalog_generation > 0
                    )
                    OR (
                        scope_type = 'segment'
                        AND product_id <> ''
                        AND segment IN ('b2c', 'b2b')
                        AND catalog_generation IS NOT NULL
                        AND catalog_generation > 0
                    )
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_versions_switch_fk'
          AND conrelid = 'public.account_policy_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_versions
                ADD CONSTRAINT account_policy_versions_switch_fk
                    FOREIGN KEY (switch_generation)
                    REFERENCES public.provider_switch_versions(generation)
                    ON DELETE RESTRICT
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_versions_ack_identity'
          AND conrelid = 'public.account_policy_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_versions
                ADD CONSTRAINT account_policy_versions_ack_identity UNIQUE (
                    account_id,
                    effective_version,
                    policy_id,
                    policy_version,
                    product_id,
                    catalog_generation,
                    switch_generation,
                    schema_version,
                    content_digest
                )
        $ddl$;
    END IF;

    INSERT INTO public.engine_schema_migrations(version) VALUES (7)
    ON CONFLICT (version) DO NOTHING;
END;
$migration$;
