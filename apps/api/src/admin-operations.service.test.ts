import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { AdminService } from "./admin.service.js";
import { AdminOperationsService } from "./admin-operations.service.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("admin operations", () => {
  let database: Database;
  let engine: FakeAdminEngine;
  let service: AdminOperationsService;
  let adminService: AdminService;
  let passwordUserId: string;
  let oauthUserId: string;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    const tables = await database.pool.query<{ tablename: string }>(`
      SELECT tablename FROM pg_tables
      WHERE schemaname = 'public' AND tablename <> '__drizzle_migrations'
      ORDER BY tablename
    `);
    if (tables.rows.length > 0) {
      await database.pool.query(
        `TRUNCATE TABLE ${tables.rows.map((row) => `"${row.tablename}"`).join(", ")} RESTART IDENTITY CASCADE`,
      );
    }
    passwordUserId = randomUUID();
    oauthUserId = randomUUID();
    await database.pool.query(`
      INSERT INTO users (id, email, display_name, password_hash, email_verified, totp_secret, totp_enabled)
      VALUES ($1, 'password@example.com', 'Password User', 'hash', true, 'encrypted', true),
             ($2, 'oauth@example.com', 'OAuth User', NULL, true, NULL, false)
    `, [passwordUserId, oauthUserId]);
    await database.pool.query(`
      INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified, metadata)
      VALUES ($1, $2, 'google', 'google-subject', 'oauth@example.com', true, '{}')
    `, [randomUUID(), oauthUserId]);
    await database.pool.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('provider', 'google', 'auth.oauth_registered', 'user', $1, '{"provider":"google"}')
    `, [oauthUserId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
      VALUES ($1, $2, 'acct_password', 'active'), ($3, $4, 'acct_oauth', 'active')
    `, [randomUUID(), passwordUserId, randomUUID(), oauthUserId]);
    await database.pool.query(`
      INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2c', 0, 4000, date_trunc('month', now())),
             ($2, 'b2c', 0, 4000, date_trunc('month', now()))
    `, [passwordUserId, oauthUserId]);
    await database.pool.query(`
      INSERT INTO pricing_usage_events (
        id, user_id, engine_account_id, ledger_entry_id, provider_id,
        provider_recovery_version, amount_nano, real_funded_nano, occurred_at
      ) VALUES
        ($1, $4, 'acct_oauth', 1, 'anthropic', 1, 2000000000, 0, now()),
        ($2, $4, 'acct_oauth', 2, 'openai', 1, 750000000, 0, now()),
        ($3, $4, 'acct_oauth', 3, 'future-provider', 1, 250000000, 0, now())
    `, [randomUUID(), randomUUID(), randomUUID(), oauthUserId]);
    await database.pool.query(`
      INSERT INTO auth_sessions (id, user_id, token_hash, expires_at, last_seen_at)
      VALUES ($1, $2, $3, now() + interval '1 day', now())
    `, [randomUUID(), passwordUserId, "a".repeat(64)]);
    const checkoutId = randomUUID();
    const paymentId = randomUUID();
    await database.pool.query(`
      INSERT INTO checkout_sessions (
        id, user_id, engine_account_id, provider, amount_usd, amount_nano,
        provider_payment_id, status, completed_at
      ) VALUES ($1, $2, 'acct_password', 'cryptomus', 25, 25000000000, 'provider-1', 'paid', now())
    `, [checkoutId, passwordUserId]);
    await database.pool.query(`
      INSERT INTO payments (
        id, checkout_id, user_id, provider, provider_payment_id, amount_minor,
        currency, amount_nano, status, paid_at
      ) VALUES ($1, $2, $3, 'cryptomus', 'provider-1', 2500, 'USD', 25000000000, 'paid', now())
    `, [paymentId, checkoutId, passwordUserId]);
    await database.pool.query(`
      INSERT INTO engine_credits (id, payment_id, engine_account_id, amount_nano, idempotency_ref, status)
      VALUES ($1, $2, 'acct_password', 25000000000, 'cryptomus:provider-1', 'confirmed')
    `, [randomUUID(), paymentId]);
    engine = new FakeAdminEngine();
    service = new AdminOperationsService(database, engine.client);
    adminService = new AdminService(database, engine.client, {} as never);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("reports password and OAuth registrations plus top-up totals", async () => {
    const dashboard = await service.dashboard() as {
      users: Record<string, number>;
      topups: Record<string, number | string>;
    };
    expect(dashboard.users).toMatchObject({
      total: 2,
      registered_oauth: 1,
      registered_password: 1,
      password_only: 1,
      oauth_only: 1,
      google: 1,
    });
    expect(dashboard.topups).toMatchObject({ paid_count: 1, paid_users: 1, paid_usd: "25" });

    const topups = await service.topups({ limit: 20, offset: 0 }) as {
      payments: Array<Record<string, unknown>>;
      payments_total: number;
      checkouts_total: number;
    };
    expect(topups.payments).toHaveLength(1);
    expect(topups.payments[0]).toMatchObject({ email: "password@example.com", amount_usd: "25", credit_status: "confirmed" });
    expect(topups.payments_total).toBe(1);
    expect(topups.checkouts_total).toBe(0);
  });

  it("paginates users and resolves live balances with one bounded engine request", async () => {
    const first = await adminService.listUsers({ limit: 1, offset: 0 });
    expect(first.total).toBe(2);
    expect(first.users).toHaveLength(1);
    expect(engine.accountBatchRequests).toHaveLength(1);
    expect(engine.accountBatchRequests[0]).toHaveLength(1);

    const oauth = await adminService.listUsers({ limit: 10, auth: "google" });
    expect(oauth.total).toBe(1);
    expect(oauth.users).toMatchObject([{
      email: "oauth@example.com",
      balance_usd: "12.0000",
      engine_live_status: "active",
      spent_30d_usd: "3.0000",
      provider_spend_30d: {
        anthropic_nano: "2000000000",
        openai_nano: "750000000",
        google_nano: "0",
        kimi_nano: "0",
        other_nano: "250000000",
      },
    }]);
    expect(engine.accountBatchRequests).toHaveLength(2);
  });

  it("credits a user idempotently and records the operator reason once", async () => {
    const idempotencyKey = randomUUID();
    const input = {
      userId: passwordUserId,
      amountUsd: "7",
      reason: "customer support credit",
      idempotencyKey,
      actorId: "admin-q",
    };
    const first = await service.creditUser(input);
    const replay = await service.creditUser(input);

    expect(first).toMatchObject({ credited_usd: "7", balance_usd: "12", idempotent_replay: false });
    expect(replay).toMatchObject({ credited_usd: "7", balance_usd: "12", idempotent_replay: true });
    expect(engine.credits).toEqual([{ account: "acct_password", amountNano: "7000000000", ref: `admin-credit:${idempotencyKey}` }]);
    const audit = await database.pool.query(`
      SELECT actor_id, metadata FROM audit_log WHERE action = 'admin.credit'
    `);
    expect(audit.rows).toEqual([{
      actor_id: "admin-q",
      metadata: expect.objectContaining({ reason: "customer support credit", amount_nano: "7000000000" }),
    }]);
  });

  it.each([
    ["historical NULL amount", null, "4000000000", "4"],
    ["persisted new amount", "5000000000", "5000000000", "5"],
  ] as const)("revokes the exact %s welcome bonus and replays idempotently", async (
    _case,
    storedAmountNano,
    expectedAmountNano,
    expectedUsd,
  ) => {
    await database.pool.query(`
      INSERT INTO signup_profiles (
        user_id, email_canonical, bonus_granted, bonus_amount_nano
      ) VALUES ($1, 'oauth@example.com', true, $2)
    `, [oauthUserId, storedAmountNano]);
    const input = {
      userId: oauthUserId,
      reason: "confirmed bonus abuse",
      actorId: "admin-q",
    };

    await expect(service.revokeSignupBonus(input)).resolves.toMatchObject({
      revoked_usd: expectedUsd,
      idempotent_replay: false,
    });
    await expect(service.revokeSignupBonus(input)).resolves.toMatchObject({
      revoked_usd: expectedUsd,
      idempotent_replay: true,
    });
    expect(engine.debits).toEqual([{
      account: "acct_oauth",
      amountNano: expectedAmountNano,
      ref: `bonus-revoke:${oauthUserId}`,
    }]);
    const stored = await database.pool.query(`
      SELECT bonus_amount_nano::text, flagged_reason,
             (SELECT metadata->>'amount_nano' FROM audit_log
              WHERE action = 'admin.credit' AND metadata->>'ref' = $2) AS revoked_amount_nano
      FROM signup_profiles
      WHERE user_id = $1
    `, [oauthUserId, `bonus-revoke:${oauthUserId}`]);
    expect(stored.rows).toEqual([{
      bonus_amount_nano: storedAmountNano,
      flagged_reason: "admin-revoked",
      revoked_amount_nano: `-${expectedAmountNano}`,
    }]);
  });

  it("disables and re-enables both commerce and the authoritative engine account", async () => {
    await expect(service.setUserStatus({
      userId: passwordUserId,
      status: "disabled",
      reason: "suspected account compromise",
      actorId: "admin-q",
    })).resolves.toMatchObject({ status: "disabled", sessions_revoked: 1 });

    let state = await database.pool.query(`
      SELECT u.status AS user_status, ea.status AS engine_status,
             (SELECT count(*)::int FROM auth_sessions WHERE user_id = u.id AND revoked_at IS NULL) AS sessions
      FROM users u JOIN engine_accounts ea ON ea.user_id = u.id WHERE u.id = $1
    `, [passwordUserId]);
    expect(state.rows[0]).toEqual({ user_status: "disabled", engine_status: "disabled", sessions: 0 });

    await service.setUserStatus({
      userId: passwordUserId,
      status: "active",
      reason: "identity verified",
      actorId: "admin-q",
    });
    state = await database.pool.query(`
      SELECT u.status AS user_status, ea.status AS engine_status
      FROM users u JOIN engine_accounts ea ON ea.user_id = u.id WHERE u.id = $1
    `, [passwordUserId]);
    expect(state.rows[0]).toEqual({ user_status: "active", engine_status: "active" });
    expect(engine.statusChanges).toEqual([
      { account: "acct_password", status: "disabled" },
      { account: "acct_password", status: "active" },
    ]);
  });






  it("resets TOTP and revokes active sessions with an audit event", async () => {
    const result = await service.resetTotp({
      userId: passwordUserId,
      reason: "lost authenticator",
      actorId: "admin-q",
    });
    expect(result).toMatchObject({ totp_enabled: false, sessions_revoked: 1 });
    const state = await database.pool.query(`
      SELECT totp_enabled, totp_secret,
             (SELECT count(*)::int FROM auth_sessions WHERE user_id = users.id AND revoked_at IS NULL) AS sessions
      FROM users WHERE id = $1
    `, [passwordUserId]);
    expect(state.rows[0]).toEqual({ totp_enabled: false, totp_secret: null, sessions: 0 });
  });
});

