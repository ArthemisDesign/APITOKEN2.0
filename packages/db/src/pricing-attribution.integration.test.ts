import { randomUUID } from "node:crypto";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  applyPricingLedgerPage,
  applyPricingProviderBackfillPage,
  completePricingProviderBackfill,
  getPricingProviderBackfillCursor,
  PricingLedgerAttributionError,
} from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("immutable pricing ledger attribution", () => {
  let database: Database;
  let userId: string;
  let bindingId: string;
  const engineAccountId = "acct_pricing_attribution";
  const policyId = "policy-pricing-attribution";
  const now = new Date(Date.now() - 60_000);

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await database.pool.end();
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE
        pricing_usage_funding_allocations, pricing_usage_attributions,
        pricing_usage_events, pricing_usage_cursors, pricing_months,
        account_policy_rules, account_policy_versions, account_policy_bindings,
        pricing_policy_rules, pricing_policy_heads, pricing_policy_versions, pricing_policies,
        provider_switch_head, provider_switch_entries, provider_switch_versions,
        product_catalog_heads, product_catalog_entries, product_catalog_versions,
        provider_capability_head, provider_capability_aliases,
        provider_capability_entries, provider_capability_versions,
        customer_profiles, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    bindingId = randomUUID();
    const engineAccountRecordId = randomUUID();
    const windowStart = new Date(now.getTime() - 24 * 60 * 60 * 1000);

    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Attribution Test')",
      [userId, `${userId}@test.invalid`],
    );
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 4000, 'active')
    `, [engineAccountRecordId, userId, engineAccountId]);
    await database.pool.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start,
        free_balance_nano, tier_window_start, tier_window_spent_nano
      ) VALUES ($1, 'b2c', 1, 4000, date_trunc('month', now()), 40, $2, 0)
    `, [userId, windowStart]);
    await database.pool.query(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id)
      VALUES ($1, $2, 0)
    `, [engineAccountId, userId]);

    await database.pool.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime, source_revision, observed_at
      ) VALUES (1, 1, 'capability-v1', 'pricing-attribution-test', 'test-revision', now())
    `);
    await database.pool.query(`
      INSERT INTO provider_capability_entries (
        generation, provider_id, canonical_model_id, entry_digest, capability_data
      ) VALUES (1, 'anthropic', 'claude-sonnet', 'capability-sonnet-v1', '{}'::jsonb)
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation,
        capability_digest, content_digest, actor_type, actor_id, reason
      ) VALUES (
        'main', 1, 1, 1, 'capability-v1', 'catalog-main-v1',
        'system', 'pricing-attribution-test', 'integration fixture'
      )
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_entries (
        product_id, generation, capability_generation, provider_id, canonical_model_id, enabled
      ) VALUES ('main', 1, 1, 'anthropic', 'claude-sonnet', true)
    `);
    await database.pool.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, actor_id, reason
      ) VALUES (
        1, 1, 1, 'capability-v1', 'switch-v1',
        'system', 'pricing-attribution-test', 'integration fixture'
      )
    `);
    await database.pool.query(`
      INSERT INTO provider_switch_entries (
        generation, provider_id, scope_type, product_id, segment, catalog_generation, enabled
      ) VALUES
        (1, 'anthropic', 'master', '', '', NULL, true),
        (1, 'anthropic', 'segment', 'main', 'b2c', 1, true)
    `);
    await database.pool.query(`
      INSERT INTO pricing_policies (id, owner_type, owner_id, product_id)
      VALUES ($1, 'global_b2c', 'global', 'main')
    `, [policyId]);
    await database.pool.query(`
      INSERT INTO pricing_policy_versions (
        policy_id, version, schema_version, product_id, catalog_generation,
        content_digest, actor_type, actor_id, reason
      ) VALUES (
        $1, 1, 1, 'main', 1, 'source-policy-v1',
        'system', 'pricing-attribution-test', 'integration fixture'
      )
    `, [policyId]);
    await database.pool.query(`
      INSERT INTO pricing_policy_rules (
        policy_id, policy_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 1, 'main', 1, 'track-provider', 'source-track-v1',
          'provider', 'anthropic', NULL, 'track', 'managed', NULL, NULL, true, true, true),
        ($1, 1, 'main', 1, 'static-model', 'source-static-v1',
          'model', 'anthropic', 'claude-sonnet', 'discount', 'managed', 5000, 5000,
          false, false, false)
    `, [policyId]);
    await database.pool.query(`
      INSERT INTO account_policy_bindings (
        id, user_id, engine_account_record_id, engine_account_id,
        account_class, product_id, policy_id
      ) VALUES ($1, $2, $3, $4, 'b2c', 'main', $5)
    `, [bindingId, userId, engineAccountRecordId, engineAccountId, policyId]);
    await database.pool.query(`
      INSERT INTO account_policy_versions (
        binding_id, effective_version, policy_id, policy_version, policy_digest,
        product_id, account_class, schema_version, catalog_generation,
        switch_generation, content_digest
      ) VALUES (
        $1, 1, $2, 1, 'source-policy-v1', 'main', 'b2c', 1, 1, 1,
        'effective-policy-v1'
      )
    `, [bindingId, policyId]);
    await database.pool.query(`
      INSERT INTO account_policy_rules (
        binding_id, effective_version, product_id, catalog_generation, rule_id,
        rule_digest, scope_type, provider_id, canonical_model_id, pricing_mode,
        rule_origin, discount_bps, payable_multiplier_bp, track_eligible,
        retention_eligible, commission_eligible
      ) VALUES
        ($1, 1, 'main', 1, 'track-provider', 'effective-track-v1',
          'provider', 'anthropic', NULL, 'track', 'managed', NULL, 4000, true, true, true),
        ($1, 1, 'main', 1, 'static-model', 'effective-static-v1',
          'model', 'anthropic', 'claude-sonnet', 'discount', 'managed', 5000, 5000,
          false, false, false)
    `, [bindingId]);
  });

  function policyEntry(input: {
    id: number;
    chargedNano: bigint;
    officialNano: bigint;
    rule: "track" | "static";
    allocations: Array<{ sourceType: string; amountNano: bigint; sourceRef: string }>;
    attributionOverrides?: Partial<NonNullable<EngineLedgerEntry["attribution"]>>;
  }): EngineLedgerEntry {
    const isTrack = input.rule === "track";
    let paidFundedNano = 0n;
    let bonusFundedNano = 0n;
    let otherFundedNano = 0n;
    const rawFunding = input.allocations.map((allocation, index) => {
      if (allocation.sourceType === "paid") paidFundedNano += allocation.amountNano;
      else if (allocation.sourceType === "welcome_track_bonus") {
        bonusFundedNano += allocation.amountNano;
      } else otherFundedNano += allocation.amountNano;
      return {
        bucket_id: `bucket-${input.id}-${index}`,
        source_type: allocation.sourceType,
        bucket_version: "1",
        reserved_nano: allocation.amountNano.toString(),
        charged_nano: allocation.amountNano.toString(),
        released_nano: "0",
        allocation_order: String(index),
      };
    });
    const attribution: NonNullable<EngineLedgerEntry["attribution"]> = {
      attribution_schema_version: "1",
      snapshot_kind: "policy_v1",
      provider_id: "anthropic",
      product_id: "main",
      account_class: "b2c",
      requested_model_id: "claude-sonnet-latest",
      canonical_model_id: "claude-sonnet",
      served_model_id: "claude-sonnet",
      served_canonical_model_id: "claude-sonnet",
      billing_invariant_code: null,
      alias_generation: "1",
      rule_id: isTrack ? "track-provider" : "static-model",
      rule_digest: isTrack ? "effective-track-v1" : "effective-static-v1",
      rule_scope: isTrack ? "provider" : "model",
      pricing_mode: isTrack ? "track" : "discount",
      rule_origin: "managed",
      discount_bps: isTrack ? null : 5000,
      payable_multiplier_bp: isTrack ? 4000 : 5000,
      policy_id: policyId,
      policy_version: "1",
      effective_policy_version: "1",
      policy_digest: "effective-policy-v1",
      source_policy_digest: "source-policy-v1",
      catalog_generation: "1",
      switch_generation: "1",
      admission_catalog_generation: "1",
      admission_catalog_digest: "catalog-main-v1",
      admission_switch_generation: "1",
      admission_switch_digest: "switch-v1",
      runtime_manifest_generation: "1",
      runtime_manifest_digest: "runtime-manifest-v1",
      tariff_schedule_id: "official-2026-08",
      tariff_priced_ts: String(Math.floor(now.getTime() / 1000)),
      official_nano: input.officialNano.toString(),
      official_cost_json: {
        schema_version: 1,
        provider: "anthropic",
        official_nano: input.officialNano.toString(),
      },
      paid_funded_nano: paidFundedNano.toString(),
      bonus_funded_nano: bonusFundedNano.toString(),
      other_funded_nano: otherFundedNano.toString(),
      funding_allocation_json: rawFunding,
      track_eligible: isTrack,
      retention_eligible: isTrack,
      commission_eligible: isTrack,
      snapshot_digest: `snapshot-${input.id}`,
      ...input.attributionOverrides,
    };
    return {
      id: String(input.id),
      kind: "charge",
      request_id: `request-${input.id}`,
      amount_nano: input.chargedNano.toString(),
      amount: input.chargedNano.toString(),
      key_masked: "sk-pool-…test",
      ref: null,
      balance_after_nano: null,
      ts: String(Math.floor(now.getTime() / 1000) + input.id),
      model: "claude-sonnet",
      provider: "anthropic",
      official_nano: input.officialNano.toString(),
      attribution,
      funding_allocations: input.allocations.map((allocation, index) => ({
        bucket_id: `bucket-${input.id}-${index}`,
        source_type: allocation.sourceType,
        source_ref: allocation.sourceRef,
        bucket_version: "1",
        direction: "debit",
        amount_nano: allocation.amountNano.toString(),
        allocation_order: String(index),
      })),
    };
  }

  function legacyEntry(
    id: number,
    kind: "topup" | "charge",
    amountNano: bigint,
    ref: string | null,
    provider?: string,
  ) {
    return {
      id: String(id),
      kind,
      amount_nano: amountNano.toString(),
      amount: amountNano.toString(),
      key_masked: null,
      ref,
      balance_after_nano: null,
      ts: String(Math.floor(now.getTime() / 1000) + id),
      model: null,
      provider,
    } satisfies EngineLedgerEntry;
  }

  it("persists one policy event, full lineage, normalized funding, and exact paid commission basis", async () => {
    const entry = policyEntry({
      id: 1,
      chargedNano: 100n,
      officialNano: 250n,
      rule: "track",
      allocations: [
        { sourceType: "welcome_track_bonus", amountNano: 40n, sourceRef: "welcome:test" },
        { sourceType: "paid", amountNano: 60n, sourceRef: "payment:test" },
      ],
    });

    await applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]);
    await applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]);

    const state = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_usage_events) AS events,
        (SELECT count(*)::int FROM pricing_usage_attributions) AS attributions,
        (SELECT count(*)::int FROM pricing_usage_funding_allocations) AS allocations,
        (SELECT real_funded_nano::text FROM pricing_usage_events) AS real_funded_nano,
        (SELECT free_balance_nano::text FROM customer_profiles WHERE user_id = $1) AS free_balance_nano,
        (SELECT tier_window_spent_nano::text FROM customer_profiles WHERE user_id = $1) AS window_spent_nano,
        (SELECT spent_nano::text FROM pricing_months WHERE user_id = $1) AS month_spent_nano,
        (SELECT last_ledger_id::text FROM pricing_usage_cursors WHERE user_id = $1) AS cursor
    `, [userId]);
    expect(state.rows).toEqual([{
      events: 1,
      attributions: 1,
      allocations: 2,
      real_funded_nano: "60",
      free_balance_nano: "0",
      window_spent_nano: "100",
      month_spent_nano: "100",
      cursor: "1",
    }]);

    const attribution = await database.pool.query(`
      SELECT snapshot_kind, binding_id, engine_request_id, provider_id, product_id, account_class,
             rule_id, rule_digest, pricing_mode, policy_id, policy_version::text,
             effective_policy_version::text, effective_policy_digest, policy_digest,
             source_policy_digest, catalog_generation::text, switch_generation::text,
             admission_catalog_generation::text, admission_catalog_digest,
             admission_switch_generation::text, admission_switch_digest,
             runtime_manifest_generation::text, runtime_manifest_digest,
             paid_funded_nano::text, bonus_funded_nano::text, other_funded_nano::text,
             commission_eligible, retention_eligible, snapshot_digest
      FROM pricing_usage_attributions
    `);
    expect(attribution.rows).toEqual([{
      snapshot_kind: "policy_v1",
      binding_id: bindingId,
      engine_request_id: "request-1",
      provider_id: "anthropic",
      product_id: "main",
      account_class: "b2c",
      rule_id: "track-provider",
      rule_digest: "effective-track-v1",
      pricing_mode: "track",
      policy_id: policyId,
      policy_version: "1",
      effective_policy_version: "1",
      effective_policy_digest: "effective-policy-v1",
      policy_digest: "source-policy-v1",
      source_policy_digest: "source-policy-v1",
      catalog_generation: "1",
      switch_generation: "1",
      admission_catalog_generation: "1",
      admission_catalog_digest: "catalog-main-v1",
      admission_switch_generation: "1",
      admission_switch_digest: "switch-v1",
      runtime_manifest_generation: "1",
      runtime_manifest_digest: "runtime-manifest-v1",
      paid_funded_nano: "60",
      bonus_funded_nano: "40",
      other_funded_nano: "0",
      commission_eligible: true,
      retention_eligible: true,
      snapshot_digest: "snapshot-1",
    }]);
    const allocations = await database.pool.query(`
      SELECT ordinal, engine_bucket_id, bucket_version::text, source_type, source_ref,
             amount_nano::text
      FROM pricing_usage_funding_allocations
      ORDER BY ordinal
    `);
    expect(allocations.rows).toEqual([
      {
        ordinal: 0,
        engine_bucket_id: "bucket-1-0",
        bucket_version: "1",
        source_type: "welcome_track_bonus",
        source_ref: "welcome:test",
        amount_nano: "40",
      },
      {
        ordinal: 1,
        engine_bucket_id: "bucket-1-1",
        bucket_version: "1",
        source_type: "paid",
        source_ref: "payment:test",
        amount_nano: "60",
      },
    ]);
  });

  it("stores static paid funding for audit but excludes it from commission and retention", async () => {
    await applyPricingLedgerPage(database, { userId, engineAccountId }, [policyEntry({
      id: 2,
      chargedNano: 100n,
      officialNano: 200n,
      rule: "static",
      allocations: [{ sourceType: "paid", amountNano: 100n, sourceRef: "payment:static" }],
    })]);

    const state = await database.pool.query(`
      SELECT event.amount_nano::text, event.real_funded_nano::text,
             attribution.paid_funded_nano::text, attribution.pricing_mode,
             attribution.commission_eligible, attribution.retention_eligible,
             profile.free_balance_nano::text, profile.tier_window_spent_nano::text,
             month.spent_nano::text AS month_spent_nano
      FROM pricing_usage_events event
      JOIN pricing_usage_attributions attribution ON attribution.pricing_usage_event_id = event.id
      JOIN customer_profiles profile ON profile.user_id = event.user_id
      JOIN pricing_months month ON month.user_id = event.user_id
    `);
    expect(state.rows).toEqual([{
      amount_nano: "100",
      real_funded_nano: "0",
      paid_funded_nano: "100",
      pricing_mode: "discount",
      commission_eligible: false,
      retention_eligible: false,
      free_balance_nano: "40",
      tier_window_spent_nano: "0",
      month_spent_nano: "0",
    }]);
  });

  it("rolls back the event, free projection, month, and cursor on policy graph mismatch", async () => {
    const corruptions: Array<Partial<NonNullable<EngineLedgerEntry["attribution"]>>> = [
      { attribution_schema_version: "2" },
      { source_policy_digest: "wrong-source-policy" },
      { rule_digest: "wrong-effective-rule" },
      { admission_catalog_digest: "wrong-admission-catalog" },
      { admission_switch_digest: "wrong-admission-switch" },
    ];
    for (const [index, attributionOverrides] of corruptions.entries()) {
      const entry = policyEntry({
        id: 30 + index,
        chargedNano: 100n,
        officialNano: 250n,
        rule: "track",
        allocations: [
          { sourceType: "welcome_track_bonus", amountNano: 40n, sourceRef: "welcome:test" },
          { sourceType: "paid", amountNano: 60n, sourceRef: "payment:test" },
        ],
        attributionOverrides,
      });
      await expect(applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]))
        .rejects.toBeInstanceOf(PricingLedgerAttributionError);
    }
    const state = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_usage_events) AS events,
        (SELECT count(*)::int FROM pricing_usage_attributions) AS attributions,
        (SELECT count(*)::int FROM pricing_months) AS months,
        (SELECT free_balance_nano::text FROM customer_profiles WHERE user_id = $1) AS free_balance_nano,
        (SELECT last_ledger_id::text FROM pricing_usage_cursors WHERE user_id = $1) AS cursor
    `, [userId]);
    expect(state.rows).toEqual([{
      events: 0,
      attributions: 0,
      months: 0,
      free_balance_nano: "40",
      cursor: "0",
    }]);
  });

  it("rejects funding category tampering before it can create commission", async () => {
    const entry = policyEntry({
      id: 4,
      chargedNano: 100n,
      officialNano: 250n,
      rule: "track",
      allocations: [{ sourceType: "welcome_track_bonus", amountNano: 100n, sourceRef: "welcome:test" }],
      attributionOverrides: {
        paid_funded_nano: "100",
        bonus_funded_nano: "0",
      },
    });

    await expect(applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]))
      .rejects.toThrow("funding categories do not match immutable bucket allocations");
    await expect(database.pool.query("SELECT count(*)::int AS count FROM pricing_usage_events"))
      .resolves.toMatchObject({ rows: [{ count: 0 }] });
    await expect(database.pool.query(
      "SELECT last_ledger_id::text AS cursor FROM pricing_usage_cursors WHERE user_id = $1",
      [userId],
    )).resolves.toMatchObject({ rows: [{ cursor: "0" }] });
  });

  it("keeps the unattributed legacy free-first fallback and replay idempotency", async () => {
    const entries = [
      legacyEntry(5, "topup", 50n, `promo:${randomUUID()}`),
      { ...legacyEntry(6, "charge", 100n, null), model: "gpt-5" },
    ];
    await applyPricingLedgerPage(database, { userId, engineAccountId }, entries);
    await applyPricingLedgerPage(database, { userId, engineAccountId }, entries);

    const state = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_usage_events) AS events,
        (SELECT count(*)::int FROM pricing_usage_attributions) AS attributions,
        (SELECT real_funded_nano::text FROM pricing_usage_events) AS real_funded_nano,
        (SELECT provider_id FROM pricing_usage_events) AS provider_id,
        (SELECT free_balance_nano::text FROM customer_profiles WHERE user_id = $1) AS free_balance_nano,
        (SELECT last_ledger_id::text FROM pricing_usage_cursors WHERE user_id = $1) AS cursor
    `, [userId]);
    expect(state.rows).toEqual([{
      events: 1,
      attributions: 0,
      real_funded_nano: "10",
      provider_id: "unattributed",
      free_balance_nano: "0",
      cursor: "6",
    }]);
  });

  it("stores and restores exact Claude, GPT, and Gemini provider evidence", async () => {
    const entries = [
      legacyEntry(10, "charge", 10n, null, "anthropic"),
      legacyEntry(11, "charge", 20n, null, "openai"),
      legacyEntry(12, "charge", 30n, null, "google"),
    ];
    await applyPricingLedgerPage(database, { userId, engineAccountId }, entries);
    await expect(database.pool.query(`
      SELECT ledger_entry_id::text, provider_id
      FROM pricing_usage_events
      ORDER BY ledger_entry_id
    `)).resolves.toMatchObject({ rows: [
      { ledger_entry_id: "10", provider_id: "anthropic" },
      { ledger_entry_id: "11", provider_id: "openai" },
      { ledger_entry_id: "12", provider_id: "google" },
    ] });

    await database.pool.query("UPDATE pricing_usage_events SET provider_id = NULL");
    await expect(getPricingProviderBackfillCursor(
      database,
      { userId, engineAccountId },
      12n,
    )).resolves.toBe(9n);
    await expect(applyPricingProviderBackfillPage(
      database,
      { userId, engineAccountId },
      [{ ...entries[0]!, amount_nano: "11" }],
    )).rejects.toThrow("provider backfill amount differs");
    await expect(applyPricingProviderBackfillPage(
      database,
      { userId, engineAccountId },
      entries,
    )).resolves.toBe(3);
    await expect(completePricingProviderBackfill(
      database,
      { userId, engineAccountId },
      12n,
    )).resolves.toBe(0);
    await expect(database.pool.query(`
      SELECT ledger_entry_id::text, provider_id
      FROM pricing_usage_events
      ORDER BY ledger_entry_id
    `)).resolves.toMatchObject({ rows: [
      { ledger_entry_id: "10", provider_id: "anthropic" },
      { ledger_entry_id: "11", provider_id: "openai" },
      { ledger_entry_id: "12", provider_id: "google" },
    ] });
  });
});
