import type { SalesDatabase } from "./client.js";
import type { PartnerStatus } from "./auth.js";

// Аналитическая модель партнёров для админки: одна строка на партнёра с денежными и активностными
// агрегатами (всё готово к отдаче в API — nano как строки, счётчики числами, даты ISO). Плюс единая
// лента активности (getPartnerActivity), собранная из данных, а не только из audit-log.

export const PARTNER_ANALYTICS_SORTS = [
  "deposits_total", "deposits_30d", "referred_users", "converted_users",
  "spend_total", "spend_30d", "earned_total", "earned_30d", "unpaid",
  "team_size", "last_seen_at", "created_at",
] as const;
export type PartnerAnalyticsSort = (typeof PARTNER_ANALYTICS_SORTS)[number];

const SORT_EXPR: Record<PartnerAnalyticsSort, string> = {
  deposits_total: "t.deposits_total", deposits_30d: "t.deposits_30d",
  referred_users: "t.referred_users", converted_users: "t.converted_users",
  spend_total: "t.spend_total", spend_30d: "t.spend_30d",
  earned_total: "t.earned_total", earned_30d: "t.earned_30d",
  unpaid: "payable_nano", team_size: "t.team_size",
  last_seen_at: "t.last_seen_at", created_at: "t.created_at",
};

export interface PartnerAnalyticsRow {
  id: string;
  email: string | null;
  telegramUsername: string | null;
  displayName: string | null;
  status: PartnerStatus;
  referralCode: string;
  parentId: string | null;
  parentLabel: string | null;
  commissionBps: number;
  subCommissionBps: number;
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  parentOverrideBps: number | null;
  referralDiscountEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
  b2bGrantSourcePartnerId: string | null;
  referralDiscountBps: number;
  promoEnabled: boolean;
  depositsTotalNano: string;
  deposits30dNano: string;
  referredUsers: number;
  convertedUsers: number; // рефералы, сделавшие ≥1 реальный депозит
  spendTotalNano: string;
  spend30dNano: string;
  earnedTotalNano: string;
  earned30dNano: string;
  adjustmentTotalNano: string;
  adjustment30dNano: string;
  netTotalNano: string;
  net30dNano: string;
  paidNano: string;
  unpaidNano: string; // legacy gross − committed; retained for expand-only API compatibility
  debtNano: string; // paid − net, clamped at zero
  payableNano: string; // net − committed(paid+in-flight), clamped at zero
  teamSize: number;
  linksTotal: number;
  linksUsed: number;
  promosTotal: number;
  promosUsed: number;
  lastSeenAt: string | null;
  lastReferralAt: string | null;
  lastDepositAt: string | null;
  createdAt: string;
}

export interface PartnerAnalyticsTotals {
  total: number;
  active: number;
  depositsNano: string;
  referredUsers: number;
  convertedUsers: number;
  unpaidNano: string; // legacy gross − committed
  adjustmentsNano: string;
  netCommissionsNano: string;
  debtNano: string;
  payableNano: string;
}

export interface PartnerAnalyticsListResult {
  items: PartnerAnalyticsRow[];
  totals: PartnerAnalyticsTotals;
}

export interface ListPartnerAnalyticsOptions {
  sort?: PartnerAnalyticsSort;
  dir?: "asc" | "desc";
  status?: PartnerStatus | "all";
  search?: string;
  partnerId?: string; // ограничить одним партнёром (для детальной карточки)
  limit?: number;
  offset?: number;
}

// Собирает WHERE по фильтрам (status/search/id) поверх таблицы partners p; возвращает clause и params.
function partnerFilter(status: PartnerStatus | "all", search: string, partnerId: string | undefined, startIndex: number): { clause: string; params: unknown[] } {
  const conds: string[] = [];
  const params: unknown[] = [];
  if (partnerId) { params.push(partnerId); conds.push(`p.id = $${startIndex + params.length}`); }
  if (status && status !== "all") { params.push(status); conds.push(`p.status = $${startIndex + params.length}`); }
  const q = search.trim();
  if (q) {
    params.push(`%${q}%`);
    const i = startIndex + params.length;
    conds.push(`(p.telegram_username ILIKE $${i} OR p.email ILIKE $${i} OR p.display_name ILIKE $${i} OR p.referral_code ILIKE $${i})`);
  }
  return { clause: conds.length ? `WHERE ${conds.join(" AND ")}` : "", params };
}

/** Одна аналитическая строка по партнёру (для детальной карточки), либо null. */
export async function getPartnerAnalytics(database: SalesDatabase, partnerId: string): Promise<PartnerAnalyticsRow | null> {
  const { items } = await listPartnerAnalytics(database, { partnerId, limit: 1 });
  return items[0] ?? null;
}

