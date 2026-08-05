import { createHash, randomUUID } from "node:crypto";
import { drizzle } from "drizzle-orm/node-postgres";
import { migrate } from "drizzle-orm/node-postgres/migrator";
import { Client } from "pg";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import type { PricingPolicyEditorRule } from "@claude-api/contracts";
import { createEmailUser } from "./auth.js";
import { createDatabase, type Database } from "./client.js";
import { MIGRATIONS_FOLDER } from "./migrate.js";
import { completeExternalSignIn } from "./oauth.js";
import {
  assertUserPolicyReadyForKey,
  getManagedPricingCatalog,
  getManagedPricingPolicy,
  materializeProvisionedUserPolicy,
  repairDeadPreCutoverPolicyDelivery,
  updateManagedPricingPolicy,
  updateManagedProviderSwitches,
} from "./pricing-policy-write.js";
import {
  claimNextPricingControlJob,
  confirmPricingControlJob,
  type ClaimedPricingControlJob,
} from "./pricing-control-jobs.js";
import { createBusinessInvite, convertCustomerToBusiness, rotateBusinessInvite } from "./pricing.js";
import { runStage5Backfill } from "./multi-discount-backfill.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;

const ANTHROPIC_60: PricingPolicyEditorRule = {
  scope: { provider: { providerId: "anthropic" } },
  pricingMode: "discount",
  discountBps: 6_000,
};
const OPENAI_50: PricingPolicyEditorRule = {
  scope: { provider: { providerId: "openai" } },
  pricingMode: "discount",
  discountBps: 5_000,
};

function quoteIdentifier(identifier: string): string {
  if (!/^[a-z][a-z0-9_]*$/.test(identifier)) throw new Error(`unsafe identifier ${identifier}`);
  return `"${identifier}"`;
}

