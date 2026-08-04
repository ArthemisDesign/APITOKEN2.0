import type { Database } from "./client.js";

// Read-only финансовые агрегаты для админ-панели (admin.apitoken.sale → GET /admin/finance/*,
// GET /admin/refunds). Источник — commerce PostgreSQL (payments, checkout_sessions,
// customer_profiles, pricing_usage_events, auth_sessions); ничего не пишется и live-деньги
// движка не читаются. Все денежные суммы отдаются строками nano-USD (без JS number и float),
// агрегация выполняется на стороне БД (GROUP BY), таблицы в память не выгружаются.
//
// Возвраты: авторитет статуса — payments.status ('refunded'/'disputed'). Таблица
// engine_adjustments (движковый дебет по возврату) пока наполняется не полностью
// (AUDIT-TODO(C24) в schema.ts), поэтому список возвратов — операционный срез payments,
// а не подтверждённый engine-ledger.

export interface AdminFinanceOverview {
  revenue30dNano: string;
  revenuePrev30dNano: string;
  payments30dCount: number;
  payingUsers30d: number;
  activeUsers30d: number;
  tiers: Array<{ customerType: "b2c" | "b2b"; tier: number | null; users: number }>;
}

export interface AdminFinanceRevenueDayRow {
  day: string;
  provider: string;
  totalNano: string;
  paymentsCount: number;
}

export interface AdminFinanceFunnelRow {
  provider: string;
  created: number;
  paid: number;
  canceled: number;
  failed: number;
  expired: number;
  pending: number;
  avgSecondsToPay: number | null;
  paidTimed: number;
  paidNano: string;
}

export interface AdminFinanceTopCustomerRow {
  userId: string;
  email: string;
  totalNano: string;
  paymentsCount: number;
}

export interface AdminFinanceTopSpenderRow {
  userId: string;
  email: string;
  spentNano: string;
}

export interface AdminRefundRow {
  id: string;
  userId: string;
  email: string;
  provider: string;
  providerPaymentId: string;
  amountNano: string;
  currency: string;
  status: string;
  paidAt: Date | null;
  updatedAt: Date;
}

export interface AdminFinanceCohortRow {
  week: string;
  registered: number;
  paidUsers: number;
  medianDaysToFirstPayment: number | null;
  revenueNano: string;
}

export interface AdminFinanceChurnRow {
  userId: string;
  email: string;
  lastSeenAt: Date | null;
  lastPaidAt: Date | null;
  spent30dNano: string;
}

export type AdminPayingUserProvider = "anthropic" | "openai" | "google" | "other";
export type AdminPayingUserSort = "spent" | "paid" | "last_paid" | "last_seen";

export interface AdminPayingUsersQuery {
  days: 1 | 7 | 30;
  limit?: number;
  offset?: number;
  q?: string;
  status?: "active" | "disabled";
  provider?: AdminPayingUserProvider;
  sort?: AdminPayingUserSort;
  dir?: "asc" | "desc";
}

export interface AdminPayingUserRow {
  userId: string;
  email: string;
  displayName: string;
  status: "active" | "disabled";
  customerType: "b2c" | "b2b" | null;
  tier: number | null;
  multiplierBp: number | null;
  paidNano: string;
  paymentsCount: number;
  /** Часть paidNano, зачисленная напрямую в движке (admin-credit/ручное), без платёжной системы. */
  manualPaidNano: string;
  manualTopupsCount: number;
  lastPaidAt: Date | null;
  spentNano: string;
  providerSpendNano: Record<AdminPayingUserProvider, string>;
  activeApiKeys: number;
  lastSeenAt: Date | null;
  createdAt: Date;
}

export interface AdminPayingUsersSummary {
  payingUsers: number;
  activeSpenders: number;
  paidNano: string;
  manualPaidNano: string;
  spentNano: string;
  providerSpendNano: Record<AdminPayingUserProvider, string>;
  providerUsers: Record<AdminPayingUserProvider, number>;
}

export interface AdminPayingUsersPage {
  rows: AdminPayingUserRow[];
  total: number;
  limit: number;
  offset: number;
  days: 1 | 7 | 30;
  summary: AdminPayingUsersSummary;
}

