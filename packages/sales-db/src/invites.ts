import type { SalesDatabase } from "./client.js";

export interface PartnerInvite {
  id: string;
  partnerId: string;
  code: string;
  commissionBps: number | null;
  expiresAt: Date | null;
  consumedAt: Date | null;
  consumedByPartnerId: string | null;
  createdAt: Date;
}

export class InviteCodeCollisionError extends Error {}

interface InviteRow {
  id: string;
  partner_id: string;
  code: string;
  commission_bps: number | null;
  expires_at: Date | null;
  consumed_at: Date | null;
  consumed_by_partner_id: string | null;
  created_at: Date;
}

function mapInvite(row: InviteRow): PartnerInvite {
  return {
    id: row.id,
    partnerId: row.partner_id,
    code: row.code,
    commissionBps: row.commission_bps,
    expiresAt: row.expires_at,
    consumedAt: row.consumed_at,
    consumedByPartnerId: row.consumed_by_partner_id,
    createdAt: row.created_at,
  };
}

export async function createPartnerInvite(database: SalesDatabase, input: {
  partnerId: string;
  code: string;
  commissionBps: number | null;
  expiresAt: Date;
}): Promise<PartnerInvite> {
  try {
    const result = await database.pool.query<InviteRow>(`
      INSERT INTO partner_invites (partner_id, code, commission_bps, expires_at)
      VALUES ($1, $2, $3, $4)
      RETURNING id, partner_id, code, commission_bps, expires_at, consumed_at, consumed_by_partner_id, created_at
    `, [input.partnerId, input.code, input.commissionBps, input.expiresAt]);
    return mapInvite(result.rows[0]!);
  } catch (error) {
    if (typeof error === "object" && error !== null && "code" in error && error.code === "23505") {
      throw new InviteCodeCollisionError("invite code collision");
    }
    throw error;
  }
}

export async function listPartnerInvites(database: SalesDatabase, partnerId: string): Promise<PartnerInvite[]> {
  const result = await database.pool.query<InviteRow>(`
    SELECT id, partner_id, code, commission_bps, expires_at, consumed_at, consumed_by_partner_id, created_at
    FROM partner_invites WHERE partner_id = $1 ORDER BY created_at DESC
  `, [partnerId]);
  return result.rows.map(mapInvite);
}
