import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  AmbiguousTopupCursorBoundaryError,
  listPaidTopupsAfter,
  listPaidTopupsV2After,
  listUsageEventsAfter,
  recordReferralAttribution,
  ReferralAttributionConflictError,
} from "./sales-feed.js";

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
        pricing_usage_events, referral_attributions, customer_profiles,
        payments, checkout_sessions, engine_accounts, users
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
        "INSERT INTO referral_attributions (user_id, code, created_at) VALUES ($1, 'partner-code', now() - interval '5 minutes')",
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

  it("emits the free-first commission basis and no retired lineage fields", async () => {
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

  it("emits externally funded referred B2B usage through the same scalar commission contract", async () => {
    const referred = await insertUser(true);
    await database.pool.query(`
      INSERT INTO customer_profiles
        (user_id, customer_type, current_tier, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2b', NULL, 3700, date_trunc('month', now()))
    `, [referred]);
    await insertUsage(referred, 31, new Date(Date.now() - 60_000), 606_000_000n);

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.items).toEqual([expect.objectContaining({
      userId: referred,
      amountNano: 606_000_000n,
      accountClass: null,
      pricingMode: null,
      paidFundedNano: null,
      commissionEligible: null,
      snapshotDigest: null,
    })]);
  });

  it("treats an exact attribution replay as idempotent and rejects a different first-touch owner", async () => {
    const referred = await insertUser(false);
    await recordReferralAttribution(database, referred, "first-owner");
    await expect(recordReferralAttribution(database, referred, "first-owner")).resolves.toBeUndefined();
    await expect(recordReferralAttribution(database, referred, "other-owner"))
      .rejects.toBeInstanceOf(ReferralAttributionConflictError);

    const stored = await database.pool.query<{ code: string }>(
      "SELECT code FROM referral_attributions WHERE user_id = $1",
      [referred],
    );
    expect(stored.rows).toEqual([{ code: "first-owner" }]);
  });

  it("never attributes spend that occurred before the referral was recorded", async () => {
    const referred = await insertUser(true);
    const attributedAt = new Date(Date.now() - 120_000);
    await database.pool.query(
      "UPDATE referral_attributions SET created_at = $2 WHERE user_id = $1",
      [referred, attributedAt],
    );
    await insertUsage(referred, 40, new Date(attributedAt.getTime() - 1_000), 111n);
    await insertUsage(referred, 41, new Date(attributedAt.getTime() + 1_000), 222n);

    const page = await listUsageEventsAfter(database, 0n, 100);
    expect(page.items).toEqual([
      expect.objectContaining({ userId: referred, amountNano: 222n }),
    ]);
    // The ineligible historical row is still part of the source stream and advances the cursor.
    expect(page.nextCursor).toBe(2n);
  });

  it("excludes ordinary customer top-ups while preserving referred top-ups", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const base = Date.now() - 120_000;
    await database.pool.query(
      "UPDATE referral_attributions SET created_at = now() - interval '3 minutes' WHERE user_id = $1",
      [referred],
    );

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

  it("never exposes top-ups paid before attribution", async () => {
    const referred = await insertUser(true);
    const attributedAt = new Date(Date.now() - 120_000);
    await database.pool.query(
      "UPDATE referral_attributions SET created_at = $2 WHERE user_id = $1",
      [referred, attributedAt],
    );
    await insertPaidTopup(referred, "before-attribution", new Date(attributedAt.getTime() - 1_000));
    const afterId = await insertPaidTopup(referred, "after-attribution", new Date(attributedAt.getTime() + 1_000));

    const page = await listPaidTopupsAfter(database, 0n, 100);
    expect(page.items).toEqual([
      expect.objectContaining({ userId: referred, paymentId: afterId }),
    ]);
    expect(page.nextCursor).toBe(BigInt((attributedAt.getTime() + 1_000) * 1_000));
  });

  it("fails closed when equal paid_at timestamps cross the page boundary", async () => {
    const referred = await insertUser(true);
    const paidAt = new Date(Date.now() - 120_000);
    await insertPaidTopup(referred, "tie-a", paidAt);
    await insertPaidTopup(referred, "tie-b", paidAt);

    await expect(listPaidTopupsAfter(database, 0n, 1))
      .rejects.toBeInstanceOf(AmbiguousTopupCursorBoundaryError);
  });

  it("resumes equal paid_at top-ups independently by commit-ordered sequence", async () => {
    const referred = await insertUser(true);
    const paidAt = new Date(Date.now() - 120_000);
    const firstPaymentId = await insertPaidTopup(referred, "v2-tie-a", paidAt);
    const secondPaymentId = await insertPaidTopup(referred, "v2-tie-b", paidAt);

    const first = await listPaidTopupsV2After(database, 0n, 1);
    expect(first).toMatchObject({
      items: [expect.objectContaining({ id: 1n, paymentId: firstPaymentId, paidAt })],
      nextCursor: 1n,
    });
    const second = await listPaidTopupsV2After(database, first.nextCursor, 1);
    expect(second).toMatchObject({
      items: [expect.objectContaining({ id: 2n, paymentId: secondPaymentId, paidAt })],
      nextCursor: 2n,
    });
    await expect(listPaidTopupsV2After(database, second.nextCursor, 1)).resolves.toEqual({
      items: [],
      nextCursor: 2n,
    });
  });

  it("keeps a verified deposit replayable after its payment is refunded", async () => {
    const referred = await insertUser(true);
    const paidAt = new Date(Date.now() - 120_000);
    const paymentId = await insertPaidTopup(referred, "v2-refunded", paidAt);
    await database.pool.query("UPDATE payments SET status = 'refunded' WHERE id = $1", [paymentId]);

    await expect(listPaidTopupsV2After(database, 0n, 100)).resolves.toMatchObject({
      items: [expect.objectContaining({ id: 1n, paymentId, userId: referred, paidAt })],
      nextCursor: 1n,
    });
  });

  it("advances topups-v2 over every source row before referral filtering", async () => {
    const ordinaryBefore = await insertUser(false);
    const referred = await insertUser(true);
    const ordinaryAfter = await insertUser(false);
    const paidAt = new Date(Date.now() - 120_000);
    await insertPaidTopup(ordinaryBefore, "v2-ordinary-before", paidAt);
    const referredPaymentId = await insertPaidTopup(referred, "v2-referred", paidAt);
    await insertPaidTopup(ordinaryAfter, "v2-ordinary-after", paidAt);

    const first = await listPaidTopupsV2After(database, 0n, 1);
    expect(first).toEqual({ items: [], nextCursor: 1n });
    const second = await listPaidTopupsV2After(database, first.nextCursor, 1);
    expect(second).toMatchObject({
      items: [expect.objectContaining({ id: 2n, paymentId: referredPaymentId })],
      nextCursor: 2n,
    });
    const third = await listPaidTopupsV2After(database, second.nextCursor, 1);
    expect(third).toEqual({ items: [], nextCursor: 3n });
  });

  it("fails closed instead of advancing past a paid row without paid_at", async () => {
    const referred = await insertUser(true);
    await insertPaidTopup(referred, "v2-missing-paid-at", new Date(Date.now() - 120_000));
    await database.pool.query("UPDATE payments SET paid_at = NULL");

    await expect(listPaidTopupsV2After(database, 0n, 100))
      .rejects.toThrow("has no paid_at");
  });

  it("serializes attribution ids against an in-flight legacy insert", async () => {
    const blockerUser = await insertUser(false);
    const referred = await insertUser(false);
    const blocker = await database.pool.connect();
    try {
      await blocker.query("BEGIN");
      await blocker.query(
        "INSERT INTO referral_attributions (user_id, code) VALUES ($1, 'legacy-in-flight')",
        [blockerUser],
      );

      const recording = recordReferralAttribution(database, referred, "new-writer");
      let observedWait = false;
      for (let attempt = 0; attempt < 50 && !observedWait; attempt += 1) {
        const locks = await database.pool.query<{ waiting: boolean }>(`
          SELECT EXISTS (
            SELECT 1 FROM pg_locks
            WHERE relation = 'referral_attributions'::regclass
              AND mode = 'ShareRowExclusiveLock' AND NOT granted
          ) AS waiting
        `);
        observedWait = locks.rows[0]?.waiting ?? false;
        if (!observedWait) await new Promise((resolve) => setTimeout(resolve, 20));
      }
      expect(observedWait).toBe(true);

      await blocker.query("COMMIT");
      await recording;
      const rows = await database.pool.query<{ code: string }>(
        "SELECT code FROM referral_attributions ORDER BY id",
      );
      expect(rows.rows.map((row) => row.code)).toEqual(["legacy-in-flight", "new-writer"]);
    } finally {
      await blocker.query("ROLLBACK").catch(() => undefined);
      blocker.release();
    }
  });
});
