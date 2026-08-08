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

const nullableStringSchema = z.string().nullish().transform((value) => value ?? null);
const nullableNonNegativeIntegerSchema = nonNegativeIntegerSchema.nullish()
  .transform((value) => value ?? null);

export const engineAccountFundingSchema = z.object({
  account_class: z.enum(["b2c", "b2b", "openkeys", "service"]).nullish()
    .transform((value) => value ?? null),
  funding_enforcement: z.enum(["legacy_single", "shadow", "strict"]).nullish()
    .transform((value) => value ?? null),
  reconciliation_state: z.enum(["pending", "verified", "exception"]).nullish()
    .transform((value) => value ?? null),
  bucket_count: nonNegativeIntegerSchema,
  paid_balance_nano: decimalIntegerSchema,
  bonus_balance_nano: decimalIntegerSchema,
  other_balance_nano: decimalIntegerSchema,
  unattributed_balance_nano: decimalIntegerSchema,
  paid_reserved_nano: decimalIntegerSchema,
  bonus_reserved_nano: decimalIntegerSchema,
  other_reserved_nano: decimalIntegerSchema,
  unattributed_reserved_nano: decimalIntegerSchema,
  paid_spent_nano: decimalIntegerSchema,
  bonus_spent_nano: decimalIntegerSchema,
  other_spent_nano: decimalIntegerSchema,
  unattributed_spent_nano: decimalIntegerSchema,
});

export type EngineAccountFunding = z.infer<typeof engineAccountFundingSchema>;

export const engineAccountSchema = z.object({
  account: z.string().startsWith("acct_"),
  balance_nano: decimalIntegerSchema,
  spent_nano: decimalIntegerSchema,
  reserved_nano: decimalIntegerSchema,
  balance: z.string(),
  mult_bp: z.number().int(),
  status: z.enum(["active", "disabled"]),
  handle: z.string().nullable(),
  // Expand compatibility: old engine slots omit this field. Callers must treat absence as unknown
  // instead of inferring a paid/bonus split.
  funding: engineAccountFundingSchema.nullable().optional(),
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

export const engineLedgerFundingAllocationSchema = z.object({
  bucket_id: z.string().min(1),
  source_type: z.string().min(1),
  source_ref: z.string(),
  bucket_version: nonNegativeIntegerSchema,
  direction: z.enum(["debit", "credit"]),
  amount_nano: nonNegativeIntegerSchema,
  allocation_order: nullableNonNegativeIntegerSchema,
});

export type EngineLedgerFundingAllocation = z.infer<typeof engineLedgerFundingAllocationSchema>;

export const engineSettlementFundingEvidenceSchema = z.object({
  bucket_id: z.string().min(1),
  source_type: z.string().min(1),
  bucket_version: nonNegativeIntegerSchema,
  reserved_nano: nonNegativeIntegerSchema,
  charged_nano: nonNegativeIntegerSchema,
  released_nano: nonNegativeIntegerSchema,
  allocation_order: nonNegativeIntegerSchema,
});

export type EngineSettlementFundingEvidence = z.infer<typeof engineSettlementFundingEvidenceSchema>;

// Funding evidence of a release-v2 (Stage 9) charge row. Mirrors the durable
// funding_ledger_allocations_v2 identity instead of the legacy bucket shape.
export const engineSettlementFundingEvidenceV2Schema = z.object({
  allocation_order: nonNegativeIntegerSchema,
  lot_id: z.string().min(1),
  lot_source_type: z.string().min(1),
  lot_version: nonNegativeIntegerSchema,
  direction: z.enum(["debit"]),
  amount_nano: nonNegativeIntegerSchema,
});

export type EngineSettlementFundingEvidenceV2 = z.infer<typeof engineSettlementFundingEvidenceV2Schema>;

export const engineLedgerAttributionSchema = z.object({
  attribution_schema_version: nonNegativeIntegerSchema,
  snapshot_kind: z.enum(["policy_v1", "legacy_scalar", "release_v2"]).nullish()
    .transform((value) => value ?? null),
  provider_id: nullableStringSchema,
  product_id: nullableStringSchema,
  account_class: z.enum(["b2c", "b2b", "openkeys", "service"]).nullish()
    .transform((value) => value ?? null),
  requested_model_id: nullableStringSchema,
  canonical_model_id: nullableStringSchema,
  served_model_id: nullableStringSchema,
  served_canonical_model_id: nullableStringSchema,
  billing_invariant_code: nullableStringSchema,
  alias_generation: nullableNonNegativeIntegerSchema,
  rule_id: nullableStringSchema,
  rule_digest: nullableStringSchema,
  rule_scope: z.enum(["global", "provider", "model"]).nullish().transform((value) => value ?? null),
  pricing_mode: z.enum(["track", "discount", "legacy_scalar"]).nullish()
    .transform((value) => value ?? null),
  rule_origin: z.enum(["managed", "legacy"]).nullish().transform((value) => value ?? null),
  discount_bps: z.number().int().min(0).max(9_500).nullish()
    .transform((value) => value ?? null),
  payable_multiplier_bp: z.number().int().min(0).max(10_000).nullish()
    .transform((value) => value ?? null),
  policy_id: nullableStringSchema,
  policy_version: nullableNonNegativeIntegerSchema,
  effective_policy_version: nullableNonNegativeIntegerSchema,
  policy_digest: nullableStringSchema,
  source_policy_digest: nullableStringSchema,
  catalog_generation: nullableNonNegativeIntegerSchema,
  switch_generation: nullableNonNegativeIntegerSchema,
  admission_catalog_generation: nullableNonNegativeIntegerSchema,
  admission_catalog_digest: nullableStringSchema,
  admission_switch_generation: nullableNonNegativeIntegerSchema,
  admission_switch_digest: nullableStringSchema,
  runtime_manifest_generation: nullableNonNegativeIntegerSchema,
  runtime_manifest_digest: nullableStringSchema,
  tariff_schedule_id: nullableStringSchema,
  tariff_priced_ts: nullableNonNegativeIntegerSchema,
  official_nano: nullableNonNegativeIntegerSchema,
  official_cost_json: z.record(z.string(), z.unknown()).nullish()
    .transform((value) => value ?? null),
  paid_funded_nano: nullableNonNegativeIntegerSchema,
  bonus_funded_nano: nullableNonNegativeIntegerSchema,
  other_funded_nano: nullableNonNegativeIntegerSchema,
  funding_allocation_json: z.array(
    z.union([engineSettlementFundingEvidenceSchema, engineSettlementFundingEvidenceV2Schema]),
  ).nullish()
    .transform((value) => value ?? null),
  track_eligible: z.boolean().nullish().transform((value) => value ?? null),
  retention_eligible: z.boolean().nullish().transform((value) => value ?? null),
  commission_eligible: z.boolean().nullish().transform((value) => value ?? null),
  snapshot_digest: nullableStringSchema,
  release_schema_version: nullableNonNegativeIntegerSchema,
  release_generation: nullableNonNegativeIntegerSchema,
  release_digest: nullableStringSchema,
  release_billing_mode: z.enum(["balance", "meter_only"]).nullish()
    .transform((value) => value ?? null),
  release_funding_generation: nullableNonNegativeIntegerSchema,
});

export type EngineLedgerAttribution = z.infer<typeof engineLedgerAttributionSchema>;

export const engineLedgerEntrySchema = z.object({
  id: nonNegativeIntegerSchema,
  kind: z.enum(["topup", "charge", "adjust"]),
  request_id: z.string().nullable().optional(),
  amount_nano: decimalIntegerSchema,
  amount: z.string(),
  key_masked: z.string().nullable(),
  ref: z.string().nullable(),
  balance_after_nano: decimalIntegerSchema.nullable(),
  ts: nonNegativeIntegerSchema,
  // Claude-модель за charge-строкой (topup/adjust → null). nullish — устойчивость к старому движку без поля.
  model: z.string().nullish(),
  provider: z.string().nullable().optional(),
  official_nano: nonNegativeIntegerSchema.nullable().optional(),
  attribution: engineLedgerAttributionSchema.nullable().optional(),
  funding_allocations: z.array(engineLedgerFundingAllocationSchema).optional(),
});

export type EngineLedgerEntry = z.infer<typeof engineLedgerEntrySchema>;

export const engineLedgerSchema = z.object({
  account: z.string().startsWith("acct_"),
  entries: z.array(engineLedgerEntrySchema),
});

// Разбивка расхода по токенам/моделям (`/admin/account/:id/usage`). Токены — числа (реалистично
// < 2^53); нанодоллары — decimal-строки (bigint-safe, деньги никогда не через JS number).
const usageBucketSchema = z.object({
  tokens: z.coerce.number().int().safe().nonnegative(),
  official_nano: decimalIntegerSchema,
});
const usageWebSearchBucketSchema = z.object({
  requests: z.coerce.number().int().safe().nonnegative(),
  official_nano: decimalIntegerSchema,
});
const usageMoneyOnlyBucketSchema = z.object({
  official_nano: decimalIntegerSchema,
});
export const engineUsageModelSchema = z.object({
  model: z.string(),
  // Free-form on purpose: the engine tags rows with its own provider ids
  // ("anthropic", "openai", "google" for Gemini traffic, more later) and an
  // exact enum here already took the usage endpoint down twice.
  provider: z.string().optional(),
  requests: z.coerce.number().int().safe().nonnegative(),
  input_tokens: z.coerce.number().int().safe().nonnegative(),
  output_tokens: z.coerce.number().int().safe().nonnegative(),
  cache_read_tokens: z.coerce.number().int().safe().nonnegative(),
  cache_write_5m_tokens: z.coerce.number().int().safe().nonnegative(),
  cache_write_1h_tokens: z.coerce.number().int().safe().nonnegative(),
  web_search_requests: z.coerce.number().int().safe().nonnegative(),
  official_nano: decimalIntegerSchema,
  charged_nano: decimalIntegerSchema,
});
export const engineUsageSchema = z.object({
  account: z.string().startsWith("acct_"),
  window: z.string(),
  since_ts: z.coerce.number().int().nonnegative(),
  until_ts: z.coerce.number().int().nonnegative(),
  requests: z.coerce.number().int().safe().nonnegative(),
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
    requests: z.coerce.number().int().safe().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })),
  daily_providers: z.array(z.object({
    day_ts: z.coerce.number().int().nonnegative(),
    // Free-form, same rationale as engineUsageModelSchema.provider.
    provider: z.string(),
    requests: z.coerce.number().int().safe().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })).default([]),
  keys: z.array(z.object({
    key_masked: z.string().nullable(),
    requests: z.coerce.number().int().safe().nonnegative(),
    official_nano: decimalIntegerSchema,
    charged_nano: decimalIntegerSchema,
  })),
});

export type EngineUsage = z.infer<typeof engineUsageSchema>;

// GET /spend-stats — операторская разбивка расхода ВСЕГО флота движка по окнам 24ч/7д/30д:
// суммы приходят в USD-числах (charge_usd — списано клиенту с его множителем, real_usd —
// эквивалент официального прайса провайдера), провайдер/модель — свободные строки по той же
// причине, что и в engineUsageModelSchema. accounts[] — top-50 аккаунтов окна, включая те,
// у которых нет commerce-юзера (OpenKeys, внутренние).
const spendStatsAccountSchema = z.object({
  account: z.string(),
  handle: z.string().nullable().default(null),
  requests: z.coerce.number().int().nonnegative().default(0),
  charge_usd: z.coerce.number().default(0),
  real_usd: z.coerce.number().default(0),
  last_ts: z.coerce.number().int().nonnegative().default(0),
});
const spendStatsProviderSchema = z.object({
  provider: z.string(),
  requests: z.coerce.number().int().nonnegative().default(0),
  charge_usd: z.coerce.number().default(0),
  real_usd: z.coerce.number().default(0),
});
const spendStatsModelSchema = spendStatsProviderSchema.extend({
  model: z.string(),
  provider: z.string().optional(),
});
export const engineSpendPeriodSchema = z.object({
  requests: z.coerce.number().int().nonnegative().default(0),
  charge_usd: z.coerce.number().default(0),
  real_usd: z.coerce.number().default(0),
  accounts: z.array(spendStatsAccountSchema).default([]),
  providers: z.array(spendStatsProviderSchema).default([]),
  models: z.array(spendStatsModelSchema).default([]),
});
export const engineSpendStatsSchema = z.object({
  now: z.coerce.number().int().nonnegative().default(0),
  periods: z.object({
    d1: engineSpendPeriodSchema,
    d7: engineSpendPeriodSchema,
    d30: engineSpendPeriodSchema,
  }),
});

