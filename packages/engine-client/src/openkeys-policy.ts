import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";

export const OFFICIAL_ONE_TO_ONE_MULT_BP = 10_000;
export const OFFICIAL_ONE_TO_ONE_CONTRACT = "official_1_to_1" as const;
export const OPENKEYS_POLICY_PROVIDERS = ["anthropic", "openai"] as const;

export interface OpenKeysPricingAuthority {
  catalog: PricingCatalogSpec;
  switches: ProviderSwitchSpec;
}

export class OpenKeysPolicyError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "OpenKeysPolicyError";
  }
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalValue);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .filter(([, child]) => child !== undefined)
        .sort(([left], [right]) => compareUtf8(left, right))
        .map(([key, child]) => [key, canonicalValue(child)]),
    );
  }
  return value;
}

export function canonicalPricingJson(value: unknown): string {
  return JSON.stringify(canonicalValue(value));
}

export function stage7OpenKeysDigest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`multi-discount-stage7:${label}\n`, "utf8")
    .update(canonicalPricingJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}

function catalogEntryKey(entry: { provider_id: string; canonical_model_id: string }): string {
  return `${entry.provider_id}\u0000${entry.canonical_model_id}`;
}

/**
 * Reviewed OpenKeys catalog identities, oldest first. Generations 1 and 2 are
 * historical, generation 5 is the Anthropic/OpenAI authority without Gemini,
 * and generation 6 — the active successor — adds only GPT Image 2. Each identity pins
 * its exact catalog generation, capability pin and complete enabled entry set,
 * so a partial or forged catalog never passes.
 */
const REVIEWED_OPENKEYS_CATALOGS = [
  {
    generation: 1,
    capability_generation: MULTI_DISCOUNT_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_CAPABILITY_DIGEST,
    entries: CURRENT_PRODUCT_CATALOG_ENTRIES,
  },
  {
    generation: 2,
    capability_generation: MULTI_DISCOUNT_GEN2_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN2_CAPABILITY_DIGEST,
    entries: MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES,
  },
  {
    generation: 5,
    capability_generation: MULTI_DISCOUNT_GEN5_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN5_CAPABILITY_DIGEST,
    entries: MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES,
  },
  {
    generation: 6,
    capability_generation: MULTI_DISCOUNT_GEN6_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN6_CAPABILITY_DIGEST,
    entries: MULTI_DISCOUNT_GEN6_OPENKEYS_CATALOG_ENTRIES,
  },
  {
    generation: 7,
    capability_generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
    entries: MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES,
  },
] as const;

function matchesReviewedCatalog(
  catalog: PricingCatalogSpec,
  reviewed: (typeof REVIEWED_OPENKEYS_CATALOGS)[number],
): boolean {
  if (
    catalog.generation !== reviewed.generation ||
    catalog.capability_generation !== reviewed.capability_generation ||
    catalog.capability_digest !== reviewed.capability_digest
  ) {
    return false;
  }
  const expected = new Set(reviewed.entries.map(catalogEntryKey));
  const actual = new Set(catalog.entries.map(catalogEntryKey));
  return catalog.entries.length === expected.size &&
    actual.size === expected.size &&
    catalog.entries.every((entry) => entry.enabled && expected.has(catalogEntryKey(entry)));
}

export function assertOpenKeysCatalog(catalog: PricingCatalogSpec): void {
  if (
    catalog.product_id !== OPENKEYS_PRICING_PRODUCT_ID ||
    catalog.schema_version !== MULTI_DISCOUNT_SCHEMA_VERSION
  ) {
    throw new OpenKeysPolicyError(
      "catalog_identity_mismatch",
      "active OpenKeys catalog does not match the reviewed pricing capability",
    );
  }

  if (!REVIEWED_OPENKEYS_CATALOGS.some((reviewed) => matchesReviewedCatalog(catalog, reviewed))) {
    throw new OpenKeysPolicyError(
      "catalog_models_mismatch",
      "OpenKeys issuance requires the exact reviewed Anthropic/OpenAI catalog",
    );
  }
}

function isProductScope(
  scope: ProviderSwitchSpec["entries"][number]["scope"],
): scope is { product: { product_id: string } } {
  return typeof scope === "object" && "product" in scope;
}

