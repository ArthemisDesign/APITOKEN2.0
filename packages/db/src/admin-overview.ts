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
  paidAt: Date;
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
  email: string;
  multiplierBp: number;
  expiresAt: Date;
  consumedAt: Date | null;
  consumedByUserId: string | null;
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
  paidPaymentsCount: number;
  paidTotalNano: string;
  lastPaidAt: Date | null;
  pendingCheckoutsCount: number;
  apiKeysActive: number;
  apiKeysTotal: number;
  lastSeenAt: Date | null;
  spent30dNano: string;
}

export interface AdminUserOverviewQuery {
  limit?: number;
  offset?: number;
  search?: string;
  status?: "active" | "disabled";
  auth?: "password" | "google" | "github";
  customerType?: "b2c" | "b2b";
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
  paid_payments_count: string;
  paid_total_nano: string;
  last_paid_at: Date | null;
  pending_checkouts_count: string;
  api_keys_active: string;
  api_keys_total: string;
  last_seen_at: Date | null;
  spent_30d_nano: string;
}

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
      COALESCE(p.paid_count, 0)::text AS paid_payments_count,
      COALESCE(p.paid_total, 0)::text AS paid_total_nano,
      p.last_paid_at,
      COALESCE(cs.pending_count, 0)::text AS pending_checkouts_count,
      COALESCE(k.active_count, 0)::text AS api_keys_active,
      COALESCE(k.total_count, 0)::text AS api_keys_total,
      s.last_seen_at,
      COALESCE(ue.spent_30d, 0)::text AS spent_30d_nano
    FROM users u
    LEFT JOIN customer_profiles cp ON cp.user_id = u.id
    LEFT JOIN engine_accounts ea ON ea.user_id = u.id
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
      SELECT sum(amount_nano) AS spent_30d
      FROM pricing_usage_events
      WHERE user_id = u.id AND occurred_at > now() - interval '30 days'
    ) ue ON TRUE
    WHERE ${filters}
    ORDER BY u.created_at DESC
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
    paidPaymentsCount: Number(row.paid_payments_count),
    paidTotalNano: row.paid_total_nano,
    lastPaidAt: row.last_paid_at,
    pendingCheckoutsCount: Number(row.pending_checkouts_count),
    apiKeysActive: Number(row.api_keys_active),
    apiKeysTotal: Number(row.api_keys_total),
    lastSeenAt: row.last_seen_at,
    spent30dNano: row.spent_30d_nano,
    })),
    total: Number(countResult.rows[0]?.total ?? 0),
    limit,
    offset,
  };
}