export type EngineSpendPeriod = z.infer<typeof engineSpendPeriodSchema>;
export type EngineSpendStats = z.infer<typeof engineSpendStatsSchema>;
export type EngineSpendAccount = z.infer<typeof spendStatsAccountSchema>;
export type EngineSpendModel = z.infer<typeof spendStatsModelSchema>;
export type EngineSpendProvider = z.infer<typeof spendStatsProviderSchema>;

export const createEngineAccountSchema = z.object({
  handle: z.string().trim().min(1).max(200).optional(),
  multBp: z.number().int().min(0).max(100_000).optional(),
});

export type CreateEngineAccount = z.infer<typeof createEngineAccountSchema>;

const pricingIdentifierSchema = z.string().min(1).max(200)
  .refine((value) => value.trim() === value && !/[\u0000-\u001f\u007f]/.test(value),
    "pricing identifier contains unsupported whitespace or control characters");
const pricingVersionSchema = z.number().int().safe().positive();

export const pricingVersionTargetSchema = z.object({
  version: pricingVersionSchema,
  content_digest: pricingIdentifierSchema,
}).strict();
export type PricingVersionTarget = z.infer<typeof pricingVersionTargetSchema>;

export const pricingActiveExpectationSchema = z.union([
  z.literal("absent"),
  z.object({ exact: pricingVersionTargetSchema }).strict(),
]);
export type PricingActiveExpectation = z.infer<typeof pricingActiveExpectationSchema>;

export const pricingCatalogEntrySchema = z.object({
  provider_id: pricingIdentifierSchema,
  canonical_model_id: pricingIdentifierSchema,
  enabled: z.boolean(),
}).strict();

/**
 * Canonical first-generation product catalog shared by commerce backfill and
 * OpenKeys issuance. Keeping this identity at the transport-contract boundary
 * prevents a second application-local list from silently gaining or losing a
 * model. New models still require an explicit generation/code change.
 */
export const MULTI_DISCOUNT_SCHEMA_VERSION = 1;
export const MULTI_DISCOUNT_CAPABILITY_GENERATION = 1;
export const MULTI_DISCOUNT_CAPABILITY_DIGEST =
  "sha256:v1:88da6b622727dda8aac0e1cd1749524f4929f7738f097c2dd3b81ba1cc14e7fd";
export const MAIN_PRICING_PRODUCT_ID = "main";
export const OPENKEYS_PRICING_PRODUCT_ID = "openkeys";

export const CURRENT_ANTHROPIC_CANONICAL_MODELS = [
  "claude-haiku-4-5",
  "claude-opus-4-7",
  "claude-opus-4-8",
  "claude-sonnet-4-6",
  "claude-sonnet-5",
] as const;

export const CURRENT_OPENAI_CANONICAL_MODELS = [
  "gpt-5.4",
  "gpt-5.5",
  "gpt-5.6-luna",
  "gpt-5.6-sol",
  "gpt-5.6-terra",
] as const;

export const CURRENT_PRODUCT_CATALOG_ENTRIES = Object.freeze([
  ...CURRENT_ANTHROPIC_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "anthropic" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
  ...CURRENT_OPENAI_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "openai" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
]);

/**
 * Canonical second-generation product catalog: the frozen generation 1 above
 * plus `claude-opus-5` and `claude-fable-5` (provider `anthropic`). Generation 2
 * is additive only — the generation-1 constants stay byte-identical because the
 * active production authority still pins them, and the OpenAI model set does not
 * change. The capability digest is reproducible: `multi-discount-stage5` domain
 * SHA-256 over the canonical JSON of
 * `{generation: 2, schema_version: 1, entries, aliases}` where each entry carries
 * `entry_digest = stage5Digest("capability-entry", {provider_id, canonical_model_id,
 * enabled: true, capability_data: {pricing_supported: true}})` and the only alias
 * remains `gpt-5.6 -> gpt-5.6-sol`; the canonical digest primitive lives in
 * `packages/db/src/pricing-release-digest.ts` and is pinned by unit tests.
 * Generation 2 becomes live only when the commerce operator materializes it and
 * the pricing worker activates it in the engine authority; until then every
 * runtime check keeps validating against generation 1.
 */
export const MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION = 2;
export const MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST =
  "sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0";

export const MULTI_DISCOUNT_GEN2_ANTHROPIC_CANONICAL_MODELS = [
  "claude-fable-5",
  "claude-haiku-4-5",
  "claude-opus-4-7",
  "claude-opus-4-8",
  "claude-opus-5",
  "claude-sonnet-4-6",
  "claude-sonnet-5",
] as const;

export const MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN2_ANTHROPIC_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "anthropic" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
  ...CURRENT_OPENAI_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "openai" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
]);

/**
 * Canonical Gemini model identities pinned by the native implementation and
 * `crates/metering`. This is compile-time pricing/capability evidence, not a
 * production publication list: rejected dormant identities remain here when an
 * immutable capability generation already references them. The engine provider
 * id is `google`; Gemini is the public product/provider name.
 */
export const CURRENT_GEMINI_CANONICAL_MODELS = [
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-2.5-pro",
  "gemini-3-flash-preview",
  "gemini-3.1-flash-image",
  "gemini-3.1-flash-lite",
  "gemini-3.1-pro-preview",
  "gemini-3.5-flash",
  "gemini-3.6-flash",
] as const;

/**
 * Frozen Gemini identity set of capability generation 3. A newer catalogue must never mutate
 * this list because prepared policies and runtime manifests may still pin its digest.
 */
export const MULTI_DISCOUNT_GEN3_GEMINI_CANONICAL_MODELS = [
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-2.5-pro",
  "gemini-3.1-flash-image",
  "gemini-3.1-flash-lite",
  "gemini-3.1-pro-preview",
  "gemini-3.5-flash",
  "gemini-3.6-flash",
] as const;

export const MULTI_DISCOUNT_GEN3_CAPABILITY_GENERATION = 3;
export const MULTI_DISCOUNT_GEN3_CAPABILITY_DIGEST =
  "sha256:v1:e062a218571c1029490c8a28d2343f35aec0318a83a74d2244396b3e01f4fd83";

export const MULTI_DISCOUNT_GEN3_MAIN_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  ...MULTI_DISCOUNT_GEN3_GEMINI_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "google" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
]);

/**
 * Rejected dormant capability retained for immutable-history verification. Generation 4 extends
 * frozen generation 3 with Gemini 3 Flash Preview, but its live generation gate returned 404.
 * Never materialize or activate a catalog/release on this generation; a future successful model
 * admission must use a new additive generation. The exact entries and digest remain frozen.
 *
 * The digest is reproducible with the Stage 5 capability builder over
 * `{generation: 4, schema_version: 1, entries, aliases}`. Each entry carries
 * the usual `pricing_supported: true` capability data; the only alias remains
 * `gpt-5.6 -> gpt-5.6-sol`.
 */
export const MULTI_DISCOUNT_TARGET_CAPABILITY_GENERATION = 4;
export const MULTI_DISCOUNT_TARGET_CAPABILITY_DIGEST =
  "sha256:v1:10802bdb863c116518820df4f662b74d9a48d59db51dd1d2da2a1e8ff08dfab2";

export const MULTI_DISCOUNT_TARGET_MAIN_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  ...CURRENT_GEMINI_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "google" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
]);


/**
 * Admitted publication capability for Gemini 3 Flash Preview. Generation 4 remains an immutable
 * rejected artifact; generation 5 repeats the exact reviewed model set under a new identity only
 * after the complete Pro+Ultra live matrix passed on the implementation SHA. Keeping a separate
 * frozen list prevents a later change to CURRENT_GEMINI_CANONICAL_MODELS from rewriting this
 * capability generation.
 */
export const MULTI_DISCOUNT_GEN5_GEMINI_CANONICAL_MODELS = [
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-2.5-pro",
  "gemini-3-flash-preview",
  "gemini-3.1-flash-image",
  "gemini-3.1-flash-lite",
  "gemini-3.1-pro-preview",
  "gemini-3.5-flash",
  "gemini-3.6-flash",
] as const;

export const MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION = 5;
export const MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST =
  "sha256:v1:f4f69a2032497741a6c5b1c60e14974ddbc7b0f5992e03516f720daa6492f185";

export const MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  ...MULTI_DISCOUNT_GEN5_GEMINI_CANONICAL_MODELS.map((canonicalModelId) => ({
    provider_id: "google" as const,
    canonical_model_id: canonicalModelId,
    enabled: true as const,
  })),
]);

/** OpenKeys remains explicit in generation 5 and does not gain Gemini models. */
export const MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES =
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES;

/**
 * Additive GPT Image 2 pricing capability. Generation 5 remains byte-identical because the active
 * production release may still pin it; generation 6 adds only the immutable image snapshot after
 * public generation and edit both passed the production gate through the sealed OpenAI OAuth pool.
 */
export const MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION = 6;
export const MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST =
  "sha256:v1:1652b971ce678094d5717173ed97b32d942d8507b183688cce613b6883e3c7d9";
export const GPT_IMAGE_2_CANONICAL_MODEL = "gpt-image-2-2026-04-21";

const MULTI_DISCOUNT_GEN6_IMAGE_CATALOG_ENTRY = Object.freeze({
  provider_id: "openai" as const,
  canonical_model_id: GPT_IMAGE_2_CANONICAL_MODEL,
  enabled: true as const,
});

export const MULTI_DISCOUNT_GEN6_MAIN_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_IMAGE_CATALOG_ENTRY,
]);

export const MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_IMAGE_CATALOG_ENTRY,
]);

/**
 * Additive pricing capability generation 7. Generation 6 remains
 * byte-identical because the active production release may still pin it; generation
 * 7 adds only: claude-opus-4-6, claude-opus-4-5, claude-sonnet-4-5, plus capability aliases
 * mapping the dated 4.x snapshots (opus-4-5-20251101, sonnet-4-5-20250929, haiku-4-5-20251001)
 * onto their tariffed canonical identities.
 */
export const MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION = 7;
export const MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST =
  "sha256:v1:7e68adf53ba505f9d3f9cf80f859092a6f01fe2d8bcc26ff12b54613bdde6056";
export const CLAUDE_OPUS_4_6_CANONICAL_MODEL = "claude-opus-4-6";
export const CLAUDE_OPUS_4_5_CANONICAL_MODEL = "claude-opus-4-5";
export const CLAUDE_SONNET_4_5_CANONICAL_MODEL = "claude-sonnet-4-5";

const MULTI_DISCOUNT_GEN7_OPUS_4_6_CATALOG_ENTRY = Object.freeze({
  provider_id: "anthropic" as const,
  canonical_model_id: CLAUDE_OPUS_4_6_CANONICAL_MODEL,
  enabled: true as const,
});

const MULTI_DISCOUNT_GEN7_OPUS_4_5_CATALOG_ENTRY = Object.freeze({
  provider_id: "anthropic" as const,
  canonical_model_id: CLAUDE_OPUS_4_5_CANONICAL_MODEL,
  enabled: true as const,
});

const MULTI_DISCOUNT_GEN7_SONNET_4_5_CATALOG_ENTRY = Object.freeze({
  provider_id: "anthropic" as const,
  canonical_model_id: CLAUDE_SONNET_4_5_CANONICAL_MODEL,
  enabled: true as const,
});

export const MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN6_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN7_OPUS_4_6_CATALOG_ENTRY,
  MULTI_DISCOUNT_GEN7_OPUS_4_5_CATALOG_ENTRY,
  MULTI_DISCOUNT_GEN7_SONNET_4_5_CATALOG_ENTRY,
]);

export const MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES = Object.freeze([
  ...MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN7_OPUS_4_6_CATALOG_ENTRY,
  MULTI_DISCOUNT_GEN7_OPUS_4_5_CATALOG_ENTRY,
  MULTI_DISCOUNT_GEN7_SONNET_4_5_CATALOG_ENTRY,
]);

