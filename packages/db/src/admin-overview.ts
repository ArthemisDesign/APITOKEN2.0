import type { Database } from "./client.js";

export interface AdminDashboard {
  generatedAt: Date;
  users: {
    total: number;
    active: number;
    disabled: number;
    registered24h: number;
    registered30d: number;
    active7d: number;
    registeredOauth: number;
    registeredPassword: number;
    passwordOnly: number;
    oauthOnly: number;
    hybrid: number;
    google: number;
    github: number;
    verified: number;
    totp: number;
  };
  topups: {
    paidCount: number;
    paidUsers: number;
    paidNano: string;
    paid30dCount: number;
    paid30dNano: string;
    pendingCheckouts: number;
    failed30d: number;
    refundedCount: number;
    refundedNano: string;
    manualCount: number;
    manualNano: string;
    manual30dCount: number;
    manual30dNano: string;
  };
  platform: {
    b2cUsers: number;
    b2bUsers: number;
    activeApiKeys: number;
    totalApiKeys: number;
    activeSessions: number;
    engineActive: number;
    enginePending: number;
    engineError: number;
    engineDisabled: number;
  };
}

export interface AdminTopupRow {
  id: string;
  userId: string;
  email: string;
  provider: string;
  providerPaymentId: string;
  amountNano: string;
  currency: string;
  status: string;
  // NULL возможен только со status-фильтром (например failed-платежи без оплаты); в окне
  // по умолчанию (без фильтров) список ограничен paid_at IS NOT NULL, как и раньше.
  paidAt: Date | null;
  createdAt: Date;
  creditStatus: string | null;
}

export interface AdminCheckoutRow {
  id: string;
  userId: string;
  email: string;
  provider: string;
  providerPaymentId: string | null;
  amountUsd: string;
  status: string;
  createdAt: Date;
  completedAt: Date | null;
  expiresAt: Date | null;
}

export interface AdminAuditRow {
  id: string;
  actorType: string;
  actorId: string | null;
  action: string;
  targetType: string;
  targetId: string;
  metadata: unknown;
  createdAt: Date;
}

export interface AdminBusinessInviteRow {
  id: string;
  email: string | null;
  multiplierBp: number;
  expiresAt: Date;
  consumedAt: Date | null;
  consumedByUserId: string | null;
  revokedAt: Date | null;
  supersededByInviteId: string | null;
  createdByActor: string | null;
  deliveryStatus: string;
  deliveryAttempts: number | null;
  deliveryError: string | null;
  deliverySentAt: Date | null;
  createdAt: Date;
}

export interface AdminUserControlTarget {
  id: string;
  status: "active" | "disabled";
  engineAccountId: string | null;
  engineAccountStatus: "pending" | "active" | "error" | "disabled" | null;
}

// Админ-обзор пользователей для панели (admin.apitoken.sale → GET /admin/users).
// Read-only агрегат по commerce PostgreSQL; live-баланс/расход движка доклеивает
// apps/api поверх (Control API). Все денежные суммы — строки nano-USD (без JS number).
export interface AdminUserOverviewRow {
  id: string;
  email: string;
  displayName: string;
  emailVerified: boolean;
  status: "active" | "disabled";
  createdAt: Date;
  hasPassword: boolean;
  totpEnabled: boolean;
  providers: string[];
  customerType: "b2c" | "b2b" | null;
  currentTier: number | null;
  multiplierBp: number | null;
  cumulativeTopupNano: string;
  tierWindowSpentNano: string;
  engineAccountId: string | null;
  engineAccountStatus: string | null;
  pricingSyncStatus: string | null;
  pricingSyncAttempts: number | null;
  pricingSyncError: string | null;
  pricingSyncConfirmedAt: Date | null;
  paidPaymentsCount: number;
  paidTotalNano: string;
  lastPaidAt: Date | null;
  pendingCheckoutsCount: number;
  apiKeysActive: number;
  apiKeysTotal: number;
  lastSeenAt: Date | null;
  spent30dNano: string;
  providerSpend30dNano: {
    anthropic: string;
    openai: string;
    google: string;
    kimi: string;
    other: string;
  };
}

/** Допустимые поля сортировки админ-списка пользователей (GET /admin/users?sort=). */
export type AdminUserSort = "created_at" | "last_seen_at" | "paid_total" | "topup_total" | "spent_30d";

export type AdminSortDir = "asc" | "desc";

export interface AdminUserOverviewQuery {
  limit?: number;
  offset?: number;
  search?: string;
  status?: "active" | "disabled";
  auth?: "password" | "google" | "github";
  customerType?: "b2c" | "b2b";
  sort?: AdminUserSort;
  dir?: AdminSortDir;
}

export interface AdminUserOverviewPage {
  rows: AdminUserOverviewRow[];
  total: number;
  limit: number;
  offset: number;
}

