import { randomUUID } from "node:crypto";
import { createHash } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { FundingNormalizationPlanV2, PricingReleaseOptOutAckV2 } from "@claude-api/contracts";
import type { AccountStrictCutoverPreflightTransport } from "@claude-api/engine-client";
import { createEmailUser } from "./auth.js";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import { runStage5Backfill } from "./multi-discount-backfill.js";
import { convertCustomerToBusiness } from "./pricing.js";
import { materializeProvisionedUserPolicy, updateManagedPricingPolicy } from "./pricing-policy-write.js";
import { advanceAccountStrictChain, listPendingStrictChainAccounts } from "./strict-chain.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function digest(label: string): string {
  return `sha256:v2:${createHash("sha256").update(label, "utf8").digest("hex")}`;
}

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function blockedPlan(accountId: string): FundingNormalizationPlanV2 {
  return {
    account_id: accountId,
    account_status: "active",
    status: "blocked",
    source: "aggregate_paid_only",
    source_state_digest: `sha256:v2:${"a".repeat(64)}`,
    normalization_digest: null,
    funding_generation: null,
    funding_head_version: null,
    balance_nano: "5000000000",
    reserved_nano: "0",
    spent_nano: "0",
    lots: [],
    blockers: [{ code: "active_legacy_reservation", detail: "reservation r1 is open" }],
  };
}

function shadowPricingState() {
  return {
    active: {
      policy: { effective_version: 1, content_digest: "engine-digest-v1" },
      binding: {
        policy_enforcement: "shadow",
        funding_enforcement: "legacy_single",
        reconciliation_state: "verified",
      },
    },
  };
}

type StrictChainTestTransport = AccountStrictCutoverPreflightTransport & {
  optOutPricingReleaseV2(input: { accountId: string }): Promise<PricingReleaseOptOutAckV2>;
};

function fakeEngine(input: {
  plan?: FundingNormalizationPlanV2 | null;
  optOut?: PricingReleaseOptOutAckV2;
} = {}) {
  const keyStamps: Array<{ keyId: string; ack: unknown }> = [];
  const calls: string[] = [];
  const optOutAck: PricingReleaseOptOutAckV2 = input.optOut ?? {
    result: "applied",
    identity: { account_id: "acct_test" },
    pricing_release_opt_out_ts: 1_700_000_000,
  };
  const transport = {
    getFundingNormalizationPlanV2: vi.fn(async () => {
      calls.push("plan");
      return input.plan ?? null;
    }),
    applyFundingNormalizationV2: vi.fn(async () => {
      calls.push("apply");
      return null;
    }),
    getAccountPricingState: vi.fn(async () => {
      calls.push("state");
      return shadowPricingState();
    }),
    listKeys: vi.fn(async () => {
      calls.push("keys");
      return [
        { key_id: "key_active", key_masked: "sk-pool-act…ive", label: "prod", status: "active", spent_nano: "0", spent: "$0.000000000" },
        { key_id: "key_disabled", key_masked: "sk-pool-dis…ed", label: null, status: "disabled", spent_nano: "0", spent: "$0.000000000" },
      ];
    }),
    setKeyStatus: vi.fn(async (keyId: string, _status: string, ack: unknown) => {
      keyStamps.push({ keyId, ack });
    }),
    optOutPricingReleaseV2: vi.fn(async () => {
      calls.push("opt-out");
      return optOutAck;
    }),
  } as unknown as StrictChainTestTransport;
  return { transport, keyStamps, calls };
}