/**
 * Buyer/operator-facing supported-model list for OpenKeys issuance. Keep public display on the
 * generation-5 main set until generation 6 is activated and paid customer generation/edit pass;
 * publication then advances this authority in its own checkpoint.
 */
export const MULTI_DISCOUNT_TARGET_OPENKEYS_CATALOG_ENTRIES =
  MULTI_DISCOUNT_GEN5_MAIN_CATALOG_ENTRIES;

export const pricingCatalogSpecSchema = z.object({
  product_id: pricingIdentifierSchema,
  generation: pricingVersionSchema,
  schema_version: pricingVersionSchema,
  capability_generation: pricingVersionSchema,
  capability_digest: pricingIdentifierSchema,
  content_digest: pricingIdentifierSchema,
  entries: z.array(pricingCatalogEntrySchema),
}).strict();
export type PricingCatalogSpec = z.infer<typeof pricingCatalogSpecSchema>;

export const providerSwitchScopeSchema = z.union([
  z.literal("master"),
  z.object({
    product: z.object({ product_id: pricingIdentifierSchema }).strict(),
  }).strict(),
  z.object({
    segment: z.object({
      product_id: pricingIdentifierSchema,
      segment: z.enum(["b2c", "b2b"]),
    }).strict(),
  }).strict(),
]);

export const providerSwitchEntrySchema = z.object({
  provider_id: pricingIdentifierSchema,
  scope: providerSwitchScopeSchema,
  catalog_generation: pricingVersionSchema.nullable(),
  enabled: z.boolean(),
}).strict();

export const providerSwitchSpecSchema = z.object({
  generation: pricingVersionSchema,
  schema_version: pricingVersionSchema,
  capability_generation: pricingVersionSchema,
  capability_digest: pricingIdentifierSchema,
  content_digest: pricingIdentifierSchema,
  entries: z.array(providerSwitchEntrySchema),
}).strict();
export type ProviderSwitchSpec = z.infer<typeof providerSwitchSpecSchema>;

export const accountPolicyBindingSchema = z.object({
  policy_enforcement: z.enum(["legacy_scalar", "shadow", "strict"]),
  funding_enforcement: z.enum(["legacy_single", "shadow", "strict"]),
  reconciliation_state: z.enum(["pending", "verified", "exception"]),
}).strict();
export type AccountPolicyBinding = z.infer<typeof accountPolicyBindingSchema>;

export const accountPolicyRuleSchema = z.object({
  rule_id: pricingIdentifierSchema,
  rule_digest: pricingIdentifierSchema,
  scope: z.union([
    z.object({
      provider: z.object({ provider_id: pricingIdentifierSchema }).strict(),
    }).strict(),
    z.object({
      model: z.object({
        provider_id: pricingIdentifierSchema,
        canonical_model_id: pricingIdentifierSchema,
      }).strict(),
    }).strict(),
  ]),
  pricing_mode: z.enum(["track", "discount"]),
  rule_origin: z.enum(["managed", "legacy"]),
  discount_bps: z.number().int().min(0).max(10_000).nullable(),
  payable_multiplier_bp: z.number().int().min(0).max(10_000),
  track_eligible: z.boolean(),
  retention_eligible: z.boolean(),
  commission_eligible: z.boolean(),
}).strict();

export const accountPolicySpecSchema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  effective_version: pricingVersionSchema,
  policy_id: pricingIdentifierSchema,
  policy_version: pricingVersionSchema,
  source_policy_digest: pricingIdentifierSchema,
  owner_type: z.enum(["global_b2c", "b2b_client", "open_keys", "service"]),
  owner_id: pricingIdentifierSchema,
  account_class: z.enum(["b2c", "b2b", "open_keys", "service"]),
  product_id: pricingIdentifierSchema,
  schema_version: pricingVersionSchema,
  catalog_generation: pricingVersionSchema,
  switch_generation: pricingVersionSchema,
  content_digest: pricingIdentifierSchema,
  replacement_locked: z.boolean(),
  rules: z.array(accountPolicyRuleSchema),
}).strict();
export type AccountPolicySpec = z.infer<typeof accountPolicySpecSchema>;

export const activePolicyTargetSchema = z.object({
  target: pricingVersionTargetSchema,
  binding: accountPolicyBindingSchema,
}).strict();

export const policyActiveExpectationSchema = z.union([
  z.literal("unbound"),
  z.object({ inactive: accountPolicyBindingSchema }).strict(),
  z.object({ exact: activePolicyTargetSchema }).strict(),
]);
export type PolicyActiveExpectation = z.infer<typeof policyActiveExpectationSchema>;

export const pricingPolicySnapshotSchema = z.union([
  z.literal("unbound"),
  z.object({
    inactive: z.object({
      product_id: pricingIdentifierSchema,
      account_class: z.enum(["b2c", "b2b", "open_keys", "service"]),
      binding: accountPolicyBindingSchema,
    }).strict(),
  }).strict(),
  z.object({
    active: z.object({
      policy: accountPolicySpecSchema,
      binding: accountPolicyBindingSchema,
    }).strict(),
  }).strict(),
]);
export type PricingPolicySnapshot = z.infer<typeof pricingPolicySnapshotSchema>;

export const pricingRejectionSchema = z.union([
  z.object({ invalid: z.object({ reason: z.string() }).strict() }).strict(),
  z.object({ missing_dependency: z.object({ dependency: z.string() }).strict() }).strict(),
  z.object({ stale: z.object({ actual: pricingVersionTargetSchema.nullable() }).strict() }).strict(),
  z.literal("version_conflict"),
  z.object({ cas_mismatch: z.object({ actual: pricingVersionTargetSchema.nullable() }).strict() }).strict(),
  z.object({
    policy_cas_mismatch: z.object({ actual: z.union([
      z.literal("unbound"),
      z.object({ inactive: accountPolicyBindingSchema }).strict(),
      z.object({ active: activePolicyTargetSchema }).strict(),
    ]) }).strict(),
  }).strict(),
  z.literal("locked"),
]);

const pricingMutationSuccessAckSchema = z.object({
  result: z.enum(["stored", "applied", "unchanged"]),
  identity: z.unknown(),
}).strict();

const pricingMutationRejectedAckSchema = z.discriminatedUnion("code", [
  z.object({
    result: z.literal("rejected"),
    code: z.literal("invalid"),
    identity: z.unknown(),
    rejection: z.object({ invalid: z.object({ reason: z.string() }).strict() }).strict(),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("missing_dependency"),
    identity: z.unknown(),
    rejection: z.object({
      missing_dependency: z.object({ dependency: z.string() }).strict(),
    }).strict(),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("stale"),
    identity: z.unknown(),
    rejection: z.object({ stale: z.object({
      actual: pricingVersionTargetSchema.nullable(),
    }).strict() }).strict(),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("version_conflict"),
    identity: z.unknown(),
    rejection: z.literal("version_conflict"),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("cas_mismatch"),
    identity: z.unknown(),
    rejection: z.object({ cas_mismatch: z.object({
      actual: pricingVersionTargetSchema.nullable(),
    }).strict() }).strict(),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("policy_cas_mismatch"),
    identity: z.unknown(),
    rejection: z.object({ policy_cas_mismatch: z.object({ actual: z.union([
      z.literal("unbound"),
      z.object({ inactive: accountPolicyBindingSchema }).strict(),
      z.object({ active: activePolicyTargetSchema }).strict(),
    ]) }).strict() }).strict(),
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("locked"),
    identity: z.unknown(),
    rejection: z.literal("locked"),
  }).strict(),
]);

export const pricingMutationAckSchema = z.union([
  pricingMutationSuccessAckSchema,
  pricingMutationRejectedAckSchema,
]);
export type PricingMutationAck = z.infer<typeof pricingMutationAckSchema>;

/**
 * Replacement-locked legacy OpenKeys transition. The request names the complete managed
 * provider-only 1:1 successor policy and the exact current replacement-locked legacy target;
 * the ACK identity echoes the successor, the fixed shadow+legacy_single+verified active target
 * and the expected legacy active target. Contract: docs/engine/CONTROL_API.md
 * «Replacement-locked legacy OpenKeys transition».
 */
export const lockedOpenkeysPolicyTransitionRequestSchema = z.object({
  policy: accountPolicySpecSchema,
  expected_active: activePolicyTargetSchema,
}).strict();
export type LockedOpenkeysPolicyTransitionRequest =
  z.infer<typeof lockedOpenkeysPolicyTransitionRequestSchema>;

export const lockedOpenkeysPolicyTransitionBindingSchema = z.object({
  policy_enforcement: z.literal("shadow"),
  funding_enforcement: z.literal("legacy_single"),
  reconciliation_state: z.literal("verified"),
}).strict();

export const lockedOpenkeysPolicyTransitionIdentitySchema = z.object({
  policy: accountPolicySpecSchema,
  active: z.object({
    target: pricingVersionTargetSchema,
    binding: lockedOpenkeysPolicyTransitionBindingSchema,
  }).strict(),
  expected_active: activePolicyTargetSchema,
}).strict();
export type LockedOpenkeysPolicyTransitionIdentity =
  z.infer<typeof lockedOpenkeysPolicyTransitionIdentitySchema>;

/** Additive prepare/read/activation contract for the pricing-release v2 authority. */
export const PRICING_RELEASE_SCHEMA_VERSION_V2 = 2 as const;
export const canonicalSha256V2Schema = z.string().regex(/^sha256:v2:[0-9a-f]{64}$/);
export const pricingReleaseBillingModeV2Schema = z.enum(["balance", "meter_only"]);
export const pricingReleaseKindV2Schema = z.enum(["target", "recovery"]);
export const pricingReleaseAccountClassV2Schema = z.enum(["b2c", "b2b", "open_keys", "service"]);
export const pricingReleasePolicyOwnerTypeV2Schema = z.enum([
  "global_b2c",
  "b2b_client",
  "open_keys",
  "service",
]);

export const pricingReleaseRuleScopeV2Schema = z.discriminatedUnion("scope", [
  z.object({ scope: z.literal("global") }).strict(),
  z.object({
    scope: z.literal("provider"),
    provider_id: pricingIdentifierSchema,
  }).strict(),
  z.object({
    scope: z.literal("model"),
    provider_id: pricingIdentifierSchema,
    canonical_model_id: pricingIdentifierSchema,
  }).strict(),
]);
export type PricingReleaseRuleScopeV2 = z.infer<typeof pricingReleaseRuleScopeV2Schema>;

export const pricingReleasePolicyRuleV2Schema = z.object({
  rule_id: pricingIdentifierSchema,
  rule_digest: pricingIdentifierSchema,
  scope: pricingReleaseRuleScopeV2Schema,
  discount_bps: z.number().int().min(0).max(10_000),
  payable_multiplier_bp: z.number().int().min(0).max(10_000),
}).strict();
export type PricingReleasePolicyRuleV2 = z.infer<typeof pricingReleasePolicyRuleV2Schema>;

export const pricingReleasePolicyV2Schema = z.object({
  policy_id: pricingIdentifierSchema,
  policy_version: pricingVersionSchema,
  owner_type: pricingReleasePolicyOwnerTypeV2Schema,
  owner_id: pricingIdentifierSchema,
  account_class: pricingReleaseAccountClassV2Schema,
  product_id: pricingIdentifierSchema.nullable(),
  billing_mode: pricingReleaseBillingModeV2Schema,
  schema_version: z.literal(PRICING_RELEASE_SCHEMA_VERSION_V2),
  capability_generation: pricingVersionSchema,
  capability_digest: pricingIdentifierSchema,
  catalog_generation: pricingVersionSchema.nullable(),
  catalog_digest: pricingIdentifierSchema.nullable(),
  switch_generation: pricingVersionSchema.nullable(),
  switch_digest: pricingIdentifierSchema.nullable(),
  content_digest: pricingIdentifierSchema,
  rules: z.array(pricingReleasePolicyRuleV2Schema),
}).strict();
export type PricingReleasePolicyV2 = z.infer<typeof pricingReleasePolicyV2Schema>;

export const pricingReleaseAssignmentV2Schema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  account_class: pricingReleaseAccountClassV2Schema,
  policy_id: pricingIdentifierSchema,
  policy_version: pricingVersionSchema,
  policy_digest: pricingIdentifierSchema,
  billing_mode: pricingReleaseBillingModeV2Schema,
  funding_generation: pricingVersionSchema.nullable(),
  purpose: pricingIdentifierSchema.nullable(),
  responsible: pricingIdentifierSchema.nullable(),
  assignment_digest: pricingIdentifierSchema,
}).strict();
export type PricingReleaseAssignmentV2 = z.infer<typeof pricingReleaseAssignmentV2Schema>;