interface RawRow {
  id: string;
  email: string;
  display_name: string;
  email_verified: boolean;
  status: "active" | "disabled";
  created_at: Date;
  has_password: boolean;
  totp_enabled: boolean;
  providers: string[] | null;
  customer_type: "b2c" | "b2b" | null;
  current_tier: number | null;
  multiplier_bp: number | null;
  cumulative_topup_nano: string | null;
  tier_window_spent_nano: string | null;
  engine_account_id: string | null;
  engine_account_status: string | null;
  pricing_sync_status: string | null;
  pricing_sync_attempts: number | null;
  pricing_sync_error: string | null;
  pricing_sync_confirmed_at: Date | null;
  paid_payments_count: string;
  paid_total_nano: string;
  last_paid_at: Date | null;
  pending_checkouts_count: string;
  api_keys_active: string;
  api_keys_total: string;
  last_seen_at: Date | null;
  spent_30d_nano: string;
  spent_30d_anthropic_nano: string;
  spent_30d_openai_nano: string;
  spent_30d_google_nano: string;
  spent_30d_kimi_nano: string;
  spent_30d_other_nano: string;
}

// Белый список сортировок admin-списка пользователей. В SQL интерполируется ТОЛЬКО значение
// из этой закрытой таблицы, никогда — сырое значение из query-параметра: это защита от
// SQL-инъекции через sort/dir (HTTP-слой дополнительно валидирует zod-enum'ом).
// Намеренно НЕТ balance_usd/spent_usd: это live-поля движка, которых нет в commerce БД —
// apps/api доклеивает их через Control API уже после пагинации страницы. Сортировать на
// стороне БД по ним невозможно, а сортировка одной страницы врала бы о глобальном порядке.
const ADMIN_USER_SORT_SQL: Record<AdminUserSort, string> = {
  created_at: "u.created_at",
  last_seen_at: "s.last_seen_at",
  paid_total: "COALESCE(p.paid_total, 0)",
  topup_total: "COALESCE(cp.cumulative_topup_nano, 0)",
  spent_30d: "COALESCE(ue.spent_30d, 0)",
};

/** След админского начисления в audit_log (движок уже кредитован идемпотентно по ref). */
export async function recordAdminCredit(database: Database, input: {
  userId: string;
  engineAccountId: string;
  amountNano: bigint;
  ref: string;
  balanceAfterNano: string;
  reason?: string;
  actorId?: string;
}): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // Concurrent retries may both reach the idempotent engine endpoint. Serialize only the audit
    // write so one operator action has one durable commerce record without needing a new table.
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [input.ref]);
    await client.query(
      `INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
       SELECT 'commercial-admin', $2, 'admin.credit', 'user', $1, $3::jsonb
       WHERE NOT EXISTS (
         SELECT 1 FROM audit_log
         WHERE action = 'admin.credit' AND metadata->>'ref' = $4
       )`,
      [input.userId, input.actorId ?? null, JSON.stringify({
        engine_account_id: input.engineAccountId,
        amount_nano: input.amountNano.toString(),
        ref: input.ref,
        balance_after_nano: input.balanceAfterNano,
        reason: input.reason ?? null,
      }), input.ref],
    );
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function findAdminCreditByRef(database: Database, ref: string): Promise<{
  userId: string;
  amountNano: string;
  balanceAfterNano: string;
} | null> {
  const result = await database.pool.query<{
    target_id: string;
    amount_nano: string;
    balance_after_nano: string;
  }>(`
    SELECT target_id, metadata->>'amount_nano' AS amount_nano,
           metadata->>'balance_after_nano' AS balance_after_nano
    FROM audit_log
    WHERE action = 'admin.credit' AND metadata->>'ref' = $1
    ORDER BY created_at ASC
    LIMIT 1
  `, [ref]);
  const row = result.rows[0];
  return row ? {
    userId: row.target_id,
    amountNano: row.amount_nano,
    balanceAfterNano: row.balance_after_nano,
  } : null;
}

