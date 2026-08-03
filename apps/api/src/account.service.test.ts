import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import { AccountService } from "./account.service.js";

const connectionString = process.env.TEST_DATABASE_URL;
const rawKey = `sk-pool-${"a".repeat(48)}`;

describe.runIf(Boolean(connectionString))("commercial account and engine integration", () => {
  let database: Database;
  let engine: FakeEngine;
  let service: AccountService;
  let aliceId: string;
  let bobId: string;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    aliceId = await createUser(database, "alice@example.com", "acct_alice", "github");
    bobId = await createUser(database, "bob@example.com", "acct_bob");
    engine = new FakeEngine();
    service = new AccountService(database, engine.client);
  });

  afterAll(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities,
               checkout_sessions, engine_accounts, users RESTART IDENTITY CASCADE
    `);
    await database.pool.end();
  });

  it("serves authoritative balance and ledger without exposing the engine account ID", async () => {
    const account = await service.getAccount(aliceId) as Record<string, unknown>;
    expect(account).toMatchObject({
      balanceNano: "37000000000",
      reservedNano: "0",
      spentNano: "12",
      funding: {
        accountClass: "b2c",
        fundingEnforcement: "strict",
        balances: {
          paidNano: "33000000000",
          bonusNano: "4000000000",
          otherNano: "0",
          unattributedNano: "0",
        },
      },
      pricingPolicies: [],
    });
    expect(JSON.stringify(account)).not.toContain("acct_alice");

    const ledger = await service.getLedger(aliceId, 25) as Record<string, unknown>;
    expect(ledger).toMatchObject({
      entries: [
        { amountNano: "37000000000", reference: "payment:1", attribution: null },
        {
          requestId: "request-strict",
          provider: "anthropic",
          officialNano: "600",
          attribution: {
            snapshotKind: "policy_v1",
            canonicalModelId: "claude-read",
            pricingMode: "track",
            officialCost: { schema_version: 1, official_nano: 600 },
            bonusFundedNano: "300",
            trackEligible: true,
          },
          fundingAllocations: [{
            bucketId: "read-bonus",
            sourceType: "welcome_track_bonus",
            amountNano: "300",
          }],
        },
      ],
    });
    expect(JSON.stringify(ledger)).not.toContain("acct_alice");

    const usage = await service.getUsage(aliceId, "30d") as Record<string, unknown>;
    expect(usage).toMatchObject({
      sinceTs: 1_700_000_000,
      untilTs: 1_702_592_000,
      totalOfficialNano: "25000000",
      totalChargedNano: "10000000",
      buckets: { unattributedLegacy: { officialNano: "10000000" } },
      models: [{ provider: "anthropic" }],
      daily: [{ dayTs: 1_701_993_600, officialNano: "25000000", chargedNano: "10000000" }],
      dailyProviders: [{
        dayTs: 1_701_993_600,
        provider: "anthropic",
        officialNano: "25000000",
        chargedNano: "10000000",
      }],
      keys: [{
        keyMasked: "sk-pool-aaaa…aaaa",
        officialNano: "25000000",
        chargedNano: "10000000",
      }],
    });
    expect(JSON.stringify(usage)).not.toContain("acct_alice");
  });

  it("returns a raw key once, stores no usable secret, and enforces ownership on revocation", async () => {
    const created = await service.createApiKey(aliceId, {
      label: "production", spendLimitUsd: "25.50", expiresAt: "2099-01-01T00:00:00.000Z",
    }) as Record<string, unknown>;
    expect(created.key).toBe(rawKey);
    expect(created).toMatchObject({
      label: "production", status: "active", spentNano: "0",
      spendLimitNano: "25500000000", expiresAt: "2099-01-01T00:00:00.000Z",
    });
    expect(engine.releaseHeadReads).toBe(2);

    const persisted = await database.pool.query("SELECT * FROM api_keys WHERE user_id = $1", [aliceId]);
    expect(JSON.stringify(persisted.rows)).not.toContain(rawKey);
    const apiKeyId = String(created.id);

    const listed = await service.listApiKeys(aliceId) as { keys: Array<Record<string, unknown>> };
    expect(listed.keys).toHaveLength(1);
    expect(listed.keys[0]).not.toHaveProperty("key");
    expect(listed.keys[0]).toMatchObject({ id: apiKeyId, keyMasked: `sk-pool-aaaa…aaaa` });

    const policy = await service.updateApiKeyPolicy(aliceId, apiKeyId, {
      spendLimitUsd: null,
      expiresAt: "2099-02-01T00:00:00.000Z",
    }) as Record<string, unknown>;
    expect(policy).toMatchObject({ spendLimitNano: null, expiresAt: "2099-02-01T00:00:00.000Z" });
    expect(engine.policyUpdates).toEqual([{
      account: "acct_alice", keyId: "key_issued", spendLimitNano: null, expiresTs: 4_073_587_200,
    }]);
    await expect(service.updateApiKeyPolicy(bobId, apiKeyId, {
      spendLimitUsd: null, expiresAt: null,
    })).rejects.toThrow("API key not found");
    expect(engine.policyUpdates).toHaveLength(1);
    engine.rejectNextPolicyUpdate = true;
    await expect(service.updateApiKeyPolicy(aliceId, apiKeyId, {
      spendLimitUsd: "1", expiresAt: null,
    })).rejects.toThrow("spend limit cannot be below billed and reserved usage");

    await expect(service.disableApiKey(bobId, apiKeyId)).resolves.toBe(false);
    expect(engine.disabledKeyIds).toEqual([]);
    await expect(service.disableApiKey(aliceId, apiKeyId)).resolves.toBe(true);
    expect(engine.disabledKeyIds).toEqual(["key_issued"]);
    const status = await database.pool.query("SELECT status FROM api_keys WHERE id = $1", [apiKeyId]);
    expect(status.rows[0]?.status).toBe("disabled");
  });

  it("recovers failed provisioning through the engine's idempotent user handle", async () => {
    await database.pool.query(`
      UPDATE engine_accounts SET engine_account_id = NULL, status = 'error' WHERE user_id = $1
    `, [aliceId]);
    await database.pool.query(`
      UPDATE signup_profiles SET bonus_amount_nano = 5000000000 WHERE user_id = $1
    `, [aliceId]);
    engine.recoveredAccountId = "acct_recovered";
    await expect(service.ensureEngineAccount(aliceId)).resolves.toBe("acct_recovered");
    const mapping = await database.pool.query(`
      SELECT engine_account_id, status FROM engine_accounts WHERE user_id = $1
    `, [aliceId]);
    expect(mapping.rows[0]).toEqual({ engine_account_id: "acct_recovered", status: "active" });
    expect(engine.signupCredits).toEqual([{
      account: "acct_recovered", amountNano: "5000000000", reference: `signup-bonus:${aliceId}`,
    }]);
  });

  it("reprovisions when a previously mapped engine account no longer exists", async () => {
    engine.missingAccountIds.add("acct_alice");
    engine.recoveredAccountId = "acct_recovered";
    const account = await service.getAccount(aliceId) as Record<string, unknown>;
    expect(account.balanceNano).toBe("37000000000");
    const mapping = await database.pool.query(`
      SELECT engine_account_id, status FROM engine_accounts WHERE user_id = $1
    `, [aliceId]);
    expect(mapping.rows[0]).toEqual({ engine_account_id: "acct_recovered", status: "active" });
    expect(engine.signupCredits).toEqual([{
      account: "acct_recovered", amountNano: "4000000000", reference: `signup-bonus:${aliceId}`,
    }]);
  });

  it("does not grant the welcome bonus while recovering a password account", async () => {
    await database.pool.query(`
      UPDATE engine_accounts SET engine_account_id = NULL, status = 'error' WHERE user_id = $1
    `, [bobId]);
    engine.recoveredAccountId = "acct_password_recovered";

    await expect(service.ensureEngineAccount(bobId)).resolves.toBe("acct_password_recovered");
    expect(engine.signupCredits).toEqual([]);
  });
});

async function createUser(
  database: Database,
  email: string,
  engineAccountId: string,
  oauthProvider?: "google" | "github",
): Promise<string> {
  const userId = randomUUID();
  await database.pool.query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)", [
    userId, email, "Account Test",
  ]);
  await database.pool.query(`
    INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
    VALUES ($1, $2, $3, 'active')
  `, [randomUUID(), userId, engineAccountId]);
  await database.pool.query(`
    INSERT INTO customer_profiles (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
    VALUES ($1, 'b2c', 0, 4000, date_trunc('month', now())::date)
  `, [userId]);
  if (oauthProvider) {
    await database.pool.query(`
      INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified)
      VALUES ($1, $2, $3, $4, $5, true)
    `, [randomUUID(), userId, oauthProvider, `${oauthProvider}:${userId}`, email]);
    // Historical pre-0034 grant: NULL retains the immutable $4 nominal during recovery.
    await database.pool.query(`
      INSERT INTO signup_profiles (user_id, email_canonical, bonus_granted)
      VALUES ($1, $2, true)
    `, [userId, email]);
  }
  return userId;
}

class FakeEngine {
  readonly disabledKeyIds: string[] = [];
  readonly policyUpdates: Array<{
    account: string; keyId: string; spendLimitNano: string | null; expiresTs: number | null;
  }> = [];
  rejectNextPolicyUpdate = false;
  readonly signupCredits: Array<{ account: string; amountNano: string; reference: string }> = [];
  readonly missingAccountIds = new Set<string>();
  releaseHeadReads = 0;
  recoveredAccountId = "acct_alice";
  private issued = false;
  private spendLimitNano: string | null = "25500000000";
  private expiresTs: number | null = 4_070_908_800;
  readonly client = new EngineClient({
    baseUrl: "http://engine.test",
    controlKey: "test-control-key",
    fetch: async (input, init) => this.fetch(String(input), init),
  });

  private async fetch(url: string, init?: RequestInit): Promise<Response> {
    const path = new URL(url).pathname;
    if (path === "/admin/pricing/v2/head") {
      this.releaseHeadReads += 1;
      return Response.json({ head: null });
    }
    if (path === "/admin/account" && init?.method === "POST") {
      return Response.json({ account: this.recoveredAccountId, mult_bp: 2000, handle: "user:test" });
    }
    if (path.endsWith("/credit") && init?.method === "POST") {
      const account = path.slice("/admin/account/".length, -"/credit".length);
      const body = JSON.parse(String(init.body)) as { amount_nano: unknown; ref: string };
      if (typeof body.amount_nano !== "string") {
        return new Response("Failed to deserialize the JSON body", { status: 422 });
      }
      this.signupCredits.push({ account, amountNano: body.amount_nano, reference: body.ref });
      return Response.json({ account, balance_nano: body.amount_nano, balance: "$4.000000000" });
    }
    if (path === "/admin/key" && init?.method === "POST") {
      this.issued = true;
      const body = JSON.parse(String(init.body)) as { spend_limit_nano?: string; expires_ts?: number };
      return Response.json({
        key: rawKey, key_id: "key_issued", account: "acct_alice", label: "production",
        spend_limit_nano: body.spend_limit_nano ?? null, expires_ts: body.expires_ts ?? null,
      });
    }
    const policyMatch = path.match(/^\/admin\/account\/([^/]+)\/key-id\/([^/]+)\/policy$/);
    if (policyMatch && init?.method === "POST") {
      if (this.rejectNextPolicyUpdate) {
        this.rejectNextPolicyUpdate = false;
        return Response.json({
          error: "spend limit is below settled and reserved usage", code: "limit_below_committed",
        }, { status: 409 });
      }
      const body = JSON.parse(String(init.body)) as {
        spend_limit_nano: string | null; expires_ts: number | null;
      };
      this.spendLimitNano = body.spend_limit_nano;
      this.expiresTs = body.expires_ts;
      this.policyUpdates.push({
        account: policyMatch[1]!, keyId: policyMatch[2]!,
        spendLimitNano: body.spend_limit_nano, expiresTs: body.expires_ts,
      });
      return Response.json({
        key_id: policyMatch[2], spend_limit_nano: body.spend_limit_nano,
        expires_ts: body.expires_ts, updated: 1,
      });
    }
    if (path === "/admin/key-id/key_issued/status") {
      this.disabledKeyIds.push("key_issued");
      return Response.json({ key_id: "key_issued", status: "disabled", updated: 1 });
    }
    if (path.endsWith("/keys")) {
      return Response.json({
        account: "acct_alice",
        keys: this.issued ? [{
          key_id: "key_issued", key_masked: "sk-pool-aaaa…aaaa", label: "production",
          status: "active", spent_nano: 0, spent: "$0.000000000",
          reserved_nano: 0, spend_limit_nano: this.spendLimitNano, expires_ts: this.expiresTs,
          created_ts: 1_700_000_000, last_used_ts: null,
        }] : [],
      });
    }
    if (path.endsWith("/ledger")) {
      return Response.json({
        account: "acct_alice",
        entries: [{
          id: 1, kind: "topup", amount_nano: 37_000_000_000, amount: "$37.000000000",
          key_masked: null, ref: "payment:1", balance_after_nano: 37_000_000_000, ts: 1_700_000_000,
        }, {
          id: 2,
          kind: "charge",
          request_id: "request-strict",
          amount_nano: 300,
          amount: "$0.000000300",
          key_masked: "sk-pool-read…only",
          ref: "provider:read",
          balance_after_nano: 36_999_999_700,
          ts: 1_700_000_001,
          model: "claude-read",
          provider: "anthropic",
          official_nano: 600,
          attribution: {
            attribution_schema_version: 1,
            snapshot_kind: "policy_v1",
            provider_id: "anthropic",
            product_id: "main",
            account_class: "b2c",
            requested_model_id: "claude-read",
            canonical_model_id: "claude-read",
            served_model_id: "claude-read",
            served_canonical_model_id: "claude-read",
            billing_invariant_code: null,
            alias_generation: 1,
            rule_id: "read-rule",
            rule_digest: "read-rule-digest",
            rule_scope: "provider",
            pricing_mode: "track",
            rule_origin: "managed",
            discount_bps: null,
            payable_multiplier_bp: 5000,
            policy_id: "read-policy",
            policy_version: 1,
            effective_policy_version: 1,
            policy_digest: "read-policy-digest",
            source_policy_digest: "read-source-policy",
            catalog_generation: 1,
            switch_generation: 1,
            admission_catalog_generation: 1,
            admission_catalog_digest: "read-catalog",
            admission_switch_generation: 1,
            admission_switch_digest: "read-switch",
            runtime_manifest_generation: 1,
            runtime_manifest_digest: "read-runtime",
            tariff_schedule_id: "read-tariff",
            tariff_priced_ts: 1_700_000_001,
            official_nano: 600,
            official_cost_json: { schema_version: 1, official_nano: 600 },
            paid_funded_nano: 0,
            bonus_funded_nano: 300,
            other_funded_nano: 0,
            funding_allocation_json: [{
              bucket_id: "read-bonus",
              source_type: "welcome_track_bonus",
              bucket_version: 1,
              reserved_nano: 300,
              charged_nano: 300,
              released_nano: 0,
              allocation_order: 1,
            }],
            track_eligible: true,
            retention_eligible: true,
            commission_eligible: false,
            snapshot_digest: "read-snapshot",
          },
          funding_allocations: [{
            bucket_id: "read-bonus",
            source_type: "welcome_track_bonus",
            source_ref: "welcome",
            bucket_version: 1,
            direction: "debit",
            amount_nano: 300,
            allocation_order: 1,
          }],
        }],
      });
    }
    if (path.endsWith("/usage")) {
      return Response.json({
        account: "acct_alice",
        window: "30d",
        since_ts: 1_700_000_000,
        until_ts: 1_702_592_000,
        requests: 2,
        total_official_nano: 25_000_000,
        total_charged_nano: 10_000_000,
        buckets: {
          input: { tokens: 10, official_nano: 5_000_000 },
          output: { tokens: 10, official_nano: 10_000_000 },
          cache_read: { tokens: 10, official_nano: 0 },
          cache_write: { tokens: 0, official_nano: 0 },
          web_search: { requests: 1, official_nano: 0 },
          unattributed_legacy: { official_nano: 10_000_000 },
        },
        models: [{
          model: "claude-opus-4-8", provider: "anthropic", requests: 2, input_tokens: 10, output_tokens: 10,
          cache_read_tokens: 10, cache_write_5m_tokens: 0, cache_write_1h_tokens: 0,
          web_search_requests: 1, official_nano: 25_000_000, charged_nano: 10_000_000,
        }],
        daily: [{
          day_ts: 1_701_993_600, requests: 2,
          official_nano: 25_000_000, charged_nano: 10_000_000,
        }],
        daily_providers: [{
          day_ts: 1_701_993_600, provider: "anthropic", requests: 2,
          official_nano: 25_000_000, charged_nano: 10_000_000,
        }],
        keys: [{
          key_masked: "sk-pool-aaaa…aaaa", requests: 2,
          official_nano: 25_000_000, charged_nano: 10_000_000,
        }],
      });
    }
    if (path.startsWith("/admin/account/")) {
      const accountId = path.slice("/admin/account/".length);
      if (this.missingAccountIds.has(accountId)) {
        return Response.json({ error: "unknown account" }, { status: 404 });
      }
      return Response.json({
        account: accountId, balance_nano: 37_000_000_000,
        spent_nano: 12, reserved_nano: 0, balance: "$37.000000000", mult_bp: 2000,
        status: "active", handle: null,
        funding: {
          account_class: "b2c",
          funding_enforcement: "strict",
          reconciliation_state: "verified",
          bucket_count: 2,
          paid_balance_nano: 33_000_000_000,
          bonus_balance_nano: 4_000_000_000,
          other_balance_nano: 0,
          unattributed_balance_nano: 0,
          paid_reserved_nano: 0,
          bonus_reserved_nano: 0,
          other_reserved_nano: 0,
          unattributed_reserved_nano: 0,
          paid_spent_nano: 12,
          bonus_spent_nano: 0,
          other_spent_nano: 0,
          unattributed_spent_nano: 0,
        },
      });
    }
    return Response.json({ error: "not found" }, { status: 404 });
  }
}
