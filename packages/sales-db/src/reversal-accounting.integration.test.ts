import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  hasIncompletePartnerFundingEvidence,
  getPartnerReversalAccountingHealth,
  PartnerFundingReplayConflictError,
  reconcilePartnerFundingEvidence,
  recordPaidFundingLot,
  recordPaymentReversalPage,
} from "./reversal-accounting.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("payment reversal consumer accounting", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!, "sales-payment-reversal-consumer-test");
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(async () => {
    await truncate();
  });

  async function truncate(): Promise<void> {
    await db?.pool.query(`
      TRUNCATE sync_cursors, partner_commission_adjustments,
        partner_commission_funding_allocations, partner_payment_reversals,
        partner_usage_funding_allocations, partner_paid_funding_lots,
        commission_entries_v2, partner_usage_events_v2,
        commission_entries, partner_usage_events,
        referred_topups, referred_users, partners
      RESTART IDENTITY CASCADE
    `);
  }

  async function seedReferral(): Promise<{
    partnerId: string;
    userId: string;
    paymentId: string;
    topupId: bigint;
  }> {
    const partner = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES($1, 'active', 2300, 1000)
      RETURNING id
    `, [`consumer-${randomUUID()}`]);
    const partnerId = partner.rows[0]!.id;
    const userId = randomUUID();
    const paymentId = randomUUID();
    await db.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, attributed_at)
      VALUES($1, $2, '2026-08-01T00:00:00.000Z')
    `, [userId, partnerId]);
    const topup = await db.pool.query<{ id: string }>(`
      INSERT INTO referred_topups(
        commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at
      ) VALUES($1, $2, $3, 600, '2026-08-02T00:00:00.000Z')
      RETURNING id::text
    `, [paymentId, userId, partnerId]);
    return { partnerId, userId, paymentId, topupId: BigInt(topup.rows[0]!.id) };
  }

  it("replays lots exactly and rejects an immutable source conflict", async () => {
    const source = await seedReferral();
    const input = {
      commerceTopupId: 10n,
      commercePaymentId: source.paymentId,
      commerceUserId: source.userId,
      amountNano: 600n,
      paidAt: new Date("2026-08-02T00:00:00.000Z"),
    };
    await expect(recordPaidFundingLot(db, input)).resolves.toBe("recorded");
    await expect(recordPaidFundingLot(db, input)).resolves.toBe("duplicate");
    await expect(recordPaidFundingLot(db, { ...input, commerceTopupId: 11n }))
      .rejects.toBeInstanceOf(PartnerFundingReplayConflictError);

    const rows = await db.pool.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM partner_paid_funding_lots",
    );
    expect(rows.rows[0]!.count).toBe("1");
  });

  it("allocates FIFO commission slices and atomically claws back one payment", async () => {
    const source = await seedReferral();
    await recordPaidFundingLot(db, {
      commerceTopupId: 10n,
      commercePaymentId: source.paymentId,
      commerceUserId: source.userId,
      amountNano: 600n,
      paidAt: new Date("2026-08-02T00:00:00.000Z"),
    });
    const secondPaymentId = randomUUID();
    await db.pool.query(`
      INSERT INTO referred_topups(
        commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at
      ) VALUES($1, $2, $3, 600, '2026-08-02T01:00:00.000Z')
    `, [secondPaymentId, source.userId, source.partnerId]);
    await recordPaidFundingLot(db, {
      commerceTopupId: 11n,
      commercePaymentId: secondPaymentId,
      commerceUserId: source.userId,
      amountNano: 600n,
      paidAt: new Date("2026-08-02T01:00:00.000Z"),
    });

    const usage = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(50, $1, $2, 1000, '2026-08-03T00:00:00.000Z')
      RETURNING id::text
    `, [source.userId, source.partnerId]);
    await db.pool.query(`
      INSERT INTO commission_entries(usage_event_id, partner_id, level, applied_bps, amount_nano)
      VALUES($1, $2, 0, 2300, 230)
    `, [usage.rows[0]!.id, source.partnerId]);

    await expect(hasIncompletePartnerFundingEvidence(db)).resolves.toBe(true);
    await expect(reconcilePartnerFundingEvidence(db)).resolves.toEqual({ examined: 1, completed: 1 });
    await expect(hasIncompletePartnerFundingEvidence(db)).resolves.toBe(false);

    const slices = await db.pool.query<{
      topup_id: string;
      paid: string;
      commission: string;
    }>(`
      SELECT lot.commerce_topup_id::text AS topup_id,
             usage_allocation.allocated_paid_nano::text AS paid,
             commission_allocation.allocated_commission_nano::text AS commission
      FROM partner_usage_funding_allocations usage_allocation
      JOIN partner_paid_funding_lots lot ON lot.id = usage_allocation.funding_lot_id
      JOIN partner_commission_funding_allocations commission_allocation
        ON commission_allocation.usage_funding_allocation_id = usage_allocation.id
      ORDER BY lot.commerce_topup_id
    `);
    expect(slices.rows).toEqual([
      { topup_id: "10", paid: "600", commission: "138" },
      { topup_id: "11", paid: "400", commission: "92" },
    ]);

    const reversal = {
      commerceReversalId: 77n,
      commercePaymentId: source.paymentId,
      commerceUserId: source.userId,
      kind: "refund" as const,
      amountNano: 600n,
      reversedAt: new Date("2026-08-04T00:00:00.000Z"),
    };
    await recordPaymentReversalPage(db, [reversal], 78n);
    // Simulate a crash replay by putting the cursor behind while keeping immutable evidence.
    await db.pool.query("UPDATE sync_cursors SET last_id=0 WHERE feed='payment_reversals'");
    await recordPaymentReversalPage(db, [reversal], 78n);

    const result = await db.pool.query<{
      reversals: string;
      adjustments: string;
      amount: string;
      cursor: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT count(*)::text FROM partner_commission_adjustments) AS adjustments,
        (SELECT COALESCE(sum(amount_nano), 0)::text
         FROM partner_commission_adjustments) AS amount,
        (SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals') AS cursor
    `);
    expect(result.rows[0]).toEqual({
      reversals: "1",
      adjustments: "1",
      amount: "-138",
      cursor: "78",
    });
    await expect(getPartnerReversalAccountingHealth(db)).resolves.toMatchObject({
      paymentReversalCursor: 78n,
      incompleteUsageCount: 0n,
      missingCommissionSliceCount: 0n,
      reversalCount: 1n,
      adjustmentCount: 1n,
      adjustmentNano: -138n,
    });
  });

  it("keeps the cursor behind on missing lots and immutable replay conflicts", async () => {
    const source = await seedReferral();
    const base = {
      commerceReversalId: 90n,
      commercePaymentId: source.paymentId,
      commerceUserId: source.userId,
      kind: "refund" as const,
      amountNano: 600n,
      reversedAt: new Date("2026-08-04T00:00:00.000Z"),
    };
    await expect(recordPaymentReversalPage(db, [base], 91n))
      .rejects.toThrow("waiting for its funding lot");
    await expect(db.pool.query(
      "SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals'",
    )).resolves.toMatchObject({ rows: [] });

    await recordPaidFundingLot(db, {
      commerceTopupId: 10n,
      commercePaymentId: source.paymentId,
      commerceUserId: source.userId,
      amountNano: 600n,
      paidAt: new Date("2026-08-02T00:00:00.000Z"),
    });
    await recordPaymentReversalPage(db, [base], 91n);
    await db.pool.query("UPDATE sync_cursors SET last_id=0 WHERE feed='payment_reversals'");
    await expect(recordPaymentReversalPage(db, [{ ...base, kind: "dispute" }], 91n))
      .rejects.toBeInstanceOf(PartnerFundingReplayConflictError);
    const cursor = await db.pool.query<{ last_id: string }>(
      "SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals'",
    );
    expect(cursor.rows[0]!.last_id).toBe("0");
  });
});
