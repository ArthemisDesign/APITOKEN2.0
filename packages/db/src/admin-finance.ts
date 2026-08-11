import type { Database } from "./client.js";

// Read-only финансовые агрегаты для админ-панели (admin.apitoken.sale → GET /admin/finance/*,
// GET /admin/refunds). Источник — commerce PostgreSQL (payments, checkout_sessions,
// customer_profiles, pricing_usage_events, auth_sessions); ничего не пишется и live-деньги
// движка не читаются. Все денежные суммы отдаются строками nano-USD (без JS number и float),
// агрегация выполняется на стороне БД (GROUP BY), таблицы в память не выгружаются.
//
// Возвраты: авторитет статуса — payments.status ('refunded'/'disputed'). Компенсация live-баланса
// видна отдельно через durable engine_adjustments: статус платежа не откатывается из-за временной
// недоступности движка, а админка показывает, подтверждён ли идемпотентный дебет.

export interface AdminFinanceOverview {
  revenue30dNano: string;
  revenuePrev30dNano: string;
  payments30dCount: number;
  payingUsers30d: number;
  activeUsers30d: number;
  customerClasses: Array<{ customerType: "b2c" | "b2b"; users: number }>;
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
  adjustmentStatus: string | null;
  adjustmentConfirmedAt: Date | null;
  adjustmentLastError: string | null;
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

export type AdminPayingUserProvider = "anthropic" | "openai" | "google" | "kimi" | "other";
export type AdminPayingUserSort = "spent" | "paid" | "last_paid" | "last_seen";
export type AdminPayingUserFunding = "payments" | "manual" | "bonus" | "all" | "spenders";
export type AdminPayingUserFundingKind =
  | "payments"
  | "payments_and_manual"
  | "manual"
  | "bonus_only"
  | "spend_only";

export interface AdminPayingUsersQuery {
  days: 1 | 7 | 30;
  limit?: number;
  offset?: number;
  q?: string;
  status?: "active" | "disabled";
  provider?: AdminPayingUserProvider;
  /**
   * Когорта по источнику денег. Отсутствующий фильтр сохраняет прежний payment/manual union;
   * `bonus` означает строгий bonus-only расход выбранного окна, `all` объединяет эти старые
   * когорты, а `spenders` включает любой положительный расход окна. Как определение когорты
   * фильтр сужает и строки, и сводку.
   */
  funding?: AdminPayingUserFunding;
  /** Opt-in live engine usage enrichment; omitted/false keeps the endpoint commerce-DB-only. */
  includeUsage?: boolean;
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
  fundingKind: AdminPayingUserFundingKind;
  paidNano: string;
  paymentsCount: number;
  /** Часть paidNano из ручных внешних зачислений; подарочные admin-credit исключены. */
  manualPaidNano: string;
  manualTopupsCount: number;
  lastPaidAt: Date | null;
  spentNano: string;
  paidFundedSpentNano: string;
  bonusFundedSpentNano: string;
  otherFundedSpentNano: string;
  unattributedSpentNano: string;
  providerSpendNano: Record<AdminPayingUserProvider, string>;
  engineAccountId: string | null;
  usageAccountIds: string[];
  activeApiKeys: number;
  lastSeenAt: Date | null;
  createdAt: Date;
}

export interface AdminPayingUsersSummary {
  /** Backward-compatible money-funded count; use cohortUsers for the complete selected cohort. */
  payingUsers: number;
  cohortUsers: number;
  bonusOnlyUsers: number;
  activeSpenders: number;
  paidNano: string;
  manualPaidNano: string;
  spentNano: string;
  bonusOnlySpentNano: string;
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

/** Скалярная сводка: выручка текущих и предыдущих 30 дней + распределение клиентов по классам. */
export async function getAdminFinanceOverview(database: Database): Promise<AdminFinanceOverview> {
  const [scalars, classes] = await Promise.all([
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
    database.pool.query<{ customer_type: "b2c" | "b2b"; users: string }>(`
      /* admin-finance:overview-customer-classes */
      SELECT customer_type::text AS customer_type, count(*)::text AS users
      FROM customer_profiles
      GROUP BY customer_type
      ORDER BY customer_type
    `),
  ]);
  const row = scalars.rows[0]!;
  return {
    revenue30dNano: row.revenue_30d_nano,
    revenuePrev30dNano: row.revenue_prev_30d_nano,
    payments30dCount: Number(row.payments_30d_count),
    payingUsers30d: Number(row.paying_users_30d),
    activeUsers30d: Number(row.active_users_30d),
    customerClasses: classes.rows.map((item) => ({
      customerType: item.customer_type,
      users: Number(item.users),
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
 * Пагинированный money-funded, строгий bonus-only или полный spender cohort. Расход берётся
 * из immutable pricing_usage_events за выбранное окно, а провайдер — из exact attribution snapshot
 * либо authoritative top-level engine ledger evidence на событии. Bonus-only требует modern split
 * для каждого события окна; остальные spend-only строки сохраняют legacy/неполную атрибуцию.
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
  const funding = query.funding ?? "";
  const sort = query.sort ?? "spent";
  const dir = query.dir ?? "desc";
  const sortExpr = PAYING_USER_SORT_SQL[sort];
  if (sortExpr === undefined) throw new Error(`unsupported paying users sort: ${String(query.sort)}`);
  if (dir !== "asc" && dir !== "desc") throw new Error(`unsupported paying users sort dir: ${String(query.dir)}`);
  if (provider && !["anthropic", "openai", "google", "kimi", "other"].includes(provider)) {
    throw new Error(`unsupported paying users provider: ${String(query.provider)}`);
  }
  if (status && status !== "active" && status !== "disabled") {
    throw new Error(`unsupported paying users status: ${String(query.status)}`);
  }
  if (funding && !["payments", "manual", "bonus", "all", "spenders"].includes(funding)) {
    throw new Error(`unsupported paying users funding: ${String(query.funding)}`);
  }

  // `money` is lifetime payment/manual authority. Historical admin-credit rows were initially
  // stored as `manual`; exclude them by immutable ref as well as classifying all new rows as bonus.
  // This read-time compatibility rule corrects finance without rewriting the ledger copy.
  // `usage` is selected-window charge authority.
  // The paid/bonus split of spend comes from the one authority that records it: free-first
  // accounting on the immutable usage event. real_funded_nano is the part the customer paid for;
  // the remainder was covered by free credit. No balance/model/top-up proxy participates.
  const commonCtes = (fundingParameter: number) => `
    WITH money AS (
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
        WHERE source = 'manual' AND (ref IS NULL OR ref NOT LIKE 'admin-credit:%')
        GROUP BY user_id
      ) sources
      GROUP BY user_id
    ), window_events AS (
      SELECT e.user_id, e.engine_account_id, e.amount_nano, e.provider_id,
        e.real_funded_nano >= 0 AND e.real_funded_nano <= e.amount_nano AS exact_modern_funding,
        LEAST(GREATEST(e.real_funded_nano, 0), e.amount_nano) AS paid_funded_nano,
        e.amount_nano - LEAST(GREATEST(e.real_funded_nano, 0), e.amount_nano) AS bonus_funded_nano,
        0::numeric AS other_funded_nano
      FROM pricing_usage_events e
      -- now() is one coherent transaction-start timestamp at microsecond precision: an event
      -- written microseconds before this query stays inside the window (a JavaScript Date
      -- carries only milliseconds and could truncate above such a row), and all three cohort
      -- queries share the exact same window end.
      WHERE e.occurred_at >= now() - make_interval(days => $1)
        AND e.occurred_at < now()
    ), usage AS (
      SELECT user_id,
        array_agg(DISTINCT engine_account_id ORDER BY engine_account_id) AS event_account_ids,
        sum(amount_nano) AS spent_nano,
        count(*) AS event_count,
        count(*) FILTER (WHERE exact_modern_funding) AS exact_modern_event_count,
        sum(paid_funded_nano) AS paid_funded_nano,
        sum(bonus_funded_nano) AS bonus_funded_nano,
        sum(other_funded_nano) AS other_funded_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE NOT exact_modern_funding), 0) AS unattributed_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE provider_id = 'anthropic'), 0) AS anthropic_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE provider_id = 'openai'), 0) AS openai_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE provider_id = 'google'), 0) AS google_nano,
        COALESCE(sum(amount_nano) FILTER (WHERE provider_id = 'kimi'), 0) AS kimi_nano,
        -- Every named provider must also leave this bucket, or its spend would be counted twice:
        -- once in its own column and once as "other".
        COALESCE(sum(amount_nano) FILTER (
          WHERE provider_id IS NULL OR provider_id NOT IN ('anthropic', 'openai', 'google', 'kimi')
        ), 0) AS other_nano
      FROM window_events
      GROUP BY user_id
    ), paid AS (
      SELECT u.id AS user_id,
        COALESCE(money.payments_count, 0) AS payments_count,
        COALESCE(money.manual_topups_count, 0) AS manual_topups_count,
        COALESCE(money.payments_nano, 0) AS payments_nano,
        COALESCE(money.manual_nano, 0) AS manual_nano,
        COALESCE(money.paid_nano, 0) AS paid_nano,
        money.last_paid_at,
        CASE
          WHEN COALESCE(money.payments_count, 0) > 0 AND COALESCE(money.manual_topups_count, 0) > 0
            THEN 'payments_and_manual'
          WHEN COALESCE(money.payments_count, 0) > 0 THEN 'payments'
          WHEN COALESCE(money.manual_topups_count, 0) > 0 THEN 'manual'
          WHEN COALESCE(usage.spent_nano, 0) > 0
            AND usage.event_count = usage.exact_modern_event_count
            AND usage.paid_funded_nano = 0
            AND usage.bonus_funded_nano = usage.spent_nano
            AND usage.other_funded_nano = 0
            AND usage.unattributed_nano = 0
            THEN 'bonus_only'
          ELSE 'spend_only'
        END AS funding_kind
      FROM users u
      LEFT JOIN money ON money.user_id = u.id
      LEFT JOIN usage ON usage.user_id = u.id
      WHERE (
        ($${fundingParameter}::text IN ('', 'all')
          AND (COALESCE(money.payments_count, 0) > 0 OR COALESCE(money.manual_topups_count, 0) > 0))
        OR ($${fundingParameter} = 'payments' AND COALESCE(money.payments_count, 0) > 0)
        OR ($${fundingParameter} = 'manual'
          AND COALESCE(money.manual_topups_count, 0) > 0
          AND COALESCE(money.payments_count, 0) = 0)
        OR ($${fundingParameter} IN ('bonus', 'all')
          AND COALESCE(money.payments_count, 0) = 0
          AND COALESCE(money.manual_topups_count, 0) = 0
          AND COALESCE(usage.spent_nano, 0) > 0
          AND usage.event_count = usage.exact_modern_event_count
          AND usage.paid_funded_nano = 0
          AND usage.bonus_funded_nano = usage.spent_nano
          AND usage.other_funded_nano = 0
          AND usage.unattributed_nano = 0)
        OR ($${fundingParameter} = 'spenders' AND COALESCE(usage.spent_nano, 0) > 0)
      )
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
      OR ($4 = 'kimi' AND COALESCE(usage.kimi_nano, 0) > 0)
      OR ($4 = 'other' AND COALESCE(usage.other_nano, 0) > 0))
  `;
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    const [pageResult, countResult, summaryResult] = [
      await client.query<{
      user_id: string; email: string; display_name: string; status: "active" | "disabled";
      customer_type: "b2c" | "b2b" | null; current_tier: number | null; multiplier_bp: number | null;
      funding_kind: AdminPayingUserFundingKind; paid_nano: string; payments_count: string;
      last_paid_at: Date | null; manual_paid_nano: string; manual_topups_count: string;
      spent_nano: string; paid_funded_nano: string; bonus_funded_nano: string;
      other_funded_nano: string; unattributed_nano: string;
      anthropic_nano: string; openai_nano: string; google_nano: string; kimi_nano: string;
      other_nano: string;
      engine_account_id: string | null; usage_account_ids: string[]; active_api_keys: string;
      last_seen_at: Date | null; created_at: Date;
    }>(`
      /* admin-finance:paying-users */
      ${commonCtes(5)}
      SELECT u.id AS user_id, u.email, u.display_name, u.status,
        cp.customer_type, cp.current_tier, cp.multiplier_bp, paid.funding_kind,
        paid.paid_nano::text, paid.payments_count::text, paid.last_paid_at,
        paid.manual_nano::text AS manual_paid_nano, paid.manual_topups_count::text,
        COALESCE(usage.spent_nano, 0)::text AS spent_nano,
        COALESCE(usage.paid_funded_nano, 0)::text AS paid_funded_nano,
        COALESCE(usage.bonus_funded_nano, 0)::text AS bonus_funded_nano,
        COALESCE(usage.other_funded_nano, 0)::text AS other_funded_nano,
        COALESCE(usage.unattributed_nano, 0)::text AS unattributed_nano,
        COALESCE(usage.anthropic_nano, 0)::text AS anthropic_nano,
        COALESCE(usage.openai_nano, 0)::text AS openai_nano,
        COALESCE(usage.google_nano, 0)::text AS google_nano,
        COALESCE(usage.kimi_nano, 0)::text AS kimi_nano,
        COALESCE(usage.other_nano, 0)::text AS other_nano,
        ea.engine_account_id,
        CASE
          WHEN usage.event_account_ids IS NOT NULL THEN usage.event_account_ids
          WHEN ea.engine_account_id IS NOT NULL THEN ARRAY[ea.engine_account_id]
          ELSE ARRAY[]::text[]
        END AS usage_account_ids,
        COALESCE(api_keys.active_count, 0)::text AS active_api_keys,
        sessions.last_seen_at, u.created_at
      FROM users u
      JOIN paid ON paid.user_id = u.id
      LEFT JOIN customer_profiles cp ON cp.user_id = u.id
      LEFT JOIN engine_accounts ea ON ea.user_id = u.id
      LEFT JOIN usage ON usage.user_id = u.id
      LEFT JOIN sessions ON sessions.user_id = u.id
      LEFT JOIN api_keys ON api_keys.user_id = u.id
      WHERE ${filters}
      ORDER BY ${sortExpr} ${dir === "asc" ? "ASC" : "DESC"} NULLS LAST, u.id ASC
      LIMIT $6 OFFSET $7
    `, [days, q, status, provider, funding, limit, offset]),
    await client.query<{ total: string }>(`
      /* admin-finance:paying-users-count */
      ${commonCtes(5)}
      SELECT count(*)::text AS total
      FROM users u
      JOIN paid ON paid.user_id = u.id
      LEFT JOIN usage ON usage.user_id = u.id
      WHERE ${filters}
    `, [days, q, status, provider, funding]),
    await client.query<{
      paying_users: string; cohort_users: string; bonus_only_users: string;
      active_spenders: string; paid_nano: string; manual_paid_nano: string;
      spent_nano: string; bonus_only_spent_nano: string;
      anthropic_nano: string; openai_nano: string; google_nano: string; kimi_nano: string;
      other_nano: string;
      anthropic_users: string; openai_users: string; google_users: string; kimi_users: string;
      other_users: string;
    }>(`
      /* admin-finance:paying-users-summary */
      ${commonCtes(2)}
      SELECT count(*) FILTER (
          WHERE paid.funding_kind IN ('payments', 'payments_and_manual', 'manual')
        )::text AS paying_users,
        count(*)::text AS cohort_users,
        count(*) FILTER (WHERE paid.funding_kind = 'bonus_only')::text AS bonus_only_users,
        count(*) FILTER (WHERE COALESCE(usage.spent_nano, 0) > 0)::text AS active_spenders,
        COALESCE(sum(paid.paid_nano), 0)::text AS paid_nano,
        COALESCE(sum(paid.manual_nano), 0)::text AS manual_paid_nano,
        COALESCE(sum(usage.spent_nano), 0)::text AS spent_nano,
        COALESCE(sum(usage.spent_nano) FILTER (WHERE paid.funding_kind = 'bonus_only'), 0)::text
          AS bonus_only_spent_nano,
        COALESCE(sum(usage.anthropic_nano), 0)::text AS anthropic_nano,
        COALESCE(sum(usage.openai_nano), 0)::text AS openai_nano,
        COALESCE(sum(usage.google_nano), 0)::text AS google_nano,
        COALESCE(sum(usage.kimi_nano), 0)::text AS kimi_nano,
        COALESCE(sum(usage.other_nano), 0)::text AS other_nano,
        count(*) FILTER (WHERE COALESCE(usage.anthropic_nano, 0) > 0)::text AS anthropic_users,
        count(*) FILTER (WHERE COALESCE(usage.openai_nano, 0) > 0)::text AS openai_users,
        count(*) FILTER (WHERE COALESCE(usage.google_nano, 0) > 0)::text AS google_users,
        count(*) FILTER (WHERE COALESCE(usage.kimi_nano, 0) > 0)::text AS kimi_users,
        count(*) FILTER (WHERE COALESCE(usage.other_nano, 0) > 0)::text AS other_users
      FROM paid
      LEFT JOIN usage ON usage.user_id = paid.user_id
    `, [days, funding]),
  ];
  await client.query("COMMIT");

  const summary = summaryResult.rows[0] ?? {
    paying_users: "0", cohort_users: "0", bonus_only_users: "0", active_spenders: "0",
    paid_nano: "0", manual_paid_nano: "0", spent_nano: "0", bonus_only_spent_nano: "0",
    anthropic_nano: "0", openai_nano: "0", google_nano: "0", kimi_nano: "0", other_nano: "0",
    anthropic_users: "0", openai_users: "0", google_users: "0", kimi_users: "0", other_users: "0",
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
      fundingKind: row.funding_kind,
      paidNano: row.paid_nano,
      paymentsCount: Number(row.payments_count),
      manualPaidNano: row.manual_paid_nano,
      manualTopupsCount: Number(row.manual_topups_count),
      lastPaidAt: row.last_paid_at,
      spentNano: row.spent_nano,
      paidFundedSpentNano: row.paid_funded_nano,
      bonusFundedSpentNano: row.bonus_funded_nano,
      otherFundedSpentNano: row.other_funded_nano,
      unattributedSpentNano: row.unattributed_nano,
      providerSpendNano: {
        anthropic: row.anthropic_nano,
        openai: row.openai_nano,
        google: row.google_nano,
        kimi: row.kimi_nano,
        other: row.other_nano,
      },
      engineAccountId: row.engine_account_id,
      usageAccountIds: row.usage_account_ids,
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
      cohortUsers: Number(summary.cohort_users),
      bonusOnlyUsers: Number(summary.bonus_only_users),
      activeSpenders: Number(summary.active_spenders),
      paidNano: summary.paid_nano,
      manualPaidNano: summary.manual_paid_nano,
      spentNano: summary.spent_nano,
      bonusOnlySpentNano: summary.bonus_only_spent_nano,
      providerSpendNano: {
        anthropic: summary.anthropic_nano,
        openai: summary.openai_nano,
        google: summary.google_nano,
        kimi: summary.kimi_nano,
        other: summary.other_nano,
      },
      providerUsers: {
        anthropic: Number(summary.anthropic_users),
        openai: Number(summary.openai_users),
        google: Number(summary.google_users),
        kimi: Number(summary.kimi_users),
        other: Number(summary.other_users),
      },
    },
  };
  } catch (error) {
    try {
      await client.query("ROLLBACK");
    } catch {
      // Preserve the query/commit failure: rollback is best-effort cleanup only.
    }
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Возвраты и диспуты: payments со статусом refunded/disputed. Авторитет возврата —
 * payments.status; состояние отдельной идемпотентной компенсации читается из последнего
 * engine_adjustments для платежа. Сортировка — по updated_at DESC (свежий акт возврата первым),
 * пагинация limit/offset + общее число и сумма всех возвратов.
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
      adjustment_status: string | null; adjustment_confirmed_at: Date | null;
      adjustment_last_error: string | null;
    }>(`
      /* admin-finance:refunds */
      SELECT p.id, p.user_id, u.email, p.provider, p.provider_payment_id,
             p.amount_nano::text AS amount_nano, p.currency, p.status::text AS status,
             p.paid_at, p.updated_at, adjustment.status AS adjustment_status,
             adjustment.confirmed_at AS adjustment_confirmed_at,
             adjustment.last_error AS adjustment_last_error
      FROM payments p
      JOIN users u ON u.id = p.user_id
      LEFT JOIN LATERAL (
        SELECT status::text AS status, confirmed_at, last_error
        FROM engine_adjustments
        WHERE payment_id = p.id
        ORDER BY created_at DESC, id DESC
        LIMIT 1
      ) adjustment ON true
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
      adjustmentStatus: row.adjustment_status,
      adjustmentConfirmedAt: row.adjustment_confirmed_at,
      adjustmentLastError: row.adjustment_last_error,
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
