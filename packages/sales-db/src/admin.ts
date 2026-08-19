import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";

export interface SalesOverview {
  partners: number;
  activePartners: number;
  referredUsers: number;
  totalSpendNano: bigint;
  totalCommissionsNano: bigint;
  totalAdjustmentsNano: bigint;
  totalNetCommissionsNano: bigint;
  totalDebtNano: bigint;
  totalPayableNano: bigint;
  pendingPayoutsNano: bigint;
  paidPayoutsNano: bigint;
}

export async function getSalesOverview(database: SalesDatabase): Promise<SalesOverview> {
  const result = await database.pool.query<{
    partners: string; active_partners: string; referred_users: string;
    total_spend: string; total_commissions: string; total_adjustments: string;
    total_debt: string; total_payable: string; pending_payouts: string; paid_payouts: string;
  }>(`
    SELECT
      (SELECT count(*) FROM partners)::text AS partners,
      (SELECT count(*) FROM partners WHERE status = 'active')::text AS active_partners,
      (SELECT count(*) FROM referred_users)::text AS referred_users,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT amount_nano FROM partner_usage_events
        UNION ALL
        SELECT paid_funded_nano FROM partner_usage_events_v2
      ) all_usage), 0)::text AS total_spend,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT amount_nano FROM commission_entries
        UNION ALL
        SELECT amount_nano FROM commission_entries_v2
      ) all_commissions), 0)::text AS total_commissions,
      COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments), 0)::text AS total_adjustments,
      COALESCE((
        SELECT SUM(GREATEST(0, -balance)) FROM (
          SELECT
            COALESCE((SELECT SUM(amount_nano) FROM (
              SELECT partner_id, amount_nano FROM commission_entries
              UNION ALL
              SELECT partner_id, amount_nano FROM commission_entries_v2
            ) commission WHERE commission.partner_id = partner.id), 0)
            + COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
                        WHERE adjustment.partner_id = partner.id), 0)
            - COALESCE((SELECT SUM(amount_nano) FROM payouts payout
                        WHERE payout.partner_id = partner.id AND payout.status = 'paid'), 0) AS balance
          FROM partners partner
        ) partner_balances
      ), 0)::text AS total_debt,
      COALESCE((
        SELECT SUM(GREATEST(0, balance)) FROM (
          SELECT
            COALESCE((SELECT SUM(amount_nano) FROM (
              SELECT partner_id, amount_nano FROM commission_entries
              UNION ALL
              SELECT partner_id, amount_nano FROM commission_entries_v2
            ) commission WHERE commission.partner_id = partner.id), 0)
            + COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
                        WHERE adjustment.partner_id = partner.id), 0)
            - COALESCE((SELECT SUM(amount_nano) FROM payouts payout
                        WHERE payout.partner_id = partner.id
                          AND payout.status IN ('requested','approved','paid')), 0) AS balance
          FROM partners partner
        ) partner_balances
      ), 0)::text AS total_payable,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE status IN ('requested', 'approved')), 0)::text AS pending_payouts,
      COALESCE((SELECT SUM(amount_nano) FROM payouts WHERE status = 'paid'), 0)::text AS paid_payouts
  `);
  const row = result.rows[0]!;
  const totalCommissionsNano = BigInt(row.total_commissions);
  const totalAdjustmentsNano = BigInt(row.total_adjustments);
  return {
    partners: Number(row.partners),
    activePartners: Number(row.active_partners),
    referredUsers: Number(row.referred_users),
    totalSpendNano: BigInt(row.total_spend),
    totalCommissionsNano,
    totalAdjustmentsNano,
    totalNetCommissionsNano: totalCommissionsNano + totalAdjustmentsNano,
    totalDebtNano: BigInt(row.total_debt),
    totalPayableNano: BigInt(row.total_payable),
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
  adjustmentNano: bigint;
  netNano: bigint;
  debtNano: bigint;
  payableNano: bigint;
  paidNano: bigint;
  promoEnabled: boolean;
  promoMaxValueNano: bigint;
  promoMaxCount: number;
  promoUsed: number;
  referralDiscountBps: number;
  referralDiscountEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  createdAt: Date;
}

export async function listPartnersWithAggregates(database: SalesDatabase): Promise<AdminPartnerSummary[]> {
  const result = await database.pool.query<{
    id: string; email: string | null; telegram_username: string | null; display_name: string | null;
    status: PartnerStatus;
    email_verified: boolean; referral_code: string; commission_bps: number; sub_commission_bps: number;
    parent_partner_id: string | null; parent_email: string | null; parent_telegram_username: string | null;
    referred_users: string; team_size: string; earned: string; adjustment: string;
    paid: string; committed: string;
    promo_enabled: boolean; promo_max_value_nano: string; promo_max_count: number; promo_used: string;
    referral_discount_bps: number; referral_discount_enabled: boolean;
    b2b_enabled: boolean; b2b_max_discount_bps: number;
    created_at: Date;
  }>(`
    SELECT p.id, p.email, p.telegram_username, p.display_name, p.status, p.email_verified, p.referral_code,
      p.commission_bps, p.sub_commission_bps, p.parent_partner_id, parent.email AS parent_email,
      parent.telegram_username AS parent_telegram_username,
      p.promo_enabled, p.promo_max_value_nano::text AS promo_max_value_nano, p.promo_max_count,
      p.referral_discount_bps, p.referral_discount_enabled,
      p.b2b_enabled, p.b2b_max_discount_bps,
      (SELECT count(*) FROM promo_codes pc WHERE pc.partner_id = p.id)::text AS promo_used,
      (SELECT count(*) FROM referred_users ru WHERE ru.partner_id = p.id)::text AS referred_users,
      (SELECT count(*) FROM partners child WHERE child.parent_partner_id = p.id)::text AS team_size,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT partner_id, amount_nano FROM commission_entries
        UNION ALL
        SELECT partner_id, amount_nano FROM commission_entries_v2
      ) ce WHERE ce.partner_id = p.id), 0)::text AS earned,
      COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
                WHERE adjustment.partner_id = p.id), 0)::text AS adjustment,
      COALESCE((SELECT SUM(amount_nano) FROM payouts po WHERE po.partner_id = p.id AND po.status = 'paid'), 0)::text AS paid,
      COALESCE((SELECT SUM(amount_nano) FROM payouts po WHERE po.partner_id = p.id
                AND po.status IN ('requested','approved','paid')), 0)::text AS committed,
      p.created_at
    FROM partners p
    LEFT JOIN partners parent ON parent.id = p.parent_partner_id
    ORDER BY p.created_at DESC
  `);
  return result.rows.map((row) => {
    const earnedNano = BigInt(row.earned);
    const adjustmentNano = BigInt(row.adjustment);
    const netNano = earnedNano + adjustmentNano;
    const paidNano = BigInt(row.paid);
    const committedNano = BigInt(row.committed);
    return {
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
      earnedNano,
      adjustmentNano,
      netNano,
      paidNano,
      debtNano: netNano - paidNano < 0n ? paidNano - netNano : 0n,
      payableNano: netNano - committedNano > 0n ? netNano - committedNano : 0n,
    promoEnabled: row.promo_enabled,
    promoMaxValueNano: BigInt(row.promo_max_value_nano),
    promoMaxCount: row.promo_max_count,
    promoUsed: Number(row.promo_used),
    referralDiscountBps: row.referral_discount_bps,
    referralDiscountEnabled: row.referral_discount_enabled,
    b2bEnabled: row.b2b_enabled,
    b2bMaxDiscountBps: row.b2b_max_discount_bps,
      createdAt: row.created_at,
    };
  });
}

/**
 * Updates the retained referral marker ceiling. This is compatibility/audit metadata and never
 * changes a referral's Commerce/engine price. The DB predicate enforces the legacy writer grant.
 */
export async function setPartnerReferralDiscount(
  database: SalesDatabase,
  partnerId: string,
  bps: number,
): Promise<boolean> {
  const result = await database.pool.query(
    `UPDATE partners SET referral_discount_bps = $2, updated_at = now()
     WHERE id = $1 AND status = 'active' AND referral_discount_enabled = true`,
    [partnerId, bps],
  );
  return (result.rowCount ?? 0) > 0;
}

export class PartnerHasHistoryError extends Error {}

/**
 * Полное удаление партнёра — только если у него нет финансовой истории и рефералов
 * (иначе PartnerHasHistoryError: используйте suspend). Чистятся сессии, его инвайты,
 * ссылки из заявок и потреблённых инвайтов.
 */
export async function deletePartnerAdmin(database: SalesDatabase, partnerId: string, actorId: string): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const existing = await client.query<{ id: string; telegram_username: string | null }>(
      "SELECT id, telegram_username FROM partners WHERE id = $1 FOR UPDATE",
      [partnerId],
    );
    if (!existing.rows[0]) {
      await client.query("ROLLBACK");
      return false;
    }
    const history = await client.query<{ referred: string; topups: string; commissions: string; adjustments: string; payouts: string; children: string; promos: string; links: string; aliases: string }>(`
      SELECT
        (SELECT count(*) FROM referred_users WHERE partner_id = $1)::text AS referred,
        (SELECT count(*) FROM referred_topups WHERE partner_id = $1)::text AS topups,
        (SELECT count(*) FROM (
          SELECT partner_id FROM commission_entries
          UNION ALL
          SELECT partner_id FROM commission_entries_v2
        ) ce WHERE ce.partner_id = $1)::text AS commissions,
        (SELECT count(*) FROM partner_commission_adjustments WHERE partner_id = $1)::text AS adjustments,
        (SELECT count(*) FROM payouts WHERE partner_id = $1)::text AS payouts,
        (SELECT count(*) FROM promo_codes WHERE partner_id = $1)::text AS promos,
        (SELECT count(*) FROM partner_discount_links WHERE partner_id = $1)::text AS links,
        (SELECT count(*) FROM external_referral_aliases WHERE partner_id = $1)::text AS aliases,
        (SELECT count(*) FROM partners WHERE parent_partner_id = $1)::text AS children
    `, [partnerId]);
    const h = history.rows[0]!;
    if (h.referred !== "0" || h.topups !== "0" || h.commissions !== "0" || h.adjustments !== "0" || h.payouts !== "0" || h.promos !== "0" || h.links !== "0" || h.aliases !== "0" || h.children !== "0") {
      await client.query("ROLLBACK");
      throw new PartnerHasHistoryError(
        "partner has referrals, earnings, payouts or sub-partners — suspend instead of deleting",
      );
    }
    await client.query("DELETE FROM partner_sessions WHERE partner_id = $1", [partnerId]);
    await client.query("DELETE FROM partner_invites WHERE partner_id = $1", [partnerId]);
    await client.query("UPDATE partner_invites SET consumed_by_partner_id = NULL WHERE consumed_by_partner_id = $1", [partnerId]);
    await client.query("UPDATE partner_applications SET created_partner_id = NULL WHERE created_partner_id = $1", [partnerId]);
    await client.query("DELETE FROM partner_auth_tokens WHERE partner_id = $1", [partnerId]);
    await client.query("DELETE FROM partner_email_outbox WHERE partner_id = $1", [partnerId]);
    await client.query("DELETE FROM partners WHERE id = $1", [partnerId]);
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'partner.deleted', 'partner', $2, $3::jsonb)
    `, [actorId, partnerId, JSON.stringify({ telegramUsername: existing.rows[0].telegram_username })]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function updatePartnerAdmin(database: SalesDatabase, partnerId: string, input: {
  commissionBps?: number;
  subCommissionBps?: number;
  referralDiscountBps?: number;
  referralDiscountEnabled?: boolean;
  /**
   * B2B grant. Turning it off also zeroes the ceiling in the same statement: a leftover ceiling
   * on a revoked grant reads like authority that no longer exists, and the CHECK forbids the pair.
   */
  b2bEnabled?: boolean;
  b2bMaxDiscountBps?: number;
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
          referral_discount_bps = COALESCE($5, referral_discount_bps),
          referral_discount_enabled = COALESCE($6, referral_discount_enabled),
          b2b_enabled = COALESCE($7, b2b_enabled),
          b2b_max_discount_bps = CASE
            WHEN COALESCE($7, b2b_enabled) THEN COALESCE($8, b2b_max_discount_bps)
            ELSE 0
          END,
          status = COALESCE($4::partner_status, status),
          updated_at = now()
      WHERE id = $1
      RETURNING id
    `, [
      partnerId, input.commissionBps ?? null, input.subCommissionBps ?? null, input.status ?? null,
      input.referralDiscountBps ?? null, input.referralDiscountEnabled ?? null,
      input.b2bEnabled ?? null, input.b2bMaxDiscountBps ?? null,
    ]);
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
      // Retained marker permission/ceiling changes remain visible in the audit trail.
      referralDiscountBps: input.referralDiscountBps ?? null,
      referralDiscountEnabled: input.referralDiscountEnabled ?? null,
      // Granting or revoking the right to create B2B customers is a margin decision: it must be
      // reconstructable from the audit trail, not only from the partner's current row.
      b2bEnabled: input.b2bEnabled ?? null,
      b2bMaxDiscountBps: input.b2bMaxDiscountBps ?? null,
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
