import { readFileSync } from "node:fs";
import { basename, dirname, isAbsolute, join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  DEFAULT_MIGRATION_LOCK_TIMEOUT_MS,
  DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS,
  MIGRATION_DATABASE_URL_ENV,
  MIGRATION_LOCK_KEY,
  MIGRATIONS_FOLDER,
  resolveMigrationConfig,
} from "./migrate.js";

const MULTI_DISCOUNT_TABLES = [
  "account_policy_bindings",
  "account_policy_reconciliations",
  "account_policy_rules",
  "account_policy_versions",
  "business_invite_policy_bindings",
  "engine_catalog_jobs",
  "engine_policy_jobs",
  "engine_switch_jobs",
  "pricing_policies",
  "pricing_policy_heads",
  "pricing_policy_rules",
  "pricing_policy_versions",
  "pricing_usage_attributions",
  "pricing_usage_funding_allocations",
  "product_catalog_entries",
  "product_catalog_heads",
  "product_catalog_versions",
  "provider_capability_aliases",
  "provider_capability_entries",
  "provider_capability_head",
  "provider_capability_versions",
  "provider_switch_entries",
  "provider_switch_head",
  "provider_switch_versions",
] as const;

const PRICING_RELEASE_V2_TABLES = [
  "business_invite_policy_snapshots_v2",
  "pricing_funding_normalizations_v2",
  "pricing_policy_documents_v2",
  "pricing_policy_rules_v2",
  "pricing_release_activation_receipts_v2",
  "pricing_release_assignments_v2",
  "pricing_release_control_jobs_v2",
  "pricing_release_plans_v2",
  "pricing_stage8_evidence_v2",
  "service_account_inventory_v2",
] as const;

const PRICING_STAGE5_EVIDENCE_TABLES = [
  "pricing_stage5_blockers_v2",
  "pricing_stage5_prepare_acks_v2",
  "pricing_stage5_runs_v2",
] as const;

