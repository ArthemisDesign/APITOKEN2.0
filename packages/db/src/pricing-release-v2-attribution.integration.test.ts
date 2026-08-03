import { randomUUID } from "node:crypto";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  applyPricingLedgerPage,
  PricingLedgerAttributionError,
} from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("release-v2 ledger attribution ingest", () => {
  let database: Database;
  let userId: string;
  const engineAccountId = "acct_release_v2_attribution";
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
        customer_profiles, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    const windowStart = new Date(now.getTime() - 24 * 60 * 60 * 1000);
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Release V2 Test')",
      [userId, `${userId}@test.invalid`],
    );
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, $3, 5000, 'active')
    `, [randomUUID(), userId, engineAccountId]);
    await database.pool.query(`
      INSERT INTO customer_profiles (
        user_id, customer_type, current_tier, multiplier_bp, pricing_month_start,
        free_balance_nano, tier_window_start, tier_window_spent_nano
      ) VALUES ($1, 'b2c', 1, 5000, date_trunc('month', now()), 40, $2, 0)
    `, [userId, windowStart]);
    await database.pool.query(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, last_ledger_id)
      VALUES ($1, $2, 0)
    `, [engineAccountId, userId]);
  });

  function releaseV2Entry(input: {
    id: number;
    chargedNano: bigint;
    officialNano: bigint;
    accountClass?: "b2c" | "b2b" | "openkeys" | "service";
    billingMode?: "balance" | "meter_only";
    withRule?: boolean;
    allocations: Array<{ lotSourceType: string; amountNano: bigint }>;
    attributionOverrides?: Partial<NonNullable<EngineLedgerEntry["attribution"]>>;
  }): EngineLedgerEntry {
    const accountClass = input.accountClass ?? "b2c";
    const billingMode = input.billingMode ?? "balance";
    let paidFundedNano = 0n;
    let bonusFundedNano = 0n;
    let otherFundedNano = 0n;
    const rawFunding = input.allocations.map((allocation, index) => {
      if (allocation.lotSourceType === "paid") paidFundedNano += allocation.amountNano;
      else if (allocation.lotSourceType === "welcome_bonus") bonusFundedNano += allocation.amountNano;
      else otherFundedNano += allocation.amountNano;
      return {
        allocation_order: String(index),
        lot_id: `lot-${input.id}-${index}`,
        lot_source_type: allocation.lotSourceType,
        lot_version: "1",
        direction: "debit" as const,
        amount_nano: allocation.amountNano.toString(),
      };
    });
    const attribution: NonNullable<EngineLedgerEntry["attribution"]> = {
      attribution_schema_version: "2",
      snapshot_kind: "release_v2",
      provider_id: "anthropic",
      product_id: null,
      account_class: accountClass,
      requested_model_id: "claude-sonnet-latest",
      canonical_model_id: "claude-sonnet",
      served_model_id: "claude-sonnet",
      served_canonical_model_id: "claude-sonnet",
      billing_invariant_code: null,
      alias_generation: null,
      rule_id: input.withRule ? "rule-global-50" : null,
      rule_digest: input.withRule ? "rule-global-50-digest" : null,
      rule_scope: input.withRule ? "global" : null,
      pricing_mode: null,
      rule_origin: null,
      discount_bps: input.withRule ? 5000 : null,
      payable_multiplier_bp: input.withRule ? 5000 : null,
      policy_id: "policy-release-v2",
      policy_version: "7",
      effective_policy_version: null,
      policy_digest: "release-policy-digest-v7",
      source_policy_digest: null,
      catalog_generation: null,
      switch_generation: null,
      admission_catalog_generation: null,
      admission_catalog_digest: null,
      admission_switch_generation: null,
      admission_switch_digest: null,
      runtime_manifest_generation: null,
      runtime_manifest_digest: null,
      tariff_schedule_id: "official-2026-08",
      tariff_priced_ts: String(Math.floor(now.getTime() / 1000)),
      official_nano: input.officialNano.toString(),
      official_cost_json: {
        schema_version: 2,
        provider: "anthropic",
        official_nano: input.officialNano.toString(),
      },
      paid_funded_nano: paidFundedNano.toString(),
      bonus_funded_nano: bonusFundedNano.toString(),
      other_funded_nano: otherFundedNano.toString(),
      funding_allocation_json: rawFunding,
      track_eligible: null,
      retention_eligible: null,
      commission_eligible: null,
      snapshot_digest: `release-snapshot-${input.id}`,
      release_schema_version: "2",
      release_generation: "3",
      release_digest: "release-digest-g3",
      release_billing_mode: billingMode,
      release_funding_generation: billingMode === "balance" ? "5" : null,
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
    };
  }

  it("persists full release-v2 lineage, exact split, and derived commission without tier records", async () => {
    const entry = releaseV2Entry({
      id: 1,
      chargedNano: 100n,
      officialNano: 250n,
      withRule: true,
      allocations: [
        { lotSourceType: "welcome_bonus", amountNano: 40n },
        { lotSourceType: "paid", amountNano: 60n },
      ],
    });

    await applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]);
    await applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]);

    const state = await database.pool.query(`
      SELECT
        (SELECT count(*)::int FROM pricing_usage_events) AS events,
        (SELECT count(*)::int FROM pricing_usage_attributions) AS attributions,
        (SELECT count(*)::int FROM pricing_usage_funding_allocations) AS allocations,
        (SELECT count(*)::int FROM pricing_months) AS months,
        (SELECT real_funded_nano::text FROM pricing_usage_events) AS real_funded_nano,
        (SELECT free_balance_nano::text FROM customer_profiles WHERE user_id = $1) AS free_balance_nano,
        (SELECT tier_window_spent_nano::text FROM customer_profiles WHERE user_id = $1) AS window_spent_nano,
        (SELECT last_ledger_id::text FROM pricing_usage_cursors WHERE user_id = $1) AS cursor
    `, [userId]);
    expect(state.rows).toEqual([{
      events: 1,
      attributions: 1,
      allocations: 0,
      months: 0,
      real_funded_nano: "60",
      free_balance_nano: "40",
      window_spent_nano: "0",
      cursor: "1",
    }]);

    const attribution = await database.pool.query(`
      SELECT attribution_schema_version::text, snapshot_kind, binding_id, engine_request_id,
             provider_id, product_id, account_class,
             rule_id, rule_digest, rule_scope, pricing_mode, rule_origin,
             discount_bps, payable_multiplier_bp,
             policy_id, policy_version::text, effective_policy_version::text,
             effective_policy_digest, policy_digest, source_policy_digest,
             tariff_schedule_id, tariff_priced_at IS NOT NULL AS tariff_priced,
             official_nano::text, charged_nano::text, official_cost_json IS NOT NULL AS official_cost,
             paid_funded_nano::text, bonus_funded_nano::text, other_funded_nano::text,
             funding_allocation_json, track_eligible, retention_eligible, commission_eligible,
             snapshot_digest,
             release_schema_version::text, release_generation::text, release_digest,
             release_billing_mode, release_funding_generation::text
      FROM pricing_usage_attributions
    `);
    expect(attribution.rows).toEqual([{
      attribution_schema_version: "2",
      snapshot_kind: "release_v2",
      binding_id: null,
      engine_request_id: "request-1",
      provider_id: "anthropic",
      product_id: null,
      account_class: "b2c",
      rule_id: "rule-global-50",
      rule_digest: "rule-global-50-digest",
      rule_scope: "global",
      pricing_mode: null,
      rule_origin: null,
      discount_bps: 5000,
      payable_multiplier_bp: 5000,
      policy_id: "policy-release-v2",
      policy_version: "7",
      effective_policy_version: null,
      effective_policy_digest: null,
      policy_digest: "release-policy-digest-v7",
      source_policy_digest: null,
      tariff_schedule_id: "official-2026-08",
      tariff_priced: true,
      official_nano: "250",
      charged_nano: "100",
      official_cost: true,
      paid_funded_nano: "60",
      bonus_funded_nano: "40",
      other_funded_nano: "0",
      funding_allocation_json: [
        {
          allocation_order: "0",
          lot_id: "lot-1-0",
          lot_source_type: "welcome_bonus",
          lot_version: "1",
          direction: "debit",
          amount_nano: "40",
        },
        {
          allocation_order: "1",
          lot_id: "lot-1-1",
          lot_source_type: "paid",
          lot_version: "1",
          direction: "debit",
          amount_nano: "60",
        },
      ],
      track_eligible: false,
      retention_eligible: false,
      commission_eligible: true,
      snapshot_digest: "release-snapshot-1",
      release_schema_version: "2",
      release_generation: "3",
      release_digest: "release-digest-g3",
      release_billing_mode: "balance",
      release_funding_generation: "5",
    }]);
  });

  it("derives commission only from b2c class with positive paid funding", async () => {
    const bonusOnly = releaseV2Entry({
      id: 2,
      chargedNano: 100n,
      officialNano: 200n,
      allocations: [{ lotSourceType: "welcome_bonus", amountNano: 100n }],
    });
    const b2bPaid = releaseV2Entry({
      id: 3,
      chargedNano: 80n,
      officialNano: 160n,
      accountClass: "b2b",
      allocations: [{ lotSourceType: "paid", amountNano: 80n }],
    });
    await applyPricingLedgerPage(database, { userId, engineAccountId }, [bonusOnly, b2bPaid]);

    const rows = await database.pool.query(`
      SELECT attribution.account_class, attribution.paid_funded_nano::text,
             attribution.commission_eligible, event.real_funded_nano::text
      FROM pricing_usage_events event
      JOIN pricing_usage_attributions attribution ON attribution.pricing_usage_event_id = event.id
      ORDER BY event.ledger_entry_id
    `);
    expect(rows.rows).toEqual([
      {
        account_class: "b2c",
        paid_funded_nano: "0",
        commission_eligible: false,
        real_funded_nano: "0",
      },
      {
        account_class: "b2b",
        paid_funded_nano: "80",
        commission_eligible: false,
        real_funded_nano: "0",
      },
    ]);
  });

  it("rejects progressive-field and funding tampering with a full rollback", async () => {
    const tampering: Array<Partial<NonNullable<EngineLedgerEntry["attribution"]>>> = [
      { attribution_schema_version: "1" },
      { pricing_mode: "track" },
      { rule_origin: "managed" },
      { track_eligible: true },
      { retention_eligible: false },
      { commission_eligible: true },
      { paid_funded_nano: "100", bonus_funded_nano: "0" },
      { rule_id: null, rule_digest: "rule-global-50-digest" },
      { release_billing_mode: "meter_only", release_funding_generation: "5" },
      { release_funding_generation: null },
    ];
    for (const [index, attributionOverrides] of tampering.entries()) {
      const entry = releaseV2Entry({
        id: 30 + index,
        chargedNano: 100n,
        officialNano: 250n,
        withRule: true,
        allocations: [
          { lotSourceType: "welcome_bonus", amountNano: 40n },
          { lotSourceType: "paid", amountNano: 60n },
        ],
        attributionOverrides,
      });
      await expect(applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]))
        .rejects.toBeInstanceOf(PricingLedgerAttributionError);
    }
    // Allocation-order regression and a split that does not cover the charge fail the same way.
    const disordered = releaseV2Entry({
      id: 50,
      chargedNano: 100n,
      officialNano: 250n,
      allocations: [
        { lotSourceType: "welcome_bonus", amountNano: 40n },
        { lotSourceType: "paid", amountNano: 60n },
      ],
    });
    disordered.attribution!.funding_allocation_json = [
      {
        allocation_order: "1",
        lot_id: "lot-50-0",
        lot_source_type: "welcome_bonus",
        lot_version: "1",
        direction: "debit",
        amount_nano: "40",
      },
      {
        allocation_order: "1",
        lot_id: "lot-50-1",
        lot_source_type: "paid",
        lot_version: "1",
        direction: "debit",
        amount_nano: "60",
      },
    ];
    await expect(applyPricingLedgerPage(database, { userId, engineAccountId }, [disordered]))
      .rejects.toThrow("raw funding allocation order is not strictly increasing");

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

  it("accepts a meter-only snapshot without a funding generation", async () => {
    const entry = releaseV2Entry({
      id: 4,
      chargedNano: 50n,
      officialNano: 50n,
      accountClass: "service",
      billingMode: "meter_only",
      allocations: [{ lotSourceType: "service_credit", amountNano: 50n }],
    });
    await applyPricingLedgerPage(database, { userId, engineAccountId }, [entry]);

    const rows = await database.pool.query(`
      SELECT release_billing_mode, release_funding_generation::text, commission_eligible,
             other_funded_nano::text
      FROM pricing_usage_attributions
    `);
    expect(rows.rows).toEqual([{
      release_billing_mode: "meter_only",
      release_funding_generation: null,
      commission_eligible: false,
      other_funded_nano: "50",
    }]);
  });
});