export async function listAdminUserOverview(
  database: Database,
  query: AdminUserOverviewQuery = {},
): Promise<AdminUserOverviewPage> {
  const limit = Math.max(1, Math.min(100, query.limit ?? 50));
  const offset = Math.max(0, query.offset ?? 0);
  const search = query.search?.trim() ?? "";
  const status = query.status ?? "";
  const auth = query.auth ?? "";
  const customerType = query.customerType ?? "";
  const sort = query.sort ?? "created_at";
  const dir = query.dir ?? "desc";
  const sortExpr: string | undefined = ADMIN_USER_SORT_SQL[sort];
  if (sortExpr === undefined) throw new Error(`unsupported admin user sort: ${String(query.sort)}`);
  if (dir !== "asc" && dir !== "desc") throw new Error(`unsupported admin user sort dir: ${String(query.dir)}`);
  // Дефолт (created_at DESC) повторяет исторический ORDER BY байт-в-байт — обратная
  // совместимость ответа. Остальные сортировки идут с NULLS LAST и стабильным tiebreaker,
  // чтобы offset-пагинация не перескакивала через строки.
  const orderBy = sort === "created_at" && dir === "desc"
    ? "u.created_at DESC"
    : `${sortExpr} ${dir === "asc" ? "ASC" : "DESC"} NULLS LAST, u.id ASC`;
  const filters = `
    ($1::text = '' OR u.email ILIKE '%' || $1 || '%'
      OR u.display_name ILIKE '%' || $1 || '%' OR u.id::text ILIKE '%' || $1 || '%')
    AND ($2::text = '' OR u.status::text = $2)
    AND ($3::text = '' OR ($3 = 'password' AND u.password_hash IS NOT NULL)
      OR ($3 <> 'password' AND EXISTS (
        SELECT 1 FROM auth_identities auth_filter
        WHERE auth_filter.user_id = u.id AND auth_filter.provider::text = $3
      )))
    AND ($4::text = '' OR cp.customer_type::text = $4)
  `;
  const params = [search, status, auth, customerType, limit, offset];
  const [result, countResult] = await Promise.all([
    database.pool.query<RawRow>(`
    SELECT
      u.id, u.email, u.display_name, u.email_verified, u.status, u.created_at,
      (u.password_hash IS NOT NULL) AS has_password,
      u.totp_enabled,
      ai.providers,
      cp.customer_type, cp.current_tier, cp.multiplier_bp,
      cp.cumulative_topup_nano::text AS cumulative_topup_nano,
      cp.tier_window_spent_nano::text AS tier_window_spent_nano,
      ea.engine_account_id, ea.status AS engine_account_status,
      pj.status::text AS pricing_sync_status, pj.attempts AS pricing_sync_attempts,
      pj.last_error AS pricing_sync_error, pj.confirmed_at AS pricing_sync_confirmed_at,
      COALESCE(p.paid_count, 0)::text AS paid_payments_count,
      COALESCE(p.paid_total, 0)::text AS paid_total_nano,
      p.last_paid_at,
      COALESCE(cs.pending_count, 0)::text AS pending_checkouts_count,
      COALESCE(k.active_count, 0)::text AS api_keys_active,
      COALESCE(k.total_count, 0)::text AS api_keys_total,
      s.last_seen_at,
      COALESCE(ue.spent_30d, 0)::text AS spent_30d_nano,
      COALESCE(ue.anthropic_nano, 0)::text AS spent_30d_anthropic_nano,
      COALESCE(ue.openai_nano, 0)::text AS spent_30d_openai_nano,
      COALESCE(ue.google_nano, 0)::text AS spent_30d_google_nano,
      COALESCE(ue.kimi_nano, 0)::text AS spent_30d_kimi_nano,
      COALESCE(ue.other_nano, 0)::text AS spent_30d_other_nano
    FROM users u
    LEFT JOIN customer_profiles cp ON cp.user_id = u.id
    LEFT JOIN engine_accounts ea ON ea.user_id = u.id
    LEFT JOIN LATERAL (
      -- A B2B price is delivered as one default job plus zero or more provider jobs. Pick one
      -- deterministic bundle status instead of joining every target (which duplicated users in
      -- the admin table). Any unfinished target keeps the bundle unfinished; retry is surfaced
      -- before processing/pending, and a confirmed bundle is dated by its last confirmation.
      SELECT ranked.status, ranked.attempts, ranked.last_error,
             CASE WHEN ranked.unfinished_count = 0 THEN ranked.bundle_confirmed_at END AS confirmed_at
      FROM (
        SELECT job.status::text AS status, job.attempts, job.last_error,
               count(*) FILTER (WHERE job.status <> 'confirmed') OVER () AS unfinished_count,
               max(job.confirmed_at) OVER () AS bundle_confirmed_at
        FROM engine_pricing_jobs job
        WHERE job.user_id = u.id
        ORDER BY CASE job.status
          WHEN 'retry' THEN 1
          WHEN 'processing' THEN 2
          WHEN 'pending' THEN 3
          WHEN 'confirmed' THEN 4
        END, job.last_error IS NULL, job.provider_id NULLS FIRST, job.id
      ) ranked
      LIMIT 1
    ) pj ON TRUE
    LEFT JOIN LATERAL (
      SELECT array_agg(provider ORDER BY provider) AS providers
      FROM auth_identities WHERE user_id = u.id
    ) ai ON TRUE
    LEFT JOIN LATERAL (
      SELECT count(*) AS paid_count, sum(amount_nano) AS paid_total, max(paid_at) AS last_paid_at
      FROM payments WHERE user_id = u.id AND status = 'paid'
    ) p ON TRUE
    LEFT JOIN LATERAL (
      SELECT count(*) AS pending_count
      FROM checkout_sessions WHERE user_id = u.id AND status = 'pending'
    ) cs ON TRUE
    LEFT JOIN LATERAL (
      SELECT count(*) FILTER (WHERE status = 'active') AS active_count, count(*) AS total_count
      FROM api_keys WHERE user_id = u.id
    ) k ON TRUE
    LEFT JOIN LATERAL (
      SELECT max(last_seen_at) AS last_seen_at
      FROM auth_sessions WHERE user_id = u.id AND revoked_at IS NULL
    ) s ON TRUE
    LEFT JOIN LATERAL (
      SELECT
        sum(amount_nano) AS spent_30d,
        sum(amount_nano) FILTER (WHERE provider_id = 'anthropic') AS anthropic_nano,
        sum(amount_nano) FILTER (WHERE provider_id = 'openai') AS openai_nano,
        sum(amount_nano) FILTER (WHERE provider_id = 'google') AS google_nano,
        sum(amount_nano) FILTER (WHERE provider_id = 'kimi') AS kimi_nano,
        sum(amount_nano) FILTER (
          WHERE provider_id IS NULL OR provider_id NOT IN ('anthropic', 'openai', 'google', 'kimi')
        ) AS other_nano
      FROM pricing_usage_events
      WHERE user_id = u.id
        AND occurred_at >= now() - interval '30 days'
        AND occurred_at < now()
    ) ue ON TRUE
    WHERE ${filters}
    ORDER BY ${orderBy}
    LIMIT $5 OFFSET $6
  `, params),
    database.pool.query<{ total: string }>(`
      SELECT count(*)::text AS total
      FROM users u
      LEFT JOIN customer_profiles cp ON cp.user_id = u.id
      WHERE ${filters}
    `, params.slice(0, 4)),
  ]);
  return {
    rows: result.rows.map((row) => ({
    id: row.id,
    email: row.email,
    displayName: row.display_name,
    emailVerified: row.email_verified,
    status: row.status,
    createdAt: row.created_at,
    hasPassword: row.has_password,
    totpEnabled: row.totp_enabled,
    providers: row.providers ?? [],
    customerType: row.customer_type,
    currentTier: row.current_tier,
    multiplierBp: row.multiplier_bp,
    cumulativeTopupNano: row.cumulative_topup_nano ?? "0",
    tierWindowSpentNano: row.tier_window_spent_nano ?? "0",
    engineAccountId: row.engine_account_id,
    engineAccountStatus: row.engine_account_status,
    pricingSyncStatus: row.pricing_sync_status,
    pricingSyncAttempts: row.pricing_sync_attempts,
    pricingSyncError: row.pricing_sync_error,
    pricingSyncConfirmedAt: row.pricing_sync_confirmed_at,
    paidPaymentsCount: Number(row.paid_payments_count),
    paidTotalNano: row.paid_total_nano,
    lastPaidAt: row.last_paid_at,
    pendingCheckoutsCount: Number(row.pending_checkouts_count),
    apiKeysActive: Number(row.api_keys_active),
    apiKeysTotal: Number(row.api_keys_total),
    lastSeenAt: row.last_seen_at,
    spent30dNano: row.spent_30d_nano,
    providerSpend30dNano: {
      anthropic: row.spent_30d_anthropic_nano,
      openai: row.spent_30d_openai_nano,
      google: row.spent_30d_google_nano,
      kimi: row.spent_30d_kimi_nano,
      other: row.spent_30d_other_nano,
    },
    })),
    total: Number(countResult.rows[0]?.total ?? 0),
    limit,
    offset,
  };
}

