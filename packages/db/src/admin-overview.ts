import type { Database } from "./client.js";

// Админ-обзор пользователей для панели (panel.apitoken.sale → GET /admin/users).
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
}): Promise<void> {
  await database.pool.query(
    `INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
     VALUES ('commercial-admin', NULL, 'admin.credit', 'user', $1, $2)`,
    [input.userId, JSON.stringify({
      engine_account_id: input.engineAccountId,
      amount_nano: input.amountNano.toString(),
      ref: input.ref,
      balance_after_nano: input.balanceAfterNano,
    })],
  );
}

export async function listAdminUserOverview(database: Database): Promise<AdminUserOverviewRow[]> {
  const result = await database.pool.query<RawRow>(`
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
    ORDER BY u.created_at DESC
  `);
  return result.rows.map((row) => ({
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
  }));
}