/** Скалярная сводка: выручка текущих и предыдущих 30 дней + распределение клиентов по тирам. */
export async function getAdminFinanceOverview(database: Database): Promise<AdminFinanceOverview> {
  const [scalars, tiers] = await Promise.all([
    database.pool.query<{
      revenue_30d_nano: string;
      revenue_prev_30d_nano: string;
      payments_30d_count: string;
      paying_users_30d: string;
      active_users_30d: string;
    }>(`
      /* admin-finance:overview */
      WITH paid AS (
        SELECT user_id, amount_nano, paid_at
        FROM payments
        WHERE status = 'paid' AND paid_at >= now() - interval '60 days'
      )
      SELECT
        COALESCE(sum(amount_nano) FILTER (WHERE paid_at >= now() - interval '30 days'), 0)::text
          AS revenue_30d_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE paid_at < now() - interval '30 days'), 0)::text
          AS revenue_prev_30d_nano,
        count(*) FILTER (WHERE paid_at >= now() - interval '30 days')::text AS payments_30d_count,
        count(DISTINCT user_id) FILTER (WHERE paid_at >= now() - interval '30 days')::text
          AS paying_users_30d,
        (SELECT count(DISTINCT user_id) FROM auth_sessions
          WHERE last_seen_at >= now() - interval '30 days')::text AS active_users_30d
      FROM paid
    `),
    database.pool.query<{ customer_type: "b2c" | "b2b"; current_tier: number | null; users: string }>(`
      /* admin-finance:overview-tiers */
      SELECT customer_type::text AS customer_type, current_tier, count(*)::text AS users
      FROM customer_profiles
      GROUP BY customer_type, current_tier
      ORDER BY customer_type, current_tier
    `),
  ]);
  const row = scalars.rows[0]!;
  return {
    revenue30dNano: row.revenue_30d_nano,
    revenuePrev30dNano: row.revenue_prev_30d_nano,
    payments30dCount: Number(row.payments_30d_count),
    payingUsers30d: Number(row.paying_users_30d),
    activeUsers30d: Number(row.active_users_30d),
    tiers: tiers.rows.map((tier) => ({
      customerType: tier.customer_type,
      tier: tier.current_tier,
      users: Number(tier.users),
    })),
  };
}

/** Выручка по дням и провайдерам за окно days (status='paid', по paid_at). */
export async function listAdminFinanceRevenueDaily(
  database: Database,
  days: number,
): Promise<AdminFinanceRevenueDayRow[]> {
  const result = await database.pool.query<{
    day: string; provider: string; total_nano: string; payments_count: string;
  }>(`
    /* admin-finance:revenue-daily */
    SELECT date_trunc('day', paid_at)::date::text AS day, provider,
           sum(amount_nano)::text AS total_nano, count(*)::text AS payments_count
    FROM payments
    WHERE status = 'paid' AND paid_at >= now() - make_interval(days => $1)
    GROUP BY 1, 2
    ORDER BY 1, 2
  `, [days]);
  return result.rows.map((row) => ({
    day: row.day,
    provider: row.provider,
    totalNano: row.total_nano,
    paymentsCount: Number(row.payments_count),
  }));
}

/**
 * Воронка чекаутов за окно days по провайдерам. «Истёкшие» — checkout в creating/pending
 * с прошедшим expires_at (отдельного статуса expired в схеме нет). avg_seconds_to_pay —
 * среднее completed_at - created_at по оплаченным; paid_timed — сколько оплаченных имеют
 * completed_at (для взвешивания среднего между провайдерами).
 */
