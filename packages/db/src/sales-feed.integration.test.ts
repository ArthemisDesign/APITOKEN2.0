import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import { listPaidTopupsAfter, listUsageEventsAfter } from "./sales-feed.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("referral-only sales feeds", () => {
  let database: Database;
  const policyId = "sales-feed-policy";

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE
        pricing_usage_attributions, pricing_usage_events,
        account_policy_versions, account_policy_bindings,
        pricing_policy_versions, pricing_policies,
        provider_switch_versions, product_catalog_versions, provider_capability_versions,
        referral_attributions, payments, checkout_sessions, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  async function insertUser(referred: boolean): Promise<string> {
    const userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Sales Feed Test')",
      [userId, `${userId}@test.invalid`],
    );
    if (referred) {
      await database.pool.query(
        "INSERT INTO referral_attributions (user_id, code, created_at) VALUES ($1, 'partner-code', now() - interval '1 minute')",
        [userId],
      );
    }
    return userId;
  }

  async function insertUsage(
    userId: string,
    ledgerEntryId: number,
    occurredAt: Date,
    realFundedNano = 750n,
  ): Promise<string> {
    const id = randomUUID();
    await database.pool.query(`
      INSERT INTO pricing_usage_events
        (id, user_id, engine_account_id, ledger_entry_id, amount_nano, real_funded_nano, occurred_at, created_at)
      VALUES ($1, $2, $3, $4, 1000, $5, $6, now() - interval '1 minute')
    `, [id, userId, `acct-${ledgerEntryId}`, ledgerEntryId, realFundedNano.toString(), occurredAt]);
    return id;
  }

  async function seedPolicyBinding(userId: string): Promise<string> {
    const engineAccountRecordId = randomUUID();
    const bindingId = randomUUID();
    await database.pool.query(`
      INSERT INTO engine_accounts (id, user_id, engine_account_id, mult_bp, status)
      VALUES ($1, $2, 'acct-sales-feed-policy', 4000, 'active')
    `, [engineAccountRecordId, userId]);
    await database.pool.query(`
      INSERT INTO provider_capability_versions (
        generation, schema_version, content_digest, source_runtime, source_revision, observed_at
      ) VALUES (1, 1, 'sales-feed-capability-v1', 'sales-feed-test', 'fixture', now())
    `);
    await database.pool.query(`
      INSERT INTO product_catalog_versions (
        product_id, generation, schema_version, capability_generation,
        capability_digest, content_digest, actor_type, actor_id, reason
      ) VALUES (
        'main', 1, 1, 1, 'sales-feed-capability-v1', 'sales-feed-catalog-v1',
        'system', 'sales-feed-test', 'integration fixture'
      )
    `);
    await database.pool.query(`
      INSERT INTO provider_switch_versions (
        generation, schema_version, capability_generation, capability_digest,
        content_digest, actor_type, actor_id, reason
      ) VALUES (
        1, 1, 1, 'sales-feed-capability-v1', 'sales-feed-switch-v1',
        'system', 'sales-feed-test', 'integration fixture'
      )
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
        $1, 1, 1, 'main', 1, 'sales-feed-source-policy-v1',
        'system', 'sales-feed-test', 'integration fixture'
      )
    `, [policyId]);
    await database.pool.query(`
      INSERT INTO account_policy_bindings (
        id, user_id, engine_account_record_id, engine_account_id,
        account_class, product_id, policy_id
      ) VALUES ($1, $2, $3, 'acct-sales-feed-policy', 'b2c', 'main', $4)
    `, [bindingId, userId, engineAccountRecordId, policyId]);
    await database.pool.query(`
      INSERT INTO account_policy_versions (
        binding_id, effective_version, policy_id, policy_version, policy_digest,
        product_id, account_class, schema_version, catalog_generation,
        switch_generation, content_digest
      ) VALUES (
        $1, 1, $2, 1, 'sales-feed-source-policy-v1',
        'main', 'b2c', 1, 1, 1, 'sales-feed-effective-policy-v1'
      )
    `, [bindingId, policyId]);
    return bindingId;
  }

  async function insertPolicyAttribution(input: {
    eventId: string;
    bindingId: string;
    accountClass: "b2c" | "service";
    pricingMode: "track" | "discount";
    paidFundedNano: bigint;
    commissionEligible: boolean;
    snapshotDigest: string;
    schemaVersion?: bigint;
  }): Promise<void> {
    const track = input.pricingMode === "track";
    await database.pool.query(`
      INSERT INTO pricing_usage_attributions (
        pricing_usage_event_id, attribution_schema_version, snapshot_kind,
        engine_request_id, provider_id, product_id, account_class, binding_id,
        requested_model_id, canonical_model_id, alias_generation,
        rule_id, rule_digest, rule_scope, pricing_mode, rule_origin,
        discount_bps, payable_multiplier_bp, policy_id, policy_version,
        effective_policy_version, effective_policy_digest, policy_digest, source_policy_digest,
        catalog_generation, switch_generation, tariff_schedule_id, tariff_priced_at,
        official_nano, charged_nano, official_cost_json, paid_funded_nano, bonus_funded_nano,
        other_funded_nano, funding_allocation_json, track_eligible,
        retention_eligible, commission_eligible, snapshot_digest
      ) VALUES (
        $1, $2, 'policy_v1', $3, 'anthropic', 'main', $4, $5,
        'claude-sonnet-latest', 'claude-sonnet', 1,
        $6, $7, 'provider', $8, 'managed',
        $9, $10, $11, 1,
        1, 'sales-feed-effective-policy-v1', 'sales-feed-source-policy-v1',
        'sales-feed-source-policy-v1', 1, 1, 'sales-feed-tariff-v1', now(),
        1000, 1000, '{}'::jsonb, $12, $13, 0, '[]'::jsonb, $14, $14, $15, $16
      )
    `, [
      input.eventId,
      (input.schemaVersion ?? 1n).toString(),
      `request-${input.snapshotDigest}`,
      input.accountClass,
      input.bindingId,
      track ? "track-provider" : "static-provider",
      track ? "track-digest" : "static-digest",
      input.pricingMode,
      track ? null : 5000,
      track ? 4000 : 5000,
      policyId,
      input.paidFundedNano.toString(),
      (1000n - input.paidFundedNano).toString(),
      track,
      input.commissionEligible,
      input.snapshotDigest,
    ]);
  }

  async function insertReleaseV2Attribution(input: {
    eventId: string;
    accountClass: "b2c" | "b2b" | "openkeys" | "service";
    paidFundedNano: bigint;
    bonusFundedNano: bigint;
    otherFundedNano: bigint;
    commissionEligible: boolean;
    snapshotDigest: string;
    schemaVersion?: bigint;
  }): Promise<void> {
    await database.pool.query(`
      INSERT INTO pricing_usage_attributions (
        pricing_usage_event_id, attribution_schema_version, snapshot_kind,
        engine_request_id, provider_id, product_id, account_class, binding_id,
        requested_model_id, canonical_model_id,
        rule_id, rule_digest, rule_scope, pricing_mode, rule_origin,
        discount_bps, payable_multiplier_bp, policy_id, policy_version,
        effective_policy_version, effective_policy_digest, policy_digest, source_policy_digest,
        tariff_schedule_id, tariff_priced_at,
        official_nano, charged_nano, official_cost_json, paid_funded_nano, bonus_funded_nano,
        other_funded_nano, funding_allocation_json, track_eligible,
        retention_eligible, commission_eligible, snapshot_digest,
        release_schema_version, release_generation, release_digest,
        release_billing_mode, release_funding_generation
      ) VALUES (
        $1, $2, 'release_v2', $3, 'anthropic', NULL, $4, NULL,
        'claude-sonnet-latest', 'claude-sonnet',
        NULL, NULL, NULL, NULL, NULL,
        NULL, NULL, 'sales-feed-release-policy', 7,
        NULL, NULL, 'sales-feed-release-policy-digest', NULL,
        'sales-feed-tariff-v1', now(),
        1000, 1000, '{}'::jsonb, $5, $6, $7, '[]'::jsonb, false, false, $8, $9,
        2, 3, 'sales-feed-release-digest', 'balance', 5
      )
    `, [
      input.eventId,
      (input.schemaVersion ?? 2n).toString(),
      `request-${input.snapshotDigest}`,
      input.accountClass,
      input.paidFundedNano.toString(),
      input.bonusFundedNano.toString(),
      input.otherFundedNano.toString(),
      input.commissionEligible,
      input.snapshotDigest,
    ]);
  }

  async function insertPaidTopup(userId: string, suffix: string, paidAt: Date): Promise<string> {
    const checkoutId = randomUUID();
    const paymentId = randomUUID();
    await database.pool.query(`
      INSERT INTO checkout_sessions
        (id, user_id, engine_account_id, provider, amount_usd, amount_nano, status, created_at)
      VALUES ($1, $2, $3, 'test', 1, 1000000000, 'paid', now() - interval '1 minute')
    `, [checkoutId, userId, `acct-${suffix}`]);
    await database.pool.query(`
      INSERT INTO payments
        (id, checkout_id, user_id, provider, provider_payment_id, amount_minor, currency,
         amount_nano, status, paid_at, created_at)
      VALUES ($1, $2, $3, 'test', $4, 100, 'USD', 1000000000, 'paid', $5, now() - interval '1 minute')
    `, [paymentId, checkoutId, userId, `payment-${suffix}`, paidAt]);
    return paymentId;
  }

  it("excludes ordinary customer spend before and after referred spend", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const occurredAt = new Date(Date.now() - 60_000);

    await insertUsage(ordinaryBefore, 1, occurredAt);
    await insertUsage(referred, 2, occurredAt);
    await insertUsage(ordinaryAfter, 3, occurredAt);

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.items).toHaveLength(1);
    expect(page.items[0]).toMatchObject({
      userId: referred,
      amountNano: 750n,
      providerId: null,
      accountClass: null,
      pricingMode: null,
      paidFundedNano: null,
      commissionEligible: null,
      snapshotDigest: null,
    });
    expect(page.nextCursor).toBe(3n);

    const firstPage = await listUsageEventsAfter(database, 0n, 1);
    expect(firstPage.items).toEqual([]);
    expect(firstPage.nextCursor).toBe(1n);
    const secondPage = await listUsageEventsAfter(database, firstPage.nextCursor, 1);
    expect(secondPage.items).toHaveLength(1);
    expect(secondPage.items[0]).toMatchObject({ userId: referred, amountNano: 750n });
  });

  it("emits only exact policy B2C track paid funding while advancing through every source row", async () => {
    const referred = await insertUser(true);
    const ordinary = await insertUser(false);
    const bindingId = await seedPolicyBinding(referred);
    const occurredAt = new Date(Date.now() - 60_000);

    const staticEvent = await insertUsage(referred, 10, occurredAt, 900n);
    await insertPolicyAttribution({
      eventId: staticEvent,
      bindingId,
      accountClass: "b2c",
      pricingMode: "discount",
      paidFundedNano: 900n,
      commissionEligible: false,
      snapshotDigest: "snapshot-static",
    });
    const serviceEvent = await insertUsage(referred, 11, occurredAt, 800n);
    await insertPolicyAttribution({
      eventId: serviceEvent,
      bindingId,
      accountClass: "service",
      pricingMode: "track",
      paidFundedNano: 800n,
      commissionEligible: true,
      snapshotDigest: "snapshot-service",
    });
    const unreferredEvent = await insertUsage(ordinary, 12, occurredAt, 700n);
    await insertPolicyAttribution({
      eventId: unreferredEvent,
      bindingId,
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: 700n,
      commissionEligible: true,
      snapshotDigest: "snapshot-unreferred",
    });
    const unknownSchemaEvent = await insertUsage(referred, 13, occurredAt, 650n);
    await insertPolicyAttribution({
      eventId: unknownSchemaEvent,
      bindingId,
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: 650n,
      commissionEligible: true,
      snapshotDigest: "snapshot-schema-2",
      schemaVersion: 2n,
    });
    const eligibleEvent = await insertUsage(referred, 14, occurredAt, 123n);
    await insertPolicyAttribution({
      eventId: eligibleEvent,
      bindingId,
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: 600n,
      commissionEligible: true,
      snapshotDigest: "snapshot-eligible",
    });

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.nextCursor).toBe(5n);
    expect(page.items).toEqual([{
      id: 5n,
      userId: referred,
      amountNano: 600n,
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: 600n,
      commissionEligible: true,
      snapshotDigest: "snapshot-eligible",
      occurredAt,
    }]);

    let cursor = 0n;
    for (const expectedCursor of [1n, 2n, 3n, 4n]) {
      const filteredPage = await listUsageEventsAfter(database, cursor, 1);
      expect(filteredPage.items).toEqual([]);
      expect(filteredPage.nextCursor).toBe(expectedCursor);
      cursor = filteredPage.nextCursor;
    }
    const eligiblePage = await listUsageEventsAfter(database, cursor, 1);
    expect(eligiblePage.nextCursor).toBe(5n);
    expect(eligiblePage.items[0]).toMatchObject({
      amountNano: 600n,
      paidFundedNano: 600n,
      snapshotDigest: "snapshot-eligible",
    });
  });

  it("emits release-v2 referred B2C paid funding without a pricing mode", async () => {
    const referred = await insertUser(true);
    const ordinary = await insertUser(false);
    const occurredAt = new Date(Date.now() - 60_000);

    const bonusOnlyEvent = await insertUsage(referred, 20, occurredAt, 0n);
    await insertReleaseV2Attribution({
      eventId: bonusOnlyEvent,
      accountClass: "b2c",
      paidFundedNano: 0n,
      bonusFundedNano: 1000n,
      otherFundedNano: 0n,
      commissionEligible: false,
      snapshotDigest: "release-bonus-only",
    });
    const b2bEvent = await insertUsage(referred, 21, occurredAt, 0n);
    await insertReleaseV2Attribution({
      eventId: b2bEvent,
      accountClass: "b2b",
      paidFundedNano: 1000n,
      bonusFundedNano: 0n,
      otherFundedNano: 0n,
      commissionEligible: false,
      snapshotDigest: "release-b2b",
    });
    const unreferredEvent = await insertUsage(ordinary, 22, occurredAt, 0n);
    await insertReleaseV2Attribution({
      eventId: unreferredEvent,
      accountClass: "b2c",
      paidFundedNano: 1000n,
      bonusFundedNano: 0n,
      otherFundedNano: 0n,
      commissionEligible: true,
      snapshotDigest: "release-unreferred",
    });
    const eligibleEvent = await insertUsage(referred, 23, occurredAt, 0n);
    await insertReleaseV2Attribution({
      eventId: eligibleEvent,
      accountClass: "b2c",
      paidFundedNano: 650n,
      bonusFundedNano: 300n,
      otherFundedNano: 50n,
      commissionEligible: true,
      snapshotDigest: "release-eligible",
    });

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.nextCursor).toBe(4n);
    expect(page.items).toEqual([{
      id: 4n,
      userId: referred,
      amountNano: 650n,
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: null,
      paidFundedNano: 650n,
      commissionEligible: true,
      snapshotDigest: "release-eligible",
      officialNano: 1000n,
      chargedNano: 1000n,
      bonusFundedNano: 300n,
      otherFundedNano: 50n,
      releaseGeneration: 3n,
      releaseDigest: "sales-feed-release-digest",
      occurredAt,
    }]);

    let cursor = 0n;
    for (const expectedCursor of [1n, 2n, 3n]) {
      const filteredPage = await listUsageEventsAfter(database, cursor, 1);
      expect(filteredPage.items).toEqual([]);
      expect(filteredPage.nextCursor).toBe(expectedCursor);
      cursor = filteredPage.nextCursor;
    }
    const eligiblePage = await listUsageEventsAfter(database, cursor, 1);
    expect(eligiblePage.items).toHaveLength(1);
    expect(eligiblePage.items[0]).toMatchObject({
      amountNano: 650n,
      pricingMode: null,
      releaseDigest: "sales-feed-release-digest",
    });
  });

  it("keeps legacy rows free of the release-v2 fields", async () => {
    const referred = await insertUser(true);
    const occurredAt = new Date(Date.now() - 60_000);
    await insertUsage(referred, 30, occurredAt, 750n);

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.items).toHaveLength(1);
    expect(page.items[0]).toMatchObject({
      userId: referred,
      amountNano: 750n,
      pricingMode: null,
    });
    expect("officialNano" in page.items[0]!).toBe(false);
    expect("releaseDigest" in page.items[0]!).toBe(false);
  });

  it("excludes ordinary customer top-ups while preserving referred top-ups", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const base = Date.now() - 120_000;

    await insertPaidTopup(ordinaryBefore, "ordinary-before", new Date(base));
    const referredPaymentId = await insertPaidTopup(referred, "referred", new Date(base + 1_000));
    await insertPaidTopup(ordinaryAfter, "ordinary-after", new Date(base + 2_000));

    const page = await listPaidTopupsAfter(database, 0n, 100);
    expect(page.items).toHaveLength(1);
    expect(page.items[0]).toMatchObject({ userId: referred, paymentId: referredPaymentId, amountNano: 1_000_000_000n });
    expect(page.nextCursor).toBe(BigInt((base + 2_000) * 1_000));

    const firstPage = await listPaidTopupsAfter(database, 0n, 1);
    expect(firstPage.items).toEqual([]);
    expect(firstPage.nextCursor).toBe(BigInt(base * 1_000));
    const secondPage = await listPaidTopupsAfter(database, firstPage.nextCursor, 1);
    expect(secondPage.items).toHaveLength(1);
    expect(secondPage.items[0]).toMatchObject({ userId: referred, paymentId: referredPaymentId });
  });
});
