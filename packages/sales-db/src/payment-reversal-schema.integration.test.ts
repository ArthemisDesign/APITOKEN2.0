import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("payment reversal accounting schema", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!, "sales-payment-reversal-schema-test");
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await db.pool.query(`
      TRUNCATE partner_commission_adjustments,
        partner_commission_funding_allocations,
        partner_payment_reversals,
        partner_usage_funding_allocations,
        partner_paid_funding_lots,
        commission_entries_v2, partner_usage_events_v2,
        commission_entries, partner_usage_events,
        referred_topups, referred_users, partners
      RESTART IDENTITY CASCADE
    `);
    await db.pool.end();
  });

  beforeEach(async () => {
    await db.pool.query(`
      TRUNCATE partner_commission_adjustments,
        partner_commission_funding_allocations,
        partner_payment_reversals,
        partner_usage_funding_allocations,
        partner_paid_funding_lots,
        commission_entries_v2, partner_usage_events_v2,
        commission_entries, partner_usage_events,
        referred_topups, referred_users, partners
      RESTART IDENTITY CASCADE
    `);
  });

  async function seedReferral(): Promise<{ partnerId: string; userId: string }> {
    const partner = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES($1, 'active', 2300, 1000)
      RETURNING id
    `, [`reversal-${randomUUID()}`]);
    const partnerId = partner.rows[0]!.id;
    const userId = randomUUID();
    await db.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, attributed_at)
      VALUES($1, $2, '2026-08-01T00:00:00.000Z')
    `, [userId, partnerId]);
    return { partnerId, userId };
  }

  async function insertLot(input: {
    partnerId: string;
    userId: string;
    commerceTopupId: bigint;
    paymentId: string;
    amountNano: bigint;
    paidAt: string;
  }): Promise<bigint> {
    const topup = await db.pool.query<{ id: string }>(`
      INSERT INTO referred_topups(
        commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at
      ) VALUES($1, $2, $3, $4, $5)
      RETURNING id::text
    `, [
      input.paymentId, input.userId, input.partnerId,
      input.amountNano.toString(), input.paidAt,
    ]);
    const lot = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_paid_funding_lots(
        referred_topup_id, commerce_topup_id, commerce_payment_id,
        commerce_user_id, partner_id, original_amount_nano, paid_at
      ) VALUES($1, $2, $3, $4, $5, $6, $7)
      RETURNING id::text
    `, [
      topup.rows[0]!.id, input.commerceTopupId.toString(), input.paymentId,
      input.userId, input.partnerId, input.amountNano.toString(), input.paidAt,
    ]);
    return BigInt(lot.rows[0]!.id);
  }

  async function insertUsageAndCommission(input: {
    partnerId: string;
    userId: string;
    commerceEventId: bigint;
    amountNano: bigint;
    occurredAt: string;
  }): Promise<{ usageId: bigint; commissionId: bigint }> {
    const usage = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES($1, $2, $3, $4, $5)
      RETURNING id::text
    `, [
      input.commerceEventId.toString(), input.userId, input.partnerId,
      input.amountNano.toString(), input.occurredAt,
    ]);
    const commissionAmount = input.amountNano * 2300n / 10_000n;
    const commission = await db.pool.query<{ id: string }>(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, amount_nano
      ) VALUES($1, $2, 0, 2300, $3)
      RETURNING id::text
    `, [usage.rows[0]!.id, input.partnerId, commissionAmount.toString()]);
    return {
      usageId: BigInt(usage.rows[0]!.id),
      commissionId: BigInt(commission.rows[0]!.id),
    };
  }

  it("pins deterministic FIFO slices and negates only the reversed payment share", async () => {
    const referral = await seedReferral();
    const firstLotId = await insertLot({
      ...referral,
      commerceTopupId: 10n,
      paymentId: "payment-first",
      amountNano: 600n,
      paidAt: "2026-08-02T00:00:00.000Z",
    });
    const secondLotId = await insertLot({
      ...referral,
      commerceTopupId: 11n,
      paymentId: "payment-second",
      amountNano: 600n,
      paidAt: "2026-08-02T00:00:00.000Z",
    });
    const source = await insertUsageAndCommission({
      ...referral,
      commerceEventId: 50n,
      amountNano: 1000n,
      occurredAt: "2026-08-03T00:00:00.000Z",
    });

    const firstAllocation = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 600)
      RETURNING id::text
    `, [firstLotId.toString(), source.usageId.toString()]);
    const secondAllocation = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 400)
      RETURNING id::text
    `, [secondLotId.toString(), source.usageId.toString()]);

    const firstCommissionSlice = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 138)
      RETURNING id::text
    `, [firstAllocation.rows[0]!.id, source.commissionId.toString()]);
    await db.pool.query(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 92)
    `, [secondAllocation.rows[0]!.id, source.commissionId.toString()]);

    const reversalClient = await db.pool.connect();
    let reversalId: string;
    try {
      await reversalClient.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
      await reversalClient.query("SET CONSTRAINTS partner_reversal_adjustment_set_guard DEFERRED");
      const reversal = await reversalClient.query<{ id: string }>(`
        INSERT INTO partner_payment_reversals(
          commerce_reversal_id, funding_lot_id, commerce_payment_id,
          commerce_user_id, kind, original_amount_nano, reversed_at
        ) VALUES(7001, $1, 'payment-first', $2, 'dispute', 600,
          '2026-08-04T00:00:00.000Z')
        RETURNING id::text
      `, [firstLotId.toString(), referral.userId]);
      reversalId = reversal.rows[0]!.id;
      await reversalClient.query(`
        INSERT INTO partner_commission_adjustments(
          reversal_id, commission_funding_allocation_id, partner_id,
          amount_nano, effective_at
        ) VALUES($1, $2, $3, -138, '2026-08-04T00:00:00.000Z')
      `, [reversalId, firstCommissionSlice.rows[0]!.id, referral.partnerId]);
      await reversalClient.query("COMMIT");
    } finally {
      await reversalClient.query("ROLLBACK").catch(() => undefined);
      reversalClient.release();
    }

    const balance = await db.pool.query<{ gross: string; adjustments: string; net: string }>(`
      SELECT
        (SELECT sum(amount_nano)::text FROM commission_entries) AS gross,
        (SELECT sum(amount_nano)::text FROM partner_commission_adjustments) AS adjustments,
        ((SELECT sum(amount_nano) FROM commission_entries)
          + (SELECT sum(amount_nano) FROM partner_commission_adjustments))::text AS net
    `);
    expect(balance.rows[0]).toEqual({ gross: "230", adjustments: "-138", net: "92" });

    await expect(db.pool.query(`
      INSERT INTO partner_commission_adjustments(
        reversal_id, commission_funding_allocation_id, partner_id,
        amount_nano, effective_at
      ) VALUES($1, $2, $3, -137, '2026-08-04T00:00:00.000Z')
    `, [reversalId, firstCommissionSlice.rows[0]!.id, referral.partnerId]))
      .rejects.toMatchObject({ code: "23514" });
    await expect(db.pool.query(`
      INSERT INTO partner_commission_adjustments(
        reversal_id, commission_funding_allocation_id, partner_id,
        amount_nano, effective_at
      ) VALUES($1, $2, $3, -138, '2026-08-04T00:00:00.000Z')
    `, [reversalId, firstCommissionSlice.rows[0]!.id, referral.partnerId]))
      .rejects.toMatchObject({ code: "25001" });
    await expect(db.pool.query(`
      UPDATE partner_payment_reversals SET kind='refund' WHERE id=$1
    `, [reversalId])).rejects.toMatchObject({ code: "23514" });
  });

  it("rejects FIFO violations, over-allocation, invented rounding and mismatched reversals", async () => {
    const referral = await seedReferral();
    const firstLotId = await insertLot({
      ...referral,
      commerceTopupId: 20n,
      paymentId: "a-payment-old",
      amountNano: 700n,
      paidAt: "2026-08-02T00:00:00.000Z",
    });
    const secondLotId = await insertLot({
      ...referral,
      commerceTopupId: 21n,
      paymentId: "b-payment-new",
      amountNano: 600n,
      paidAt: "2026-08-02T00:00:00.000Z",
    });
    const source = await insertUsageAndCommission({
      ...referral,
      commerceEventId: 60n,
      amountNano: 1000n,
      occurredAt: "2026-08-03T00:00:00.000Z",
    });

    await expect(db.pool.query(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 400)
    `, [secondLotId.toString(), source.usageId.toString()]))
      .rejects.toMatchObject({ code: "23514" });

    const firstAllocation = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 700)
      RETURNING id::text
    `, [firstLotId.toString(), source.usageId.toString()]);
    await expect(db.pool.query(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 161)
    `, [firstAllocation.rows[0]!.id, source.commissionId.toString()]))
      .rejects.toMatchObject({ code: "23514" });
    await expect(db.pool.query(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 500)
    `, [secondLotId.toString(), source.usageId.toString()]))
      .rejects.toMatchObject({ code: "23514" });

    const secondAllocation = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 300)
      RETURNING id::text
    `, [secondLotId.toString(), source.usageId.toString()]);
    await expect(db.pool.query(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 162)
    `, [firstAllocation.rows[0]!.id, source.commissionId.toString()]))
      .rejects.toMatchObject({ code: "23514" });
    const commissionSlices = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 161), ($3, $2, 69)
      RETURNING id::text
    `, [
      firstAllocation.rows[0]!.id, source.commissionId.toString(),
      secondAllocation.rows[0]!.id,
    ]);

    // A second commission-bearing usage slice funded by the same second payment makes the expected
    // reversal adjustment set contain two rows.
    const laterUsage = await insertUsageAndCommission({
      ...referral,
      commerceEventId: 61n,
      amountNano: 100n,
      occurredAt: "2026-08-03T01:00:00.000Z",
    });
    const laterUsageAllocation = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_funding_allocations(
        funding_lot_id, usage_event_id, allocated_paid_nano
      ) VALUES($1, $2, 100)
      RETURNING id::text
    `, [secondLotId.toString(), laterUsage.usageId.toString()]);
    await db.pool.query(`
      INSERT INTO partner_commission_funding_allocations(
        usage_funding_allocation_id, commission_entry_id, allocated_commission_nano
      ) VALUES($1, $2, 23)
    `, [laterUsageAllocation.rows[0]!.id, laterUsage.commissionId.toString()]);

    const incompleteClient = await db.pool.connect();
    try {
      await incompleteClient.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
      await incompleteClient.query("SET CONSTRAINTS partner_reversal_adjustment_set_guard DEFERRED");
      const reversal = await incompleteClient.query<{ id: string }>(`
        INSERT INTO partner_payment_reversals(
          commerce_reversal_id, funding_lot_id, commerce_payment_id,
          commerce_user_id, kind, original_amount_nano, reversed_at
        ) VALUES(7002, $1, 'b-payment-new', $2, 'refund', 600,
          '2026-08-04T00:00:00.000Z')
        RETURNING id::text
      `, [secondLotId.toString(), referral.userId]);
      const insertedAdjustment = await incompleteClient.query<{ allocation_id: string }>(`
        INSERT INTO partner_commission_adjustments(
          reversal_id, commission_funding_allocation_id, partner_id,
          amount_nano, effective_at
        )
        SELECT $1, allocation.id, $2, -allocation.allocated_commission_nano,
               '2026-08-04T00:00:00.000Z'
        FROM partner_commission_funding_allocations allocation
        JOIN partner_usage_funding_allocations usage
          ON usage.id = allocation.usage_funding_allocation_id
        WHERE usage.funding_lot_id = $3 AND allocation.allocated_commission_nano > 0
        ORDER BY allocation.id DESC
        LIMIT 1
        RETURNING commission_funding_allocation_id::text AS allocation_id
      `, [reversal.rows[0]!.id, referral.partnerId, secondLotId.toString()]);
      expect(commissionSlices.rows.map((row) => row.id)).not.toContain(
        insertedAdjustment.rows[0]!.allocation_id,
      );
      const setCounts = await incompleteClient.query<{ adjustments: string; slices: string }>(`
        SELECT
          (SELECT count(*)::text
           FROM partner_commission_adjustments
           WHERE reversal_id = $1) AS adjustments,
          (SELECT count(*)::text
           FROM partner_commission_funding_allocations allocation
           JOIN partner_usage_funding_allocations usage
             ON usage.id = allocation.usage_funding_allocation_id
           WHERE usage.funding_lot_id = $2
             AND allocation.allocated_commission_nano > 0) AS slices
      `, [reversal.rows[0]!.id, secondLotId.toString()]);
      expect(setCounts.rows[0]).toEqual({ adjustments: "1", slices: "2" });
      await expect(incompleteClient.query(
        "SET CONSTRAINTS partner_reversal_adjustment_set_guard IMMEDIATE",
      )).rejects.toMatchObject({ code: "23514" });
    } finally {
      await incompleteClient.query("ROLLBACK").catch(() => undefined);
      incompleteClient.release();
    }

    await expect(db.pool.query(`
      INSERT INTO partner_payment_reversals(
        commerce_reversal_id, funding_lot_id, commerce_payment_id,
        commerce_user_id, kind, original_amount_nano, reversed_at
      ) VALUES(7003, $1, 'a-payment-old', $2, 'refund', 701,
        '2026-08-04T00:00:00.000Z')
    `, [firstLotId.toString(), referral.userId])).rejects.toMatchObject({ code: "23514" });
  });
});
