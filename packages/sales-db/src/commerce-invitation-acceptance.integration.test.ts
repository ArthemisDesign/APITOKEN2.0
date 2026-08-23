import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  declineCommercePartnerInvitation,
  findPendingCommercePartnerInvitation,
  resolveCommercePartnerMembership,
} from "./commerce-partners.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

/**
 * An invitation is an offer, not a fact: reading the partner surface must never turn one into a
 * membership. These cases exercise the real SQL for the whole decision.
 */
describe.runIf(Boolean(connectionString))("Team invitation acceptance", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
  });

  async function truncate(): Promise<void> {
    await database.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function inviter(): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(
        referral_code, status, commission_bps, sub_commission_bps, team_override_max_bps,
        commerce_user_id, program_enabled, program_started_at, team_invites_enabled,
        b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      )
      VALUES($1, 'active', 1000, 1000, 2000, $2, true, now(), true, true, 2500, true)
      RETURNING id
    `, [`inviter-${randomUUID().slice(0, 8)}`, randomUUID()]);
    return result.rows[0]!.id;
  }

  async function invite(input: { partnerId: string; commerceUserId: string; parentOverrideBps?: number }): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_invites(
        code, partner_id, commerce_user_id, commission_bps, sub_commission_bps,
        team_override_max_bps, parent_override_bps, team_invites_enabled,
        b2b_enabled, b2b_max_discount_bps, b2b_can_delegate, expires_at
      )
      VALUES($1, $2, $3, 1000, 1000, 1000, $4, true, true, 1500, false, now() + interval '30 days')
      RETURNING id
    `, [`invite-${randomUUID().slice(0, 8)}`, input.partnerId, input.commerceUserId, input.parentOverrideBps ?? 1500]);
    return result.rows[0]!.id;
  }

  async function partnerCount(commerceUserId: string): Promise<number> {
    const result = await database.pool.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM partners WHERE commerce_user_id = $1",
      [commerceUserId],
    );
    return Number(result.rows[0]!.count);
  }

  it("reads the pending terms without consuming the invitation", async () => {
    const commerceUserId = randomUUID();
    const inviteId = await invite({ partnerId: await inviter(), commerceUserId });

    const pending = await findPendingCommercePartnerInvitation(database, commerceUserId);

    expect(pending).toMatchObject({ id: inviteId, commissionBps: 1_000 });
    expect(pending?.parentOverrideBps).toBe(1_500);
    expect(pending?.teamOverrideMaxBps).toBe(1_000);
    expect(pending?.b2bEnabled).toBe(true);
    expect(pending?.b2bMaxDiscountBps).toBe(1_500);
    expect(await partnerCount(commerceUserId)).toBe(0);
  });

  it("does not activate a pending invitation while only resolving state", async () => {
    const commerceUserId = randomUUID();
    await invite({ partnerId: await inviter(), commerceUserId });

    const resolution = await resolveCommercePartnerMembership(database, { commerceUserId, activate: false });

    expect(resolution.state).toBe("unavailable");
    expect(resolution.activated).toBe(false);
    expect(await partnerCount(commerceUserId)).toBe(0);
    expect(await findPendingCommercePartnerInvitation(database, commerceUserId)).not.toBeNull();
  });

  it("creates the membership on the invited terms only when the invitee accepts", async () => {
    const commerceUserId = randomUUID();
    const parentId = await inviter();
    await invite({ partnerId: parentId, commerceUserId, parentOverrideBps: 1_200 });

    const accepted = await resolveCommercePartnerMembership(database, { commerceUserId, activate: true });

    expect(accepted.state).toBe("active");
    expect(accepted.activated).toBe(true);
    expect(accepted.partner).toMatchObject({
      commerceUserId,
      commissionBps: 1_000,
      parentPartnerId: parentId,
      parentOverrideBps: 1_200,
      teamOverrideMaxBps: 1_000,
      b2bEnabled: true,
      b2bMaxDiscountBps: 1_500,
    });
    expect(await findPendingCommercePartnerInvitation(database, commerceUserId)).toBeNull();
    expect(await partnerCount(commerceUserId)).toBe(1);
  });

  it("declines only the invitee's own invitation and leaves the account without a membership", async () => {
    const commerceUserId = randomUUID();
    const otherUserId = randomUUID();
    const parentId = await inviter();
    const inviteId = await invite({ partnerId: parentId, commerceUserId });
    await invite({ partnerId: parentId, commerceUserId: otherUserId });

    const wrongOwner = await declineCommercePartnerInvitation(database, { commerceUserId: otherUserId, inviteId });
    expect(wrongOwner.declined).toBe(false);

    const declined = await declineCommercePartnerInvitation(database, { commerceUserId, inviteId });
    expect(declined.declined).toBe(true);
    expect(await findPendingCommercePartnerInvitation(database, commerceUserId)).toBeNull();
    expect(await findPendingCommercePartnerInvitation(database, otherUserId)).not.toBeNull();

    const afterDecline = await resolveCommercePartnerMembership(database, { commerceUserId, activate: true });
    expect(afterDecline.state).toBe("unavailable");
    expect(await partnerCount(commerceUserId)).toBe(0);
  });

  it("cannot be declined twice", async () => {
    const commerceUserId = randomUUID();
    const inviteId = await invite({ partnerId: await inviter(), commerceUserId });

    expect((await declineCommercePartnerInvitation(database, { commerceUserId, inviteId })).declined).toBe(true);
    expect((await declineCommercePartnerInvitation(database, { commerceUserId, inviteId })).declined).toBe(false);
  });
});