describe.runIf(Boolean(connectionString))("new-account direct strict chain", () => {
  let admin: Client;
  let seedClient: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `strict_chain_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seedClient = new Client({ connectionString: url.toString() });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "strict-chain-test");
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

  // The real registration arming path: a fresh user whose engine account is provisioned gets
  // the managed global B2C policy materialized, and provisioning arms the direct strict chain.
  async function registerUser(email: string): Promise<{ userId: string; engineAccountId: string }> {
    const user = await createEmailUser(database, email, "password-hash");
    const engineAccountId = `acct_chain_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [{ account_id: engineAccountId, multiplier_bp: 5_000, status: "active" }],
      openkeys_accounts: [],
    }, { mode: "safe" });
    const materialized = await materializeProvisionedUserPolicy(database, {
      userId: user.id,
      engineAccountId,
    });
    expect(materialized.policyRequired).toBe(true);
    return { userId: user.id, engineAccountId };
  }

  async function confirmShadowDelivery(userId: string): Promise<void> {
    // What confirmPricingControlJob writes after the engine ACKs the shadow delivery.
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', reconciliation_state = 'verified',
          last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [userId]);
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

  async function bindingState(userId: string) {
    const result = await seedClient.query<{
      policy_enforcement: string;
      funding_enforcement: string;
      reconciliation_state: string;
      sync_state: string;
      strict_chain_pending: boolean;
      last_error: string | null;
    }>(`
      SELECT policy_enforcement, funding_enforcement, reconciliation_state, sync_state,
             strict_chain_pending, last_error
      FROM account_policy_bindings WHERE user_id = $1
    `, [userId]);
    return result.rows[0];
  }

  // A durable cutover receipt with its FK parents, marking the post-cutover era for the
  // commerce-local check in the policy writers.
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

  it("provisioning arms the chain, which then drives shadow→strict→opt-out idempotently", async () => {
    const { userId } = await registerUser("chain-register@example.test");

    // Registration provisioning armed the chain; while the shadow delivery is in flight the
    // sweep finds the candidate but makes no engine call and changes nothing.
    const pending = await listPendingStrictChainAccounts(database, 10);
    expect(pending.map((candidate) => candidate.userId)).toEqual([userId]);
    const idle = fakeEngine();
    await expect(advanceAccountStrictChain(database, idle.transport, pending[0]!))
      .resolves.toEqual({ status: "pending" });
    expect(idle.calls).toEqual([]);
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });

    // The worker's delivery confirms the shadow policy: the chain now runs the shared
    // preflight (exact ACK on the active key only) and stages the atomic strict binding.
    // The flag stays armed — it is disarmed only by the opt-out step.
    await confirmShadowDelivery(userId);
    const staging = fakeEngine();
    const advanced = await advanceAccountStrictChain(
      database,
      staging.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(advanced).toMatchObject({
      status: "staged",
      funding: "nothing_to_normalize",
      keysStamped: 1,
    });
    expect(staging.calls).toEqual(["plan", "state", "keys"]);
    expect(staging.keyStamps).toEqual([{
      keyId: "key_active",
      ack: { effectivePolicyVersion: 1, policyDigest: "engine-digest-v1" },
    }]);
    expect(await bindingState(userId)).toEqual({
      policy_enforcement: "strict",
      funding_enforcement: "strict",
      reconciliation_state: "verified",
      sync_state: "confirmed",
      strict_chain_pending: true,
      last_error: null,
    });
    const job = await seedClient.query<{ status: string; binding: unknown }>(`
      SELECT status, payload->'binding' AS binding FROM engine_policy_jobs
    `);
    expect(job.rows).toHaveLength(1);
    expect(job.rows[0]).toMatchObject({
      status: "pending",
      binding: {
        policy_enforcement: "strict",
        funding_enforcement: "strict",
        reconciliation_state: "verified",
      },
    });

    // While the engine has not durably flipped (its guard answers missing_dependency), the
    // chain waits quietly: armed, no error recorded, and the account is NOT opted out.
    const waiting = fakeEngine({
      optOut: {
        result: "rejected",
        code: "missing_dependency",
        identity: {},
        rejection: { missing_dependency: { dependency: "active_strict_policy_binding" } },
      },
    });
    await expect(advanceAccountStrictChain(
      database,
      waiting.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "pending" });
    expect(waiting.calls).toEqual(["opt-out"]);
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: null,
    });

    // The strict delivery confirmed engine-side: the opt-out marker lands and the flag disarms.
    const completing = fakeEngine();
    await expect(advanceAccountStrictChain(
      database,
      completing.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "opted_out" });
    expect(completing.calls).toEqual(["opt-out"]);
    expect(await bindingState(userId)).toMatchObject({
      policy_enforcement: "strict",
      strict_chain_pending: false,
      last_error: null,
    });
    expect(await listPendingStrictChainAccounts(database, 10)).toEqual([]);

    // Worker redelivery or a duplicate sweep pass can never double-apply: the engine replay is
    // `unchanged`, which is also a completed chain.
    await seedClient.query(`
      UPDATE account_policy_bindings SET strict_chain_pending = true WHERE user_id = $1
    `, [userId]);
    const replay = fakeEngine({
      optOut: {
        result: "unchanged",
        identity: { account_id: "acct_test" },
        pricing_release_opt_out_ts: 1_700_000_000,
      },
    });
    await expect(advanceAccountStrictChain(
      database,
      replay.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    )).resolves.toEqual({ status: "opted_out" });
    expect((await bindingState(userId))?.strict_chain_pending).toBe(false);
  });

  it("orders actionable candidates ahead of silently-pending ones", async () => {
    // A pending candidate keeps its old updated_at and would pin the head of the LIMITed sweep
    // forever; the completable strict candidate must sort first regardless of age.
    const { userId: pendingUser } = await registerUser("chain-pending@example.test");
    // The second binding does not need full provisioning: the test exercises only the selector's
    // ordering. Seed the row directly in the strict/verified/confirmed shape — binding first
    // (desired/applied NULL), then its policy version row (the desired/applied FKs point at
    // account_policy_versions(binding_id, effective_version, content_digest)), then the versions.
    const strictCreated = await createEmailUser(database, "chain-strict@example.test", "password-hash");
    const strictAccount = `acct_chain_${strictCreated.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [strictCreated.id, strictAccount]);
    await seedClient.query(`
      INSERT INTO account_policy_bindings (
        id, user_id, engine_account_record_id, engine_account_id, account_class, product_id, policy_id,
        policy_enforcement, funding_enforcement, reconciliation_state, sync_state,
        strict_chain_pending
      )
      SELECT gen_random_uuid(), $1,
             (SELECT id FROM engine_accounts WHERE user_id = $1), $2,
             'b2c', product_id, policy_id,
             'legacy_scalar', 'legacy_single', 'pending', 'legacy', true
      FROM account_policy_bindings WHERE user_id = $3
    `, [strictCreated.id, strictAccount, pendingUser]);
    await seedClient.query(`
      INSERT INTO account_policy_versions (
        binding_id, effective_version, policy_id, policy_version, policy_digest, product_id,
        account_class, schema_version, catalog_generation, switch_generation, content_digest,
        replacement_locked, created_at
      )
      SELECT b.id, v.effective_version, v.policy_id, v.policy_version, v.policy_digest, v.product_id,
             v.account_class, v.schema_version, v.catalog_generation, v.switch_generation,
             v.content_digest, v.replacement_locked, now()
      FROM account_policy_versions v
      JOIN account_policy_bindings b ON b.user_id = $1
      WHERE v.binding_id = (SELECT id FROM account_policy_bindings WHERE user_id = $2)
    `, [strictCreated.id, pendingUser]);
    await seedClient.query(`
      UPDATE account_policy_bindings b
      SET desired_effective_version = v.effective_version, desired_digest = v.content_digest,
          applied_effective_version = v.effective_version, applied_digest = v.content_digest,
          policy_enforcement = 'strict', funding_enforcement = 'strict',
          reconciliation_state = 'verified', sync_state = 'confirmed',
          last_ack_at = now()
      FROM account_policy_versions v
      WHERE v.binding_id = b.id AND b.user_id = $1
    `, [strictCreated.id]);
    const strictUser = strictCreated.id;
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET policy_enforcement = 'shadow', reconciliation_state = 'verified',
          sync_state = 'pending', updated_at = now() - interval '2 hours'
      WHERE user_id = $1
    `, [pendingUser]);
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET updated_at = now() - interval '1 hour'
      WHERE user_id = $1
    `, [strictUser]);

    const ordered = await listPendingStrictChainAccounts(database, 10);
    expect(ordered.map((candidate) => candidate.userId)).toEqual([strictUser, pendingUser]);
  });

  it("records a blocked funding preflight on the binding and keeps the chain armed", async () => {
    const { userId, engineAccountId } = await registerUser("chain-blocked@example.test");
    await confirmShadowDelivery(userId);

    const engine = fakeEngine({ plan: blockedPlan(engineAccountId) });
    const candidate = (await listPendingStrictChainAccounts(database, 10))[0]!;
    const result = await advanceAccountStrictChain(database, engine.transport, candidate);
    expect(result.status).toBe("failed");
    expect((result as { error: string }).error).toContain(
      "active_legacy_reservation: reservation r1 is open",
    );
    // The failure is loud on the binding and the chain stays armed for the next sweep — the
    // account is never opted out and keeps working on its current path.
    expect(await bindingState(userId)).toMatchObject({
      policy_enforcement: "shadow",
      strict_chain_pending: true,
      last_error: "funding normalization is blocked: active_legacy_reservation: reservation r1 is open",
    });
    expect(engine.calls).toEqual(["plan"]);
  });

  it("records a non-transient opt-out rejection on the binding and keeps the chain armed", async () => {
    const { userId } = await registerUser("chain-rejected@example.test");
    await confirmShadowDelivery(userId);
    const staging = fakeEngine();
    await advanceAccountStrictChain(
      database,
      staging.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );

    const rejected = fakeEngine({
      optOut: {
        result: "rejected",
        code: "invalid",
        identity: {},
        rejection: { invalid: { reason: "account is disabled" } },
      },
    });
    const result = await advanceAccountStrictChain(
      database,
      rejected.transport,
      (await listPendingStrictChainAccounts(database, 10))[0]!,
    );
    expect(result.status).toBe("failed");
    expect(await bindingState(userId)).toMatchObject({
      strict_chain_pending: true,
      last_error: "pricing release opt-out rejected with invalid",
    });
  });

  it("stops arming the legacy conversion chain for post-cutover conversions and saves", async () => {
    await markCutoverCompleted();
    const user = await createEmailUser(database, "chain-post-cutover@example.test", "password-hash");
    const engineAccountId = `acct_chain_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "customer negotiated business terms",
      multiplierBp: 4_000,
    });
    expect((await bindingState(user.id))?.strict_chain_pending).toBe(false);

    await updateManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
      expectedVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
      actorId: "admin@example.test",
      reason: "post-cutover policy save",
    });
    expect((await bindingState(user.id))?.strict_chain_pending).toBe(false);
    expect(await listPendingStrictChainAccounts(database, 10)).toEqual([]);

    // The global B2C policy is pinned by the active release: post-cutover edits fail loudly
    // instead of silently diverging the panel from enforced prices.
    await expect(updateManagedPricingPolicy(database, {
      ownerType: "global_b2c",
      ownerId: "global-b2c",
      expectedVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 5_000,
      }],
      actorId: "admin@example.test",
      reason: "post-cutover global edit",
    })).rejects.toMatchObject({ code: "release_cycle_required" });

    // Service policies are pinned the same way: the release authority runs service as
    // meter_only, so a post-cutover editor save would version a legacy document that never
    // moves release-v2 billing while reporting success. It must fail just as loudly.
    await expect(updateManagedPricingPolicy(database, {
      ownerType: "service",
      ownerId: "service:content-studio",
      productId: "main",
      expectedVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 5_000,
      }],
      actorId: "admin@example.test",
      reason: "post-cutover service edit",
    })).rejects.toMatchObject({ code: "release_cycle_required" });
  });
});