export async function getAdminFinanceFunnel(
  database: Database,
  days: number,
): Promise<AdminFinanceFunnelRow[]> {
  const result = await database.pool.query<{
    provider: string; created: string; paid: string; canceled: string; failed: string;
    expired: string; pending: string; avg_seconds_to_pay: string | null; paid_timed: string;
    paid_nano: string;
  }>(`
    /* admin-finance:funnel */
    SELECT provider,
      count(*)::text AS created,
      count(*) FILTER (WHERE status = 'paid')::text AS paid,
      count(*) FILTER (WHERE status = 'canceled')::text AS canceled,
      count(*) FILTER (WHERE status = 'failed')::text AS failed,
      count(*) FILTER (
        WHERE status IN ('creating', 'pending') AND expires_at IS NOT NULL AND expires_at < now()
      )::text AS expired,
      count(*) FILTER (
        WHERE status IN ('creating', 'pending') AND (expires_at IS NULL OR expires_at >= now())
      )::text AS pending,
      avg(EXTRACT(EPOCH FROM (completed_at - created_at)))
        FILTER (WHERE status = 'paid' AND completed_at IS NOT NULL)::text AS avg_seconds_to_pay,
      count(*) FILTER (WHERE status = 'paid' AND completed_at IS NOT NULL)::text AS paid_timed,
      COALESCE(sum(amount_nano) FILTER (WHERE status = 'paid'), 0)::text AS paid_nano
    FROM checkout_sessions
    WHERE created_at >= now() - make_interval(days => $1)
    GROUP BY provider
    ORDER BY provider
  `, [days]);
  return result.rows.map((row) => ({
    provider: row.provider,
    created: Number(row.created),
    paid: Number(row.paid),
    canceled: Number(row.canceled),
    failed: Number(row.failed),
    expired: Number(row.expired),
    pending: Number(row.pending),
    avgSecondsToPay: row.avg_seconds_to_pay === null ? null : Number(row.avg_seconds_to_pay),
    paidTimed: Number(row.paid_timed),
    paidNano: row.paid_nano,
  }));
}

/** Топ клиентов по пополнениям и по расходу за окно days + общие суммы окна для долей. */
export async function listAdminFinanceTopCustomers(
  database: Database,
  days: number,
  limit: number,
): Promise<{
  topups: AdminFinanceTopCustomerRow[];
  topupsTotalNano: string;
  spend: AdminFinanceTopSpenderRow[];
  spendTotalNano: string;
}> {
  const [topups, topupsTotal, spend, spendTotal] = await Promise.all([
    database.pool.query<{
      user_id: string; email: string; total_nano: string; payments_count: string;
    }>(`
      /* admin-finance:top-topups */
      SELECT p.user_id, u.email, sum(p.amount_nano)::text AS total_nano, count(*)::text AS payments_count
      FROM payments p
      JOIN users u ON u.id = p.user_id
      WHERE p.status = 'paid' AND p.paid_at >= now() - make_interval(days => $1)
      GROUP BY p.user_id, u.email
      ORDER BY sum(p.amount_nano) DESC, p.user_id
      LIMIT $2
    `, [days, limit]),
    database.pool.query<{ total_nano: string }>(`
      /* admin-finance:top-topups-total */
      SELECT COALESCE(sum(amount_nano), 0)::text AS total_nano
      FROM payments
      WHERE status = 'paid' AND paid_at >= now() - make_interval(days => $1)
    `, [days]),
    database.pool.query<{ user_id: string; email: string; spent_nano: string }>(`
      /* admin-finance:top-spend */
      SELECT e.user_id, u.email, sum(e.amount_nano)::text AS spent_nano
      FROM pricing_usage_events e
      JOIN users u ON u.id = e.user_id
      WHERE e.occurred_at >= now() - make_interval(days => $1)
      GROUP BY e.user_id, u.email
      ORDER BY sum(e.amount_nano) DESC, e.user_id
      LIMIT $2
    `, [days, limit]),
    database.pool.query<{ total_nano: string }>(`
      /* admin-finance:top-spend-total */
      SELECT COALESCE(sum(amount_nano), 0)::text AS total_nano
      FROM pricing_usage_events
      WHERE occurred_at >= now() - make_interval(days => $1)
    `, [days]),
  ]);
  return {
    topups: topups.rows.map((row) => ({
      userId: row.user_id,
      email: row.email,
      totalNano: row.total_nano,
      paymentsCount: Number(row.payments_count),
    })),
    topupsTotalNano: topupsTotal.rows[0]?.total_nano ?? "0",
    spend: spend.rows.map((row) => ({
      userId: row.user_id,
      email: row.email,
      spentNano: row.spent_nano,
    })),
    spendTotalNano: spendTotal.rows[0]?.total_nano ?? "0",
  };
}

const PAYING_USER_SORT_SQL: Record<AdminPayingUserSort, string> = {
  spent: "COALESCE(usage.spent_nano, 0)",
  paid: "paid.paid_nano",
  last_paid: "paid.last_paid_at",
  last_seen: "sessions.last_seen_at",
};