describe("migration configuration", () => {
  it("keeps the advisory lock key stable", () => {
    expect(MIGRATION_LOCK_KEY).toBe("719471115124720130");
  });

  it("resolves the package migrations folder", () => {
    expect(isAbsolute(MIGRATIONS_FOLDER)).toBe(true);
    expect(basename(MIGRATIONS_FOLDER)).toBe("migrations");
    expect(basename(dirname(MIGRATIONS_FOLDER))).toBe("db");
  });

  it("reads DATABASE_URL with bounded timeout defaults", () => {
    expect(resolveMigrationConfig({ [MIGRATION_DATABASE_URL_ENV]: "postgresql://db.example/app" })).toEqual({
      connectionString: "postgresql://db.example/app",
      lockTimeoutMs: DEFAULT_MIGRATION_LOCK_TIMEOUT_MS,
      statementTimeoutMs: DEFAULT_MIGRATION_STATEMENT_TIMEOUT_MS,
    });
  });

  it("accepts timeout overrides", () => {
    expect(
      resolveMigrationConfig({
        DATABASE_URL: "postgresql://db.example/app",
        DB_MIGRATION_LOCK_TIMEOUT_MS: "45000",
        DB_MIGRATION_STATEMENT_TIMEOUT_MS: "1200000",
      }),
    ).toEqual({
      connectionString: "postgresql://db.example/app",
      lockTimeoutMs: 45_000,
      statementTimeoutMs: 1_200_000,
    });
  });

  it("rejects missing connection configuration and unbounded timeouts", () => {
    expect(() => resolveMigrationConfig({})).toThrow("DATABASE_URL is required");
    expect(() =>
      resolveMigrationConfig({
        DATABASE_URL: "postgresql://db.example/app",
        DB_MIGRATION_LOCK_TIMEOUT_MS: "0",
      }),
    ).toThrow("DB_MIGRATION_LOCK_TIMEOUT_MS must be between 1 and 2147483647 milliseconds");
  });

  it("keeps the tier-5 expansion compatible across adjacent rollout migrations", () => {
    for (const migration of [
      "0008_prepay_tier_columns.sql",
      "0009_round_joshua_kane.sql",
      "0010_optimal_komodo.sql",
    ]) {
      const sql = readFileSync(join(MIGRATIONS_FOLDER, migration), "utf8");
      expect(sql).toContain('"current_tier" BETWEEN 0 AND 5');
      expect(sql).not.toContain('"current_tier" BETWEEN 0 AND 4');
    }
  });

  it("keeps the multi-discount migration schema-only, additive, and detached from historical DDL", () => {
    const migrationName = "0022_multi_discount_expand.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as {
      entries: Array<{
        idx: number;
        version: string;
        when: number;
        tag: string;
        breakpoints: boolean;
      }>;
    };
    const previousEntry = journal.entries.find((entry) => entry.idx === 21);
    const currentEntry = journal.entries.find((entry) => entry.idx === 22);

    expect(currentEntry).toMatchObject({
      idx: 22,
      version: "7",
      tag: "0022_multi_discount_expand",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);

    expect(migrationSql).not.toMatch(
      /^(?:ALTER TYPE|CREATE TYPE|CREATE FUNCTION|CREATE TRIGGER|DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im,
    );
    for (const historicalObject of [
      "email_outbox_status",
      "signup_profiles",
      "device_sightings",
      "admin_account_domains",
      "customer_profiles",
    ]) {
      expect(migrationSql).not.toContain(historicalObject);
    }

    const createdTables = [...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm)]
      .map((match) => match[1])
      .sort();
    expect(createdTables).toEqual([...MULTI_DISCOUNT_TABLES].sort());

    const alteredTables = [...migrationSql.matchAll(/^ALTER TABLE "([^"]+)"/gm)]
      .map((match) => match[1]);
    expect(alteredTables.every((table) =>
      MULTI_DISCOUNT_TABLES.includes(table as typeof MULTI_DISCOUNT_TABLES[number])
    )).toBe(true);
    expect(alteredTables.length).toBeGreaterThan(0);

    const indexedTables = [
      ...migrationSql.matchAll(/^CREATE (?:UNIQUE )?INDEX "[^"]+" ON "([^"]+)"/gm),
    ].map((match) => match[1]);
    expect(indexedTables.every((table) =>
      MULTI_DISCOUNT_TABLES.includes(table as typeof MULTI_DISCOUNT_TABLES[number])
    )).toBe(true);
    expect(indexedTables.length).toBeGreaterThan(0);

    const unsupportedStatements = migrationSql
      .split("--> statement-breakpoint")
      .map((statement) => statement.trim())
      .filter(Boolean)
      .filter((statement) =>
        !/^CREATE TABLE "[^"]+"/s.test(statement)
        && !/^ALTER TABLE "[^"]+" ADD CONSTRAINT /s.test(statement)
        && !/^CREATE (?:UNIQUE )?INDEX "[^"]+" ON "[^"]+"/s.test(statement)
      );
    expect(unsupportedStatements).toEqual([]);

    const databaseObjectNames = [
      ...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migrationSql.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migrationSql.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });

  it("captures manual 0018-0021 changes and the complete multi-discount schema snapshot", () => {
    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0022_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        name: string;
        schema: string;
        columns: Record<string, unknown>;
        foreignKeys: Record<string, unknown>;
        checkConstraints: Record<string, { value: string }>;
      }>;
      enums: Record<string, { values: string[] }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0017_snapshot.json"), "utf8"),
    ) as { id: string; tables: Record<string, unknown> };

    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.enums["public.email_outbox_status"]?.values).toContain("canceled");
    expect(snapshot.tables).toHaveProperty("public.signup_profiles");
    expect(snapshot.tables).toHaveProperty("public.device_sightings");
    expect(
      snapshot.tables["public.customer_profiles"]
        ?.checkConstraints["customer_profiles_referral_floor_check"]?.value,
    ).toContain("BETWEEN 0 AND 9500");
    expect(
      snapshot.tables["public.admin_account_domains"]
        ?.checkConstraints["admin_account_domains_domain_check"]?.value,
    ).toContain("monitoring.apitoken.sale");

    const addedTables = Object.keys(snapshot.tables)
      .filter((table) => !(table in previousSnapshot.tables))
      .sort();
    expect(addedTables).toEqual([
      "public.device_sightings",
      ...MULTI_DISCOUNT_TABLES.map((table) => `public.${table}`),
      "public.signup_profiles",
    ].sort());

    for (const table of MULTI_DISCOUNT_TABLES) {
      const snapshotTable = snapshot.tables[`public.${table}`];
      expect(snapshotTable).toMatchObject({ name: table, schema: "" });
      expect(Object.keys(snapshotTable?.columns ?? {}).length).toBeGreaterThan(0);
    }

    expect(
      snapshot.tables["public.account_policy_bindings"]?.foreignKeys,
    ).toHaveProperty("account_policy_bindings_desired_fk");
    expect(
      snapshot.tables["public.account_policy_bindings"]?.foreignKeys,
    ).toHaveProperty("account_policy_bindings_applied_fk");
  });

  it("keeps the pre-writer invariant fixup narrow and schema-only", () => {
    const migrationName = "0023_multi_discount_invariants.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as {
      entries: Array<{
        idx: number;
        version: string;
        when: number;
        tag: string;
        breakpoints: boolean;
      }>;
    };
    const previousEntry = journal.entries.find((entry) => entry.idx === 22);
    const currentEntry = journal.entries.find((entry) => entry.idx === 23);

    expect(currentEntry).toMatchObject({
      idx: 23,
      version: "7",
      tag: "0023_multi_discount_invariants",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);

    expect(migrationSql).toContain("multi_discount_invariants_empty_preflight");
    expect(migrationSql).toContain("SHARE ROW EXCLUSIVE MODE NOWAIT");
    for (const table of MULTI_DISCOUNT_TABLES) {
      expect(migrationSql).toContain(`'${table}'`);
    }

    expect(migrationSql).not.toMatch(
      /^(?:CREATE FUNCTION|CREATE TRIGGER|CREATE CONSTRAINT TRIGGER|INSERT|UPDATE|DELETE|TRUNCATE|DROP TABLE|DROP COLUMN)\b/im,
    );

    const alteredTables = [...migrationSql.matchAll(/^ALTER TABLE "([^"]+)"/gm)]
      .map((match) => match[1]);
    expect(new Set(alteredTables)).toEqual(new Set([
      "account_policy_bindings",
      "pricing_usage_attributions",
      "pricing_usage_funding_allocations",
      "provider_switch_entries",
      "provider_switch_versions",
    ]));

    const unsupportedStatements = migrationSql
      .split("--> statement-breakpoint")
      .map((statement) => statement.trim())
      .filter(Boolean)
      .filter((statement) =>
        !/^(?:--[^\n]*\n)*DO \$block\$/s.test(statement)
        && !/^ALTER TABLE "[^"]+" (?:ADD|DROP|ALTER) /s.test(statement)
      );
    expect(unsupportedStatements).toEqual([]);

    const databaseObjectNames = [
      ...migrationSql.matchAll(/CONSTRAINT "([^"]+)"/g),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0023_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, unknown>;
        foreignKeys: Record<string, unknown>;
        checkConstraints: Record<string, { value: string }>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0022_snapshot.json"), "utf8"),
    ) as { id: string };

    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.provider_switch_versions"]?.columns)
      .toHaveProperty("capability_generation");
    expect(snapshot.tables["public.provider_switch_versions"]?.columns)
      .toHaveProperty("capability_digest");
    expect(snapshot.tables["public.provider_switch_entries"]?.columns)
      .toHaveProperty("catalog_generation");
    expect(snapshot.tables["public.pricing_usage_attributions"]?.columns)
      .toHaveProperty("binding_id");
    expect(snapshot.tables["public.pricing_usage_attributions"]?.columns)
      .toHaveProperty("effective_policy_digest");
    expect(snapshot.tables["public.pricing_usage_attributions"]?.foreignKeys)
      .toHaveProperty("pricing_usage_attributions_effective_fk");
    expect(snapshot.tables["public.pricing_usage_attributions"]?.checkConstraints)
      .toHaveProperty("pricing_usage_attributions_policy_funding_check");
    expect(
      snapshot.tables["public.pricing_usage_funding_allocations"]
        ?.columns["engine_bucket_id"],
    ).toMatchObject({ notNull: true });
  });

  it("expands immutable usage lineage before the Stage 10 history writer", () => {
    const migrationSql = readFileSync(
      join(MIGRATIONS_FOLDER, "0025_multi_discount_history_expand.sql"),
      "utf8",
    );
    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0025_snapshot.json"), "utf8"),
    ) as {
      tables: Record<string, { columns: Record<string, { notNull: boolean }> }>;
    };
    const expectedColumns = [
      "source_policy_digest",
      "admission_catalog_generation",
      "admission_catalog_digest",
      "admission_switch_generation",
      "admission_switch_digest",
      "runtime_manifest_generation",
      "runtime_manifest_digest",
    ];

    expect(migrationSql).not.toMatch(/^(?:DROP|UPDATE|DELETE|TRUNCATE)\b/im);
    for (const column of expectedColumns) {
      expect(migrationSql).toContain(`ADD COLUMN "${column}"`);
      expect(snapshot.tables["public.pricing_usage_attributions"]?.columns[column])
        .toMatchObject({ notNull: false });
    }
  });

  it("expands the one-head pricing release schema without activating or preserving legacy pricing semantics", () => {
    const migrationName = "0026_pricing_release_expand.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 25);
    const currentEntry = journal.entries.find((entry) => entry.idx === 26);

    expect(currentEntry).toMatchObject({
      idx: 26,
      version: "7",
      tag: "0026_pricing_release_expand",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);

    expect(migrationSql).not.toMatch(/^(?:DROP|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migrationSql).not.toMatch(/\b(?:track|tier|retention)\b/i);
    for (const scope of ["global", "provider", "model"]) {
      expect(migrationSql).toContain(`"scope_type" = '${scope}'`);
    }
    expect(migrationSql).toContain("'meter_only'");
    expect(migrationSql).toContain("'activate_release'");
    expect(migrationSql).toContain("'activate_recovery'");
    expect(migrationSql).toContain("pricing_release_activation_receipts_v2_head_unique");

    const createdTables = [...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm)]
      .map((match) => match[1])
      .sort();
    expect(createdTables).toEqual([...PRICING_RELEASE_V2_TABLES].sort());

    const alteredTables = [...migrationSql.matchAll(/^ALTER TABLE "([^"]+)"/gm)]
      .map((match) => match[1]);
    expect(new Set(alteredTables)).toEqual(new Set(["pricing_release_control_jobs_v2"]));

    const databaseObjectNames = [
      ...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migrationSql.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migrationSql.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
      ...migrationSql.matchAll(/^CREATE FUNCTION "([^"]+)"/gm),
      ...migrationSql.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0026_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, { notNull: boolean }>;
        foreignKeys: Record<string, unknown>;
        uniqueConstraints: Record<string, unknown>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0025_snapshot.json"), "utf8"),
    ) as { id: string };

    expect(snapshot.prevId).toBe(previousSnapshot.id);
    for (const table of PRICING_RELEASE_V2_TABLES) {
      expect(snapshot.tables).toHaveProperty(`public.${table}`);
    }
    const servicePolicy = snapshot.tables["public.pricing_policy_documents_v2"]!;
    for (const column of [
      "product_id",
      "catalog_generation",
      "catalog_digest",
      "switch_generation",
      "switch_digest",
    ]) {
      expect(servicePolicy.columns[column]).toMatchObject({ notNull: false });
    }
    expect(
      snapshot.tables["public.pricing_release_control_jobs_v2"]?.foreignKeys,
    ).toHaveProperty("pricing_release_control_jobs_v2_evidence_fk");
    expect(
      snapshot.tables["public.pricing_release_activation_receipts_v2"]?.uniqueConstraints,
    ).toHaveProperty("pricing_release_activation_receipts_v2_head_unique");
  });

  it("expands funding normalization storage for exact blocked plans without fabricated targets", () => {
    const migrationName = "0027_funding_normalization_blockers.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 26);
    const currentEntry = journal.entries.find((entry) => entry.idx === 27);

    expect(currentEntry).toMatchObject({
      idx: 27,
      version: "7",
      tag: "0027_funding_normalization_blockers",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|DROP TABLE)\b/im);
    expect(migrationSql).toContain('ALTER COLUMN "funding_generation" DROP NOT NULL');
    expect(migrationSql).toContain('ALTER COLUMN "target_funding_digest" DROP NOT NULL');
    expect(migrationSql).toContain('ADD COLUMN "normalization_source" text');
    expect(migrationSql).toContain('ADD COLUMN "blockers" jsonb');
    expect(migrationSql).toContain('"status" = \'ready\'');
    expect(migrationSql).toContain('"applied_funding_digest" = "pricing_funding_normalizations_v2"."target_funding_digest"');

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0027_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, { columns: Record<string, { notNull: boolean; type: string }> }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0026_snapshot.json"), "utf8"),
    ) as { id: string };
    const funding = snapshot.tables["public.pricing_funding_normalizations_v2"]!;

    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(funding.columns.funding_generation).toMatchObject({ notNull: false });
    expect(funding.columns.target_funding_digest).toMatchObject({ notNull: false });
    expect(funding.columns.normalization_source).toMatchObject({ notNull: false, type: "text" });
    expect(funding.columns.blockers).toMatchObject({ notNull: false, type: "jsonb" });
  });

  it("adds dormant exact Stage 5 inventory, blocker, and prepare ACK evidence", () => {
    const migrationName = "0028_pricing_stage5_evidence.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 27);
    const currentEntry = journal.entries.find((entry) => entry.idx === 28);

    expect(currentEntry).toMatchObject({
      idx: 28,
      version: "7",
      tag: "0028_pricing_stage5_evidence",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migrationSql).not.toMatch(/\b(?:track|tier|retention)\b/i);

    const createdTables = [...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm)]
      .map((match) => match[1])
      .sort();
    expect(createdTables).toEqual([...PRICING_STAGE5_EVIDENCE_TABLES].sort());
    expect(migrationSql).toContain(
      '"engine_scan_second_digest" = "pricing_stage5_runs_v2"."engine_scan_first_digest"',
    );
    expect(migrationSql).toContain(
      '"openkeys_scan_second_digest" = "pricing_stage5_runs_v2"."openkeys_scan_first_digest"',
    );
    expect(migrationSql).toContain(
      '"readback_digest" = "pricing_stage5_prepare_acks_v2"."expected_digest"',
    );
    expect(migrationSql).toContain("'blocked', 'planned', 'materializing', 'prepared', 'failed'");

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0028_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, { notNull: boolean; type: string }>;
        foreignKeys: Record<string, unknown>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0027_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    for (const table of PRICING_STAGE5_EVIDENCE_TABLES) {
      expect(snapshot.tables).toHaveProperty(`public.${table}`);
    }
    expect(snapshot.tables["public.pricing_stage5_runs_v2"]!.columns.inventory_artifact)
      .toMatchObject({ notNull: true, type: "jsonb" });
    expect(snapshot.tables["public.pricing_stage5_runs_v2"]!.columns.plan_artifact)
      .toMatchObject({ notNull: true, type: "jsonb" });
    expect(snapshot.tables["public.pricing_stage5_blockers_v2"]!.foreignKeys)
      .toHaveProperty("pricing_stage5_blockers_v2_run_fk");
    expect(snapshot.tables["public.pricing_stage5_prepare_acks_v2"]!.foreignKeys)
      .toHaveProperty("pricing_stage5_prepare_acks_v2_run_fk");
  });

  it("expands pricing release storage for guarded two-phase funding finalization", () => {
    const migrationName = "0029_pricing_release_two_phase_finalize.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 28);
    const currentEntry = journal.entries.find((entry) => entry.idx === 29);

    expect(currentEntry).toMatchObject({
      idx: 29,
      version: "7",
      tag: "0029_pricing_release_two_phase_finalize",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:INSERT|UPDATE|DELETE|TRUNCATE|DROP TABLE)\b/im);
    expect(migrationSql).toContain('ALTER COLUMN "funding_manifest_digest" DROP NOT NULL');
    expect(migrationSql).toContain('ALTER COLUMN "engine_release_digest" DROP NOT NULL');
    expect(migrationSql).toContain('ALTER COLUMN "target_digest" DROP NOT NULL');
    expect(migrationSql).toContain('ALTER COLUMN "recovery_digest" DROP NOT NULL');
    expect(migrationSql).toContain('CREATE FUNCTION "guard_pricing_release_assignment_v2"()');
    expect(migrationSql).toContain('CREATE FUNCTION "guard_pricing_release_plan_v2"()');
    expect(migrationSql).toContain('CREATE FUNCTION "guard_pricing_stage5_run_v2"()');
    expect(migrationSql).toContain("OLD.\"funding_generation\" IS NULL");
    expect(migrationSql).toContain("normalization.\"status\" <> 'ready'");

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0029_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, { notNull: boolean; type: string }>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0028_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.pricing_release_plans_v2"]!.columns.funding_manifest_digest)
      .toMatchObject({ notNull: false, type: "text" });
    expect(snapshot.tables["public.pricing_release_plans_v2"]!.columns.engine_release_digest)
      .toMatchObject({ notNull: false, type: "text" });
    expect(snapshot.tables["public.pricing_stage5_runs_v2"]!.columns.target_digest)
      .toMatchObject({ notNull: false, type: "text" });
    expect(snapshot.tables["public.pricing_stage5_runs_v2"]!.columns.recovery_digest)
      .toMatchObject({ notNull: false, type: "text" });
  });

  it("keeps Stage 8 legacy inflight observable without requiring a traffic drain", () => {
    const migrationName = "0030_pricing_stage8_zero_drain.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 29);
    const currentEntry = journal.entries.find((entry) => entry.idx === 30);

    expect(currentEntry).toMatchObject({
      idx: 30,
      version: "7",
      tag: "0030_pricing_stage8_zero_drain",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).toContain(
      'DROP CONSTRAINT "pricing_stage8_evidence_v2_shape_check"',
    );
    expect(migrationSql).toContain('"legacy_inflight_count" >= 0');
    expect(migrationSql).toContain(
      '"passed" AND "pricing_stage8_evidence_v2"."blocker_count" = 0',
    );
    expect(migrationSql).not.toContain(
      '"passed" AND "pricing_stage8_evidence_v2"."legacy_inflight_count" = 0',
    );

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0030_snapshot.json"), "utf8"),
    ) as { prevId: string };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0029_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
  });

  it("adds dormant nullable activation evidence, request, and receipt capture", () => {
    const migrationName = "0031_pricing_activation_evidence_capture.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 30);
    const currentEntry = journal.entries.find((entry) => entry.idx === 31);

    expect(currentEntry).toMatchObject({
      idx: 31,
      version: "7",
      tag: "0031_pricing_activation_evidence_capture",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:CREATE|DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migrationSql).not.toMatch(/\b(?:SET NOT NULL|DEFAULT)\b/i);

    const expectedStatements = [
      'ALTER TABLE "pricing_release_activation_receipts_v2" ADD COLUMN "receipt_payload" jsonb;',
      'ALTER TABLE "pricing_release_control_jobs_v2" ADD COLUMN "activation_payload" jsonb;',
      'ALTER TABLE "pricing_stage8_evidence_v2" ADD COLUMN "engine_evidence_digest" text;',
      'ALTER TABLE "pricing_stage8_evidence_v2" ADD COLUMN "engine_captured_at" timestamp with time zone;',
    ];
    const statements = migrationSql
      .split("--> statement-breakpoint")
      .map((statement) => statement.trim())
      .filter(Boolean);
    expect(statements).toEqual(expectedStatements);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0031_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, { columns: Record<string, { notNull: boolean; type: string }> }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0030_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.pricing_stage8_evidence_v2"]!.columns.engine_evidence_digest)
      .toMatchObject({ notNull: false, type: "text" });
    expect(snapshot.tables["public.pricing_stage8_evidence_v2"]!.columns.engine_captured_at)
      .toMatchObject({ notNull: false, type: "timestamp with time zone" });
    expect(snapshot.tables["public.pricing_release_control_jobs_v2"]!.columns.activation_payload)
      .toMatchObject({ notNull: false, type: "jsonb" });
    expect(snapshot.tables["public.pricing_release_activation_receipts_v2"]!.columns.receipt_payload)
      .toMatchObject({ notNull: false, type: "jsonb" });
  });

  it("adds dormant managed Stage 8 capture jobs with append-only raw artifacts", () => {
    const migrationName = "0033_pricing_stage8_managed_capture.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 32);
    const currentEntry = journal.entries.find((entry) => entry.idx === 33);

    expect(currentEntry).toMatchObject({
      idx: 33,
      version: "7",
      tag: "0033_pricing_stage8_managed_capture",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migrationSql).not.toMatch(/\b(?:track|tier|retention)\b/i);
    expect(
      [...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm)].map((match) => match[1]).sort(),
    ).toEqual([
      "pricing_stage8_capture_artifacts_v2",
      "pricing_stage8_capture_jobs_v2",
    ]);
    expect(migrationSql).toContain("'pending', 'processing', 'retry', 'passed', 'blocked', 'dead'");
    expect(migrationSql).toContain('"engine_payload_json" text NOT NULL');
    expect(migrationSql).toContain('"combined_payload_json" text');
    expect(migrationSql).toContain("'stored', 'unchanged', 'not_persisted'");
    expect(migrationSql).toContain('CONSTRAINT "pricing_stage8_capture_artifacts_v2_job_fk"');

    const databaseObjectNames = [
      ...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migrationSql.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migrationSql.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0033_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, { notNull: boolean; type: string }>;
        foreignKeys: Record<string, unknown>;
        indexes: Record<string, unknown>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0032_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.pricing_stage8_capture_jobs_v2"]!.columns.request_digest)
      .toMatchObject({ notNull: true, type: "text" });
    expect(snapshot.tables["public.pricing_stage8_capture_artifacts_v2"]!.columns.engine_payload_json)
      .toMatchObject({ notNull: true, type: "text" });
    expect(snapshot.tables["public.pricing_stage8_capture_artifacts_v2"]!.columns.combined_payload_json)
      .toMatchObject({ notNull: false, type: "text" });
    expect(snapshot.tables["public.pricing_stage8_capture_artifacts_v2"]!.foreignKeys)
      .toHaveProperty("pricing_stage8_capture_artifacts_v2_job_fk");
    expect(snapshot.tables["public.pricing_stage8_capture_jobs_v2"]!.indexes)
      .toHaveProperty("pricing_stage8_capture_jobs_v2_claim_idx");
  });

  it("adds dormant full-inventory shadow rollout jobs without activating pricing", () => {
    const migrationName = "0035_pricing_shadow_rollout_jobs.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 34);
    const currentEntry = journal.entries.find((entry) => entry.idx === 35);

    expect(currentEntry).toMatchObject({
      idx: 35,
      version: "7",
      tag: "0035_pricing_shadow_rollout_jobs",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql).not.toMatch(/^(?:DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migrationSql).not.toMatch(/\b(?:track|tier|retention)\b/i);
    expect(
      [...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm)].map((match) => match[1]).sort(),
    ).toEqual([
      "pricing_shadow_policy_jobs_v2",
      "pricing_shadow_rollouts_v2",
    ]);
    expect(migrationSql).toContain("pricing_shadow_rollouts_v2_target_fk");
    expect(migrationSql).toContain("pricing_shadow_rollouts_v2_recovery_fk");
    expect(migrationSql).toContain("pricing_shadow_policy_jobs_v2_rollout_fk");
    expect(migrationSql).toContain("'pending', 'processing', 'retry', 'confirmed', 'blocked', 'dead'");
    expect(migrationSql).toContain('jsonb_typeof("pricing_shadow_policy_jobs_v2"."request_payload") = \'object\'');

    const databaseObjectNames = [
      ...migrationSql.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migrationSql.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migrationSql.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0035_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, {
        columns: Record<string, { notNull: boolean; type: string }>;
        foreignKeys: Record<string, unknown>;
        indexes: Record<string, unknown>;
      }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0034_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.pricing_shadow_rollouts_v2"]!.foreignKeys)
      .toHaveProperty("pricing_shadow_rollouts_v2_target_fk");
    expect(snapshot.tables["public.pricing_shadow_rollouts_v2"]!.foreignKeys)
      .toHaveProperty("pricing_shadow_rollouts_v2_recovery_fk");
    expect(snapshot.tables["public.pricing_shadow_policy_jobs_v2"]!.columns.request_payload)
      .toMatchObject({ notNull: true, type: "jsonb" });
    expect(snapshot.tables["public.pricing_shadow_policy_jobs_v2"]!.indexes)
      .toHaveProperty("pricing_shadow_policy_jobs_v2_claim_idx");
  });

  it("adds nullable provider evidence to pricing usage events without rewriting history", () => {
    const migrationName = "0036_pricing_usage_provider_attribution.sql";
    const migrationSql = readFileSync(join(MIGRATIONS_FOLDER, migrationName), "utf8");
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; version: string; when: number; tag: string; breakpoints: boolean }> };
    const previousEntry = journal.entries.find((entry) => entry.idx === 35);
    const currentEntry = journal.entries.find((entry) => entry.idx === 36);

    expect(currentEntry).toMatchObject({
      idx: 36,
      version: "7",
      tag: "0036_pricing_usage_provider_attribution",
      breakpoints: true,
    });
    expect(currentEntry!.when).toBeGreaterThan(previousEntry!.when);
    expect(migrationSql.trim()).toBe(
      'ALTER TABLE "pricing_usage_events" ADD COLUMN "provider_id" text;',
    );
    expect(migrationSql).not.toMatch(/^(?:DROP|INSERT|UPDATE|DELETE|TRUNCATE)\b/im);

    const snapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0036_snapshot.json"), "utf8"),
    ) as {
      prevId: string;
      tables: Record<string, { columns: Record<string, { notNull: boolean; type: string }> }>;
    };
    const previousSnapshot = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "0035_snapshot.json"), "utf8"),
    ) as { id: string };
    expect(snapshot.prevId).toBe(previousSnapshot.id);
    expect(snapshot.tables["public.pricing_usage_events"]!.columns.provider_id)
      .toMatchObject({ notNull: false, type: "text" });
  });
});
