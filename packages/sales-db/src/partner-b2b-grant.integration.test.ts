import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { createPartnerInvite } from "./invites.js";
import { createTelegramPartner, getPartner } from "./auth.js";
import { updatePartnerAdmin } from "./admin.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

// The B2B grant lets a partner turn their OWN referrals into B2B customers, no deeper than the
// ceiling an admin set. Everything here guards one property: the grant is an explicit exception,
// and a ceiling never exists without the grant that gives it meaning.
describe.runIf(Boolean(connectionString))("partner B2B grant", () => {
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
      TRUNCATE partner_invites, sales_audit_log, partners RESTART IDENTITY CASCADE
    `);
  }

  async function invite(overrides: { b2bEnabled?: boolean; b2bMaxDiscountBps?: number } = {}) {
    return createPartnerInvite(db, {
      partnerId: null,
      code: `inv${randomUUID().slice(0, 8)}`,
      telegramUsername: "grantee",
      commissionBps: 1000,
      subCommissionBps: 1000,
      promoEnabled: false,
      promoMaxValueNano: 0n,
      promoMaxCount: 0,
      referralDiscountBps: 0,
      referralDiscountEnabled: false,
      expiresAt: new Date(Date.now() + 86_400_000),
      ...overrides,
    });
  }

  async function partnerFromInvite(code: string, telegramId: string) {
    return createTelegramPartner(db, {
      telegramId,
      telegramUsername: "grantee",
      telegramPhotoUrl: null,
      displayName: null,
      referralCode: `ref${randomUUID().slice(0, 8)}`,
      inviteCode: code,
      defaultCommissionBps: 1000,
      defaultSubCommissionBps: 1000,
    });
  }

  it("onboards an ordinary partner with no B2B right by default", async () => {
    const created = await invite();
    expect(created.b2bEnabled).toBe(false);
    expect(created.b2bMaxDiscountBps).toBe(0);

    const partner = await partnerFromInvite(created.code, "5001");
    // The default partner brings plain B2C customers — nothing about B2B is implied by joining.
    expect(partner.b2bEnabled).toBe(false);
    expect(partner.b2bMaxDiscountBps).toBe(0);
  });

  it("carries the grant from the invite so onboarding is a single step", async () => {
    const created = await invite({ b2bEnabled: true, b2bMaxDiscountBps: 7000 });
    const partner = await partnerFromInvite(created.code, "5002");
    expect(partner.b2bEnabled).toBe(true);
    expect(partner.b2bMaxDiscountBps).toBe(7000);
  });

  it("ignores a ceiling on an invite that does not grant the right", async () => {
    // A ceiling without the grant is not a smaller permission — it is none at all.
    const created = await invite({ b2bEnabled: false, b2bMaxDiscountBps: 9000 });
    expect(created.b2bMaxDiscountBps).toBe(0);
    const partner = await partnerFromInvite(created.code, "5003");
    expect(partner.b2bEnabled).toBe(false);
    expect(partner.b2bMaxDiscountBps).toBe(0);
  });

  it("grants and re-ceilings an existing partner", async () => {
    const created = await invite();
    const partner = await partnerFromInvite(created.code, "5004");

    await updatePartnerAdmin(db, partner.id, {
      b2bEnabled: true, b2bMaxDiscountBps: 6000, actorId: "test-admin",
    });
    expect(await getPartner(db, partner.id)).toMatchObject({ b2bEnabled: true, b2bMaxDiscountBps: 6000 });

    await updatePartnerAdmin(db, partner.id, { b2bMaxDiscountBps: 8000, actorId: "test-admin" });
    expect(await getPartner(db, partner.id)).toMatchObject({ b2bEnabled: true, b2bMaxDiscountBps: 8000 });
  });

  it("clears the ceiling when the grant is revoked", async () => {
    const created = await invite({ b2bEnabled: true, b2bMaxDiscountBps: 7000 });
    const partner = await partnerFromInvite(created.code, "5005");

    await updatePartnerAdmin(db, partner.id, { b2bEnabled: false, actorId: "test-admin" });
    // A ceiling left behind on a revoked grant would read like authority that no longer exists.
    expect(await getPartner(db, partner.id)).toMatchObject({ b2bEnabled: false, b2bMaxDiscountBps: 0 });
  });

  it("keeps unrelated partner edits from disturbing the grant", async () => {
    const created = await invite({ b2bEnabled: true, b2bMaxDiscountBps: 5000 });
    const partner = await partnerFromInvite(created.code, "5006");

    await updatePartnerAdmin(db, partner.id, { commissionBps: 2500, actorId: "test-admin" });
    expect(await getPartner(db, partner.id)).toMatchObject({
      commissionBps: 2500, b2bEnabled: true, b2bMaxDiscountBps: 5000,
    });
  });

  it("records every grant change in the audit trail", async () => {
    const created = await invite();
    const partner = await partnerFromInvite(created.code, "5007");
    await updatePartnerAdmin(db, partner.id, {
      b2bEnabled: true, b2bMaxDiscountBps: 6500, actorId: "test-admin",
    });
    const audit = await db.pool.query<{ metadata: { b2bEnabled: boolean; b2bMaxDiscountBps: number } }>(
      `SELECT metadata FROM sales_audit_log
       WHERE action = 'partner.updated' AND target_id = $1 ORDER BY id DESC LIMIT 1`,
      [partner.id],
    );
    // Margin decisions must be reconstructable from the trail, not only from the current row.
    expect(audit.rows[0]!.metadata).toMatchObject({ b2bEnabled: true, b2bMaxDiscountBps: 6500 });
  });

  it("refuses a ceiling above the 95% policy maximum", async () => {
    const created = await invite();
    const partner = await partnerFromInvite(created.code, "5008");
    await expect(updatePartnerAdmin(db, partner.id, {
      b2bEnabled: true, b2bMaxDiscountBps: 9600, actorId: "test-admin",
    })).rejects.toThrow();
  });
});