export async function getAdminDashboard(database: Database): Promise<AdminDashboard> {
  const result = await database.pool.query<Record<string, string | Date>>(`
    WITH oauth_ident AS (
      SELECT user_id,
             bool_or(true) AS has_oauth,
             bool_or(provider = 'google') AS has_google,
             bool_or(provider = 'github') AS has_github
      FROM auth_identities
      GROUP BY user_id
    ),
    oauth_reg AS (
      SELECT DISTINCT target_id AS user_id
      FROM audit_log
      WHERE target_type = 'user' AND action = 'auth.oauth_registered'
    ),
    sess_7d AS (
      SELECT DISTINCT user_id
      FROM auth_sessions
      WHERE last_seen_at >= now() - interval '7 days'
    ),
    user_agg AS (
      SELECT
        count(*) AS users_total,
        count(*) FILTER (WHERE u.status = 'active') AS users_active,
        count(*) FILTER (WHERE u.status = 'disabled') AS users_disabled,
        count(*) FILTER (WHERE u.created_at >= now() - interval '24 hours') AS users_registered_24h,
        count(*) FILTER (WHERE u.created_at >= now() - interval '30 days') AS users_registered_30d,
        count(*) FILTER (WHERE s.user_id IS NOT NULL) AS users_active_7d,
        count(*) FILTER (WHERE oreg.user_id IS NOT NULL) AS users_registered_oauth,
        count(*) FILTER (WHERE oreg.user_id IS NULL) AS users_registered_password,
        count(*) FILTER (WHERE u.password_hash IS NOT NULL AND oi.user_id IS NULL) AS users_password_only,
        count(*) FILTER (WHERE oi.user_id IS NOT NULL AND u.password_hash IS NULL) AS users_oauth_only,
        count(*) FILTER (WHERE oi.user_id IS NOT NULL AND u.password_hash IS NOT NULL) AS users_hybrid,
        count(*) FILTER (WHERE oi.has_google) AS users_google,
        count(*) FILTER (WHERE oi.has_github) AS users_github,
        count(*) FILTER (WHERE u.email_verified) AS users_verified,
        count(*) FILTER (WHERE u.totp_enabled) AS users_totp
      FROM users u
      LEFT JOIN oauth_ident oi ON oi.user_id = u.id
      LEFT JOIN oauth_reg oreg ON oreg.user_id = u.id::text
      LEFT JOIN sess_7d s ON s.user_id = u.id
    ), paid AS (
      SELECT count(*) FILTER (WHERE status = 'paid') AS paid_count,
             count(DISTINCT user_id) FILTER (WHERE status = 'paid') AS paid_users,
             COALESCE(sum(amount_nano) FILTER (WHERE status = 'paid'), 0) AS paid_nano,
             count(*) FILTER (WHERE status = 'paid' AND paid_at >= now() - interval '30 days') AS paid_30d_count,
             COALESCE(sum(amount_nano) FILTER (
               WHERE status = 'paid' AND paid_at >= now() - interval '30 days'
             ), 0) AS paid_30d_nano,
             count(*) FILTER (WHERE status = 'refunded') AS refunded_count,
             COALESCE(sum(amount_nano) FILTER (WHERE status = 'refunded'), 0) AS refunded_nano
      FROM payments
    ), manual AS (
      SELECT count(*) AS manual_count,
             COALESCE(sum((metadata->>'amount_nano')::numeric), 0) AS manual_nano,
             count(*) FILTER (WHERE created_at >= now() - interval '30 days') AS manual_30d_count,
             COALESCE(sum((metadata->>'amount_nano')::numeric)
               FILTER (WHERE created_at >= now() - interval '30 days'), 0) AS manual_30d_nano
      FROM audit_log
      WHERE action = 'admin.credit' AND metadata->>'amount_nano' ~ '^[0-9]+$'
    )
    SELECT now() AS generated_at,
      user_agg.users_total, user_agg.users_active, user_agg.users_disabled,
      user_agg.users_registered_24h, user_agg.users_registered_30d, user_agg.users_active_7d,
      user_agg.users_registered_oauth, user_agg.users_registered_password,
      user_agg.users_password_only, user_agg.users_oauth_only, user_agg.users_hybrid,
      user_agg.users_google, user_agg.users_github, user_agg.users_verified, user_agg.users_totp,
      paid.paid_count, paid.paid_users, paid.paid_nano, paid.paid_30d_count, paid.paid_30d_nano,
      paid.refunded_count, paid.refunded_nano,
      (SELECT count(*) FROM checkout_sessions WHERE status IN ('creating', 'pending')) AS pending_checkouts,
      (SELECT count(*) FROM checkout_sessions WHERE status = 'failed' AND created_at >= now() - interval '30 days') AS failed_30d,
      manual.manual_count, manual.manual_nano, manual.manual_30d_count, manual.manual_30d_nano,
      (SELECT count(*) FROM customer_profiles WHERE customer_type = 'b2c') AS b2c_users,
      (SELECT count(*) FROM customer_profiles WHERE customer_type = 'b2b') AS b2b_users,
      (SELECT count(*) FROM api_keys WHERE status = 'active') AS active_api_keys,
      (SELECT count(*) FROM api_keys) AS total_api_keys,
      (SELECT count(*) FROM auth_sessions WHERE revoked_at IS NULL AND expires_at > now()) AS active_sessions,
      (SELECT count(*) FROM engine_accounts WHERE status = 'active') AS engine_active,
      (SELECT count(*) FROM engine_accounts WHERE status = 'pending') AS engine_pending,
      (SELECT count(*) FROM engine_accounts WHERE status = 'error') AS engine_error,
      (SELECT count(*) FROM engine_accounts WHERE status = 'disabled') AS engine_disabled
    FROM user_agg CROSS JOIN paid CROSS JOIN manual
  `);
  const row = result.rows[0]!;
  const count = (key: string): number => Number(row[key] ?? 0);
  const money = (key: string): string => String(row[key] ?? "0");
  return {
    generatedAt: row.generated_at instanceof Date ? row.generated_at : new Date(String(row.generated_at)),
    users: {
      total: count("users_total"), active: count("users_active"), disabled: count("users_disabled"),
      registered24h: count("users_registered_24h"), registered30d: count("users_registered_30d"),
      active7d: count("users_active_7d"), passwordOnly: count("users_password_only"),
      registeredOauth: count("users_registered_oauth"), registeredPassword: count("users_registered_password"),
      oauthOnly: count("users_oauth_only"), hybrid: count("users_hybrid"),
      google: count("users_google"), github: count("users_github"),
      verified: count("users_verified"), totp: count("users_totp"),
    },
    topups: {
      paidCount: count("paid_count"), paidUsers: count("paid_users"), paidNano: money("paid_nano"),
      paid30dCount: count("paid_30d_count"), paid30dNano: money("paid_30d_nano"),
      pendingCheckouts: count("pending_checkouts"), failed30d: count("failed_30d"),
      refundedCount: count("refunded_count"), refundedNano: money("refunded_nano"),
      manualCount: count("manual_count"), manualNano: money("manual_nano"),
      manual30dCount: count("manual_30d_count"), manual30dNano: money("manual_30d_nano"),
    },
    platform: {
      b2cUsers: count("b2c_users"), b2bUsers: count("b2b_users"),
      activeApiKeys: count("active_api_keys"), totalApiKeys: count("total_api_keys"),
      activeSessions: count("active_sessions"), engineActive: count("engine_active"),
      enginePending: count("engine_pending"), engineError: count("engine_error"),
      engineDisabled: count("engine_disabled"),
    },
  };
}

