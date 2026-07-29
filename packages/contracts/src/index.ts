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

export const engineAccountListSchema = z.object({
  accounts: z.array(engineAccountSchema),
});

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
  reserved_nano: nonNegativeIntegerSchema.nullish().transform((value) => value ?? "0"),
  spend_limit_nano: nonNegativeIntegerSchema.nullish().transform((value) => value ?? null),
  expires_ts: nonNegativeIntegerSchema.nullish().transform((value) => value ?? null),
  created_ts: nonNegativeIntegerSchema.nullish().transform((value) => value ?? "0"),
  last_used_ts: nonNegativeIntegerSchema.nullish().transform((value) => value ?? null),
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
  spend_limit_nano: nonNegativeIntegerSchema.nullish().transform((value) => value ?? null),
  expires_ts: nonNegativeIntegerSchema.nullish().transform((value) => value ?? null),
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
  // Claude-модель за charge-строкой (topup/adjust → null). nullish — устойчивость к старому движку без поля.
  model: z.string().nullish(),
});

export type EngineLedgerEntry = z.infer<typeof engineLedgerEntrySchema>;

export const engineLedgerSchema = z.object({
  account: z.string().startsWith("acct_"),
  entries: z.array(engineLedgerEntrySchema),
});

// Разбивка расхода по токенам/моделям (`/admin/account/:id/usage`). Токены — числа (реалистично
// < 2^53); нанодоллары — decimal-строки (bigint-safe, деньги никогда не через JS number).
const usageBucketSchema = z.object({
  tokens: z.coerce.number().int().nonnegative(),
  official_nano: decimalIntegerSchema,
});
const usageWebSearchBucketSchema = z.object({
  requests: z.coerce.number().int().nonnegative(),
  official_nano: decimalIntegerSchema,
});
const usageMoneyOnlyBucketSchema = z.object({
  official_nano: decimalIntegerSchema,
});
export const engineUsageModelSchema = z.object({
  model: z.string(),
  provider: z.enum(["anthropic", "openai"]).optional(),
  requests: z.coerce.number().int().nonnegative(),
  input_tokens: z.coerce.number().int().nonnegative(),
  output_tokens: z.coerce.number().int().nonnegative(),
  cache_read_tokens: z.coerce.number().int().nonnegative(),
  cache_write_5m_tokens: z.coerce.number().int().nonnegative(),
  cache_write_1h_tokens: z.coerce.number().int().nonnegative(),
  web_search_requests: z.coerce.number().int().nonnegative(),
  official_nano: decimalIntegerSchema,
  charged_nano: decimalIntegerSchema,
});
export const engineUsageSchema = z.object({
  account: z.string().startsWith("acct_"),
  window: z.string(),
  since_ts: z.coerce.number().int().nonnegative(),
  until_ts: z.coerce.number().int().nonnegative(),
  requests: z.coerce.number().int().nonnegative(),
  total_official_nano: decimalIntegerSchema,
  total_charged_nano: decimalIntegerSchema,
  buckets: z.object({
    input: usageBucketSchema,
    output: usageBucketSchema,
    cache_read: usageBucketSchema,
    cache_write: usageBucketSchema,
    web_search: usageWebSearchBucketSchema,
    unattributed_legacy: usageMoneyOnlyBucketSchema,
  }),
  models: z.array(engineUsageModelSchema),
  daily: z.array(z.object({
    day_ts: z.coerce.number().int().nonnegative(),
    requests: z.coerce.number().int().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })),
  daily_providers: z.array(z.object({
    day_ts: z.coerce.number().int().nonnegative(),
    provider: z.enum(["anthropic", "openai"]),
    requests: z.coerce.number().int().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })).default([]),
  keys: z.array(z.object({
    key_masked: z.string().nullable(),
    requests: z.coerce.number().int().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })),
});

export type EngineUsage = z.infer<typeof engineUsageSchema>;

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

export const paymentProviderSchema = z.enum(["cryptomus", "platega"]);

