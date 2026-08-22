import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { decideApplication, submitApplication } from "./applications.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("application authority onboarding", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partner_applications, partners, sales_audit_log RESTART IDENTITY CASCADE");
  });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partner_applications, partners, sales_audit_log RESTART IDENTITY CASCADE");
    await db.pool.end();
  });

  it("creates the root partner with every approved boundary in the same transaction", async () => {
    const application = await submitApplication(db, {
      telegramId: "88110022",
      telegramUsername: "atomic_partner",
      displayName: "Atomic Partner",
      telegramPhotoUrl: null,
      note: "enterprise pipeline",
    });
    const result = await decideApplication(db, {
      applicationId: application.id,
      action: "approve",
      referralCode: "atomic-root",
      commissionBps: 1_500,
      subCommissionBps: 800,
      teamOverrideMaxBps: 1_300,
      teamInvitesEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 6_500,
      b2bCanDelegate: true,
      adminNote: "approved atomically",
      actorId: "admin@example.com",
    });
    expect(result.partnerId).toBeTypeOf("string");
    const partner = await db.pool.query(`
      SELECT commission_bps, sub_commission_bps, team_override_max_bps,
             team_invites_enabled, b2b_enabled, b2b_max_discount_bps,
             b2b_can_delegate, b2b_grant_source_partner_id
      FROM partners WHERE id = $1
    `, [result.partnerId]);
    expect(partner.rows[0]).toEqual({
      commission_bps: 1_500,
      sub_commission_bps: 800,
      team_override_max_bps: 1_300,
      team_invites_enabled: false,
      b2b_enabled: true,
      b2b_max_discount_bps: 6_500,
      b2b_can_delegate: true,
      b2b_grant_source_partner_id: null,
    });
    const audit = await db.pool.query<{ metadata: Record<string, unknown> }>(`
      SELECT metadata FROM sales_audit_log WHERE action = 'application.approved'
    `);
    expect(audit.rows[0]?.metadata).toMatchObject({
      partnerId: result.partnerId,
      teamOverrideMaxBps: 1_300,
      b2bMaxDiscountBps: 6_500,
      b2bCanDelegate: true,
    });
  });

  it("rolls back the decision when an authority constraint rejects partner creation", async () => {
    const application = await submitApplication(db, {
      telegramId: "88110023",
      telegramUsername: "invalid_authority",
      displayName: "Invalid Authority",
      telegramPhotoUrl: null,
      note: "must remain pending",
    });
    await expect(decideApplication(db, {
      applicationId: application.id,
      action: "approve",
      referralCode: "invalid-authority",
      commissionBps: 1_000,
      subCommissionBps: 500,
      teamOverrideMaxBps: 1_000,
      teamInvitesEnabled: true,
      b2bEnabled: false,
      b2bMaxDiscountBps: 1_000,
      b2bCanDelegate: false,
      adminNote: "must roll back",
      actorId: "admin@example.com",
    })).rejects.toThrow();

    const state = await db.pool.query<{ status: string; created_partner_id: string | null }>(`
      SELECT status, created_partner_id FROM partner_applications WHERE id = $1
    `, [application.id]);
    expect(state.rows[0]).toEqual({ status: "pending", created_partner_id: null });
    expect((await db.pool.query("SELECT 1 FROM partners WHERE telegram_id = '88110023'")).rowCount).toBe(0);
    expect((await db.pool.query("SELECT 1 FROM sales_audit_log WHERE target_id = $1", [application.id])).rowCount).toBe(0);
  });

  it("does not approve with unapplied terms when another onboarding path already created the partner", async () => {
    const application = await submitApplication(db, {
      telegramId: "88110024",
      telegramUsername: "existing_partner",
      displayName: "Existing Partner",
      telegramPhotoUrl: null,
      note: "conflicting onboarding",
    });
    await db.pool.query(`
      INSERT INTO partners (telegram_id, telegram_username, referral_code, status, commission_bps)
      VALUES ('88110024', 'existing_partner', 'already-created', 'active', 777)
    `);

    await expect(decideApplication(db, {
      applicationId: application.id,
      action: "approve",
      referralCode: "must-not-link",
      commissionBps: 1_500,
      subCommissionBps: 800,
      teamOverrideMaxBps: 1_300,
      teamInvitesEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 6_500,
      b2bCanDelegate: true,
      adminNote: "must not claim success",
      actorId: "admin@example.com",
    })).rejects.toThrow("partner account already exists");

    const state = await db.pool.query<{ status: string; created_partner_id: string | null }>(`
      SELECT status, created_partner_id FROM partner_applications WHERE id = $1
    `, [application.id]);
    expect(state.rows[0]).toEqual({ status: "pending", created_partner_id: null });
    const partner = await db.pool.query<{ commission_bps: number }>(`
      SELECT commission_bps FROM partners WHERE telegram_id = '88110024'
    `);
    expect(partner.rows[0]?.commission_bps).toBe(777);
  });
});
