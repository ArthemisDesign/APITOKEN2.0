import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";
import {
  lowerTeamOverrideCeiling,
  TeamMemberNotFoundError,
  TeamOverrideLimitError,
} from "./commissions.js";

export class PartnerB2BAuthorityError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PartnerB2BAuthorityError";
  }
}

export class PartnerTeamAuthorityError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PartnerTeamAuthorityError";
  }
}

interface B2BAuthority {
  enabled: boolean;
  maximumBps: number;
  canDelegate: boolean;
  sourcePartnerId: string | null;
}

interface AuthorityTreeRow {
  id: string;
  depth: number;
  b2b_enabled: boolean;
  b2b_max_discount_bps: number;
  b2b_can_delegate: boolean;
  b2b_grant_source_partner_id: string | null;
}

function normalizedAuthority(input: {
  enabled: boolean;
  maximumBps: number;
  canDelegate: boolean;
  sourcePartnerId: string | null;
}): B2BAuthority {
  if (!input.enabled) {
    return { enabled: false, maximumBps: 0, canDelegate: false, sourcePartnerId: null };
  }
  if (!Number.isInteger(input.maximumBps) || input.maximumBps < 0 || input.maximumBps > 9_500) {
    throw new PartnerB2BAuthorityError("B2B maximum discount must be between 0 and 9500 bps");
  }
  return {
    enabled: true,
    maximumBps: input.maximumBps,
    canDelegate: input.canDelegate,
    sourcePartnerId: input.sourcePartnerId,
  };
}

/**
 * Applies one B2B authority decision and every inherited consequence in the same transaction.
 * Descendants are updated leaf-first so migration-0025 narrowing guards always observe already
 * clamped/revoked children and pending invitations.
 */
export async function applyPartnerB2BAuthorityCascade(
  client: PoolClient,
  input: {
    partnerId: string;
    enabled?: boolean;
    maximumBps?: number;
    canDelegate?: boolean;
    sourcePartnerId: string | null;
    actorType: "admin" | "partner";
    actorId: string;
  },
): Promise<B2BAuthority> {
  const tree = await client.query<AuthorityTreeRow>(`
    WITH RECURSIVE inherited AS (
      SELECT partner.id, 0 AS depth, ARRAY[partner.id] AS path
      FROM partners partner
      WHERE partner.id = $1
      UNION ALL
      SELECT child.id, inherited.depth + 1, inherited.path || child.id
      FROM partners child
      JOIN inherited ON child.b2b_grant_source_partner_id = inherited.id
      WHERE NOT child.id = ANY(inherited.path)
    )
    SELECT partner.id, inherited.depth, partner.b2b_enabled,
           partner.b2b_max_discount_bps, partner.b2b_can_delegate,
           partner.b2b_grant_source_partner_id
    FROM inherited
    JOIN partners partner ON partner.id = inherited.id
    ORDER BY inherited.depth, partner.id
    FOR UPDATE OF partner
  `, [input.partnerId]);
  const root = tree.rows[0];
  if (!root) throw new TeamMemberNotFoundError();

  const rootEnabled = input.enabled ?? root.b2b_enabled;
  if (!rootEnabled && ((input.maximumBps ?? 0) > 0 || input.canDelegate === true)) {
    throw new PartnerB2BAuthorityError("a revoked B2B grant cannot retain a ceiling or delegation");
  }
  const rootTarget = normalizedAuthority({
    enabled: rootEnabled,
    maximumBps: rootEnabled ? (input.maximumBps ?? root.b2b_max_discount_bps) : 0,
    canDelegate: rootEnabled ? (input.canDelegate ?? root.b2b_can_delegate) : false,
    sourcePartnerId: input.sourcePartnerId,
  });
  const targets = new Map<string, B2BAuthority>([[root.id, rootTarget]]);
  for (const row of tree.rows.slice(1)) {
    const parentTarget = row.b2b_grant_source_partner_id
      ? targets.get(row.b2b_grant_source_partner_id)
      : undefined;
    if (!parentTarget?.enabled || !parentTarget.canDelegate) {
      targets.set(row.id, normalizedAuthority({
        enabled: false,
        maximumBps: 0,
        canDelegate: false,
        sourcePartnerId: null,
      }));
      continue;
    }
    targets.set(row.id, normalizedAuthority({
      enabled: row.b2b_enabled,
      maximumBps: Math.min(row.b2b_max_discount_bps, parentTarget.maximumBps),
      canDelegate: row.b2b_can_delegate,
      sourcePartnerId: row.b2b_grant_source_partner_id,
    }));
  }

  const cascadeEvidence: Array<{
    partnerId: string;
    enabled: boolean;
    maximumBps: number;
    canDelegate: boolean;
    sourcePartnerId: string | null;
  }> = [];
  for (const row of [...tree.rows].sort((left, right) => right.depth - left.depth)) {
    const target = targets.get(row.id)!;
    if (target.enabled && target.canDelegate) {
      await client.query(`
        UPDATE partner_invites
        SET b2b_max_discount_bps = LEAST(b2b_max_discount_bps, $2)
        WHERE partner_id = $1 AND consumed_at IS NULL AND b2b_enabled
      `, [row.id, target.maximumBps]);
    } else {
      await client.query(`
        UPDATE partner_invites
        SET b2b_enabled = false, b2b_max_discount_bps = 0, b2b_can_delegate = false
        WHERE partner_id = $1 AND consumed_at IS NULL AND b2b_enabled
      `, [row.id]);
    }
    const changed = row.b2b_enabled !== target.enabled
      || row.b2b_max_discount_bps !== target.maximumBps
      || row.b2b_can_delegate !== target.canDelegate
      || row.b2b_grant_source_partner_id !== target.sourcePartnerId;
    if (!changed) continue;
    await client.query(`
      UPDATE partners
      SET b2b_enabled = $2,
          b2b_max_discount_bps = $3,
          b2b_can_delegate = $4,
          b2b_grant_source_partner_id = $5,
          updated_at = now()
      WHERE id = $1
    `, [
      row.id,
      target.enabled,
      target.maximumBps,
      target.canDelegate,
      target.sourcePartnerId,
    ]);
    cascadeEvidence.push({
      partnerId: row.id,
      enabled: target.enabled,
      maximumBps: target.maximumBps,
      canDelegate: target.canDelegate,
      sourcePartnerId: target.sourcePartnerId,
    });
  }
  await client.query(`
    INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ($1, $2, 'partner.b2b_authority_updated', 'partner', $3, $4::jsonb)
  `, [input.actorType, input.actorId, input.partnerId, JSON.stringify({
    authority: rootTarget,
    cascade: cascadeEvidence,
  })]);
  return rootTarget;
}

