import { afterEach, describe, expect, it, vi } from "vitest";
import { EngineClient, EngineClientError } from "./index.js";

afterEach(() => vi.useRealTimers());

describe("EngineClient", () => {
  it("sends nanodollars without floating-point conversion", async () => {
    let requestBody = "";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (_input, init) => {
        requestBody = String(init?.body);
        return new Response('{"account":"acct_test","balance_nano":9007199254740993123,"balance":"$9007199254.740993123"}');
      },
    });

    const result = await client.creditAccount("acct_test", 9_007_199_254_740_993_123n, "payment:test");
    expect(JSON.parse(requestBody)).toEqual({
      amount_nano: "9007199254740993123",
      ref: "payment:test",
    });
    expect(result.balance_nano).toBe("9007199254740993123");
  });

  it("preserves the HTTP status when the engine returns a non-JSON error", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => new Response("Failed to deserialize the JSON body", { status: 422 }),
    });

    await expect(client.creditAccount("acct_test", 1n, "payment:test")).rejects.toMatchObject({
      message: "engine returned HTTP 422 with a non-JSON response",
      status: 422,
      retryable: false,
    });
  });

  it("rejects policy values that cannot be represented safely by the engine", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2030-01-01T00:00:00.100Z"));
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => { throw new Error("validation should happen before fetch"); },
    });

    await expect(client.issueKey("acct_test", {
      spendLimitNano: 9_223_372_036_854_775_808n,
    })).rejects.toThrow("signed 64-bit");
    await expect(client.issueKey("acct_test", {
      expiresAt: new Date("2030-01-01T00:00:00.900Z"),
    })).rejects.toThrow("whole second");
  });

  it("does not send the control key to the public health endpoint", async () => {
    let sentHeaders: RequestInit["headers"];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "must-not-leak",
      fetch: async (_input, init) => {
        sentHeaders = init?.headers;
        return new Response('{"ok":true}');
      },
    });
    await expect(client.health()).resolves.toBe(true);
    expect(sentHeaders).not.toHaveProperty("x-api-key");
  });

  it("normalizes exact engine integer fields to decimal strings", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        account: "acct_test",
        balance_nano: 0,
        spent_nano: 12,
        reserved_nano: 0,
        balance: "$0.000000000",
        mult_bp: 2000,
        status: "active",
        handle: null,
        funding: {
          account_class: "b2c",
          funding_enforcement: "strict",
          reconciliation_state: "verified",
          bucket_count: 2,
          paid_balance_nano: 700,
          bonus_balance_nano: 200,
          other_balance_nano: 0,
          unattributed_balance_nano: 0,
          paid_reserved_nano: 40,
          bonus_reserved_nano: 0,
          other_reserved_nano: 0,
          unattributed_reserved_nano: 0,
          paid_spent_nano: 0,
          bonus_spent_nano: 300,
          other_spent_nano: 0,
          unattributed_spent_nano: 0,
        },
      }),
    });
    const account = await client.getAccount("acct_test");
    expect(account).toMatchObject({ balance_nano: "0", spent_nano: "12", reserved_nano: "0" });
    expect(account.funding).toMatchObject({
      bucket_count: "2",
      paid_balance_nano: "700",
      bonus_balance_nano: "200",
      paid_reserved_nano: "40",
    });
  });

  it("reads a bounded account page with one batch request", async () => {
    let requestUrl = "";
    let requestBody = "";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        requestUrl = String(input);
        requestBody = String(init?.body);
        return Response.json({
          accounts: [{
            account: "acct_one",
            balance_nano: 10,
            spent_nano: 20,
            reserved_nano: 0,
            balance: "$0.000000010",
            mult_bp: 2000,
            status: "active",
            handle: "user:one",
          }],
        });
      },
    });

    await expect(client.getAccounts(["acct_one", "acct_one"])).resolves.toMatchObject([
      { account: "acct_one", balance_nano: "10", spent_nano: "20" },
    ]);
    expect(requestUrl).toBe("http://engine.test/admin/accounts/query");
    expect(JSON.parse(requestBody)).toEqual({ account_ids: ["acct_one"] });
    await expect(client.getAccounts([])).resolves.toEqual([]);
  });

  it("manages keys by a non-secret stable identifier", async () => {
    const requests: Array<{ url: string; body: string }> = [];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const url = String(input);
        requests.push({ url, body: String(init?.body ?? "") });
        if (url.endsWith("/admin/key")) {
          return Response.json({
            key: "sk-pool-secret", key_id: "key_public", account: "acct_test", label: "prod",
          });
        }
        if (url.endsWith("/policy")) {
          return Response.json({
            key_id: "key_public", spend_limit_nano: null, expires_ts: 4_070_908_800, updated: 1,
          });
        }
        if (url.includes("/key-id/")) {
          return Response.json({ key_id: "key_public", status: "disabled", updated: 1 });
        }
        return Response.json({
          account: "acct_test",
          keys: [{
            key_id: "key_public", key_masked: "sk-pool-sec…cret", label: "prod",
            status: "active", spent_nano: 0, spent: "$0.000000000",
          }],
        });
      },
    });

    const expiresAt = new Date("2099-01-01T00:00:00.000Z");
    const issued = await client.issueKey("acct_test", {
      label: "prod", spendLimitNano: 9_007_199_254_740_993_123n, expiresAt,
    });
    expect(issued.key_id).toBe("key_public");
    expect(JSON.parse(requests[0]!.body)).toEqual({
      account_id: "acct_test",
      label: "prod",
      spend_limit_nano: "9007199254740993123",
      expires_ts: 4_070_908_800,
    });
    await expect(client.listKeys("acct_test")).resolves.toMatchObject([{ spent_nano: "0" }]);
    await expect(client.replaceKeyPolicy("acct_test", "key_public", {
      spendLimitNano: null,
      expiresAt,
    })).resolves.toBeUndefined();
    expect(requests.at(-1)).toEqual({
      url: "http://engine.test/admin/account/acct_test/key-id/key_public/policy",
      body: '{"spend_limit_nano":null,"expires_ts":4070908800}',
    });
    await expect(client.disableKey("key_public")).resolves.toBeUndefined();
    expect(requests.at(-1)?.url).toContain("/admin/key-id/key_public/status");
    expect(requests.at(-1)?.body).not.toContain("sk-pool-secret");
  });

  it("validates replacement policies before contacting the engine", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => { throw new Error("validation should happen before fetch"); },
    });
    await expect(client.replaceKeyPolicy("acct_test", "key_public", {
      spendLimitNano: 0n,
      expiresAt: null,
    })).rejects.toThrow("positive signed 64-bit");
    await expect(client.replaceKeyPolicy("acct_test", "key_public", {
      spendLimitNano: null,
      expiresAt: new Date(0),
    })).rejects.toThrow("whole second in the future");
  });

  it("reads a bounded ledger without numeric money conversion", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        account: "acct_test",
        entries: [{
          id: 1, kind: "topup", amount_nano: 1_000_000_000, amount: "$1.000000000",
          key_masked: null, ref: "payment:1", balance_after_nano: 1_000_000_000, ts: 1_700_000_000,
        }, {
          id: "9007199254740993123",
          kind: "charge",
          request_id: "request-strict",
          amount_nano: "300",
          amount: "$0.000000300",
          key_masked: "sk-pool-read…only",
          ref: "provider:read",
          balance_after_nano: "9007199254740993000",
          ts: "1700000001",
          model: "claude-read",
          provider: "anthropic",
          official_nano: "600",
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
            tariff_priced_ts: "1700000001",
            official_nano: "600",
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
            commission_eligible: true,
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
      }),
    });
    const entries = await client.getLedger("acct_test", 10);
    expect(entries[0]).toMatchObject({ id: "1", amount_nano: "1000000000", ts: "1700000000" });
    expect(entries[0]?.attribution).toBeUndefined();
    expect(entries[0]?.funding_allocations).toBeUndefined();
    expect(entries[1]).toMatchObject({
      id: "9007199254740993123",
      request_id: "request-strict",
      official_nano: "600",
      attribution: {
        source_policy_digest: "read-source-policy",
        bonus_funded_nano: "300",
        runtime_manifest_digest: "read-runtime",
      },
      funding_allocations: [{
        bucket_id: "read-bonus",
        bucket_version: "1",
        amount_nano: "300",
      }],
    });
    await expect(client.getLedger("acct_test", 0)).rejects.toThrow("limit");
  });

  it("reads ledger pages after an exact cursor", async () => {
    let requestedUrl = "";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input) => {
        requestedUrl = String(input);
        return Response.json({ account: "acct_test", entries: [] });
      },
    });
    await expect(client.getLedgerAfter("acct_test", 9_007_199_254_740_993_123n, 1000)).resolves.toEqual([]);
    expect(requestedUrl).toContain("after_id=9007199254740993123&limit=1000");
  });

  it("preserves exact authoritative usage breakdowns", async () => {
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        account: "acct_test",
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
          day_ts: 1_701_993_600, provider: "openai", requests: 1,
          official_nano: 15_000_000, charged_nano: 6_000_000,
        }, {
          day_ts: 1_701_993_600, provider: "anthropic", requests: 1,
          official_nano: 10_000_000, charged_nano: 4_000_000,
        }],
        keys: [{
          key_masked: "sk-pool-test…test", requests: 2,
          official_nano: 25_000_000, charged_nano: 10_000_000,
        }],
      }),
    });

    const usage = await client.getUsage("acct_test", "30d");
    expect(usage.total_official_nano).toBe("25000000");
    expect(usage.buckets.unattributed_legacy.official_nano).toBe("10000000");
    expect(usage.daily[0]).toMatchObject({
      day_ts: 1_701_993_600,
      official_nano: "25000000",
      charged_nano: "10000000",
    });
    expect(usage.daily_providers).toEqual([
      expect.objectContaining({ provider: "openai", official_nano: "15000000" }),
      expect.objectContaining({ provider: "anthropic", official_nano: "10000000" }),
    ]);
    expect(usage.keys[0]).toMatchObject({
      key_masked: "sk-pool-test…test",
      official_nano: "25000000",
      charged_nano: "10000000",
    });
  });

  it("acknowledges an exact durable ledger cursor", async () => {
    let request: { url: string; body: string } | undefined;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        request = { url: String(input), body: String(init?.body) };
        return Response.json({ account: "acct_test", consumer: "pricing", last_id: "42" });
      },
    });
    await expect(client.acknowledgeLedger("acct_test", 42n)).resolves.toBeUndefined();
    expect(request).toEqual({
      url: "http://engine.test/admin/account/acct_test/ledger/ack",
      body: '{"last_id":"42"}',
    });
    await expect(client.acknowledgeLedger("acct_test", -1n)).rejects.toThrow("lastId");
  });

  it("updates account pricing through the control API", async () => {
    let request: { url: string; body: string } | undefined;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        request = { url: String(input), body: String(init?.body) };
        return Response.json({ account: "acct_test", mult_bp: 3500, updated: 1 });
      },
    });
    await expect(client.setAccountMultiplier("acct_test", 3500)).resolves.toBeUndefined();
    expect(request).toEqual({
      url: "http://engine.test/admin/account/acct_test/pricing",
      body: '{"mult_bp":3500}',
    });
    await expect(client.setAccountMultiplier("acct_test", 10_001)).rejects.toThrow("multiplierBp");
  });

  it("preserves complete immutable pricing identities through prepare, state, and activate", async () => {
    const catalog = {
      product_id: "main",
      generation: 1,
      schema_version: 1,
      capability_generation: 1,
      capability_digest: "capability-v1",
      content_digest: "catalog-v1",
      entries: [{
        provider_id: "anthropic",
        canonical_model_id: "claude-sonnet",
        enabled: true,
      }],
    };
    const policy = {
      account_id: "acct_test",
      effective_version: 1,
      policy_id: "global-b2c",
      policy_version: 1,
      source_policy_digest: "source-policy-v1",
      owner_type: "global_b2c" as const,
      owner_id: "global",
      account_class: "b2c" as const,
      product_id: "main",
      schema_version: 1,
      catalog_generation: 1,
      switch_generation: 1,
      content_digest: "account-policy-v1",
      replacement_locked: false,
      rules: [{
        rule_id: "anthropic-track",
        rule_digest: "anthropic-track-v1",
        scope: { provider: { provider_id: "anthropic" } },
        pricing_mode: "track" as const,
        rule_origin: "managed" as const,
        discount_bps: null,
        payable_multiplier_bp: 4000,
        track_eligible: true,
        retention_eligible: true,
        commission_eligible: true,
      }],
    };
    const binding = {
      policy_enforcement: "shadow" as const,
      funding_enforcement: "legacy_single" as const,
      reconciliation_state: "pending" as const,
    };
    const requests: Array<{ url: string; body: unknown }> = [];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const url = String(input);
        const body = init?.body === undefined ? undefined : JSON.parse(String(init.body));
        requests.push({ url, body });
        if (url.endsWith("/catalog/prepare")) {
          return Response.json({ result: "stored", identity: { catalog } });
        }
        if (url.endsWith("/catalog/main/active")) {
          return Response.json({ error: "no active catalog" }, { status: 404 });
        }
        if (url.endsWith("/catalog/main/activate")) {
          return Response.json({ result: "applied", identity: body });
        }
        if (url.endsWith("/policy/prepare")) {
          return Response.json({ result: "stored", identity: { policy } });
        }
        if (url.endsWith("/policy/acct_test/state")) {
          return Response.json({ state: { account_id: "acct_test", policy: "unbound" } });
        }
        if (url.endsWith("/policy/acct_test/activate")) {
          return Response.json({
            result: "applied",
            identity: {
              policy,
              activation: {
                account_id: "acct_test",
                effective_version: 1,
                content_digest: "account-policy-v1",
                binding,
              },
              expectation: "unbound",
            },
          });
        }
        throw new Error(`unexpected request ${url}`);
      },
    });

    await expect(client.preparePricingCatalog(catalog)).resolves.toMatchObject({ result: "stored" });
    await expect(client.getActivePricingCatalog("main")).resolves.toBeNull();
    await expect(client.activatePricingCatalog(catalog, "absent")).resolves.toMatchObject({
      result: "applied",
      identity: { catalog, expectation: "absent" },
    });
    await expect(client.prepareAccountPolicy(policy)).resolves.toMatchObject({ result: "stored" });
    await expect(client.getAccountPricingState("acct_test")).resolves.toBe("unbound");
    await expect(client.activateAccountPolicy(policy, binding, "unbound")).resolves.toMatchObject({
      result: "applied",
      identity: { policy, expectation: "unbound" },
    });
    expect(requests.find((request) => request.url.endsWith("/catalog/main/activate"))?.body)
      .toEqual({ catalog, expectation: "absent" });
    expect(requests.find((request) => request.url.endsWith("/policy/acct_test/activate"))?.body)
      .toEqual({ policy, binding, expectation: "unbound" });
  });

  it("returns typed pricing conflicts but rejects a forged successful ACK", async () => {
    const catalog = {
      product_id: "main",
      generation: 1,
      schema_version: 1,
      capability_generation: 1,
      capability_digest: "capability-v1",
      content_digest: "catalog-v1",
      entries: [],
    };
    let forged = false;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => forged
        ? Response.json({
            result: "applied",
            identity: { catalog: { ...catalog, capability_digest: "forged" }, expectation: "absent" },
          })
        : Response.json({
            result: "rejected",
            code: "cas_mismatch",
            identity: { catalog, expectation: "absent" },
            rejection: { cas_mismatch: { actual: null } },
          }, { status: 409 }),
    });

    await expect(client.activatePricingCatalog(catalog, "absent")).resolves.toMatchObject({
      result: "rejected",
      code: "cas_mismatch",
    });
    forged = true;
    await expect(client.activatePricingCatalog(catalog, "absent"))
      .rejects.toThrow("ACK identity does not match");
  });

  it("treats malformed pricing responses as permanent protocol failures", async () => {
    const catalog = {
      product_id: "main",
      generation: 1,
      schema_version: 1,
      capability_generation: 1,
      capability_digest: "capability-v1",
      content_digest: "catalog-v1",
      entries: [],
    };
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        result: "rejected",
        code: "locked",
        identity: { catalog, expectation: "absent" },
        rejection: { cas_mismatch: { actual: null } },
      }, { status: 423 }),
    });

    const failure = await client.activatePricingCatalog(catalog, "absent").catch((error) => error);
    expect(failure).toBeInstanceOf(EngineClientError);
    expect(failure).toMatchObject({
      message: "engine returned a malformed pricing response",
      status: 423,
      retryable: false,
    });
  });

  it("updates account status through the control API", async () => {
    let request: { url: string; body: string } | undefined;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        request = { url: String(input), body: String(init?.body) };
        return Response.json({ account: "acct_test", status: "disabled", updated: 1 });
      },
    });
    await expect(client.setAccountStatus("acct_test", "disabled")).resolves.toBeUndefined();
    expect(request).toEqual({
      url: "http://engine.test/admin/account/acct_test/status",
      body: '{"status":"disabled"}',
    });
  });
});
