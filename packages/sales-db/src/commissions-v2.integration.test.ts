import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  assertReferredSpendV2Shape,
  isCommissionableSpendV2,
  pendingUsageV2Ref,
  recordReferredSpendV2,
  reconcilePendingReferralUsageEventsV2,
  ReferredSpendV2ShapeError,
  type ReferredSpendV2Event,
} from "./commissions-v2.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("recordReferredSpendV2 (release-v2 writer)", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(truncate);

  async function truncate(): Promise<void> {
    await db.pool.query(`
      TRUNCATE commission_entries_v2, partner_usage_events_v2,
        pending_referral_usage_events_v2, referred_users, partners RESTART IDENTITY CASCADE
    `);
  }

  async function seedChain(): Promise<{ direct: string; parent: string }> {
    const parent = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code,status,commission_bps,sub_commission_bps)
      VALUES('v2w-parent','active',1000,500)
      RETURNING id
    `);
    const direct = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(
        referral_code,status,parent_partner_id,commission_bps,sub_commission_bps
      ) VALUES('v2w-direct','active',$1,1000,1000)
      RETURNING id
    `, [parent.rows[0]!.id]);
    return { direct: direct.rows[0]!.id, parent: parent.rows[0]!.id };
  }

  async function attribute(commerceUserId: string, partnerId: string): Promise<void> {
    await db.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, referral_code)
      VALUES($1, $2, 'v2w-direct')
    `, [commerceUserId, partnerId]);
  }

  function event(overrides: Partial<ReferredSpendV2Event> = {}): ReferredSpendV2Event {
    return {
      commerceEventId: 7001n,
      commerceUserId: randomUUID(),
      providerId: "google",
      accountClass: "b2c",
      officialNano: 20_000n,
      chargedNano: 12_000n,
      paidFundedNano: 10_000n,
      bonusFundedNano: 2_000n,
      otherFundedNano: 0n,
      commissionEligible: true,
      releaseGeneration: 7n,
      releaseDigest: "release-v2",
      snapshotDigest: "snapshot-v2",
      occurredAt: new Date("2026-08-01T12:00:00.000Z"),
      ...overrides,
    };
  }

  it("records the usage event and the exact paid-funded commission chain atomically", async () => {
    const chain = await seedChain();
    const input = event();
    await attribute(input.commerceUserId, chain.direct);

    await expect(recordReferredSpendV2(db, input)).resolves.toBe("recorded");

    const usage = await db.pool.query<{
      partner_id: string; paid_funded_nano: string; bonus_funded_nano: string;
      commission_eligible: boolean; release_generation: string;
    }>(`
      SELECT partner_id, paid_funded_nano::text, bonus_funded_nano::text,
             commission_eligible, release_generation::text
      FROM partner_usage_events_v2 WHERE commerce_event_id = 7001
    `);
    expect(usage.rows).toEqual([{
      partner_id: chain.direct,
      paid_funded_nano: "10000",
      bonus_funded_nano: "2000",
      commission_eligible: true,
      release_generation: "7",
    }]);

    const entries = await db.pool.query<{
      partner_id: string; level: number; applied_bps: number;
      base_paid_funded_nano: string; amount_nano: string;
    }>(`
      SELECT partner_id, level, applied_bps, base_paid_funded_nano::text, amount_nano::text
      FROM commission_entries_v2 ORDER BY level
    `);
    // level 0 = 10000 * 1000/10000 = 1000; level 1 = 1000 * 500/10000 = 50.
    // Bonus-funded 2000 никогда не входит в basis.
    expect(entries.rows).toEqual([
      { partner_id: chain.direct, level: 0, applied_bps: 1000, base_paid_funded_nano: "10000", amount_nano: "1000" },
      { partner_id: chain.parent, level: 1, applied_bps: 500, base_paid_funded_nano: "10000", amount_nano: "50" },
    ]);
  });

  it("is idempotent: an exact replay returns duplicate without a second record", async () => {
    const chain = await seedChain();
    const input = event();
    await attribute(input.commerceUserId, chain.direct);

    await expect(recordReferredSpendV2(db, input)).resolves.toBe("recorded");
    await expect(recordReferredSpendV2(db, input)).resolves.toBe("duplicate");

    const counts = await db.pool.query<{ usage: string; entries: string }>(`
      SELECT (SELECT count(*) FROM partner_usage_events_v2)::text AS usage,
             (SELECT count(*) FROM commission_entries_v2)::text AS entries
    `);
    expect(counts.rows[0]).toEqual({ usage: "1", entries: "2" });
  });

  it("buffers a pre-attribution event and reconcile replays it after attribution", async () => {
    const chain = await seedChain();
    const input = event({ commerceEventId: 7002n });

    await expect(recordReferredSpendV2(db, input)).resolves.toBe("buffered");
    const pending = await db.pool.query<{ commerce_ref: string }>(`
      SELECT commerce_ref FROM pending_referral_usage_events_v2 WHERE commerce_event_id = 7002
    `);
    expect(pending.rows).toEqual([{ commerce_ref: pendingUsageV2Ref(7002n) }]);

    // Повторная буферизация того же события — идемпотентный no-op.
    await expect(recordReferredSpendV2(db, input)).resolves.toBe("buffered");
    // Reconcile без атрибуции ничего не трогает.
    await expect(reconcilePendingReferralUsageEventsV2(db)).resolves.toBe(0);

    await attribute(input.commerceUserId, chain.direct);
    await expect(reconcilePendingReferralUsageEventsV2(db)).resolves.toBe(1);

    const after = await db.pool.query<{ pending: string; usage: string; entries: string }>(`
      SELECT (SELECT count(*) FROM pending_referral_usage_events_v2)::text AS pending,
             (SELECT count(*) FROM partner_usage_events_v2)::text AS usage,
             (SELECT count(*) FROM commission_entries_v2)::text AS entries
    `);
    expect(after.rows[0]).toEqual({ pending: "0", usage: "1", entries: "2" });

    // Повторный replay того же commerce_event_id — duplicate, двойной записи нет.
    await expect(recordReferredSpendV2(db, input)).resolves.toBe("duplicate");
  });

  it("fail-closed: ineligible or zero-paid events are skipped without any writes", async () => {
    const chain = await seedChain();
    const ineligible = event({ commerceEventId: 7003n, commissionEligible: false });
    await attribute(ineligible.commerceUserId, chain.direct);
    await expect(recordReferredSpendV2(db, ineligible)).resolves.toBe("skipped");

    const bonusOnly = event({
      commerceEventId: 7004n,
      paidFundedNano: 0n,
      chargedNano: 2_000n,
    });
    await attribute(bonusOnly.commerceUserId, chain.direct);
    await expect(recordReferredSpendV2(db, bonusOnly)).resolves.toBe("skipped");

    const counts = await db.pool.query<{ usage: string; entries: string; pending: string }>(`
      SELECT (SELECT count(*) FROM partner_usage_events_v2)::text AS usage,
             (SELECT count(*) FROM commission_entries_v2)::text AS entries,
             (SELECT count(*) FROM pending_referral_usage_events_v2)::text AS pending
    `);
    expect(counts.rows[0]).toEqual({ usage: "0", entries: "0", pending: "0" });
  });

  it("rejects a malformed lineage before touching the database", async () => {
    const chain = await seedChain();
    const malformed = event({ commerceEventId: 7005n, chargedNano: 12_001n });
    await attribute(malformed.commerceUserId, chain.direct);
    await expect(recordReferredSpendV2(db, malformed)).rejects.toBeInstanceOf(ReferredSpendV2ShapeError);

    const counts = await db.pool.query<{ usage: string }>(
      "SELECT count(*)::text AS usage FROM partner_usage_events_v2",
    );
    expect(counts.rows[0]).toEqual({ usage: "0" });
  });

  it("keeps pure helpers consistent with the writer contract", () => {
    const input = event();
    expect(() => assertReferredSpendV2Shape(input)).not.toThrow();
    expect(isCommissionableSpendV2(input)).toBe(true);
    expect(isCommissionableSpendV2(event({ commissionEligible: false }))).toBe(false);
  });

  it("readers aggregate both schemas without double counting", async () => {
    const chain = await seedChain();
    // v1 evidence: track-событие 4_000 nano, level-0 комиссия 400.
    const v1User = randomUUID();
    await attribute(v1User, chain.direct);
    const v1Usage = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events (
        commerce_event_id, commerce_user_id, partner_id, amount_nano,
        provider_id, account_class, pricing_mode, paid_funded_nano,
        commission_eligible, snapshot_digest, occurred_at
      ) VALUES (8001, $1, $2, 4000, 'anthropic', 'b2c', 'track', 4000, true, 'snapshot-v1', now())
      RETURNING id::text
    `, [v1User, chain.direct]);
    await db.pool.query(`
      INSERT INTO commission_entries (usage_event_id, partner_id, level, applied_bps, amount_nano)
      VALUES ($1, $2, 0, 1000, 400)
    `, [v1Usage.rows[0]!.id, chain.direct]);

    // v2 evidence: paid 10_000 → level 0 = 1000 (direct), level 1 = 50 (parent).
    const v2Input = event({ commerceEventId: 8002n });
    await attribute(v2Input.commerceUserId, chain.direct);
    await expect(recordReferredSpendV2(db, v2Input)).resolves.toBe("recorded");

    const { getPartnerEarningsTotals } = await import("./commissions.js");
    const { listReferredUsers } = await import("./referrals.js");
    const totals = await getPartnerEarningsTotals(db, chain.direct);
    expect(totals.earnedNano).toBe(1_400n);   // 400 (v1) + 1000 (v2)
    expect(totals.directNano).toBe(1_400n);
    expect(totals.overrideNano).toBe(0n);      // level 1 принадлежит parent, не direct
    expect(totals.last30dSpendNano).toBe(14_000n); // 4000 (v1 amount) + 10000 (v2 paid)

    const parentTotals = await getPartnerEarningsTotals(db, chain.parent);
    expect(parentTotals.earnedNano).toBe(50n);
    expect(parentTotals.overrideNano).toBe(50n);

    const referrals = await listReferredUsers(db, chain.direct);
    const v1Row = referrals.find((row) => row.commerceUserId === v1User);
    const v2Row = referrals.find((row) => row.commerceUserId === v2Input.commerceUserId);
    expect(v1Row).toMatchObject({ spendNano: 4_000n, earnedNano: 400n });
    expect(v2Row).toMatchObject({ spendNano: 10_000n, earnedNano: 1_000n });

    const { getPartnerPeriodState } = await import("./payout-periods.js");
    const state = await getPartnerPeriodState(db, chain.direct, new Date());
    expect(state.lifetimeEarnedNano).toBe("1400");
  });
});
