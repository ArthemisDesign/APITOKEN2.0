import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, getManagedPricingPolicy, runStage5Backfill, type Database } from "@claude-api/db";
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
    // The B2B conversion path provisions a managed pricing policy, so the versioned pricing
    // foundation (catalog, switches, global policy) must exist just like in production.
    await runStage5Backfill(database, {
      schema_version: 1,
      engine_accounts: [],
      openkeys_accounts: [],
    }, { mode: "safe" });
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

  it("converts B2C to B2B with the negotiated discount in one transaction", async () => {
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
      discountPercent: 80,
    });
    expect(result).toMatchObject({
      customer_type: "b2b",
      discount_percent: 80,
      multiplier_bp: 2000,
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
      multiplier_bp: 2000,
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
        previousMultiplierBp: 2750,
        negotiatedMultiplierBp: 2000,
        previousTier: 2,
        previousReferralFloorBps: 7250,
        managedPolicyId: `policy:main:b2b:${passwordUserId}`,
        managedPolicyVersion: 1,
      }),
    }]);

    // The conversion provisions the managed b2b_client policy so the admin policy editor can
    // manage the customer exactly like an invite-redeemed one.
    const policy = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: passwordUserId,
    }) as { currentVersion: number; rules: unknown[]; targets: unknown[] } | null;
    expect(policy).toMatchObject({
      currentVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 8000,
      }],
    });
    expect(policy!.targets).toHaveLength(1);

    await expect(service.convertToBusiness({
      userId: passwordUserId,
      reason: "safe retry",
      actorId: "admin-q",
      discountPercent: 70,
    })).resolves.toMatchObject({ converted: false, multiplier_bp: 2000, sync_status: "unchanged" });
  });

  it("repairs the missing managed policy of a pre-provisioning B2B conversion", async () => {
    // Simulates a customer converted before managed-policy provisioning existed: B2B profile
    // with the negotiated scalar, but no b2b_client policy and no account binding.
    await database.pool.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL, multiplier_bp = 2000
      WHERE user_id = $1
    `, [passwordUserId]);

    const result = await service.convertToBusiness({
      userId: passwordUserId,
      reason: "repair missing managed policy",
      actorId: "admin-q",
      discountPercent: 50,
    });
    // The scalar already in effect is kept (the passed discount is ignored on the repair path),
    // while the policy provisioning job is staged for engine delivery.
    expect(result).toMatchObject({ converted: false, multiplier_bp: 2000, sync_status: "pending" });
    const policy = await getManagedPricingPolicy(database, {
      ownerType: "b2b_client",
      ownerId: passwordUserId,
    }) as { currentVersion: number; rules: unknown[] } | null;
    expect(policy).toMatchObject({
      currentVersion: 1,
      rules: [{
        scope: { provider: { providerId: "anthropic" } },
        pricingMode: "discount",
        discountBps: 8000,
      }],
    });
    const profile = await database.pool.query(`
      SELECT multiplier_bp FROM customer_profiles WHERE user_id = $1
    `, [passwordUserId]);
    expect(profile.rows[0]).toEqual({ multiplier_bp: 2000 });
    const repairAudit = await database.pool.query(`
      SELECT actor_id, metadata FROM audit_log WHERE action = 'pricing.b2b_policy_provisioned'
    `);
    expect(repairAudit.rows).toEqual([{
      actor_id: "admin-q",
      metadata: expect.objectContaining({ multiplierBp: 2000, policyVersion: 1 }),
    }]);

    // Once repaired, the same action is an unchanged no-op again.
    await expect(service.convertToBusiness({
      userId: passwordUserId,
      reason: "safe retry after repair",
      actorId: "admin-q",
      discountPercent: 50,
    })).resolves.toMatchObject({ converted: false, sync_status: "unchanged" });
  });

  it("cuts a converted client over to strict policy enforcement and reports replays", async () => {
    await service.convertToBusiness({
      userId: passwordUserId,
      reason: "customer negotiated business terms",
      actorId: "admin-q",
      discountPercent: 60,
    });
    // The conversion armed the automatic strict chain on the binding.
    const armed = await database.pool.query<{ strict_chain_pending: boolean }>(`
      SELECT strict_chain_pending FROM account_policy_bindings WHERE user_id = $1
    `, [passwordUserId]);
    expect(armed.rows).toEqual([{ strict_chain_pending: true }]);
    // The engine confirmed the first policy delivery: the binding is shadow with applied v1
    // and the durable delivery job is terminal-confirmed.
    await database.pool.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [passwordUserId]);
    await database.pool.query(`
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

    const result = await service.cutoverUserPolicyToStrict({
      userId: passwordUserId,
      reason: "enforce the negotiated per-provider rates",
      actorId: "admin-q",
    }) as { job_id?: unknown };
    expect(result).toMatchObject({
      user_id: passwordUserId,
      account_id: "acct_password",
      cutover: "staged",
      job_status: "pending",
      effective_version: 1,
      funding: "nothing_to_normalize",
    });
    expect(result.job_id).toEqual(expect.any(String));

    // Every active key is stamped with the exact active-policy ACK the engine reported;
    // the disabled key is left untouched.
    expect(engine.keyStamps).toEqual([{
      keyId: "key_active",
      status: "active",
      ack: { effective_policy_version: 1, policy_digest: "engine-digest-v1" },
    }]);

    const binding = await database.pool.query(`
      SELECT policy_enforcement, funding_enforcement, reconciliation_state, sync_state,
             desired_effective_version::text, applied_effective_version::text,
             strict_chain_pending
      FROM account_policy_bindings WHERE user_id = $1
    `, [passwordUserId]);
    expect(binding.rows).toEqual([{
      policy_enforcement: "strict",
      funding_enforcement: "strict",
      reconciliation_state: "verified",
      sync_state: "confirmed",
      desired_effective_version: "1",
      applied_effective_version: "1",
      strict_chain_pending: false,
    }]);
    const job = await database.pool.query<{ status: string; binding: unknown }>(`
      SELECT status, payload->'binding' AS binding FROM engine_policy_jobs WHERE id = $1
    `, [result.job_id]);
    expect(job.rows[0]).toMatchObject({
      status: "pending",
      binding: {
        policy_enforcement: "strict",
        funding_enforcement: "strict",
        reconciliation_state: "verified",
      },
    });

    // A replay stamps keys again (idempotent) and reports the already-strict state with the
    // original job instead of staging a duplicate.
    const replay = await service.cutoverUserPolicyToStrict({
      userId: passwordUserId,
      reason: "operator replay",
      actorId: "admin-q",
    });
    expect(replay).toMatchObject({
      cutover: "already_strict",
      job_id: result.job_id,
      job_status: "pending",
    });
  });

  it("rejects the strict cutover when the user has no account policy binding", async () => {
    await expect(service.cutoverUserPolicyToStrict({
      userId: oauthUserId,
      reason: "no policy exists",
      actorId: "admin-q",
    })).rejects.toMatchObject({ status: 404 });
  });

  it("rejects the strict cutover loudly when funding normalization is blocked", async () => {
    await service.convertToBusiness({
      userId: passwordUserId,
      reason: "customer negotiated business terms",
      actorId: "admin-q",
      discountPercent: 60,
    });
    await database.pool.query(`
      UPDATE account_policy_bindings
      SET applied_effective_version = desired_effective_version,
          applied_digest = desired_digest,
          policy_enforcement = 'shadow', last_ack_at = now(), sync_state = 'confirmed'
      WHERE user_id = $1
    `, [passwordUserId]);
    engine.normalizationPlan = {
      account_id: "acct_password",
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
    await expect(service.cutoverUserPolicyToStrict({
      userId: passwordUserId,
      reason: "enforce the negotiated per-provider rates",
      actorId: "admin-q",
    })).rejects.toMatchObject({
      status: 409,
      message: expect.stringContaining("active_legacy_reservation: reservation r1 is open"),
    });
    // No partial state: the binding stays shadow and the automatic chain stays armed, so the
    // worker sweep retries the cutover once the blocker is resolved.
    const binding = await database.pool.query<{
      policy_enforcement: string;
      strict_chain_pending: boolean;
    }>(`
      SELECT policy_enforcement, strict_chain_pending FROM account_policy_bindings
      WHERE user_id = $1
    `, [passwordUserId]);
    expect(binding.rows).toEqual([{ policy_enforcement: "shadow", strict_chain_pending: true }]);
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
      return Response.json({ error: "unexpected request" }, { status: 500 });
    },
  });
}