class FakeAdminEngine {
  readonly credits: Array<{ account: string; amountNano: string; ref: string }> = [];
  readonly debits: Array<{ account: string; amountNano: string; ref: string }> = [];
  readonly statusChanges: Array<{ account: string; status: string }> = [];
  readonly accountBatchRequests: string[][] = [];
  readonly keyStamps: Array<{
    keyId: string;
    status: string;
    ack: { effective_policy_version: number; policy_digest: string } | null;
  }> = [];
  normalizationPlan: Record<string, unknown> | null = null;
  pricingStateBinding: { policy_enforcement: string; funding_enforcement: string; reconciliation_state: string } = {
    policy_enforcement: "shadow",
    funding_enforcement: "legacy_single",
    reconciliation_state: "verified",
  };
  readonly client = new EngineClient({
    baseUrl: "http://engine.test",
    controlKey: "test-control",
    fetch: async (input, init) => {
      const url = new URL(String(input));
      const account = decodeURIComponent(url.pathname.split("/")[3] ?? "");
      const body = JSON.parse(String(init?.body ?? "{}")) as Record<string, string>;
      if (url.pathname === "/admin/accounts/query") {
        const accountIds = (body.account_ids ?? []) as unknown as string[];
        this.accountBatchRequests.push(accountIds);
        return Response.json({
          accounts: accountIds.map((id) => ({
            account: id,
            balance_nano: "12000000000",
            spent_nano: "3000000000",
            reserved_nano: "0",
            balance: "$12.000000000",
            mult_bp: 4000,
            status: "active",
            handle: null,
          })),
        });
      }
      if (url.pathname.endsWith("/normalization")) {
        if (this.normalizationPlan !== null) {
          return Response.json({ normalization: this.normalizationPlan });
        }
        return Response.json({ error: "no normalization plan" }, { status: 404 });
      }
      if (url.pathname.includes("/pricing/policy/") && url.pathname.endsWith("/state")) {
        const stateAccount = decodeURIComponent(url.pathname.split("/")[4] ?? "");
        return Response.json({
          state: {
            account_id: stateAccount,
            policy: {
              active: {
                policy: {
                  account_id: stateAccount,
                  effective_version: 1,
                  policy_id: "policy:main:b2b:test",
                  policy_version: 1,
                  source_policy_digest: "source-digest-v1",
                  owner_type: "b2b_client",
                  owner_id: "user-1",
                  account_class: "b2b",
                  product_id: "main",
                  schema_version: 1,
                  catalog_generation: 1,
                  switch_generation: 1,
                  content_digest: "engine-digest-v1",
                  replacement_locked: false,
                  rules: [{
                    rule_id: "provider:anthropic:legacy-scalar",
                    rule_digest: "rule-digest-v1",
                    scope: { provider: { provider_id: "anthropic" } },
                    pricing_mode: "discount",
                    rule_origin: "legacy",
                    discount_bps: 2000,
                    payable_multiplier_bp: 8_000,
                    track_eligible: false,
                    retention_eligible: false,
                    commission_eligible: false,
                  }],
                },
                binding: this.pricingStateBinding,
              },
            },
          },
        });
      }
      if (url.pathname.endsWith("/keys")) {
        const keysAccount = decodeURIComponent(url.pathname.split("/")[3] ?? "");
        return Response.json({
          account: keysAccount,
          keys: [{
            key_id: "key_active", key_masked: "sk-pool-act…ive", label: "prod",
            status: "active", spent_nano: "0", spent: "$0.000000000",
          }, {
            key_id: "key_disabled", key_masked: "sk-pool-dis…ed", label: null,
            status: "disabled", spent_nano: "0", spent: "$0.000000000",
          }],
        });
      }
      if (url.pathname.includes("/key-id/")) {
        const keyId = decodeURIComponent(url.pathname.split("/")[3] ?? "");
        const ack = (body as Record<string, unknown>).activation_policy_ack as {
          effective_policy_version: number; policy_digest: string;
        } | undefined;
        this.keyStamps.push({
          keyId,
          status: body.status!,
          ack: ack === undefined ? null : {
            effective_policy_version: Number(ack.effective_policy_version),
            policy_digest: String(ack.policy_digest),
          },
        });
        return Response.json({ key_id: keyId, status: body.status, updated: 1 });
      }
      if (url.pathname.endsWith("/credit")) {
        const signedAmountNano = BigInt(body.amount_nano!);
        if (signedAmountNano < 0n) {
          this.debits.push({
            account,
            amountNano: (-signedAmountNano).toString(),
            ref: body.ref!,
          });
        } else {
          this.credits.push({ account, amountNano: signedAmountNano.toString(), ref: body.ref! });
        }
        return Response.json({ account, balance_nano: "12000000000", balance: "$12.000000000" });
      }
      if (url.pathname.endsWith("/status")) {
        this.statusChanges.push({ account, status: body.status! });
        return Response.json({ account, status: body.status, updated: 1 });
      }
      // Release-v2 provisioning probe (post-cutover conversion sync): no head in these tests,
      // so the sync reports pre_cutover and the legacy lane assertions stay focused.
      if (url.pathname === "/admin/pricing/v2/head") {
        return Response.json({ head: null });
      }
      return Response.json({ error: "unexpected request" }, { status: 500 });
    },
  });
}
