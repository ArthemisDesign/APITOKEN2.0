import { randomUUID } from "node:crypto";
import { createHash } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type {
  PricingReleaseAssignmentV2,
  PricingReleaseOptOutAckV2,
  PricingReleasePolicyV2,
} from "@claude-api/contracts";
import type { AccountStrictCutoverPreflightTransport } from "@claude-api/engine-client";
import { getAdminPricingBackfillHealth } from "./admin-pipelines.js";
import { createEmailUser } from "./auth.js";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import { runStage5Backfill } from "./multi-discount-backfill.js";
import { convertCustomerToBusiness } from "./pricing.js";
import {
  listPricingBackfillCandidates,
  runPricingBackfillSweep,
  type PricingBackfillReleaseTransport,
} from "./pricing-backfill.js";
import { materializeProvisionedUserPolicy, updateManagedPricingPolicy } from "./pricing-policy-write.js";
import {
  advanceAccountStrictChain,
  listPendingStrictChainAccounts,
  type StrictChainOptOutTransport,
} from "./strict-chain.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function digest(label: string): string {
  return `sha256:v2:${createHash("sha256").update(label, "utf8").digest("hex")}`;
}

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function releasePolicy(input: {
  policyId: string;
  accountClass: "b2c" | "b2b";
  rules: Array<{
    scope: "global" | "provider" | "model";
    providerId?: string;
    canonicalModelId?: string;
    payableMultiplierBp: number;
  }>;
}): PricingReleasePolicyV2 {
  return {
    policy_id: input.policyId,
    policy_version: 1,
    owner_type: input.accountClass === "b2c" ? "global_b2c" : "b2b_client",
    owner_id: input.accountClass === "b2c" ? "global" : "acct_owner",
    account_class: input.accountClass,
    product_id: "main",
    billing_mode: "balance",
    schema_version: 2,
    capability_generation: 1,
    capability_digest: digest("capability"),
    catalog_generation: 1,
    catalog_digest: digest("catalog"),
    switch_generation: 1,
    switch_digest: digest("switch"),
    content_digest: digest(`policy:${input.policyId}`),
    rules: input.rules.map((rule, index) => ({
      rule_id: `rule-${index}`,
      rule_digest: digest(`rule:${input.policyId}:${index}`),
      scope: rule.scope === "global"
        ? { scope: "global" as const }
        : rule.scope === "provider"
          ? { scope: "provider" as const, provider_id: rule.providerId! }
          : {
              scope: "model" as const,
              provider_id: rule.providerId!,
              canonical_model_id: rule.canonicalModelId!,
            },
      discount_bps: 10_000 - rule.payableMultiplierBp,
      payable_multiplier_bp: rule.payableMultiplierBp,
    })),
  } as PricingReleasePolicyV2;
}

function releaseAssignment(accountId: string, policy: PricingReleasePolicyV2): PricingReleaseAssignmentV2 {
  return {
    account_id: accountId,
    account_class: policy.account_class,
    policy_id: policy.policy_id,
    policy_version: policy.policy_version,
    policy_digest: policy.content_digest,
    billing_mode: "balance",
    funding_generation: 1,
    purpose: null,
    responsible: null,
    assignment_digest: digest(`assignment:${accountId}`),
  } as PricingReleaseAssignmentV2;
}

type ReleaseFixture = {
  /** Base assignments of the active release (extension always wins when present). */
  assignments?: PricingReleaseAssignmentV2[];
  /** accountId → assignment delivered through the head-pinned assignment extension. */
  extensions?: Record<string, PricingReleaseAssignmentV2>;
  /** `${policy_id}@${policy_version}` → stored release policy. */
  policies: Record<string, PricingReleasePolicyV2>;
  /** Accounts whose extension read throws (transport failure isolation). */
  throwOnExtensionFor?: ReadonlySet<string>;
  /** Engine-side scalar per account before the lane aligns it (default: already aligned). */
  initialScalarBp?: Record<string, number>;
  /** Engine active policy identity for the reconciliation cross-check (default: v1 fixture). */
  pricingStatePolicy?: { effective_version: number; content_digest: string };
  optOut?: PricingReleaseOptOutAckV2;
};

function shadowPricingState(fixture: ReleaseFixture) {
  return {
    active: {
      policy: fixture.pricingStatePolicy ?? { effective_version: 1, content_digest: "engine-digest-v1" },
      binding: {
        policy_enforcement: "shadow",
        funding_enforcement: "legacy_single",
        reconciliation_state: "verified",
      },
    },
  };
}

/** The combined transport: release-v2 reads for the sweep + preflight/opt-out for the chain. */
type BackfillTestTransport = PricingBackfillReleaseTransport
  & AccountStrictCutoverPreflightTransport
  & StrictChainOptOutTransport;