export interface DirectTeamMemberAuthority {
  memberId: string;
  overrideBps: number;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
}

/** One direct-member edit: edge, delegated Team ceiling and optional B2B authority are atomic. */
export async function updateDirectTeamMemberAuthority(
  database: SalesDatabase,
  input: {
    parentPartnerId: string;
    memberId: string;
    overrideBps?: number;
    teamOverrideMaxBps?: number;
    teamInvitesEnabled?: boolean;
    b2bEnabled?: boolean;
    b2bMaxDiscountBps?: number;
    b2bCanDelegate?: boolean;
  },
): Promise<DirectTeamMemberAuthority> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const parent = await client.query<{
      maximum_bps: number;
      team_invites_enabled: boolean;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
    }>(`
      SELECT COALESCE(team_override_max_bps, 2000) AS maximum_bps,
             team_invites_enabled, b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      FROM partners
      WHERE id = $1 AND status = 'active'
      FOR UPDATE
    `, [input.parentPartnerId]);
    const source = parent.rows[0];
    if (!source) throw new TeamMemberNotFoundError();
    if ((input.overrideBps !== undefined && input.overrideBps > source.maximum_bps)
      || (input.teamOverrideMaxBps !== undefined && input.teamOverrideMaxBps > source.maximum_bps)) {
      throw new TeamOverrideLimitError(source.maximum_bps);
    }
    if (input.teamInvitesEnabled === true && !source.team_invites_enabled) {
      throw new PartnerTeamAuthorityError("your Team invitation authority cannot be delegated");
    }
    const member = await client.query<{
      override_bps: number;
      team_override_max_bps: number;
      team_invites_enabled: boolean;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
      b2b_grant_source_partner_id: string | null;
    }>(`
      SELECT COALESCE(child.parent_override_bps, parent.sub_commission_bps) AS override_bps,
             COALESCE(child.team_override_max_bps, 2000) AS team_override_max_bps,
             child.team_invites_enabled, child.b2b_enabled, child.b2b_max_discount_bps,
             child.b2b_can_delegate, child.b2b_grant_source_partner_id
      FROM partners child
      JOIN partners parent ON parent.id = $1
      WHERE child.id = $2 AND child.parent_partner_id = parent.id
      FOR UPDATE OF child
    `, [input.parentPartnerId, input.memberId]);
    const current = member.rows[0];
    if (!current) throw new TeamMemberNotFoundError();
    if (input.teamOverrideMaxBps !== undefined
      && input.teamOverrideMaxBps < current.team_override_max_bps) {
      await lowerTeamOverrideCeiling(client, input.memberId, input.teamOverrideMaxBps);
    }

    const changesB2B = input.b2bEnabled !== undefined
      || input.b2bMaxDiscountBps !== undefined
      || input.b2bCanDelegate !== undefined;
    if (changesB2B) {
      if (current.b2b_enabled && current.b2b_grant_source_partner_id === null) {
        throw new PartnerB2BAuthorityError("a direct platform B2B grant can be changed only by an admin");
      }
      const enabled = input.b2bEnabled ?? current.b2b_enabled;
      if (!enabled && ((input.b2bMaxDiscountBps ?? 0) > 0 || input.b2bCanDelegate === true)) {
        throw new PartnerB2BAuthorityError("a revoked B2B grant cannot retain a ceiling or delegation");
      }
      const maximumBps = enabled
        ? (input.b2bMaxDiscountBps ?? current.b2b_max_discount_bps)
        : 0;
      const canDelegate = enabled
        ? (input.b2bCanDelegate ?? current.b2b_can_delegate)
        : false;
      if (enabled && (!source.b2b_enabled || !source.b2b_can_delegate)) {
        throw new PartnerB2BAuthorityError("your B2B grant cannot be delegated");
      }
      if (maximumBps > source.b2b_max_discount_bps) {
        throw new PartnerB2BAuthorityError(
          `B2B maximum discount exceeds your limit of ${source.b2b_max_discount_bps} bps`,
        );
      }
      await applyPartnerB2BAuthorityCascade(client, {
        partnerId: input.memberId,
        enabled,
        maximumBps,
        canDelegate,
        sourcePartnerId: enabled ? input.parentPartnerId : null,
        actorType: "partner",
        actorId: input.parentPartnerId,
      });
    }

    const updated = await client.query<{
      override_bps: number;
      team_override_max_bps: number;
      team_invites_enabled: boolean;
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
    }>(`
      UPDATE partners
      SET parent_override_bps = CASE WHEN $3::boolean THEN $4 ELSE parent_override_bps END,
          team_override_max_bps = CASE WHEN $5::boolean THEN $6 ELSE team_override_max_bps END,
          team_invites_enabled = CASE WHEN $7::boolean THEN $8 ELSE team_invites_enabled END,
          updated_at = now()
      WHERE id = $2 AND parent_partner_id = $1
      RETURNING COALESCE(parent_override_bps, $9) AS override_bps,
                COALESCE(team_override_max_bps, 2000) AS team_override_max_bps,
                team_invites_enabled, b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
    `, [
      input.parentPartnerId,
      input.memberId,
      input.overrideBps !== undefined,
      input.overrideBps ?? null,
      input.teamOverrideMaxBps !== undefined,
      input.teamOverrideMaxBps ?? null,
      input.teamInvitesEnabled !== undefined,
      input.teamInvitesEnabled ?? null,
      current.override_bps,
    ]);
    const row = updated.rows[0];
    if (!row) throw new TeamMemberNotFoundError();
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'team.member_authority_updated', 'partner', $2, $3::jsonb)
    `, [input.parentPartnerId, input.memberId, JSON.stringify({
      overrideBps: input.overrideBps ?? null,
      teamOverrideMaxBps: input.teamOverrideMaxBps ?? null,
      teamInvitesEnabled: input.teamInvitesEnabled ?? null,
      b2bEnabled: input.b2bEnabled ?? null,
      b2bMaxDiscountBps: input.b2bMaxDiscountBps ?? null,
      b2bCanDelegate: input.b2bCanDelegate ?? null,
    })]);
    await client.query("COMMIT");
    return {
      memberId: input.memberId,
      overrideBps: row.override_bps,
      teamOverrideMaxBps: row.team_override_max_bps,
      teamInvitesEnabled: row.team_invites_enabled,
      b2bEnabled: row.b2b_enabled,
      b2bMaxDiscountBps: row.b2b_max_discount_bps,
      b2bCanDelegate: row.b2b_can_delegate,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