/**
 * Пагинированный список только когда-либо плативших клиентов. Расход берётся из immutable
 * pricing_usage_events за выбранное окно, а провайдер — из exact attribution snapshot либо
 * authoritative top-level engine ledger evidence на событии. Неатрибутированные старые события
 * и неизвестные будущие provider_id не теряются: они попадают в other.
 */
export async function listAdminPayingUsers(
  database: Database,
  query: AdminPayingUsersQuery,
): Promise<AdminPayingUsersPage> {
  if (![1, 7, 30].includes(query.days)) throw new Error(`unsupported paying users window: ${query.days}`);
  const days = query.days;
  const limit = Math.max(1, Math.min(100, query.limit ?? 50));
  const offset = Math.max(0, query.offset ?? 0);
  const q = query.q?.trim() ?? "";
  const status = query.status ?? "";
  const provider = query.provider ?? "";
  const sort = query.sort ?? "spent";
  const dir = query.dir ?? "desc";
  const sortExpr = PAYING_USER_SORT_SQL[sort];
  if (sortExpr === undefined) throw new Error(`unsupported paying users sort: ${String(query.sort)}`);
  if (dir !== "asc" && dir !== "desc") throw new Error(`unsupported paying users sort dir: ${String(query.dir)}`);
  if (provider && !["anthropic", "openai", "google", "other"].includes(provider)) {
    throw new Error(`unsupported paying users provider: ${String(query.provider)}`);
  }
  if (status && status !== "active" && status !== "disabled") {
    throw new Error(`unsupported paying users status: ${String(query.status)}`);
  }

  // «Оплачено» = подтверждённые платежи + РУЧНЫЕ пополнения баланса движка (admin-credit и
  // прочие зачисления мимо платёжной системы): это тоже реальные деньги клиента, и без них
  // клиент, оплативший вне сайта, вообще не считался платящим. Пополнения с источником
  // `payment` намеренно исключены — их авторитет payments, иначе платёж был бы посчитан дважды;
  // `bonus` (welcome/промо) деньгами не является.
  const commonCtes = `
    WITH paid AS (
      SELECT user_id,
             sum(payments_count)::bigint AS payments_count,
             sum(manual_count)::bigint AS manual_topups_count,
             sum(payments_nano) AS payments_nano,
             sum(manual_nano) AS manual_nano,
             sum(payments_nano) + sum(manual_nano) AS paid_nano,
             max(last_paid_at) AS last_paid_at
      FROM (
        SELECT user_id, count(*) AS payments_count, 0::bigint AS manual_count,
               sum(amount_nano) AS payments_nano, 0::numeric AS manual_nano,
               max(paid_at) AS last_paid_at
        FROM payments
        WHERE status = 'paid'
        GROUP BY user_id
        UNION ALL
        SELECT user_id, 0::bigint, count(*), 0::numeric, sum(amount_nano), max(occurred_at)
        FROM pricing_usage_topups
        WHERE source = 'manual'
        GROUP BY user_id
      ) sources
      GROUP BY user_id
    ), usage AS (
      SELECT e.user_id,
        sum(e.amount_nano) AS spent_nano,
        COALESCE(sum(e.amount_nano) FILTER (
          WHERE COALESCE(a.provider_id, e.provider_id) = 'anthropic'
        ), 0) AS anthropic_nano,
        COALESCE(sum(e.amount_nano) FILTER (
          WHERE COALESCE(a.provider_id, e.provider_id) = 'openai'
        ), 0) AS openai_nano,
        COALESCE(sum(e.amount_nano) FILTER (
          WHERE COALESCE(a.provider_id, e.provider_id) = 'google'
        ), 0) AS google_nano,
        COALESCE(sum(e.amount_nano) FILTER (
          WHERE COALESCE(a.provider_id, e.provider_id) IS NULL
            OR COALESCE(a.provider_id, e.provider_id) NOT IN ('anthropic', 'openai', 'google')
        ), 0) AS other_nano
      FROM pricing_usage_events e
      LEFT JOIN pricing_usage_attributions a ON a.pricing_usage_event_id = e.id
      WHERE e.occurred_at >= now() - make_interval(days => $1)
      GROUP BY e.user_id
    ), sessions AS (
      SELECT user_id, max(last_seen_at) AS last_seen_at
      FROM auth_sessions
      WHERE revoked_at IS NULL
      GROUP BY user_id
    ), api_keys AS (
      SELECT user_id, count(*) FILTER (WHERE status = 'active') AS active_count
      FROM api_keys
      GROUP BY user_id
    )
  `;
  const filters = `
    ($2::text = '' OR u.email ILIKE '%' || $2 || '%'
      OR u.display_name ILIKE '%' || $2 || '%' OR u.id::text ILIKE '%' || $2 || '%')
    AND ($3::text = '' OR u.status::text = $3)
    AND ($4::text = ''
      OR ($4 = 'anthropic' AND COALESCE(usage.anthropic_nano, 0) > 0)
      OR ($4 = 'openai' AND COALESCE(usage.openai_nano, 0) > 0)
      OR ($4 = 'google' AND COALESCE(usage.google_nano, 0) > 0)
      OR ($4 = 'other' AND COALESCE(usage.other_nano, 0) > 0))
  `;
  const [pageResult, countResult, summaryResult] = await Promise.all([
    database.pool.query<{
      user_id: string; email: string; display_name: string; status: "active" | "disabled";
      customer_type: "b2c" | "b2b" | null; current_tier: number | null; multiplier_bp: number | null;
      paid_nano: string; payments_count: string; last_paid_at: Date | null;
      manual_paid_nano: string; manual_topups_count: string; spent_nano: string;
      anthropic_nano: string; openai_nano: string; google_nano: string; other_nano: string;
      active_api_keys: string; last_seen_at: Date | null; created_at: Date;
    }>(`
      /* admin-finance:paying-users */
      ${commonCtes}
      SELECT u.id AS user_id, u.email, u.display_name, u.status,
        cp.customer_type, cp.current_tier, cp.multiplier_bp,
        paid.paid_nano::text, paid.payments_count::text, paid.last_paid_at,
        paid.manual_nano::text AS manual_paid_nano, paid.manual_topups_count::text,
        COALESCE(usage.spent_nano, 0)::text AS spent_nano,
        COALESCE(usage.anthropic_nano, 0)::text AS anthropic_nano,
        COALESCE(usage.openai_nano, 0)::text AS openai_nano,
        COALESCE(usage.google_nano, 0)::text AS google_nano,
        COALESCE(usage.other_nano, 0)::text AS other_nano,
        COALESCE(api_keys.active_count, 0)::text AS active_api_keys,
        sessions.last_seen_at, u.created_at
      FROM users u
      JOIN paid ON paid.user_id = u.id
      LEFT JOIN customer_profiles cp ON cp.user_id = u.id
      LEFT JOIN usage ON usage.user_id = u.id
      LEFT JOIN sessions ON sessions.user_id = u.id
      LEFT JOIN api_keys ON api_keys.user_id = u.id
      WHERE ${filters}
      ORDER BY ${sortExpr} ${dir === "asc" ? "ASC" : "DESC"} NULLS LAST, u.id ASC
      LIMIT $5 OFFSET $6
    `, [days, q, status, provider, limit, offset]),
    database.pool.query<{ total: string }>(`
      /* admin-finance:paying-users-count */
      ${commonCtes}
      SELECT count(*)::text AS total
      FROM users u
      JOIN paid ON paid.user_id = u.id
      LEFT JOIN usage ON usage.user_id = u.id
      WHERE ${filters}
    `, [days, q, status, provider]),
    database.pool.query<{
      paying_users: string; active_spenders: string; paid_nano: string; manual_paid_nano: string;
      spent_nano: string;
      anthropic_nano: string; openai_nano: string; google_nano: string; other_nano: string;
      anthropic_users: string; openai_users: string; google_users: string; other_users: string;
    }>(`
      /* admin-finance:paying-users-summary */
      WITH paid AS (
        SELECT user_id, sum(paid_nano) AS paid_nano, sum(manual_nano) AS manual_nano
        FROM (
          SELECT user_id, sum(amount_nano) AS paid_nano, 0::numeric AS manual_nano
          FROM payments WHERE status = 'paid' GROUP BY user_id
          UNION ALL
          SELECT user_id, sum(amount_nano), sum(amount_nano)
          FROM pricing_usage_topups WHERE source = 'manual' GROUP BY user_id
        ) sources
        GROUP BY user_id
      ), usage AS (
        SELECT e.user_id,
          sum(e.amount_nano) AS spent_nano,
          COALESCE(sum(e.amount_nano) FILTER (
            WHERE COALESCE(a.provider_id, e.provider_id) = 'anthropic'
          ), 0) AS anthropic_nano,
          COALESCE(sum(e.amount_nano) FILTER (
            WHERE COALESCE(a.provider_id, e.provider_id) = 'openai'
          ), 0) AS openai_nano,
          COALESCE(sum(e.amount_nano) FILTER (
            WHERE COALESCE(a.provider_id, e.provider_id) = 'google'
          ), 0) AS google_nano,
          COALESCE(sum(e.amount_nano) FILTER (
            WHERE COALESCE(a.provider_id, e.provider_id) IS NULL
              OR COALESCE(a.provider_id, e.provider_id) NOT IN ('anthropic', 'openai', 'google')
          ), 0) AS other_nano
        FROM pricing_usage_events e
        LEFT JOIN pricing_usage_attributions a ON a.pricing_usage_event_id = e.id
        WHERE e.occurred_at >= now() - make_interval(days => $1)
        GROUP BY e.user_id
      )
      SELECT count(*)::text AS paying_users,
        count(*) FILTER (WHERE COALESCE(usage.spent_nano, 0) > 0)::text AS active_spenders,
        COALESCE(sum(paid.paid_nano), 0)::text AS paid_nano,
        COALESCE(sum(paid.manual_nano), 0)::text AS manual_paid_nano,
        COALESCE(sum(usage.spent_nano), 0)::text AS spent_nano,
        COALESCE(sum(usage.anthropic_nano), 0)::text AS anthropic_nano,
        COALESCE(sum(usage.openai_nano), 0)::text AS openai_nano,
        COALESCE(sum(usage.google_nano), 0)::text AS google_nano,
        COALESCE(sum(usage.other_nano), 0)::text AS other_nano,
        count(*) FILTER (WHERE COALESCE(usage.anthropic_nano, 0) > 0)::text AS anthropic_users,
        count(*) FILTER (WHERE COALESCE(usage.openai_nano, 0) > 0)::text AS openai_users,
        count(*) FILTER (WHERE COALESCE(usage.google_nano, 0) > 0)::text AS google_users,
        count(*) FILTER (WHERE COALESCE(usage.other_nano, 0) > 0)::text AS other_users
      FROM paid
      LEFT JOIN usage ON usage.user_id = paid.user_id
    `, [days]),
  ]);

  const summary = summaryResult.rows[0] ?? {
    paying_users: "0", active_spenders: "0", paid_nano: "0", manual_paid_nano: "0", spent_nano: "0",
    anthropic_nano: "0", openai_nano: "0", google_nano: "0", other_nano: "0",
    anthropic_users: "0", openai_users: "0", google_users: "0", other_users: "0",
  };
  return {
    rows: pageResult.rows.map((row) => ({
      userId: row.user_id,
      email: row.email,
      displayName: row.display_name,
      status: row.status,
      customerType: row.customer_type,
      tier: row.current_tier,
      multiplierBp: row.multiplier_bp,
      paidNano: row.paid_nano,
      paymentsCount: Number(row.payments_count),
      manualPaidNano: row.manual_paid_nano,
      manualTopupsCount: Number(row.manual_topups_count),
      lastPaidAt: row.last_paid_at,
      spentNano: row.spent_nano,
      providerSpendNano: {
        anthropic: row.anthropic_nano,
        openai: row.openai_nano,
        google: row.google_nano,
        other: row.other_nano,
      },
      activeApiKeys: Number(row.active_api_keys),
      lastSeenAt: row.last_seen_at,
      createdAt: row.created_at,
    })),
    total: Number(countResult.rows[0]?.total ?? 0),
    limit,
    offset,
    days,
    summary: {
      payingUsers: Number(summary.paying_users),
      activeSpenders: Number(summary.active_spenders),
      paidNano: summary.paid_nano,
      manualPaidNano: summary.manual_paid_nano,
      spentNano: summary.spent_nano,
      providerSpendNano: {
        anthropic: summary.anthropic_nano,
        openai: summary.openai_nano,
        google: summary.google_nano,
        other: summary.other_nano,
      },
      providerUsers: {
        anthropic: Number(summary.anthropic_users),
        openai: Number(summary.openai_users),
        google: Number(summary.google_users),
        other: Number(summary.other_users),
      },
    },
  };
}

