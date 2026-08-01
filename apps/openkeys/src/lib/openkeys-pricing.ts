import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  CURRENT_PRODUCT_CATALOG_ENTRIES,
  MULTI_DISCOUNT_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type IssuedEngineApiKey,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";

export const OFFICIAL_ONE_TO_ONE_MULT_BP = 10_000;
export const OFFICIAL_ONE_TO_ONE_CONTRACT = "official_1_to_1" as const;

const OPENKEYS_PROVIDERS = ["anthropic", "openai"] as const;
const PRICING_OVERRIDE_FIELDS = new Set([
  "discount",
  "discount_bps",
  "discountBps",
  "mult_bp",
  "multBp",
  "multiplier",
  "multiplier_bp",
  "multiplierBp",
  "pricing_contract",
  "pricingContract",
]);

export class OpenKeysPricingError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "OpenKeysPricingError";
  }
}

/** API and direct service callers cannot smuggle an alternative economic contract. */
export function assertNoOpenKeysPricingOverride(input: object): void {
  const override = Object.keys(input).find((field) => PRICING_OVERRIDE_FIELDS.has(field));
  if (override !== undefined) {
    throw new OpenKeysPricingError(
      "pricing_override_forbidden",
      `OpenKeys pricing is fixed at 1:1; field ${override} is not accepted`,
    );
  }
}

