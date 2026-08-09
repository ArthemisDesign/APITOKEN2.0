import { createHash, randomBytes, randomUUID } from "node:crypto";
import { ConflictException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  copyBusinessInvitationPolicyToUser,
  claimNextPricingControlJob,
  confirmPricingControlJob,
  createBusinessInvite,
  createDatabase,
  materializeProvisionedUserPolicy,
  runMigrations,
  runStage5Backfill,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { AccountService } from "./account.service.js";
import { AuthService } from "./auth.service.js";
import type { Environment } from "./config.js";

const connectionString = process.env.TEST_DATABASE_URL;
const TEST_TIMEOUT_MS = 120_000;
const rawKey = `sk-pool-${"p".repeat(48)}`;

describe.runIf(Boolean(connectionString))("policy-before-key issuance race", () => {
  let adminDatabase: Database;
  let database: Database;
  let databaseName: string;

  beforeAll(async () => {
    databaseName = `account_policy_${process.pid}_${randomUUID().replaceAll("-", "").slice(0, 10)}`;
    adminDatabase = createDatabase(connectionString!, "account-policy-admin-test");
    await adminDatabase.pool.query(`CREATE DATABASE "${databaseName}"`);
    const url = new URL(connectionString!);
    url.pathname = `/${databaseName}`;
    await runMigrations({ DATABASE_URL: url.toString() });
    database = createDatabase(url.toString(), "account-policy-test");
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [],
      openkeys_accounts: [],
    }, { mode: "safe" });
  }, TEST_TIMEOUT_MS);

  afterAll(async () => {
    await database?.pool.end();
    if (adminDatabase) {
      await adminDatabase.pool.query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()",
        [databaseName],
      );
      await adminDatabase.pool.query(`DROP DATABASE IF EXISTS "${databaseName}"`);
      await adminDatabase.pool.end();
    }
  }, TEST_TIMEOUT_MS);

  it("disables a remotely issued key when policy authority appears between preflight and postflight", async () => {
    const userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
      userId,
      "policy-race@example.test",
      "Policy Race",
    ]);
    await database.pool.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2b', NULL, 10000, date_trunc('month', now())::date)
    `, [userId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 10000, 'active')
    `, [randomUUID(), userId, `acct_policy_race_${userId.replaceAll("-", "")}`]);

    const invitation = await createBusinessInvite(database, {
      email: "source@example.test",
      tokenHash: createHash("sha256").update(randomUUID()).digest("hex"),
      encryptedToken: "encrypted-policy-source",
      multiplierBp: 10_000,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "operator@example.test",
      reason: "prepare a reviewed B2B source policy",
      policyRules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
    });

    let issued = 0;
    const disabled: string[] = [];
    const engine = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const path = new URL(String(input)).pathname;
        if (path === "/admin/pricing/v2/head") return Response.json({ head: null });
        if (path.includes("/pricing/policy/") && path.endsWith("/state")) {
          return Response.json({
            state: { account_id: decodeURIComponent(path.split("/")[4] ?? ""), policy: "unbound" },
          });
        }
        if (path === "/admin/key" && init?.method === "POST") {
          issued += 1;
          const client = await database.pool.connect();
          try {
            await client.query("BEGIN");
            await copyBusinessInvitationPolicyToUser(client, { inviteId: invitation.id, userId });
            await client.query("COMMIT");
          } catch (error) {
            await client.query("ROLLBACK");
            throw error;
          } finally {
            client.release();
          }
          return Response.json({
            key: rawKey,
            key_id: "key_policy_race",
            account: `acct_policy_race_${userId.replaceAll("-", "")}`,
            label: "race",
            spend_limit_nano: null,
            expires_ts: null,
          });
        }
        if (path === "/admin/key-id/key_policy_race/status" && init?.method === "POST") {
          disabled.push("key_policy_race");
          return Response.json({ key_id: "key_policy_race", status: "disabled", updated: 1 });
        }
        return Response.json({ error: "not found" }, { status: 404 });
      },
    });
    const service = new AccountService(database, engine);

    await expect(service.createApiKey(userId, { label: "race" })).rejects.toBeInstanceOf(ConflictException);
    expect(issued).toBe(1);
    expect(disabled).toEqual(["key_policy_race"]);
    const stored = await database.pool.query<{ count: number }>(
      "SELECT count(*)::int AS count FROM api_keys WHERE user_id = $1",
      [userId],
    );
    expect(stored.rows[0]).toEqual({ count: 0 });

    // Once authority exists, the preflight itself blocks another remote issuance until exact ACK.
    await expect(service.createApiKey(userId, { label: "blocked" })).rejects.toBeInstanceOf(ConflictException);
    expect(issued).toBe(1);
  });

  it("does not issue a key while the direct strict chain is still settling", async () => {
    const userId = randomUUID();
    const accountId = `acct_chain_pending_${userId.replaceAll("-", "")}`;
    await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1,$2,'Chain pending')", [
      userId,
      `chain-pending-${userId}@example.test`,
    ]);
    await database.pool.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1,'b2c',0,5000,date_trunc('month',now())::date)
    `, [userId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id,user_id,engine_account_id,mult_bp,status)
      VALUES ($1,$2,$3,5000,'active')
    `, [randomUUID(), userId, accountId]);
    // The real registration path: the managed global policy is materialized and provisioning
    // arms the direct strict chain. The shadow delivery is then confirmed, but the strict
    // staging has not landed — key issuance must wait instead of handing out an unusable
    // secret (an uncovered account is served only after the opt-out marker). The global policy
    // is seeded directly: running the Stage 5 backfill again would conflict with the
    // invitations other tests of this file already created.
    const catalogHead = await database.pool.query<{ active_generation: string }>(
      "SELECT active_generation::text FROM product_catalog_heads WHERE product_id = 'main'",
    );
    const catalogGeneration = Number(catalogHead.rows[0]!.active_generation);
    await database.pool.query(`
      INSERT INTO pricing_policies (id, owner_type, owner_id, product_id, replacement_locked, status)
      VALUES ('policy:main:global-b2c', 'global_b2c', 'global-b2c', 'main', false, 'active')
    `);
    await database.pool.query(`
      INSERT INTO pricing_policy_versions (
        policy_id, version, schema_version, product_id, catalog_generation,
        content_digest, actor_type, actor_id, reason
      ) VALUES ('policy:main:global-b2c', 1, 1, 'main', $1, 'chain-pending-source',
                'admin', 'account-policy-test', 'strict chain fixture')
    `, [catalogGeneration]);
    await database.pool.query(`
      INSERT INTO pricing_policy_heads (policy_id, current_version, current_digest)
      VALUES ('policy:main:global-b2c', 1, 'chain-pending-source')
    `);
    const materialized = await materializeProvisionedUserPolicy(database, {
      userId,
      engineAccountId: accountId,
    });
    expect(materialized.policyRequired).toBe(true);
    await database.pool.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', reconciliation_state = 'verified',
          last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [userId]);
    // What confirmPricingControlJob writes on the account once the binding is fully confirmed.
    await database.pool.query(
      "UPDATE engine_accounts SET status = 'active', last_error = NULL, updated_at = now() WHERE user_id = $1",
      [userId],
    );
    let issued = 0;
    const engine = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const path = new URL(String(input)).pathname;
        if (path.includes("/pricing/policy/") && path.endsWith("/state")) {
          return Response.json({
            state: { account_id: decodeURIComponent(path.split("/")[4] ?? ""), policy: "unbound" },
          });
        }
        if (path === "/admin/key" && init?.method === "POST") issued += 1;
        return Response.json({ error: "not found" }, { status: 404 });
      },
    });

    await expect(new AccountService(database, engine).createApiKey(userId, { label: "blocked" }))
      .rejects.toBeInstanceOf(ConflictException);
    expect(issued).toBe(0);
    const binding = await database.pool.query<{ strict_chain_pending: boolean }>(
      "SELECT strict_chain_pending FROM account_policy_bindings WHERE user_id = $1",
      [userId],
    );
    expect(binding.rows[0]?.strict_chain_pending).toBe(true);
  });

  it("materializes a managed policy while recovering an account before key preflight", async () => {
    const userId = randomUUID();
    await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
      userId,
      "policy-recovery@example.test",
      "Policy Recovery",
    ]);
    await database.pool.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2b', NULL, 10000, date_trunc('month', now())::date)
    `, [userId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, NULL, 10000, 'error')
    `, [randomUUID(), userId]);
    const invitation = await createBusinessInvite(database, {
      email: "policy-recovery-source@example.test",
      tokenHash: createHash("sha256").update(randomUUID()).digest("hex"),
      encryptedToken: "encrypted-policy-recovery-source",
      multiplierBp: 10_000,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "operator@example.test",
      reason: "prepare the policy recovery regression",
      policyRules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 6_000,
      }],
    });
    const client = await database.pool.connect();
    try {
      await client.query("BEGIN");
      await copyBusinessInvitationPolicyToUser(client, { inviteId: invitation.id, userId });
      await client.query("COMMIT");
    } catch (error) {
      await client.query("ROLLBACK");
      throw error;
    } finally {
      client.release();
    }

    let createdAccounts = 0;
    let issuedKeys = 0;
    const engineAccountId = `acct_policy_recovery_${userId.replaceAll("-", "")}`;
    const engine = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const path = new URL(String(input)).pathname;
        if (path === "/admin/pricing/v2/head") return Response.json({ head: null });
        if (path.includes("/pricing/policy/") && path.endsWith("/state")) {
          return Response.json({
            state: { account_id: decodeURIComponent(path.split("/")[4] ?? ""), policy: "unbound" },
          });
        }
        if (path === "/admin/account" && init?.method === "POST") {
          createdAccounts += 1;
          return Response.json({ account: engineAccountId, mult_bp: 10_000, handle: `user:${userId}` });
        }
        if (path === "/admin/key" && init?.method === "POST") {
          issuedKeys += 1;
          return Response.json({
            key: rawKey,
            key_id: "key_policy_recovery",
            account: engineAccountId,
            label: "recovery",
            spend_limit_nano: null,
            expires_ts: null,
          });
        }
        return Response.json({ error: "not found" }, { status: 404 });
      },
    });

    const account = new AccountService(database, engine);
    await expect(account.createApiKey(userId, { label: "recovery" }))
      .rejects.toBeInstanceOf(ConflictException);
    expect(createdAccounts).toBe(1);
    expect(issuedKeys).toBe(0);
    const staged = await database.pool.query<{
      account_status: string;
      engine_account_id: string;
      sync_state: string;
      desired_effective_version: string;
    }>(`
      SELECT account.status AS account_status, account.engine_account_id,
             binding.sync_state, binding.desired_effective_version::text
      FROM engine_accounts account
      JOIN account_policy_bindings binding ON binding.user_id = account.user_id
      WHERE account.user_id = $1
    `, [userId]);
    expect(staged.rows[0]).toEqual({
      account_status: "pending",
      engine_account_id: engineAccountId,
      sync_state: "pending",
      desired_effective_version: "1",
    });
  });

}, TEST_TIMEOUT_MS);
