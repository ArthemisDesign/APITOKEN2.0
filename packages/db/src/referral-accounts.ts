import type { Database } from "./client.js";

export interface ReferralCommerceAccount {
  id: string;
  email: string;
  emailVerified: boolean;
  status: "active" | "disabled";
  customerType: "b2c" | "b2b" | null;
  discountBps: number | null;
  providerDiscounts: Array<{ providerId: string; discountBps: number }>;
}

interface ReferralCommerceAccountRow {
  id: string;
  email: string;
  email_verified: boolean;
  status: "active" | "disabled";
  customer_type: "b2c" | "b2b" | null;
  multiplier_bp: number | null;
  provider_discounts: unknown;
}

const ACCOUNT_PROJECTION = `
  SELECT
    u.id,
    u.email,
    u.email_verified,
    u.status,
    profile.customer_type,
    profile.multiplier_bp,
    COALESCE((
      SELECT jsonb_agg(
        jsonb_build_object(
          'providerId', discount.provider_id,
          'discountBps', 10000 - discount.multiplier_bp
        )
        ORDER BY discount.provider_id
      )
      FROM customer_provider_discounts discount
      WHERE discount.user_id = u.id
    ), '[]'::jsonb) AS provider_discounts
  FROM users u
  LEFT JOIN customer_profiles profile ON profile.user_id = u.id
`;

export async function findActiveReferralCommerceAccountByEmail(
  database: Database,
  email: string,
): Promise<ReferralCommerceAccount | null> {
  const normalized = email.trim().toLowerCase();
  if (!normalized) return null;
  const result = await database.pool.query<ReferralCommerceAccountRow>(`${ACCOUNT_PROJECTION}
    WHERE lower(u.email) = $1 AND u.status = 'active'
    LIMIT 1
  `, [normalized]);
  return result.rows[0] ? mapAccount(result.rows[0]) : null;
}

export async function findActiveReferralCommerceAccountById(
  database: Database,
  userId: string,
): Promise<ReferralCommerceAccount | null> {
  const result = await database.pool.query<ReferralCommerceAccountRow>(`${ACCOUNT_PROJECTION}
    WHERE u.id = $1 AND u.status = 'active'
    LIMIT 1
  `, [userId]);
  return result.rows[0] ? mapAccount(result.rows[0]) : null;
}

export async function listReferralCommerceAccountsByIds(
  database: Database,
  userIds: readonly string[],
): Promise<ReferralCommerceAccount[]> {
  const uniqueIds = [...new Set(userIds)];
  if (uniqueIds.length === 0) return [];
  const result = await database.pool.query<ReferralCommerceAccountRow>(`${ACCOUNT_PROJECTION}
    WHERE u.id = ANY($1::uuid[])
  `, [uniqueIds]);
  return result.rows.map(mapAccount);
}

function mapAccount(row: ReferralCommerceAccountRow): ReferralCommerceAccount {
  return {
    id: row.id,
    email: row.email,
    emailVerified: row.email_verified,
    status: row.status,
    customerType: row.customer_type,
    discountBps: row.multiplier_bp === null ? null : 10_000 - row.multiplier_bp,
    providerDiscounts: parseProviderDiscounts(row.provider_discounts),
  };
}

function parseProviderDiscounts(value: unknown): Array<{ providerId: string; discountBps: number }> {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object" || Array.isArray(item)) return [];
    const providerId = (item as Record<string, unknown>).providerId;
    const discountBps = (item as Record<string, unknown>).discountBps;
    return typeof providerId === "string" && typeof discountBps === "number" && Number.isInteger(discountBps)
      ? [{ providerId, discountBps }]
      : [];
  });
}