export async function getAdminDashboard(database: Database): Promise<AdminDashboard> {
  const result = await database.pool.query<Record<string, string | Date>>(`
    WITH user_auth AS (
      SELECT u.id, u.status, u.created_at, u.email_verified, u.totp_enabled,
             (u.password_hash IS NOT NULL) AS has_password,
             EXISTS (SELECT 1 FROM auth_identities ai WHERE ai.user_id = u.id) AS has_oauth,
             EXISTS (SELECT 1 FROM auth_identities ai WHERE ai.user_id = u.id AND ai.provider = 'google') AS has_google,
             EXISTS (SELECT 1 FROM auth_identities ai WHERE ai.user_id = u.id AND ai.provider = 'github') AS has_github,
             EXISTS (
               SELECT 1 FROM audit_log al
               WHERE al.target_type = 'user' AND al.target_id = u.id::text
                 AND al.action = 'auth.oauth_registered'
             ) AS registered_oauth,
             EXISTS (
               SELECT 1 FROM auth_sessions s
               WHERE s.user_id = u.id AND s.last_seen_at >= now() - interval '7 days'
             ) AS active_7d
      FROM users u
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
      (SELECT count(*) FROM user_auth) AS users_total,
      (SELECT count(*) FROM user_auth WHERE status = 'active') AS users_active,
      (SELECT count(*) FROM user_auth WHERE status = 'disabled') AS users_disabled,
      (SELECT count(*) FROM user_auth WHERE created_at >= now() - interval '24 hours') AS users_registered_24h,
      (SELECT count(*) FROM user_auth WHERE created_at >= now() - interval '30 days') AS users_registered_30d,
      (SELECT count(*) FROM user_auth WHERE active_7d) AS users_active_7d,
      (SELECT count(*) FROM user_auth WHERE registered_oauth) AS users_registered_oauth,
      (SELECT count(*) FROM user_auth WHERE NOT registered_oauth) AS users_registered_password,
      (SELECT count(*) FROM user_auth WHERE has_password AND NOT has_oauth) AS users_password_only,
      (SELECT count(*) FROM user_auth WHERE has_oauth AND NOT has_password) AS users_oauth_only,
      (SELECT count(*) FROM user_auth WHERE has_oauth AND has_password) AS users_hybrid,
      (SELECT count(*) FROM user_auth WHERE has_google) AS users_google,
      (SELECT count(*) FROM user_auth WHERE has_github) AS users_github,
      (SELECT count(*) FROM user_auth WHERE email_verified) AS users_verified,
      (SELECT count(*) FROM user_auth WHERE totp_enabled) AS users_totp,
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
    FROM paid CROSS JOIN manual
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

export async function listAdminTopups(database: Database, limit: number): Promise<{
  payments: AdminTopupRow[];
  checkouts: AdminCheckoutRow[];
}> {
  const [payments, checkouts] = await Promise.all([
    database.pool.query<{
      id: string; user_id: string; email: string; provider: string; provider_payment_id: string;
      amount_nano: string; currency: string; status: string; paid_at: Date; created_at: Date;
      credit_status: string | null;
    }>(`
      SELECT p.id, p.user_id, u.email, p.provider, p.provider_payment_id,
             p.amount_nano::text AS amount_nano, p.currency, p.status, p.paid_at, p.created_at,
             ec.status AS credit_status
      FROM payments p
      JOIN users u ON u.id = p.user_id
      LEFT JOIN engine_credits ec ON ec.payment_id = p.id
      WHERE p.paid_at IS NOT NULL
      ORDER BY p.paid_at DESC
      LIMIT $1
    `, [limit]),
    database.pool.query<{
      id: string; user_id: string; email: string; provider: string; provider_payment_id: string | null;
      amount_usd: string; status: string; created_at: Date; completed_at: Date | null; expires_at: Date | null;
    }>(`
      SELECT cs.id, cs.user_id, u.email, cs.provider, cs.provider_payment_id,
             cs.amount_usd::text AS amount_usd, cs.status, cs.created_at, cs.completed_at, cs.expires_at
      FROM checkout_sessions cs
      JOIN users u ON u.id = cs.user_id
      WHERE cs.status <> 'paid'
      ORDER BY cs.created_at DESC
      LIMIT $1
    `, [limit]),
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
  };
}

export async function listAdminAudit(database: Database, limit: number): Promise<AdminAuditRow[]> {
  const result = await database.pool.query<{
    id: string; actor_type: string; actor_id: string | null; action: string; target_type: string;
    target_id: string; metadata: unknown; created_at: Date;
  }>(`
    SELECT id::text, actor_type, actor_id, action, target_type, target_id, metadata, created_at
    FROM audit_log
    ORDER BY created_at DESC
    LIMIT $1
  `, [limit]);
  return result.rows.map((row) => ({
    id: row.id, actorType: row.actor_type, actorId: row.actor_id, action: row.action,
    targetType: row.target_type, targetId: row.target_id, metadata: row.metadata, createdAt: row.created_at,
  }));
}

export async function listAdminBusinessInvites(database: Database, limit: number): Promise<AdminBusinessInviteRow[]> {
  const result = await database.pool.query<{
    id: string; email: string; multiplier_bp: number; expires_at: Date; consumed_at: Date | null;
    consumed_by_user_id: string | null; created_at: Date;
  }>(`
    SELECT id, email, multiplier_bp, expires_at, consumed_at, consumed_by_user_id, created_at
    FROM business_invites
    ORDER BY created_at DESC
    LIMIT $1
  `, [limit]);
  return result.rows.map((row) => ({
    id: row.id, email: row.email, multiplierBp: row.multiplier_bp, expiresAt: row.expires_at,
    consumedAt: row.consumed_at, consumedByUserId: row.consumed_by_user_id, createdAt: row.created_at,
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
