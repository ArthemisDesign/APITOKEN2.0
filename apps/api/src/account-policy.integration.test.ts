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

  it("keeps invited provisioning pending until exact ACK and permits a key only afterwards", async () => {
    const inviteToken = randomBytes(32).toString("base64url");
    await createBusinessInvite(database, {
      email: "invited-auth@example.test",
      tokenHash: createHash("sha256").update(inviteToken).digest("hex"),
      encryptedToken: "encrypted-auth-invitation",
      multiplierBp: 10_000,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "operator@example.test",
      reason: "create a complete policy before invited provisioning",
      policyRules: [
        {
          scope: { provider: { providerId: "anthropic" } },
          pricingMode: "discount",
          discountBps: 6_000,
        },
        {
          scope: { provider: { providerId: "openai" } },
          pricingMode: "discount",
          discountBps: 5_000,
        },
      ],
    });

    let createdAccountId = "";
    let issuedKeys = 0;
    const engine = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const path = new URL(String(input)).pathname;
        if (path === "/admin/account" && init?.method === "POST") {
          const body = JSON.parse(String(init.body)) as { handle: string; mult_bp: number };
          createdAccountId = `acct_invited_${body.handle.slice("user:".length).replaceAll("-", "")}`;
          return Response.json({ account: createdAccountId, mult_bp: body.mult_bp, handle: body.handle });
        }
        if (path === "/admin/key" && init?.method === "POST") {
          const body = JSON.parse(String(init.body)) as { account_id: string; label?: string };
          issuedKeys += 1;
          return Response.json({
            key: rawKey,
            key_id: "key_after_policy_ack",
            account: body.account_id,
            label: body.label ?? "default",
            spend_limit_nano: null,
            expires_ts: null,
          });
        }
        return Response.json({ error: "not found" }, { status: 404 });
      },
    });
    const auth = new AuthService(
      database,
      engine,
      new ConfigService<Environment, true>({
        SESSION_TTL_SECONDS: 604_800,
        AUTH_TOKEN_ENCRYPTION_KEY: Buffer.alloc(32, 9).toString("base64url"),
        EMAIL_VERIFICATION_REQUIRED: false,
        EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
        PASSWORD_RESET_TTL_SECONDS: 3_600,
      } as Environment),
    );

    const registration = await auth.register({
      email: "invited-auth@example.test",
      password: "correct horse battery staple",
      inviteToken,
      userAgent: "policy-test",
      ipAddress: "192.0.2.50",
    });
    expect(registration.user).toMatchObject({ customerType: "b2b", engineAccountStatus: "pending" });
    expect(createdAccountId).toMatch(/^acct_invited_/);
    const beforeAck = await database.pool.query<{ status: string; policy_enforcement: string; sync_state: string }>(`
      SELECT account.status, binding.policy_enforcement, binding.sync_state
      FROM engine_accounts account
      JOIN account_policy_bindings binding ON binding.user_id = account.user_id
      WHERE account.user_id = $1
    `, [registration.user.id]);
    expect(beforeAck.rows[0]).toEqual({
      status: "pending",
      policy_enforcement: "legacy_scalar",
      sync_state: "pending",
    });

    const account = new AccountService(database, engine);
    await expect(account.createApiKey(registration.user.id, { label: "too-early" }))
      .rejects.toBeInstanceOf(ConflictException);
    expect(issuedKeys).toBe(0);

    for (let index = 0; index < 8; index += 1) {
      const job = await claimNextPricingControlJob(database, `auth-policy-${index}`);
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

    const afterAck = await database.pool.query<{ status: string; policy_enforcement: string; sync_state: string }>(`
      SELECT account.status, binding.policy_enforcement, binding.sync_state
      FROM engine_accounts account
      JOIN account_policy_bindings binding ON binding.user_id = account.user_id
      WHERE account.user_id = $1
    `, [registration.user.id]);
    expect(afterAck.rows[0]).toEqual({ status: "active", policy_enforcement: "strict", sync_state: "confirmed" });
    await expect(account.createApiKey(registration.user.id, { label: "after-ack" }))
      .resolves.toMatchObject({ key: rawKey, status: "active" });
    expect(issuedKeys).toBe(1);
  });
}, TEST_TIMEOUT_MS);
