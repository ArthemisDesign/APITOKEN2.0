import { randomBytes } from "node:crypto";
import type { PoolClient } from "pg";
import type { Partner } from "./auth.js";
import { getPartner } from "./auth.js";
import type { SalesDatabase } from "./client.js";
import { lowerTeamOverrideCeiling } from "./commissions.js";
import { applyPartnerB2BAuthorityCascade } from "./partner-authority.js";

export const COMMERCE_TEAM_SHARE_MAX_BPS = 2_000;

export class CommercePartnerConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CommercePartnerConflictError";
  }
}

export class CommercePartnerAuthorityError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CommercePartnerAuthorityError";
  }
}

export class CommercePartnerNotFoundError extends Error {
  constructor(message = "Commerce partner membership not found") {
    super(message);
    this.name = "CommercePartnerNotFoundError";
  }
}

export type CommercePartnerMembershipResolution =
  | { state: "unavailable"; activated: false; partner: null }
  | { state: "disabled"; activated: false; partner: Partner }
  | { state: "active"; activated: boolean; partner: Partner };

export interface CommercePartnerAuthorityInput {
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
}

export interface CommerceTeamInvite {
  id: string;
  inviterPartnerId: string;
  commerceUserId: string;
  overrideBps: number;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  expiresAt: Date;
  createdAt: Date;
  created: boolean;
}

export interface RevokedCommerceTeamInvite {
  id: string;
  commerceUserId: string;
  revokedAt: Date;
  revoked: boolean;
}

function randomCode(prefix: "partner" | "invite"): string {
  return `${prefix === "partner" ? "p" : "i"}_${randomBytes(12).toString("hex")}`;
}

function assertBps(value: number, maximum: number, label: string): void {
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw new CommercePartnerAuthorityError(`${label} must be between 0 and ${maximum} bps`);
  }
}

function assertAuthority(input: CommercePartnerAuthorityInput): void {
  assertBps(input.teamOverrideMaxBps, COMMERCE_TEAM_SHARE_MAX_BPS, "Team share ceiling");
  assertBps(input.b2bMaxDiscountBps, 9_500, "B2B discount ceiling");
  if (!input.b2bEnabled && (input.b2bMaxDiscountBps !== 0 || input.b2bCanDelegate)) {
    throw new CommercePartnerAuthorityError("a disabled B2B grant cannot retain a ceiling or delegation");
  }
}

async function lockCommerceAccount(client: PoolClient, commerceUserId: string): Promise<void> {
  await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", [commerceUserId]);
}

/**
 * Admin onboarding is immediate: no invitation link and no second identity. An open Team invite is
 * revoked in the same transaction. Existing Commerce memberships are edited in place so immutable
 * financial history and the original earning start remain intact.
 */