/**
 * Фильтры GET /admin/topups. `status` — объединение статусов платежей и чекаутов:
 * один фильтр применяется к ОБОИМ спискам (к платежам по payments.status, к чекаутам по
 * checkout_sessions.status, exact match). Без `status` списки сохраняют исторические окна:
 * payments — только строки с paid_at (реально оплаченные/возвращённые/диспуты), checkouts —
 * только неоплаченные (status <> 'paid'). С заданным `status` эти окна снимаются: так
 * status=failed показывает и неудачные платежи (у них paid_at NULL), и failed-чекауты,
 * а status=paid — оплаченные чекауты рядом с оплаченными платежами.
 */
export type AdminTopupStatus =
  | "paid" | "refunded" | "disputed" | "failed" | "pending" | "canceled" | "creating";

export interface AdminTopupsQuery {
  limit: number;
  offset?: number;
  q?: string;
  provider?: string;
  status?: AdminTopupStatus;
}

export interface AdminTopupsPage {
  payments: AdminTopupRow[];
  checkouts: AdminCheckoutRow[];
  paymentsTotal: number;
  checkoutsTotal: number;
}

export async function listAdminTopups(
  database: Database,
  query: AdminTopupsQuery,
): Promise<AdminTopupsPage> {
  const limit = Math.max(1, Math.min(500, query.limit));
  const offset = Math.max(0, query.offset ?? 0);
  const search = query.q?.trim() ?? "";
  const provider = query.provider?.trim() ?? "";
  const status = query.status ?? "";
  // Общие фильтры обоих списков: $1 — подстрока email (case-insensitive), $2 — точный
  // provider, $3 — status (семантика окна по умолчанию описана у AdminTopupStatus).
  const paymentFilters = `
    ($1::text = '' OR u.email ILIKE '%' || $1 || '%')
    AND ($2::text = '' OR p.provider = $2)
    AND (($3::text = '' AND p.paid_at IS NOT NULL) OR ($3::text <> '' AND p.status::text = $3))
  `;
  const checkoutFilters = `
    ($1::text = '' OR u.email ILIKE '%' || $1 || '%')
    AND ($2::text = '' OR cs.provider = $2)
    AND (($3::text = '' AND cs.status <> 'paid') OR ($3::text <> '' AND cs.status::text = $3))
  `;
  const filterParams = [search, provider, status];
  const pageParams = [...filterParams, limit, offset];
  const [payments, paymentsCount, checkouts, checkoutsCount] = await Promise.all([
    database.pool.query<{
      id: string; user_id: string; email: string; provider: string; provider_payment_id: string;
      amount_nano: string; currency: string; status: string; paid_at: Date | null; created_at: Date;
      credit_status: string | null;
    }>(`
      SELECT p.id, p.user_id, u.email, p.provider, p.provider_payment_id,
             p.amount_nano::text AS amount_nano, p.currency, p.status::text AS status, p.paid_at, p.created_at,
             ec.status AS credit_status
      FROM payments p
      JOIN users u ON u.id = p.user_id
      LEFT JOIN engine_credits ec ON ec.payment_id = p.id
      WHERE ${paymentFilters}
      ORDER BY COALESCE(p.paid_at, p.created_at) DESC, p.id
      LIMIT $4 OFFSET $5
    `, pageParams),
    database.pool.query<{ total: string }>(`
      SELECT count(*)::text AS total
      FROM payments p
      JOIN users u ON u.id = p.user_id
      WHERE ${paymentFilters}
    `, filterParams),
    database.pool.query<{
      id: string; user_id: string; email: string; provider: string; provider_payment_id: string | null;
      amount_usd: string; status: string; created_at: Date; completed_at: Date | null; expires_at: Date | null;
    }>(`
      SELECT cs.id, cs.user_id, u.email, cs.provider, cs.provider_payment_id,
             cs.amount_usd::text AS amount_usd, cs.status::text AS status, cs.created_at, cs.completed_at, cs.expires_at
      FROM checkout_sessions cs
      JOIN users u ON u.id = cs.user_id
      WHERE ${checkoutFilters}
      ORDER BY cs.created_at DESC, cs.id
      LIMIT $4 OFFSET $5
    `, pageParams),
    database.pool.query<{ total: string }>(`
      SELECT count(*)::text AS total
      FROM checkout_sessions cs
      JOIN users u ON u.id = cs.user_id
      WHERE ${checkoutFilters}
    `, filterParams),
  ]);
  return {
    payments: payments.rows.map((row) => ({
      id: row.id, userId: row.user_id, email: row.email, provider: row.provider,
      providerPaymentId: row.provider_payment_id, amountNano: row.amount_nano,
      currency: row.currency, status: row.status, paidAt: row.paid_at,
      createdAt: row.created_at, creditStatus: row.credit_status,
    })),
    checkouts: checkouts.rows.map((row) => ({
      id: row.id, userId: row.user_id, email: row.email, provider: row.provider,
      providerPaymentId: row.provider_payment_id, amountUsd: row.amount_usd,
      status: row.status, createdAt: row.created_at, completedAt: row.completed_at, expiresAt: row.expires_at,
    })),
    paymentsTotal: Number(paymentsCount.rows[0]?.total ?? 0),
    checkoutsTotal: Number(checkoutsCount.rows[0]?.total ?? 0),
  };
}