export async function listPartnerAnalytics(
  database: SalesDatabase,
  options: ListPartnerAnalyticsOptions = {},
): Promise<PartnerAnalyticsListResult> {
  const sort: PartnerAnalyticsSort = PARTNER_ANALYTICS_SORTS.includes(options.sort as PartnerAnalyticsSort)
    ? (options.sort as PartnerAnalyticsSort) : "deposits_total";
  const dir = options.dir === "asc" ? "ASC" : "DESC";
  const status = options.status ?? "all";
  const search = options.search ?? "";
  const limit = Math.min(Math.max(options.limit ?? 100, 1), 500);
  const offset = Math.max(options.offset ?? 0, 0);

  const filter = partnerFilter(status, search, options.partnerId, 0);
  // Тяжёлые пер-партнёрские агрегаты считаем один раз, во внутреннем select; внешний добавляет unpaid,
  // сортировку и пагинацию. NULLS LAST держит пустые last_seen/deposits внизу при DESC.
  const inner = `
    SELECT
      p.id, p.email, p.telegram_username, p.display_name, p.status, p.referral_code,
      p.parent_partner_id AS parent_id,
      COALESCE(parent.telegram_username, parent.email) AS parent_label,
      p.commission_bps, p.sub_commission_bps,
      COALESCE(p.team_override_max_bps, 2000) AS team_override_max_bps,
      p.team_invites_enabled, p.parent_override_bps, p.referral_discount_enabled, p.referral_discount_bps,
      p.b2b_enabled, p.b2b_max_discount_bps, p.b2b_can_delegate, p.b2b_grant_source_partner_id,
      p.promo_enabled, p.created_at,
      COALESCE((SELECT SUM(rt.amount_nano) FROM referred_topups rt WHERE rt.partner_id = p.id), 0) AS deposits_total,
      COALESCE((SELECT SUM(rt.amount_nano) FROM referred_topups rt WHERE rt.partner_id = p.id AND rt.paid_at >= now() - interval '30 days'), 0) AS deposits_30d,
      (SELECT count(*) FROM referred_users ru WHERE ru.partner_id = p.id) AS referred_users,
      (SELECT count(DISTINCT rt.commerce_user_id) FROM referred_topups rt WHERE rt.partner_id = p.id) AS converted_users,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT partner_id, amount_nano FROM partner_usage_events
        UNION ALL
        SELECT partner_id, paid_funded_nano FROM partner_usage_events_v2
      ) pue WHERE pue.partner_id = p.id), 0) AS spend_total,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT partner_id, amount_nano, occurred_at FROM partner_usage_events
        UNION ALL
        SELECT partner_id, paid_funded_nano, occurred_at FROM partner_usage_events_v2
      ) pue WHERE pue.partner_id = p.id AND pue.occurred_at >= now() - interval '30 days'), 0) AS spend_30d,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT partner_id, amount_nano FROM commission_entries
        UNION ALL
        SELECT partner_id, amount_nano FROM commission_entries_v2
      ) ce WHERE ce.partner_id = p.id), 0) AS earned_total,
      COALESCE((SELECT SUM(amount_nano) FROM (
        SELECT partner_id, amount_nano, created_at FROM commission_entries
        UNION ALL
        SELECT partner_id, amount_nano, created_at FROM commission_entries_v2
      ) ce WHERE ce.partner_id = p.id AND ce.created_at >= now() - interval '30 days'), 0) AS earned_30d,
      COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
                WHERE adjustment.partner_id = p.id), 0) AS adjustment_total,
      COALESCE((SELECT SUM(amount_nano) FROM partner_commission_adjustments adjustment
                WHERE adjustment.partner_id = p.id
                  AND adjustment.effective_at >= now() - interval '30 days'), 0) AS adjustment_30d,
      COALESCE((SELECT SUM(po.amount_nano) FROM payouts po WHERE po.partner_id = p.id AND po.status = 'paid'), 0) AS paid_total,
      COALESCE((SELECT SUM(po.amount_nano) FROM payouts po WHERE po.partner_id = p.id AND po.status IN ('requested','approved','paid')), 0) AS committed_total,
      (SELECT count(*) FROM partners c WHERE c.parent_partner_id = p.id) AS team_size,
      (SELECT count(*) FROM partner_discount_links dl WHERE dl.partner_id = p.id) AS links_total,
      (SELECT count(*) FROM partner_discount_links dl WHERE dl.partner_id = p.id AND dl.consumed_at IS NOT NULL) AS links_used,
      (SELECT count(*) FROM promo_codes pc WHERE pc.partner_id = p.id) AS promos_total,
      (SELECT count(*) FROM promo_codes pc WHERE pc.partner_id = p.id AND pc.status = 'redeemed') AS promos_used,
      (SELECT max(s.last_seen_at) FROM partner_sessions s WHERE s.partner_id = p.id) AS last_seen_at,
      (SELECT max(ru.attributed_at) FROM referred_users ru WHERE ru.partner_id = p.id) AS last_referral_at,
      (SELECT max(rt.paid_at) FROM referred_topups rt WHERE rt.partner_id = p.id) AS last_deposit_at
    FROM partners p
    LEFT JOIN partners parent ON parent.id = p.parent_partner_id
    ${filter.clause}
  `;
  const rowsSql = `
    SELECT t.*,
      (t.earned_total - t.committed_total) AS unpaid_nano,
      (t.earned_total + t.adjustment_total) AS net_total,
      (t.earned_30d + t.adjustment_30d) AS net_30d,
      GREATEST(t.paid_total - (t.earned_total + t.adjustment_total), 0) AS debt_nano,
      GREATEST((t.earned_total + t.adjustment_total) - t.committed_total, 0) AS payable_nano
    FROM (${inner}) t
    ORDER BY ${SORT_EXPR[sort]} ${dir} NULLS LAST, t.created_at DESC
    LIMIT $${filter.params.length + 1} OFFSET $${filter.params.length + 2}
  `;
  const rows = await database.pool.query(rowsSql, [...filter.params, limit, offset]);

  // Итоги по ВСЕМУ отфильтрованному набору (без пагинации) — лёгкими агрегатами, без пер-партнёрских
  // подзапросов. Фильтр использует те же позиции $1..$k в каждом подзапросе (Postgres допускает
  // повторное использование одного параметра), поэтому передаём filter.params один раз.
  const andActive = filter.clause ? " AND" : "WHERE";
  const totalsSql = `
    SELECT
      (SELECT count(*) FROM partners p ${filter.clause})::int AS total,
      (SELECT count(*) FROM partners p ${filter.clause}${andActive} p.status = 'active')::int AS active,
      COALESCE((SELECT SUM(rt.amount_nano) FROM referred_topups rt JOIN partners p ON p.id = rt.partner_id ${filter.clause}), 0)::text AS deposits,
      (SELECT count(*) FROM referred_users ru JOIN partners p ON p.id = ru.partner_id ${filter.clause})::int AS referred,
      (SELECT count(DISTINCT rt.commerce_user_id) FROM referred_topups rt JOIN partners p ON p.id = rt.partner_id ${filter.clause})::int AS converted,
      (
        COALESCE((SELECT SUM(ce.amount_nano) FROM (
                    SELECT partner_id, amount_nano FROM commission_entries
                    UNION ALL
                    SELECT partner_id, amount_nano FROM commission_entries_v2
                  ) ce JOIN partners p ON p.id = ce.partner_id ${filter.clause}), 0)
        - COALESCE((SELECT SUM(po.amount_nano) FROM payouts po JOIN partners p ON p.id = po.partner_id ${filter.clause}${andActive} po.status IN ('requested','approved','paid')), 0)
      )::text AS unpaid
      ,COALESCE((SELECT SUM(adjustment.amount_nano)
                 FROM partner_commission_adjustments adjustment
                 JOIN partners p ON p.id = adjustment.partner_id ${filter.clause}), 0)::text AS adjustments
      ,(
        COALESCE((SELECT SUM(ce.amount_nano) FROM (
                    SELECT partner_id, amount_nano FROM commission_entries
                    UNION ALL
                    SELECT partner_id, amount_nano FROM commission_entries_v2
                  ) ce JOIN partners p ON p.id = ce.partner_id ${filter.clause}), 0)
        + COALESCE((SELECT SUM(adjustment.amount_nano)
                    FROM partner_commission_adjustments adjustment
                    JOIN partners p ON p.id = adjustment.partner_id ${filter.clause}), 0)
      )::text AS net_commissions
      ,COALESCE((
        SELECT SUM(GREATEST(row.paid_total - row.net_total, 0)) FROM (
          SELECT p.id,
            COALESCE((SELECT SUM(ce.amount_nano) FROM (
              SELECT partner_id, amount_nano FROM commission_entries
              UNION ALL
              SELECT partner_id, amount_nano FROM commission_entries_v2
            ) ce WHERE ce.partner_id = p.id), 0)
            + COALESCE((SELECT SUM(adjustment.amount_nano)
                        FROM partner_commission_adjustments adjustment
                        WHERE adjustment.partner_id = p.id), 0) AS net_total,
            COALESCE((SELECT SUM(po.amount_nano) FROM payouts po
                      WHERE po.partner_id = p.id AND po.status = 'paid'), 0) AS paid_total
          FROM partners p ${filter.clause}
        ) row
      ), 0)::text AS debt
      ,COALESCE((
        SELECT SUM(GREATEST(row.net_total - row.committed_total, 0)) FROM (
          SELECT p.id,
            COALESCE((SELECT SUM(ce.amount_nano) FROM (
              SELECT partner_id, amount_nano FROM commission_entries
              UNION ALL
              SELECT partner_id, amount_nano FROM commission_entries_v2
            ) ce WHERE ce.partner_id = p.id), 0)
            + COALESCE((SELECT SUM(adjustment.amount_nano)
                        FROM partner_commission_adjustments adjustment
                        WHERE adjustment.partner_id = p.id), 0) AS net_total,
            COALESCE((SELECT SUM(po.amount_nano) FROM payouts po
                      WHERE po.partner_id = p.id
                        AND po.status IN ('requested','approved','paid')), 0) AS committed_total
          FROM partners p ${filter.clause}
        ) row
      ), 0)::text AS payable
  `;
  const totalsRes = await database.pool.query<{
    total: number; active: number; deposits: string; referred: number; converted: number;
    unpaid: string; adjustments: string; net_commissions: string; debt: string; payable: string;
  }>(totalsSql, filter.params);
  const tr = totalsRes.rows[0]!;

  return {
    items: rows.rows.map(serializeRow),
    totals: {
      total: tr.total, active: tr.active, depositsNano: tr.deposits,
      referredUsers: tr.referred, convertedUsers: tr.converted, unpaidNano: tr.unpaid,
      adjustmentsNano: tr.adjustments, netCommissionsNano: tr.net_commissions,
      debtNano: tr.debt, payableNano: tr.payable,
    },
  };
}

