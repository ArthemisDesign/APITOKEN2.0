import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { recordReferredSpend, recordReferredDeposit, reconcilePendingReferralEvents } from "./commissions.js";
import { upsertReferredUser } from "./referrals.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;

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
    const r = await db.pool.query<{ id: string }>("INSERT INTO partners (referral_code, status, telegram_username, commission_bps) VALUES ($1,'active',$1,1000) RETURNING id", [code]);
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

  it("buffers a spend that arrives before attribution, then reconciles it into commission", async () => {
    const user = randomUUID();
    // spend arrives BEFORE the user is attributed
    const out = await recordReferredSpend(db, { commerceEventId: 501n, commerceUserId: user, amountNano: 100n * USD, occurredAt: new Date() });
    expect(out).toBe("buffered");
    expect(await pendingCount()).toBe(1);

    // now the attribution arrives
    const p = await partner("late1");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late1", attributedAt: new Date(), sourceAttributionId: 1n });

    const replayed = await reconcilePendingReferralEvents(db);
    expect(replayed).toBe(1);
    expect(await pendingCount()).toBe(0);
    expect(await commissionTotal(p)).toBe(10n * USD); // 10% of $100
  });

  it("buffers a deposit that arrives before attribution, then reconciles it into referred_topups", async () => {
    const user = randomUUID();
    const out = await recordReferredDeposit(db, { commercePaymentId: "pay-late-1", commerceUserId: user, amountNano: 40n * USD, paidAt: new Date() });
    expect(out).toBe("buffered");

    const p = await partner("late2");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late2", attributedAt: new Date(), sourceAttributionId: 2n });
    expect(await reconcilePendingReferralEvents(db)).toBe(1);

    const dep = await db.pool.query<{ t: string }>("SELECT COALESCE(SUM(amount_nano),0)::text AS t FROM referred_topups WHERE partner_id = $1", [p]);
    expect(BigInt(dep.rows[0]!.t)).toBe(40n * USD);
    expect(await pendingCount()).toBe(0);
  });

  it("leaves the buffer untouched while the user is still unattributed, and is idempotent", async () => {
    const user = randomUUID();
    await recordReferredSpend(db, { commerceEventId: 777n, commerceUserId: user, amountNano: 5n * USD, occurredAt: new Date() });
    // no attribution yet → reconcile finds nothing to do
    expect(await reconcilePendingReferralEvents(db)).toBe(0);
    expect(await pendingCount()).toBe(1);

    // buffering the same event again is idempotent (no duplicate row)
    await recordReferredSpend(db, { commerceEventId: 777n, commerceUserId: user, amountNano: 5n * USD, occurredAt: new Date() });
    expect(await pendingCount()).toBe(1);

    // attribute + reconcile once
    const p = await partner("late3");
    await upsertReferredUser(db, { commerceUserId: user, partnerId: p, referralCode: "late3", attributedAt: new Date(), sourceAttributionId: 3n });
    expect(await reconcilePendingReferralEvents(db)).toBe(1);
    // running again does nothing (buffer already drained)
    expect(await reconcilePendingReferralEvents(db)).toBe(0);
    expect(await commissionTotal(p)).toBe((5n * USD) / 10n);
  });
});