export const pricingReleaseAssignmentExtensionMemberV2Schema = z.object({
  release_generation: pricingVersionSchema,
  assignment: pricingReleaseAssignmentV2Schema,
  extension_digest: pricingIdentifierSchema,
}).strict();
export type PricingReleaseAssignmentExtensionMemberV2 =
  z.infer<typeof pricingReleaseAssignmentExtensionMemberV2Schema>;

export const pricingReleaseAssignmentExtensionV2Schema = z.object({
  provisioning_head_generation: pricingVersionSchema,
  provisioning_head_digest: pricingIdentifierSchema,
  provisioning_head_version: pricingVersionSchema,
  paired_recovery_generation: pricingVersionSchema.nullable(),
  paired_recovery_digest: pricingIdentifierSchema.nullable(),
  extension_group_digest: pricingIdentifierSchema,
  members: z.array(pricingReleaseAssignmentExtensionMemberV2Schema).min(1).max(2),
}).strict().superRefine((extension, context) => {
  const paired = extension.paired_recovery_generation !== null;
  if (paired !== (extension.paired_recovery_digest !== null)) {
    context.addIssue({
      code: "custom",
      message: "paired recovery generation and digest must both be null or both be set",
    });
  }
  if (extension.paired_recovery_generation !== null
      && extension.paired_recovery_generation <= extension.provisioning_head_generation) {
    context.addIssue({ code: "custom", message: "paired recovery generation must be newer than the active head" });
  }
  if (extension.members.length !== (paired ? 2 : 1)) {
    context.addIssue({ code: "custom", message: "members must contain the exact active/recovery pair" });
  }

  const expectedGenerations = new Set([extension.provisioning_head_generation]);
  if (extension.paired_recovery_generation !== null) {
    expectedGenerations.add(extension.paired_recovery_generation);
  }
  const actualGenerations = new Set(extension.members.map((member) => member.release_generation));
  if (actualGenerations.size !== extension.members.length
      || actualGenerations.size !== expectedGenerations.size
      || [...expectedGenerations].some((generation) => !actualGenerations.has(generation))) {
    context.addIssue({ code: "custom", message: "members do not cover the exact active/recovery generations" });
  }
  if (new Set(extension.members.map((member) => member.extension_digest)).size !== extension.members.length) {
    context.addIssue({ code: "custom", message: "member extension digests must be unique" });
  }

  const first = extension.members[0]?.assignment;
  if (first) {
    const semantics = (assignment: PricingReleaseAssignmentV2): string => JSON.stringify({
      account_id: assignment.account_id,
      account_class: assignment.account_class,
      policy_id: assignment.policy_id,
      policy_version: assignment.policy_version,
      policy_digest: assignment.policy_digest,
      billing_mode: assignment.billing_mode,
      funding_generation: assignment.funding_generation,
      purpose: assignment.purpose,
      responsible: assignment.responsible,
    });
    if (extension.members.some((member) => semantics(member.assignment) !== semantics(first))) {
      context.addIssue({ code: "custom", message: "active/recovery members must have identical assignment semantics" });
    }
  }
  for (const member of extension.members) {
    const assignment = member.assignment;
    const service = assignment.account_class === "service";
    const validService = service
      && assignment.billing_mode === "meter_only"
      && assignment.funding_generation === null
      && assignment.purpose !== null
      && assignment.responsible !== null;
    const validCustomer = !service
      && assignment.billing_mode === "balance"
      && assignment.funding_generation !== null
      && assignment.purpose === null
      && assignment.responsible === null;
    if (!validService && !validCustomer) {
      context.addIssue({
        code: "custom",
        message: "service assignments must be meter_only; customer assignments require one funding generation",
      });
    }
  }
});
export type PricingReleaseAssignmentExtensionV2 =
  z.infer<typeof pricingReleaseAssignmentExtensionV2Schema>;

export const pricingReleaseAssignmentExtensionIdentityV2Schema = z.object({
  provisioning_head_generation: pricingVersionSchema,
  provisioning_head_version: pricingVersionSchema,
  account_id: z.string().startsWith("acct_").max(200),
  extension_group_digest: pricingIdentifierSchema,
}).strict();
export type PricingReleaseAssignmentExtensionIdentityV2 =
  z.infer<typeof pricingReleaseAssignmentExtensionIdentityV2Schema>;

export const pricingReleaseV2Schema = z.object({
  generation: pricingVersionSchema,
  release_kind: pricingReleaseKindV2Schema,
  schema_version: z.literal(PRICING_RELEASE_SCHEMA_VERSION_V2),
  capability_generation: pricingVersionSchema,
  capability_digest: pricingIdentifierSchema,
  main_catalog_generation: pricingVersionSchema,
  main_catalog_digest: pricingIdentifierSchema,
  openkeys_catalog_generation: pricingVersionSchema,
  openkeys_catalog_digest: pricingIdentifierSchema,
  switch_generation: pricingVersionSchema,
  switch_digest: pricingIdentifierSchema,
  inventory_digest: pricingIdentifierSchema,
  policy_manifest_digest: pricingIdentifierSchema,
  assignment_manifest_digest: pricingIdentifierSchema,
  funding_manifest_digest: pricingIdentifierSchema,
  minimum_runtime_schema_version: pricingVersionSchema,
  content_digest: pricingIdentifierSchema,
  assignments: z.array(pricingReleaseAssignmentV2Schema),
}).strict();
export type PricingReleaseV2 = z.infer<typeof pricingReleaseV2Schema>;

export const pricingReleaseRecoveryLinkV2Schema = z.object({
  target_generation: pricingVersionSchema,
  target_digest: pricingIdentifierSchema,
  recovery_generation: pricingVersionSchema,
  recovery_digest: pricingIdentifierSchema,
  link_digest: pricingIdentifierSchema,
}).strict();
export type PricingReleaseRecoveryLinkV2 = z.infer<typeof pricingReleaseRecoveryLinkV2Schema>;

export const pricingReleaseHeadV2Schema = z.object({
  active_generation: pricingVersionSchema,
  active_digest: pricingIdentifierSchema,
  head_version: pricingVersionSchema,
  updated_ts: z.number().int().safe().nonnegative(),
}).strict();
export type PricingReleaseHeadV2 = z.infer<typeof pricingReleaseHeadV2Schema>;

export const pricingReleaseActivationKindV2Schema = z.enum(["cutover", "recovery", "successor"]);

export const pricingReleaseProvisioningReleaseV2Schema = z.object({
  generation: pricingVersionSchema,
  release_kind: pricingReleaseKindV2Schema,
  schema_version: z.literal(PRICING_RELEASE_SCHEMA_VERSION_V2),
  capability_generation: pricingVersionSchema,
  capability_digest: pricingReleaseV2Schema.shape.capability_digest,
  main_catalog_generation: pricingVersionSchema,
  main_catalog_digest: pricingReleaseV2Schema.shape.main_catalog_digest,
  openkeys_catalog_generation: pricingVersionSchema,
  openkeys_catalog_digest: pricingReleaseV2Schema.shape.openkeys_catalog_digest,
  switch_generation: pricingVersionSchema,
  switch_digest: pricingReleaseV2Schema.shape.switch_digest,
  inventory_digest: canonicalSha256V2Schema,
  funding_manifest_digest: canonicalSha256V2Schema,
  minimum_runtime_schema_version: pricingVersionSchema,
  content_digest: canonicalSha256V2Schema,
}).strict();
export type PricingReleaseProvisioningReleaseV2 =
  z.infer<typeof pricingReleaseProvisioningReleaseV2Schema>;

export const pricingReleaseProvisioningActivationV2Schema = z.object({
  activation_id: nonNegativeIntegerSchema.refine((value) => BigInt(value) > 0n, "activation_id must be positive"),
  activation_kind: pricingReleaseActivationKindV2Schema,
  evidence_digest: canonicalSha256V2Schema,
  activated_ts: z.number().int().safe().positive(),
}).strict();
export type PricingReleaseProvisioningActivationV2 =
  z.infer<typeof pricingReleaseProvisioningActivationV2Schema>;

export const pricingReleaseProvisioningRecoveryV2Schema = z.object({
  release: pricingReleaseProvisioningReleaseV2Schema,
  recovery_link: pricingReleaseRecoveryLinkV2Schema,
}).strict();
export type PricingReleaseProvisioningRecoveryV2 =
  z.infer<typeof pricingReleaseProvisioningRecoveryV2Schema>;

export const pricingReleaseProvisioningContextV2Schema = z.object({
  head: pricingReleaseHeadV2Schema,
  activation: pricingReleaseProvisioningActivationV2Schema,
  active_release: pricingReleaseProvisioningReleaseV2Schema,
  paired_recovery: pricingReleaseProvisioningRecoveryV2Schema.nullable(),
}).strict().superRefine((context, refinement) => {
  const active = context.active_release;
  if (context.head.active_generation !== active.generation
      || context.head.active_digest !== active.content_digest) {
    refinement.addIssue({
      code: z.ZodIssueCode.custom,
      message: "active release must match the exact provisioning head",
    });
  }
  const expectedKind = context.activation.activation_kind === "recovery" ? "recovery" : "target";
  if (active.release_kind !== expectedKind) {
    refinement.addIssue({
      code: z.ZodIssueCode.custom,
      message: "active release kind must match the activation kind",
    });
  }
  if (context.activation.activation_kind === "recovery") {
    if (context.paired_recovery !== null) {
      refinement.addIssue({
        code: z.ZodIssueCode.custom,
        message: "an active recovery cannot carry a further confirmed recovery pair",
      });
    }
    return;
  }
  const paired = context.paired_recovery;
  if (paired === null) {
    refinement.addIssue({
      code: z.ZodIssueCode.custom,
      message: "an active target requires the evidence-selected recovery pair",
    });
    return;
  }
  const link = paired.recovery_link;
  const sameRuntimeLineage = paired.release.schema_version === active.schema_version
    && paired.release.capability_generation === active.capability_generation
    && paired.release.capability_digest === active.capability_digest
    && paired.release.main_catalog_generation === active.main_catalog_generation
    && paired.release.main_catalog_digest === active.main_catalog_digest
    && paired.release.openkeys_catalog_generation === active.openkeys_catalog_generation
    && paired.release.openkeys_catalog_digest === active.openkeys_catalog_digest
    && paired.release.switch_generation === active.switch_generation
    && paired.release.switch_digest === active.switch_digest
    && paired.release.inventory_digest === active.inventory_digest
    && paired.release.funding_manifest_digest === active.funding_manifest_digest
    && paired.release.minimum_runtime_schema_version === active.minimum_runtime_schema_version;
  if (paired.release.release_kind !== "recovery"
      || paired.release.generation <= active.generation
      || !sameRuntimeLineage
      || link.target_generation !== active.generation
      || link.target_digest !== active.content_digest
      || link.recovery_generation !== paired.release.generation
      || link.recovery_digest !== paired.release.content_digest) {
    refinement.addIssue({
      code: z.ZodIssueCode.custom,
      message: "paired recovery must match the exact active target lineage",
    });
  }
});
export type PricingReleaseProvisioningContextV2 =
  z.infer<typeof pricingReleaseProvisioningContextV2Schema>;

export const pricingReleaseProvisioningContextEnvelopeV2Schema = z.object({
  context: pricingReleaseProvisioningContextV2Schema.nullable(),
}).strict();
export type PricingReleaseProvisioningContextEnvelopeV2 =
  z.infer<typeof pricingReleaseProvisioningContextEnvelopeV2Schema>;

export const pricingReleaseHeadExpectationV2Schema = z.union([
  z.literal("absent"),
  z.object({ exact: pricingReleaseHeadV2Schema }).strict(),
]);
export type PricingReleaseHeadExpectationV2 =
  z.infer<typeof pricingReleaseHeadExpectationV2Schema>;

const pricingReleaseActivationTimestampV2Schema = z.number().int().safe().positive();
export const pricingReleaseActivationOperatorV2Schema = z.string().min(1).max(200)
  .refine((value) => !/[\u0000-\u001f\u007f-\u009f]/u.test(value), "operator_id contains control characters");