describe.runIf(Boolean(connectionString))("managed multi-discount policy writes", () => {
  let admin: Client;
  let seedClient: Client;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `policy_write_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    admin = new Client({ connectionString });
    await admin.connect();
    await admin.query(`CREATE DATABASE ${quoteIdentifier(databaseName)}`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    seedClient = new Client({ connectionString: url.toString() });
    await seedClient.connect();
    await migrate(drizzle(seedClient), { migrationsFolder: MIGRATIONS_FOLDER });
    database = createDatabase(url.toString(), "policy-write-test");
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

  it("creates a complete invitation policy atomically and replays only the exact policy", async () => {
    const idempotencyKey = randomUUID();
    const input = inviteInput(idempotencyKey, [ANTHROPIC_60, OPENAI_50]);
    const created = await createBusinessInvite(database, input);
    const replay = await createBusinessInvite(database, {
      ...input,
      tokenHash: tokenHash("replayed-token"),
      encryptedToken: "different-ciphertext",
    });

    expect(replay).toMatchObject({ id: created.id, idempotentReplay: true });
    await expect(getManagedPricingPolicy(database, {
      ownerType: "b2b_invitation",
      ownerId: created.id,
    })).resolves.toMatchObject({
      currentVersion: 1,
      rules: [ANTHROPIC_60, OPENAI_50],
      targets: [],
    });
    await expect(createBusinessInvite(database, {
      ...input,
      tokenHash: tokenHash("conflicting-token"),
      encryptedToken: "conflicting-ciphertext",
      policyRules: [ANTHROPIC_60],
    })).rejects.toMatchObject({ code: "version_conflict" });
    await expect(createBusinessInvite(database, {
      ...input,
      idempotencyKey: randomUUID(),
      multiplierBp: 4_000,
    })).rejects.toMatchObject({ code: "invalid_owner_rule" });
    const stored = await seedClient.query<{ policies: string; bindings: string }>(`
      SELECT
        (SELECT count(*)::text FROM pricing_policies WHERE owner_type = 'b2b_invitation') AS policies,
        (SELECT count(*)::text FROM business_invite_policy_bindings) AS bindings
    `);
    expect(stored.rows[0]).toEqual({ policies: "1", bindings: "1" });
  });

  it("enforces full replacement CAS, catalog membership, uniqueness, and static B2B rules", async () => {
    const created = await createBusinessInvite(database, inviteInput(randomUUID(), [ANTHROPIC_60]));
    const base = {
      ownerType: "b2b_invitation" as const,
      ownerId: created.id,
      expectedVersion: 1,
      actorId: "admin@example.test",
      reason: "update negotiated provider policy",
    };

    await expect(updateManagedPricingPolicy(database, {
      ...base,
      rules: [{ ...ANTHROPIC_60, pricingMode: "track", discountBps: null }],
    })).rejects.toMatchObject({ code: "invalid_owner_rule" });
    await expect(updateManagedPricingPolicy(database, {
      ...base,
      rules: [{
        scope: { provider: { providerId: "gemini" } },
        pricingMode: "discount",
        discountBps: 5_000,
      }],
    })).rejects.toMatchObject({ code: "rule_outside_catalog" });
    await expect(updateManagedPricingPolicy(database, { ...base, rules: [] })).rejects.toThrow();
    await expect(updateManagedPricingPolicy(database, {
      ...base,
      rules: [ANTHROPIC_60, { ...ANTHROPIC_60, discountBps: 5_000 }],
    })).rejects.toThrow();

    const updated = await updateManagedPricingPolicy(database, {
      ...base,
      rules: [ANTHROPIC_60, OPENAI_50],
    });
    expect(updated).toMatchObject({ currentVersion: 2, rules: [ANTHROPIC_60, OPENAI_50] });
    await expect(updateManagedPricingPolicy(database, {
      ...base,
      rules: [ANTHROPIC_60],
    })).rejects.toMatchObject({ code: "version_conflict" });
  });

  it("copies the exact invitation snapshot through OAuth redemption", async () => {
    const token = `oauth-invite-${randomUUID()}`;
    const created = await createBusinessInvite(database, {
      ...inviteInput(randomUUID(), [ANTHROPIC_60, OPENAI_50]),
      email: "oauth-buyer@example.test",
      tokenHash: tokenHash(token),
    });
    const user = await completeExternalSignIn(database, {
      provider: "github",
      subject: `github-${randomUUID()}`,
      email: "oauth-buyer@example.test",
      emailVerified: true,
      displayName: "OAuth Buyer",
      metadata: { login: "oauth-buyer" },
    }, tokenHash(token));

    expect(user).toMatchObject({ customerType: "b2b", isNewAccount: true });
    const [invitationPolicy, clientPolicy] = await Promise.all([
      getManagedPricingPolicy(database, { ownerType: "b2b_invitation", ownerId: created.id }),
      getManagedPricingPolicy(database, { ownerType: "b2b_client", ownerId: user.id }),
    ]);
    expect(invitationPolicy).not.toBeNull();
    expect(clientPolicy).not.toBeNull();
    expect(clientPolicy).toMatchObject({ currentVersion: 1, rules: invitationPolicy!.rules });
    const audit = await seedClient.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM audit_log
      WHERE action = 'business_invite.policy_copied' AND target_id = $1
    `, [clientPolicy!.policyId]);
    expect(audit.rows[0]?.metadata).toMatchObject({
      inviteId: created.id,
      invitationPolicyVersion: invitationPolicy!.currentVersion,
      invitationPolicyDigest: invitationPolicy!.currentDigest,
      clientPolicyVersion: clientPolicy!.currentVersion,
      clientPolicyDigest: clientPolicy!.currentDigest,
    });
  });

  it("re-points the backfilled B2C binding to the client policy on manual conversion", async () => {
    // Production state for long-lived B2C customers: the Stage 5 backfill already bound the
    // account to the global B2C policy, account_policy_bindings allows exactly one row per
    // user, and the engine confirmed the backfilled delivery — which fixed the account's
    // legacy delivery lineage. Conversion re-points the binding, but the identity switch is
    // NOT staged through the legacy lane (the engine would reject it with version_conflict);
    // it ships via the release-cutover lane, and any drifted desired state is folded back to
    // the engine-confirmed applied state.
    const user = await createEmailUser(database, "backfilled-b2c@example.test", "password-hash");
    const engineAccountId = `acct_backfilled_${user.id.replaceAll("-", "")}`;
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, engineAccountId]);
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [{ account_id: engineAccountId, multiplier_bp: 4_000, status: "active" }],
      openkeys_accounts: [],
    }, { mode: "safe" });
    // The engine acknowledged the backfilled delivery: applied v1 is what the account runs.
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [user.id]);
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
    const before = await seedClient.query<{ account_class: string; policy_id: string }>(`
      SELECT account_class, policy_id FROM account_policy_bindings WHERE user_id = $1
    `, [user.id]);
    expect(before.rows).toEqual([{ account_class: "b2c", policy_id: "policy:main:global-b2c" }]);

    const converted = await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "customer negotiated business terms",
      multiplierBp: 2_000,
    });
    expect(converted).toMatchObject({ converted: true, multiplierBp: 2_000 });
    expect(converted.jobId).not.toBeNull(); // legacy scalar delivery, unaffected by policy lineage

    const binding = await seedClient.query<{
      account_class: string;
      policy_id: string;
      sync_state: string;
      desired_effective_version: string | null;
      applied_effective_version: string | null;
      last_error: string | null;
    }>(`
      SELECT account_class, policy_id, sync_state, last_error,
             desired_effective_version::text, applied_effective_version::text
      FROM account_policy_bindings WHERE user_id = $1
    `, [user.id]);
    // The binding now aims at the B2B client policy, but no new effective version is staged:
    // the engine still runs the confirmed backfilled v1 and the identity switch is cutover work.
    expect(binding.rows).toEqual([{
      account_class: "b2b",
      policy_id: `policy:main:b2b:${user.id}`,
      sync_state: "confirmed",
      desired_effective_version: "1",
      applied_effective_version: "1",
      last_error: null,
    }]);
    const staged = await seedClient.query<{ versions: string; jobs: string }>(`
      SELECT
        (SELECT count(*)::text FROM account_policy_versions) AS versions,
        (SELECT count(*)::text FROM engine_policy_jobs) AS jobs
    `);
    expect(staged.rows[0]).toEqual({ versions: "1", jobs: "1" });
    const policy = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
    });
    expect(policy).toMatchObject({
      currentVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 8_000,
      }],
    });
    expect(policy!.targets).toHaveLength(1);
    expect(policy!.targets[0]).toMatchObject({
      accountClass: "b2b",
      desiredVersion: 1,
      appliedVersion: 1,
      syncState: "confirmed",
      deliveryState: "confirmed",
    });

    // Regression for the production "dead delivery" state: the earlier staging left converted
    // customers with a desired version the engine keeps rejecting. Saving the managed policy
    // must heal the binding back to the engine-confirmed state instead of stacking another
    // rejected delivery on top; the source policy itself still versions normally. The drift
    // mirrors production: the buggy staging created an immutable v2 effective row aimed at the
    // client policy, which the engine then refused to prepare.
    await seedClient.query(`
      INSERT INTO account_policy_versions (
        binding_id, effective_version, policy_id, policy_version, policy_digest,
        product_id, account_class, schema_version, catalog_generation,
        switch_generation, content_digest, replacement_locked
      )
      SELECT binding.id, 2, source.policy_id, source.version, source.content_digest,
             source.product_id, 'b2b', source.schema_version, source.catalog_generation,
             applied.switch_generation, 'undeliverable', applied.replacement_locked
      FROM account_policy_bindings binding
      JOIN pricing_policy_versions source
        ON source.policy_id = $2 AND source.version = 1
      JOIN account_policy_versions applied
        ON applied.binding_id = binding.id AND applied.effective_version = 1
      WHERE binding.user_id = $1
    `, [user.id, `policy:main:b2b:${user.id}`]);
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET desired_effective_version = 2, desired_digest = 'undeliverable',
          sync_state = 'pending',
          last_error = 'account-policy prepare rejected with version_conflict'
      WHERE user_id = $1
    `, [user.id]);
    const updated = await updateManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
      expectedVersion: 1,
      rules: [{ ...ANTHROPIC_60, discountBps: 7_000 }],
      actorId: "admin@example.test",
      reason: "adjust negotiated discount",
    });
    expect(updated.currentVersion).toBe(2);
    const healed = await seedClient.query<{
      sync_state: string;
      desired_effective_version: string | null;
      applied_effective_version: string | null;
      last_error: string | null;
    }>(`
      SELECT sync_state, last_error,
             desired_effective_version::text, applied_effective_version::text
      FROM account_policy_bindings WHERE user_id = $1
    `, [user.id]);
    expect(healed.rows).toEqual([{
      sync_state: "confirmed",
      desired_effective_version: "1",
      applied_effective_version: "1",
      last_error: null,
    }]);
    // The immutable drifted v2 row stays as history; no new delivery job was staged.
    const afterHeal = await seedClient.query<{ versions: string; jobs: string }>(`
      SELECT
        (SELECT count(*)::text FROM account_policy_versions) AS versions,
        (SELECT count(*)::text FROM engine_policy_jobs) AS jobs
    `);
    expect(afterHeal.rows[0]).toEqual({ versions: "2", jobs: "1" });
  });

  it("provisions a managed policy on manual B2B conversion and repairs pre-policy conversions", async () => {
    const user = await createEmailUser(database, "convert-buyer@example.test", "password-hash");
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [user.id, `acct_convert_${user.id.replaceAll("-", "")}`]);

    const converted = await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "customer negotiated business terms",
      multiplierBp: 2_000,
    });
    expect(converted).toMatchObject({ converted: true, multiplierBp: 2_000 });
    const policy = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
    });
    expect(policy).toMatchObject({
      policyId: `policy:main:b2b:${user.id}`,
      currentVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 8_000,
      }],
    });
    expect(policy!.targets).toHaveLength(1);
    expect(policy!.targets[0]).toMatchObject({
      accountClass: "b2b",
      desiredVersion: 1,
      syncState: "pending",
      deliveryState: "pending",
    });
    const jobs = await seedClient.query<{ policy_jobs: string; legacy_jobs: string }>(`
      SELECT
        (SELECT count(*)::text FROM engine_policy_jobs) AS policy_jobs,
        (SELECT count(*)::text FROM engine_pricing_jobs) AS legacy_jobs
    `);
    expect(jobs.rows[0]).toEqual({ policy_jobs: "1", legacy_jobs: "1" });
    const conversionAudit = await seedClient.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM audit_log WHERE action = 'pricing.b2b_converted' AND target_id = $1
    `, [user.id]);
    expect(conversionAudit.rows[0]?.metadata).toMatchObject({
      managedPolicyId: `policy:main:b2b:${user.id}`,
      managedPolicyVersion: 1,
    });

    // Safe retry: a fully provisioned B2B customer stays an unchanged no-op.
    const retry = await convertCustomerToBusiness(database, {
      userId: user.id,
      actorId: "admin@example.test",
      reason: "safe retry",
      multiplierBp: 3_000,
    });
    expect(retry).toMatchObject({ converted: false, multiplierBp: 2_000, jobId: null });
    const afterRetry = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: user.id,
    });
    expect(afterRetry!.currentVersion).toBe(1);

    // A customer converted before managed-policy provisioning existed carries no b2b_client
    // policy; re-running the conversion repairs exactly that gap against the multiplier already
    // in effect and ignores the scalar passed with the retry.
    const legacy = await createEmailUser(database, "legacy-b2b@example.test", "password-hash");
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [legacy.id, `acct_legacy_${legacy.id.replaceAll("-", "")}`]);
    await seedClient.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 4_000
      WHERE user_id = $1
    `, [legacy.id]);
    const repaired = await convertCustomerToBusiness(database, {
      userId: legacy.id,
      actorId: "admin@example.test",
      reason: "repair missing managed policy",
      multiplierBp: 9_000,
    });
    expect(repaired).toMatchObject({ converted: false, multiplierBp: 4_000 });
    expect(repaired.jobId).not.toBeNull();
    const repairedPolicy = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: legacy.id,
    });
    expect(repairedPolicy).toMatchObject({
      currentVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
    });
    expect(repairedPolicy!.targets).toHaveLength(1);
    const profile = await seedClient.query<{ multiplier_bp: number }>(`
      SELECT multiplier_bp FROM customer_profiles WHERE user_id = $1
    `, [legacy.id]);
    expect(profile.rows[0]?.multiplier_bp).toBe(4_000);
    const repairAudit = await seedClient.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM audit_log
      WHERE action = 'pricing.b2b_policy_provisioned' AND target_id = $1
    `, [legacy.id]);
    expect(repairAudit.rows).toHaveLength(1);
    expect(repairAudit.rows[0]?.metadata).toMatchObject({
      multiplierBp: 4_000,
      policyVersion: 1,
    });

    // A scalar that does not map to a whole-percent managed discount is rejected loudly instead
    // of silently rounding money.
    const odd = await createEmailUser(database, "odd-b2b@example.test", "password-hash");
    await seedClient.query(`
      UPDATE engine_accounts SET engine_account_id = $2, status = 'active' WHERE user_id = $1
    `, [odd.id, `acct_odd_${odd.id.replaceAll("-", "")}`]);
    await seedClient.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 3_750
      WHERE user_id = $1
    `, [odd.id]);
    await expect(convertCustomerToBusiness(database, {
      userId: odd.id,
      actorId: "admin@example.test",
      reason: "repair attempt with a fractional-percent scalar",
      multiplierBp: 3_750,
    })).rejects.toThrow("whole-percent");
  });

  it("rotates a policy invitation as an independent exact snapshot with a neutral scalar placeholder", async () => {
    const created = await createBusinessInvite(database, inviteInput(randomUUID(), [ANTHROPIC_60, OPENAI_50]));
    // Stage 5 can attach a full policy to a legacy invitation whose historical scalar was not
    // neutral. Rotation must not carry that scalar forward as authority or presentation data.
    await seedClient.query("UPDATE business_invites SET multiplier_bp = 4000 WHERE id = $1", [created.id]);
    const replacementIdempotencyKey = randomUUID();
    const replacement = await rotateBusinessInvite(database, {
      inviteId: created.id,
      tokenHash: tokenHash(`replacement-${randomUUID()}`),
      encryptedToken: "replacement-ciphertext",
      expiresAt: new Date(Date.now() + 172_800_000),
      idempotencyKey: replacementIdempotencyKey,
      actorId: "admin@example.test",
      reason: "rotate the invitation without changing negotiated policy",
    });
    expect(replacement.multiplierBp).toBe(10_000);
    const [sourcePolicy, replacementPolicy] = await Promise.all([
      getManagedPricingPolicy(database, { ownerType: "b2b_invitation", ownerId: created.id }),
      getManagedPricingPolicy(database, { ownerType: "b2b_invitation", ownerId: replacement.id }),
    ]);
    expect(replacementPolicy).toMatchObject({ currentVersion: 1, rules: sourcePolicy!.rules });
    expect(replacementPolicy!.policyId).not.toBe(sourcePolicy!.policyId);
    const replay = await rotateBusinessInvite(database, {
      inviteId: created.id,
      tokenHash: tokenHash("ignored-replay-token"),
      encryptedToken: "ignored-replay-ciphertext",
      expiresAt: new Date(Date.now() + 259_200_000),
      idempotencyKey: replacementIdempotencyKey,
      actorId: "admin@example.test",
      reason: "exact replay",
    });
    expect(replay).toMatchObject({ id: replacement.id, idempotentReplay: true, multiplierBp: 10_000 });
  });

  it("updates provider switches as one versioned full replacement and preserves unrelated scopes", async () => {
    const token = `switch-policy-${randomUUID()}`;
    const invitation = await createBusinessInvite(database, {
      ...inviteInput(randomUUID(), [ANTHROPIC_60]),
      tokenHash: tokenHash(token),
    });
    const user = await createEmailUser(database, "buyer@example.test", "password-hash", tokenHash(token));
    expect(invitation.id).toBeTruthy();
    await materializeProvisionedUserPolicy(database, {
      userId: user.id,
      engineAccountId: `acct_switch_${user.id.replaceAll("-", "")}`,
    });
    const current = await getManagedPricingCatalog(database);
    expect(current).toMatchObject({
      catalogGeneration: 1,
      switchGeneration: 1,
      providers: [
        { providerId: "anthropic", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: true },
        { providerId: "openai", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: true },
      ],
    });
    const updated = await updateManagedProviderSwitches(database, {
      expectedGeneration: current.switchGeneration,
      reason: "disable Anthropic only for new B2B admissions",
      actorId: "admin@example.test",
      providers: [{
        providerId: "anthropic",
        masterEnabled: true,
        productEnabled: true,
        b2cEnabled: true,
        b2bEnabled: false,
      }],
    });
    expect(updated).toMatchObject({
      switchGeneration: 2,
      switchSyncState: "pending",
      providers: [
        { providerId: "anthropic", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: false },
        { providerId: "openai", masterEnabled: true, productEnabled: true, b2cEnabled: true, b2bEnabled: true },
      ],
    });
    await expect(updateManagedProviderSwitches(database, {
      expectedGeneration: 1,
      reason: "stale switch replacement must not win",
      actorId: "admin@example.test",
      providers: [{
        providerId: "anthropic",
        masterEnabled: true,
        productEnabled: true,
        b2cEnabled: true,
        b2bEnabled: true,
      }],
    })).rejects.toMatchObject({ code: "version_conflict" });
    const stored = await seedClient.query<{
      entries: string;
      openkeys_enabled: string;
      jobs: string;
      policy_versions: string;
      desired_switch_generation: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM provider_switch_entries WHERE generation = 2) AS entries,
        (SELECT count(*)::text FROM provider_switch_entries
          WHERE generation = 2 AND product_id = 'openkeys' AND enabled) AS openkeys_enabled,
        (SELECT count(*)::text FROM engine_switch_jobs WHERE generation = 2) AS jobs,
        (SELECT count(*)::text FROM account_policy_versions version
          JOIN account_policy_bindings binding ON binding.id = version.binding_id
          WHERE binding.user_id = $1) AS policy_versions,
        (SELECT version.switch_generation::text FROM account_policy_bindings binding
          JOIN account_policy_versions version
            ON version.binding_id = binding.id AND version.effective_version = binding.desired_effective_version
          WHERE binding.user_id = $1) AS desired_switch_generation
    `, [user.id]);
    expect(stored.rows[0]).toEqual({
      entries: "10",
      openkeys_enabled: "2",
      jobs: "1",
      policy_versions: "2",
      desired_switch_generation: "2",
    });
  });

  it("serializes invitation edit against redemption and blocks keys until the exact policy ACK", async () => {
    const token = `invite-${randomUUID()}`;
    const created = await createBusinessInvite(database, {
      ...inviteInput(randomUUID(), [ANTHROPIC_60]),
      tokenHash: tokenHash(token),
    });
    const edit = updateManagedPricingPolicy(database, {
      ownerType: "b2b_invitation",
      ownerId: created.id,
      expectedVersion: 1,
      rules: [ANTHROPIC_60, OPENAI_50],
      actorId: "admin@example.test",
      reason: "race a complete policy edit with redemption",
    });
    const redemption = createEmailUser(database, "buyer@example.test", "password-hash", tokenHash(token));
    const [editResult, redemptionResult] = await Promise.allSettled([edit, redemption]);
    if (redemptionResult.status !== "fulfilled") throw redemptionResult.reason;
    if (editResult.status === "rejected") {
      expect(editResult.reason).toMatchObject({ code: "invitation_not_editable" });
    }

    const userId = redemptionResult.value.id;
    const policies = await seedClient.query<{
      owner_type: string;
      version: string;
      content_digest: string;
      rules: unknown;
    }>(`
      SELECT policy.owner_type, head.current_version::text AS version,
             head.current_digest AS content_digest,
             jsonb_agg(jsonb_build_object(
               'scope_type', rule.scope_type,
               'provider_id', rule.provider_id,
               'canonical_model_id', rule.canonical_model_id,
               'pricing_mode', rule.pricing_mode,
               'discount_bps', rule.discount_bps
             ) ORDER BY rule.provider_id, rule.scope_type, rule.canonical_model_id) AS rules
      FROM pricing_policies policy
      JOIN pricing_policy_heads head ON head.policy_id = policy.id
      JOIN pricing_policy_rules rule
        ON rule.policy_id = head.policy_id AND rule.policy_version = head.current_version
      WHERE (policy.owner_type = 'b2b_invitation' AND policy.owner_id = $1)
         OR (policy.owner_type = 'b2b_client' AND policy.owner_id = $2)
      GROUP BY policy.owner_type, head.current_version, head.current_digest
      ORDER BY policy.owner_type DESC
    `, [created.id, userId]);
    expect(policies.rows).toHaveLength(2);
    expect(policies.rows[0]!.owner_type).toBe("b2b_invitation");
    expect(policies.rows[1]!.owner_type).toBe("b2b_client");
    expect(policies.rows[1]!.rules).toEqual(policies.rows[0]!.rules);
    const copiedAudit = await seedClient.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM audit_log
      WHERE action = 'business_invite.policy_copied' AND target_id LIKE $1
    `, [`policy:main:b2b:${userId}`]);
    expect(copiedAudit.rows[0]?.metadata).toMatchObject({
      invitationPolicyVersion: Number(policies.rows[0]!.version),
      invitationPolicyDigest: policies.rows[0]!.content_digest,
    });

    // A copied source policy is already authority: absence of its binding/ACK must fail closed,
    // including the race before provisioning has materialized the effective account policy.
    await expect(assertUserPolicyReadyForKey(database, userId))
      .rejects.toMatchObject({ code: "provisioning_policy_missing" });

    const staged = await materializeProvisionedUserPolicy(database, {
      userId,
      engineAccountId: `acct_policy_${userId.replaceAll("-", "")}`,
    });
    expect(staged).toMatchObject({ policyRequired: true, ready: false, jobId: expect.any(String) });
    await expect(assertUserPolicyReadyForKey(database, userId))
      .rejects.toMatchObject({ code: "provisioning_policy_missing" });

    let policyJob: Extract<ClaimedPricingControlJob, { kind: "policy" }> | null = null;
    for (let index = 0; index < 5; index += 1) {
      const job = await claimNextPricingControlJob(database, `policy-write-${index}`);
      if (!job) break;
      if (job.kind === "catalog") {
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: { catalog: job.spec, expectation: "absent" },
        });
      } else if (job.kind === "switches") {
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: { switches: job.spec, expectation: "absent" },
        });
      } else {
        policyJob = job;
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: {
            policy: job.spec,
            activation: {
              account_id: job.spec.account_id,
              effective_version: job.spec.effective_version,
              content_digest: job.spec.content_digest,
              binding: job.binding,
            },
            expectation: "unbound",
          },
        });
      }
    }
    expect(policyJob).not.toBeNull();
    await expect(assertUserPolicyReadyForKey(database, userId)).resolves.toBeUndefined();
    const account = await seedClient.query<{ status: string }>(
      "SELECT status FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    expect(account.rows[0]?.status).toBe("active");
    const archived = await seedClient.query<{
      copied_to_user_id: string;
      copied_client_policy_digest: string;
      redeemed_source_policy_digest: string;
    }>(`
      SELECT copied_to_user_id::text, copied_client_policy_digest, redeemed_source_policy_digest
      FROM business_invite_policy_bindings WHERE invite_id = $1
    `, [created.id]);
    expect(archived.rows[0]).toMatchObject({
      copied_to_user_id: userId,
      copied_client_policy_digest: policies.rows[1]!.content_digest,
      redeemed_source_policy_digest: policies.rows[0]!.content_digest,
    });
  });

  it("replaces only the exact dead strict + legacy_single delivery and preserves immutable history", async () => {
    const token = `repair-invite-${randomUUID()}`;
    const email = `policy-repair-${randomUUID()}@example.test`;
    await createBusinessInvite(database, {
      ...inviteInput(randomUUID(), [ANTHROPIC_60]),
      email,
      tokenHash: tokenHash(token),
    });
    const user = await createEmailUser(
      database,
      email,
      "password-hash",
      tokenHash(token),
    );
    const engineAccountId = `acct_repair_${user.id.replaceAll("-", "")}`;
    const staged = await materializeProvisionedUserPolicy(database, {
      userId: user.id,
      engineAccountId,
    });
    expect(staged).toMatchObject({ policyRequired: true, ready: false, jobId: expect.any(String) });

    const original = await seedClient.query<{
      id: string;
      binding_id: string;
      effective_version: string;
      content_digest: string;
    }>(`
      SELECT id::text, binding_id::text, effective_version::text, content_digest
      FROM engine_policy_jobs WHERE id = $1
    `, [staged.jobId]);
    const terminal = original.rows[0]!;
    await seedClient.query(`
      UPDATE engine_policy_jobs
      SET status = 'dead', attempts = 1,
          payload = jsonb_set(payload, '{binding,policy_enforcement}', '"strict"'::jsonb),
          last_error = 'account-policy activation rejected with invalid', updated_at = now()
      WHERE id = $1
    `, [terminal.id]);
    await seedClient.query(`
      UPDATE account_policy_bindings
      SET sync_state = 'failed', last_error = 'account-policy activation rejected with invalid',
          updated_at = now()
      WHERE id = $1
    `, [terminal.binding_id]);

    const repaired = await repairDeadPreCutoverPolicyDelivery(database, {
      jobId: terminal.id,
      expectedEffectiveVersion: Number(terminal.effective_version),
      expectedContentDigest: terminal.content_digest,
      actorId: "operator@example.test",
      reason: "repair the historical pre-cutover compatibility failure",
    });
    expect(repaired).toMatchObject({
      status: "queued",
      superseded_job_id: terminal.id,
      replacement_job_id: expect.any(String),
      binding_id: terminal.binding_id,
      engine_account_id: engineAccountId,
      previous_effective_version: 1,
      replacement_effective_version: 2,
    });

    const stored = await seedClient.query<{
      id: string;
      status: string;
      effective_version: string;
      binding: Record<string, string>;
      desired_effective_version: string;
      sync_state: string;
    }>(`
      SELECT job.id::text, job.status, job.effective_version::text,
             job.payload->'binding' AS binding,
             binding.desired_effective_version::text,
             binding.sync_state
      FROM engine_policy_jobs job
      JOIN account_policy_bindings binding ON binding.id = job.binding_id
      WHERE job.binding_id = $1
      ORDER BY job.effective_version
    `, [terminal.binding_id]);
    expect(stored.rows).toEqual([
      expect.objectContaining({
        id: terminal.id,
        status: "superseded",
        effective_version: "1",
        desired_effective_version: "2",
        sync_state: "pending",
        binding: {
          policy_enforcement: "strict",
          funding_enforcement: "legacy_single",
          reconciliation_state: "verified",
        },
      }),
      expect.objectContaining({
        id: repaired.replacement_job_id,
        status: "pending",
        effective_version: "2",
        desired_effective_version: "2",
        sync_state: "pending",
        binding: {
          policy_enforcement: "shadow",
          funding_enforcement: "legacy_single",
          reconciliation_state: "verified",
        },
      }),
    ]);
    await expect(repairDeadPreCutoverPolicyDelivery(database, {
      jobId: terminal.id,
      expectedEffectiveVersion: 1,
      expectedContentDigest: terminal.content_digest,
      actorId: "operator@example.test",
      reason: "exact retry after a lost response",
    })).resolves.toMatchObject({
      status: "unchanged",
      replacement_job_id: repaired.replacement_job_id,
    });
    await expect(repairDeadPreCutoverPolicyDelivery(database, {
      jobId: terminal.id,
      expectedEffectiveVersion: 1,
      expectedContentDigest: `sha256:v1:${"0".repeat(64)}`,
      actorId: "operator@example.test",
      reason: "stale identity must not win",
    })).rejects.toMatchObject({ code: "repair_job_changed" });

    let replacementClaimed = false;
    for (let index = 0; index < 5; index += 1) {
      const job = await claimNextPricingControlJob(database, `policy-repair-${index}`);
      if (!job) break;
      if (job.kind === "catalog") {
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: { catalog: job.spec, expectation: "absent" },
        });
      } else if (job.kind === "switches") {
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: { switches: job.spec, expectation: "absent" },
        });
      } else {
        replacementClaimed = job.id === repaired.replacement_job_id;
        await confirmPricingControlJob(database, job, {
          result: "applied",
          identity: {
            policy: job.spec,
            activation: {
              account_id: job.spec.account_id,
              effective_version: job.spec.effective_version,
              content_digest: job.spec.content_digest,
              binding: job.binding,
            },
            expectation: "unbound",
          },
        });
      }
    }
    expect(replacementClaimed).toBe(true);
    const completed = await seedClient.query<{ account_status: string; sync_state: string }>(`
      SELECT account.status AS account_status, binding.sync_state
      FROM engine_accounts account
      JOIN account_policy_bindings binding ON binding.user_id = account.user_id
      WHERE account.user_id = $1
    `, [user.id]);
    expect(completed.rows[0]).toEqual({ account_status: "active", sync_state: "confirmed" });

    const audit = await seedClient.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM audit_log
      WHERE action = 'pricing.policy_delivery.compatibility_repaired'
        AND target_id = $1
    `, [terminal.id]);
    expect(audit.rows).toHaveLength(1);
    expect(audit.rows[0]!.metadata).toMatchObject({
      supersededJobId: terminal.id,
      replacementJobId: repaired.replacement_job_id,
      bindingId: terminal.binding_id,
      engineAccountId,
      previousEffectiveVersion: 1,
      replacementEffectiveVersion: 2,
    });
  });
}, TEST_TIMEOUT_MS);

function inviteInput(idempotencyKey: string, policyRules: readonly PricingPolicyEditorRule[]) {
  const token = `token-${idempotencyKey}`;
  return {
    email: "buyer@example.test",
    tokenHash: tokenHash(token),
    encryptedToken: `encrypted-${token}`,
    multiplierBp: 10_000,
    expiresAt: new Date(Date.now() + 86_400_000),
    idempotencyKey,
    actorId: "admin@example.test",
    reason: "negotiated provider and model pricing policy",
    policyRules,
  };
}

function tokenHash(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}