/**
 * Возвраты и диспуты: payments со статусом refunded/disputed. Авторитет — payments.status;
 * engine_adjustments (дебет движка по возврату) наполняется не полностью (AUDIT-TODO(C24)),
 * поэтому join с ним здесь намеренно отсутствует. Сортировка — по updated_at DESC (свежий
 * акт возврата первым), пагинация limit/offset + общее число и сумма всех возвратов.
 */
export async function listAdminRefunds(
  database: Database,
  limit: number,
  offset: number,
): Promise<{ rows: AdminRefundRow[]; total: number; totalNano: string }> {
  const [rows, totals] = await Promise.all([
    database.pool.query<{
      id: string; user_id: string; email: string; provider: string; provider_payment_id: string;
      amount_nano: string; currency: string; status: string; paid_at: Date | null; updated_at: Date;
    }>(`
      /* admin-finance:refunds */
      SELECT p.id, p.user_id, u.email, p.provider, p.provider_payment_id,
             p.amount_nano::text AS amount_nano, p.currency, p.status::text AS status,
             p.paid_at, p.updated_at
      FROM payments p
      JOIN users u ON u.id = p.user_id
      WHERE p.status IN ('refunded', 'disputed')
      ORDER BY p.updated_at DESC, p.id
      LIMIT $1 OFFSET $2
    `, [limit, offset]),
    database.pool.query<{ total: string; total_nano: string }>(`
      /* admin-finance:refunds-total */
      SELECT count(*)::text AS total, COALESCE(sum(amount_nano), 0)::text AS total_nano
      FROM payments
      WHERE status IN ('refunded', 'disputed')
    `),
  ]);
  return {
    rows: rows.rows.map((row) => ({
      id: row.id,
      userId: row.user_id,
      email: row.email,
      provider: row.provider,
      providerPaymentId: row.provider_payment_id,
      amountNano: row.amount_nano,
      currency: row.currency,
      status: row.status,
      paidAt: row.paid_at,
      updatedAt: row.updated_at,
    })),
    total: Number(totals.rows[0]?.total ?? 0),
    totalNano: totals.rows[0]?.total_nano ?? "0",
  };
}