export const pricingReleaseActivationReasonV2Schema = z.string().min(1).max(2_000)
  .refine((value) => !/[\u0000-\u001f\u007f-\u009f]/u.test(value), "reason contains control characters");

/** Protected commerce operator contract for the pre-activation Stage 5/6 control-plane. */
export const pricingStageControlMutationReasonV2Schema = z.string().trim().min(3).max(2_000)
  .refine((value) => !/[\u0000-\u001f\u007f-\u009f]/u.test(value), "reason contains control characters");

export const pricingStage5DryRunRequestV2Schema = z.object({}).strict();
export type PricingStage5DryRunRequestV2 = z.infer<typeof pricingStage5DryRunRequestV2Schema>;

export const pricingStage5MaterializeRequestV2Schema = z.object({
  plan_digest: canonicalSha256V2Schema,
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingStage5MaterializeRequestV2 =
  z.infer<typeof pricingStage5MaterializeRequestV2Schema>;

export const pricingStage5BlockerV2Schema = z.object({
  blocker_code: z.string().min(1).max(200),
  blocker_context: z.enum(["commerce", "engine", "openkeys", "service", "funding", "release"]),
  subject_id: z.string().min(1).max(500),
  detail: z.string().min(1).max(2_000),
  blocker_digest: canonicalSha256V2Schema,
}).strict();
export type PricingStage5BlockerV2 = z.infer<typeof pricingStage5BlockerV2Schema>;

export const pricingStage5ControlResultV2Schema = z.object({
  mode: z.enum(["dry_run", "apply"]),
  status: z.enum(["dry_run", "blocked", "planned", "materializing"]),
  plan_digest: canonicalSha256V2Schema,
  run_id: z.string().uuid().nullable(),
  writes_committed: z.boolean(),
  engine_prepared: z.boolean(),
  commerce_inventory_digest: canonicalSha256V2Schema,
  engine_scan_first_digest: canonicalSha256V2Schema,
  engine_scan_second_digest: canonicalSha256V2Schema,
  openkeys_scan_first_digest: canonicalSha256V2Schema,
  openkeys_scan_second_digest: canonicalSha256V2Schema,
  service_inventory_digest: canonicalSha256V2Schema,
  funding_plan_digest: canonicalSha256V2Schema,
  target_generation: z.number().int().safe().positive(),
  target_plan_digest: canonicalSha256V2Schema,
  recovery_generation: z.number().int().safe().positive(),
  recovery_plan_digest: canonicalSha256V2Schema,
  blocker_count: z.number().int().safe().nonnegative(),
  blockers: z.array(pricingStage5BlockerV2Schema),
}).strict();
export type PricingStage5ControlResultV2 = z.infer<typeof pricingStage5ControlResultV2Schema>;

export const pricingStage5RunQueryV2Schema = z.object({
  plan_digest: canonicalSha256V2Schema,
}).strict();
export type PricingStage5RunQueryV2 = z.infer<typeof pricingStage5RunQueryV2Schema>;

const pricingStage5RunPositiveIntegerV2Schema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) > 0n,
  "value must be positive",
);
export const pricingStage5RunV2Schema = z.object({
  run_id: z.string().uuid(),
  plan_digest: canonicalSha256V2Schema,
  status: z.enum(["blocked", "planned", "materializing", "prepared", "failed"]),
  target_generation: pricingStage5RunPositiveIntegerV2Schema,
  target_plan_digest: canonicalSha256V2Schema,
  target_release_digest: canonicalSha256V2Schema.nullable(),
  recovery_generation: pricingStage5RunPositiveIntegerV2Schema,
  recovery_plan_digest: canonicalSha256V2Schema,
  recovery_release_digest: canonicalSha256V2Schema.nullable(),
  blocker_count: nonNegativeIntegerSchema,
}).strict();
export type PricingStage5RunV2 = z.infer<typeof pricingStage5RunV2Schema>;

export const pricingStage6PlanQueryV2Schema = z.object({
  plan_digest: canonicalSha256V2Schema,
}).strict();
export type PricingStage6PlanQueryV2 = z.infer<typeof pricingStage6PlanQueryV2Schema>;