function serializeRow(r: Record<string, unknown>): PartnerAnalyticsRow {
  const s = (v: unknown): string => (v === null || v === undefined ? "0" : String(v));
  const iso = (v: unknown): string | null => (v ? new Date(v as string | Date).toISOString() : null);
  return {
    id: r.id as string,
    email: (r.email as string) ?? null,
    telegramUsername: (r.telegram_username as string) ?? null,
    displayName: (r.display_name as string) ?? null,
    status: r.status as PartnerStatus,
    referralCode: r.referral_code as string,
    parentId: (r.parent_id as string) ?? null,
    parentLabel: (r.parent_label as string) ?? null,
    commissionBps: Number(r.commission_bps),
    subCommissionBps: Number(r.sub_commission_bps),
    teamOverrideMaxBps: Number(r.team_override_max_bps),
    teamInvitesEnabled: Boolean(r.team_invites_enabled),
    parentOverrideBps: r.parent_override_bps === null || r.parent_override_bps === undefined
      ? null
      : Number(r.parent_override_bps),
    referralDiscountEnabled: Boolean(r.referral_discount_enabled),
    b2bEnabled: Boolean(r.b2b_enabled),
    b2bMaxDiscountBps: Number(r.b2b_max_discount_bps ?? 0),
    b2bCanDelegate: Boolean(r.b2b_can_delegate),
    b2bGrantSourcePartnerId: (r.b2b_grant_source_partner_id as string) ?? null,
    referralDiscountBps: Number(r.referral_discount_bps),
    promoEnabled: Boolean(r.promo_enabled),
    depositsTotalNano: s(r.deposits_total),
    deposits30dNano: s(r.deposits_30d),
    referredUsers: Number(r.referred_users),
    convertedUsers: Number(r.converted_users),
    spendTotalNano: s(r.spend_total),
    spend30dNano: s(r.spend_30d),
    earnedTotalNano: s(r.earned_total),
    earned30dNano: s(r.earned_30d),
    adjustmentTotalNano: s(r.adjustment_total),
    adjustment30dNano: s(r.adjustment_30d),
    netTotalNano: s(r.net_total),
    net30dNano: s(r.net_30d),
    paidNano: s(r.paid_total),
    unpaidNano: s(r.unpaid_nano),
    debtNano: s(r.debt_nano),
    payableNano: s(r.payable_nano),
    teamSize: Number(r.team_size),
    linksTotal: Number(r.links_total),
    linksUsed: Number(r.links_used),
    promosTotal: Number(r.promos_total),
    promosUsed: Number(r.promos_used),
    lastSeenAt: iso(r.last_seen_at),
    lastReferralAt: iso(r.last_referral_at),
    lastDepositAt: iso(r.last_deposit_at),
    createdAt: new Date(r.created_at as string | Date).toISOString(),
  };
}