export function assertOpenKeysSwitches(
  switches: ProviderSwitchSpec,
  catalog: PricingCatalogSpec,
): void {
  if (
    switches.schema_version !== MULTI_DISCOUNT_SCHEMA_VERSION ||
    switches.capability_generation !== catalog.capability_generation ||
    switches.capability_digest !== catalog.capability_digest
  ) {
    throw new OpenKeysPolicyError(
      "switch_identity_mismatch",
      "active provider switches do not match the OpenKeys catalog capability",
    );
  }

  const productEntries = switches.entries.filter((entry) =>
    isProductScope(entry.scope) && entry.scope.product.product_id === OPENKEYS_PRICING_PRODUCT_ID
  );
  if (
    productEntries.length !== OPENKEYS_POLICY_PROVIDERS.length ||
    productEntries.some((entry) =>
      !OPENKEYS_POLICY_PROVIDERS.includes(
        entry.provider_id as (typeof OPENKEYS_POLICY_PROVIDERS)[number],
      ) ||
      !entry.enabled ||
      entry.catalog_generation !== catalog.generation
    )
  ) {
    throw new OpenKeysPolicyError(
      "openkeys_provider_switch_mismatch",
      "OpenKeys requires only enabled Anthropic and OpenAI product switches",
    );
  }

  for (const provider of OPENKEYS_POLICY_PROVIDERS) {
    const master = switches.entries.filter(
      (entry) => entry.provider_id === provider && entry.scope === "master",
    );
    const product = productEntries.filter((entry) => entry.provider_id === provider);
    if (master.length !== 1 || !master[0]!.enabled || product.length !== 1) {
      throw new OpenKeysPolicyError(
        "openkeys_provider_disabled",
        `OpenKeys provider ${provider} is not enabled at both master and product scope`,
      );
    }
  }
}

function officialRule(
  providerId: (typeof OPENKEYS_POLICY_PROVIDERS)[number],
): AccountPolicySpec["rules"][number] {
  const base = {
    rule_id: `provider:${providerId}:official-1-to-1`,
    scope: { provider: { provider_id: providerId } },
    pricing_mode: "discount" as const,
    rule_origin: "managed" as const,
    discount_bps: 0,
    payable_multiplier_bp: OFFICIAL_ONE_TO_ONE_MULT_BP,
    track_eligible: false,
    retention_eligible: false,
    commission_eligible: false,
  };
  return { ...base, rule_digest: stage7OpenKeysDigest("official-rule", base) };
}

/**
 * Canonical policy payload shared by Stage 5 inventory planning, Stage 7 batch
 * cutover, and all new OpenKeys issuance. There must never be a second local
 * implementation of this identity or digest domain.
 */
export function buildOfficialOpenKeysPolicy(
  accountId: string,
  authority: OpenKeysPricingAuthority,
): AccountPolicySpec {
  assertOpenKeysCatalog(authority.catalog);
  assertOpenKeysSwitches(authority.switches, authority.catalog);
  const rules = OPENKEYS_POLICY_PROVIDERS.map(officialRule);
  const source = {
    policy_id: "policy:openkeys:official-1-to-1",
    policy_version: 1,
    owner_type: "open_keys" as const,
    owner_id: "official-1-to-1",
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    rules,
  };
  const base = {
    account_id: accountId,
    effective_version: 1,
    policy_id: source.policy_id,
    policy_version: source.policy_version,
    source_policy_digest: stage7OpenKeysDigest("official-source-policy", source),
    owner_type: source.owner_type,
    owner_id: source.owner_id,
    account_class: "open_keys" as const,
    product_id: source.product_id,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    catalog_generation: authority.catalog.generation,
    switch_generation: authority.switches.generation,
    replacement_locked: false,
    rules,
  };
  return {
    ...base,
    content_digest: stage7OpenKeysDigest("official-effective-policy", base),
  };
}

export function officialOpenKeysBinding(): AccountPolicyBinding {
  return {
    policy_enforcement: "shadow",
    funding_enforcement: "legacy_single",
    reconciliation_state: "pending",
  };
}

/**
 * The direct strict binding for new OpenKeys issuance in the release-retirement flow: the
 * account is born strict/strict/verified (zero keys and zero balance make the engine's atomic
 * strict preconditions vacuous at activation), the face-value credit allocates its funding
 * bucket, and the first key is issued with the exact activation ACK so the release opt-out
 * guard can mark the account immediately.
 */
export function officialOpenKeysStrictBinding(): AccountPolicyBinding {
  return {
    policy_enforcement: "strict",
    funding_enforcement: "strict",
    reconciliation_state: "verified",
  };
}
