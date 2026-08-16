import type { Database } from "./client.js";

export interface CrmReferralProviderOverride {
  providerId: string;
  multiplierBp: number;
}

export interface CrmReferralRegistrationRow {
  candidateId: string;
  email: string;
  emailVerified: boolean;
  registeredAt: Date;
  customerStatus: "active" | "disabled";
  customerType: "b2c" | "b2b" | null;
  defaultMultiplierBp: number | null;
  providerOverrides: CrmReferralProviderOverride[];
  paidTopupNano: bigint;
  refundedNano: bigint;
  usageSpentNano: bigint;
  customerFundedSpentNano: bigint;
  engineAccountId: string | null;
  projectedEngineStatus: "pending" | "active" | "error" | "disabled" | null;
}

interface CrmReferralRegistrationDatabaseRow {
  candidate_id: string;
  email: string;
  email_verified: boolean;
  registered_at: Date;
  customer_status: "active" | "disabled";
  customer_type: "b2c" | "b2b" | null;
  default_multiplier_bp: number | null;
  provider_overrides: unknown;
  paid_topup_nano: string;
  refunded_nano: string;
  usage_spent_nano: string;
  customer_funded_spent_nano: string;
  engine_account_id: string | null;
  projected_engine_status: "pending" | "active" | "error" | "disabled" | null;
}

/**
 * Reads registrations scoped by one opaque Commerce referral alias. Callers must obtain the alias
 * from the trusted Sales external-reference binding; this function deliberately has no user/email
 * lookup mode.
 */
export async function listCrmReferralRegistrations(
  database: Database,
  referralAlias: string,
): Promise<CrmReferralRegistrationRow[]> {
  const result = await database.pool.query<CrmReferralRegistrationDatabaseRow>(`
    WITH attributed AS (
      SELECT attribution.user_id, min(attribution.created_at) AS registered_at
      FROM referral_attributions attribution
      WHERE attribution.code = $1
      GROUP BY attribution.user_id
    ), payment_totals AS (
      SELECT payment.user_id,
        COALESCE(sum(payment.amount_nano) FILTER (
          WHERE payment.status IN ('paid', 'refunded', 'disputed')
        ), 0) AS paid_topup_nano,
        COALESCE(sum(payment.amount_nano) FILTER (
          WHERE payment.status IN ('refunded', 'disputed')
        ), 0) AS refunded_nano
      FROM payments payment
      JOIN attributed ON attributed.user_id = payment.user_id
      GROUP BY payment.user_id
    ), usage_totals AS (
      SELECT usage.user_id,
        COALESCE(sum(usage.amount_nano), 0) AS usage_spent_nano,
        COALESCE(sum(usage.real_funded_nano), 0) AS customer_funded_spent_nano
      FROM pricing_usage_events usage
      JOIN attributed ON attributed.user_id = usage.user_id
      GROUP BY usage.user_id
    ), provider_overrides AS (
      SELECT discount.user_id,
        jsonb_agg(jsonb_build_object(
          'providerId', discount.provider_id,
          'multiplierBp', discount.multiplier_bp
        ) ORDER BY discount.provider_id) AS items
      FROM customer_provider_discounts discount
      JOIN attributed ON attributed.user_id = discount.user_id
      GROUP BY discount.user_id
    )
    SELECT user_account.id AS candidate_id,
      user_account.email,
      user_account.email_verified,
      attributed.registered_at,
      user_account.status AS customer_status,
      profile.customer_type,
      profile.multiplier_bp AS default_multiplier_bp,
      COALESCE(provider_overrides.items, '[]'::jsonb) AS provider_overrides,
      COALESCE(payment_totals.paid_topup_nano, 0)::text AS paid_topup_nano,
      COALESCE(payment_totals.refunded_nano, 0)::text AS refunded_nano,
      COALESCE(usage_totals.usage_spent_nano, 0)::text AS usage_spent_nano,
      COALESCE(usage_totals.customer_funded_spent_nano, 0)::text
        AS customer_funded_spent_nano,
      engine.engine_account_id,
      engine.status AS projected_engine_status
    FROM attributed
    JOIN users user_account ON user_account.id = attributed.user_id
    LEFT JOIN customer_profiles profile ON profile.user_id = attributed.user_id
    LEFT JOIN engine_accounts engine ON engine.user_id = attributed.user_id
    LEFT JOIN payment_totals ON payment_totals.user_id = attributed.user_id
    LEFT JOIN usage_totals ON usage_totals.user_id = attributed.user_id
    LEFT JOIN provider_overrides ON provider_overrides.user_id = attributed.user_id
    ORDER BY attributed.registered_at, user_account.id
  `, [referralAlias]);

  return result.rows.map((row) => ({
    candidateId: row.candidate_id,
    email: row.email,
    emailVerified: row.email_verified,
    registeredAt: row.registered_at,
    customerStatus: row.customer_status,
    customerType: row.customer_type,
    defaultMultiplierBp: row.default_multiplier_bp,
    providerOverrides: parseProviderOverrides(row.provider_overrides),
    paidTopupNano: BigInt(row.paid_topup_nano),
    refundedNano: BigInt(row.refunded_nano),
    usageSpentNano: BigInt(row.usage_spent_nano),
    customerFundedSpentNano: BigInt(row.customer_funded_spent_nano),
    engineAccountId: row.engine_account_id,
    projectedEngineStatus: row.projected_engine_status,
  }));
}

function parseProviderOverrides(value: unknown): CrmReferralProviderOverride[] {
  if (!Array.isArray(value)) throw new Error("invalid CRM referral provider overrides projection");
  return value.map((item) => {
    if (
      typeof item !== "object"
      || item === null
      || !("providerId" in item)
      || typeof item.providerId !== "string"
      || !("multiplierBp" in item)
      || typeof item.multiplierBp !== "number"
      || !Number.isInteger(item.multiplierBp)
      || item.multiplierBp < 0
      || item.multiplierBp > 10_000
    ) {
      throw new Error("invalid CRM referral provider override row");
    }
    return { providerId: item.providerId, multiplierBp: item.multiplierBp };
  });
}
