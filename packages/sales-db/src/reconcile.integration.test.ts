import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  recordReferredSpend,
  recordReferredDeposit,
  reconcilePendingReferralEvents,
  ReferredSpendAttributionError,
  ReferralEventReplayConflictError,
  type ReferredSpendAttribution,
} from "./commissions.js";
import { upsertReferredUser } from "./referrals.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;
const ATTRIBUTED_AT = new Date("2026-07-01T00:00:00.000Z");

describe.runIf(Boolean(connectionString))("pre-attribution buffer + reconcile (D1)", () => {
  let db: SalesDatabase;
  beforeAll(async () => { db = createSalesDatabase(connectionString!); await db.pool.query("SELECT 1"); });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, referred_topups, partner_usage_events, commission_entries, pending_referral_events RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, referred_topups, partner_usage_events, commission_entries, pending_referral_events RESTART IDENTITY CASCADE");
  });

  async function partner(code: string): Promise<string> {
    const r = await db.pool.query<{ id: string }>(`
      INSERT INTO partners (
        referral_code, status, telegram_username, commission_bps,
        commerce_user_id, program_enabled, program_started_at
      ) VALUES ($1, 'active', $1, 1000, gen_random_uuid(), true, '2020-01-01')
      RETURNING id
    `, [code]);
    return r.rows[0]!.id;
  }
  async function pendingCount(): Promise<number> {
    const r = await db.pool.query<{ n: string }>("SELECT count(*)::text AS n FROM pending_referral_events");
    return Number(r.rows[0]!.n);
  }
  async function commissionTotal(partnerId: string): Promise<bigint> {
    const r = await db.pool.query<{ t: string }>("SELECT COALESCE(SUM(amount_nano),0)::text AS t FROM commission_entries WHERE partner_id = $1", [partnerId]);
    return BigInt(r.rows[0]!.t);
  }
  function attribution(amountNano: bigint, snapshotDigest: string): ReferredSpendAttribution {
    return {
      providerId: "anthropic",
      accountClass: "b2c",
      pricingMode: "track",
      paidFundedNano: amountNano,
      commissionEligible: true,
      snapshotDigest,
    };
  }

  it("buffers a spend that arrives before attribution, then reconciles it into commission", async () => {
    const user = randomUUID();
    const amountNano = 100n * USD;
    const immutableAttribution = attribution(amountNano, "snapshot-buffered-501");
    // spend arrives BEFORE the user is attributed
    const out = await recordReferredSpend(db, {
      commerceEventId: 501n,
      commerceUserId: user,
      amountNano,
      attribution: immutableAttribution,
      occurredAt: new Date(),
    });
    expect(out).toBe("buffered");
    expect(await pendingCount()).toBe(1);
    await expect(db.pool.query(`
      SELECT provider_id, account_class, pricing_mode, paid_funded_nano::text,
             commission_eligible, snapshot_digest
      FROM pending_referral_events WHERE commerce_ref = '501'
    `)).resolves.toMatchObject({ rows: [{
      provider_id: "anthropic",
      account_class: "b2c",
      pricing_mode: "track",
      paid_funded_nano: amountNano.toString(),
      commission_eligible: true,
      snapshot_digest: "snapshot-buffered-501",
    }] });

    // now the attribution arrives
    const p = await partner("late1");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late1", attributedAt: ATTRIBUTED_AT, sourceAttributionId: 1n });

    const replayed = await reconcilePendingReferralEvents(db);
    expect(replayed).toBe(1);
    expect(await pendingCount()).toBe(0);
    expect(await commissionTotal(p)).toBe(10n * USD); // 10% of $100
    await expect(db.pool.query(`
      SELECT amount_nano::text, provider_id, account_class, pricing_mode,
             paid_funded_nano::text, commission_eligible, snapshot_digest
      FROM partner_usage_events WHERE commerce_event_id = 501
    `)).resolves.toMatchObject({ rows: [{
      amount_nano: amountNano.toString(),
      provider_id: "anthropic",
      account_class: "b2c",
      pricing_mode: "track",
      paid_funded_nano: amountNano.toString(),
      commission_eligible: true,
      snapshot_digest: "snapshot-buffered-501",
    }] });
  });

  it("buffers a deposit that arrives before attribution, then reconciles it into referred_topups", async () => {
    const user = randomUUID();
    const out = await recordReferredDeposit(db, { commercePaymentId: "pay-late-1", commerceUserId: user, amountNano: 40n * USD, paidAt: new Date() });
    expect(out).toBe("buffered");

    const p = await partner("late2");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late2", attributedAt: ATTRIBUTED_AT, sourceAttributionId: 2n });
    expect(await reconcilePendingReferralEvents(db)).toBe(1);

    const dep = await db.pool.query<{ t: string }>("SELECT COALESCE(SUM(amount_nano),0)::text AS t FROM referred_topups WHERE partner_id = $1", [p]);
    expect(BigInt(dep.rows[0]!.t)).toBe(40n * USD);
    expect(await pendingCount()).toBe(0);
  });

  it("leaves the buffer untouched while the user is still unattributed, and is idempotent", async () => {
    const user = randomUUID();
    const occurredAt = new Date("2026-08-01T09:00:00.000Z");
    await recordReferredSpend(db, { commerceEventId: 777n, commerceUserId: user, amountNano: 5n * USD, occurredAt });
    // no attribution yet → reconcile finds nothing to do
    expect(await reconcilePendingReferralEvents(db)).toBe(0);
    expect(await pendingCount()).toBe(1);

    // buffering the same event again is idempotent (no duplicate row)
    await recordReferredSpend(db, { commerceEventId: 777n, commerceUserId: user, amountNano: 5n * USD, occurredAt });
    expect(await pendingCount()).toBe(1);

    // attribute + reconcile once
    const p = await partner("late3");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late3", attributedAt: ATTRIBUTED_AT, sourceAttributionId: 3n });
    expect(await reconcilePendingReferralEvents(db)).toBe(1);
    // running again does nothing (buffer already drained)
    expect(await reconcilePendingReferralEvents(db)).toBe(0);
    expect(await commissionTotal(p)).toBe((5n * USD) / 10n);
  });

  it("stores exact attributed paid basis and keeps replay idempotent without downgrading authority", async () => {
    const user = randomUUID();
    const p = await partner("direct1");
    await upsertReferredUser(db, {
      commerceUserId: user,
      partnerId: p,
      referralCode: "direct1",
      attributedAt: ATTRIBUTED_AT,
      sourceAttributionId: 10n,
    });
    const occurredAt = new Date("2026-08-01T10:00:00.000Z");
    const immutableAttribution = attribution(7n * USD, "snapshot-direct-900");
    await expect(recordReferredSpend(db, {
      commerceEventId: 900n,
      commerceUserId: user,
      amountNano: 7n * USD,
      attribution: immutableAttribution,
      occurredAt,
    })).resolves.toBe("recorded");
    await expect(recordReferredSpend(db, {
      commerceEventId: 900n,
      commerceUserId: user,
      amountNano: 7n * USD,
      attribution: immutableAttribution,
      occurredAt,
    })).resolves.toBe("duplicate");
    // A delayed old-consumer replay is compatible but cannot erase the stored fields.
    await expect(recordReferredSpend(db, {
      commerceEventId: 900n,
      commerceUserId: user,
      amountNano: 7n * USD,
      occurredAt,
    })).resolves.toBe("duplicate");

    expect(await commissionTotal(p)).toBe(700_000_000n);
    await expect(db.pool.query(`
      SELECT count(*)::int AS count, min(snapshot_digest) AS snapshot_digest
      FROM partner_usage_events WHERE commerce_event_id = 900
    `)).resolves.toMatchObject({ rows: [{ count: 1, snapshot_digest: "snapshot-direct-900" }] });
  });

  it("upgrades an all-null buffered replay once and preserves it through reconcile", async () => {
    const user = randomUUID();
    const occurredAt = new Date("2026-08-01T11:00:00.000Z");
    await expect(recordReferredSpend(db, {
      commerceEventId: 901n,
      commerceUserId: user,
      amountNano: 3n * USD,
      occurredAt,
    })).resolves.toBe("buffered");
    await expect(recordReferredSpend(db, {
      commerceEventId: 901n,
      commerceUserId: user,
      amountNano: 3n * USD,
      attribution: attribution(3n * USD, "snapshot-upgraded-901"),
      occurredAt,
    })).resolves.toBe("buffered");

    const buffered = await db.pool.query<{ snapshot_digest: string | null }>(
      "SELECT snapshot_digest FROM pending_referral_events WHERE commerce_ref = '901'",
    );
    expect(buffered.rows[0]?.snapshot_digest).toBe("snapshot-upgraded-901");

    const p = await partner("upgrade1");
    await upsertReferredUser(db, {
      commerceUserId: user,
      partnerId: p,
      referralCode: "upgrade1",
      attributedAt: ATTRIBUTED_AT,
      sourceAttributionId: 11n,
    });
    expect(await reconcilePendingReferralEvents(db)).toBe(1);
    await expect(db.pool.query(
      "SELECT snapshot_digest FROM partner_usage_events WHERE commerce_event_id = 901",
    )).resolves.toMatchObject({ rows: [{ snapshot_digest: "snapshot-upgraded-901" }] });
  });

  it("rejects malformed authority and conflicting immutable replays", async () => {
    const user = randomUUID();
    const p = await partner("conflict1");
    await upsertReferredUser(db, {
      commerceUserId: user,
      partnerId: p,
      referralCode: "conflict1",
      attributedAt: ATTRIBUTED_AT,
      sourceAttributionId: 12n,
    });
    const occurredAt = new Date("2026-08-01T12:00:00.000Z");
    await expect(recordReferredSpend(db, {
      commerceEventId: 902n,
      commerceUserId: user,
      amountNano: 5n * USD,
      attribution: { ...attribution(4n * USD, "snapshot-wrong-amount") },
      occurredAt,
    })).rejects.toBeInstanceOf(ReferredSpendAttributionError);
    await expect(recordReferredSpend(db, {
      commerceEventId: 902n,
      commerceUserId: user,
      amountNano: 5n * USD,
      attribution: {
        ...attribution(5n * USD, "snapshot-service"),
        accountClass: "service",
      } as unknown as ReferredSpendAttribution,
      occurredAt,
    })).rejects.toBeInstanceOf(ReferredSpendAttributionError);

    await expect(recordReferredSpend(db, {
      commerceEventId: 902n,
      commerceUserId: user,
      amountNano: 5n * USD,
      attribution: attribution(5n * USD, "snapshot-original-902"),
      occurredAt,
    })).resolves.toBe("recorded");
    await expect(recordReferredSpend(db, {
      commerceEventId: 902n,
      commerceUserId: user,
      amountNano: 5n * USD,
      attribution: attribution(5n * USD, "snapshot-conflict-902"),
      occurredAt,
    })).rejects.toBeInstanceOf(ReferralEventReplayConflictError);
    expect(await commissionTotal(p)).toBe(500_000_000n);
  });
});