export const pricingStage6StageRequestV2Schema = pricingStage6PlanQueryV2Schema.extend({
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingStage6StageRequestV2 = z.infer<typeof pricingStage6StageRequestV2Schema>;

const legacyPricingControlDigestV1Schema = z.string().regex(/^sha256:v1:[0-9a-f]{64}$/);

/**
 * Narrow operator repair for a dead pre-cutover policy delivery that was durably built with the
 * historical invalid strict-policy + legacy-funding binding. The exact terminal job is the CAS
 * source; the producer creates a newer immutable effective policy instead of rewriting history.
 */
export const pricingPolicyDeliveryRepairRequestV2Schema = z.object({
  job_id: z.string().uuid(),
  expected_effective_version: z.number().int().safe().positive(),
  expected_content_digest: legacyPricingControlDigestV1Schema,
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingPolicyDeliveryRepairRequestV2 =
  z.infer<typeof pricingPolicyDeliveryRepairRequestV2Schema>;

export const pricingPolicyDeliveryRepairResponseV2Schema = z.object({
  status: z.enum(["queued", "unchanged"]),
  superseded_job_id: z.string().uuid(),
  replacement_job_id: z.string().uuid(),
  binding_id: z.string().uuid(),
  engine_account_id: z.string().startsWith("acct_").max(200),
  previous_effective_version: z.number().int().safe().positive(),
  replacement_effective_version: z.number().int().safe().positive(),
  replacement_content_digest: legacyPricingControlDigestV1Schema,
}).strict();
export type PricingPolicyDeliveryRepairResponseV2 =
  z.infer<typeof pricingPolicyDeliveryRepairResponseV2Schema>;

/**
 * Protected operator convergence for the commerce catalog/switch heads: stages the exact stored
 * immutable version as an engine control job. The request carries no policy payload, only the
 * version identity and the human reason.
 */
export const pricingCatalogJobStageRequestV2Schema = z.object({
  product_id: z.enum(["main", "openkeys"]),
  generation: z.number().int().safe().positive(),
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingCatalogJobStageRequestV2 = z.infer<typeof pricingCatalogJobStageRequestV2Schema>;

export const pricingSwitchJobStageRequestV2Schema = z.object({
  generation: z.number().int().safe().positive(),
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingSwitchJobStageRequestV2 = z.infer<typeof pricingSwitchJobStageRequestV2Schema>;

export const pricingControlJobStageResponseV2Schema = z.object({
  status: z.enum(["staged"]),
  job_id: z.string().uuid(),
}).strict();
export type PricingControlJobStageResponseV2 = z.infer<typeof pricingControlJobStageResponseV2Schema>;

const pricingStageControlPositiveIntegerV2Schema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) > 0n,
  "value must be positive",
);
export const pricingStage6StatusV2Schema = z.object({
  stage5_plan_digest: canonicalSha256V2Schema,
  stage5_status: z.enum(["blocked", "planned", "materializing", "prepared", "failed"]),
  target_generation: pricingStageControlPositiveIntegerV2Schema,
  target_plan_digest: canonicalSha256V2Schema,
  target_release_digest: canonicalSha256V2Schema.nullable(),
  target_status: z.enum(["planned", "materializing", "prepared", "active", "superseded", "failed"]),
  recovery_generation: pricingStageControlPositiveIntegerV2Schema,
  recovery_plan_digest: canonicalSha256V2Schema,
  recovery_release_digest: canonicalSha256V2Schema.nullable(),
  recovery_status: z.enum(["planned", "materializing", "prepared", "active", "superseded", "failed"]),
  staged_job_id: z.string().uuid().optional(),
  job_id: z.string().uuid().nullable(),
  job_status: z.enum(["pending", "processing", "retry", "confirmed", "dead"]).nullable(),
  job_attempts: z.number().int().safe().nonnegative().nullable(),
  job_last_error: z.string().nullable(),
  job_result_digest: canonicalSha256V2Schema.nullable(),
  pending_accounts: z.number().int().safe().nonnegative(),
  processing_accounts: z.number().int().safe().nonnegative(),
  retry_accounts: z.number().int().safe().nonnegative(),
  ready_accounts: z.number().int().safe().nonnegative(),
  blocker_accounts: z.number().int().safe().nonnegative(),
  target_funding_manifest_digest: canonicalSha256V2Schema.nullable(),
  recovery_funding_manifest_digest: canonicalSha256V2Schema.nullable(),
}).strict();
export type PricingStage6StatusV2 = z.infer<typeof pricingStage6StatusV2Schema>;

export const pricingReleaseActivationStageRequestV2Schema = z.object({
  activation_kind: pricingReleaseActivationKindV2Schema,
  evidence_digest: canonicalSha256V2Schema,
  reason: pricingReleaseActivationReasonV2Schema,
}).strict();
export type PricingReleaseActivationStageRequestV2 =
  z.infer<typeof pricingReleaseActivationStageRequestV2Schema>;

export const pricingReleaseActivationStageResponseV2Schema = z.object({
  job_id: z.string().uuid(),
  activation_kind: pricingReleaseActivationKindV2Schema,
  evidence_digest: canonicalSha256V2Schema,
  status: z.literal("accepted"),
}).strict();
export type PricingReleaseActivationStageResponseV2 =
  z.infer<typeof pricingReleaseActivationStageResponseV2Schema>;

export const pricingReleaseOrchestrationStageRequestV2Schema = z.object({
  idempotency_key: z.string().uuid(),
  capability_generation: pricingVersionSchema,
  reason: pricingReleaseActivationReasonV2Schema,
}).strict();
export type PricingReleaseOrchestrationStageRequestV2 =
  z.infer<typeof pricingReleaseOrchestrationStageRequestV2Schema>;

export const pricingReleaseOrchestrationStageResponseV2Schema = z.object({
  orchestration_id: z.string().uuid(),
  idempotent_replay: z.boolean(),
  status: z.literal("accepted"),
}).strict();
export type PricingReleaseOrchestrationStageResponseV2 =
  z.infer<typeof pricingReleaseOrchestrationStageResponseV2Schema>;

export const pricingReleaseActivationReconcileRequestV2Schema = z.object({
  job_id: z.string().uuid(),
  reason: pricingReleaseActivationReasonV2Schema,
}).strict();
export type PricingReleaseActivationReconcileRequestV2 =
  z.infer<typeof pricingReleaseActivationReconcileRequestV2Schema>;

export const pricingReleaseActivationReconcileResponseV2Schema = z.object({
  job_id: z.string().uuid(),
  activation_id: z.string().min(1),
  result_digest: z.string(),
  status: z.enum(["reconciled", "unchanged"]),
}).strict();
export type PricingReleaseActivationReconcileResponseV2 =
  z.infer<typeof pricingReleaseActivationReconcileResponseV2Schema>;

/**
 * Read-only Stage 8 capture producer and its explicit managed-commerce handoff. Integer fields in
 * the engine response are normalized to decimal strings because json-bigint may return either a
 * safe number or a string depending on magnitude. The request itself stays bounded safe integers
 * so JSON.stringify emits the numeric wire shape required by the Rust producer.
 */
const pricingStage8CaptureRequestV2ObjectSchema = z.object({
  target_generation: z.number().int().safe().positive(),
  recovery_generation: z.number().int().safe().positive(),
  window_start_ts: z.number().int().safe().positive(),
  window_end_ts: z.number().int().safe().positive(),
  min_samples_per_provider: z.number().int().safe().min(1).max(1_000_000),
  financial_sample_size: z.number().int().safe().min(1).max(1_000),
  gemini_client_admissions: z.number().int().safe().nonnegative(),
}).strict();

function refinePricingStage8CaptureRequestV2(
  request: z.infer<typeof pricingStage8CaptureRequestV2ObjectSchema>,
  context: z.RefinementCtx,
): void {
  if (request.recovery_generation <= request.target_generation) {
    context.addIssue({ code: "custom", message: "recovery generation must be newer than target generation" });
  }
  if (request.window_end_ts <= request.window_start_ts) {
    context.addIssue({ code: "custom", message: "capture window must be non-empty" });
  }
}

export const pricingStage8CaptureRequestV2Schema = pricingStage8CaptureRequestV2ObjectSchema
  .superRefine(refinePricingStage8CaptureRequestV2);
export type PricingStage8CaptureRequestV2 =
  z.infer<typeof pricingStage8CaptureRequestV2Schema>;

const pricingStage8PositiveIntegerV2Schema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) > 0n,
  "value must be positive",
);
const pricingStage8Sha256V1Schema = z.string().regex(/^sha256:v1:[0-9a-f]{64}$/);
const pricingStage8SubjectDigestSchema = z.string().regex(/^sha256:v(?:1|2):[0-9a-f]{64}$/);
const pricingStage8NonEmptyStringSchema = z.string().min(1);
const pricingStage8BasisPointsV2Schema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) <= 10_000n,
  "basis points must be at most 10000",
);
const pricingStage8BlockerV2Schema = z.object({
  code: pricingStage8NonEmptyStringSchema,
  count: pricingStage8PositiveIntegerV2Schema,
  subject_digests: z.array(pricingStage8Sha256V1Schema).max(20),
}).strict();
const pricingStage8CatalogV2Schema = z.object({
  product_id: pricingStage8NonEmptyStringSchema,
  generation: pricingStage8PositiveIntegerV2Schema,
  schema_version: pricingStage8PositiveIntegerV2Schema,
  capability_generation: pricingStage8PositiveIntegerV2Schema,
  capability_digest: pricingStage8NonEmptyStringSchema,
  content_digest: pricingStage8NonEmptyStringSchema,
  enabled_entries: nonNegativeIntegerSchema,
}).strict();
const pricingStage8SwitchV2Schema = z.object({
  generation: pricingStage8PositiveIntegerV2Schema,
  schema_version: pricingStage8PositiveIntegerV2Schema,
  capability_generation: pricingStage8PositiveIntegerV2Schema,
  capability_digest: pricingStage8NonEmptyStringSchema,
  content_digest: pricingStage8NonEmptyStringSchema,
  entries: nonNegativeIntegerSchema,
}).strict();
const pricingStage8RuntimeCapabilityV2Schema = z.object({
  schema_version: pricingStage8PositiveIntegerV2Schema,
  generation: pricingStage8PositiveIntegerV2Schema,
  digest: pricingStage8NonEmptyStringSchema,
}).strict();
const pricingStage8RuntimeManifestV2Schema = z.object({
  generation: pricingStage8PositiveIntegerV2Schema,
  digest: pricingStage8NonEmptyStringSchema,
  capabilities: z.array(pricingStage8RuntimeCapabilityV2Schema).min(1),
}).strict();
const pricingStage8ReleaseHeadV2Schema = z.object({
  active_generation: pricingStage8PositiveIntegerV2Schema,
  active_digest: pricingStage8NonEmptyStringSchema,
  head_version: pricingStage8PositiveIntegerV2Schema,
  updated_ts: pricingStage8PositiveIntegerV2Schema,
}).strict();
const pricingStage8ReleasePairV2Schema = z.object({
  target_generation: pricingStage8PositiveIntegerV2Schema,
  target_digest: canonicalSha256V2Schema.nullable(),
  recovery_generation: pricingStage8PositiveIntegerV2Schema,
  recovery_digest: canonicalSha256V2Schema.nullable(),
  recovery_link_digest: canonicalSha256V2Schema.nullable(),
  inventory_digest: canonicalSha256V2Schema.nullable(),
  funding_digest: canonicalSha256V2Schema.nullable(),
  target_assignment_count: nonNegativeIntegerSchema,
  recovery_assignment_count: nonNegativeIntegerSchema,
  active_head: pricingStage8ReleaseHeadV2Schema.nullable(),
}).strict();
const pricingStage8FinancialSampleV2Schema = z.object({
  subject_digest: pricingStage8Sha256V1Schema,
  evaluation_digest: pricingStage8NonEmptyStringSchema,
  provider_id: z.enum(["anthropic", "openai", "google"]),
  account_class: z.enum(["b2c", "b2b", "openkeys", "service"]),
  authorized_multiplier_bp: pricingStage8BasisPointsV2Schema,
  payable_multiplier_bp: pricingStage8BasisPointsV2Schema,
  official_hold_nano: nonNegativeIntegerSchema,
  legacy_hold_nano: nonNegativeIntegerSchema,
  policy_hold_nano: nonNegativeIntegerSchema,
  comparison_result: z.enum(["equal", "different"]),
}).strict();
const pricingStage8CountRecordV2Schema = z.record(z.string().min(1), nonNegativeIntegerSchema);
const pricingStage8CountsV2Schema = z.object({
  total_accounts: nonNegativeIntegerSchema,
  active_accounts: nonNegativeIntegerSchema,
  account_classes: pricingStage8CountRecordV2Schema,
  reconciled_accounts: nonNegativeIntegerSchema,
  snapshots_by_provider: pricingStage8CountRecordV2Schema,
  evaluations_by_outcome: pricingStage8CountRecordV2Schema,
  comparisons: pricingStage8CountRecordV2Schema,
  scalar_parity_rows: nonNegativeIntegerSchema,
  policy_divergence_rows: nonNegativeIntegerSchema,
  gemini_usage_rows: nonNegativeIntegerSchema,
  gemini_outbox_rows: nonNegativeIntegerSchema,
  live_runtime_instances: nonNegativeIntegerSchema,
  release_capable_runtime_instances: nonNegativeIntegerSchema,
  legacy_inflight_reservations: nonNegativeIntegerSchema,
  legacy_inflight_outbox_rows: nonNegativeIntegerSchema,
}).strict();

export const pricingStage8EngineEvidenceV2Schema = z.object({
  schema_version: nonNegativeIntegerSchema.refine((value) => value === "2", "schema_version must be 2"),
  captured_ts: pricingStage8PositiveIntegerV2Schema,
  window_start_ts: pricingStage8PositiveIntegerV2Schema,
  window_end_ts: pricingStage8PositiveIntegerV2Schema,
  min_samples_per_provider: pricingStage8PositiveIntegerV2Schema,
  gemini_client_admissions: nonNegativeIntegerSchema,
  passed: z.boolean(),
  release: pricingStage8ReleasePairV2Schema,
  runtime_manifest: pricingStage8RuntimeManifestV2Schema,
  catalogs: z.array(pricingStage8CatalogV2Schema),
  switches: pricingStage8SwitchV2Schema.nullable(),
  counts: pricingStage8CountsV2Schema,
  financial_samples: z.array(pricingStage8FinancialSampleV2Schema).max(1_000),
  engine_inventory_digest: canonicalSha256V2Schema,
  funding_digest: canonicalSha256V2Schema,
  shadow_digest: canonicalSha256V2Schema,
  runtime_floor_digest: canonicalSha256V2Schema,
  legacy_inflight_count: nonNegativeIntegerSchema,
  blockers: z.array(pricingStage8BlockerV2Schema),
  evidence_digest: canonicalSha256V2Schema,
}).strict().superRefine((report, context) => {
  if (BigInt(report.window_end_ts) <= BigInt(report.window_start_ts)
      || BigInt(report.window_end_ts) > BigInt(report.captured_ts)) {
    context.addIssue({ code: "custom", message: "invalid Stage 8 evidence window" });
  }
  if (BigInt(report.release.recovery_generation) <= BigInt(report.release.target_generation)) {
    context.addIssue({ code: "custom", message: "invalid target/recovery order" });
  }
  const legacyInflight = BigInt(report.counts.legacy_inflight_reservations)
    + BigInt(report.counts.legacy_inflight_outbox_rows);
  if (legacyInflight !== BigInt(report.legacy_inflight_count)) {
    context.addIssue({ code: "custom", message: "legacy inflight count mismatch" });
  }
  if (report.passed !== (report.blockers.length === 0)) {
    context.addIssue({ code: "custom", message: "passed/blocker state mismatch" });
  }
});
export type PricingStage8EngineEvidenceV2 =
  z.infer<typeof pricingStage8EngineEvidenceV2Schema>;

export const pricingStage8CaptureStageRequestV2Schema = pricingStage8CaptureRequestV2ObjectSchema.extend({
  idempotency_key: z.string().uuid(),
  reason: pricingReleaseActivationReasonV2Schema,
}).strict().superRefine(refinePricingStage8CaptureRequestV2);
export type PricingStage8CaptureStageRequestV2 =
  z.infer<typeof pricingStage8CaptureStageRequestV2Schema>;

export const pricingStage8CaptureStageResponseV2Schema = z.object({
  job_id: z.string().uuid(),
  request_digest: canonicalSha256V2Schema,
  status: z.literal("accepted"),
}).strict();
export type PricingStage8CaptureStageResponseV2 =
  z.infer<typeof pricingStage8CaptureStageResponseV2Schema>;

export const pricingStage8CaptureJobStatusV2Schema = z.enum([
  "pending",
  "processing",
  "retry",
  "passed",
  "blocked",
  "dead",
]);
export const pricingStage8CaptureControlV2Schema = z.object({
  database_observed_at: z.string().datetime({ offset: true }),
  counts_by_status: z.record(pricingStage8CaptureJobStatusV2Schema, z.number().int().safe().nonnegative()),
  jobs: z.array(z.object({
    id: z.string().uuid(),
    idempotency_key: z.string().min(1),
    request_digest: canonicalSha256V2Schema,
    target_generation: pricingStage8PositiveIntegerV2Schema,
    recovery_generation: pricingStage8PositiveIntegerV2Schema,
    window_start_at: z.string().datetime({ offset: true }),
    window_end_at: z.string().datetime({ offset: true }),
    min_samples_per_provider: pricingStage8PositiveIntegerV2Schema,
    financial_sample_size: z.number().int().safe().min(1).max(1_000),
    gemini_client_admissions: nonNegativeIntegerSchema,
    operator_id: pricingReleaseActivationOperatorV2Schema,
    reason: pricingReleaseActivationReasonV2Schema,
    status: pricingStage8CaptureJobStatusV2Schema,
    attempts: z.number().int().safe().nonnegative(),
    next_attempt_at: z.string().datetime({ offset: true }),
    locked_at: z.string().datetime({ offset: true }).nullable(),
    locked_by: z.string().nullable(),
    last_error: z.string().nullable(),
    result_engine_evidence_digest: canonicalSha256V2Schema.nullable(),
    result_combined_evidence_digest: canonicalSha256V2Schema.nullable(),
    result_passed: z.boolean().nullable(),
    completed_at: z.string().datetime({ offset: true }).nullable(),
    created_at: z.string().datetime({ offset: true }),
    updated_at: z.string().datetime({ offset: true }),
  }).strict()).max(100),
  artifacts: z.array(z.object({
    id: z.string().uuid(),
    job_id: z.string().uuid(),
    attempt: z.number().int().safe().positive(),
    engine_evidence_digest: canonicalSha256V2Schema,
    engine_captured_at: z.string().datetime({ offset: true }),
    combined_evidence_digest: canonicalSha256V2Schema.nullable(),
    combined_passed: z.boolean().nullable(),
    combined_write_result: z.enum(["stored", "unchanged", "not_persisted"]).nullable(),
    combined_observed_at: z.string().datetime({ offset: true }).nullable(),
    combined_valid_until: z.string().datetime({ offset: true }).nullable(),
    combined_blocker_count: nonNegativeIntegerSchema.nullable(),
    combined_blockers: z.array(z.object({
      source: z.enum(["commerce", "engine"]),
      code: z.string().min(1),
      count: pricingStage8PositiveIntegerV2Schema,
      subject_digests: z.array(pricingStage8SubjectDigestSchema).max(20),
    }).strict()).max(100).nullable(),
    combined_blockers_truncated: z.boolean().nullable(),
    completed_at: z.string().datetime({ offset: true }).nullable(),
    created_at: z.string().datetime({ offset: true }),
  }).strict()).max(100),
}).strict();
export type PricingStage8CaptureControlV2 =
  z.infer<typeof pricingStage8CaptureControlV2Schema>;

/**
 * Stage 7 durable shadow-rollout lane (migration 0035): the protected producer pins one exact
 * prepared Stage 5 target/recovery pair and the complete per-account policy/binding/CAS request
 * set; the bounded read model exposes only subject digests, never raw account identities.
 */
export const pricingShadowRolloutStageRequestV2Schema = z.object({
  idempotency_key: z.string().uuid(),
  stage5_run_id: z.string().uuid(),
  reason: pricingStageControlMutationReasonV2Schema,
}).strict();
export type PricingShadowRolloutStageRequestV2 =
  z.infer<typeof pricingShadowRolloutStageRequestV2Schema>;

export const pricingShadowRolloutStageResponseV2Schema = z.object({
  rollout_id: z.string().uuid(),
  rollout_digest: canonicalSha256V2Schema,
  job_count: z.number().int().safe().nonnegative(),
  idempotent_replay: z.boolean(),
  status: z.literal("accepted"),
}).strict();
export type PricingShadowRolloutStageResponseV2 =
  z.infer<typeof pricingShadowRolloutStageResponseV2Schema>;

export const pricingShadowRolloutStatusV2Schema = z.enum([
  "pending",
  "processing",
  "confirmed",
  "blocked",
  "dead",
]);
export const pricingShadowRolloutJobStatusV2Schema = z.enum([
  "pending",
  "processing",
  "retry",
  "confirmed",
  "blocked",
  "dead",
]);
const pricingShadowRolloutPositiveCountV2Schema = nonNegativeIntegerSchema;
const pricingShadowRolloutGenerationV2Schema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) > 0n,
  "generation must be positive",
);
export const pricingShadowRolloutControlV2Schema = z.object({
  database_observed_at: z.string().datetime({ offset: true }),
  counts_by_status: z.record(pricingShadowRolloutStatusV2Schema, z.number().int().safe().nonnegative()),
  rollouts: z.array(z.object({
    id: z.string().uuid(),
    idempotency_key: z.string().uuid(),
    stage5_run_id: z.string().uuid(),
    rollout_digest: canonicalSha256V2Schema,
    target_generation: pricingShadowRolloutGenerationV2Schema,
    target_digest: canonicalSha256V2Schema,
    recovery_generation: pricingShadowRolloutGenerationV2Schema,
    recovery_digest: canonicalSha256V2Schema,
    catalog_generation: pricingShadowRolloutGenerationV2Schema,
    main_catalog_digest: z.string().regex(/^sha256:v[12]:[0-9a-f]{64}$/),
    openkeys_catalog_digest: z.string().regex(/^sha256:v[12]:[0-9a-f]{64}$/),
    switch_generation: pricingShadowRolloutGenerationV2Schema,
    switch_digest: z.string().regex(/^sha256:v[12]:[0-9a-f]{64}$/),
    engine_inventory_digest: canonicalSha256V2Schema,
    assignment_manifest_digest: canonicalSha256V2Schema,
    policy_manifest_digest: canonicalSha256V2Schema,
    assignment_count: pricingShadowRolloutPositiveCountV2Schema,
    job_count: pricingShadowRolloutPositiveCountV2Schema,
    job_counts_by_status: z.record(pricingShadowRolloutJobStatusV2Schema, z.number().int().safe().nonnegative()),
    actor_id: pricingReleaseActivationOperatorV2Schema,
    reason: z.string(),
    status: pricingShadowRolloutStatusV2Schema,
    last_error: z.string().nullable(),
    completed_at: z.string().datetime({ offset: true }).nullable(),
    created_at: z.string().datetime({ offset: true }),
    updated_at: z.string().datetime({ offset: true }),
  }).strict()).max(100),
  jobs: z.array(z.object({
    id: z.string().uuid(),
    rollout_id: z.string().uuid(),
    subject_digest: canonicalSha256V2Schema,
    account_status: z.enum(["active", "disabled"]),
    account_class: z.enum(["b2c", "b2b", "openkeys", "service"]),
    owner_context: z.enum(["commerce", "openkeys", "service"]),
    release_policy_digest: canonicalSha256V2Schema,
    content_digest: canonicalSha256V2Schema,
    expected_active_digest: z.string().regex(/^sha256:v[12]:[0-9a-f]{64}$/).nullable(),
    request_digest: canonicalSha256V2Schema,
    status: pricingShadowRolloutJobStatusV2Schema,
    attempts: z.number().int().safe().nonnegative(),
    last_error: z.string().nullable(),
    ack_digest: canonicalSha256V2Schema.nullable(),
    confirmed_at: z.string().datetime({ offset: true }).nullable(),
    completed_at: z.string().datetime({ offset: true }).nullable(),
    created_at: z.string().datetime({ offset: true }),
    updated_at: z.string().datetime({ offset: true }),
  }).strict()).max(100),
}).strict();
export type PricingShadowRolloutControlV2 =
  z.infer<typeof pricingShadowRolloutControlV2Schema>;

