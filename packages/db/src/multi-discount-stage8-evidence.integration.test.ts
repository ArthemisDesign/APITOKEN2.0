import { Buffer } from "node:buffer";
import { randomUUID } from "node:crypto";
import type {
  FundingNormalizationPlanV2,
  PricingReleaseAssignmentExtensionV2,
  PricingReleaseHeadV2,
  PricingReleaseInventoryAccountV2,
} from "@claude-api/contracts";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  buildStage5ServiceInventoryV2,
  claimNextPricingReleaseActivationJobV2,
  collectStage8CombinedEvidenceV2,
  confirmPricingReleaseActivationJobV2,
  createDatabase,
  recoverStalePricingReleaseActivationJobsV2,
  stagePricingReleaseActivationJobV2,
  stage5V2CommerceInventoryDigest,
  stage5V2Digest,
  stage5V2EngineIdentityDigest,
  stage8EngineEvidenceDigestV2,
  type PricingReleaseActivationAuthorityReadersV2,
  type Stage5V2OpenKeysReader,
  type Stage8EngineEvidenceV2,
} from "./index.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const TARGET_GENERATION = 80_101n;
const RECOVERY_GENERATION = 80_102n;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function digest(label: string): string {
  return stage5V2Digest("stage8-integration", label);
}

interface AuthorityHarness {
  readers: PricingReleaseActivationAuthorityReadersV2;
  accounts: PricingReleaseInventoryAccountV2[];
  extensions: Map<string, PricingReleaseAssignmentExtensionV2>;
  funding: Map<string, FundingNormalizationPlanV2>;
  setHead(head: PricingReleaseHeadV2 | null): void;
}

function authorityHarness(
  accounts: PricingReleaseInventoryAccountV2[],
  openkeys: Stage5V2OpenKeysReader,
): AuthorityHarness {
  let head: PricingReleaseHeadV2 | null = null;
  const extensions = new Map<string, PricingReleaseAssignmentExtensionV2>();
  const funding = new Map<string, FundingNormalizationPlanV2>();
  return {
    accounts,
    extensions,
    funding,
    setHead: (next) => { head = next; },
    readers: {
      openkeys,
      engine: {
        getPricingReleaseHeadV2: async () => structuredClone(head),
        getPricingReleaseInventoryV2: async (options = {}) => {
          const { afterAccountId, limit = 500 } = options;
          const ordered = [...accounts].sort((left, right) =>
            Buffer.compare(Buffer.from(left.account_id), Buffer.from(right.account_id)));
          const remaining = afterAccountId === undefined
            ? ordered
            : ordered.filter((account) => Buffer.compare(
              Buffer.from(account.account_id),
              Buffer.from(afterAccountId),
            ) > 0);
          const page = remaining.slice(0, limit);
          return {
            accounts: structuredClone(page),
            next_after_account_id: remaining.length > page.length ? page.at(-1)!.account_id : null,
          };
        },
        getPricingReleaseAssignmentExtensionV2: async (headVersion, accountId) =>
          structuredClone(extensions.get(`${headVersion}:${accountId}`) ?? null),
        getFundingNormalizationPlanV2: async (accountId) =>
          structuredClone(funding.get(accountId) ?? null),
      },
    },
  };
}