function fakeEngine(fixture: ReleaseFixture): {
  transport: BackfillTestTransport;
  keyStamps: Array<{ keyId: string; ack: unknown }>;
  scalarWrites: Array<{ accountId: string; multiplierBp: number }>;
} {
  const keyStamps: Array<{ keyId: string; ack: unknown }> = [];
  const scalarWrites: Array<{ accountId: string; multiplierBp: number }> = [];
  const scalars = new Map<string, number>(Object.entries(fixture.initialScalarBp ?? {}));
  const optOutAck: PricingReleaseOptOutAckV2 = fixture.optOut ?? {
    result: "applied",
    identity: { account_id: "acct_test" },
    pricing_release_opt_out_ts: 1_700_000_000,
  };
  const transport = {
    getPricingReleaseHeadV2: async () => ({
      active_generation: 55,
      active_digest: digest("release:55"),
      head_version: 7,
      updated_ts: 1_700_000_000,
    }),
    getPricingReleaseV2: async (generation: number) => ({
      generation,
      content_digest: digest("release:55"),
      assignments: fixture.assignments ?? [],
    }),
    getPricingReleaseAssignmentExtensionV2: async (_headVersion: number, accountId: string) => {
      if (fixture.throwOnExtensionFor?.has(accountId)) {
        throw new Error("engine assignment extension read failed with 503");
      }
      const assignment = fixture.extensions?.[accountId];
      return assignment === undefined ? null : { members: [{ assignment }] };
    },
    getPricingReleasePolicyV2: async (policyId: string, policyVersion: number) =>
      fixture.policies[`${policyId}@${policyVersion}`] ?? null,
    getAccount: async (accountId: string) => ({
      account: accountId,
      balance_nano: "0",
      spent_nano: "0",
      reserved_nano: "0",
      balance: "$0.000000000",
      // Unlisted accounts start aligned with the release fallback of their fixture policy.
      mult_bp: scalars.get(accountId) ?? 5_000,
      status: "active",
      handle: null,
    }),
    setAccountMultiplier: async (accountId: string, multiplierBp: number) => {
      scalarWrites.push({ accountId, multiplierBp });
      scalars.set(accountId, multiplierBp);
    },
    getFundingNormalizationPlanV2: async () => null,
    applyFundingNormalizationV2: async () => null,
    getAccountPricingState: async () => shadowPricingState(fixture),
    listKeys: async () => [
      { key_id: "key_active", key_masked: "sk-pool-act…ive", label: "prod", status: "active", spent_nano: "0", spent: "$0.000000000" },
    ],
    setKeyStatus: async (keyId: string, _status: string, ack: unknown) => {
      keyStamps.push({ keyId, ack });
    },
    optOutPricingReleaseV2: async () => optOutAck,
  };
  return { transport: transport as unknown as BackfillTestTransport, keyStamps, scalarWrites };
}