export const pricingReleaseActivationEvidenceV2Schema = z.object({
  evidence_digest: canonicalSha256V2Schema,
  target_generation: pricingVersionSchema,
  target_digest: canonicalSha256V2Schema,
  recovery_generation: pricingVersionSchema,
  recovery_digest: canonicalSha256V2Schema,
  engine_inventory_digest: canonicalSha256V2Schema,
  funding_digest: canonicalSha256V2Schema,
  shadow_digest: canonicalSha256V2Schema,
  runtime_floor_digest: canonicalSha256V2Schema,
  legacy_inflight_count: z.number().int().safe().nonnegative(),
  engine_captured_ts: pricingReleaseActivationTimestampV2Schema,
  observed_ts: pricingReleaseActivationTimestampV2Schema,
  valid_until_ts: pricingReleaseActivationTimestampV2Schema,
}).strict().superRefine((evidence, context) => {
  if (evidence.recovery_generation <= evidence.target_generation) {
    context.addIssue({ code: "custom", message: "recovery generation must be newer than target generation" });
  }
  if (evidence.engine_captured_ts > evidence.observed_ts + 5
      || evidence.observed_ts - evidence.engine_captured_ts > 120) {
    context.addIssue({ code: "custom", message: "engine evidence capture time is outside the accepted window" });
  }
  if (evidence.valid_until_ts <= evidence.observed_ts
      || evidence.valid_until_ts - evidence.observed_ts > 300) {
    context.addIssue({ code: "custom", message: "activation evidence TTL must be within 300 seconds" });
  }
});
export type PricingReleaseActivationEvidenceV2 =
  z.infer<typeof pricingReleaseActivationEvidenceV2Schema>;

export const pricingReleaseActivationRequestV2Schema = z.object({
  activation_kind: pricingReleaseActivationKindV2Schema,
  expectation: pricingReleaseHeadExpectationV2Schema,
  evidence: pricingReleaseActivationEvidenceV2Schema,
  operator_id: pricingReleaseActivationOperatorV2Schema,
  reason: pricingReleaseActivationReasonV2Schema,
}).strict().superRefine((request, context) => {
  if (request.activation_kind === "cutover") {
    if (request.expectation !== "absent") {
      context.addIssue({ code: "custom", message: "cutover requires an absent head expectation" });
    }
    return;
  }
  if (request.expectation === "absent") {
    context.addIssue({
      code: "custom",
      message: `${request.activation_kind} requires an exact head expectation`,
    });
    return;
  }
  if (request.activation_kind === "recovery") {
    if (request.expectation.exact.active_generation !== request.evidence.target_generation
        || request.expectation.exact.active_digest !== request.evidence.target_digest) {
      context.addIssue({ code: "custom", message: "recovery expectation must be the exact target head" });
    }
    return;
  }
  if (request.expectation.exact.active_generation >= request.evidence.target_generation) {
    context.addIssue({
      code: "custom",
      message: "successor expectation must be the exact current head behind the target",
    });
  }
});
export type PricingReleaseActivationRequestV2 =
  z.infer<typeof pricingReleaseActivationRequestV2Schema>;

const positiveDecimalIntegerSchema = nonNegativeIntegerSchema.refine(
  (value) => BigInt(value) > 0n,
  "value must be positive",
);

export const pricingReleaseActivationReceiptV2Schema = z.object({
  activation_id: positiveDecimalIntegerSchema,
  activation_kind: pricingReleaseActivationKindV2Schema,
  from_generation: pricingVersionSchema.nullable(),
  from_digest: canonicalSha256V2Schema.nullable(),
  expected_head_version: z.number().int().safe().nonnegative(),
  head: pricingReleaseHeadV2Schema,
  evidence_digest: canonicalSha256V2Schema,
  operator_id: pricingReleaseActivationOperatorV2Schema,
  reason: pricingReleaseActivationReasonV2Schema,
  activated_ts: pricingReleaseActivationTimestampV2Schema,
}).strict();
export type PricingReleaseActivationReceiptV2 =
  z.infer<typeof pricingReleaseActivationReceiptV2Schema>;

const activationInvalidRejectionSchema = z.object({
  invalid: z.object({ reason: z.string().min(1) }).strict(),
}).strict();
const activationMissingDependencyRejectionSchema = z.object({
  missing_dependency: z.object({ dependency: z.string().min(1) }).strict(),
}).strict();
const activationCasMismatchRejectionSchema = z.object({
  cas_mismatch: z.object({ actual: pricingReleaseHeadV2Schema.nullable() }).strict(),
}).strict();
const activationEvidenceStaleRejectionSchema = z.object({
  evidence_stale: z.object({
    now_ts: pricingReleaseActivationTimestampV2Schema,
    observed_ts: pricingReleaseActivationTimestampV2Schema,
    valid_until_ts: pricingReleaseActivationTimestampV2Schema,
  }).strict(),
}).strict();
const activationEvidenceConflictRejectionSchema = z.object({
  evidence_conflict: z.object({ evidence_digest: canonicalSha256V2Schema }).strict(),
}).strict();
const activationReleaseLineageDriftRejectionSchema = z.object({
  release_lineage_drift: z.object({ reason: z.string().min(1) }).strict(),
}).strict();
const activationInventoryDriftRejectionSchema = z.object({
  inventory_drift: z.object({
    expected_digest: canonicalSha256V2Schema,
    actual_digest: canonicalSha256V2Schema,
  }).strict(),
}).strict();
const activationFundingDriftRejectionSchema = z.object({
  funding_drift: z.object({
    expected_digest: canonicalSha256V2Schema,
    actual_digest: canonicalSha256V2Schema,
  }).strict(),
}).strict();
const activationFundingInvariantDriftRejectionSchema = z.object({
  funding_invariant_drift: z.object({ account_count: z.number().int().safe().nonnegative() }).strict(),
}).strict();
const activationRuntimeFloorDriftRejectionSchema = z.object({
  runtime_floor_drift: z.object({
    expected_digest: canonicalSha256V2Schema,
    actual_digest: canonicalSha256V2Schema,
  }).strict(),
}).strict();
const activationRuntimeIncompatibleRejectionSchema = z.object({
  runtime_incompatible: z.object({
    live_instances: z.number().int().safe().nonnegative(),
    compatible_instances: z.number().int().safe().nonnegative(),
  }).strict(),
}).strict();
const activationAuthorityDriftRejectionSchema = z.object({
  authority_drift: z.object({ changed_rows: z.number().int().safe().nonnegative() }).strict(),
}).strict();

export const pricingReleaseActivationAckV2Schema = z.union([
  z.object({
    result: z.enum(["applied", "unchanged"]),
    activation: pricingReleaseActivationReceiptV2Schema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("invalid"),
    rejection: activationInvalidRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("missing_dependency"),
    rejection: activationMissingDependencyRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("cas_mismatch"),
    rejection: activationCasMismatchRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("evidence_stale"),
    rejection: activationEvidenceStaleRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("evidence_conflict"),
    rejection: activationEvidenceConflictRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("release_lineage_drift"),
    rejection: activationReleaseLineageDriftRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("inventory_drift"),
    rejection: activationInventoryDriftRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("funding_drift"),
    rejection: activationFundingDriftRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("funding_invariant_drift"),
    rejection: activationFundingInvariantDriftRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("runtime_floor_drift"),
    rejection: activationRuntimeFloorDriftRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("runtime_incompatible"),
    rejection: activationRuntimeIncompatibleRejectionSchema,
  }).strict(),
  z.object({
    result: z.literal("rejected"),
    code: z.literal("authority_drift"),
    rejection: activationAuthorityDriftRejectionSchema,
  }).strict(),
]);
export type PricingReleaseActivationAckV2 =
  z.infer<typeof pricingReleaseActivationAckV2Schema>;