export function assertOfficialEngineAccount(account: { account: string; multBp: number }): void {
  if (account.multBp !== OFFICIAL_ONE_TO_ONE_MULT_BP) {
    throw new OpenKeysPricingError(
      "engine_multiplier_mismatch",
      "engine did not create the OpenKeys account with the fixed 1:1 multiplier",
    );
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

function canonicalJson(value: unknown): string {
  return JSON.stringify(canonicalValue(value));
}

function digest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`multi-discount-stage7:${label}\n`, "utf8")
    .update(canonicalJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}

function catalogEntryKey(entry: { provider_id: string; canonical_model_id: string }): string {
  return `${entry.provider_id}\u0000${entry.canonical_model_id}`;
}

export interface OpenKeysPricingAuthority {
  catalog: PricingCatalogSpec;
  switches: ProviderSwitchSpec;
}

type OpenKeysPricingEngine = Pick<
  EngineClient,
  | "activateAccountPolicy"
  | "creditAccount"
  | "getAccountPricingState"
  | "getActiveAccountPolicy"
  | "getActivePricingCatalog"
  | "getActiveProviderSwitches"
  | "issueKey"
  | "prepareAccountPolicy"
>;

export function assertOpenKeysCatalog(catalog: PricingCatalogSpec): void {
  if (
    catalog.product_id !== OPENKEYS_PRICING_PRODUCT_ID ||
    catalog.schema_version !== MULTI_DISCOUNT_SCHEMA_VERSION ||
    catalog.capability_generation !== MULTI_DISCOUNT_CAPABILITY_GENERATION ||
    catalog.capability_digest !== MULTI_DISCOUNT_CAPABILITY_DIGEST
  ) {
    throw new OpenKeysPricingError(
      "catalog_identity_mismatch",
      "active OpenKeys catalog does not match the reviewed pricing capability",
    );
  }

  const expected = new Set(CURRENT_PRODUCT_CATALOG_ENTRIES.map(catalogEntryKey));
  const actual = new Set(catalog.entries.map(catalogEntryKey));
  if (
    catalog.entries.length !== expected.size ||
    actual.size !== expected.size ||
    catalog.entries.some((entry) => !entry.enabled || !expected.has(catalogEntryKey(entry)))
  ) {
    throw new OpenKeysPricingError(
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
    throw new OpenKeysPricingError(
      "switch_identity_mismatch",
      "active provider switches do not match the OpenKeys catalog capability",
    );
  }

  const productEntries = switches.entries.filter((entry) =>
    isProductScope(entry.scope) && entry.scope.product.product_id === OPENKEYS_PRICING_PRODUCT_ID
  );
  if (
    productEntries.length !== OPENKEYS_PROVIDERS.length ||
    productEntries.some((entry) =>
      !OPENKEYS_PROVIDERS.includes(entry.provider_id as (typeof OPENKEYS_PROVIDERS)[number]) ||
      !entry.enabled ||
      entry.catalog_generation !== catalog.generation
    )
  ) {
    throw new OpenKeysPricingError(
      "openkeys_provider_switch_mismatch",
      "OpenKeys requires only enabled Anthropic and OpenAI product switches",
    );
  }

  for (const provider of OPENKEYS_PROVIDERS) {
    const master = switches.entries.filter((entry) => entry.provider_id === provider && entry.scope === "master");
    const product = productEntries.filter((entry) => entry.provider_id === provider);
    if (master.length !== 1 || !master[0]!.enabled || product.length !== 1) {
      throw new OpenKeysPricingError(
        "openkeys_provider_disabled",
        `OpenKeys provider ${provider} is not enabled at both master and product scope`,
      );
    }
  }
}

/** Read-only authority check: this application never invents or overwrites global switches. */
export async function resolveOpenKeysPricingAuthority(
  engine: OpenKeysPricingEngine,
): Promise<OpenKeysPricingAuthority> {
  const [catalog, switches] = await Promise.all([
    engine.getActivePricingCatalog(OPENKEYS_PRICING_PRODUCT_ID),
    engine.getActiveProviderSwitches(),
  ]);
  if (catalog === null || switches === null) {
    throw new OpenKeysPricingError(
      "pricing_authority_missing",
      "OpenKeys product catalog and provider switches must be active before issuance",
    );
  }
  assertOpenKeysCatalog(catalog);
  assertOpenKeysSwitches(switches, catalog);
  return { catalog, switches };
}

function officialRule(providerId: (typeof OPENKEYS_PROVIDERS)[number]): AccountPolicySpec["rules"][number] {
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
  return { ...base, rule_digest: digest("official-rule", base) };
}

export function buildOfficialOpenKeysPolicy(
  accountId: string,
  authority: OpenKeysPricingAuthority,
): AccountPolicySpec {
  assertOpenKeysCatalog(authority.catalog);
  assertOpenKeysSwitches(authority.switches, authority.catalog);
  const rules = OPENKEYS_PROVIDERS.map(officialRule);
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
    source_policy_digest: digest("official-source-policy", source),
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
  return { ...base, content_digest: digest("official-effective-policy", base) };
}

function officialBinding(): AccountPolicyBinding {
  return {
    policy_enforcement: "shadow",
    funding_enforcement: "legacy_single",
    reconciliation_state: "pending",
  };
}

function assertMutationAccepted(ack: { result: string }, phase: string): void {
  if (ack.result === "rejected") {
    throw new OpenKeysPricingError(
      "policy_ack_rejected",
      `engine rejected the OpenKeys ${phase} policy ACK`,
    );
  }
}

function policyMatches(
  observed: { policy: AccountPolicySpec; binding: AccountPolicyBinding } | null,
  policy: AccountPolicySpec,
  binding: AccountPolicyBinding,
): boolean {
  return observed !== null &&
    canonicalJson(observed.policy) === canonicalJson(policy) &&
    canonicalJson(observed.binding) === canonicalJson(binding);
}

/** Exact prepare/activate/readback ACK. No credit or secret exists before this returns. */
export async function activateOfficialOpenKeysPolicy(
  engine: OpenKeysPricingEngine,
  accountId: string,
  authority: OpenKeysPricingAuthority,
): Promise<AccountPolicySpec> {
  const policy = buildOfficialOpenKeysPolicy(accountId, authority);
  const binding = officialBinding();
  const prepared = await engine.prepareAccountPolicy(policy);
  assertMutationAccepted(prepared, "prepare");

  const state = await engine.getAccountPricingState(accountId);
  if (typeof state === "object" && "active" in state) {
    const active = await engine.getActiveAccountPolicy(accountId);
    if (policyMatches(active, policy, binding)) return policy;
    throw new OpenKeysPricingError(
      "account_policy_already_bound",
      "new OpenKeys account is already bound to a different policy",
    );
  }
  if (state !== "unbound") {
    throw new OpenKeysPricingError(
      "account_policy_not_unbound",
      "new OpenKeys account has an unexpected inactive policy binding",
    );
  }

  const activated = await engine.activateAccountPolicy(policy, binding, "unbound");
  assertMutationAccepted(activated, "activation");
  const active = await engine.getActiveAccountPolicy(accountId);
  if (!policyMatches(active, policy, binding)) {
    throw new OpenKeysPricingError(
      "policy_ack_readback_mismatch",
      "engine policy ACK did not durably read back with the exact requested identity",
    );
  }
  return policy;
}

/** Policy ACK precedes exact face-value funding, and funding precedes the usable secret. */
export async function provisionOfficialOpenKeysCredential(
  engine: OpenKeysPricingEngine,
  input: {
    accountId: string;
    authority: OpenKeysPricingAuthority;
    faceValueNano: bigint;
    creditReference: string;
    keyLabel: string;
    onCredited?: () => Promise<void>;
  },
): Promise<IssuedEngineApiKey> {
  if (input.faceValueNano <= 0n) {
    throw new OpenKeysPricingError("invalid_face_value", "OpenKeys face value must be positive");
  }
  await activateOfficialOpenKeysPolicy(engine, input.accountId, input.authority);
  await engine.creditAccount(input.accountId, input.faceValueNano, input.creditReference);
  await input.onCredited?.();
  return engine.issueKey(input.accountId, { label: input.keyLabel });
}
