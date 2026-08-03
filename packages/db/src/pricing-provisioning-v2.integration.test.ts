import { createHash, randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type {
  FundingNormalizationPlanV2,
  PricingReleaseAssignmentExtensionV2,
  PricingReleaseHeadV2,
  PricingReleasePolicyV2,
  PricingReleaseRecoveryLinkV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import {
  ensurePricingReleaseProvisioningV2,
  PricingReleaseProvisioningV2Error,
  type PricingReleaseProvisioningEngineV2,
} from "./pricing-provisioning-v2.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

function digest(value: string): string {
  return `sha256:v2:${createHash("sha256").update(value, "utf8").digest("hex")}`;
}

function release(generation: number, kind: "target" | "recovery", contentDigest: string): PricingReleaseV2 {
  return {
    generation,
    release_kind: kind,
    schema_version: 2,
    capability_generation: 3,
    capability_digest: digest("capability"),
    main_catalog_generation: 3,
    main_catalog_digest: digest("main-catalog"),
    openkeys_catalog_generation: 3,
    openkeys_catalog_digest: digest("openkeys-catalog"),
    switch_generation: 3,
    switch_digest: digest("switches"),
    inventory_digest: digest(`inventory:${generation}`),
    policy_manifest_digest: digest(`policies:${generation}`),
    assignment_manifest_digest: digest(`assignments:${generation}`),
    funding_manifest_digest: digest(`funding:${generation}`),
    minimum_runtime_schema_version: 2,
    content_digest: contentDigest,
    assignments: [{
      account_id: "acct_existing",
      account_class: "b2c",
      policy_id: "release-v2:b2c:global",
      policy_version: 1,
      policy_digest: digest("global-policy"),
      billing_mode: "balance",
      funding_generation: 1,
      purpose: null,
      responsible: null,
      assignment_digest: digest(`base-assignment:${generation}`),
    }],
  };
}

function fakeEngine(input: {
  head: PricingReleaseHeadV2 | null;
  target: PricingReleaseV2;
  recovery: PricingReleaseV2;
  blockedFunding?: boolean;
}): {
  engine: PricingReleaseProvisioningEngineV2;
  extensions: Map<string, PricingReleaseAssignmentExtensionV2>;
  policies: Map<string, PricingReleasePolicyV2>;
  applyCalls: string[];
  setHead(head: PricingReleaseHeadV2 | null): void;
} {
  const extensions = new Map<string, PricingReleaseAssignmentExtensionV2>();
  const policies = new Map<string, PricingReleasePolicyV2>();
  const applyCalls: string[] = [];
  let currentHead = input.head;
  let normalized = false;
  const sourceDigest = digest("funding-source");
  const normalizationDigest = digest("funding-target");
  const plan = (status: "ready" | "blocked" | "normalized"): FundingNormalizationPlanV2 => ({
    account_id: "acct_post_cutover",
    account_status: "active",
    status,
    source: status === "normalized" ? "stored_generation" : "aggregate_paid_only",
    source_state_digest: sourceDigest,
    normalization_digest: status === "blocked" ? null : normalizationDigest,
    funding_generation: status === "blocked" ? null : 7,
    funding_head_version: status === "blocked" ? null : 1,
    balance_nano: "5000000000",
    reserved_nano: "0",
    spent_nano: "0",
    lots: status === "blocked" ? [] : [{
      lot_id: "fundv2_signup",
      source_type: "welcome_bonus",
      source_ref: "signup-bonus:test",
      balance_nano: "5000000000",
      reserved_nano: "0",
      spent_nano: "0",
      version: 1,
      status: "active",
    }],
    blockers: status === "blocked" ? [{ code: "active_legacy_reservation", detail: "legacy request" }] : [],
  });
  const link: PricingReleaseRecoveryLinkV2 = {
    target_generation: input.target.generation,
    target_digest: input.target.content_digest,
    recovery_generation: input.recovery.generation,
    recovery_digest: input.recovery.content_digest,
    link_digest: digest("recovery-link"),
  };
  return {
    extensions,
    policies,
    applyCalls,
    setHead: (head) => { currentHead = head; },
    engine: {
      getPricingReleaseHeadV2: async () => currentHead,
      getPricingReleaseV2: async (generation) =>
        generation === input.target.generation ? input.target
          : generation === input.recovery.generation ? input.recovery : null,
      getPricingReleaseRecoveryLinkV2: async (targetGeneration, recoveryGeneration) =>
        targetGeneration === link.target_generation && recoveryGeneration === link.recovery_generation ? link : null,
      preparePricingReleasePolicyV2: async (policy) => {
        const key = `${policy.policy_id}:${policy.policy_version}`;
        const result = policies.has(key) ? "unchanged" as const : "stored" as const;
        policies.set(key, structuredClone(policy));
        return {
          result,
          identity: {
            policy_id: policy.policy_id,
            policy_version: policy.policy_version,
            content_digest: policy.content_digest,
          },
        } as never;
      },
      getPricingReleasePolicyV2: async (policyId, policyVersion) =>
        policies.get(`${policyId}:${policyVersion}`) ?? null,
      getFundingNormalizationPlanV2: async () =>
        plan(input.blockedFunding ? "blocked" : normalized ? "normalized" : "ready"),
      applyFundingNormalizationV2: async (accountId) => {
        applyCalls.push(accountId);
        normalized = true;
        return { status: "stored", normalization: plan("normalized") };
      },
      getPricingReleaseAssignmentExtensionV2: async (headVersion, accountId) =>
        extensions.get(`${headVersion}:${accountId}`) ?? null,
      preparePricingReleaseAssignmentExtensionV2: async (extension) => {
        const accountId = extension.members[0]!.assignment.account_id;
        const key = `${extension.provisioning_head_version}:${accountId}`;
        const result = extensions.has(key) ? "unchanged" as const : "stored" as const;
        extensions.set(key, structuredClone(extension));
        return {
          result,
          identity: {
            provisioning_head_generation: extension.provisioning_head_generation,
            provisioning_head_version: extension.provisioning_head_version,
            account_id: accountId,
            extension_group_digest: extension.extension_group_digest,
          },
        } as never;
      },
    },
  };
}

describe.runIf(Boolean(connectionString))("post-cutover pricing assignment provisioning", () => {
  let admin: Client;
  let seed: Client;
  let database: Database;
  let databaseName: string;
  let userId: string;
  const accountId = "acct_post_cutover";
  const targetEngineDigest = digest("target-engine-release");
  const recoveryEngineDigest = digest("recovery-engine-release");
  const targetCommerceDigest = digest("target-commerce-plan");
  const recoveryCommerceDigest = digest("recovery-commerce-plan");
  const target = release(10, "target", targetEngineDigest);
  const recovery = release(11, "recovery", recoveryEngineDigest);
  const head: PricingReleaseHeadV2 = {
    active_generation: 10,
    active_digest: targetEngineDigest,
    head_version: 1,
    updated_ts: 1,
  };

  beforeAll(async () => {
    databaseName = `pricing_provision_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 8)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "pricing-provisioning-v2-test");
  }, TEST_TIMEOUT_MS);

  beforeEach(async () => {
    const tables = await seed.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    if (tables.rows.length > 0) {
      await seed.query(
        `TRUNCATE TABLE ${tables.rows.map((row) => quoteIdentifier(row.tablename)).join(", ")} RESTART IDENTITY CASCADE`,
      );
    }
    userId = randomUUID();
    await seed.query("INSERT INTO users (id, email, display_name) VALUES ($1,$2,'Post cutover')", [
      userId,
      `${userId}@example.test`,
    ]);
    await seed.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2c', 0, 5000, date_trunc('month', now())::date)
    `, [userId]);
    await seed.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1,$2,$3,5000,'active')
    `, [randomUUID(), userId, accountId]);

    const policyBase = {
      policy_id: "release-v2:b2c:global",
      policy_version: 1,
      owner_type: "global_b2c",
      owner_id: "global",
      account_class: "b2c",
      product_id: "main",
      billing_mode: "balance",
      schema_version: 2,
      capability_generation: target.capability_generation,
      capability_digest: target.capability_digest,
      catalog_generation: target.main_catalog_generation,
      catalog_digest: target.main_catalog_digest,
      switch_generation: target.switch_generation,
      switch_digest: target.switch_digest,
    };
    const ruleBase = {
      rule_id: "global-default",
      scope: { scope: "global" },
      discount_bps: 5000,
      payable_multiplier_bp: 5000,
    };
    const policyDigest = digest("global-policy");
    await seed.query(`
      INSERT INTO pricing_policy_documents_v2 (
        policy_id,policy_version,owner_type,owner_id,account_class,product_id,billing_mode,
        schema_version,capability_generation,capability_digest,catalog_generation,catalog_digest,
        switch_generation,switch_digest,content_digest
      ) VALUES ($1,1,$2,$3,$4,$5,$6,2,$7,$8,$9,$10,$11,$12,$13)
    `, [
      policyBase.policy_id,
      policyBase.owner_type,
      policyBase.owner_id,
      policyBase.account_class,
      policyBase.product_id,
      policyBase.billing_mode,
      policyBase.capability_generation,
      policyBase.capability_digest,
      policyBase.catalog_generation,
      policyBase.catalog_digest,
      policyBase.switch_generation,
      policyBase.switch_digest,
      policyDigest,
    ]);
    await seed.query(`
      INSERT INTO pricing_policy_rules_v2 (
        policy_id,policy_version,rule_id,rule_digest,scope_type,provider_id,
        canonical_model_id,discount_bps,payable_multiplier_bp
      ) VALUES ($1,1,$2,$3,'global',NULL,NULL,5000,5000)
    `, [policyBase.policy_id, ruleBase.rule_id, digest("global-rule")]);

    for (const item of [target, recovery]) {
      await seed.query(`
        INSERT INTO pricing_release_plans_v2 (
          generation,release_kind,schema_version,commerce_inventory_digest,engine_inventory_digest,
          openkeys_inventory_digest,service_inventory_digest,policy_manifest_digest,
          assignment_manifest_digest,funding_manifest_digest,engine_release_digest,content_digest,status
        ) VALUES ($1,$2,2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'planned')
      `, [
        item.generation,
        item.release_kind,
        digest(`commerce:${item.generation}`),
        item.inventory_digest,
        digest(`openkeys:${item.generation}`),
        digest(`service:${item.generation}`),
        item.policy_manifest_digest,
        item.assignment_manifest_digest,
        item.funding_manifest_digest,
        item.content_digest,
        item.generation === target.generation ? targetCommerceDigest : recoveryCommerceDigest,
      ]);
      await seed.query(`
        INSERT INTO pricing_release_assignments_v2 (
          release_generation,engine_account_id,account_class,owner_context,owner_id,
          policy_id,policy_version,policy_digest,billing_mode,funding_generation,
          purpose,responsible,assignment_digest
        ) VALUES ($1,'acct_existing','b2c','commerce',$2,$3,1,$4,'balance',1,NULL,NULL,$5)
      `, [item.generation, userId, policyBase.policy_id, policyDigest, digest(`base-assignment:${item.generation}`)]);
      await seed.query(`
        INSERT INTO pricing_funding_normalizations_v2 (
          release_generation,engine_account_id,funding_generation,expected_source_digest,
          target_funding_digest,applied_funding_digest,normalization_source,blockers,status
        ) VALUES ($1,'acct_existing',1,$2,$3,$3,'stored_generation',NULL,'ready')
      `, [item.generation, digest(`source:${item.generation}`), digest(`funding-assignment:${item.generation}`)]);
    }
    await seed.query("UPDATE pricing_release_plans_v2 SET status = 'prepared'");
    const evidenceDigest = digest("stage8-evidence");
    await seed.query(`
      INSERT INTO pricing_stage8_evidence_v2 (
        evidence_digest,target_generation,target_digest,recovery_generation,recovery_digest,
        commerce_inventory_digest,engine_inventory_digest,openkeys_inventory_digest,
        sales_contract_digest,funding_digest,shadow_digest,runtime_floor_digest,
        legacy_inflight_count,blocker_count,passed,observed_at,valid_until
      ) VALUES ($1,10,$2,11,$3,$4,$5,$6,$7,$8,$9,$10,0,0,true,now(),now()+interval '5 minutes')
    `, [
      evidenceDigest,
      targetCommerceDigest,
      recoveryCommerceDigest,
      digest("commerce-evidence"),
      target.inventory_digest,
      digest("openkeys-evidence"),
      digest("sales-contract"),
      target.funding_manifest_digest,
      digest("shadow"),
      digest("runtime-floor"),
    ]);
    await seed.query(`
      INSERT INTO pricing_release_activation_receipts_v2 (
        activation_id,activation_kind,release_generation,release_digest,
        evidence_digest,head_version,receipt_digest,activated_at
      ) VALUES ($1,'cutover',10,$2,$3,1,$4,now())
    `, [randomUUID(), targetCommerceDigest, evidenceDigest, digest("activation-receipt")]);
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
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

  it("normalizes funding and stores one exact active/recovery extension before succeeding", async () => {
    const state = fakeEngine({ head, target, recovery });
    await expect(ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "extension", headVersion: 1, releaseGeneration: 10 });

    expect(state.applyCalls).toEqual([accountId]);
    const extension = state.extensions.get(`1:${accountId}`)!;
    expect(extension.members.map((member) => member.release_generation)).toEqual([10, 11]);
    expect(extension.members.map((member) => member.assignment.funding_generation)).toEqual([7, 7]);
    expect(extension.members[0]!.assignment.policy_digest).toBe(digest("global-policy"));
    expect(extension.extension_group_digest).toMatch(/^sha256:v2:[0-9a-f]{64}$/);

    await expect(ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toMatchObject({ status: "extension" });
    expect(state.extensions.size).toBe(1);
  });

  it("reuses the paired member after a forward recovery head activation", async () => {
    const state = fakeEngine({ head, target, recovery });
    await ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    });
    await seed.query(`
      INSERT INTO pricing_release_activation_receipts_v2 (
        activation_id,activation_kind,release_generation,release_digest,
        evidence_digest,head_version,receipt_digest,activated_at
      ) VALUES ($1,'recovery',11,$2,$3,2,$4,now())
    `, [randomUUID(), recoveryCommerceDigest, digest("stage8-evidence"), digest("recovery-receipt")]);
    state.setHead({
      active_generation: 11,
      active_digest: recoveryEngineDigest,
      head_version: 2,
      updated_ts: 2,
    });

    await expect(ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "extension", headVersion: 2, releaseGeneration: 11 });
    expect(state.extensions.size).toBe(1);
    expect(state.applyCalls).toEqual([accountId]);
  });

  it("keeps the legacy path only while the global release head is absent", async () => {
    const state = fakeEngine({ head: null, target, recovery });
    await expect(ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "pre_cutover", headVersion: null, releaseGeneration: null });
    expect(state.applyCalls).toEqual([]);
    expect(state.extensions.size).toBe(0);
  });

  it("fails closed on a blocked account-local funding plan", async () => {
    const state = fakeEngine({ head, target, recovery, blockedFunding: true });
    const failure = await ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    }).catch((error) => error);
    expect(failure).toBeInstanceOf(PricingReleaseProvisioningV2Error);
    expect(failure).toMatchObject({ code: "funding_not_ready" });
    expect(state.extensions.size).toBe(0);
  });

  it("materializes a new B2B scalar only as an Anthropic release rule", async () => {
    await seed.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 7000
      WHERE user_id = $1
    `, [userId]);
    await seed.query("UPDATE engine_accounts SET mult_bp = 7000 WHERE user_id = $1", [userId]);
    const state = fakeEngine({ head, target, recovery });

    await expect(ensurePricingReleaseProvisioningV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toMatchObject({ status: "extension" });

    const policy = [...state.policies.values()][0]!;
    expect(policy).toMatchObject({
      owner_type: "b2b_client",
      owner_id: userId,
      account_class: "b2b",
      rules: [{
        scope: { scope: "provider", provider_id: "anthropic" },
        discount_bps: 3000,
        payable_multiplier_bp: 7000,
      }],
    });
    expect(policy.rules).toHaveLength(1);
    expect(state.extensions.get(`1:${accountId}`)?.members[0]?.assignment.account_class).toBe("b2b");
  });
});
