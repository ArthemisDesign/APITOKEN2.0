import { z } from "zod";

// json-bigint returns unsafe integers as strings and small exact integers as numbers. Normalize at
// the transport boundary so money is always represented as a decimal string inside our services.
export const decimalIntegerSchema = z.union([
  z.string().regex(/^-?\d+$/),
  z.number().int().safe(),
]).transform(String);
export const nonNegativeIntegerSchema = z.union([
  z.string().regex(/^\d+$/),
  z.number().int().safe().nonnegative(),
]).transform(String);

export const engineAccountSchema = z.object({
  account: z.string().startsWith("acct_"),
  balance_nano: decimalIntegerSchema,
  spent_nano: decimalIntegerSchema,
  reserved_nano: decimalIntegerSchema,
  balance: z.string(),
  mult_bp: z.number().int(),
  status: z.enum(["active", "disabled"]),
  handle: z.string().nullable(),
});

export type EngineAccount = z.infer<typeof engineAccountSchema>;

export const engineCreditResultSchema = z.object({
  account: z.string().startsWith("acct_"),
  balance_nano: decimalIntegerSchema,
  balance: z.string(),
});

export type EngineCreditResult = z.infer<typeof engineCreditResultSchema>;

export const engineApiKeySchema = z.object({
  key_id: z.string().startsWith("key_"),
  key_masked: z.string().min(1),
  label: z.string().nullable(),
  status: z.enum(["active", "disabled"]),
  spent_nano: nonNegativeIntegerSchema,
  spent: z.string(),
});

export type EngineApiKey = z.infer<typeof engineApiKeySchema>;

export const engineApiKeyListSchema = z.object({
  account: z.string().startsWith("acct_"),
  keys: z.array(engineApiKeySchema),
});

export const issuedEngineApiKeySchema = z.object({
  key: z.string().startsWith("sk-pool-"),
  key_id: z.string().startsWith("key_"),
  account: z.string().startsWith("acct_"),
  label: z.string().nullable(),
});

export type IssuedEngineApiKey = z.infer<typeof issuedEngineApiKeySchema>;

export const engineLedgerEntrySchema = z.object({
  id: nonNegativeIntegerSchema,
  kind: z.enum(["topup", "charge", "adjust"]),
  amount_nano: decimalIntegerSchema,
  amount: z.string(),
  key_masked: z.string().nullable(),
  ref: z.string().nullable(),
  balance_after_nano: decimalIntegerSchema.nullable(),
  ts: nonNegativeIntegerSchema,
});

export type EngineLedgerEntry = z.infer<typeof engineLedgerEntrySchema>;

export const engineLedgerSchema = z.object({
  account: z.string().startsWith("acct_"),
  entries: z.array(engineLedgerEntrySchema),
});

export const createEngineAccountSchema = z.object({
  handle: z.string().trim().min(1).max(200).optional(),
  multBp: z.number().int().min(0).max(100_000).optional(),
});

export type CreateEngineAccount = z.infer<typeof createEngineAccountSchema>;

export const enqueueCreditSchema = z.object({
  paymentId: z.string().uuid(),
  engineAccountId: z.string().startsWith("acct_"),
  amountNano: z.coerce.bigint().positive(),
  idempotencyRef: z.string().trim().min(1).max(255),
});

export type EnqueueCredit = z.infer<typeof enqueueCreditSchema>;

export interface HealthStatus {
  ok: boolean;
  service: string;
  database: "up" | "down";
  engine: "up" | "down" | "unchecked";
}

/** User-entered whole USD. JSON numbers and decimal points are intentionally rejected. */
export const wholeUsdSchema = z.string().regex(/^[1-9]\d*$/, "amountUsd must contain positive whole USD digits only");

export const createCheckoutSchema = z.object({
  amountUsd: wholeUsdSchema,
  provider: z.literal("cryptomus").default("cryptomus"),
});

export type CreateCheckout = z.infer<typeof createCheckoutSchema>;

export const checkoutStatusSchema = z.enum(["creating", "pending", "paid", "canceled", "refunded", "failed"]);

export interface CheckoutView {
  id: string;
  provider: string;
  amountUsd: string;
  status: z.infer<typeof checkoutStatusSchema>;
  checkoutUrl: string | null;
  expiresAt: string | null;
}

export const authEmailSchema = z.string().trim().toLowerCase().email().max(254);
export const authPasswordSchema = z.string().min(12).max(128)
  .refine((value) => Buffer.byteLength(value, "utf8") <= 256, "password is too long");

const credentialsSchema = z.object({
  email: authEmailSchema,
  password: authPasswordSchema,
});

export const registerSchema = credentialsSchema.extend({
  inviteToken: z.string().regex(/^[A-Za-z0-9_-]{43}$/).optional(),
}).strict();

export const loginSchema = credentialsSchema.strict();

export const emailOnlySchema = z.object({ email: authEmailSchema }).strict();
export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const verifyEmailSchema = z.object({ token: authTokenSchema }).strict();
export const resetPasswordSchema = z.object({ token: authTokenSchema, password: authPasswordSchema }).strict();
export const oauthProviderSchema = z.enum(["google", "github"]);

export const createApiKeySchema = z.object({
  label: z.string().trim().min(1).max(100).optional(),
}).strict();

export const B2C_PRICING_TIERS = [
  { code: "starter", discountPercent: 60, multiplierBp: 4000, spendThresholdNano: 0n, visibleOfficialUsageUsd: "0" },
  { code: "builder", discountPercent: 65, multiplierBp: 3500, spendThresholdNano: 25_000_000_000n, visibleOfficialUsageUsd: "70" },
  { code: "pro", discountPercent: 70, multiplierBp: 3000, spendThresholdNano: 75_000_000_000n, visibleOfficialUsageUsd: "250" },
  { code: "studio", discountPercent: 75, multiplierBp: 2500, spendThresholdNano: 200_000_000_000n, visibleOfficialUsageUsd: "800" },
  { code: "scale", discountPercent: 80, multiplierBp: 2000, spendThresholdNano: 500_000_000_000n, visibleOfficialUsageUsd: "2500" },
] as const;

export const B2C_SIGNUP_BONUS_OFFICIAL_USD = "10";
export const B2C_SIGNUP_BONUS_OFFICIAL_NANO = 10_000_000_000n;
export const B2C_SIGNUP_BONUS_BALANCE_NANO =
  B2C_SIGNUP_BONUS_OFFICIAL_NANO * BigInt(B2C_PRICING_TIERS[0].multiplierBp) / 10_000n;

export const businessDiscountSchema = z.number().int().min(0).max(95);
export const createBusinessInviteSchema = z.object({
  email: authEmailSchema,
  discountPercent: businessDiscountSchema,
  expiresInDays: z.number().int().min(1).max(30).default(7),
}).strict();
export const setBusinessPricingSchema = z.object({ discountPercent: businessDiscountSchema }).strict();

export function multiplierForDiscount(discountPercent: number): number {
  return 10_000 - discountPercent * 100;
}

export interface AuthUserView {
  id: string;
  email: string;
  emailVerified: boolean;
  engineAccountStatus: "pending" | "active" | "error" | "disabled";
  customerType: "b2c" | "b2b";
}
