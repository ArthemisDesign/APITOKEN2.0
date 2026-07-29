import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { AdminOperationsService } from "./admin-operations.service.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("admin operations", () => {
  let database: Database;
  let engine: FakeAdminEngine;
  let service: AdminOperationsService;
  let passwordUserId: string;
  let oauthUserId: string;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox,
               auth_rate_limits, auth_tokens, auth_sessions, auth_identities, oauth_transactions,
               checkout_sessions, engine_pricing_jobs, pricing_usage_events, pricing_usage_cursors,
               pricing_months, business_invites, customer_profiles, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
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

    const topups = await service.topups(20) as { payments: Array<Record<string, unknown>> };
    expect(topups.payments).toHaveLength(1);
    expect(topups.payments[0]).toMatchObject({ email: "password@example.com", amount_usd: "25", credit_status: "confirmed" });
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

  it("converts B2C to B2B while preserving the exact effective discount", async () => {
    await database.pool.query(`
      UPDATE customer_profiles
      SET current_tier = 2, multiplier_bp = 2750, referral_floor_bps = 7250,
          tier_window_start = now(), tier_window_spent_nano = 123
      WHERE user_id = $1
    `, [passwordUserId]);

    const result = await service.convertToBusiness({
      userId: passwordUserId,
      reason: "customer requested business terms",
      actorId: "admin-q",
    });
    expect(result).toMatchObject({
      customer_type: "b2b",
      discount_percent: 72.5,
      multiplier_bp: 2750,
      converted: true,
      sync_status: "pending",
    });

    const profile = await database.pool.query(`
      SELECT customer_type, current_tier, multiplier_bp, referral_floor_bps,
             tier_window_start, tier_window_spent_nano::text
      FROM customer_profiles WHERE user_id = $1
    `, [passwordUserId]);
    expect(profile.rows[0]).toEqual({
      customer_type: "b2b",
      current_tier: null,
      multiplier_bp: 2750,
      referral_floor_bps: 0,
      tier_window_start: null,
      tier_window_spent_nano: "0",
    });
    const audit = await database.pool.query(`
      SELECT actor_id, metadata FROM audit_log WHERE action = 'pricing.b2b_converted'
    `);
    expect(audit.rows).toEqual([{
      actor_id: "admin-q",
      metadata: expect.objectContaining({
        reason: "customer requested business terms",
        preservedMultiplierBp: 2750,
        previousTier: 2,
        previousReferralFloorBps: 7250,
      }),
    }]);

    await expect(service.convertToBusiness({
      userId: passwordUserId,
      reason: "safe retry",
      actorId: "admin-q",
    })).resolves.toMatchObject({ converted: false, multiplier_bp: 2750, sync_status: "unchanged" });
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
  readonly statusChanges: Array<{ account: string; status: string }> = [];
  readonly client = new EngineClient({
    baseUrl: "http://engine.test",
    controlKey: "test-control",
    fetch: async (input, init) => {
      const url = new URL(String(input));
      const account = decodeURIComponent(url.pathname.split("/")[3] ?? "");
      const body = JSON.parse(String(init?.body ?? "{}")) as Record<string, string>;
      if (url.pathname.endsWith("/credit")) {
        this.credits.push({ account, amountNano: body.amount_nano!, ref: body.ref! });
        return Response.json({ account, balance_nano: "12000000000", balance: "$12.000000000" });
      }
      if (url.pathname.endsWith("/status")) {
        this.statusChanges.push({ account, status: body.status! });
        return Response.json({ account, status: body.status, updated: 1 });
      }
      return Response.json({ error: "unexpected request" }, { status: 500 });
    },
  });
}
