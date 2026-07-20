import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { getPartnerActivity, listPartnerAnalytics } from "./analytics.js";
import type { PartnerStatus } from "./auth.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;

describe.runIf(Boolean(connectionString))("partner analytics (list + activity)", () => {
  let db: SalesDatabase;

  beforeAll(async () => { db = createSalesDatabase(connectionString!); await db.pool.query("SELECT 1"); });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, referred_topups, partner_usage_events, commission_entries, payouts, partner_discount_links, promo_codes, partner_sessions RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, referred_topups, partner_usage_events, commission_entries, payouts, partner_discount_links, promo_codes, partner_sessions RESTART IDENTITY CASCADE");
  });

  async function partner(code: string, opts: { status?: PartnerStatus; tg?: string; parent?: string } = {}): Promise<string> {
    const r = await db.pool.query<{ id: string }>(
      `INSERT INTO partners (referral_code, status, telegram_username, parent_partner_id) VALUES ($1,$2,$3,$4) RETURNING id`,
      [code, opts.status ?? "active", opts.tg ?? code, opts.parent ?? null],
    );
    return r.rows[0]!.id;
  }
  async function refer(partnerId: string, userId: string): Promise<void> {
    await db.pool.query(`INSERT INTO referred_users (commerce_user_id, partner_id, referral_code) VALUES ($1,$2,'x')`, [userId, partnerId]);
  }
  let pay = 0;
  async function deposit(partnerId: string, userId: string, nano: bigint): Promise<void> {
    pay += 1;
    await db.pool.query(
      `INSERT INTO referred_topups (commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at) VALUES ($1,$2,$3,$4, now())`,
      [`pay-${pay}`, userId, partnerId, nano.toString()],
    );
  }
  let ev = 0;
  async function earn(partnerId: string, nano: bigint): Promise<void> {
    ev += 1;
    const src = await db.pool.query<{ id: string }>(
      `INSERT INTO partner_usage_events (commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at) VALUES ($1,$2,$3,$4, now()) RETURNING id`,
      [ev, randomUUID(), partnerId, nano.toString()],
    );
    await db.pool.query(
      `INSERT INTO commission_entries (usage_event_id, partner_id, level, applied_bps, amount_nano) VALUES ($1,$2,0,1000,$3)`,
      [src.rows[0]!.id, partnerId, nano.toString()],
    );
  }
  async function payout(partnerId: string, nano: bigint, status: string): Promise<void> {
    await db.pool.query(`INSERT INTO payouts (partner_id, amount_nano, status, method) VALUES ($1,$2,$3::payout_status,'usdt-bep20')`, [partnerId, nano.toString(), status]);
  }

  it("ranks by real deposits driven and computes conversion + unpaid correctly", async () => {
    const a = await partner("aaa1", { tg: "aaa_whale" });
    const b = await partner("bbb1", { tg: "bbb_small" });
    await partner("ccc1", { status: "pending", tg: "ccc_new", parent: a }); // A's sub-partner

    const u1 = randomUUID(), u2 = randomUUID(), u3 = randomUUID();
    await refer(a, u1); await refer(a, u2); await refer(a, u3); // 3 referred
    await deposit(a, u1, 100n * USD); await deposit(a, u2, 50n * USD); // 2 converted, $150 deposits
    await earn(a, 30n * USD); await payout(a, 10n * USD, "paid"); // earned 30, committed 10 → unpaid 20
    await db.pool.query(`INSERT INTO partner_discount_links (partner_id, code, discount_bps, consumed_at) VALUES ($1,'LINKAAAAAAA1',5000, now())`, [a]);
    await db.pool.query(`INSERT INTO promo_codes (partner_id, code, value_nano) VALUES ($1,'PROMOAAA',$2)`, [a, (5n * USD).toString()]);

    const ub = randomUUID();
    await refer(b, ub); await deposit(b, ub, 20n * USD); await earn(b, 5n * USD); // unpaid 5

    const { items, totals } = await listPartnerAnalytics(db);
    expect(items.map((i) => i.referralCode)).toEqual(["aaa1", "bbb1", "ccc1"]); // deposits desc: 150, 20, 0
    const rowA = items[0]!;
    expect(rowA).toMatchObject({
      depositsTotalNano: (150n * USD).toString(),
      referredUsers: 3, convertedUsers: 2,
      earnedTotalNano: (30n * USD).toString(),
      paidNano: (10n * USD).toString(),
      unpaidNano: (20n * USD).toString(),
      teamSize: 1, linksTotal: 1, linksUsed: 1, promosTotal: 1,
    });
    expect(totals).toMatchObject({ total: 3, active: 2, depositsNano: (170n * USD).toString(), referredUsers: 4, convertedUsers: 3 });
  });

  it("filters by status and by search", async () => {
    await partner("act1", { tg: "alpha_active" });
    await partner("sus1", { tg: "beta_suspended", status: "suspended" });
    expect((await listPartnerAnalytics(db, { status: "active" })).items.map((i) => i.referralCode)).toEqual(["act1"]);
    expect((await listPartnerAnalytics(db, { status: "suspended" })).items.map((i) => i.referralCode)).toEqual(["sus1"]);
    expect((await listPartnerAnalytics(db, { search: "beta" })).items.map((i) => i.referralCode)).toEqual(["sus1"]);
    expect((await listPartnerAnalytics(db, { search: "act1" })).items.map((i) => i.referralCode)).toEqual(["act1"]);
  });

  it("sorts ascending and paginates", async () => {
    const a = await partner("p_a"); const b = await partner("p_b");
    await deposit(a, randomUUID(), 10n * USD); await deposit(b, randomUUID(), 90n * USD);
    const asc = await listPartnerAnalytics(db, { sort: "deposits_total", dir: "asc" });
    expect(asc.items.map((i) => i.referralCode)).toEqual(["p_a", "p_b"]);
    const firstPage = await listPartnerAnalytics(db, { sort: "deposits_total", dir: "desc", limit: 1, offset: 0 });
    expect(firstPage.items.map((i) => i.referralCode)).toEqual(["p_b"]);
    const secondPage = await listPartnerAnalytics(db, { sort: "deposits_total", dir: "desc", limit: 1, offset: 1 });
    expect(secondPage.items.map((i) => i.referralCode)).toEqual(["p_a"]);
  });

  it("builds a unified activity feed newest-first", async () => {
    const a = await partner("feed1");
    const u = randomUUID();
    await refer(a, u); await deposit(a, u, 40n * USD);
    await db.pool.query(`INSERT INTO partner_discount_links (partner_id, code, discount_bps) VALUES ($1,'LINKFEED0001',3000)`, [a]);
    await db.pool.query(`INSERT INTO promo_codes (partner_id, code, value_nano) VALUES ($1,'PROMOFEED',$2)`, [a, (5n * USD).toString()]);
    await db.pool.query(`INSERT INTO partner_sessions (partner_id, token_hash, expires_at) VALUES ($1,'hash', now() + interval '1 day')`, [a]);

    const feed = await getPartnerActivity(db, a, 50);
    const types = feed.map((e) => e.type);
    expect(types).toContain("referral");
    expect(types).toContain("deposit");
    expect(types).toContain("discount_link_created");
    expect(types).toContain("promo_created");
    expect(types).toContain("login");
    const deposit0 = feed.find((e) => e.type === "deposit");
    expect(deposit0?.amountNano).toBe((40n * USD).toString());
    // newest-first: timestamps must be non-increasing
    const ts = feed.map((e) => new Date(e.at).getTime());
    expect(ts).toEqual([...ts].sort((x, y) => y - x));
  });
});