export const createCheckoutSchema = z.object({
  amountUsd: wholeUsdSchema,
  provider: paymentProviderSchema.default("platega"),
  // Platega payment-method id (2 SBP, 3 ERIP, 11 card, 12 international, 13 crypto). Ignored by other providers.
  paymentMethod: z.coerce.number().int().positive().optional(),
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
export const authPasswordSchema = z.string().min(8).max(128)
  .refine((value) => Buffer.byteLength(value, "utf8") <= 256, "password is too long");

const credentialsSchema = z.object({
  email: authEmailSchema,
  password: authPasswordSchema,
});

export const registerSchema = credentialsSchema.extend({
  inviteToken: z.string().regex(/^[A-Za-z0-9_-]{43}$/).optional(),
  // Партнёрский реф-код из ссылки ?ref=CODE (sales.apitoken.sale). Только атрибуция —
  // на цену/бонусы пользователя не влияет.
  referralCode: z.string().trim().regex(/^[A-Za-z0-9_-]{3,32}$/).optional(),
}).strict();

export const loginSchema = credentialsSchema.strict();

export const emailOnlySchema = z.object({ email: authEmailSchema }).strict();
export const authTokenSchema = z.string().regex(/^[A-Za-z0-9_-]{43}$/);
export const verifyEmailSchema = z.object({ token: authTokenSchema }).strict();
export const resetPasswordSchema = z.object({ token: authTokenSchema, password: authPasswordSchema }).strict();
export const oauthProviderSchema = z.enum(["google", "github"]);
export const displayNameSchema = z.string().trim().min(1).max(80)
  .refine((value) => !/[\u0000-\u001f\u007f]/.test(value), "display name contains unsupported characters");
export const updateProfileSchema = z.object({ displayName: displayNameSchema }).strict();

// 6-значный TOTP-код (Google Authenticator). Требуется на выпуск ключа, если 2FA включена.
export const totpCodeSchema = z.string().regex(/^\d{6}$/);
export const totpCodeBodySchema = z.object({ code: totpCodeSchema }).strict();

const maxSignedI64 = 9_223_372_036_854_775_807n;
const usdAmountPattern = /^(?:0\.\d{1,2}|[1-9]\d*(?:\.\d{1,2})?)$/;
const preciseUsdAmountPattern = /^(?:0\.\d{1,9}|[1-9]\d*(?:\.\d{1,9})?)$/;

function usdAmountWithinEngineRange(value: string, pattern: RegExp): boolean {
  if (!pattern.test(value)) return true;
  const [whole = "0", fraction = ""] = value.split(".");
  const nano = BigInt(whole) * 1_000_000_000n + BigInt(fraction.padEnd(9, "0"));
  return nano > 0n && nano <= maxSignedI64;
}

const futureExpirationSchema = z.string().datetime({ offset: true })
  .refine((value) => Math.floor(Date.parse(value) / 1000) > Math.floor(Date.now() / 1000),
    "expiresAt must be at least one whole second in the future");

export const createApiKeySchema = z.object({
  label: z.string().trim().min(1).max(64).optional(),
  spendLimitUsd: z.string().trim()
    .regex(usdAmountPattern, "spendLimitUsd must be a positive USD amount with at most 2 decimals")
    .refine((value) => usdAmountWithinEngineRange(value, usdAmountPattern),
      "spendLimitUsd must be positive and within the engine maximum")
    .optional(),
  expiresAt: futureExpirationSchema.optional(),
  totpCode: totpCodeSchema.optional(),
}).strict();

export type CreateApiKey = z.infer<typeof createApiKeySchema>;

export const updateApiKeyPolicySchema = z.object({
  spendLimitUsd: z.string().trim()
    .regex(preciseUsdAmountPattern, "spendLimitUsd must be a positive USD amount with at most 9 decimals")
    .refine((value) => usdAmountWithinEngineRange(value, preciseUsdAmountPattern),
      "spendLimitUsd must be positive and within the engine maximum")
    .nullable(),
  expiresAt: futureExpirationSchema.nullable(),
  totpCode: totpCodeSchema.optional(),
}).strict();

export type UpdateApiKeyPolicy = z.infer<typeof updateApiKeyPolicySchema>;

export const renameApiKeySchema = z.object({
  label: z.string().trim().min(1).max(64),
}).strict();

export type RenameApiKey = z.infer<typeof renameApiKeySchema>;

// Prepay-модель: Starter −60% даётся СТАРТОВО (базовый тир, порог $0, удержания нет). Выше тир
// повышается только по идемпотентно потреблённым charge-строкам engine ledger: `spendThresholdNano`
// — порог накопленного `pricing_usage_events.amount_nano`, `holdNano` — расход за скользящие 30 дней.
export const B2C_PRICING_TIERS = [
  { code: "starter", discountPercent: 60, multiplierBp: 4000, spendThresholdNano: 0n, holdNano: 0n, visibleOfficialUsageUsd: "0" },
  { code: "builder", discountPercent: 62.5, multiplierBp: 3750, spendThresholdNano: 100_000_000_000n, holdNano: 50_000_000_000n, visibleOfficialUsageUsd: "267" },
  { code: "pro", discountPercent: 65, multiplierBp: 3500, spendThresholdNano: 250_000_000_000n, holdNano: 125_000_000_000n, visibleOfficialUsageUsd: "714" },
  { code: "studio", discountPercent: 67.5, multiplierBp: 3250, spendThresholdNano: 500_000_000_000n, holdNano: 250_000_000_000n, visibleOfficialUsageUsd: "1538" },
  { code: "scale", discountPercent: 70, multiplierBp: 3000, spendThresholdNano: 1_000_000_000_000n, holdNano: 500_000_000_000n, visibleOfficialUsageUsd: "3333" },
] as const;

export const B2C_SIGNUP_BONUS_OFFICIAL_USD = "10";
export const B2C_SIGNUP_BONUS_OFFICIAL_NANO = 10_000_000_000n;
export const B2C_SIGNUP_BONUS_BALANCE_NANO =
  B2C_SIGNUP_BONUS_OFFICIAL_NANO * BigInt(B2C_PRICING_TIERS[0].multiplierBp) / 10_000n;

export const businessDiscountSchema = z.number().int().min(0).max(95);
export const createBusinessInviteSchema = z.object({
  email: authEmailSchema.optional(),
  discountPercent: businessDiscountSchema,
  expiresInDays: z.number().int().min(1).max(30).default(7),
  reason: z.string().trim().min(3).max(300),
  idempotencyKey: z.string().uuid(),
}).strict();
export const setBusinessPricingSchema = z.object({
  discountPercent: businessDiscountSchema,
  reason: z.string().trim().min(3).max(300),
}).strict();

export function multiplierForDiscount(discountPercent: number): number {
  return 10_000 - discountPercent * 100;
}

export interface AuthUserView {
  id: string;
  email: string;
  displayName: string;
  emailVerified: boolean;
  passwordEnabled: boolean;
  engineAccountStatus: "pending" | "active" | "error" | "disabled";
  customerType: "b2c" | "b2b";
  totpEnabled: boolean;
}

export const contentLocaleSchema = z.enum(["en", "ru"]);
export const contentRevisionScopeSchema = z.enum(["draft", "platform", "project", "all"]);
export const contentProfileKeySchema = z.string().trim().regex(/^[a-z][a-z0-9-]{0,39}$/);

export const importContentProjectSchema = z.object({
  sourceUrl: z.string().url().max(2_048),
  locale: contentLocaleSchema.default("en"),
  sourceContent: z.string().trim().max(100_000).optional(),
}).strict();

export const updateContentProjectSchema = z.object({
  sourceTitle: z.string().trim().max(300).optional(),
  sourceAuthor: z.string().trim().max(200).nullable().optional(),
  sourceContent: z.string().trim().min(1).max(100_000).optional(),
  briefMarkdown: z.string().trim().max(100_000).optional(),
}).strict();

export const contentSourceSchema = z.object({
  url: z.string().url().max(2_048),
  title: z.string().trim().max(300).default(""),
  sourceType: z.enum(["primary", "reference", "verification"]).default("reference"),
  publisher: z.string().trim().max(200).nullable().optional(),
  notes: z.string().trim().max(2_000).default(""),
}).strict();

export const generateContentDraftsSchema = z.object({
  profiles: z.array(contentProfileKeySchema).min(1).max(12),
  locale: contentLocaleSchema,
}).strict();

export const updateContentDraftSchema = z.object({
  title: z.string().trim().max(300).optional(),
  excerpt: z.string().trim().max(500).optional(),
  bodyMarkdown: z.string().trim().max(150_000).optional(),
  status: z.enum(["draft", "approved"]).optional(),
}).strict().refine((value) => Object.keys(value).length > 0, "at least one draft field is required");

export const reviseContentDraftSchema = z.object({
  instruction: z.string().trim().min(3).max(4_000),
  scope: contentRevisionScopeSchema.default("draft"),
}).strict();

export const publishBlogPostSchema = z.object({
  slug: z.string().trim().regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/).max(120),
  authorName: z.string().trim().min(1).max(120).default("apiToken.sale Editorial"),
  seoTitle: z.string().trim().min(1).max(70),
  seoDescription: z.string().trim().min(1).max(170),
  relatedPaths: z.array(z.string().startsWith("/").max(300)).max(8).default([]),
}).strict();

export const recordExternalPublicationSchema = z.object({
  url: z.string().url().max(2_048),
}).strict();

export const platformProfileRulesSchema = z.object({
  language: contentLocaleSchema.optional(),
  tone: z.string().trim().min(1).max(500),
  audience: z.string().trim().min(1).max(500),
  length: z.string().trim().min(1).max(200),
  linkPolicy: z.string().trim().min(1).max(500),
  requiredDisclosure: z.string().trim().max(500).default(""),
  forbidden: z.array(z.string().trim().min(1).max(200)).max(20).default([]),
}).strict();

export const upsertPlatformProfileSchema = z.object({
  key: contentProfileKeySchema,
  name: z.string().trim().min(1).max(100),
  rules: platformProfileRulesSchema,
}).strict();

export type ContentLocale = z.infer<typeof contentLocaleSchema>;
export type ContentRevisionScope = z.infer<typeof contentRevisionScopeSchema>;
export type PlatformProfileRules = z.infer<typeof platformProfileRulesSchema>;