/**
 * Недельные когорты регистраций: сколько зарегистрировалось, сколько из них когда-либо
 * оплатили, медиана дней до первой оплаты и вся выручка когорты (LTV-style, без ограничения
 * окном). Медиана — double precision (дни), это статистика длительности, не деньги.
 */
export async function listAdminFinanceCohorts(
  database: Database,
  weeks: number,
): Promise<AdminFinanceCohortRow[]> {
  const result = await database.pool.query<{
    week: string; registered: string; paid_users: string;
    median_days_to_first_payment: string | null; revenue_nano: string;
  }>(`
    /* admin-finance:cohorts */
    WITH cohort_users AS (
      SELECT date_trunc('week', created_at) AS week, id, created_at
      FROM users
      WHERE created_at >= now() - make_interval(weeks => $1)
    ), first_payment AS (
      SELECT user_id, min(paid_at) AS first_paid_at
      FROM payments
      WHERE status = 'paid' AND paid_at IS NOT NULL
      GROUP BY user_id
    ), revenue AS (
      SELECT user_id, sum(amount_nano) AS revenue_nano
      FROM payments
      WHERE status = 'paid'
      GROUP BY user_id
    )
    SELECT c.week::date::text AS week,
      count(*)::text AS registered,
      count(fp.first_paid_at)::text AS paid_users,
      (percentile_cont(0.5) WITHIN GROUP (
        ORDER BY EXTRACT(EPOCH FROM (fp.first_paid_at - c.created_at)) / 86400.0
      ) FILTER (WHERE fp.first_paid_at IS NOT NULL))::text AS median_days_to_first_payment,
      COALESCE(sum(r.revenue_nano), 0)::text AS revenue_nano
    FROM cohort_users c
    LEFT JOIN first_payment fp ON fp.user_id = c.id
    LEFT JOIN revenue r ON r.user_id = c.id
    GROUP BY c.week
    ORDER BY c.week
  `, [weeks]);
  return result.rows.map((row) => ({
    week: row.week,
    registered: Number(row.registered),
    paidUsers: Number(row.paid_users),
    medianDaysToFirstPayment: row.median_days_to_first_payment === null
      ? null
      : Number(row.median_days_to_first_payment),
    revenueNano: row.revenue_nano,
  }));
}