function engineEvidence(input: {
  engineInventoryDigest: string;
  fundingDigest: string;
  targetEngineDigest: string;
  recoveryEngineDigest: string;
  passed?: boolean;
  legacyInflightReservations?: bigint;
  legacyInflightOutboxRows?: bigint;
  activeHead?: Stage8EngineEvidenceV2["release"]["active_head"];
}): Stage8EngineEvidenceV2 {
  const captured = BigInt(Math.floor(Date.now() / 1_000));
  const passed = input.passed ?? true;
  const legacyInflightReservations = input.legacyInflightReservations ?? 0n;
  const legacyInflightOutboxRows = input.legacyInflightOutboxRows ?? 0n;
  const report: Stage8EngineEvidenceV2 = {
    schema_version: 2n,
    captured_ts: captured,
    window_start_ts: captured - 100n,
    window_end_ts: captured - 10n,
    min_samples_per_provider: 1n,
    gemini_client_admissions: 1n,
    passed,
    release: {
      target_generation: TARGET_GENERATION,
      target_digest: input.targetEngineDigest,
      recovery_generation: RECOVERY_GENERATION,
      recovery_digest: input.recoveryEngineDigest,
      recovery_link_digest: digest("recovery-link"),
      inventory_digest: input.engineInventoryDigest,
      funding_digest: input.fundingDigest,
      target_assignment_count: 1n,
      recovery_assignment_count: 1n,
      active_head: input.activeHead ?? null,
    },
    runtime_manifest: {
      generation: 3n,
      digest: digest("runtime-manifest"),
      capabilities: [{
        schema_version: 2n,
        generation: 3n,
        digest: digest("capability"),
      }],
    },
    catalogs: [
      {
        product_id: "main",
        generation: 3n,
        schema_version: 2n,
        capability_generation: 3n,
        capability_digest: digest("capability"),
        content_digest: digest("main-catalog"),
        enabled_entries: 3n,
      },
      {
        product_id: "openkeys",
        generation: 3n,
        schema_version: 2n,
        capability_generation: 3n,
        capability_digest: digest("capability"),
        content_digest: digest("openkeys-catalog"),
        enabled_entries: 2n,
      },
    ],
    switches: {
      generation: 3n,
      schema_version: 2n,
      capability_generation: 3n,
      capability_digest: digest("capability"),
      content_digest: digest("switches"),
      entries: 14n,
    },
    counts: {
      total_accounts: 1n,
      active_accounts: 1n,
      account_classes: { b2c: 1n },
      reconciled_accounts: 1n,
      snapshots_by_provider: { anthropic: 1n, google: 1n, openai: 1n },
      evaluations_by_outcome: { resolved: 3n },
      comparisons: { different: 3n },
      scalar_parity_rows: 0n,
      policy_divergence_rows: 3n,
      gemini_usage_rows: 1n,
      gemini_outbox_rows: 1n,
      live_runtime_instances: 2n,
      release_capable_runtime_instances: 2n,
      legacy_inflight_reservations: legacyInflightReservations,
      legacy_inflight_outbox_rows: legacyInflightOutboxRows,
    },
    financial_samples: [],
    engine_inventory_digest: input.engineInventoryDigest,
    funding_digest: input.fundingDigest,
    shadow_digest: digest("shadow"),
    runtime_floor_digest: digest("runtime-floor"),
    legacy_inflight_count: legacyInflightReservations + legacyInflightOutboxRows,
    blockers: passed ? [] : [{
      code: "live_runtime_below_release_v2_floor",
      count: 1n,
      subject_digests: [`sha256:v1:${"1".repeat(64)}`],
    }],
    evidence_digest: `sha256:v2:${"0".repeat(64)}`,
  };
  report.evidence_digest = stage8EngineEvidenceDigestV2(report);
  return report;
}

