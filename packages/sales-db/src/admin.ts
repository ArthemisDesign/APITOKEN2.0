import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";

export interface SalesOverview {
  partners: number;
  activePartners: number;
  referredUsers: number;
  totalSpendNano: bigint;
  totalCommissionsNano: bigint;
  pendingPayoutsNano: bigint;
  paidPayoutsNano: bigint;
}

export async function getSalesOverview(database: SalesDatabase): Promise<SalesOverview> {
  const result = await database.pool.query<{
    partners: string; active_partners: string; referred_users: string;
    total_spend: string; total_commissions: string; pending_payouts: string; paid_payouts: string;
  }>(`
    SELECT
      (SELECT count(*) FROM partners)::text AS partners,
      (SELECT count(*) FROM partners WHERE status = 'active')::text AS active_partners,
      (SELECT count(*) FROM referred_users)::text AS referred_users,
      COALESCE((SELECT SUM(amount_nano) FROM partner_usage_events), 0)::text AS total_spend,
      COALESCE((SELECT SUM(amount_nano) FROM commission_entries), 0)::text AS total_commissions,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE status IN ('requested', 'approved')), 0)::text AS pending_payouts,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE status = 'paid'), 0)::text AS paid_payouts
  `);
  const row = result.rows[0]!;
  return {
    partners: Number(row.partners),
    activePartners: Number(row.active_partners),
    referredUsers: Number(row.referred_users),
    totalSpendNano: BigInt(row.total_spend),
    totalCommissionsNano: BigInt(row.total_commissions),
    pendingPayoutsNano: BigInt(row.pending_payouts),
    paidPayoutsNano: BigInt(row.paid_payouts),
  };
}

export interface AdminPartnerSummary {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: PartnerStatus;
  emailVerified: boolean;
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  parentPartnerId: string | null;
  parentEmail: string | null;
  parentTelegramUsername: string | null;
  referredUsers: number;
  teamSize: number;
  earnedNano: bigint;
  paidNano: bigint;
  createdAt: Date;
}

export async function listPartnersWithAggregates(database: SalesDatabase): Promise<AdminPartnerSummary[]> {
  const result = await database.pool.query<{
    id: string; email: string | null; telegram_username: string | null; display_name: string | null;
    status: PartnerStatus;
    email_verified: boolean; referral_code: string; commission_bps: number; sub_commission_bps: number;
    parent_partner_id: string | null; parent_email: string | null; parent_telegram_username: string | null;
    referred_users: string; team_size: string; earned: string; paid: string; created_at: Date;
  }>(`
    SELECT p.id, p.email, p.telegram_username, p.display_name, p.status, p.email_verified, p.referral_code,
      p.commission_bps, p.sub_commission_bps, p.parent_partner_id, parent.email AS parent_email,
      parent.telegram_username AS parent_telegram_username,
      (SELECT count(*) FROM referred_users ru WHERE ru.partner_id = p.id)::text AS referred_users,
      (SELECT count(*) FROM partners child WHERE child.parent_partner_id = p.id)::text AS team_size,
      COALESCE((SELECT SUM(amount_nano) FROM commission_entries ce WHERE ce.partner_id = p.id), 0)::text AS earned,
      COALESCE((SELECT SUM(amount_nano) FROM payouts po WHERE po.partner_id = p.id AND po.status = 'paid'), 0)::text AS paid,
      p.created_at
    FROM partners p
    LEFT JOIN partners parent ON parent.id = p.parent_partner_id
    ORDER BY p.created_at DESC
  `);
  return result.rows.map((row) => ({
    id: row.id,
    email: row.email,
    telegramUsername: row.telegram_username,
    displayName: row.display_name,
    status: row.status,
    emailVerified: row.email_verified,
    referralCode: row.referral_code,
    commissionBps: row.commission_bps,
    subCommissionBps: row.sub_commission_bps,
    parentPartnerId: row.parent_partner_id,
    parentEmail: row.parent_email,
    parentTelegramUsername: row.parent_telegram_username,
    referredUsers: Number(row.referred_users),
    teamSize: Number(row.team_size),
    earnedNano: BigInt(row.earned),
    paidNano: BigInt(row.paid),
    createdAt: row.created_at,
  }));
}

export async function updatePartnerAdmin(database: SalesDatabase, partnerId: string, input: {
  commissionBps?: number;
  subCommissionBps?: number;
  status?: PartnerStatus;
  actorId: string | null;
}): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const updated = await client.query<{ id: string }>(`
      UPDATE partners
      SET commission_bps = COALESCE($2, commission_bps),
          sub_commission_bps = COALESCE($3, sub_commission_bps),
          status = COALESCE($4::partner_status, status),
          updated_at = now()
      WHERE id = $1
      RETURNING id
    `, [partnerId, input.commissionBps ?? null, input.subCommissionBps ?? null, input.status ?? null]);
    if (!updated.rows[0]) {
      await client.query("ROLLBACK");
      return false;
    }
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'partner.updated', 'partner', $2, $3::jsonb)
    `, [input.actorId, partnerId, JSON.stringify({
      commissionBps: input.commissionBps ?? null,
      subCommissionBps: input.subCommissionBps ?? null,
      status: input.status ?? null,
    })]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function insertSalesAudit(database: SalesDatabase, input: {
  actorType: string;
  actorId: string | null;
  action: string;
  targetType: string;
  targetId: string;
  metadata?: unknown;
}): Promise<void> {
  await database.pool.query(`
    INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ($1, $2, $3, $4, $5, $6::jsonb)
  `, [input.actorType, input.actorId, input.action, input.targetType, input.targetId, JSON.stringify(input.metadata ?? {})]);
}
