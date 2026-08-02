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

export const engineLedgerAttributionSchema = z.object({
  attribution_schema_version: nonNegativeIntegerSchema,
  snapshot_kind: z.enum(["policy_v1", "legacy_scalar"]).nullish()
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
  rule_scope: z.enum(["provider", "model"]).nullish().transform((value) => value ?? null),
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
  funding_allocation_json: z.array(engineSettlementFundingEvidenceSchema).nullish()
    .transform((value) => value ?? null),
  track_eligible: z.boolean().nullish().transform((value) => value ?? null),
  retention_eligible: z.boolean().nullish().transform((value) => value ?? null),
  commission_eligible: z.boolean().nullish().transform((value) => value ?? null),
  snapshot_digest: nullableStringSchema,
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
  // Free-form on purpose: the engine tags rows with its own provider ids
  // ("anthropic", "openai", "google" for Gemini traffic, more later) and an
  // exact enum here already took the usage endpoint down twice.
  provider: z.string().optional(),
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
    // Free-form, same rationale as engineUsageModelSchema.provider.
    provider: z.string(),
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
 * remains `gpt-5.6 -> gpt-5.6-sol`; the exact builder lives in
 * `packages/db/src/multi-discount-catalog-gen2.ts` and is pinned by unit tests.
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
 * Additive prepare/read contract for the dormant pricing-release v2 authority.
 * These schemas intentionally contain no activation request: a consumer can
 * materialize and inspect immutable data, but cannot change the live release head.
 */
export const PRICING_RELEASE_SCHEMA_VERSION_V2 = 2 as const;
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

export const canonicalSha256V2Schema = z.string().regex(/^sha256:v2:[0-9a-f]{64}$/);

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
  pricingMode: z.enum(["track", "discount"]),
  discountBps: z.number().int().min(0).max(9_500)
    .refine((value) => value % 100 === 0, "discountBps must use whole percentage points")
    .nullable(),
}).strict().superRefine((rule, context) => {
  if (rule.pricingMode === "track" && rule.discountBps !== null) {
    context.addIssue({ code: z.ZodIssueCode.custom, path: ["discountBps"], message: "track rules do not have a fixed discount" });
  }
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