/**
 * Фильтры GET /admin/audit. `q` — case-insensitive подстрока по target_id И по сериализованному
 * JSON metadata (metadata::text): так находятся и события по id цели, и по значениям внутри
 * payload (ref, reason, engine_account_id). `%`/`_` во входе работают как ILIKE-шаблон —
 * то же поведение, что у поиска в GET /admin/users. `from`/`to` — границы created_at
 * (включительно); HTTP-слой принимает их ISO-строками.
 */
export interface AdminAuditQuery {
  limit: number;
  offset?: number;
  action?: string;
  actorType?: string;
  q?: string;
  from?: Date;
  to?: Date;
}

export async function listAdminAudit(
  database: Database,
  query: AdminAuditQuery,
): Promise<{ rows: AdminAuditRow[]; total: number }> {
  const limit = Math.max(1, Math.min(500, query.limit));
  const offset = Math.max(0, query.offset ?? 0);
  const action = query.action?.trim() ?? "";
  const actorType = query.actorType?.trim() ?? "";
  const search = query.q?.trim() ?? "";
  const from = query.from ?? null;
  const to = query.to ?? null;
  const filters = `
    ($1::text = '' OR action = $1)
    AND ($2::text = '' OR actor_type = $2)
    AND ($3::text = '' OR target_id ILIKE '%' || $3 || '%' OR metadata::text ILIKE '%' || $3 || '%')
    AND ($4::timestamptz IS NULL OR created_at >= $4)
    AND ($5::timestamptz IS NULL OR created_at <= $5)
  `;
  const filterParams = [action, actorType, search, from, to];
  const [result, countResult] = await Promise.all([
    database.pool.query<{
      id: string; actor_type: string; actor_id: string | null; action: string; target_type: string;
      target_id: string; metadata: unknown; created_at: Date;
    }>(`
      SELECT id::text, actor_type, actor_id, action, target_type, target_id, metadata, created_at
      FROM audit_log
      WHERE ${filters}
      ORDER BY created_at DESC, id DESC
      LIMIT $6 OFFSET $7
    `, [...filterParams, limit, offset]),
    database.pool.query<{ total: string }>(`
      SELECT count(*)::text AS total
      FROM audit_log
      WHERE ${filters}
    `, filterParams),
  ]);
  return {
    rows: result.rows.map((row) => ({
      id: row.id, actorType: row.actor_type, actorId: row.actor_id, action: row.action,
      targetType: row.target_type, targetId: row.target_id, metadata: row.metadata, createdAt: row.created_at,
    })),
    total: Number(countResult.rows[0]?.total ?? 0),
  };
}

