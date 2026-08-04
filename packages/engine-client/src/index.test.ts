import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  PricingReleaseAssignmentExtensionV2,
  PricingReleaseActivationRequestV2,
  PricingReleasePolicyV2,
  PricingReleaseProvisioningContextV2,
  PricingReleaseRecoveryLinkV2,
  PricingReleaseV2,
} from "@claude-api/contracts";
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

  it("retries an idempotent GET once after a transient network failure", async () => {
    let calls = 0;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => {
        calls += 1;
        if (calls === 1) throw new TypeError("fetch failed");
        return Response.json({
          account: "acct_test",
          balance_nano: 0,
          spent_nano: 12,
          reserved_nano: 0,
          balance: "$0.000000000",
          mult_bp: 2000,
          status: "active",
          handle: null,
          funding: null,
        });
      },
    });

    const account = await client.getAccount("acct_test");
    expect(calls).toBe(2);
    expect(account).toMatchObject({ account: "acct_test", spent_nano: "12" });
  });

  it("gives up an idempotent GET after exactly one retry on repeated HTTP 503", async () => {
    let calls = 0;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => {
        calls += 1;
        return Response.json({ error: "engine is restarting" }, { status: 503 });
      },
    });

    await expect(client.getAccount("acct_test")).rejects.toMatchObject({
      message: "engine is restarting",
      status: 503,
      retryable: true,
    });
    expect(calls).toBe(2);
  });

  it("never retries a mutation after a transient network failure", async () => {
    let calls = 0;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => {
        calls += 1;
        throw new TypeError("fetch failed");
      },
    });

    await expect(client.setAccountMultiplier("acct_test", 2000)).rejects.toMatchObject({
      message: "engine request failed",
      retryable: true,
    });
    expect(calls).toBe(1);
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
        }, {
          // The engine tags Gemini traffic with its registry id "google" — a
          // strict enum here 500'd the whole usage endpoint once already.
          day_ts: 1_701_993_600, provider: "google", requests: 1,
          official_nano: 5_000_000, charged_nano: 2_000_000,
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
      expect.objectContaining({ provider: "google", official_nano: "5000000" }),
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

  it("delivers one exact locked OpenKeys transition and distinguishes typed rejections", async () => {
    const successor = {
      account_id: "acct_ok_legacy",
      effective_version: 2,
      policy_id: "policy:openkeys:legacy:source-1",
      policy_version: 2,
      source_policy_digest: "sha256:managed-source",
      owner_type: "open_keys" as const,
      owner_id: "source-1",
      account_class: "open_keys" as const,
      product_id: "openkeys",
      schema_version: 1,
      catalog_generation: 5,
      switch_generation: 5,
      content_digest: "sha256:managed-policy",
      replacement_locked: false,
      rules: [{
        rule_id: "openkeys-anthropic-1to1",
        rule_digest: "sha256:anthropic-rule",
        scope: { provider: { provider_id: "anthropic" } },
        pricing_mode: "discount" as const,
        rule_origin: "managed" as const,
        discount_bps: 0,
        payable_multiplier_bp: 10_000,
        track_eligible: false,
        retention_eligible: false,
        commission_eligible: false,
      }],
    };
    const expectedActive = {
      target: { version: 1, content_digest: "sha256:legacy-policy" },
      binding: {
        policy_enforcement: "legacy_scalar" as const,
        funding_enforcement: "legacy_single" as const,
        reconciliation_state: "pending" as const,
      },
    };
    const transitionIdentity = {
      policy: successor,
      active: {
        target: { version: 2, content_digest: "sha256:managed-policy" },
        binding: {
          policy_enforcement: "shadow",
          funding_enforcement: "legacy_single",
          reconciliation_state: "verified",
        },
      },
      expected_active: expectedActive,
    };
    const requests: Array<{ url: string; body: unknown }> = [];
    let mode: "applied" | "unchanged" | "cas" | "locked" = "applied";
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const url = String(input);
        const body = init?.body === undefined ? undefined : JSON.parse(String(init.body));
        requests.push({ url, body });
        if (url.endsWith("/policy/acct_ok_legacy/locked-openkeys-transition")) {
          if (mode === "cas") {
            return Response.json({
              result: "rejected",
              code: "policy_cas_mismatch",
              identity: transitionIdentity,
              rejection: { policy_cas_mismatch: { actual: "unbound" } },
            }, { status: 409 });
          }
          if (mode === "locked") {
            return Response.json({
              result: "rejected",
              code: "locked",
              identity: transitionIdentity,
              rejection: "locked",
            }, { status: 423 });
          }
          return Response.json({ result: mode, identity: transitionIdentity });
        }
        throw new Error(`unexpected request ${url}`);
      },
    });

    await expect(
      client.lockedOpenkeysPolicyTransition("acct_ok_legacy", {
        policy: successor,
        expected_active: expectedActive,
      }),
    ).resolves.toMatchObject({ result: "applied", identity: transitionIdentity });
    expect(requests.at(-1)).toEqual({
      url: "http://engine.test/admin/pricing/policy/acct_ok_legacy/locked-openkeys-transition",
      body: { policy: successor, expected_active: expectedActive },
    });

    mode = "unchanged";
    await expect(
      client.lockedOpenkeysPolicyTransition("acct_ok_legacy", {
        policy: successor,
        expected_active: expectedActive,
      }),
    ).resolves.toMatchObject({ result: "unchanged" });

    mode = "cas";
    await expect(
      client.lockedOpenkeysPolicyTransition("acct_ok_legacy", {
        policy: successor,
        expected_active: expectedActive,
      }),
    ).resolves.toMatchObject({ result: "rejected", code: "policy_cas_mismatch" });

    mode = "locked";
    await expect(
      client.lockedOpenkeysPolicyTransition("acct_ok_legacy", {
        policy: successor,
        expected_active: expectedActive,
      }),
    ).resolves.toMatchObject({ result: "rejected", code: "locked" });

    await expect(
      client.lockedOpenkeysPolicyTransition("acct_other", {
        policy: successor,
        expected_active: expectedActive,
      }),
    ).rejects.toThrow("does not match the target account");
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

  it("uses the strict dormant release-v2 prepare/read contract without an activation call", async () => {
    const policy: PricingReleasePolicyV2 = {
      policy_id: "global-b2c-v2",
      policy_version: 1,
      owner_type: "global_b2c",
      owner_id: "global-b2c",
      account_class: "b2c",
      product_id: "main",
      billing_mode: "balance",
      schema_version: 2,
      capability_generation: 2,
      capability_digest: "capability-v2",
      catalog_generation: 2,
      catalog_digest: "main-catalog-v2",
      switch_generation: 2,
      switch_digest: "switches-v2",
      content_digest: "global-b2c-policy-v2",
      rules: [{
        rule_id: "global-50",
        rule_digest: "global-50-v1",
        scope: { scope: "global" },
        discount_bps: 5_000,
        payable_multiplier_bp: 5_000,
      }],
    };
    const release: PricingReleaseV2 = {
      generation: 10,
      release_kind: "target",
      schema_version: 2,
      capability_generation: 2,
      capability_digest: "capability-v2",
      main_catalog_generation: 2,
      main_catalog_digest: "main-catalog-v2",
      openkeys_catalog_generation: 2,
      openkeys_catalog_digest: "openkeys-catalog-v2",
      switch_generation: 2,
      switch_digest: "switches-v2",
      inventory_digest: "inventory-v2",
      policy_manifest_digest: "policy-manifest-v2",
      assignment_manifest_digest: "assignment-manifest-v2",
      funding_manifest_digest: "funding-manifest-v2",
      minimum_runtime_schema_version: 2,
      content_digest: "target-release-v2",
      assignments: [{
        account_id: "acct_test",
        account_class: "b2c",
        policy_id: policy.policy_id,
        policy_version: policy.policy_version,
        policy_digest: policy.content_digest,
        billing_mode: "balance",
        funding_generation: 3,
        purpose: null,
        responsible: null,
        assignment_digest: "assignment-acct-test-v2",
      }],
    };
    const recoveryLink: PricingReleaseRecoveryLinkV2 = {
      target_generation: 10,
      target_digest: "target-release-v2",
      recovery_generation: 11,
      recovery_digest: "recovery-release-v2",
      link_digest: "target-recovery-link-v2",
    };
    const extension: PricingReleaseAssignmentExtensionV2 = {
      provisioning_head_generation: 10,
      provisioning_head_digest: "target-release-v2",
      provisioning_head_version: 1,
      paired_recovery_generation: 11,
      paired_recovery_digest: "recovery-release-v2",
      extension_group_digest: "extension-group-acct-new-v2",
      members: [
        {
          release_generation: 10,
          assignment: {
            ...release.assignments[0]!,
            account_id: "acct_new",
            assignment_digest: "assignment-acct-new-target-v2",
          },
          extension_digest: "extension-acct-new-target-v2",
        },
        {
          release_generation: 11,
          assignment: {
            ...release.assignments[0]!,
            account_id: "acct_new",
            assignment_digest: "assignment-acct-new-recovery-v2",
          },
          extension_digest: "extension-acct-new-recovery-v2",
        },
      ],
    };
    const sourceStateDigest = `sha256:v2:${"a".repeat(64)}`;
    const normalizationDigest = `sha256:v2:${"b".repeat(64)}`;
    const normalizationPlan = {
      account_id: "acct_test",
      account_status: "active" as const,
      status: "ready" as const,
      source: "ledger_replay" as const,
      source_state_digest: sourceStateDigest,
      normalization_digest: normalizationDigest,
      funding_generation: 1,
      funding_head_version: 1,
      balance_nano: "9007199254740993123",
      reserved_nano: "7",
      spent_nano: "11",
      lots: [{
        lot_id: "fundv2_paid",
        source_type: "paid" as const,
        source_ref: "stage6:paid-residual:v2",
        balance_nano: "9007199254740993123",
        reserved_nano: "7",
        spent_nano: "11",
        version: 1,
        status: "active" as const,
      }],
      blockers: [],
    };
    const requests: Array<{ url: string; method: string; body: unknown }> = [];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        const url = String(input);
        const body = init?.body === undefined ? undefined : JSON.parse(String(init.body));
        requests.push({ url, method: init?.method ?? "GET", body });
        if (url.endsWith("/pricing/v2/policy/prepare")) {
          return Response.json({
            result: "stored",
            identity: {
              policy_id: policy.policy_id,
              policy_version: policy.policy_version,
              content_digest: policy.content_digest,
            },
          });
        }
        if (url.endsWith("/pricing/v2/policy/global-b2c-v2/version/1")) {
          return Response.json({ policy });
        }
        if (url.endsWith("/pricing/v2/release/prepare")) {
          return Response.json({
            result: "unchanged",
            identity: {
              generation: release.generation,
              content_digest: release.content_digest,
              release_kind: release.release_kind,
            },
          });
        }
        if (url.endsWith("/pricing/v2/release/10")) {
          return Response.json({ release });
        }
        if (url.endsWith("/pricing/v2/recovery-link/prepare")) {
          return Response.json({
            result: "stored",
            identity: {
              target_generation: recoveryLink.target_generation,
              recovery_generation: recoveryLink.recovery_generation,
              link_digest: recoveryLink.link_digest,
            },
          });
        }
        if (url.endsWith("/pricing/v2/recovery-link/10/11")) {
          return Response.json({ recovery_link: recoveryLink });
        }
        if (url.endsWith("/pricing/v2/assignment-extension/prepare")) {
          return Response.json({
            result: "stored",
            identity: {
              provisioning_head_generation: extension.provisioning_head_generation,
              provisioning_head_version: extension.provisioning_head_version,
              account_id: "acct_new",
              extension_group_digest: extension.extension_group_digest,
            },
          });
        }
        if (url.endsWith("/pricing/v2/assignment-extension/1/acct_new")) {
          return Response.json({ extension });
        }
        if (url.endsWith("/pricing/v2/head")) {
          return Response.json({ head: null });
        }
        if (url.endsWith("/pricing/v2/inventory?after_account_id=acct_before&limit=500")) {
          return new Response('{"inventory":{"accounts":[{"account_id":"acct_test","status":"active",' +
            '"multiplier_bp":5000,"balance_nano":9007199254740993123,"reserved_nano":7,' +
            '"spent_nano":11,"funding_generation":3,"funding_head_version":4}],' +
            '"next_after_account_id":null}}');
        }
        if (url.endsWith("/pricing/v2/funding/acct_test/normalization")) {
          if (init?.method === "POST") {
            return Response.json({
              result: {
                status: "stored",
                normalization: {
                  ...normalizationPlan,
                  status: "normalized",
                  source: "stored_generation",
                },
              },
            });
          }
          return Response.json({ normalization: normalizationPlan });
        }
        throw new Error(`unexpected request ${url}`);
      },
    });

    await expect(client.preparePricingReleasePolicyV2(policy)).resolves.toMatchObject({ result: "stored" });
    await expect(client.getPricingReleasePolicyV2(policy.policy_id, 1)).resolves.toEqual(policy);
    await expect(client.preparePricingReleaseV2(release)).resolves.toMatchObject({ result: "unchanged" });
    await expect(client.getPricingReleaseV2(10)).resolves.toEqual(release);
    await expect(client.preparePricingReleaseRecoveryLinkV2(recoveryLink)).resolves.toMatchObject({ result: "stored" });
    await expect(client.getPricingReleaseRecoveryLinkV2(10, 11)).resolves.toEqual(recoveryLink);
    await expect(client.preparePricingReleaseAssignmentExtensionV2(extension)).resolves.toMatchObject({
      result: "stored",
      identity: { account_id: "acct_new", provisioning_head_version: 1 },
    });
    await expect(client.getPricingReleaseAssignmentExtensionV2(1, "acct_new")).resolves.toEqual(extension);
    await expect(client.getPricingReleaseHeadV2()).resolves.toBeNull();
    await expect(client.getPricingReleaseInventoryV2({
      afterAccountId: "acct_before",
      limit: 500,
    })).resolves.toMatchObject({
      accounts: [{
        account_id: "acct_test",
        balance_nano: "9007199254740993123",
        reserved_nano: "7",
        spent_nano: "11",
      }],
      next_after_account_id: null,
    });
    await expect(client.getFundingNormalizationPlanV2("acct_test")).resolves.toEqual(normalizationPlan);
    await expect(client.applyFundingNormalizationV2("acct_test", {
      expected_source_state_digest: sourceStateDigest,
      expected_normalization_digest: normalizationDigest,
    })).resolves.toMatchObject({
      status: "stored",
      normalization: { status: "normalized", source: "stored_generation" },
    });
    expect(requests.find((request) => request.url.endsWith("/pricing/v2/policy/prepare")))
      .toMatchObject({ method: "POST", body: policy });
    expect(requests.find((request) => request.url.endsWith("/pricing/v2/release/prepare")))
      .toMatchObject({ method: "POST", body: release });
    expect(requests.find((request) => request.url.endsWith("/pricing/v2/assignment-extension/prepare")))
      .toMatchObject({ method: "POST", body: extension });
    expect(requests.find((request) => request.url.endsWith("/pricing/v2/funding/acct_test/normalization")
      && request.method === "POST")).toMatchObject({
      body: {
        expected_source_state_digest: sourceStateDigest,
        expected_normalization_digest: normalizationDigest,
      },
    });
    expect(requests.some((request) => request.url.includes("activate"))).toBe(false);

    const forgedReadClient = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({ release: { ...release, generation: 12 } }),
    });
    await expect(forgedReadClient.getPricingReleaseV2(10)).rejects.toThrow("different pricing release");
  });

  it("reads one strict nullable provisioning context and rejects forged lineage", async () => {
    const digest = (seed: string): string => `sha256:v2:${seed.repeat(64)}`;
    const legacyDigest = (seed: string): string => `sha256:v1:${seed.repeat(64)}`;
    const target = {
      generation: 10,
      release_kind: "target" as const,
      schema_version: 2 as const,
      capability_generation: 3,
      capability_digest: legacyDigest("a"),
      main_catalog_generation: 3,
      main_catalog_digest: legacyDigest("b"),
      openkeys_catalog_generation: 3,
      openkeys_catalog_digest: legacyDigest("c"),
      switch_generation: 3,
      switch_digest: legacyDigest("d"),
      inventory_digest: digest("e"),
      funding_manifest_digest: digest("f"),
      minimum_runtime_schema_version: 2,
      content_digest: digest("1"),
    };
    const recovery = {
      ...target,
      generation: 11,
      release_kind: "recovery" as const,
      content_digest: digest("2"),
    };
    const context: PricingReleaseProvisioningContextV2 = {
      head: {
        active_generation: target.generation,
        active_digest: target.content_digest,
        head_version: 1,
        updated_ts: 1_000,
      },
      activation: {
        activation_id: "1",
        activation_kind: "cutover",
        evidence_digest: digest("3"),
        activated_ts: 1_000,
      },
      active_release: target,
      paired_recovery: {
        release: recovery,
        recovery_link: {
          target_generation: target.generation,
          target_digest: target.content_digest,
          recovery_generation: recovery.generation,
          recovery_digest: recovery.content_digest,
          link_digest: digest("4"),
        },
      },
    };
    const paths: string[] = [];
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input) => {
        paths.push(String(input));
        return Response.json({ context });
      },
    });
    await expect(client.getPricingReleaseProvisioningContextV2()).resolves.toEqual(context);
    expect(paths).toEqual(["http://engine.test/admin/pricing/v2/provisioning-context"]);

    const preCutover = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({ context: null }),
    });
    await expect(preCutover.getPricingReleaseProvisioningContextV2()).resolves.toBeNull();

    const forged = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        context: {
          ...context,
          head: { ...context.head, active_digest: digest("9") },
        },
      }),
    });
    await expect(forged.getPricingReleaseProvisioningContextV2())
      .rejects.toThrow("malformed pricing response");
  });

  it("captures Stage 8 as raw integer-preserving JSON and keeps blocked evidence successful", async () => {
    const digestV2 = `sha256:v2:${"2".repeat(64)}`;
    const digestV1 = `sha256:v1:${"1".repeat(64)}`;
    const raw = JSON.stringify({
      schema_version: 2,
      captured_ts: 2_000,
      window_start_ts: 1_000,
      window_end_ts: 1_900,
      min_samples_per_provider: 1,
      gemini_client_admissions: 7,
      passed: false,
      release: {
        target_generation: 41,
        target_digest: digestV2,
        recovery_generation: 42,
        recovery_digest: digestV2,
        recovery_link_digest: digestV2,
        inventory_digest: digestV2,
        funding_digest: digestV2,
        target_assignment_count: 1,
        recovery_assignment_count: 1,
        active_head: null,
      },
      runtime_manifest: {
        generation: 3,
        digest: digestV2,
        capabilities: [{ schema_version: 2, generation: 3, digest: digestV2 }],
      },
      catalogs: [],
      switches: null,
      counts: {
        total_accounts: 1,
        active_accounts: 1,
        account_classes: { b2c: 1 },
        reconciled_accounts: 1,
        snapshots_by_provider: { anthropic: 1, google: 1, openai: 1 },
        evaluations_by_outcome: { resolved: 3 },
        comparisons: { different: 3 },
        scalar_parity_rows: 0,
        policy_divergence_rows: 3,
        gemini_usage_rows: 1,
        gemini_outbox_rows: 1,
        live_runtime_instances: 2,
        release_capable_runtime_instances: 1,
        legacy_inflight_reservations: 3,
        legacy_inflight_outbox_rows: 2,
      },
      financial_samples: [{
        subject_digest: digestV1,
        evaluation_digest: digestV2,
        provider_id: "google",
        account_class: "b2c",
        authorized_multiplier_bp: 10_000,
        payable_multiplier_bp: 5_000,
        official_hold_nano: "9223372036854775807",
        legacy_hold_nano: "9223372036854775807",
        policy_hold_nano: "4611686018427387904",
        comparison_result: "different",
      }],
      engine_inventory_digest: digestV2,
      funding_digest: digestV2,
      shadow_digest: digestV2,
      runtime_floor_digest: digestV2,
      legacy_inflight_count: 5,
      blockers: [{
        code: "live_runtime_below_release_v2_floor",
        count: 1,
        subject_digests: [digestV1],
      }],
      evidence_digest: digestV2,
    })
      .replace('"9223372036854775807"', "9223372036854775807")
      .replace('"9223372036854775807"', "9223372036854775807")
      .replace('"4611686018427387904"', "4611686018427387904");
    let request: { url: string; body: unknown } | undefined;
    let jsonCalled = false;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (input, init) => {
        request = { url: String(input), body: JSON.parse(String(init?.body)) };
        const response = new Response(raw);
        Object.defineProperty(response, "json", {
          value: () => {
            jsonCalled = true;
            throw new Error("response.json must not be used for Stage 8 evidence");
          },
        });
        return response;
      },
    });
    const capture = {
      target_generation: 41,
      recovery_generation: 42,
      window_start_ts: 1_000,
      window_end_ts: 1_900,
      min_samples_per_provider: 1,
      financial_sample_size: 100,
      gemini_client_admissions: 7,
    };

    await expect(client.capturePricingStage8EvidenceV2(capture)).resolves.toMatchObject({
      raw,
      evidence: {
        passed: false,
        legacy_inflight_count: "5",
        financial_samples: [{ official_hold_nano: "9223372036854775807" }],
      },
    });
    expect(jsonCalled).toBe(false);
    expect(request).toEqual({
      url: "http://engine.test/admin/pricing/v2/stage8-evidence/capture",
      body: capture,
    });
  });

  it("stops reading Stage 8 evidence at the raw response bound", async () => {
    const chunk = new Uint8Array(1024 * 1024);
    let emitted = 0;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => new Response(new ReadableStream<Uint8Array>({
        pull(controller) {
          if (emitted === 17) {
            controller.close();
            return;
          }
          emitted += 1;
          controller.enqueue(chunk);
        },
      })),
    });

    await expect(client.capturePricingStage8EvidenceV2({
      target_generation: 41,
      recovery_generation: 42,
      window_start_ts: 1_000,
      window_end_ts: 1_900,
      min_samples_per_provider: 1,
      financial_sample_size: 100,
      gemini_client_admissions: 7,
    })).rejects.toMatchObject({
      message: "engine Stage 8 evidence exceeds the bounded response size",
      retryable: false,
    });
    expect(emitted).toBe(17);
  });

  it("validates the exact pricing release activation request, receipt, and rejection", async () => {
    const digest = (value: string): string => `sha256:v2:${value.repeat(64)}`;
    const request: PricingReleaseActivationRequestV2 = {
      activation_kind: "cutover",
      expectation: "absent",
      evidence: {
        evidence_digest: digest("a"),
        target_generation: 41,
        target_digest: digest("b"),
        recovery_generation: 42,
        recovery_digest: digest("c"),
        engine_inventory_digest: digest("d"),
        funding_digest: digest("e"),
        shadow_digest: digest("f"),
        runtime_floor_digest: digest("0"),
        legacy_inflight_count: 7,
        engine_captured_ts: 1_000,
        observed_ts: 1_100,
        valid_until_ts: 1_400,
      },
      operator_id: "pricing-control-worker:test",
      reason: "activate exact prepared Stage 9 target",
    };
    let body: unknown;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async (_input, init) => {
        body = JSON.parse(String(init?.body));
        return Response.json({
          result: "applied",
          activation: {
            activation_id: 1,
            activation_kind: "cutover",
            from_generation: null,
            from_digest: null,
            expected_head_version: 0,
            head: {
              active_generation: 41,
              active_digest: digest("b"),
              head_version: 1,
              updated_ts: 1_200,
            },
            evidence_digest: digest("a"),
            operator_id: "pricing-control-worker:test",
            reason: "activate exact prepared Stage 9 target",
            activated_ts: 1_200,
          },
        });
      },
    });
    await expect(client.activatePricingReleaseV2(request)).resolves.toMatchObject({
      result: "applied",
      activation: { activation_id: "1", head: { head_version: 1 } },
    });
    expect(body).toEqual(request);

    const rejected = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        result: "rejected",
        code: "evidence_stale",
        rejection: {
          evidence_stale: { now_ts: 1_401, observed_ts: 1_100, valid_until_ts: 1_400 },
        },
      }, { status: 409 }),
    });
    await expect(rejected.activatePricingReleaseV2(request)).resolves.toMatchObject({
      result: "rejected",
      code: "evidence_stale",
    });

    const forged = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        result: "unchanged",
        activation: {
          activation_id: 1,
          activation_kind: "cutover",
          from_generation: null,
          from_digest: null,
          expected_head_version: 0,
          head: {
            active_generation: 41,
            active_digest: digest("9"),
            head_version: 1,
            updated_ts: 1_200,
          },
          evidence_digest: digest("a"),
          operator_id: "pricing-control-worker:test",
          reason: "activate exact prepared Stage 9 target",
          activated_ts: 1_200,
        },
      }),
    });
    await expect(forged.activatePricingReleaseV2(request))
      .rejects.toThrow("receipt does not match the immutable request");

    const mismatchedRejection = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => Response.json({
        result: "rejected",
        code: "invalid",
        rejection: { cas_mismatch: { actual: null } },
      }, { status: 400 }),
    });
    await expect(mismatchedRejection.activatePricingReleaseV2(request))
      .rejects.toThrow("malformed pricing response");
  });

  it("rejects malformed release-v2 scopes and cursor bounds before contacting the engine", async () => {
    let calls = 0;
    const client = new EngineClient({
      baseUrl: "http://engine.test",
      controlKey: "test-control-key",
      fetch: async () => {
        calls += 1;
        throw new Error("validation should happen before fetch");
      },
    });
    const malformedPolicy = {
      policy_id: "global-b2c-v2",
      policy_version: 1,
      owner_type: "global_b2c",
      owner_id: "global-b2c",
      account_class: "b2c",
      product_id: "main",
      billing_mode: "balance",
      schema_version: 2,
      capability_generation: 2,
      capability_digest: "capability-v2",
      catalog_generation: 2,
      catalog_digest: "main-catalog-v2",
      switch_generation: 2,
      switch_digest: "switches-v2",
      content_digest: "global-b2c-policy-v2",
      rules: [{
        rule_id: "google-provider",
        rule_digest: "google-provider-v1",
        scope: {
          scope: "provider",
          provider_id: "google",
          canonical_model_id: "must-not-be-a-provider-sibling",
        },
        discount_bps: 6_000,
        payable_multiplier_bp: 4_000,
      }],
    } as unknown as PricingReleasePolicyV2;

    await expect(client.preparePricingReleasePolicyV2(malformedPolicy)).rejects.toThrow();
    await expect(client.getPricingReleaseInventoryV2({ limit: 501 })).rejects.toThrow();
    await expect(client.getPricingReleaseInventoryV2({ afterAccountId: "not-an-account" })).rejects.toThrow();
    await expect(client.getPricingReleaseRecoveryLinkV2(10, 10)).rejects.toThrow("newer");
    await expect(client.getFundingNormalizationPlanV2("not-an-account")).rejects.toThrow();
    await expect(client.applyFundingNormalizationV2("acct_test", {
      expected_source_state_digest: "sha256:v2:not-canonical",
      expected_normalization_digest: `sha256:v2:${"b".repeat(64)}`,
    })).rejects.toThrow();
    await expect(client.preparePricingReleaseAssignmentExtensionV2({
      provisioning_head_generation: 10,
      provisioning_head_digest: "target-v2",
      provisioning_head_version: 1,
      paired_recovery_generation: 11,
      paired_recovery_digest: null,
      extension_group_digest: "group-v2",
      members: [],
    } as never)).rejects.toThrow();
    await expect(client.getPricingReleaseAssignmentExtensionV2(0, "acct_test")).rejects.toThrow();
    expect(calls).toBe(0);
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
