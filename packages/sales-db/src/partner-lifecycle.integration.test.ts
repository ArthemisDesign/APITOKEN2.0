import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { upsertReferredUser } from "./referrals.js";
import { deletePartnerAdmin, PartnerHasHistoryError } from "./admin.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("partner lifecycle guards", () => {
  let db: SalesDatabase;
  beforeAll(async () => { db = createSalesDatabase(connectionString!); await db.pool.query("SELECT 1"); });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, promo_codes, partner_discount_links RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, referred_users, promo_codes, partner_discount_links RESTART IDENTITY CASCADE");
  });

  async function partner(code: string): Promise<string> {
    const r = await db.pool.query<{ id: string }>(
      "INSERT INTO partners (referral_code, status, telegram_username) VALUES ($1,'active',$1) RETURNING id", [code]);
    return r.rows[0]!.id;
  }

  // B3: upsertReferredUser must report whether THIS attribution actually won (first-wins).
  it("upsertReferredUser returns true only for the winning attribution", async () => {
    const a = await partner("aaa"); const b = await partner("bbb");
    const user = randomUUID();
    const first = await upsertReferredUser(db, { commerceUserId: user, partnerId: a, referralCode: "aaa", attributedAt: new Date(), sourceAttributionId: 1n });
    const second = await upsertReferredUser(db, { commerceUserId: user, partnerId: b, referralCode: "bbb", attributedAt: new Date(), sourceAttributionId: 2n });
    expect(first).toBe(true);
    expect(second).toBe(false); // user already under A → B's attribution is a no-op
    const owner = await db.pool.query<{ partner_id: string }>("SELECT partner_id FROM referred_users WHERE commerce_user_id = $1", [user]);
    expect(owner.rows[0]!.partner_id).toBe(a);
  });

  // B5: delete guard must block a partner who has promo codes or discount links (even with no referrals).
  it("delete guard blocks a partner with only promo codes", async () => {
    const p = await partner("promoonly");
    await db.pool.query("INSERT INTO promo_codes (partner_id, code, value_nano) VALUES ($1,'PROMOX1',1000000000)", [p]);
    await expect(deletePartnerAdmin(db, p, "test-admin")).rejects.toBeInstanceOf(PartnerHasHistoryError);
    const still = await db.pool.query("SELECT 1 FROM partners WHERE id = $1", [p]);
    expect(still.rowCount).toBe(1);
  });

  it("delete guard blocks a partner with only discount links", async () => {
    const p = await partner("linkonly");
    await db.pool.query("INSERT INTO partner_discount_links (partner_id, code, discount_bps) VALUES ($1,'LINKX0000001',3000)", [p]);
    await expect(deletePartnerAdmin(db, p, "test-admin")).rejects.toBeInstanceOf(PartnerHasHistoryError);
  });

  it("delete guard allows a clean partner with no history", async () => {
    const p = await partner("clean");
    await expect(deletePartnerAdmin(db, p, "test-admin")).resolves.toBe(true);
    const gone = await db.pool.query("SELECT 1 FROM partners WHERE id = $1", [p]);
    expect(gone.rowCount).toBe(0);
  });
});