/** Distinct-список action в audit_log — для выпадающего фильтра панели (GET /admin/audit/actions). */
export async function listAdminAuditActions(database: Database): Promise<string[]> {
  const result = await database.pool.query<{ action: string }>(`
    SELECT DISTINCT action FROM audit_log ORDER BY action
  `);
  return result.rows.map((row) => row.action);
}

export async function listAdminBusinessInvites(database: Database, limit: number): Promise<AdminBusinessInviteRow[]> {
  const result = await database.pool.query<{
    id: string; email: string | null; multiplier_bp: number; expires_at: Date; consumed_at: Date | null;
    consumed_by_user_id: string | null; revoked_at: Date | null; superseded_by_invite_id: string | null;
    created_by_actor: string | null; delivery_status: string | null; delivery_attempts: number | null;
    delivery_error: string | null; delivery_sent_at: Date | null; created_at: Date;
  }>(`
    SELECT bi.id, bi.email, bi.multiplier_bp, bi.expires_at, bi.consumed_at,
           bi.consumed_by_user_id, bi.revoked_at, bi.superseded_by_invite_id,
           bi.created_by_actor, eo.status::text AS delivery_status,
           eo.attempts AS delivery_attempts, eo.last_error AS delivery_error,
           eo.sent_at AS delivery_sent_at, bi.created_at
    FROM business_invites bi
    LEFT JOIN LATERAL (
      SELECT status, attempts, last_error, sent_at
      FROM email_outbox
      WHERE business_invite_id = bi.id
      ORDER BY created_at DESC LIMIT 1
    ) eo ON TRUE
    ORDER BY bi.created_at DESC
    LIMIT $1
  `, [limit]);
  return result.rows.map((row) => ({
    id: row.id, email: row.email, multiplierBp: row.multiplier_bp, expiresAt: row.expires_at,
    consumedAt: row.consumed_at, consumedByUserId: row.consumed_by_user_id,
    revokedAt: row.revoked_at, supersededByInviteId: row.superseded_by_invite_id,
    createdByActor: row.created_by_actor, deliveryStatus: row.delivery_status ?? "copy_only",
    deliveryAttempts: row.delivery_attempts, deliveryError: row.delivery_error,
    deliverySentAt: row.delivery_sent_at,
    createdAt: row.created_at,
  }));
}