describe.runIf(Boolean(connectionString))("existing-account pricing release backfill (phase 2.2)", () => {
  let admin: Client;
  let seedClient: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `pricing_backfill_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seedClient = new Client({ connectionString: url.toString() });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "pricing-backfill-test");
  }, TEST_TIMEOUT_MS);

  beforeEach(async () => {
    const tables = await seedClient.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    if (tables.rows.length > 0) {
      await seedClient.query(
        `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
      );
    }
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [],
      openkeys_accounts: [],
    }, { mode: "safe" });
    await markCutoverCompleted();
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
    await seedClient?.end();
    if (admin) {
      await admin.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await admin.query(`DROP DATABASE IF EXISTS ${quoteIdentifier(databaseName)}`);
      await admin.end();
    }
  }, TEST_TIMEOUT_MS);

  // A pre-existing B2C account: provisioned BEFORE the direct strict chain existed, so its
  // policy is materialized but nothing is armed and the account still resolves under release.
  async function existingB2cUser(email: string): Promise<{ userId: string; engineAccountId: string }> {
    const user = await createEmailUser(database, email, "password-hash");
    const engineAccountId = `acct_backfill_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    return { userId: user.id, engineAccountId };
  }

  // The safe Stage 5 apply requires the exact full engine inventory: seed every account of
  // the cohort in ONE call (a second partial call would report the earlier accounts as
  // missing), then materialize each one un-armed.
  async function provisionExistingB2cCohort(
    cohort: Array<{ userId: string; engineAccountId: string }>,
  ): Promise<void> {
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: cohort.map(({ engineAccountId }) => ({
        account_id: engineAccountId,
        multiplier_bp: 5_000,
        status: "active",
      })),
      openkeys_accounts: [],
    }, { mode: "safe" });
    for (const { userId, engineAccountId } of cohort) {
      const materialized = await materializeProvisionedUserPolicy(
        database,
        { userId, engineAccountId },
        { armStrictChain: false },
      );
      expect(materialized.policyRequired).toBe(true);
      const binding = await bindingState(userId);
      expect(binding?.strict_chain_pending).toBe(false);
    }
  }

  async function bindingState(userId: string) {
    const result = await seedClient.query<{
      policy_enforcement: string;
      reconciliation_state: string;
      sync_state: string;
      strict_chain_pending: boolean;
      last_error: string | null;
    }>(`
      SELECT policy_enforcement, reconciliation_state, sync_state, strict_chain_pending, last_error
      FROM account_policy_bindings WHERE user_id = $1
    `, [userId]);
    return result.rows[0];
  }

  async function confirmShadowDelivery(userId: string): Promise<void> {
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', reconciliation_state = 'verified',
          last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [userId]);
    // The chain stages strict only against a TERMINAL shadow delivery: an in-flight job for
    // the same (binding, version) under a different binding is a "delivered right now" wait.
    await seedClient.query(`
      UPDATE engine_policy_jobs
      SET status = 'confirmed', last_error = NULL, confirmed_at = now(),
          ack_effective_version = effective_version,
          ack_policy_version = policy_version,
          ack_catalog_generation = catalog_generation,
          ack_switch_generation = switch_generation,
          ack_schema_version = schema_version,
          ack_content_digest = content_digest,
          ack_payload = payload
    `);
  }

  // What confirmPricingControlJob writes after the engine ACKs the STRICT delivery.
  async function confirmStrictDelivery(userId: string): Promise<void> {
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'strict', funding_enforcement = 'strict',
          reconciliation_state = 'verified', last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [userId]);
  }

  // The engine-side active policy identity the reconciliation cross-check compares against:
  // exactly the binding's desired (engine-ACKed) head.
  async function desiredPricingState(userId: string): Promise<{
    effective_version: number;
    content_digest: string;
  }> {
    const row = await seedClient.query<{
      desired_effective_version: string;
      desired_digest: string;
    }>(`
      SELECT desired_effective_version::text, desired_digest
      FROM account_policy_bindings WHERE user_id = $1
    `, [userId]);
    return {
      effective_version: Number(row.rows[0]!.desired_effective_version),
      content_digest: row.rows[0]!.desired_digest,
    };
  }

  // A durable cutover receipt with its FK parents: production is post-cutover, so nothing in
  // the cohort is armed by conversion/policy-save side effects.
  async function markCutoverCompleted(): Promise<void> {
    for (const [generation, kind] of [[901, "target"], [902, "recovery"]] as const) {
      await seedClient.query(`
        INSERT INTO pricing_release_plans_v2 (
          generation,release_kind,schema_version,commerce_inventory_digest,engine_inventory_digest,
          openkeys_inventory_digest,service_inventory_digest,policy_manifest_digest,
          assignment_manifest_digest,funding_manifest_digest,engine_release_digest,content_digest,status
        ) VALUES ($1,$2,2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'planned')
      `, [
        generation,
        kind,
        digest(`commerce:${generation}`),
        digest(`engine:${generation}`),
        digest(`openkeys:${generation}`),
        digest(`service:${generation}`),
        digest(`policy-manifest:${generation}`),
        digest(`assignment-manifest:${generation}`),
        digest(`funding-manifest:${generation}`),
        digest(`engine-release:${generation}`),
        digest(`content:${generation}`),
      ]);
    }
    await seedClient.query(`
      INSERT INTO pricing_stage8_evidence_v2 (
        evidence_digest,target_generation,target_digest,recovery_generation,recovery_digest,
        commerce_inventory_digest,engine_inventory_digest,openkeys_inventory_digest,
        sales_contract_digest,funding_digest,shadow_digest,runtime_floor_digest,
        legacy_inflight_count,blocker_count,passed,observed_at,valid_until
      ) VALUES ($1,901,$2,902,$3,$4,$5,$6,$7,$8,$9,$10,0,0,true,now(),now()+interval '5 minutes')
    `, [
      digest("evidence"),
      digest("content:901"),
      digest("content:902"),
      digest("commerce-evidence"),
      digest("engine:901"),
      digest("openkeys-evidence"),
      digest("sales-contract"),
      digest("funding-manifest:901"),
      digest("shadow"),
      digest("runtime-floor"),
    ]);
    await seedClient.query(`
      INSERT INTO pricing_release_activation_receipts_v2 (
        activation_id,activation_kind,release_generation,release_digest,
        evidence_digest,head_version,receipt_digest,receipt_payload,activated_at
      ) VALUES ($1,'cutover',901,$2,$3,1,$4,$5::jsonb,now())
    `, [
      randomUUID(),
      digest("content:901"),
      digest("evidence"),
      digest("activation-receipt"),
      JSON.stringify({ result: "applied" }),
    ]);
  }

  async function provisionedB2cUser(email: string) {
    const account = await existingB2cUser(email);
    await provisionExistingB2cCohort([account]);
    return account;
  }

  function b2cRelease(engineAccountId: string): ReleaseFixture {
    const policy = releasePolicy({
      policyId: "release-v2:b2c:global",
      accountClass: "b2c",
      rules: [{ scope: "global", payableMultiplierBp: 5_000 }],
    });
    return {
      assignments: [releaseAssignment(engineAccountId, policy)],
      policies: { [`${policy.policy_id}@${policy.policy_version}`]: policy },
    };
  }

  it("arms a B2C shadow account after the 5000-global identity check and the unmodified chain retires it", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-b2c@example.test");
    const engine = fakeEngine(b2cRelease(engineAccountId));

    // The shadow delivery is confirmed and verified BEFORE the sweep: only a verifiable
    // binding may be armed (arming an unconverged one is the deadlock this lane avoids).
    await confirmShadowDelivery(userId);

    // 1. The sweep materializes (idempotent reuse), proves the identity, and arms the chain.
    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({ examined: 1, armed: [engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    expect(await bindingState(userId)).toMatchObject({
      // The materialized shadow delivery is staged; the strict flip comes from the chain.
      policy_enforcement: "shadow",
      strict_chain_pending: true,
      last_error: null,
    });
    // Armed accounts leave the candidate set immediately (the fast lane owns them now).
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);

    // 2. The UNMODIFIED new-account chain drives the account: strict staging → strict
    //    delivery confirms → the one-way opt-out marker.
    const staging = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(staging.status).toBe("staged");
    await confirmStrictDelivery(userId);
    const completing = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(completing).toEqual({ status: "opted_out" });
    expect(await bindingState(userId)).toMatchObject({
      policy_enforcement: "strict",
      strict_chain_pending: false,
    });

    // 3. The durable done-proof: the audit entry removes the account from every future sweep
    //    and the progress surface reports it.
    const audit = await seedClient.query<{ action: string; metadata: unknown }>(`
      SELECT action, metadata FROM audit_log
      WHERE action = 'pricing_release.opt_out' AND target_type = 'engine_account' AND target_id = $1
    `, [engineAccountId]);
    expect(audit.rows).toHaveLength(1);
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);
    const health = await getAdminPricingBackfillHealth(database);
    expect(health.counts).toEqual({ eligible: 1, inFlight: 0, done: 1, failed: 0, pending: 0 });
  });

  it("aligns the legacy 4000-scalar B2C cohort to the release fallback before the gate and the opt-out", async () => {
    // The production cohort that the gate correctly blocked before this fix: engine
    // accounts.mult_bp=4000 (legacy scalar, dormant under the release path), release
    // assignment release-v2:b2c:global at 5000, strict global-b2c rules for
    // anthropic/google/openai only — glm/kimi scopes resolve to the scalar on the strict
    // side vs the release global, so the scalar must be aligned FIRST.
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-legacy@example.test");
    await seedClient.query(`
      UPDATE customer_profiles SET multiplier_bp = 4_000 WHERE user_id = $1
    `, [userId]);
    await seedClient.query(`
      UPDATE engine_accounts SET mult_bp = 4_000 WHERE user_id = $1
    `, [userId]);
    const engine = fakeEngine({
      ...b2cRelease(engineAccountId),
      initialScalarBp: { [engineAccountId]: 4_000 },
    });
    await confirmShadowDelivery(userId);

    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({ examined: 1, armed: [engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    // The engine scalar write (account_set_mult_bp) landed BEFORE the arm — and therefore
    // before the opt-out marker — and the commerce mirrors converged in the same pass.
    expect(engine.scalarWrites).toEqual([{ accountId: engineAccountId, multiplierBp: 5_000 }]);
    const scalars = await seedClient.query<{ profile_mult: number; engine_mult: number }>(`
      SELECT profile.multiplier_bp AS profile_mult, account.mult_bp AS engine_mult
      FROM customer_profiles profile
      JOIN engine_accounts account ON account.user_id = profile.user_id
      WHERE profile.user_id = $1
    `, [userId]);
    expect(scalars.rows).toEqual([{ profile_mult: 5_000, engine_mult: 5_000 }]);

    // The gate passes after the alignment and the unmodified chain retires the account.
    const staged = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(staged.status).toBe("staged");
    await confirmStrictDelivery(userId);
    await expect(advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "opted_out" });
    const audit = await seedClient.query(`
      SELECT 1 FROM audit_log
      WHERE action = 'pricing_release.opt_out' AND target_type = 'engine_account' AND target_id = $1
    `, [engineAccountId]);
    expect(audit.rows).toHaveLength(1);
  });

  it("a confirmed binding with ZERO release coverage skips the gate and goes end-to-end (broken-window cohort)", async () => {
    // Accounts registered in the window between the assignment-extension removal and the
    // backfill have a confirmed commerce binding but no release rows at all: the release
    // resolver already fails closed on them today, so there is no release-side resolution
    // to diverge from — the lane must skip the scope-walk gate (like a phase-2.1 new
    // account), still align the scalar, and retire the account through the unmodified chain.
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-uncovered@example.test");
    const engine = fakeEngine({ assignments: [], policies: {} });
    await confirmShadowDelivery(userId);

    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({
      examined: 1,
      armed: [engineAccountId],
      armedWithoutReleaseCoverage: [engineAccountId],
      pending: [],
      failed: [],
    });
    expect(engine.scalarWrites).toEqual([{ accountId: engineAccountId, multiplierBp: 5_000 }]);
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });

    const staged = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(staged.status).toBe("staged");
    await confirmStrictDelivery(userId);
    await expect(advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "opted_out" });
    const audit = await seedClient.query(`
      SELECT 1 FROM audit_log
      WHERE action = 'pricing_release.opt_out' AND target_type = 'engine_account' AND target_id = $1
    `, [engineAccountId]);
    expect(audit.rows).toHaveLength(1);
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);
  });

  it("arms a B2B account whose model-scoped release rules match the strict policy exactly", async () => {
    const user = await createEmailUser(database, "backfill-b2b@example.test", "password-hash");
    const engineAccountId = `acct_backfill_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    await runStage5Backfill(database, {
      schema_version: 1,
      // Pre-conversion the profile still carries the B2C 5000: the safe apply demands the
      // exact engine/profile match, the negotiated 4000 arrives with the conversion below.
      engine_accounts: [{ account_id: engineAccountId, multiplier_bp: 5_000, status: "active" }],
      openkeys_accounts: [],
    }, { mode: "safe" });
    await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "negotiated business terms",
      multiplierBp: 4_000,
    });
    // The negotiated policy: per-provider AND per-model scopes, preserved exactly.
    await updateManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
      expectedVersion: 1,
      rules: [
        { scope: { provider: { providerId: "anthropic" } }, pricingMode: "discount", discountBps: 4_000 },
        { scope: { model: { providerId: "anthropic", canonicalModelId: "claude-opus-4-8" } }, pricingMode: "discount", discountBps: 6_000 },
        { scope: { provider: { providerId: "openai" } }, pricingMode: "discount", discountBps: 2_500 },
      ],
      actorId: "admin@example.test",
      reason: "model-scoped negotiated policy",
    });
    expect((await bindingState(user.id))?.strict_chain_pending).toBe(false);
    // The shadow delivery of the negotiated policy confirms BEFORE the sweep, so the binding
    // is verifiable (the lane arms only converged, verified bindings).
    await confirmShadowDelivery(user.id);

    // Post-cutover the account's override propagates through the assignment EXTENSION —
    // the extension-wins path of the equivalence resolver.
    const policy = releasePolicy({
      policyId: `release-v2:b2b:${engineAccountId}`,
      accountClass: "b2b",
      rules: [
        { scope: "provider", providerId: "anthropic", payableMultiplierBp: 6_000 },
        { scope: "model", providerId: "anthropic", canonicalModelId: "claude-opus-4-8", payableMultiplierBp: 4_000 },
        { scope: "provider", providerId: "openai", payableMultiplierBp: 7_500 },
      ],
    });
    const engine = fakeEngine({
      extensions: { [engineAccountId]: releaseAssignment(engineAccountId, policy) },
      policies: { [`${policy.policy_id}@${policy.policy_version}`]: policy },
      // The negotiated conversion scalar (4000) is the pre-alignment state: release B2B has
      // no rule-less fallback, so the derived target is full price.
      initialScalarBp: { [engineAccountId]: 4_000 },
    });
    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({ examined: 1, armed: [engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    expect(await bindingState(user.id)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });
    // The fallback was DERIVED from the release policy (no global rule → full price), not
    // hardcoded and not inherited from the negotiated 4000: engine write, then mirrors.
    expect(engine.scalarWrites).toEqual([{ accountId: engineAccountId, multiplierBp: 10_000 }]);
    const scalars = await seedClient.query<{ profile_mult: number; engine_mult: number }>(`
      SELECT profile.multiplier_bp AS profile_mult, account.mult_bp AS engine_mult
      FROM customer_profiles profile
      JOIN engine_accounts account ON account.user_id = profile.user_id
      WHERE profile.user_id = $1
    `, [user.id]);
    expect(scalars.rows).toEqual([{ profile_mult: 10_000, engine_mult: 10_000 }]);
  });

  it("never selects service accounts, even when a customer binding shares the engine account id", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-service@example.test");
    await seedClient.query(`
      INSERT INTO service_account_inventory_v2 (
        service_id, engine_account_id, purpose, responsible, status, source_version, content_digest
      ) VALUES ('service:test', $1, 'internal service', 'ops', 'active', 1, $2)
    `, [engineAccountId, digest("service:test")]);

    // The inventory probe is the belt-and-braces exclusion on top of the identity CHECK
    // (service bindings never carry a user_id): this account must be invisible to the lane.
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);
    expect(await listPricingBackfillCandidates(database, { limit: 5, allowlist: [engineAccountId] }))
      .toEqual([]);
    const health = await getAdminPricingBackfillHealth(database);
    expect(health.counts).toEqual({ eligible: 0, inFlight: 0, done: 0, failed: 0, pending: 0 });
    expect(await bindingState(userId)).toMatchObject({ strict_chain_pending: false });
  });

  it("an equivalence mismatch skips the account with last_error and self-heals after the release side is fixed", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-mismatch@example.test");
    const mismatched = releasePolicy({
      policyId: "release-v2:b2c:global",
      accountClass: "b2c",
      rules: [{ scope: "global", payableMultiplierBp: 4_000 }],
    });
    const blocked = fakeEngine({
      assignments: [releaseAssignment(engineAccountId, mismatched)],
      policies: { [`${mismatched.policy_id}@${mismatched.policy_version}`]: mismatched },
    });

    const sweep = await runPricingBackfillSweep(database, blocked.transport, { limit: 5 });
    expect(sweep.examined).toBe(1);
    expect(sweep.armed).toEqual([]);
    expect(sweep.failed).toHaveLength(1);
    expect(sweep.failed[0]!.error).toContain("global rule resolves to 4000");
    const binding = await bindingState(userId);
    expect(binding).toMatchObject({ strict_chain_pending: false });
    expect(binding?.last_error).toContain("global rule resolves to 4000");
    // Never forced: the account stays a candidate (re-evaluated calmly on the next pass) and
    // the progress surface counts it as failed, not pending.
    expect((await listPricingBackfillCandidates(database, { limit: 5 }))
      .map((candidate) => candidate.engineAccountId)).toEqual([engineAccountId]);
    expect((await getAdminPricingBackfillHealth(database)).counts)
      .toEqual({ eligible: 1, inFlight: 0, done: 0, failed: 1, pending: 0 });

    // The operator fixes the divergence (here: the release side returns to the 5000
    // identity); the next pass re-checks and arms without any manual state repair.
    await confirmShadowDelivery(userId);
    const healed = fakeEngine(b2cRelease(engineAccountId));
    const retry = await runPricingBackfillSweep(database, healed.transport, { limit: 5 });
    expect(retry).toEqual({ examined: 1, armed: [engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });
  });

  it("honors the canary allowlist and isolates per-account failures (typed and transport)", async () => {
    const first = await existingB2cUser("backfill-canary-1@example.test");
    const second = await existingB2cUser("backfill-canary-2@example.test");
    const third = await existingB2cUser("backfill-canary-3@example.test");
    await provisionExistingB2cCohort([first, second, third]);

    // Canary mode: only the listed account is touched, everything else stays a candidate.
    await confirmShadowDelivery(first.userId);
    const canary = fakeEngine(b2cRelease(first.engineAccountId));
    const canarySweep = await runPricingBackfillSweep(database, canary.transport, {
      limit: 5,
      allowlist: [first.engineAccountId],
    });
    expect(canarySweep).toEqual({ examined: 1, armed: [first.engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    expect((await listPricingBackfillCandidates(database, { limit: 10 }))
      .map((candidate) => candidate.engineAccountId).sort())
      .toEqual([second.engineAccountId, third.engineAccountId].sort());

    // Full mode with a mixed cohort: second mismatches (typed), third's release read throws
    // (transport); neither blocks the other, and both are reported per account.
    const mismatched = releasePolicy({
      policyId: "release-v2:b2c:global",
      accountClass: "b2c",
      rules: [{ scope: "global", payableMultiplierBp: 4_000 }],
    });
    const mixed = fakeEngine({
      assignments: [releaseAssignment(second.engineAccountId, mismatched)],
      policies: { [`${mismatched.policy_id}@${mismatched.policy_version}`]: mismatched },
      throwOnExtensionFor: new Set([third.engineAccountId]),
    });
    const sweep = await runPricingBackfillSweep(database, mixed.transport, { limit: 5 });
    expect(sweep.examined).toBe(2);
    expect(sweep.armed).toEqual([]);
    expect(sweep.failed).toHaveLength(2);
    expect(sweep.failed.find((failure) => failure.engineAccountId === second.engineAccountId)?.error)
      .toContain("global rule resolves to 4000");
    expect(sweep.failed.find((failure) => failure.engineAccountId === third.engineAccountId)?.error)
      .toContain("503");
    // The typed failure is durable on the binding; the transport failure is not (it is
    // retried fresh next pass, matching the strict chain's transport-error discipline).
    expect((await bindingState(second.userId))?.last_error).toContain("4000");
    expect((await bindingState(third.userId))?.last_error).toBeNull();
  });

  it("arms an already-strict confirmed account for the opt-out step only (no re-materialization)", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-strict@example.test");
    const engine = fakeEngine(b2cRelease(engineAccountId));

    // Drive the account to strict/confirmed first (the "1 strict/confirmed binding" of the
    // production fleet): armed by hand as the pre-cutover lane would have left it, then
    // disarmed without the opt-out ever landing.
    await confirmShadowDelivery(userId);
    await seedClient.query(`
      UPDATE account_policy_bindings SET strict_chain_pending = true WHERE user_id = $1
    `, [userId]);
    const staged = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(staged.status).toBe("staged");
    await confirmStrictDelivery(userId);
    await seedClient.query(`
      UPDATE account_policy_bindings SET strict_chain_pending = false WHERE user_id = $1
    `, [userId]);

    // The sweep sees a candidate whose enforced policy is already the desired one: it skips
    // materialization, re-proves equivalence against the strict rules, and re-arms — the
    // chain's opt-out step then retires the account.
    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({ examined: 1, armed: [engineAccountId], armedWithoutReleaseCoverage: [], pending: [], failed: [] });
    const completing = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(completing).toEqual({ status: "opted_out" });
    const audit = await seedClient.query(`
      SELECT 1 FROM audit_log
      WHERE action = 'pricing_release.opt_out' AND target_type = 'engine_account' AND target_id = $1
    `, [engineAccountId]);
    expect(audit.rows).toHaveLength(1);
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);
  });

  it("an opt-out guard rejection keeps the account armed and uncounted as done", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-waiting@example.test");
    const rejected = fakeEngine({
      ...b2cRelease(engineAccountId),
      optOut: {
        result: "rejected",
        code: "missing_dependency",
        identity: {},
        rejection: { missing_dependency: { dependency: "active_strict_policy_binding" } },
      },
    });
    await confirmShadowDelivery(userId);
    await runPricingBackfillSweep(database, rejected.transport, { limit: 5 });
    await advanceAccountStrictChain(
      database,
      rejected.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    await confirmStrictDelivery(userId);
    // The engine guard says "no live strict path yet": the chain waits quietly — armed, no
    // error, no audit entry, nothing opted out.
    const waiting = await advanceAccountStrictChain(
      database,
      rejected.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(waiting).toEqual({ status: "pending" });
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });
    const health = await getAdminPricingBackfillHealth(database);
    expect(health.counts).toEqual({ eligible: 1, inFlight: 1, done: 0, failed: 0, pending: 0 });
  });

  it("verifies a shadow+confirmed binding stuck in reconciliation 'pending' before arming (the deleted rollout lane's step)", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-reconcile@example.test");
    // The deadlock cohort's exact shape: delivery confirmed (desired=applied, sync
    // confirmed), but nothing ever flipped reconciliation to 'verified'.
    await confirmShadowDelivery(userId);
    await seedClient.query(`
      UPDATE account_policy_bindings SET reconciliation_state = 'pending' WHERE user_id = $1
    `, [userId]);

    // An engine cross-check that disagrees with the durable ACK proof keeps the account
    // un-armed and quiet — no error, still a candidate.
    const mismatched = fakeEngine({
      ...b2cRelease(engineAccountId),
      pricingStatePolicy: { effective_version: 1, content_digest: "engine-digest-v1" },
    });
    const blocked = await runPricingBackfillSweep(database, mismatched.transport, { limit: 5 });
    expect(blocked).toEqual({
      examined: 1,
      armed: [],
      armedWithoutReleaseCoverage: [],
      pending: [engineAccountId],
      failed: [],
    });
    expect(await bindingState(userId)).toMatchObject({
      reconciliation_state: "pending",
      strict_chain_pending: false,
      last_error: null,
    });

    // The cross-check matching the engine-ACKed head: verified in the same pass, then armed,
    // and the unmodified chain retires the account.
    const engine = fakeEngine({
      ...b2cRelease(engineAccountId),
      pricingStatePolicy: await desiredPricingState(userId),
    });
    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep).toEqual({
      examined: 1,
      armed: [engineAccountId],
      armedWithoutReleaseCoverage: [],
      pending: [],
      failed: [],
    });
    expect(await bindingState(userId)).toMatchObject({
      reconciliation_state: "verified",
      strict_chain_pending: true,
    });
    const staged = await advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(staged.status).toBe("staged");
    await confirmStrictDelivery(userId);
    await expect(advanceAccountStrictChain(
      database,
      engine.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "opted_out" });
  });

  it("a shadow binding with desired≠applied stays un-armed and quiet until the delivery converges", async () => {
    const { userId, engineAccountId } = await provisionedB2cUser("backfill-converging@example.test");
    // Materialized but not delivered: desired is set, applied is NULL — the delivery lane
    // converges it first; arming now would be the deadlock.
    const engine = fakeEngine(b2cRelease(engineAccountId));
    const first = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(first).toEqual({
      examined: 1,
      armed: [],
      armedWithoutReleaseCoverage: [],
      pending: [engineAccountId],
      failed: [],
    });
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: false,
      last_error: null,
    });
    // Rotated, not dropped: still a candidate for the next pass.
    expect((await listPricingBackfillCandidates(database, { limit: 5 }))
      .map((candidate) => candidate.engineAccountId)).toEqual([engineAccountId]);

    // The delivery confirms (with the deadlock-era 'pending' reconciliation): the next pass
    // verifies with the engine cross-check and arms.
    await confirmShadowDelivery(userId);
    await seedClient.query(`
      UPDATE account_policy_bindings SET reconciliation_state = 'pending' WHERE user_id = $1
    `, [userId]);
    const converged = fakeEngine({
      ...b2cRelease(engineAccountId),
      pricingStatePolicy: await desiredPricingState(userId),
    });
    const second = await runPricingBackfillSweep(database, converged.transport, { limit: 5 });
    expect(second).toEqual({
      examined: 1,
      armed: [engineAccountId],
      armedWithoutReleaseCoverage: [],
      pending: [],
      failed: [],
    });
    expect(await bindingState(userId)).toMatchObject({
      reconciliation_state: "verified",
      strict_chain_pending: true,
    });
  });

  it("an account with no managed policy is marked terminal once and leaves the candidate set (hot-loop guard)", async () => {
    const user = await createEmailUser(database, "backfill-no-policy@example.test", "password-hash");
    const engineAccountId = `acct_backfill_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [{ account_id: engineAccountId, multiplier_bp: 5_000, status: "active" }],
      openkeys_accounts: [],
    }, { mode: "safe" });
    await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "negotiated business terms",
      multiplierBp: 4_000,
    });
    // The managed policy is gone (the hot-looping production account's state): there is
    // nothing to materialize from, and there never will be inside this lane.
    await seedClient.query(`
      UPDATE pricing_policies SET status = 'archived', updated_at = now()
      WHERE owner_type = 'b2b_client' AND owner_id = $1
    `, [user.id]);
    const policy = releasePolicy({
      policyId: `release-v2:b2b:${engineAccountId}`,
      accountClass: "b2b",
      rules: [{ scope: "provider", providerId: "anthropic", payableMultiplierBp: 4_000 }],
    });
    const engine = fakeEngine({
      extensions: { [engineAccountId]: releaseAssignment(engineAccountId, policy) },
      policies: { [`${policy.policy_id}@${policy.policy_version}`]: policy },
    });

    const sweep = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(sweep.examined).toBe(1);
    expect(sweep.armed).toEqual([]);
    expect(sweep.failed).toHaveLength(1);
    expect(sweep.failed[0]!.error).toContain("no managed pricing policy");
    expect((await bindingState(user.id))?.last_error)
      .toBe("terminal: account has no managed pricing policy to materialize");

    // The terminal marker is durable AND quiet: the account is excluded from every future
    // pass (no re-log, no retry), while pipeline-health keeps it visible as failed.
    expect(await listPricingBackfillCandidates(database, { limit: 5 })).toEqual([]);
    const second = await runPricingBackfillSweep(database, engine.transport, { limit: 5 });
    expect(second).toEqual({
      examined: 0,
      armed: [],
      armedWithoutReleaseCoverage: [],
      pending: [],
      failed: [],
    });
    const health = await getAdminPricingBackfillHealth(database);
    expect(health.counts).toEqual({ eligible: 1, inFlight: 0, done: 0, failed: 1, pending: 0 });
    expect(health.recentFailures[0]?.lastError).toContain("no managed pricing policy");
  });
});