/**
 * Сигналы оттока: когда-либо платившие клиенты без активности сессий и без расхода
 * за последние days дней. «Без сессий» — max(last_seen_at) по неотозванным auth_sessions
 * старше окна (тот же критерий активности, что в списке пользователей панели).
 */
export async function listAdminFinanceChurnSignals(
  database: Database,
  days: number,
  limit: number,
): Promise<AdminFinanceChurnRow[]> {
  const result = await database.pool.query<{
    user_id: string; email: string; last_seen_at: Date | null; last_paid_at: Date | null;
    spent_30d_nano: string;
  }>(`
    /* admin-finance:churn-signals */
    SELECT u.id AS user_id, u.email, s.last_seen_at, lp.last_paid_at,
           COALESCE(sp.spent_30d, 0)::text AS spent_30d_nano
    FROM users u
    JOIN LATERAL (
      SELECT max(paid_at) AS last_paid_at, count(*) AS paid_count
      FROM payments
      WHERE user_id = u.id AND status = 'paid'
    ) lp ON TRUE
    LEFT JOIN LATERAL (
      SELECT max(last_seen_at) AS last_seen_at
      FROM auth_sessions
      WHERE user_id = u.id AND revoked_at IS NULL
    ) s ON TRUE
    LEFT JOIN LATERAL (
      SELECT sum(amount_nano) AS spent_30d
      FROM pricing_usage_events
      WHERE user_id = u.id AND occurred_at >= now() - interval '30 days'
    ) sp ON TRUE
    WHERE lp.paid_count > 0
      AND (s.last_seen_at IS NULL OR s.last_seen_at < now() - make_interval(days => $1))
      AND NOT EXISTS (
        SELECT 1 FROM pricing_usage_events e
        WHERE e.user_id = u.id AND e.occurred_at >= now() - make_interval(days => $1)
      )
    ORDER BY lp.last_paid_at DESC NULLS LAST, u.id
    LIMIT $2
  `, [days, limit]);
  return result.rows.map((row) => ({
    userId: row.user_id,
    email: row.email,
    lastSeenAt: row.last_seen_at,
    lastPaidAt: row.last_paid_at,
    spent30dNano: row.spent_30d_nano,
  }));
}