export type PartnerActivityType =
  | "referral" | "deposit" | "discount_link_created" | "discount_link_used"
  | "promo_created" | "promo_redeemed" | "payout_requested" | "payout_decided"
  | "login" | "admin";

export interface PartnerActivityEvent {
  type: PartnerActivityType;
  at: string; // ISO
  amountNano: string | null;
  label: string; // короткая сводка
  meta: Record<string, unknown>;
}

/**
 * Единая лента активности партнёра: собрана из фактических данных (рефералы, депозиты, ссылки,
 * промо, выплаты, входы) И из sales_audit_log (админские действия над партнёром). Свежие сверху.
 */
export async function getPartnerActivity(
  database: SalesDatabase,
  partnerId: string,
  limit = 60,
): Promise<PartnerActivityEvent[]> {
  const capped = Math.min(Math.max(limit, 1), 200);
  const result = await database.pool.query<{
    type: PartnerActivityType; at: Date | null; amount_nano: string | null; label: string; meta: Record<string, unknown>;
  }>(`
    (SELECT 'referral'::text AS type, ru.attributed_at AS at, NULL::text AS amount_nano,
            'New referral ' || left(ru.commerce_user_id::text, 8) AS label,
            jsonb_build_object('commerceUserId', ru.commerce_user_id) AS meta
       FROM referred_users ru WHERE ru.partner_id = $1)
    UNION ALL
    (SELECT 'deposit', rt.paid_at, rt.amount_nano::text,
            'Referred deposit ' || left(rt.commerce_user_id::text, 8),
            jsonb_build_object('commerceUserId', rt.commerce_user_id)
       FROM referred_topups rt WHERE rt.partner_id = $1)
    UNION ALL
    (SELECT 'discount_link_created', dl.created_at, NULL,
            'Issued legacy marker link ' || dl.code, jsonb_build_object('code', dl.code, 'discountBps', dl.discount_bps, 'note', dl.note)
       FROM partner_discount_links dl WHERE dl.partner_id = $1)
    UNION ALL
    (SELECT 'discount_link_used', dl.consumed_at, NULL,
            'Legacy marker link ' || dl.code || ' used', jsonb_build_object('code', dl.code)
       FROM partner_discount_links dl WHERE dl.partner_id = $1 AND dl.consumed_at IS NOT NULL)
    UNION ALL
    (SELECT 'promo_created', pc.created_at, pc.value_nano::text,
            'Created promo ' || pc.code, jsonb_build_object('code', pc.code)
       FROM promo_codes pc WHERE pc.partner_id = $1)
    UNION ALL
    (SELECT 'promo_redeemed', pc.redeemed_at, pc.value_nano::text,
            'Promo ' || pc.code || ' redeemed', jsonb_build_object('code', pc.code)
       FROM promo_codes pc WHERE pc.partner_id = $1 AND pc.redeemed_at IS NOT NULL)
    UNION ALL
    (SELECT 'payout_requested', po.requested_at, po.amount_nano::text,
            'Payout requested', jsonb_build_object('status', po.status)
       FROM payouts po WHERE po.partner_id = $1)
    UNION ALL
    (SELECT 'payout_decided', po.decided_at, po.amount_nano::text,
            'Payout ' || po.status, jsonb_build_object('status', po.status, 'note', po.admin_note)
       FROM payouts po WHERE po.partner_id = $1 AND po.decided_at IS NOT NULL)
    UNION ALL
    (SELECT 'login', s.created_at, NULL,
            'Signed in', jsonb_build_object('ua', left(COALESCE(s.user_agent, ''), 80))
       FROM partner_sessions s WHERE s.partner_id = $1)
    UNION ALL
    (SELECT 'admin', a.created_at, NULL,
            'Admin: ' || a.action, a.metadata
       FROM sales_audit_log a WHERE a.target_type = 'partner' AND a.target_id = $1::text)
    ORDER BY at DESC NULLS LAST
    LIMIT $2
  `, [partnerId, capped]);

  return result.rows
    .filter((row) => row.at !== null)
    .map((row) => ({
      type: row.type,
      at: new Date(row.at as Date).toISOString(),
      amountNano: row.amount_nano,
      label: row.label,
      meta: row.meta ?? {},
    }));
}
