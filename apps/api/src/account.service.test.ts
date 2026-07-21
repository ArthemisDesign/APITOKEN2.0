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
    expect(account).toMatchObject({ balanceNano: "37000000000", reservedNano: "0", spentNano: "12" });
    expect(JSON.stringify(account)).not.toContain("acct_alice");

    const ledger = await service.getLedger(aliceId, 25) as Record<string, unknown>;
    expect(ledger).toMatchObject({ entries: [{ amountNano: "37000000000", reference: "payment:1" }] });
    expect(JSON.stringify(ledger)).not.toContain("acct_alice");
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
    engine.recoveredAccountId = "acct_recovered";
    await expect(service.ensureEngineAccount(aliceId)).resolves.toBe("acct_recovered");
    const mapping = await database.pool.query(`
      SELECT engine_account_id, status FROM engine_accounts WHERE user_id = $1
    `, [aliceId]);
    expect(mapping.rows[0]).toEqual({ engine_account_id: "acct_recovered", status: "active" });
    expect(engine.signupCredits).toEqual([{
      account: "acct_recovered", amountNano: "4000000000", reference: `signup-bonus:${aliceId}`,
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
  if (oauthProvider) {
    await database.pool.query(`
      INSERT INTO auth_identities (id, user_id, provider, subject, email, email_verified)
      VALUES ($1, $2, $3, $4, $5, true)
    `, [randomUUID(), userId, oauthProvider, `${oauthProvider}:${userId}`, email]);
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
      });
    }
    return Response.json({ error: "not found" }, { status: 404 });
  }
}
