-- Persist the complete Commerce catalog and effective-policy lineage before any writer is enabled.
--
-- Commerce versions every product catalog against an exact capability generation and digest, and
-- materializes every account policy from an immutable source policy into an immutable account
-- class. Migrations 0006/0007 omitted those three fields, so a future retry-safe ACK could otherwise
-- confirm target identity that the engine had discarded or kept only in a mutable binding. No
-- legacy scalar account, key, balance, reservation, or ledger row is touched.

DO $migration$
DECLARE
    table_name text;
    has_rows boolean;
BEGIN
    IF EXISTS (
        SELECT 1 FROM public.engine_schema_migrations WHERE version = 8
    ) THEN
        RETURN;
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'pricing_catalog_versions',
        'pricing_catalog_entries',
        'pricing_catalog_heads',
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
                CONSTRAINT = 'multi_discount_lineage_empty_preflight',
                MESSAGE = format(
                    '0008 requires empty pre-writer table public.%I; manual audit required',
                    table_name
                );
        END IF;
    END LOOP;

    EXECUTE $ddl$
        ALTER TABLE public.pricing_catalog_versions
            ADD COLUMN IF NOT EXISTS capability_generation bigint,
            ALTER COLUMN capability_generation SET NOT NULL
    $ddl$;

    EXECUTE $ddl$
        ALTER TABLE public.account_policy_versions
            ADD COLUMN IF NOT EXISTS source_policy_digest text,
            ADD COLUMN IF NOT EXISTS account_class text,
            ALTER COLUMN source_policy_digest SET NOT NULL,
            ALTER COLUMN account_class SET NOT NULL
    $ddl$;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'pricing_catalog_versions_capability_generation'
          AND conrelid = 'public.pricing_catalog_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.pricing_catalog_versions
                ADD CONSTRAINT pricing_catalog_versions_capability_generation CHECK (
                    capability_generation > 0
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_versions_source_identity'
          AND conrelid = 'public.account_policy_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_versions
                ADD CONSTRAINT account_policy_versions_source_identity CHECK (
                    source_policy_digest <> ''
                    AND (
                        (owner_type = 'global_b2c' AND account_class = 'b2c')
                        OR (owner_type = 'b2b_client' AND account_class = 'b2b')
                        OR (owner_type = 'openkeys' AND account_class = 'openkeys')
                        OR (owner_type = 'service' AND account_class = 'service')
                    )
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_versions_class_identity'
          AND conrelid = 'public.account_policy_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_versions
                ADD CONSTRAINT account_policy_versions_class_identity UNIQUE (
                    account_id,
                    effective_version,
                    product_id,
                    account_class
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_versions_lineage_identity'
          AND conrelid = 'public.account_policy_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_versions
                ADD CONSTRAINT account_policy_versions_lineage_identity UNIQUE (
                    account_id,
                    effective_version,
                    policy_id,
                    policy_version,
                    source_policy_digest,
                    owner_type,
                    owner_id,
                    product_id,
                    account_class,
                    catalog_generation,
                    switch_generation,
                    schema_version,
                    content_digest
                )
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'account_policy_bindings_active_class_fk'
          AND conrelid = 'public.account_policy_bindings'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.account_policy_bindings
                ADD CONSTRAINT account_policy_bindings_active_class_fk
                    FOREIGN KEY (
                        account_id,
                        active_effective_version,
                        product_id,
                        account_class
                    )
                    REFERENCES public.account_policy_versions(
                        account_id,
                        effective_version,
                        product_id,
                        account_class
                    )
                    ON DELETE RESTRICT
        $ddl$;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'pricing_catalog_versions_ack_identity'
          AND conrelid = 'public.pricing_catalog_versions'::regclass
    ) THEN
        EXECUTE $ddl$
            ALTER TABLE public.pricing_catalog_versions
                ADD CONSTRAINT pricing_catalog_versions_ack_identity UNIQUE (
                    product_id,
                    generation,
                    schema_version,
                    capability_generation,
                    capability_digest,
                    content_digest
                )
        $ddl$;
    END IF;

    INSERT INTO public.engine_schema_migrations(version) VALUES (8)
    ON CONFLICT (version) DO NOTHING;
END;
$migration$;
