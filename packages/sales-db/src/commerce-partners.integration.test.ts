import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { deletePartnerAdmin, PartnerHasHistoryError, updatePartnerAdmin } from "./admin.js";
import { createTelegramPartner, InvalidInviteError } from "./auth.js";
import { updateDirectTeamMemberAuthority } from "./partner-authority.js";
import {
  CommercePartnerAuthorityError,
  CommercePartnerConflictError,
  createCommerceTeamInvite,
  onboardCommercePartner,
  resolveCommercePartnerMembership,
  revokeCommerceTeamInvite,
  updateCommercePartnerWallet,
} from "./commerce-partners.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("Commerce-account partner lifecycle", () => {
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

  const rootAuthority = {
    teamOverrideMaxBps: 2_000,
    teamInvitesEnabled: true,
    b2bEnabled: true,
    b2bMaxDiscountBps: 2_500,
    b2bCanDelegate: true,
  } as const;

  const futureExpiry = () => new Date(Date.now() + 30 * 24 * 60 * 60 * 1_000);

  async function root(commerceUserId = randomUUID()) {
    return onboardCommercePartner(database, {
      commerceUserId,
      commissionBps: 1_200,
      defaultSubCommissionBps: 1_000,
      authority: rootAuthority,
      actorId: "admin:test",
    });
  }

  it("creates and idempotently updates an immediate root membership without legacy powers", async () => {
    const commerceUserId = randomUUID();
    const first = await root(commerceUserId);
    expect(first.created).toBe(true);
    expect(first.partner).toMatchObject({
      commerceUserId,
      programEnabled: true,
      status: "active",
      commissionBps: 1_200,
      teamOverrideMaxBps: 2_000,
      b2bEnabled: true,
      b2bMaxDiscountBps: 2_500,
      promoEnabled: false,
      referralDiscountEnabled: false,
    });
    expect(first.partner.programStartedAt).toBeInstanceOf(Date);

    const originalStart = first.partner.programStartedAt!.getTime();
    const again = await onboardCommercePartner(database, {
      commerceUserId,
      commissionBps: 1_500,
      defaultSubCommissionBps: 1_000,
      authority: { ...rootAuthority, teamOverrideMaxBps: 1_500 },
      actorId: "admin:test-2",
    });
    expect(again.created).toBe(false);
    expect(again.partner.commissionBps).toBe(1_500);
    expect(again.partner.teamOverrideMaxBps).toBe(1_500);
    expect(again.partner.programStartedAt!.getTime()).toBe(originalStart);
    await expect(deletePartnerAdmin(database, again.partner.id, "admin:test-delete"))
      .rejects.toBeInstanceOf(PartnerHasHistoryError);
  });

  it("binds a Team invitation to one Commerce account and activates it once", async () => {
    const inviterCommerceUserId = randomUUID();
    const inviteeCommerceUserId = randomUUID();
    const inviter = await root(inviterCommerceUserId);
    const expiresAt = futureExpiry();
    const input = {
      inviterCommerceUserId,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 2_000,
      authority: {
        teamOverrideMaxBps: 1_500,
        teamInvitesEnabled: true,
        b2bEnabled: true,
        b2bMaxDiscountBps: 1_500,
        b2bCanDelegate: false,
      },
      expiresAt,
    } as const;

    const invitation = await createCommerceTeamInvite(database, input);
    expect(invitation.created).toBe(true);
    expect((await createCommerceTeamInvite(database, input)).created).toBe(false);

    const activated = await resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId });
    expect(activated.state).toBe("active");
    expect(activated.activated).toBe(true);
    if (activated.state !== "active") throw new Error("membership did not activate");
    expect(activated.partner).toMatchObject({
      commerceUserId: inviteeCommerceUserId,
      parentPartnerId: inviter.partner.id,
      commissionBps: 1_000,
      parentOverrideBps: 2_000,
      teamOverrideMaxBps: 1_500,
      promoEnabled: false,
      referralDiscountEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 1_500,
      b2bGrantSourcePartnerId: inviter.partner.id,
    });

    const replay = await resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId });
    expect(replay).toMatchObject({ state: "active", activated: false });
    const inviteRow = await database.pool.query<{
      consumed: boolean;
      revoked_at: Date | null;
    }>(`
      SELECT consumed_at IS NOT NULL AS consumed, revoked_at
      FROM partner_invites WHERE id = $1
    `, [invitation.id]);
    expect(inviteRow.rows[0]).toEqual({ consumed: true, revoked_at: null });
  });

  it("never lets the legacy Telegram flow consume an account-bound invitation", async () => {
    const inviter = await root();
    const inviteeCommerceUserId = randomUUID();
    const invitation = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });
    const code = await database.pool.query<{ code: string }>(
      "SELECT code FROM partner_invites WHERE id = $1",
      [invitation.id],
    );
    await expect(createTelegramPartner(database, {
      telegramId: "42424242",
      telegramUsername: "legacy_attempt",
      telegramPhotoUrl: null,
      displayName: "Legacy attempt",
      referralCode: `legacy-${randomUUID()}`,
      inviteCode: code.rows[0]!.code,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
    })).rejects.toBeInstanceOf(InvalidInviteError);
    await expect(resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }))
      .resolves.toMatchObject({ state: "active", activated: true });
  });

  it("never lets an inviter choose the platform direct commission or exceed delegated authority", async () => {
    const inviterCommerceUserId = randomUUID();
    await root(inviterCommerceUserId);
    const base = {
      inviterCommerceUserId,
      inviteeCommerceUserId: randomUUID(),
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: true,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    } as const;
    const invitation = await createCommerceTeamInvite(database, base);
    const row = await database.pool.query<{ commission_bps: number }>(`
      SELECT commission_bps FROM partner_invites WHERE id = $1
    `, [invitation.id]);
    expect(row.rows[0]?.commission_bps).toBe(1_000);

    await expect(createCommerceTeamInvite(database, {
      ...base,
      inviteeCommerceUserId: randomUUID(),
      overrideBps: 2_001,
    })).rejects.toBeInstanceOf(CommercePartnerAuthorityError);
    await expect(createCommerceTeamInvite(database, {
      ...base,
      inviteeCommerceUserId: randomUUID(),
      authority: { ...base.authority, b2bEnabled: true, b2bMaxDiscountBps: 3_000 },
    })).rejects.toBeInstanceOf(CommercePartnerAuthorityError);
  });

  it("keeps one account out of competing Team trees and lets admin onboarding supersede a pending invite", async () => {
    const firstRoot = await root();
    const secondRoot = await root();
    const inviteeCommerceUserId = randomUUID();
    const invite = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: firstRoot.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });
    await expect(createCommerceTeamInvite(database, {
      inviterCommerceUserId: secondRoot.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 500,
      authority: {
        teamOverrideMaxBps: 500,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    })).rejects.toBeInstanceOf(CommercePartnerConflictError);

    const onboarded = await root(inviteeCommerceUserId);
    expect(onboarded.partner.parentPartnerId).toBeNull();
    const revoked = await database.pool.query<{ revoked: boolean }>(`
      SELECT revoked_at IS NOT NULL AS revoked FROM partner_invites WHERE id = $1
    `, [invite.id]);
    expect(revoked.rows[0]?.revoked).toBe(true);
  });

  it("serializes competing invitations and concurrent activation per Commerce account", async () => {
    const firstRoot = await root();
    const secondRoot = await root();
    const inviteeCommerceUserId = randomUUID();
    const invitationInput = (inviterCommerceUserId: string, overrideBps: number) => ({
      inviterCommerceUserId,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });
    const invitationResults = await Promise.allSettled([
      createCommerceTeamInvite(database, invitationInput(firstRoot.partner.commerceUserId!, 1_000)),
      createCommerceTeamInvite(database, invitationInput(secondRoot.partner.commerceUserId!, 500)),
    ]);
    expect(invitationResults.filter((result) => result.status === "fulfilled")).toHaveLength(1);
    const rejected = invitationResults.find((result) => result.status === "rejected");
    expect(rejected).toMatchObject({ reason: expect.any(CommercePartnerConflictError) });
    const open = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM partner_invites
      WHERE commerce_user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
    `, [inviteeCommerceUserId]);
    expect(open.rows[0]?.count).toBe("1");

    const activations = await Promise.all([
      resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }),
      resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }),
    ]);
    expect(activations.map((result) => result.state)).toEqual(["active", "active"]);
    expect(activations.filter((result) => result.activated)).toHaveLength(1);
    const members = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM partners WHERE commerce_user_id = $1
    `, [inviteeCommerceUserId]);
    expect(members.rows[0]?.count).toBe("1");
  });

  it("returns unavailable without mutating an account that has no grant", async () => {
    const result = await resolveCommercePartnerMembership(database, { commerceUserId: randomUUID() });
    expect(result).toEqual({ state: "unavailable", activated: false, partner: null });
  });

  it("binds a BSC wallet and its audit evidence atomically to the active membership", async () => {
    const commerceUserId = randomUUID();
    const membership = await root(commerceUserId);
    const address = "0x1111111111111111111111111111111111111111";
    const updated = await updateCommercePartnerWallet(database, { commerceUserId, address });
    expect(updated).toMatchObject({
      payoutMethod: "usdt-bep20",
      payoutDetails: { network: "BSC", asset: "USDT (BEP-20)", address },
    });
    const audit = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM sales_audit_log
      WHERE action = 'partner.wallet_changed' AND target_id = $1
    `, [membership.partner.id]);
    expect(audit.rows[0]?.count).toBe("1");

    await updatePartnerAdmin(database, membership.partner.id, {
      programEnabled: false,
      actorId: "admin:disable-wallet-test",
    });
    await expect(updateCommercePartnerWallet(database, {
      commerceUserId,
      address: "0x2222222222222222222222222222222222222222",
    })).rejects.toMatchObject({ name: "CommercePartnerNotFoundError" });
    const unchanged = await database.pool.query<{ address: string }>(`
      SELECT payout_details->>'address' AS address FROM partners WHERE id = $1
    `, [membership.partner.id]);
    expect(unchanged.rows[0]?.address).toBe(address);
  });

  it("revokes only the owner's pending invitation and makes exact retries idempotent", async () => {
    const inviter = await root();
    const other = await root();
    const inviteeCommerceUserId = randomUUID();
    const invitation = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: true,
        b2bMaxDiscountBps: 1_500,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });

    await expect(revokeCommerceTeamInvite(database, {
      inviterCommerceUserId: other.partner.commerceUserId!,
      inviteId: invitation.id,
    })).rejects.toMatchObject({ name: "CommercePartnerNotFoundError" });
    await expect(revokeCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteId: invitation.id,
    })).resolves.toMatchObject({ revoked: true, commerceUserId: inviteeCommerceUserId });
    await expect(revokeCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteId: invitation.id,
    })).resolves.toMatchObject({ revoked: false, commerceUserId: inviteeCommerceUserId });
    await updatePartnerAdmin(database, inviter.partner.id, {
      teamOverrideMaxBps: 500,
      b2bEnabled: true,
      b2bMaxDiscountBps: 500,
      b2bCanDelegate: true,
      actorId: "admin:immutability-test",
    });
    const immutable = await database.pool.query<{
      team_override_max_bps: number;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
    }>(`
      SELECT team_override_max_bps, b2b_enabled, b2b_max_discount_bps
      FROM partner_invites WHERE id = $1
    `, [invitation.id]);
    expect(immutable.rows[0]).toEqual({
      team_override_max_bps: 1_000,
      b2b_enabled: true,
      b2b_max_discount_bps: 1_500,
    });
    await expect(resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }))
      .resolves.toEqual({ state: "unavailable", activated: false, partner: null });
  });

  it("serializes invitation activation against owner revocation without a partial terminal state", async () => {
    const inviter = await root();
    const inviteeCommerceUserId = randomUUID();
    const invitation = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });

    const [activation, revocation] = await Promise.allSettled([
      resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }),
      revokeCommerceTeamInvite(database, {
        inviterCommerceUserId: inviter.partner.commerceUserId!,
        inviteId: invitation.id,
      }),
    ]);
    expect(activation.status).toBe("fulfilled");
    if (revocation.status === "rejected") {
      expect(revocation.reason).toBeInstanceOf(CommercePartnerConflictError);
    }

    const terminal = await database.pool.query<{
      consumed: boolean;
      revoked: boolean;
      members: string;
    }>(`
      SELECT invite.consumed_at IS NOT NULL AS consumed,
             invite.revoked_at IS NOT NULL AS revoked,
             (SELECT count(*)::text FROM partners WHERE commerce_user_id = $2) AS members
      FROM partner_invites invite WHERE invite.id = $1
    `, [invitation.id, inviteeCommerceUserId]);
    expect(terminal.rows[0]?.consumed).not.toBe(terminal.rows[0]?.revoked);
    expect(terminal.rows[0]?.members).toBe(terminal.rows[0]?.consumed ? "1" : "0");
  });

  it("revokes pending account invitations when an admin disables the inviter", async () => {
    const inviter = await root();
    const inviteeCommerceUserId = randomUUID();
    const invitation = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteeCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: new Date(Date.now() + 30 * 24 * 60 * 60 * 1_000),
    });
    await updatePartnerAdmin(database, inviter.partner.id, {
      programEnabled: false,
      actorId: "admin:disable-test",
    });
    const row = await database.pool.query<{ revoked: boolean }>(`
      SELECT revoked_at IS NOT NULL AS revoked FROM partner_invites WHERE id = $1
    `, [invitation.id]);
    expect(row.rows[0]?.revoked).toBe(true);
    await expect(resolveCommercePartnerMembership(database, { commerceUserId: inviteeCommerceUserId }))
      .resolves.toEqual({ state: "unavailable", activated: false, partner: null });
  });

  it("permanently revokes a member's pending invitations when their Team authority is removed", async () => {
    const inviter = await root();
    const memberCommerceUserId = randomUUID();
    await createCommerceTeamInvite(database, {
      inviterCommerceUserId: inviter.partner.commerceUserId!,
      inviteeCommerceUserId: memberCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 1_000,
      authority: {
        teamOverrideMaxBps: 1_000,
        teamInvitesEnabled: true,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });
    const activatedMember = await resolveCommercePartnerMembership(database, {
      commerceUserId: memberCommerceUserId,
    });
    if (activatedMember.state !== "active") throw new Error("member did not activate");

    const leafCommerceUserId = randomUUID();
    const leafInvite = await createCommerceTeamInvite(database, {
      inviterCommerceUserId: memberCommerceUserId,
      inviteeCommerceUserId: leafCommerceUserId,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
      overrideBps: 500,
      authority: {
        teamOverrideMaxBps: 500,
        teamInvitesEnabled: false,
        b2bEnabled: false,
        b2bMaxDiscountBps: 0,
        b2bCanDelegate: false,
      },
      expiresAt: futureExpiry(),
    });

    await updateDirectTeamMemberAuthority(database, {
      parentPartnerId: inviter.partner.id,
      memberId: activatedMember.partner.id,
      teamInvitesEnabled: false,
      requireProgramEnabled: true,
    });
    await updateDirectTeamMemberAuthority(database, {
      parentPartnerId: inviter.partner.id,
      memberId: activatedMember.partner.id,
      teamInvitesEnabled: true,
      requireProgramEnabled: true,
    });

    const stored = await database.pool.query<{ revoked: boolean }>(`
      SELECT revoked_at IS NOT NULL AS revoked FROM partner_invites WHERE id = $1
    `, [leafInvite.id]);
    expect(stored.rows[0]?.revoked).toBe(true);
    await expect(resolveCommercePartnerMembership(database, { commerceUserId: leafCommerceUserId }))
      .resolves.toEqual({ state: "unavailable", activated: false, partner: null });
  });
});