export interface AdminEngineAccountOwner {
  engineAccountId: string;
  userId: string;
  email: string;
  displayName: string;
  status: "active" | "disabled";
  customerType: "b2c" | "b2b" | null;
}

/**
 * Справочник «engine-аккаунт → клиент коммерции». Нужен админской странице расхода движка:
 * `/spend-stats` знает только `account`/`handle`, а кто за ним стоит (и стоит ли вообще —
 * OpenKeys и внутренние аккаунты commerce-юзера не имеют) знает только эта таблица.
 */
export async function listAdminEngineAccountOwners(
  database: Database,
): Promise<AdminEngineAccountOwner[]> {
  const result = await database.pool.query<{
    engine_account_id: string; user_id: string; email: string; display_name: string;
    status: "active" | "disabled"; customer_type: "b2c" | "b2b" | null;
  }>(`
    /* admin-finance:engine-account-owners */
    SELECT ea.engine_account_id, u.id AS user_id, u.email, u.display_name, u.status,
           cp.customer_type
    FROM engine_accounts ea
    JOIN users u ON u.id = ea.user_id
    LEFT JOIN customer_profiles cp ON cp.user_id = u.id
    WHERE ea.engine_account_id IS NOT NULL
  `);
  return result.rows.map((row) => ({
    engineAccountId: row.engine_account_id,
    userId: row.user_id,
    email: row.email,
    displayName: row.display_name,
    status: row.status,
    customerType: row.customer_type,
  }));
}