export async function onboardCommercePartner(database: SalesDatabase, input: {
  commerceUserId: string;
  commissionBps: number;
  defaultSubCommissionBps: number;
  authority: CommercePartnerAuthorityInput;
  actorId: string;
}): Promise<{ partner: Partner; created: boolean }> {
  assertBps(input.commissionBps, 10_000, "direct commission");
  assertBps(input.defaultSubCommissionBps, 10_000, "default Team fallback");
  assertAuthority(input.authority);
  const client = await database.pool.connect();
  let partnerId: string | null = null;
  let created = false;
  try {
    await client.query("BEGIN");
    await lockCommerceAccount(client, input.commerceUserId);
    const existing = await client.query<{
      id: string;
      team_override_max_bps: number | null;
    }>(`
      SELECT id, team_override_max_bps
      FROM partners WHERE commerce_user_id = $1 FOR UPDATE
    `, [input.commerceUserId]);
    partnerId = existing.rows[0]?.id ?? null;

    const revoked = await client.query<{ id: string }>(`
      UPDATE partner_invites
      SET revoked_at = now()
      WHERE commerce_user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
      RETURNING id
    `, [input.commerceUserId]);

    if (partnerId) {
      const currentCeiling = existing.rows[0]!.team_override_max_bps ?? COMMERCE_TEAM_SHARE_MAX_BPS;
      if (input.authority.teamOverrideMaxBps < currentCeiling) {
        await lowerTeamOverrideCeiling(client, partnerId, input.authority.teamOverrideMaxBps);
      }
      await applyPartnerB2BAuthorityCascade(client, {
        partnerId,
        enabled: input.authority.b2bEnabled,
        maximumBps: input.authority.b2bMaxDiscountBps,
        canDelegate: input.authority.b2bCanDelegate,
        sourcePartnerId: null,
        actorType: "admin",
        actorId: input.actorId,
      });
      await client.query(`
        UPDATE partners
        SET status = 'active', program_enabled = true,
            program_started_at = COALESCE(program_started_at, now()),
            commission_bps = $2, sub_commission_bps = $3,
            team_override_max_bps = $4, team_invites_enabled = $5,
            promo_enabled = false, promo_max_value_nano = 0, promo_max_count = 0,
            referral_discount_enabled = false, referral_discount_bps = 0,
            updated_at = now()
        WHERE id = $1
      `, [
        partnerId,
        input.commissionBps,
        input.defaultSubCommissionBps,
        input.authority.teamOverrideMaxBps,
        input.authority.teamInvitesEnabled,
      ]);
    } else {
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO partners (
          commerce_user_id, program_enabled, program_started_at,
          status, referral_code, commission_bps, sub_commission_bps,
          team_override_max_bps, team_invites_enabled,
          b2b_enabled, b2b_max_discount_bps, b2b_can_delegate,
          promo_enabled, promo_max_value_nano, promo_max_count,
          referral_discount_enabled, referral_discount_bps
        ) VALUES (
          $1, true, now(), 'active', $2, $3, $4, $5, $6,
          $7, $8, $9, false, 0, 0, false, 0
        ) RETURNING id
      `, [
        input.commerceUserId,
        randomCode("partner"),
        input.commissionBps,
        input.defaultSubCommissionBps,
        input.authority.teamOverrideMaxBps,
        input.authority.teamInvitesEnabled,
        input.authority.b2bEnabled,
        input.authority.b2bEnabled ? input.authority.b2bMaxDiscountBps : 0,
        input.authority.b2bEnabled ? input.authority.b2bCanDelegate : false,
      ]);
      partnerId = inserted.rows[0]!.id;
      created = true;
    }

    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, $2, 'partner', $3, $4::jsonb)
    `, [
      input.actorId,
      created ? "commerce_partner.created" : "commerce_partner.reenabled_or_updated",
      partnerId,
      JSON.stringify({
        commerceUserId: input.commerceUserId,
        commissionBps: input.commissionBps,
        authority: input.authority,
        revokedInviteIds: revoked.rows.map((row) => row.id),
      }),
    ]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
  const partner = await getPartner(database, partnerId!);
  if (!partner) throw new CommercePartnerNotFoundError();
  return { partner, created };
}

/** Create an account-bound Team invitation. Exact retries return the existing open invitation. */
export async function createCommerceTeamInvite(database: SalesDatabase, input: {
  inviterCommerceUserId: string;
  inviteeCommerceUserId: string;
  defaultCommissionBps: number;
  defaultSubCommissionBps: number;
  overrideBps: number;
  authority: CommercePartnerAuthorityInput;
  expiresAt: Date;
}): Promise<CommerceTeamInvite> {
  assertBps(input.defaultCommissionBps, 10_000, "platform direct commission");
  assertBps(input.defaultSubCommissionBps, 10_000, "default Team fallback");
  assertBps(input.overrideBps, COMMERCE_TEAM_SHARE_MAX_BPS, "Team share");
  assertAuthority(input.authority);
  if (!(input.expiresAt instanceof Date) || !Number.isFinite(input.expiresAt.getTime())
    || input.expiresAt.getTime() <= Date.now()) {
    throw new CommercePartnerAuthorityError("Team invitation expiry must be in the future");
  }
  if (input.inviterCommerceUserId === input.inviteeCommerceUserId) {
    throw new CommercePartnerConflictError("a partner cannot invite their own Commerce account");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await lockCommerceAccount(client, input.inviteeCommerceUserId);
    const inviterResult = await client.query<{
      id: string;
      team_override_max_bps: number | null;
      team_invites_enabled: boolean;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
    }>(`
      SELECT id, team_override_max_bps, team_invites_enabled,
             b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      FROM partners
      WHERE commerce_user_id = $1 AND program_enabled = true AND status = 'active'
      FOR UPDATE
    `, [input.inviterCommerceUserId]);
    const inviter = inviterResult.rows[0];
    if (!inviter) throw new CommercePartnerNotFoundError("active inviter membership not found");
    if (!inviter.team_invites_enabled) {
      throw new CommercePartnerAuthorityError("Team invitations are disabled for this partner");
    }
    const inviterTeamMaximum = inviter.team_override_max_bps ?? COMMERCE_TEAM_SHARE_MAX_BPS;
    if (input.overrideBps > inviterTeamMaximum
      || input.authority.teamOverrideMaxBps > inviterTeamMaximum) {
      throw new CommercePartnerAuthorityError(
        `Team controls exceed the inviter ceiling of ${inviterTeamMaximum} bps`,
      );
    }
    if (input.authority.teamInvitesEnabled && !inviter.team_invites_enabled) {
      throw new CommercePartnerAuthorityError("Team invitation authority cannot be delegated");
    }
    if (input.authority.b2bEnabled) {
      if (!inviter.b2b_enabled || !inviter.b2b_can_delegate) {
        throw new CommercePartnerAuthorityError("the inviter's B2B grant cannot be delegated");
      }
      if (input.authority.b2bMaxDiscountBps > inviter.b2b_max_discount_bps) {
        throw new CommercePartnerAuthorityError(
          `B2B discount exceeds the inviter ceiling of ${inviter.b2b_max_discount_bps} bps`,
        );
      }
    }

    const member = await client.query<{ id: string }>(`
      SELECT id FROM partners WHERE commerce_user_id = $1 FOR SHARE
    `, [input.inviteeCommerceUserId]);
    if (member.rows[0]) {
      throw new CommercePartnerConflictError("this Commerce account already has a partner membership");
    }

    // An expired row remains immutable evidence but must not monopolize the partial unique index.
    const expired = await client.query<{ id: string }>(`
      UPDATE partner_invites
      SET revoked_at = now()
      WHERE commerce_user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
        AND expires_at IS NOT NULL AND expires_at <= now()
      RETURNING id
    `, [input.inviteeCommerceUserId]);
    for (const row of expired.rows) {
      await client.query(`
        INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
        VALUES ('partner', $1, 'commerce_team.invite_expired', 'partner_invite', $2, $3::jsonb)
      `, [inviter.id, row.id, JSON.stringify({ commerceUserId: input.inviteeCommerceUserId })]);
    }

    const existing = await client.query<{
      id: string;
      partner_id: string;
      parent_override_bps: number;
      team_override_max_bps: number;
      team_invites_enabled: boolean;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
      expires_at: Date;
      created_at: Date;
    }>(`
      SELECT id, partner_id, parent_override_bps, team_override_max_bps,
             team_invites_enabled, b2b_enabled, b2b_max_discount_bps, b2b_can_delegate,
             expires_at, created_at
      FROM partner_invites
      WHERE commerce_user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
      FOR UPDATE
    `, [input.inviteeCommerceUserId]);
    const open = existing.rows[0];
    if (open) {
      const exact = open.partner_id === inviter.id
        && open.parent_override_bps === input.overrideBps
        && open.team_override_max_bps === input.authority.teamOverrideMaxBps
        && open.team_invites_enabled === input.authority.teamInvitesEnabled
        && open.b2b_enabled === input.authority.b2bEnabled
        && open.b2b_max_discount_bps === input.authority.b2bMaxDiscountBps
        && open.b2b_can_delegate === input.authority.b2bCanDelegate;
      if (!exact) throw new CommercePartnerConflictError("this account already has another open Team invitation");
      await client.query("COMMIT");
      return {
        id: open.id,
        inviterPartnerId: open.partner_id,
        commerceUserId: input.inviteeCommerceUserId,
        overrideBps: open.parent_override_bps,
        teamOverrideMaxBps: open.team_override_max_bps,
        teamInvitesEnabled: open.team_invites_enabled,
        b2bEnabled: open.b2b_enabled,
        b2bMaxDiscountBps: open.b2b_max_discount_bps,
        b2bCanDelegate: open.b2b_can_delegate,
        expiresAt: open.expires_at,
        createdAt: open.created_at,
        created: false,
      };
    }

    const inserted = await client.query<{
      id: string;
      created_at: Date;
    }>(`
      INSERT INTO partner_invites (
        partner_id, code, commerce_user_id, commission_bps, sub_commission_bps,
        team_override_max_bps, parent_override_bps,
        promo_enabled, promo_max_value_nano, promo_max_count,
        referral_discount_enabled, referral_discount_bps,
        b2b_enabled, b2b_max_discount_bps, team_invites_enabled, b2b_can_delegate,
        expires_at
      ) VALUES (
        $1, $2, $3, $4, $5, $6, $7,
        false, 0, 0, false, 0, $8, $9, $10, $11, $12
      ) RETURNING id, created_at
    `, [
      inviter.id,
      randomCode("invite"),
      input.inviteeCommerceUserId,
      input.defaultCommissionBps,
      input.defaultSubCommissionBps,
      input.authority.teamOverrideMaxBps,
      input.overrideBps,
      input.authority.b2bEnabled,
      input.authority.b2bEnabled ? input.authority.b2bMaxDiscountBps : 0,
      input.authority.teamInvitesEnabled,
      input.authority.b2bEnabled ? input.authority.b2bCanDelegate : false,
      input.expiresAt,
    ]);
    const invitation = inserted.rows[0]!;
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'commerce_team.invite_created', 'partner_invite', $2, $3::jsonb)
    `, [inviter.id, invitation.id, JSON.stringify({
      commerceUserId: input.inviteeCommerceUserId,
      overrideBps: input.overrideBps,
      authority: input.authority,
    })]);
    await client.query("COMMIT");
    return {
      id: invitation.id,
      inviterPartnerId: inviter.id,
      commerceUserId: input.inviteeCommerceUserId,
      overrideBps: input.overrideBps,
      teamOverrideMaxBps: input.authority.teamOverrideMaxBps,
      teamInvitesEnabled: input.authority.teamInvitesEnabled,
      b2bEnabled: input.authority.b2bEnabled,
      b2bMaxDiscountBps: input.authority.b2bMaxDiscountBps,
      b2bCanDelegate: input.authority.b2bCanDelegate,
      expiresAt: input.expiresAt,
      createdAt: invitation.created_at,
      created: true,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Revoke one still-pending account-bound invitation owned by the active inviter. Exact retries are safe. */
export async function revokeCommerceTeamInvite(database: SalesDatabase, input: {
  inviterCommerceUserId: string;
  inviteId: string;
}): Promise<RevokedCommerceTeamInvite> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const target = await client.query<{ commerce_user_id: string }>(`
      SELECT commerce_user_id
      FROM partner_invites
      WHERE id = $1 AND commerce_user_id IS NOT NULL
    `, [input.inviteId]);
    const inviteeCommerceUserId = target.rows[0]?.commerce_user_id;
    if (!inviteeCommerceUserId) throw new CommercePartnerNotFoundError("Team invitation not found");
    // Match create/activate/admin onboarding lock order. The row is re-read FOR UPDATE below;
    // 0027 prevents account-bound invitation deletion between the two reads.
    await lockCommerceAccount(client, inviteeCommerceUserId);
    const inviter = await client.query<{ id: string }>(`
      SELECT id FROM partners
      WHERE commerce_user_id = $1 AND status = 'active' AND program_enabled = true
      FOR UPDATE
    `, [input.inviterCommerceUserId]);
    const inviterPartnerId = inviter.rows[0]?.id;
    if (!inviterPartnerId) throw new CommercePartnerNotFoundError("active inviter membership not found");

    const invitation = await client.query<{
      id: string;
      commerce_user_id: string;
      consumed_at: Date | null;
      revoked_at: Date | null;
    }>(`
      SELECT id, commerce_user_id, consumed_at, revoked_at
      FROM partner_invites
      WHERE id = $1 AND partner_id = $2 AND commerce_user_id IS NOT NULL
      FOR UPDATE
    `, [input.inviteId, inviterPartnerId]);
    const current = invitation.rows[0];
    if (!current) throw new CommercePartnerNotFoundError("Team invitation not found");
    if (current.commerce_user_id !== inviteeCommerceUserId) {
      throw new CommercePartnerConflictError("Team invitation target changed during revocation");
    }
    if (current.consumed_at) {
      throw new CommercePartnerConflictError("an activated Team invitation cannot be revoked");
    }
    if (current.revoked_at) {
      await client.query("COMMIT");
      return {
        id: current.id,
        commerceUserId: inviteeCommerceUserId,
        revokedAt: current.revoked_at,
        revoked: false,
      };
    }
    const updated = await client.query<{ revoked_at: Date }>(`
      UPDATE partner_invites SET revoked_at = now()
      WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
      RETURNING revoked_at
    `, [current.id]);
    const revokedAt = updated.rows[0]?.revoked_at;
    if (!revokedAt) throw new CommercePartnerConflictError("Team invitation state changed during revocation");
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'commerce_team.invite_revoked', 'partner_invite', $2, $3::jsonb)
    `, [
      inviterPartnerId,
      current.id,
      JSON.stringify({ commerceUserId: inviteeCommerceUserId }),
    ]);
    await client.query("COMMIT");
    return {
      id: current.id,
      commerceUserId: inviteeCommerceUserId,
      revokedAt,
      revoked: true,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Resolve current access and atomically activate a valid account-bound invitation. */
export async function resolveCommercePartnerMembership(
  database: SalesDatabase,
  input: { commerceUserId: string },
): Promise<CommercePartnerMembershipResolution> {
  const client = await database.pool.connect();
  let partnerId: string | null = null;
  let state: "active" | "disabled" | "unavailable" = "unavailable";
  let activated = false;
  try {
    await client.query("BEGIN");
    await lockCommerceAccount(client, input.commerceUserId);
    const existing = await client.query<{
      id: string;
      status: string;
      program_enabled: boolean;
    }>(`
      SELECT id, status::text, program_enabled
      FROM partners WHERE commerce_user_id = $1 FOR UPDATE
    `, [input.commerceUserId]);
    if (existing.rows[0]) {
      partnerId = existing.rows[0].id;
      state = existing.rows[0].program_enabled && existing.rows[0].status === "active"
        ? "active"
        : "disabled";
      await client.query("COMMIT");
    } else {
      const invitation = await client.query<{
        id: string;
        partner_id: string | null;
        commission_bps: number | null;
        sub_commission_bps: number | null;
        team_override_max_bps: number | null;
        parent_override_bps: number | null;
        b2b_enabled: boolean;
        b2b_max_discount_bps: number;
        team_invites_enabled: boolean;
        b2b_can_delegate: boolean;
      }>(`
        SELECT id, partner_id, commission_bps, sub_commission_bps,
               team_override_max_bps, parent_override_bps,
               b2b_enabled, b2b_max_discount_bps, team_invites_enabled, b2b_can_delegate
        FROM partner_invites
        WHERE commerce_user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
        FOR UPDATE
      `, [input.commerceUserId]);
      const invite = invitation.rows[0];
      if (!invite) {
        await client.query("COMMIT");
      } else {
        if (invite.partner_id) {
          const parent = await client.query<{ allowed: boolean }>(`
            SELECT (
              status = 'active' AND program_enabled = true AND team_invites_enabled = true
            ) AS allowed
            FROM partners WHERE id = $1 FOR SHARE
          `, [invite.partner_id]);
          if (!parent.rows[0]?.allowed) {
            await client.query(`
              UPDATE partner_invites SET revoked_at = now() WHERE id = $1
            `, [invite.id]);
            await client.query(`
              INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
              VALUES ('system', 'commerce-membership', 'commerce_team.invite_revoked_invalid_parent',
                      'partner_invite', $1, $2::jsonb)
            `, [invite.id, JSON.stringify({ commerceUserId: input.commerceUserId })]);
            await client.query("COMMIT");
            return { state: "unavailable", activated: false, partner: null };
          }
        }
        const inserted = await client.query<{ id: string }>(`
          INSERT INTO partners (
            commerce_user_id, program_enabled, program_started_at,
            status, referral_code, parent_partner_id, commission_bps, sub_commission_bps,
            team_override_max_bps, parent_override_bps, team_invites_enabled,
            b2b_enabled, b2b_max_discount_bps, b2b_can_delegate, b2b_grant_source_partner_id,
            promo_enabled, promo_max_value_nano, promo_max_count,
            referral_discount_enabled, referral_discount_bps
          ) VALUES (
            $1, true, now(), 'active', $2, $3, $4, $5, $6, $7, $8,
            $9, $10, $11, CASE WHEN $9 AND $3::uuid IS NOT NULL THEN $3 ELSE NULL END,
            false, 0, 0, false, 0
          ) RETURNING id
        `, [
          input.commerceUserId,
          randomCode("partner"),
          invite.partner_id,
          invite.commission_bps ?? 1_000,
          invite.sub_commission_bps ?? 1_000,
          invite.team_override_max_bps ?? COMMERCE_TEAM_SHARE_MAX_BPS,
          invite.parent_override_bps,
          invite.team_invites_enabled,
          invite.b2b_enabled,
          invite.b2b_enabled ? invite.b2b_max_discount_bps : 0,
          invite.b2b_enabled ? invite.b2b_can_delegate : false,
        ]);
        partnerId = inserted.rows[0]!.id;
        await client.query(`
          UPDATE partner_invites
          SET consumed_at = now(), consumed_by_partner_id = $2
          WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
        `, [invite.id, partnerId]);
        await client.query(`
          INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
          VALUES ('commerce_user', $1, 'commerce_partner.invite_activated', 'partner', $2, $3::jsonb)
        `, [input.commerceUserId, partnerId, JSON.stringify({ inviteId: invite.id, parentPartnerId: invite.partner_id })]);
        state = "active";
        activated = true;
        await client.query("COMMIT");
      }
    }
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
  if (state === "unavailable" || !partnerId) {
    return { state: "unavailable", activated: false, partner: null };
  }
  const partner = await getPartner(database, partnerId);
  if (!partner) throw new CommercePartnerNotFoundError();
  return state === "active"
    ? { state, activated, partner }
    : { state, activated: false, partner };
}

/** Atomically bind the only supported payout wallet and its audit evidence. */
export async function updateCommercePartnerWallet(database: SalesDatabase, input: {
  commerceUserId: string;
  address: string;
}): Promise<Partner> {
  if (!/^0x[a-fA-F0-9]{40}$/.test(input.address)) {
    throw new CommercePartnerAuthorityError("invalid BSC payout address");
  }
  const client = await database.pool.connect();
  let partnerId: string | null = null;
  try {
    await client.query("BEGIN");
    await lockCommerceAccount(client, input.commerceUserId);
    const partner = await client.query<{ id: string }>(`
      SELECT id FROM partners
      WHERE commerce_user_id = $1 AND status = 'active' AND program_enabled = true
      FOR UPDATE
    `, [input.commerceUserId]);
    partnerId = partner.rows[0]?.id ?? null;
    if (!partnerId) throw new CommercePartnerNotFoundError("active Commerce partner membership not found");
    const payoutDetails = {
      network: "BSC",
      asset: "USDT (BEP-20)",
      address: input.address,
    };
    await client.query(`
      UPDATE partners
      SET payout_method = 'usdt-bep20', payout_details = $2::jsonb, updated_at = now()
      WHERE id = $1
    `, [partnerId, JSON.stringify(payoutDetails)]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'partner.wallet_changed', 'partner', $1, $2::jsonb)
    `, [partnerId, JSON.stringify({ method: "usdt-bep20", address: input.address })]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
  const updated = await getPartner(database, partnerId!);
  if (!updated) throw new CommercePartnerNotFoundError();
  return updated;
}