export async function getAdminUserControlTarget(
  database: Database,
  userId: string,
): Promise<AdminUserControlTarget | null> {
  const result = await database.pool.query<{
    id: string; status: "active" | "disabled"; engine_account_id: string | null;
    engine_account_status: "pending" | "active" | "error" | "disabled" | null;
  }>(`
    SELECT u.id, u.status, ea.engine_account_id, ea.status AS engine_account_status
    FROM users u
    LEFT JOIN engine_accounts ea ON ea.user_id = u.id
    WHERE u.id = $1
  `, [userId]);
  const row = result.rows[0];
  return row ? {
    id: row.id, status: row.status, engineAccountId: row.engine_account_id,
    engineAccountStatus: row.engine_account_status,
  } : null;
}

export async function setAdminUserStatus(
  database: Database,
  input: { userId: string; status: "active" | "disabled"; reason: string; actorId: string },
): Promise<{ sessionsRevoked: number }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const locked = await client.query<{ status: "active" | "disabled"; engine_status: string | null }>(`
      SELECT u.status, ea.status AS engine_status
      FROM users u LEFT JOIN engine_accounts ea ON ea.user_id = u.id
      WHERE u.id = $1
      FOR UPDATE OF u
    `, [input.userId]);
    if (!locked.rows[0]) throw new AdminUserNotFoundError();
    await client.query("UPDATE users SET status = $2, updated_at = now() WHERE id = $1", [input.userId, input.status]);
    if (input.status === "disabled") {
      await client.query("UPDATE engine_accounts SET status = 'disabled', updated_at = now() WHERE user_id = $1", [input.userId]);
    } else if (locked.rows[0].engine_status === "disabled") {
      await client.query("UPDATE engine_accounts SET status = 'active', updated_at = now() WHERE user_id = $1", [input.userId]);
    }
    const revoked = input.status === "disabled"
      ? await client.query("UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL", [input.userId])
      : { rowCount: 0 };
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('commercial-admin', $3, $2, 'user', $1, $4::jsonb)
    `, [input.userId, `admin.user.${input.status}`, input.actorId, JSON.stringify({ reason: input.reason })]);
    await client.query("COMMIT");
    return { sessionsRevoked: revoked.rowCount ?? 0 };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function revokeAdminUserSessions(
  database: Database,
  input: { userId: string; reason: string; actorId: string },
): Promise<number> {
  return adminSecurityAction(database, input, "admin.sessions.revoked", false);
}

export async function resetAdminUserTotp(
  database: Database,
  input: { userId: string; reason: string; actorId: string },
): Promise<number> {
  return adminSecurityAction(database, input, "admin.totp.reset", true);
}

async function adminSecurityAction(
  database: Database,
  input: { userId: string; reason: string; actorId: string },
  action: string,
  resetTotp: boolean,
): Promise<number> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const user = await client.query("SELECT id FROM users WHERE id = $1 FOR UPDATE", [input.userId]);
    if (!user.rows[0]) throw new AdminUserNotFoundError();
    if (resetTotp) {
      await client.query(`
        UPDATE users SET totp_secret = NULL, totp_enabled = false, updated_at = now() WHERE id = $1
      `, [input.userId]);
    }
    const revoked = await client.query(`
      UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL
    `, [input.userId]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('commercial-admin', $3, $2, 'user', $1, $4::jsonb)
    `, [input.userId, action, input.actorId, JSON.stringify({ reason: input.reason, sessions_revoked: revoked.rowCount ?? 0 })]);
    await client.query("COMMIT");
    return revoked.rowCount ?? 0;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export class AdminUserNotFoundError extends Error {
  constructor() {
    super("user not found");
    this.name = "AdminUserNotFoundError";
  }
}
