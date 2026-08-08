import { createHash, randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PricingReleaseAssignmentExtensionV2,
  PricingReleaseHeadV2,
  PricingReleasePolicyV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import { runStage5Backfill } from "./multi-discount-backfill.js";
import { stageProvisionedAccountStrictJob } from "./pricing-control-jobs.js";
import { materializeProvisionedUserPolicy } from "./pricing-policy-write.js";
import {
  syncPricingReleasePolicyOverrideV2,
  type PricingReleaseOverrideEngineV2,
} from "./pricing-release-override-v2.js";

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
}): {
  engine: PricingReleaseOverrideEngineV2;
  extensions: Map<string, PricingReleaseAssignmentExtensionV2>;
  policies: Map<string, PricingReleasePolicyV2>;
  calls: string[];
} {
  const extensions = new Map<string, PricingReleaseAssignmentExtensionV2>();
  const policies = new Map<string, PricingReleasePolicyV2>();
  const calls: string[] = [];
  return {
    extensions,
    policies,
    calls,
    engine: {
      getPricingReleaseHeadV2: async () => {
        calls.push("head");
        return input.head;
      },
      getPricingReleaseV2: async (generation: number) => {
        calls.push("release");
        return generation === input.target.generation ? input.target
          : generation === input.recovery.generation ? input.recovery : null;
      },
      preparePricingReleasePolicyV2: async (policy: PricingReleasePolicyV2) => {
        calls.push("policy-prepare");
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
      getPricingReleasePolicyV2: async (policyId: string, policyVersion: number) => {
        calls.push("policy-readback");
        return policies.get(`${policyId}:${policyVersion}`) ?? null;
      },
      getPricingReleaseAssignmentExtensionV2: async (headVersion: number, accountId: string) => {
        calls.push("extension-readback");
        return extensions.get(`${headVersion}:${accountId}`) ?? null;
      },
      preparePricingReleaseAssignmentExtensionV2: async (extension: PricingReleaseAssignmentExtensionV2) => {
        calls.push("extension-prepare");
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

describe.runIf(Boolean(connectionString))("post-cutover B2B release policy override", () => {
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
    databaseName = `pricing_override_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 8)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seed = new Client({ connectionString: url.toString() });
    await seed.connect();
    await migrate(drizzle(seed), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "pricing-release-override-v2-test");
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

  it("keeps the pre-cutover bypass while the global release head is absent", async () => {
    await seed.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 7000
      WHERE user_id = $1
    `, [userId]);
    await seed.query("UPDATE engine_accounts SET mult_bp = 7000 WHERE user_id = $1", [userId]);
    const state = fakeEngine({ head: null, target, recovery });
    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "pre_cutover" });
    expect(state.extensions.size).toBe(0);
  });

  it("overrides a base-covered B2B assignment with a strictly newer policy version, idempotently", async () => {
    const b2bAccountId = "acct_override_b2b";
    await seed.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 1500
      WHERE user_id = $1
    `, [userId]);
    await seed.query(
      "UPDATE engine_accounts SET mult_bp = 1500, engine_account_id = $2 WHERE user_id = $1",
      [userId, b2bAccountId],
    );
    const b2bPolicyId = `policy:main:b2b:${userId}`;
    await seed.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime, source_revision, observed_at
      ) VALUES (3, 1, 'override-capability', 'pricing-release-override-v2-test', 'test-revision', now())
    `);
    await seed.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation,
        capability_digest, content_digest, actor_type, actor_id, reason
      ) VALUES (
        'main', 3, 1, 3, 'override-capability', 'override-catalog',
        'system', 'pricing-release-override-v2-test', 'integration fixture'
      )
    `);
    await seed.query(`
      INSERT INTO pricing_policies (id, owner_type, owner_id, product_id)
      VALUES ($1, 'b2b_client', $2, 'main')
    `, [b2bPolicyId, userId]);
    await seed.query(`
      INSERT INTO pricing_policy_versions (
        policy_id, version, schema_version, product_id, catalog_generation,
        content_digest, actor_type, actor_id, reason
      ) VALUES ($1, 3, 1, 'main', 3, 'override-head-v3', 'admin', 'integration-test', 'extend b2b')
    `, [b2bPolicyId]);
    await seed.query(`
      INSERT INTO pricing_policy_heads (policy_id, current_version, current_digest)
      VALUES ($1, 3, 'override-head-v3')
    `, [b2bPolicyId]);
    await seed.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 3, 'main', 3, 'provider:anthropic:discount', 'override-rule-anthropic',
          'provider', 'anthropic', NULL, 'discount', 'managed', 8500, 1500, false, false, false),
        ($1, 3, 'main', 3, 'provider:google:discount', 'override-rule-google',
          'provider', 'google', NULL, 'discount', 'managed', 8500, 1500, false, false, false)
    `, [b2bPolicyId]);
    const basePolicyDigest = digest("b2b-base-policy");
    const overrideTarget: PricingReleaseV2 = {
      ...target,
      assignments: [
        ...target.assignments,
        {
          account_id: b2bAccountId,
          account_class: "b2b",
          policy_id: `release-v2:b2b:${b2bAccountId}`,
          policy_version: 2,
          policy_digest: basePolicyDigest,
          billing_mode: "balance",
          funding_generation: 1,
          purpose: null,
          responsible: null,
          assignment_digest: digest("b2b-base-assignment"),
        },
      ],
    };
    const state = fakeEngine({ head, target: overrideTarget, recovery });

    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: b2bAccountId,
    })).resolves.toEqual({ status: "override", headVersion: 1, policyVersion: 3 });

    const prepared = [...state.policies.values()]
      .find((policy) => policy.policy_id === `release-v2:b2b:${b2bAccountId}`)!;
    expect(prepared.policy_version).toBe(3);
    expect(prepared.rules).toHaveLength(2);
    expect(prepared.rules).toEqual(expect.arrayContaining([
      expect.objectContaining({
        scope: { scope: "provider", provider_id: "anthropic" },
        payable_multiplier_bp: 1500,
      }),
      expect.objectContaining({
        scope: { scope: "provider", provider_id: "google" },
        payable_multiplier_bp: 1500,
      }),
    ]));
    const extension = state.extensions.get(`1:${b2bAccountId}`);
    expect(extension?.members).toHaveLength(2);
    expect(extension?.members[0]?.assignment).toMatchObject({
      account_id: b2bAccountId,
      account_class: "b2b",
      policy_version: 3,
      billing_mode: "balance",
      funding_generation: 1,
    });

    // Double-run idempotency: the exact replay stores nothing new and succeeds again.
    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: b2bAccountId,
    })).resolves.toMatchObject({ status: "override" });
    expect(state.extensions.size).toBe(1);
    expect(state.policies.size).toBe(1);
  });

  // Post-cutover conversion fixture: commerce already says b2b, but the immutable base
  // assignment in the active release still says b2c — the exact state convertCustomerToBusiness
  // leaves behind until the class-changing extension lands.
  it("propagates a post-cutover B2C-to-B2B conversion through the class-changing extension", async () => {
    await seed.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 7000
      WHERE user_id = $1
    `, [userId]);
    await seed.query("UPDATE engine_accounts SET mult_bp = 7000 WHERE user_id = $1", [userId]);
    const convertedTarget: PricingReleaseV2 = {
      ...target,
      assignments: [
        ...target.assignments,
        {
          account_id: accountId,
          account_class: "b2c" as const,
          policy_id: "release-v2:b2c:global",
          policy_version: 1,
          policy_digest: digest("global-policy"),
          billing_mode: "balance" as const,
          funding_generation: 7,
          purpose: null,
          responsible: null,
          assignment_digest: digest("converted-base-assignment"),
        },
      ],
    };
    const state = fakeEngine({ head, target: convertedTarget, recovery });

    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "override", headVersion: 1, policyVersion: 1 });

    // The new per-account b2b lineage starts at version 1 — it is not comparable to the global
    // b2c base version, and the extension still pins the exact active/recovery pair.
    const policy = [...state.policies.values()]
      .find((item) => item.policy_id === `release-v2:b2b:${accountId}`)!;
    expect(policy.account_class).toBe("b2b");
    expect(policy.rules).toEqual([expect.objectContaining({
      scope: { scope: "provider", provider_id: "anthropic" },
      discount_bps: 3_000,
      payable_multiplier_bp: 7_000,
    })]);
    const extension = state.extensions.get(`1:${accountId}`);
    expect(extension?.members.map((member) => member.release_generation)).toEqual([10, 11]);
    expect(extension?.members[0]?.assignment).toMatchObject({
      account_id: accountId,
      account_class: "b2b",
      billing_mode: "balance",
      funding_generation: 7,
    });
  });

  // Phase 2.1 of the release-v2 retirement: an account whose commerce binding is already strict
  // (a new-account direct strict chain graduate or a pre-cutover strict conversion) is owned by
  // the policy_v1 delivery lane — the override must not write any release-v2 state for it.
  it("returns policy_owned for a strict binding without touching the release authority", async () => {
    // Build the strict binding through the real chain machinery: managed global policy
    // materialized, shadow delivery confirmed, atomic strict staging applied.
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [{ account_id: accountId, multiplier_bp: 5_000, status: "active" }],
      openkeys_accounts: [],
    }, { mode: "safe" });
    const materialized = await materializeProvisionedUserPolicy(database, {
      userId,
      engineAccountId: accountId,
    });
    expect(materialized.policyRequired).toBe(true);
    await seed.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', reconciliation_state = 'verified',
          last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [userId]);
    await seed.query(`
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
    await expect(stageProvisionedAccountStrictJob(database, { userId }))
      .resolves.toMatchObject({ status: "staged" });

    const state = fakeEngine({ head, target, recovery });
    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "policy_owned" });
    expect(state.calls).toEqual([]);
    expect(state.policies.size).toBe(0);
    expect(state.extensions.size).toBe(0);

    // A shadow binding keeps the release-lane behavior: the head is consulted as before.
    await seed.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 7000
      WHERE user_id = $1
    `, [userId]);
    await seed.query("UPDATE engine_accounts SET mult_bp = 7000 WHERE user_id = $1", [userId]);
    await seed.query(`
      UPDATE account_policy_bindings
      SET policy_enforcement = 'shadow', funding_enforcement = 'legacy_single'
      WHERE user_id = $1
    `, [userId]);
    await expect(syncPricingReleasePolicyOverrideV2(database, state.engine, {
      userId,
      engineAccountId: accountId,
    })).resolves.toEqual({ status: "not_covered" });
    expect(state.calls).toContain("head");
  });
});
