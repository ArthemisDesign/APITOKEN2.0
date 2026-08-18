import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  getPartnerEarningsByProvider,
  getPartnerEarningsTotals,
  recordReferredSpend,
  reconcilePendingReferralEvents,
} from "./commissions.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

// The live feed emits the scalar usage form, where the provider travels OUTSIDE the retired
// attribution tuple. These tests pin the two properties that matter: the dimension is recorded and
// reported, and it never becomes an input to a money decision.
describe.runIf(Boolean(connectionString))("spend provider reporting dimension", () => {
  let db: SalesDatabase;
  let partnerId: string;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(async () => {
    await truncate();
    const partner = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code,status,commission_bps,sub_commission_bps)
      VALUES('spd-direct','active',1000,1000)
      RETURNING id
    `);
    partnerId = partner.rows[0]!.id;
  });

  async function truncate(): Promise<void> {
    await db.pool.query(`
      TRUNCATE commission_entries, partner_usage_events, pending_referral_events,
        referred_users, partners RESTART IDENTITY CASCADE
    `);
  }

  async function attribute(commerceUserId: string): Promise<void> {
    await db.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, referral_code, attributed_at)
      VALUES($1, $2, 'spd-direct', '2026-07-01T00:00:00.000Z')
    `, [commerceUserId, partnerId]);
  }

  const occurredAt = new Date("2026-07-02T00:00:00.000Z");

  it("records the provider of scalar spend and reports the split", async () => {
    const user = randomUUID();
    await attribute(user);
    await recordReferredSpend(db, {
      commerceEventId: 1n, commerceUserId: user, amountNano: 100_000n,
      spendProviderId: "anthropic", occurredAt,
    });
    await recordReferredSpend(db, {
      commerceEventId: 2n, commerceUserId: user, amountNano: 300_000n,
      spendProviderId: "openai", occurredAt,
    });

    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    expect(split.map((row) => [row.providerId, row.spendNano, row.earnedNano])).toEqual([
      ["openai", 300_000n, 30_000n],
      ["anthropic", 100_000n, 10_000n],
    ]);
  });

  it("reconciles exactly with the partner's recorded earnings", async () => {
    const user = randomUUID();
    await attribute(user);
    const events = [
      [1n, "anthropic", 100_000n], [2n, "openai", 300_000n],
      [3n, "google", 50_000n], [4n, "kimi", 7_000n],
    ] as const;
    for (const [id, provider, amount] of events) {
      await recordReferredSpend(db, {
        commerceEventId: id, commerceUserId: user, amountNano: amount,
        spendProviderId: provider, occurredAt,
      });
    }
    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    const totals = await getPartnerEarningsTotals(db, partnerId);
    const summed = split.reduce((sum, row) => sum + row.earnedNano, 0n);
    // The split re-groups recorded commission; the parts must equal the whole exactly.
    expect(summed).toBe(totals.earnedNano);
  });

  it("groups spend with no recorded provider instead of dropping it", async () => {
    const user = randomUUID();
    await attribute(user);
    await recordReferredSpend(db, {
      commerceEventId: 1n, commerceUserId: user, amountNano: 100_000n, occurredAt,
    });
    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    expect(split).toEqual([
      { providerId: null, events: 1, spendNano: 100_000n, earnedNano: 10_000n },
    ]);
  });

  it("pays identical commission whichever provider served the request", async () => {
    // The dimension must stay descriptive: same money in, same money out.
    const anthropicUser = randomUUID();
    const kimiUser = randomUUID();
    await attribute(anthropicUser);
    await attribute(kimiUser);
    await recordReferredSpend(db, {
      commerceEventId: 1n, commerceUserId: anthropicUser, amountNano: 250_000n,
      spendProviderId: "anthropic", occurredAt,
    });
    await recordReferredSpend(db, {
      commerceEventId: 2n, commerceUserId: kimiUser, amountNano: 250_000n,
      spendProviderId: "kimi", occurredAt,
    });
    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    expect(split).toHaveLength(2);
    expect(new Set(split.map((row) => row.earnedNano)).size).toBe(1);
  });

  it("keeps the provider through the pending buffer when spend precedes attribution", async () => {
    const user = randomUUID();
    // No attribution yet: the event is buffered, not recorded.
    const first = await recordReferredSpend(db, {
      commerceEventId: 9n, commerceUserId: user, amountNano: 100_000n,
      spendProviderId: "google", occurredAt,
    });
    expect(first).toBe("buffered");

    await attribute(user);
    await reconcilePendingReferralEvents(db);

    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    expect(split).toEqual([
      { providerId: "google", events: 1, spendNano: 100_000n, earnedNano: 10_000n },
    ]);
  });

  it("enriches a row imported before the provider was carried, and never conflicts on it", async () => {
    const user = randomUUID();
    await attribute(user);
    await recordReferredSpend(db, {
      commerceEventId: 5n, commerceUserId: user, amountNano: 100_000n, occurredAt,
    });
    // A later replay of the same event now carries the provider: it fills the gap...
    await expect(recordReferredSpend(db, {
      commerceEventId: 5n, commerceUserId: user, amountNano: 100_000n,
      spendProviderId: "openai", occurredAt,
    })).resolves.toBe("duplicate");
    expect((await getPartnerEarningsByProvider(db, partnerId, 3650))[0]!.providerId).toBe("openai");

    // ...and a replay that disagrees is NOT a conflict — a reporting label must never fail a feed
    // page or rewrite recorded history.
    await expect(recordReferredSpend(db, {
      commerceEventId: 5n, commerceUserId: user, amountNano: 100_000n,
      spendProviderId: "google", occurredAt,
    })).resolves.toBe("duplicate");
    const split = await getPartnerEarningsByProvider(db, partnerId, 3650);
    expect(split).toEqual([
      { providerId: "openai", events: 1, spendNano: 100_000n, earnedNano: 10_000n },
    ]);
  });
});