export const pricingReleaseInventoryAccountV2Schema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  status: z.enum(["active", "disabled"]),
  multiplier_bp: z.number().int().safe().nonnegative(),
  balance_nano: decimalIntegerSchema,
  reserved_nano: nonNegativeIntegerSchema,
  spent_nano: nonNegativeIntegerSchema,
  funding_generation: pricingVersionSchema.nullable(),
  funding_head_version: pricingVersionSchema.nullable(),
}).strict();
export type PricingReleaseInventoryAccountV2 = z.infer<typeof pricingReleaseInventoryAccountV2Schema>;

export const pricingReleaseInventoryPageV2Schema = z.object({
  accounts: z.array(pricingReleaseInventoryAccountV2Schema).max(500),
  next_after_account_id: z.string().startsWith("acct_").max(200).nullable(),
}).strict();
export type PricingReleaseInventoryPageV2 = z.infer<typeof pricingReleaseInventoryPageV2Schema>;

/**
 * Admin-managed authority for engine-native service accounts. The status is
 * supplied by the engine inventory producer, never by the operator mutation.
 */
export const serviceAccountInventoryServiceIdV2Schema = z.string().trim()
  .regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$/);
export const serviceAccountInventoryEntryV2Schema = z.object({
  service_id: serviceAccountInventoryServiceIdV2Schema,
  engine_account_id: z.string().startsWith("acct_").max(200),
  purpose: z.string().trim().min(3).max(500),
  responsible: z.string().trim().min(1).max(200),
  status: z.enum(["active", "disabled"]),
  source_version: pricingVersionSchema,
  content_digest: canonicalSha256V2Schema,
}).strict();
export type ServiceAccountInventoryEntryV2 = z.infer<typeof serviceAccountInventoryEntryV2Schema>;

export const serviceAccountInventoryV2Schema = z.object({
  schema_version: z.literal(PRICING_RELEASE_SCHEMA_VERSION_V2),
  inventory_digest: canonicalSha256V2Schema,
  accounts: z.array(serviceAccountInventoryEntryV2Schema),
}).strict();
export type ServiceAccountInventoryV2 = z.infer<typeof serviceAccountInventoryV2Schema>;

export const serviceAccountInventoryMutationV2Schema = z.object({
  expected_source_version: pricingVersionSchema.nullable(),
  expected_content_digest: canonicalSha256V2Schema.nullable(),
  engine_account_id: z.string().startsWith("acct_").max(200),
  purpose: z.string().trim().min(3).max(500),
  responsible: z.string().trim().min(1).max(200),
  reason: z.string().trim().min(3).max(2_000),
}).strict().superRefine((value, context) => {
  if ((value.expected_source_version === null) !== (value.expected_content_digest === null)) {
    context.addIssue({
      code: "custom",
      message: "expected_source_version and expected_content_digest must both be null or both be set",
    });
  }
});
export type ServiceAccountInventoryMutationV2 = z.infer<typeof serviceAccountInventoryMutationV2Schema>;

export const openKeysPricingInventoryAccountV2Schema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  source_id: z.string().uuid(),
  lifecycle: z.enum(["active", "disabled", "removed"]),
  pricing_contract: z.enum(["legacy", "official_1_to_1"]),
  source_multiplier_bp: z.number().int().min(1).max(10_000),
  content_digest: canonicalSha256V2Schema,
}).strict();
export type OpenKeysPricingInventoryAccountV2 = z.infer<typeof openKeysPricingInventoryAccountV2Schema>;

export const openKeysPricingInventoryPageV2Schema = z.object({
  inventory_digest: canonicalSha256V2Schema,
  accounts: z.array(openKeysPricingInventoryAccountV2Schema).max(500),
  next_after_account_id: z.string().startsWith("acct_").max(200).nullable(),
}).strict();
export type OpenKeysPricingInventoryPageV2 = z.infer<typeof openKeysPricingInventoryPageV2Schema>;

export const fundingNormalizationDigestV2Schema = canonicalSha256V2Schema;
export const fundingNormalizationSourceV2Schema = z.enum([
  "aggregate_paid_only",
  "ledger_replay",
  "legacy_buckets",
  "stored_generation",
]);
export const fundingNormalizationBlockerCodeV2Schema = z.enum([
  "account_deleted",
  "active_legacy_reservation",
  "aggregate_reservation_mismatch",
  "orphaned_funding_v2_state",
  "legacy_bucket_mismatch",
  "invalid_ledger_evidence",
  "arithmetic_overflow",
]);

export const fundingNormalizationLotV2Schema = z.object({
  lot_id: z.string().startsWith("fundv2_").max(200),
  source_type: z.enum(["paid", "welcome_bonus"]),
  source_ref: z.string().min(1).max(500),
  balance_nano: decimalIntegerSchema,
  reserved_nano: nonNegativeIntegerSchema,
  spent_nano: nonNegativeIntegerSchema,
  version: pricingVersionSchema,
  status: z.enum(["active", "exhausted"]),
}).strict();
export type FundingNormalizationLotV2 = z.infer<typeof fundingNormalizationLotV2Schema>;

export const fundingNormalizationBlockerV2Schema = z.object({
  code: fundingNormalizationBlockerCodeV2Schema,
  detail: z.string().min(1).max(2_000),
}).strict();
export type FundingNormalizationBlockerV2 = z.infer<typeof fundingNormalizationBlockerV2Schema>;

export const fundingNormalizationPlanV2Schema = z.object({
  account_id: z.string().startsWith("acct_").max(200),
  account_status: z.enum(["active", "disabled", "deleted"]),
  status: z.enum(["ready", "blocked", "normalized"]),
  source: fundingNormalizationSourceV2Schema,
  source_state_digest: fundingNormalizationDigestV2Schema,
  normalization_digest: fundingNormalizationDigestV2Schema.nullable(),
  funding_generation: pricingVersionSchema.nullable(),
  funding_head_version: pricingVersionSchema.nullable(),
  balance_nano: decimalIntegerSchema,
  reserved_nano: nonNegativeIntegerSchema,
  spent_nano: nonNegativeIntegerSchema,
  lots: z.array(fundingNormalizationLotV2Schema),
  blockers: z.array(fundingNormalizationBlockerV2Schema),
}).strict().superRefine((plan, context) => {
  const materialized = plan.normalization_digest !== null
    && plan.funding_generation !== null
    && plan.funding_head_version !== null;
  if (plan.status === "blocked") {
    if (materialized || plan.normalization_digest !== null
        || plan.funding_generation !== null || plan.funding_head_version !== null) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "blocked normalization cannot carry target identity" });
    }
    if (plan.blockers.length === 0) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "blocked normalization requires a blocker" });
    }
  } else {
    if (!materialized) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "ready normalization requires complete target identity" });
    }
    if (plan.blockers.length !== 0) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "ready normalization cannot carry blockers" });
    }
  }
  if ((plan.status === "normalized") !== (plan.source === "stored_generation")) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "stored generation source must be normalized" });
  }
});
export type FundingNormalizationPlanV2 = z.infer<typeof fundingNormalizationPlanV2Schema>;

export const fundingNormalizationApplyRequestV2Schema = z.object({
  expected_source_state_digest: fundingNormalizationDigestV2Schema,
  expected_normalization_digest: fundingNormalizationDigestV2Schema,
}).strict();
export type FundingNormalizationApplyRequestV2 = z.infer<typeof fundingNormalizationApplyRequestV2Schema>;

export const fundingNormalizationApplyResultV2Schema = z.object({
  status: z.enum(["stored", "unchanged", "stale", "blocked", "conflict"]),
  normalization: fundingNormalizationPlanV2Schema,
}).strict();
export type FundingNormalizationApplyResultV2 = z.infer<typeof fundingNormalizationApplyResultV2Schema>;

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

// The progressive B2C tier ladder was retired with the pricing release cutover on 2026-08-04:
// B2C is one flat 50% discount (docs/commerce/PRICING.md). The constant is removed so no new
// code can resurrect tier semantics; historical rows keep their values as immutable history.

// Welcome credit is an exact platform-balance amount, independent of the active pricing policy.
// The legacy nominal remains explicit only for rows issued before exact per-claim storage existed.
export const B2C_SIGNUP_BONUS_BALANCE_NANO = 5_000_000_000n;
export const B2C_LEGACY_SIGNUP_BONUS_BALANCE_NANO = 4_000_000_000n;

export const businessDiscountSchema = z.number().int().min(0).max(95);
export const pricingPolicyEditorRuleSchema = z.object({
  scope: z.union([
    z.object({
      provider: z.object({ providerId: pricingIdentifierSchema }).strict(),
    }).strict(),
    z.object({
      model: z.object({
        providerId: pricingIdentifierSchema,
        canonicalModelId: pricingIdentifierSchema,
      }).strict(),
    }).strict(),
  ]),
  // The editor schema is discount-only: the retired `track` mode is no longer accepted at the
  // write boundary. Historical stored rows keep their values and stay readable as history.
  pricingMode: z.enum(["discount"]),
  discountBps: z.number().int().min(0).max(9_500)
    .refine((value) => value % 100 === 0, "discountBps must use whole percentage points")
    .nullable(),
}).strict().superRefine((rule, context) => {
  if (rule.pricingMode === "discount" && rule.discountBps === null) {
    context.addIssue({ code: z.ZodIssueCode.custom, path: ["discountBps"], message: "discount rules require discountBps" });
  }
});
export type PricingPolicyEditorRule = z.infer<typeof pricingPolicyEditorRuleSchema>;

export const pricingPolicyEditorRulesSchema = z.array(pricingPolicyEditorRuleSchema).min(1).max(100)
  .superRefine((rules, context) => {
    const scopes = new Set<string>();
    rules.forEach((rule, index) => {
      const key = "provider" in rule.scope
        ? `${rule.scope.provider.providerId}\0`
        : `${rule.scope.model.providerId}\0${rule.scope.model.canonicalModelId}`;
      if (scopes.has(key)) {
        context.addIssue({ code: z.ZodIssueCode.custom, path: [index, "scope"], message: "pricing rule scope is duplicated" });
      }
      scopes.add(key);
    });
  });

export const pricingPolicyMutationSchema = z.object({
  expectedVersion: z.number().int().positive(),
  reason: z.string().trim().min(3).max(300),
  rules: pricingPolicyEditorRulesSchema,
}).strict();
export type PricingPolicyMutation = z.infer<typeof pricingPolicyMutationSchema>;

export const providerSwitchEditorStateSchema = z.object({
  providerId: pricingIdentifierSchema,
  masterEnabled: z.boolean(),
  productEnabled: z.boolean(),
  b2cEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
}).strict();
export const providerSwitchEditorMutationSchema = z.object({
  expectedGeneration: z.number().int().positive(),
  reason: z.string().trim().min(3).max(300),
  providers: z.array(providerSwitchEditorStateSchema).min(1).max(20),
}).strict().superRefine((value, context) => {
  const providers = new Set<string>();
  value.providers.forEach((provider, index) => {
    if (providers.has(provider.providerId)) {
      context.addIssue({ code: z.ZodIssueCode.custom, path: ["providers", index, "providerId"], message: "provider switch is duplicated" });
    }
    providers.add(provider.providerId);
  });
});
export type ProviderSwitchEditorMutation = z.infer<typeof providerSwitchEditorMutationSchema>;

export const createBusinessInviteSchema = z.object({
  email: authEmailSchema.optional(),
  // discountPercent remains an additive rolling-deploy fallback for an older admin slot. A
  // full-policy request never combines with it or turns the scalar into extra rules.
  discountPercent: businessDiscountSchema.optional(),
  policy: z.object({ rules: pricingPolicyEditorRulesSchema }).strict().optional(),
  expiresInDays: z.number().int().min(1).max(30).default(7),
  reason: z.string().trim().min(3).max(300),
  idempotencyKey: z.string().uuid(),
}).strict().refine(
  (value) => (value.policy === undefined) !== (value.discountPercent === undefined),
  { message: "provide exactly one of policy or the legacy scalar fallback" },
);
export const setBusinessPricingSchema = z.object({
  discountPercent: businessDiscountSchema.optional(),
  policy: pricingPolicyMutationSchema.omit({ reason: true }).optional(),
  reason: z.string().trim().min(3).max(300),
}).strict().refine(
  (value) => (value.policy === undefined) !== (value.discountPercent === undefined),
  { message: "provide exactly one of policy or discountPercent" },
);

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