describe.runIf(Boolean(connectionString))("Stage 8 combined commerce evidence", () => {
  let admin: Client;
  let seed: Client;
  let databaseName: string;
  let databaseUrl: string;

  beforeAll(async () => {
    databaseName = `stage8_v2_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    databaseUrl = url.toString();
    seed = new Client({ connectionString: databaseUrl });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await seed?.end();
    if (admin) {
      await admin.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await admin.query(`DROP DATABASE IF EXISTS ${quoteIdentifier(databaseName)}`);
      await admin.end();
    }
  }, TEST_TIMEOUT_MS);

  beforeEach(async () => {
    const tables = await seed.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    await seed.query(
      `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
    );
  });

  async function seedPreparedPair(options: { recoveryOwnerDrift?: boolean } = {}): Promise<{
    report: Stage8EngineEvidenceV2;
    openkeys: Stage5V2OpenKeysReader;
    authority: AuthorityHarness;
    accountId: string;
    userId: string;
  }> {
    const userId = randomUUID();
    const recordId = randomUUID();
    const accountId = "acct_stage8_combined_b2c";
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'Stage 8 combined', true, 'active')
    `, [userId, `${userId}@example.test`]);
    await seed.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES ($1, 'b2c', 0, 4000, now())
    `, [userId]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 4000, 'active')
    `, [recordId, userId, accountId]);

    const commerceDigest = stage5V2CommerceInventoryDigest({
      accounts: [{
        user_id: userId,
        engine_account_record_id: recordId,
        engine_account_id: accountId,
        account_class: "b2c",
        profile_multiplier_bp: 4_000,
        commerce_multiplier_bp: 4_000,
        commerce_status: "active",
      }],
      invitations: [],
    });
    const serviceDigest = buildStage5ServiceInventoryV2([]).inventory_digest;
    const engineAccount: PricingReleaseInventoryAccountV2 = {
      account_id: accountId,
      status: "active",
      multiplier_bp: 4_000,
      balance_nano: "5000000000",
      reserved_nano: "0",
      spent_nano: "0",
      funding_generation: 7,
      funding_head_version: 1,
    };
    const engineInventoryDigest = stage5V2EngineIdentityDigest([engineAccount]);
    const openkeysDigest = digest("openkeys-inventory-empty");
    const fundingDigest = digest("funding-manifest");
    const targetPlanDigest = digest("target-plan");
    const recoveryPlanDigest = digest("recovery-plan");
    const targetEngineDigest = digest("target-engine-release");
    const recoveryEngineDigest = digest("recovery-engine-release");
    const policyDigest = digest("policy");
    await seed.query(`
      INSERT INTO pricing_policy_documents_v2 (
        policy_id, policy_version, owner_type, owner_id, account_class,
        product_id, billing_mode, schema_version,
        capability_generation, capability_digest,
        catalog_generation, catalog_digest, switch_generation, switch_digest,
        content_digest
      ) VALUES (
        'release-v2:b2c:global', 1, 'global_b2c', 'global', 'b2c',
        'main', 'balance', 2, 3, $1, 3, $2, 3, $3, $4
      )
    `, [digest("capability"), digest("main-catalog"), digest("switches"), policyDigest]);
    await seed.query(`
      INSERT INTO pricing_release_plans_v2 (
        generation, release_kind, schema_version,
        commerce_inventory_digest, engine_inventory_digest,
        openkeys_inventory_digest, service_inventory_digest,
        policy_manifest_digest, assignment_manifest_digest,
        funding_manifest_digest, engine_release_digest, content_digest, status
      ) VALUES
        ($1, 'target', 2, $3, $4, $5, $6, $7, $8, NULL, NULL, $10, 'materializing'),
        ($2, 'recovery', 2, $3, $4, $5, $6, $7, $9, NULL, NULL, $11, 'materializing')
    `, [
      TARGET_GENERATION,
      RECOVERY_GENERATION,
      commerceDigest,
      engineInventoryDigest,
      openkeysDigest,
      serviceDigest,
      digest("policy-manifest"),
      digest("target-assignment-manifest"),
      digest("recovery-assignment-manifest"),
      targetPlanDigest,
      recoveryPlanDigest,
    ]);
    for (const generation of [TARGET_GENERATION, RECOVERY_GENERATION]) {
      const ownerId = generation === RECOVERY_GENERATION && options.recoveryOwnerDrift
        ? randomUUID()
        : userId;
      await seed.query(`
        INSERT INTO pricing_release_assignments_v2 (
          release_generation, engine_account_id, account_class, owner_context,
          owner_id, policy_id, policy_version, policy_digest, billing_mode,
          funding_generation, purpose, responsible, assignment_digest
        ) VALUES (
          $1, $2, 'b2c', 'commerce', $3,
          'release-v2:b2c:global', 1, $4, 'balance', NULL, NULL, NULL, $5
        )
      `, [generation, accountId, ownerId, policyDigest, digest(`assignment:${generation}`)]);
      await seed.query(`
        INSERT INTO pricing_funding_normalizations_v2 (
          release_generation, engine_account_id, funding_generation,
          expected_source_digest, target_funding_digest, applied_funding_digest,
          normalization_source, blockers, status
        ) VALUES ($1, $2, 7, $3, $4, $4, 'ledger_replay', NULL, 'ready')
      `, [generation, accountId, digest("funding-source"), digest("account-funding")]);
      await seed.query(`
        UPDATE pricing_release_assignments_v2
        SET funding_generation = 7
        WHERE release_generation = $1 AND engine_account_id = $2
      `, [generation, accountId]);
    }
    await seed.query(`
      UPDATE pricing_release_plans_v2 SET
        funding_manifest_digest = $3,
        engine_release_digest = CASE generation WHEN $1 THEN $4 ELSE $5 END,
        status = 'prepared', updated_at = now()
      WHERE generation IN ($1, $2)
    `, [
      TARGET_GENERATION,
      RECOVERY_GENERATION,
      fundingDigest,
      targetEngineDigest,
      recoveryEngineDigest,
    ]);
    const openkeys: Stage5V2OpenKeysReader = {
      getPage: async () => ({
        inventory_digest: openkeysDigest,
        accounts: [],
        next_after_account_id: null,
      }),
    };
    const authority = authorityHarness([engineAccount], openkeys);
    return {
      accountId,
      userId,
      report: engineEvidence({
        engineInventoryDigest,
        fundingDigest,
        targetEngineDigest,
        recoveryEngineDigest,
      }),
      openkeys,
      authority,
    };
  }

  async function addPostCutoverB2c(
    seeded: Awaited<ReturnType<typeof seedPreparedPair>>,
    options: {
      extension?: "exact" | "missing" | "policy_mismatch";
      fundingHeadMismatch?: boolean;
    } = {},
  ): Promise<{ accountId: string; head: PricingReleaseHeadV2; report: Stage8EngineEvidenceV2 }> {
    const accountId = "acct_stage8_post_cutover_b2c";
    const userId = randomUUID();
    const recordId = randomUUID();
    await seed.query(`
      INSERT INTO users (id, email, display_name, email_verified, status)
      VALUES ($1, $2, 'Stage 8 post-cutover', true, 'active')
    `, [userId, `${userId}@example.test`]);
    await seed.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start
      ) VALUES ($1, 'b2c', 0, 5000, now())
    `, [userId]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 5000, 'active')
    `, [recordId, userId, accountId]);

    const account: PricingReleaseInventoryAccountV2 = {
      account_id: accountId,
      status: "active",
      multiplier_bp: 5_000,
      balance_nano: "9000000000",
      reserved_nano: "1000000000",
      spent_nano: "2000000000",
      funding_generation: 8,
      funding_head_version: 1,
    };
    seeded.authority.accounts.push(account);
    seeded.authority.funding.set(accountId, {
      account_id: accountId,
      account_status: "active",
      status: "normalized",
      source: "stored_generation",
      source_state_digest: digest("post-cutover-funding-source"),
      normalization_digest: digest("post-cutover-funding-generation"),
      funding_generation: 8,
      funding_head_version: options.fundingHeadMismatch ? 2 : 1,
      balance_nano: account.balance_nano,
      reserved_nano: account.reserved_nano,
      spent_nano: account.spent_nano,
      lots: [{
        lot_id: "fundv2_stage8_post_cutover",
        source_type: "paid",
        source_ref: "stage8-post-cutover",
        balance_nano: account.balance_nano,
        reserved_nano: account.reserved_nano,
        spent_nano: account.spent_nano,
        version: 1,
        status: "active",
      }],
      blockers: [],
    });

    const activatedTs = Math.floor(Date.now() / 1_000) - 1;
    const head: PricingReleaseHeadV2 = {
      active_generation: Number(TARGET_GENERATION),
      active_digest: seeded.report.release.target_digest!,
      head_version: 1,
      updated_ts: activatedTs,
    };
    seeded.authority.setHead(head);
    if ((options.extension ?? "exact") !== "missing") {
      const policyDigest = options.extension === "policy_mismatch"
        ? digest("post-cutover-wrong-policy")
        : digest("policy");
      const semantics = {
        account_id: accountId,
        account_class: "b2c" as const,
        policy_id: "release-v2:b2c:global",
        policy_version: 1,
        policy_digest: policyDigest,
        billing_mode: "balance" as const,
        funding_generation: 8,
        purpose: null,
        responsible: null,
      };
      const members = [TARGET_GENERATION, RECOVERY_GENERATION].map((generation) => {
        const assignment = {
          ...semantics,
          assignment_digest: digest(`post-cutover-assignment:${generation}`),
        };
        return {
          release_generation: Number(generation),
          assignment,
          extension_digest: digest(`post-cutover-extension:${generation}`),
        };
      });
      seeded.authority.extensions.set(`1:${accountId}`, {
        provisioning_head_generation: Number(TARGET_GENERATION),
        provisioning_head_digest: seeded.report.release.target_digest!,
        provisioning_head_version: 1,
        paired_recovery_generation: Number(RECOVERY_GENERATION),
        paired_recovery_digest: seeded.report.release.recovery_digest!,
        extension_group_digest: digest("post-cutover-extension-group"),
        members,
      });
    }
    return {
      accountId,
      head,
      report: engineEvidence({
        engineInventoryDigest: seeded.report.engine_inventory_digest,
        fundingDigest: seeded.report.funding_digest,
        targetEngineDigest: seeded.report.release.target_digest!,
        recoveryEngineDigest: seeded.report.release.recovery_digest!,
        activeHead: {
          active_generation: BigInt(head.active_generation),
          active_digest: head.active_digest,
          head_version: BigInt(head.head_version),
          updated_ts: BigInt(head.updated_ts),
        },
      }),
    };
  }

  it("stores one passed identity bound to exact commerce, OpenKeys and engine evidence", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-combined-pass");
    try {
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      expect(report).toMatchObject({
        schema_version: 2,
        passed: true,
        write_result: "stored",
        blocker_count: "0",
        legacy_inflight_count: "0",
      });
      expect(report.evidence_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
      expect(report.source.engine_evidence_digest).toBe(seeded.report.evidence_digest);
      expect(JSON.stringify(report)).not.toContain(seeded.accountId);
      const stored = await seed.query<{
        evidence_digest: string;
        engine_evidence_digest: string;
        engine_captured_at: Date;
        service_inventory_digest: string;
        passed: boolean;
        blocker_count: string;
      }>(`
        SELECT evidence_digest, engine_evidence_digest, engine_captured_at,
               service_inventory_digest, passed, blocker_count::text
        FROM pricing_stage8_evidence_v2
      `);
      expect(stored.rows).toEqual([{
        evidence_digest: report.evidence_digest,
        engine_evidence_digest: seeded.report.evidence_digest,
        engine_captured_at: new Date(Number(seeded.report.captured_ts) * 1_000),
        service_inventory_digest: report.inventories.service_digest,
        passed: true,
        blocker_count: "0",
      }]);
    } finally {
      await database.pool.end();
    }
  });

  it("requires and revalidates the exact persisted service inventory identity", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-service-evidence-authority");
    try {
      const evidence = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      await seed.query(`
        UPDATE pricing_stage8_evidence_v2
        SET service_inventory_digest = NULL
        WHERE evidence_digest = $1
      `, [evidence.evidence_digest]);
      await expect(stagePricingReleaseActivationJobV2(database, {
        activationKind: "cutover",
        evidenceDigest: evidence.evidence_digest,
        operatorId: "pricing-control-worker:service-evidence",
        reason: "reject legacy evidence without exact service authority",
      })).rejects.toThrow("exact persisted service inventory identity");

      await seed.query(`
        UPDATE pricing_stage8_evidence_v2
        SET service_inventory_digest = $2
        WHERE evidence_digest = $1
      `, [evidence.evidence_digest, digest("foreign-service-inventory")]);
      const jobId = await stagePricingReleaseActivationJobV2(database, {
        activationKind: "cutover",
        evidenceDigest: evidence.evidence_digest,
        operatorId: "pricing-control-worker:service-evidence",
        reason: "prove exact service evidence at first delivery",
      });
      await expect(claimNextPricingReleaseActivationJobV2(
        database,
        "activation-service-evidence",
        seeded.authority.readers,
      )).rejects.toThrow("activation authority changed after Stage 8 evidence");
      const stored = await seed.query<{ status: string; last_error: string }>(`
        SELECT status, last_error FROM pricing_release_control_jobs_v2 WHERE id = $1
      `, [jobId]);
      expect(stored.rows[0]).toMatchObject({ status: "dead" });
      expect(stored.rows[0]!.last_error).toContain("service_evidence_authority_drift");
    } finally {
      await database.pool.end();
    }
  });

  it("rejects authority drift immediately before the first activation delivery", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-activation-authority-drift");
    try {
      const evidence = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      const jobId = await stagePricingReleaseActivationJobV2(database, {
        activationKind: "cutover",
        evidenceDigest: evidence.evidence_digest,
        operatorId: "pricing-control-worker:authority-drift",
        reason: "prove the final mutable authority fence",
      });
      await seed.query(`
        UPDATE engine_accounts SET status = 'disabled' WHERE engine_account_id = $1
      `, [seeded.accountId]);

      await expect(claimNextPricingReleaseActivationJobV2(
        database,
        "activation-authority-drift",
        seeded.authority.readers,
      )).rejects.toThrow("activation authority changed after Stage 8 evidence");
      const stored = await seed.query<{ status: string; last_error: string }>(`
        SELECT status, last_error FROM pricing_release_control_jobs_v2 WHERE id = $1
      `, [jobId]);
      expect(stored.rows[0]).toMatchObject({ status: "dead" });
      expect(stored.rows[0]!.last_error).toContain("account_status_authority_drift");
    } finally {
      await database.pool.end();
    }
  });

  it("accepts a post-cutover account only through an exact target/recovery extension", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-post-cutover-extension");
    try {
      const postCutover = await addPostCutoverB2c(seeded);
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        postCutover.report,
      );
      expect(report).toMatchObject({ passed: true, blocker_count: "0", write_result: "stored" });
      expect(JSON.stringify(report)).not.toContain(postCutover.accountId);
    } finally {
      await database.pool.end();
    }
  });

  it.each([
    ["missing extension", { extension: "missing" as const }, "post_cutover_assignment_extension_missing"],
    ["mismatched policy", { extension: "policy_mismatch" as const }, "post_cutover_assignment_extension_drift"],
    ["mismatched funding head", { fundingHeadMismatch: true }, "post_cutover_assignment_extension_drift"],
  ])("rejects a post-cutover account with %s", async (_label, options, blockerCode) => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, `stage8-post-cutover-${blockerCode}`);
    try {
      const postCutover = await addPostCutoverB2c(seeded, options);
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        postCutover.report,
      );
      expect(report.passed).toBe(false);
      expect(report.blockers).toContainEqual(expect.objectContaining({
        source: "commerce",
        code: blockerCode,
      }));
      expect(JSON.stringify(report)).not.toContain(postCutover.accountId);
    } finally {
      await database.pool.end();
    }
  });

  it("durably replays a lost cutover ACK and binds forward recovery to its exact receipt", async () => {
    const seeded = await seedPreparedPair();
    const database = createDatabase(databaseUrl, "stage8-activation-lifecycle");
    try {
      const cutoverEvidence = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      const cutoverJobId = await stagePricingReleaseActivationJobV2(database, {
        activationKind: "cutover",
        evidenceDigest: cutoverEvidence.evidence_digest,
        operatorId: "pricing-control-worker:integration",
        reason: "activate exact prepared Stage 9 target",
      });
      const scalarJobId = randomUUID();
      await seed.query(`
        INSERT INTO engine_pricing_jobs (
          id, user_id, engine_account_id, multiplier_bp, reason
        ) VALUES ($1, $2, $3, 4000, 'activation-race-test')
      `, [scalarJobId, seeded.userId, seeded.accountId]);
      await expect(claimNextPricingReleaseActivationJobV2(
        database,
        "activation-integration-blocked",
        seeded.authority.readers,
      )).resolves.toBeNull();
      const deferred = await seed.query<{ status: string }>(`
        SELECT status FROM pricing_release_control_jobs_v2 WHERE id = $1
      `, [cutoverJobId]);
      expect(deferred.rows[0]?.status).toBe("pending");
      await seed.query(`
        UPDATE engine_pricing_jobs
        SET status = 'confirmed', confirmed_at = now()
        WHERE id = $1
      `, [scalarJobId]);
      const firstClaim = await claimNextPricingReleaseActivationJobV2(
        database,
        "activation-integration-first",
        seeded.authority.readers,
      );
      expect(firstClaim).toMatchObject({
        id: cutoverJobId,
        attempts: 1,
        request: {
          activation_kind: "cutover",
          expectation: "absent",
          evidence: {
            evidence_digest: cutoverEvidence.evidence_digest,
            target_digest: seeded.report.release.target_digest,
            recovery_digest: seeded.report.release.recovery_digest,
          },
        },
      });
      const cutoverActivatedTs = firstClaim!.request.evidence.observed_ts + 1;
      const cutoverHead: PricingReleaseHeadV2 = {
        active_generation: Number(TARGET_GENERATION),
        active_digest: seeded.report.release.target_digest!,
        head_version: 1,
        updated_ts: cutoverActivatedTs,
      };
      // Simulate an applied CAS whose HTTP ACK was lost. Replay must keep the exact stored
      // request even though the mutable head no longer satisfies the original cutover preflight.
      seeded.authority.setHead(cutoverHead);
      await seed.query(`
        UPDATE pricing_release_control_jobs_v2
        SET locked_at = now() - interval '10 minutes'
        WHERE id = $1
      `, [cutoverJobId]);
      await expect(recoverStalePricingReleaseActivationJobsV2(database)).resolves.toBe(1);
      const replayClaim = await claimNextPricingReleaseActivationJobV2(
        database,
        "activation-integration-replay",
        seeded.authority.readers,
      );
      expect(replayClaim).toMatchObject({ id: cutoverJobId, attempts: 2 });
      expect(replayClaim!.request).toEqual(firstClaim!.request);
      const cutoverResultDigest = await confirmPricingReleaseActivationJobV2(
        database,
        replayClaim!,
        "activation-integration-replay",
        {
          result: "unchanged",
          activation: {
            activation_id: "1",
            activation_kind: "cutover",
            from_generation: null,
            from_digest: null,
            expected_head_version: 0,
            head: cutoverHead,
            evidence_digest: cutoverEvidence.evidence_digest,
            operator_id: "pricing-control-worker:integration",
            reason: "activate exact prepared Stage 9 target",
            activated_ts: cutoverActivatedTs,
          },
        },
      );
      expect(cutoverResultDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
      await expect(stagePricingReleaseActivationJobV2(database, {
        activationKind: "cutover",
        evidenceDigest: cutoverEvidence.evidence_digest,
        operatorId: "pricing-control-worker:integration",
        reason: "activate exact prepared Stage 9 target",
      })).resolves.toBe(cutoverJobId);

      const recoverySource = engineEvidence({
        engineInventoryDigest: seeded.report.engine_inventory_digest,
        fundingDigest: seeded.report.funding_digest,
        targetEngineDigest: seeded.report.release.target_digest!,
        recoveryEngineDigest: seeded.report.release.recovery_digest!,
        activeHead: {
          active_generation: TARGET_GENERATION,
          active_digest: seeded.report.release.target_digest!,
          head_version: 1n,
          updated_ts: BigInt(cutoverActivatedTs),
        },
      });
      const recoveryEvidence = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        recoverySource,
      );
      expect(recoveryEvidence.evidence_digest).not.toBe(cutoverEvidence.evidence_digest);
      const recoveryJobId = await stagePricingReleaseActivationJobV2(database, {
        activationKind: "recovery",
        evidenceDigest: recoveryEvidence.evidence_digest,
        operatorId: "pricing-control-worker:integration",
        reason: "activate exact prepared recovery release",
      });
      const recoveryClaim = await claimNextPricingReleaseActivationJobV2(
        database,
        "activation-integration-recovery",
        seeded.authority.readers,
      );
      expect(recoveryClaim).toMatchObject({
        id: recoveryJobId,
        attempts: 1,
        request: {
          activation_kind: "recovery",
          expectation: {
            exact: {
              active_generation: Number(TARGET_GENERATION),
              active_digest: seeded.report.release.target_digest,
              head_version: 1,
              updated_ts: cutoverActivatedTs,
            },
          },
          evidence: { evidence_digest: recoveryEvidence.evidence_digest },
        },
      });
      const recoveryActivatedTs = recoveryClaim!.request.evidence.observed_ts + 1;
      const recoveryResultDigest = await confirmPricingReleaseActivationJobV2(
        database,
        recoveryClaim!,
        "activation-integration-recovery",
        {
          result: "applied",
          activation: {
            activation_id: "2",
            activation_kind: "recovery",
            from_generation: Number(TARGET_GENERATION),
            from_digest: seeded.report.release.target_digest!,
            expected_head_version: 1,
            head: {
              active_generation: Number(RECOVERY_GENERATION),
              active_digest: seeded.report.release.recovery_digest!,
              head_version: 2,
              updated_ts: recoveryActivatedTs,
            },
            evidence_digest: recoveryEvidence.evidence_digest,
            operator_id: "pricing-control-worker:integration",
            reason: "activate exact prepared recovery release",
            activated_ts: recoveryActivatedTs,
          },
        },
      );
      expect(recoveryResultDigest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);
      expect(recoveryResultDigest).not.toBe(cutoverResultDigest);

      const stored = await seed.query<{
        confirmed_jobs: string;
        receipts: string;
        payloads: string;
      }>(`
        SELECT
          (SELECT count(*)::text FROM pricing_release_control_jobs_v2
           WHERE status = 'confirmed' AND result_digest LIKE 'sha256:v2:%') AS confirmed_jobs,
          (SELECT count(*)::text FROM pricing_release_activation_receipts_v2) AS receipts,
          (SELECT count(*)::text FROM pricing_release_activation_receipts_v2
           WHERE receipt_payload IS NOT NULL) AS payloads
      `);
      expect(stored.rows[0]).toEqual({ confirmed_jobs: "2", receipts: "2", payloads: "2" });
    } finally {
      await database.pool.end();
    }
  });

  it("stores passed evidence while preserving nonzero legacy inflight audit counts", async () => {
    const seeded = await seedPreparedPair();
    seeded.report = engineEvidence({
      engineInventoryDigest: seeded.report.engine_inventory_digest,
      fundingDigest: seeded.report.funding_digest,
      targetEngineDigest: seeded.report.release.target_digest!,
      recoveryEngineDigest: seeded.report.release.recovery_digest!,
      legacyInflightReservations: 3n,
      legacyInflightOutboxRows: 2n,
    });
    const database = createDatabase(databaseUrl, "stage8-combined-zero-drain");
    try {
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      expect(report).toMatchObject({
        passed: true,
        write_result: "stored",
        blocker_count: "0",
        legacy_inflight_count: "5",
      });
      const stored = await seed.query<{
        passed: boolean;
        legacy_inflight_count: string;
        blocker_count: string;
      }>(`
        SELECT passed, legacy_inflight_count::text, blocker_count::text
        FROM pricing_stage8_evidence_v2
      `);
      expect(stored.rows).toEqual([{
        passed: true,
        legacy_inflight_count: "5",
        blocker_count: "0",
      }]);
    } finally {
      await database.pool.end();
    }
  });

  it("persists a failed snapshot when commerce inventory changed after release preparation", async () => {
    const seeded = await seedPreparedPair();
    await seed.query("UPDATE engine_accounts SET mult_bp = 4100 WHERE engine_account_id = $1", [seeded.accountId]);
    const database = createDatabase(databaseUrl, "stage8-combined-drift");
    try {
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      expect(report.passed).toBe(false);
      expect(report.write_result).toBe("stored");
      expect(report.blockers.map((blocker) => blocker.code)).toEqual(expect.arrayContaining([
        "target_release_identity_drift",
        "recovery_release_identity_drift",
      ]));
      expect(JSON.stringify(report)).not.toContain(seeded.accountId);
      const stored = await seed.query<{ passed: boolean; blocker_count: string }>(`
        SELECT passed, blocker_count::text FROM pricing_stage8_evidence_v2
      `);
      expect(stored.rows[0]!.passed).toBe(false);
      expect(BigInt(stored.rows[0]!.blocker_count)).toBeGreaterThan(0n);
    } finally {
      await database.pool.end();
    }
  });

  it("rejects target and recovery assignments with different commerce ownership", async () => {
    const seeded = await seedPreparedPair({ recoveryOwnerDrift: true });
    const database = createDatabase(databaseUrl, "stage8-combined-lineage-drift");
    try {
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        seeded.report,
      );
      expect(report.passed).toBe(false);
      expect(report.blockers).toContainEqual(expect.objectContaining({
        source: "commerce",
        code: "target_recovery_commerce_lineage_mismatch",
      }));
    } finally {
      await database.pool.end();
    }
  });

  it("carries engine runtime blockers into immutable combined evidence", async () => {
    const seeded = await seedPreparedPair();
    const blocked = engineEvidence({
      engineInventoryDigest: seeded.report.engine_inventory_digest,
      fundingDigest: seeded.report.funding_digest,
      targetEngineDigest: seeded.report.release.target_digest!,
      recoveryEngineDigest: seeded.report.release.recovery_digest!,
      passed: false,
    });
    const database = createDatabase(databaseUrl, "stage8-combined-engine-blocker");
    try {
      const report = await collectStage8CombinedEvidenceV2(
        database,
        seeded.authority.readers,
        blocked,
      );
      expect(report.passed).toBe(false);
      expect(report.blockers).toContainEqual(expect.objectContaining({
        source: "engine",
        code: "live_runtime_below_release_v2_floor",
      }));
    } finally {
      await database.pool.end();
    }
  });
});
