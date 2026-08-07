import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  MAIN_PRICING_PRODUCT_ID,
  MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
  MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
  MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES,
  MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES,
  MULTI_DISCOUNT_SCHEMA_VERSION,
  OPENKEYS_PRICING_PRODUCT_ID,
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  openKeysPricingInventoryPageV2Schema,
  pricingCatalogSpecSchema,
  pricingReleasePolicyV2Schema,
  providerSwitchSpecSchema,
  serviceAccountInventoryEntryV2Schema,
  serviceAccountInventoryV2Schema,
  type OpenKeysPricingInventoryAccountV2,
  type OpenKeysPricingInventoryPageV2,
  type PricingCatalogSpec,
  type PricingReleaseHeadV2,
  type PricingReleaseInventoryAccountV2,
  type PricingReleasePolicyV2,
  type ProviderSwitchSpec,
  type ServiceAccountInventoryEntryV2,
  type ServiceAccountInventoryV2,
} from "@claude-api/contracts";
import {
  canonicalPricingReleaseV2Json,
  pricingReleaseV2Digest,
  type EngineClient,
} from "@claude-api/engine-client";

export const STAGE5_V2_CATALOG_GENERATION = 7;
export const STAGE5_V2_SWITCH_GENERATION = 7;
export const STAGE5_V2_POLICY_VERSION = 4;

export type Stage5V2EngineReader = Pick<
  EngineClient,
  | "getPricingReleaseInventoryV2"
  | "getPricingReleaseHeadV2"
  | "getPricingReleaseV2"
>;

export interface Stage5V2OpenKeysReader {
  getPage(options: { afterAccountId?: string; limit: number }): Promise<OpenKeysPricingInventoryPageV2>;
}

export interface Stage5V2B2bPolicyHeadRule {
  scope_type: "provider" | "model";
  provider_id: string;
  canonical_model_id: string | null;
  pricing_mode: string;
  payable_multiplier_bp: number;
}

export interface Stage5V2CommerceAccount {
  user_id: string;
  engine_account_record_id: string;
  engine_account_id: string;
  account_class: "b2c" | "b2b";
  profile_multiplier_bp: number;
  commerce_multiplier_bp: number;
  commerce_status: "pending" | "active" | "error" | "disabled";
  policy_rules: Stage5V2B2bPolicyHeadRule[] | null;
}

export interface Stage5V2BusinessInvitation {
  invite_id: string;
  multiplier_bp: number;
  expires_at: string;
}

export interface Stage5V2ExistingReleasePolicy {
  policy_id: string;
  policy_version: number;
  content_digest: string;
}

export interface Stage5V2CommerceSnapshot {
  accounts: Stage5V2CommerceAccount[];
  invitations: Stage5V2BusinessInvitation[];
}

export interface Stage5V2EngineScan {
  accounts: PricingReleaseInventoryAccountV2[];
  identity_digest: string;
  full_digest: string;
}

export interface Stage5V2OpenKeysScan {
  accounts: OpenKeysPricingInventoryAccountV2[];
  inventory_digest: string;
}

export type Stage5V2BlockerContext =
  | "commerce"
  | "engine"
  | "openkeys"
  | "service"
  | "funding"
  | "release";

export interface Stage5V2Blocker {
  blocker_code: string;
  blocker_context: Stage5V2BlockerContext;
  subject_id: string;
  detail: string;
  blocker_digest: string;
}

export interface Stage5V2CapabilityProjection {
  generation: number;
  schema_version: number;
  content_digest: string;
  entries: Array<{
    provider_id: string;
    canonical_model_id: string;
    enabled: true;
    entry_digest: string;
    capability_data: { pricing_supported: true };
  }>;
  aliases: Array<{
    provider_id: string;
    alias_model_id: string;
    canonical_model_id: string;
  }>;
}

export interface Stage5V2PlannedAssignment {
  release_generation: number;
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  owner_id: string;
  policy_id: string;
  policy_version: number;
  policy_digest: string;
  billing_mode: "balance" | "meter_only";
  funding_generation: null;
  purpose: string | null;
  responsible: string | null;
  assignment_digest: string;
}

export interface Stage5V2ReleasePlan {
  generation: number;
  release_kind: "target" | "recovery";
  schema_version: 2;
  commerce_inventory_digest: string;
  engine_inventory_digest: string;
  openkeys_inventory_digest: string;
  service_inventory_digest: string;
  policy_manifest_digest: string;
  assignment_manifest_digest: string;
  funding_manifest_digest: null;
  engine_release_digest: null;
  content_digest: string;
  assignments: Stage5V2PlannedAssignment[];
}

export interface Stage5V2Plan {
  schema_version: 2;
  commerce_inventory_digest: string;
  engine_scan_first_digest: string;
  engine_scan_second_digest: string;
  openkeys_scan_first_digest: string;
  openkeys_scan_second_digest: string;
  service_inventory_digest: string;
  funding_plan_digest: string;
  target_generation: number;
  target_digest: null;
  recovery_generation: number;
  recovery_digest: null;
  capability: Stage5V2CapabilityProjection;
  catalogs: [PricingCatalogSpec, PricingCatalogSpec];
  switches: ProviderSwitchSpec;
  policies: PricingReleasePolicyV2[];
  invitation_snapshots: Array<{
    invite_id: string;
    policy_id: string;
    policy_version: number;
    policy_digest: string;
    snapshot_digest: string;
  }>;
  target: Stage5V2ReleasePlan;
  recovery: Stage5V2ReleasePlan;
  blockers: Stage5V2Blocker[];
  inventory_artifact: Record<string, unknown>;
  plan_digest: string;
}

export class Stage5MaterializerV2Error extends Error {
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "Stage5MaterializerV2Error";
  }
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

export function stage5V2CanonicalJson(value: unknown): string {
  return canonicalPricingReleaseV2Json(value);
}

export function stage5V2Digest(label: string, value: unknown): string {
  return pricingReleaseV2Digest(label, value);
}

function legacyStage5Digest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`multi-discount-stage5:${label}\n`, "utf8")
    .update(stage5V2CanonicalJson(value), "utf8")
    .digest("hex");
  return `sha256:v1:${hex}`;
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return stage5V2CanonicalJson(left) === stage5V2CanonicalJson(right);
}

function sortedByAccount<T extends { account_id: string }>(accounts: readonly T[]): T[] {
  return [...accounts].sort((left, right) => compareUtf8(left.account_id, right.account_id));
}

function assertStrictAccountOrder<T extends { account_id: string }>(
  accounts: readonly T[],
  label: string,
): void {
  for (let index = 1; index < accounts.length; index += 1) {
    if (compareUtf8(accounts[index - 1]!.account_id, accounts[index]!.account_id) >= 0) {
      throw new Stage5MaterializerV2Error(
        `${label}_order_invalid`,
        `${label} cursor returned duplicate or non-increasing account ids`,
      );
    }
  }
}

export function stage5V2EngineIdentityDigest(
  accounts: readonly PricingReleaseInventoryAccountV2[],
): string {
  return stage5V2Digest("engine-identity-inventory", sortedByAccount(accounts).map((account) => ({
    account_id: account.account_id,
    status: account.status,
    multiplier_bp: account.multiplier_bp,
  })));
}

export function stage5V2EngineFullDigest(
  accounts: readonly PricingReleaseInventoryAccountV2[],
): string {
  return stage5V2Digest("engine-full-inventory", sortedByAccount(accounts));
}

export async function scanStage5EngineInventoryV2(
  engine: Pick<EngineClient, "getPricingReleaseInventoryV2">,
): Promise<Stage5V2EngineScan> {
  const accounts: PricingReleaseInventoryAccountV2[] = [];
  const seenCursors = new Set<string>();
  let afterAccountId: string | undefined;
  for (;;) {
    const page = await engine.getPricingReleaseInventoryV2({
      ...(afterAccountId === undefined ? {} : { afterAccountId }),
      limit: 500,
    });
    assertStrictAccountOrder(page.accounts, "engine_inventory");
    if (afterAccountId !== undefined && page.accounts[0]
        && compareUtf8(page.accounts[0].account_id, afterAccountId) <= 0) {
      throw new Stage5MaterializerV2Error(
        "engine_inventory_cursor_regressed",
        "engine inventory page did not advance beyond its cursor",
      );
    }
    accounts.push(...page.accounts);
    if (page.next_after_account_id === null) break;
    if (page.accounts.length === 0 || seenCursors.has(page.next_after_account_id)) {
      throw new Stage5MaterializerV2Error(
        "engine_inventory_cursor_loop",
        "engine inventory cursor is empty or repeated before exhaustion",
      );
    }
    seenCursors.add(page.next_after_account_id);
    afterAccountId = page.next_after_account_id;
  }
  assertStrictAccountOrder(accounts, "engine_inventory");
  return {
    accounts,
    identity_digest: stage5V2EngineIdentityDigest(accounts),
    full_digest: stage5V2EngineFullDigest(accounts),
  };
}

export async function scanStage5OpenKeysInventoryV2(
  openkeys: Stage5V2OpenKeysReader,
): Promise<Stage5V2OpenKeysScan> {
  const accounts: OpenKeysPricingInventoryAccountV2[] = [];
  const seenCursors = new Set<string>();
  let expectedDigest: string | undefined;
  let afterAccountId: string | undefined;
  for (;;) {
    const page = openKeysPricingInventoryPageV2Schema.parse(await openkeys.getPage({
      ...(afterAccountId === undefined ? {} : { afterAccountId }),
      limit: 500,
    }));
    if (expectedDigest === undefined) expectedDigest = page.inventory_digest;
    if (page.inventory_digest !== expectedDigest) {
      throw new Stage5MaterializerV2Error(
        "openkeys_manifest_changed_during_scan",
        "OpenKeys full-manifest digest changed before cursor exhaustion",
      );
    }
    assertStrictAccountOrder(page.accounts, "openkeys_inventory");
    if (afterAccountId !== undefined && page.accounts[0]
        && compareUtf8(page.accounts[0].account_id, afterAccountId) <= 0) {
      throw new Stage5MaterializerV2Error(
        "openkeys_inventory_cursor_regressed",
        "OpenKeys inventory page did not advance beyond its cursor",
      );
    }
    accounts.push(...page.accounts);
    if (page.next_after_account_id === null) break;
    if (page.accounts.length === 0 || seenCursors.has(page.next_after_account_id)) {
      throw new Stage5MaterializerV2Error(
        "openkeys_inventory_cursor_loop",
        "OpenKeys inventory cursor is empty or repeated before exhaustion",
      );
    }
    seenCursors.add(page.next_after_account_id);
    afterAccountId = page.next_after_account_id;
  }
  assertStrictAccountOrder(accounts, "openkeys_inventory");
  return { accounts, inventory_digest: expectedDigest! };
}

export function createStage5OpenKeysInventoryReaderV2(input: {
  baseUrl: string;
  controlKey: string;
  fetch?: typeof globalThis.fetch;
}): Stage5V2OpenKeysReader {
  const baseUrl = new URL(input.baseUrl);
  if (baseUrl.username || baseUrl.password || baseUrl.search || baseUrl.hash) {
    throw new TypeError("OpenKeys internal base URL must not contain credentials, query or fragment");
  }
  const fetchImpl = input.fetch ?? globalThis.fetch;
  return {
    async getPage(options): Promise<OpenKeysPricingInventoryPageV2> {
      const url = new URL("/api/internal/pricing/v2/inventory", baseUrl);
      url.searchParams.set("limit", String(options.limit));
      if (options.afterAccountId !== undefined) {
        url.searchParams.set("after_account_id", options.afterAccountId);
      }
      let response: Response;
      try {
        response = await fetchImpl(url, {
          headers: {
            accept: "application/json",
            "x-openkeys-control-key": input.controlKey,
          },
        });
      } catch {
        throw new Stage5MaterializerV2Error(
          "openkeys_inventory_unavailable",
          "OpenKeys inventory request failed",
        );
      }
      if (!response.ok) {
        throw new Stage5MaterializerV2Error(
          "openkeys_inventory_unavailable",
          `OpenKeys inventory returned HTTP ${response.status}`,
        );
      }
      let payload: unknown;
      try {
        payload = await response.json() as unknown;
      } catch {
        throw new Stage5MaterializerV2Error(
          "openkeys_inventory_malformed",
          "OpenKeys inventory response is not valid JSON",
        );
      }
      if (payload === null || typeof payload !== "object" || !("inventory" in payload)) {
        throw new Stage5MaterializerV2Error(
          "openkeys_inventory_malformed",
          "OpenKeys inventory response has no strict inventory envelope",
        );
      }
      const parsed = openKeysPricingInventoryPageV2Schema.safeParse(
        (payload as { inventory: unknown }).inventory,
      );
      if (!parsed.success) {
        throw new Stage5MaterializerV2Error(
          "openkeys_inventory_malformed",
          "OpenKeys inventory response does not match the strict contract",
        );
      }
      return parsed.data;
    },
  };
}

function capabilityEntry(
  entry: { provider_id: string; canonical_model_id: string; enabled: boolean },
): Stage5V2CapabilityProjection["entries"][number] {
  const capabilityData = { pricing_supported: true as const };
  return {
    provider_id: entry.provider_id,
    canonical_model_id: entry.canonical_model_id,
    enabled: true,
    entry_digest: legacyStage5Digest("capability-entry", {
      ...entry,
      capability_data: capabilityData,
    }),
    capability_data: capabilityData,
  };
}

export function buildStage5V2Capability(): Stage5V2CapabilityProjection {
  const base = {
    generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    entries: MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES.map(capabilityEntry),
    aliases: [
      {
        provider_id: "anthropic",
        alias_model_id: "claude-haiku-4-5-20251001",
        canonical_model_id: "claude-haiku-4-5",
      },
      {
        provider_id: "anthropic",
        alias_model_id: "claude-opus-4-5-20251101",
        canonical_model_id: "claude-opus-4-5",
      },
      {
        provider_id: "anthropic",
        alias_model_id: "claude-sonnet-4-5-20250929",
        canonical_model_id: "claude-sonnet-4-5",
      },
      {
        provider_id: "openai",
        alias_model_id: "gpt-5.6",
        canonical_model_id: "gpt-5.6-sol",
      },
      {
        provider_id: "openai",
        alias_model_id: "gpt-image-2",
        canonical_model_id: "gpt-image-2-2026-04-21",
      },
    ],
  };
  const contentDigest = legacyStage5Digest("capability", base);
  if (contentDigest !== MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST) {
    throw new Stage5MaterializerV2Error(
      "target_capability_digest_drift",
      "target capability projection differs from its reviewed digest",
    );
  }
  return { ...base, content_digest: contentDigest };
}

function buildCatalog(
  productId: string,
  entries: readonly { provider_id: string; canonical_model_id: string; enabled: boolean }[],
): PricingCatalogSpec {
  const normalizedEntries = [...entries].sort((left, right) =>
    compareUtf8(left.provider_id, right.provider_id)
    || compareUtf8(left.canonical_model_id, right.canonical_model_id));
  const base = {
    product_id: productId,
    generation: STAGE5_V2_CATALOG_GENERATION,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
    entries: normalizedEntries.map((entry) => ({ ...entry })),
  };
  return pricingCatalogSpecSchema.parse({
    ...base,
    content_digest: legacyStage5Digest("catalog", base),
  });
}

function switchParts(scope: ProviderSwitchSpec["entries"][number]["scope"]): string[] {
  if (scope === "master") return ["master", "", ""];
  if ("product" in scope) return ["product", scope.product.product_id, ""];
  return ["segment", scope.segment.product_id, scope.segment.segment];
}

function sortSwitches(entries: ProviderSwitchSpec["entries"]): ProviderSwitchSpec["entries"] {
  return [...entries].sort((left, right) => {
    const leftParts = [left.provider_id, ...switchParts(left.scope)];
    const rightParts = [right.provider_id, ...switchParts(right.scope)];
    for (let index = 0; index < leftParts.length; index += 1) {
      const order = compareUtf8(leftParts[index]!, rightParts[index]!);
      if (order !== 0) return order;
    }
    return 0;
  });
}

export function buildStage5V2CatalogsAndSwitches(): {
  catalogs: [PricingCatalogSpec, PricingCatalogSpec];
  switches: ProviderSwitchSpec;
} {
  const catalogs: [PricingCatalogSpec, PricingCatalogSpec] = [
    buildCatalog(MAIN_PRICING_PRODUCT_ID, MULTI_DISCOUNT_GEN7_MAIN_CATALOG_ENTRIES),
    buildCatalog(OPENKEYS_PRICING_PRODUCT_ID, MULTI_DISCOUNT_GEN7_OPENKEYS_CATALOG_ENTRIES),
  ];
  const entries: ProviderSwitchSpec["entries"] = [];
  for (const providerId of ["anthropic", "openai", "google"] as const) {
    entries.push(
      { provider_id: providerId, scope: "master", catalog_generation: null, enabled: true },
      {
        provider_id: providerId,
        scope: { product: { product_id: MAIN_PRICING_PRODUCT_ID } },
        catalog_generation: STAGE5_V2_CATALOG_GENERATION,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: MAIN_PRICING_PRODUCT_ID, segment: "b2c" } },
        catalog_generation: STAGE5_V2_CATALOG_GENERATION,
        enabled: true,
      },
      {
        provider_id: providerId,
        scope: { segment: { product_id: MAIN_PRICING_PRODUCT_ID, segment: "b2b" } },
        catalog_generation: STAGE5_V2_CATALOG_GENERATION,
        enabled: true,
      },
    );
    if (providerId !== "google") {
      entries.push({
        provider_id: providerId,
        scope: { product: { product_id: OPENKEYS_PRICING_PRODUCT_ID } },
        catalog_generation: STAGE5_V2_CATALOG_GENERATION,
        enabled: true,
      });
    }
  }
  const base = {
    generation: STAGE5_V2_SWITCH_GENERATION,
    schema_version: MULTI_DISCOUNT_SCHEMA_VERSION,
    capability_generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
    entries: sortSwitches(entries),
  };
  const switches = providerSwitchSpecSchema.parse({
    ...base,
    content_digest: legacyStage5Digest("switches", base),
  });
  return { catalogs, switches };
}

function policyRule(input: {
  rule_id: string;
  scope: { scope: "global" }
    | { scope: "provider"; provider_id: string }
    | { scope: "model"; provider_id: string; canonical_model_id: string };
  discount_bps: number;
}): PricingReleasePolicyV2["rules"][number] {
  const base = {
    rule_id: input.rule_id,
    scope: input.scope,
    discount_bps: input.discount_bps,
    payable_multiplier_bp: 10_000 - input.discount_bps,
  };
  return { ...base, rule_digest: stage5V2Digest("policy-rule", base) };
}

function buildPolicy(input: Omit<PricingReleasePolicyV2, "content_digest">): PricingReleasePolicyV2 {
  const normalized = {
    ...input,
    rules: [...input.rules].sort((left, right) =>
      compareUtf8(stage5V2CanonicalJson(left.scope), stage5V2CanonicalJson(right.scope))
      || compareUtf8(left.rule_id, right.rule_id)),
  };
  return pricingReleasePolicyV2Schema.parse({
    ...normalized,
    content_digest: stage5V2Digest("policy", normalized),
  });
}

function customerPolicyBase(
  catalog: PricingCatalogSpec,
  switches: ProviderSwitchSpec,
): Pick<
  PricingReleasePolicyV2,
  | "billing_mode"
  | "schema_version"
  | "capability_generation"
  | "capability_digest"
  | "catalog_generation"
  | "catalog_digest"
  | "switch_generation"
  | "switch_digest"
> {
  return {
    billing_mode: "balance",
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    capability_generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
    capability_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
    catalog_generation: catalog.generation,
    catalog_digest: catalog.content_digest,
    switch_generation: switches.generation,
    switch_digest: switches.content_digest,
  };
}

function policyForB2b(
  policyId: string,
  ownerId: string,
  multiplierBp: number,
  mainCatalog: PricingCatalogSpec,
  switches: ProviderSwitchSpec,
  headRules?: readonly Stage5V2B2bPolicyHeadRule[] | null,
): PricingReleasePolicyV2 {
  const rules = headRules && headRules.length > 0
    ? headRules.map((rule) => policyRule({
      rule_id: rule.scope_type === "provider"
        ? `provider:${rule.provider_id}`
        : `model:${rule.provider_id}:${rule.canonical_model_id}`,
      scope: rule.scope_type === "provider"
        ? { scope: "provider" as const, provider_id: rule.provider_id }
        : {
          scope: "model" as const,
          provider_id: rule.provider_id,
          canonical_model_id: rule.canonical_model_id!,
        },
      discount_bps: 10_000 - rule.payable_multiplier_bp,
    }))
    : [policyRule({
      rule_id: "anthropic",
      scope: { scope: "provider", provider_id: "anthropic" },
      discount_bps: 10_000 - multiplierBp,
    })];
  return buildPolicy({
    policy_id: policyId,
    policy_version: STAGE5_V2_POLICY_VERSION,
    owner_type: "b2b_client",
    owner_id: ownerId,
    account_class: "b2b",
    product_id: MAIN_PRICING_PRODUCT_ID,
    ...customerPolicyBase(mainCatalog, switches),
    rules,
  });
}

function resolveReleasePolicyVersion(
  policy: PricingReleasePolicyV2,
  existing: readonly Stage5V2ExistingReleasePolicy[],
): PricingReleasePolicyV2 {
  if (policy.policy_version !== STAGE5_V2_POLICY_VERSION) return policy;
  const rows = existing
    .filter((row) => row.policy_id === policy.policy_id)
    .sort((left, right) => right.policy_version - left.policy_version);
  const { content_digest: _contentDigest, ...policyWithoutDigest } = policy;
  if (rows.length === 0) return policy;
  const newest = rows[0]!;
  const candidate = buildPolicy({ ...policyWithoutDigest, policy_version: newest.policy_version });
  if (candidate.content_digest === newest.content_digest) return candidate;
  const next = newest.policy_version + 1;
  return buildPolicy({ ...policyWithoutDigest, policy_version: next });
}

function blocker(
  blockerCode: string,
  blockerContext: Stage5V2BlockerContext,
  subjectId: string,
  detail: string,
): Stage5V2Blocker {
  const base = {
    blocker_code: blockerCode,
    blocker_context: blockerContext,
    subject_id: subjectId,
    detail,
  };
  return { ...base, blocker_digest: stage5V2Digest("blocker", base) };
}

function serviceManifest(accounts: readonly ServiceAccountInventoryEntryV2[]): ServiceAccountInventoryV2 {
  const sorted = [...accounts].sort((left, right) => compareUtf8(left.service_id, right.service_id));
  const identity = {
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    accounts: sorted,
  };
  return serviceAccountInventoryV2Schema.parse({
    ...identity,
    inventory_digest: createHash("sha256")
      .update("pricing-service-account-inventory-v2:manifest\n", "utf8")
      .update(stage5V2CanonicalJson(identity), "utf8")
      .digest("hex")
      .replace(/^/, "sha256:v2:"),
  });
}

export function buildStage5ServiceInventoryV2(
  accounts: readonly ServiceAccountInventoryEntryV2[],
): ServiceAccountInventoryV2 {
  return serviceManifest(accounts.map((account) => serviceAccountInventoryEntryV2Schema.parse(account)));
}

export function stage5V2CommerceInventoryDigest(
  snapshot: Stage5V2CommerceSnapshot,
): string {
  return stage5V2Digest("commerce-inventory", {
    accounts: [...snapshot.accounts]
      .sort((left, right) => compareUtf8(left.engine_account_id, right.engine_account_id)),
    invitations: [...snapshot.invitations]
      .sort((left, right) => compareUtf8(left.invite_id, right.invite_id)),
  });
}

function assignment(input: Omit<Stage5V2PlannedAssignment, "assignment_digest">): Stage5V2PlannedAssignment {
  return {
    ...input,
    assignment_digest: stage5V2Digest("assignment", input),
  };
}

function releasePlan(input: Omit<Stage5V2ReleasePlan, "content_digest" | "assignment_manifest_digest">): Stage5V2ReleasePlan {
  const assignments = [...input.assignments]
    .sort((left, right) => compareUtf8(left.engine_account_id, right.engine_account_id));
  const assignmentManifestDigest = stage5V2Digest("assignment-manifest", assignments);
  const base = {
    ...input,
    assignments,
    assignment_manifest_digest: assignmentManifestDigest,
  };
  return { ...base, content_digest: stage5V2Digest("release-plan", base) };
}

function validMultiplier(value: number): boolean {
  return Number.isSafeInteger(value) && value >= 0 && value <= 10_000;
}

export function buildStage5V2Plan(input: {
  commerce: Stage5V2CommerceSnapshot;
  service: ServiceAccountInventoryV2;
  existing_release_policies?: Stage5V2ExistingReleasePolicy[];
  engine_first: Stage5V2EngineScan;
  engine_second: Stage5V2EngineScan;
  openkeys_first: Stage5V2OpenKeysScan;
  openkeys_second: Stage5V2OpenKeysScan;
  head_first: PricingReleaseHeadV2 | null;
  head_second: PricingReleaseHeadV2 | null;
  target_generation: number;
  recovery_generation: number;
  occupied_generations?: number[];
}): Stage5V2Plan {
  if (!Number.isSafeInteger(input.target_generation) || input.target_generation <= 0
      || !Number.isSafeInteger(input.recovery_generation)
      || input.recovery_generation !== input.target_generation + 1) {
    throw new Stage5MaterializerV2Error(
      "release_generation_invalid",
      "Stage 5 requires adjacent positive target and recovery generations",
    );
  }
  const capability = buildStage5V2Capability();
  const { catalogs, switches } = buildStage5V2CatalogsAndSwitches();
  const mainCatalog = catalogs[0];
  const openkeysCatalog = catalogs[1];
  const commerceAccounts = [...input.commerce.accounts]
    .sort((left, right) => compareUtf8(left.engine_account_id, right.engine_account_id));
  const invitations = [...input.commerce.invitations]
    .sort((left, right) => compareUtf8(left.invite_id, right.invite_id));
  const serviceAccounts = [...input.service.accounts]
    .sort((left, right) => compareUtf8(left.service_id, right.service_id));
  const engineAccounts = sortedByAccount(input.engine_second.accounts);
  const openkeysAccounts = sortedByAccount(input.openkeys_second.accounts);
  const commerceInventoryDigest = stage5V2CommerceInventoryDigest({
    accounts: commerceAccounts,
    invitations,
  });
  const blockers: Stage5V2Blocker[] = [];

  if (input.engine_first.identity_digest !== input.engine_second.identity_digest
      || !sameCanonical(
        sortedByAccount(input.engine_first.accounts).map(({ account_id, status, multiplier_bp }) =>
          ({ account_id, status, multiplier_bp })),
        engineAccounts.map(({ account_id, status, multiplier_bp }) =>
          ({ account_id, status, multiplier_bp })),
      )) {
    blockers.push(blocker(
      "engine_inventory_changed_between_scans",
      "engine",
      "full-inventory",
      `${input.engine_first.identity_digest} -> ${input.engine_second.identity_digest}`,
    ));
  }
  if (input.openkeys_first.inventory_digest !== input.openkeys_second.inventory_digest
      || !sameCanonical(input.openkeys_first.accounts, input.openkeys_second.accounts)) {
    blockers.push(blocker(
      "openkeys_inventory_changed_between_scans",
      "openkeys",
      "full-inventory",
      `${input.openkeys_first.inventory_digest} -> ${input.openkeys_second.inventory_digest}`,
    ));
  }
  if (!sameCanonical(input.head_first, input.head_second)) {
    blockers.push(blocker(
      "release_head_changed_during_plan",
      "release",
      "global-head",
      "global release head changed while Stage 5 inventories were collected",
    ));
  }
  if (input.head_second && input.target_generation <= input.head_second.active_generation) {
    blockers.push(blocker(
      "release_generation_not_monotonic",
      "release",
      String(input.target_generation),
      `target must be newer than active generation ${input.head_second.active_generation}`,
    ));
  }
  for (const generation of input.occupied_generations ?? []) {
    blockers.push(blocker(
      "release_generation_already_occupied",
      "release",
      String(generation),
      "engine already contains an immutable release at the reserved generation",
    ));
  }
  if (engineAccounts.length === 0) {
    blockers.push(blocker(
      "engine_inventory_empty",
      "engine",
      "full-inventory",
      "a pricing release cannot be built without engine accounts",
    ));
  }

  type Claim = { context: "commerce" | "openkeys" | "service"; owner_id: string };
  const claims = new Map<string, Claim[]>();
  const addClaim = (accountId: string, claim: Claim): void => {
    const current = claims.get(accountId) ?? [];
    current.push(claim);
    claims.set(accountId, current);
  };
  for (const account of commerceAccounts) addClaim(account.engine_account_id, {
    context: "commerce",
    owner_id: account.user_id,
  });
  for (const account of openkeysAccounts) addClaim(account.account_id, {
    context: "openkeys",
    owner_id: account.source_id,
  });
  for (const account of serviceAccounts) addClaim(account.engine_account_id, {
    context: "service",
    owner_id: account.service_id,
  });

  const engineById = new Map(engineAccounts.map((account) => [account.account_id, account]));
  for (const account of engineAccounts) {
    const accountClaims = claims.get(account.account_id) ?? [];
    if (accountClaims.length === 0) {
      blockers.push(blocker(
        "engine_account_missing_owner",
        "engine",
        account.account_id,
        `engine ${account.status} account is absent from commerce, OpenKeys and service authorities`,
      ));
    } else if (accountClaims.length > 1) {
      blockers.push(blocker(
        "engine_account_owner_collision",
        "engine",
        account.account_id,
        `engine account is claimed by ${accountClaims.map((claim) => claim.context).join(",")}`,
      ));
    }
  }
  for (const [accountId, accountClaims] of claims) {
    if (!engineById.has(accountId)) {
      for (const claim of accountClaims) {
        blockers.push(blocker(
          "owner_account_missing_from_engine",
          claim.context,
          accountId,
          `${claim.context} authority references an account absent from full engine inventory`,
        ));
      }
    }
  }

  const policies: PricingReleasePolicyV2[] = [];
  const existingPolicies = input.existing_release_policies ?? [];
  const resolvePolicy = (policy: PricingReleasePolicyV2): PricingReleasePolicyV2 =>
    resolveReleasePolicyVersion(policy, existingPolicies);
  const b2cPolicy = resolvePolicy(buildPolicy({
    policy_id: "release-v2:b2c:global",
    policy_version: STAGE5_V2_POLICY_VERSION,
    owner_type: "global_b2c",
    owner_id: "global",
    account_class: "b2c",
    product_id: MAIN_PRICING_PRODUCT_ID,
    ...customerPolicyBase(mainCatalog, switches),
    rules: [policyRule({
      rule_id: "global-default",
      scope: { scope: "global" },
      discount_bps: 5_000,
    })],
  }));
  policies.push(b2cPolicy);
  const openkeysPolicy = resolvePolicy(buildPolicy({
    policy_id: "release-v2:openkeys:global",
    policy_version: STAGE5_V2_POLICY_VERSION,
    owner_type: "open_keys",
    owner_id: "openkeys",
    account_class: "open_keys",
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    ...customerPolicyBase(openkeysCatalog, switches),
    rules: [policyRule({
      rule_id: "global-one-to-one",
      scope: { scope: "global" },
      discount_bps: 0,
    })],
  }));
  policies.push(openkeysPolicy);

  const policyByAccount = new Map<string, PricingReleasePolicyV2>();
  for (const account of commerceAccounts) {
    const engine = engineById.get(account.engine_account_id);
    if ((account.commerce_status !== "active" && account.commerce_status !== "disabled")
        || (engine && account.commerce_status !== engine.status)) {
      blockers.push(blocker(
        "commerce_status_drift",
        "commerce",
        account.engine_account_id,
        `commerce ${account.commerce_status} account maps to engine ${engine?.status ?? "missing"}`,
      ));
    }
    if (account.account_class === "b2c") {
      policyByAccount.set(account.engine_account_id, b2cPolicy);
      continue;
    }
    if (!validMultiplier(account.profile_multiplier_bp)
        || !validMultiplier(account.commerce_multiplier_bp)
        || (engine && !validMultiplier(engine.multiplier_bp))) {
      blockers.push(blocker(
        "b2b_multiplier_out_of_range",
        "commerce",
        account.engine_account_id,
        "B2B multiplier must be one exact integer in 0..10000 basis points",
      ));
      continue;
    }
    if (engine && (account.profile_multiplier_bp !== account.commerce_multiplier_bp
        || account.commerce_multiplier_bp !== engine.multiplier_bp)) {
      blockers.push(blocker(
        "b2b_multiplier_source_mismatch",
        "commerce",
        account.engine_account_id,
        `profile/commerce/engine multipliers are ${account.profile_multiplier_bp}/${account.commerce_multiplier_bp}/${engine.multiplier_bp}`,
      ));
      continue;
    }
    const headRules = account.account_class === "b2b" ? account.policy_rules : null;
    if (headRules) {
      const unsupported = headRules.find((rule) =>
        rule.pricing_mode !== "discount"
        || !validMultiplier(rule.payable_multiplier_bp)
        || (rule.scope_type !== "provider" && rule.scope_type !== "model")
        || (rule.scope_type === "model" && !rule.canonical_model_id));
      if (unsupported) {
        blockers.push(blocker(
          "b2b_policy_rule_unsupported",
          "commerce",
          account.engine_account_id,
          "B2B policy head carries a rule the release-v2 target cannot express: "
            + `${unsupported.scope_type}:${unsupported.provider_id}:${unsupported.canonical_model_id ?? ""}`
            + ` mode=${unsupported.pricing_mode} payable_bp=${unsupported.payable_multiplier_bp}`,
        ));
        continue;
      }
      const anthropic = headRules.find((rule) =>
        rule.scope_type === "provider" && rule.provider_id === "anthropic");
      if (!anthropic || anthropic.payable_multiplier_bp !== account.commerce_multiplier_bp) {
        blockers.push(blocker(
          "b2b_policy_anthropic_rule_mismatch",
          "commerce",
          account.engine_account_id,
          "B2B policy head must keep one provider:anthropic rule equal to the live scalar "
            + `${account.commerce_multiplier_bp} payable bp, otherwise the cutover changes the live Anthropic price`,
        ));
        continue;
      }
    }
    const baselineHead = headRules && headRules.length === 1
      && headRules[0]!.scope_type === "provider"
      && headRules[0]!.provider_id === "anthropic"
      && headRules[0]!.payable_multiplier_bp === account.commerce_multiplier_bp;
    const policy = resolvePolicy(policyForB2b(
      `release-v2:b2b:${account.engine_account_id}`,
      account.user_id,
      account.commerce_multiplier_bp,
      mainCatalog,
      switches,
      baselineHead ? null : headRules,
    ));
    policies.push(policy);
    policyByAccount.set(account.engine_account_id, policy);
  }

  const invitationSnapshots: Stage5V2Plan["invitation_snapshots"] = [];
  for (const invitation of invitations) {
    if (!validMultiplier(invitation.multiplier_bp)) {
      blockers.push(blocker(
        "b2b_invitation_multiplier_out_of_range",
        "commerce",
        invitation.invite_id,
        "B2B invitation multiplier must be an integer in 0..10000 basis points",
      ));
      continue;
    }
    const policy = resolvePolicy(policyForB2b(
      `release-v2:b2b-invite:${invitation.invite_id}`,
      `invite:${invitation.invite_id}`,
      invitation.multiplier_bp,
      mainCatalog,
      switches,
    ));
    policies.push(policy);
    const snapshotBase = {
      invite_id: invitation.invite_id,
      policy_id: policy.policy_id,
      policy_version: policy.policy_version,
      policy_digest: policy.content_digest,
    };
    invitationSnapshots.push({
      ...snapshotBase,
      snapshot_digest: stage5V2Digest("invitation-snapshot", snapshotBase),
    });
  }

  const serviceByAccount = new Map<string, ServiceAccountInventoryEntryV2>();
  for (const service of serviceAccounts) {
    serviceByAccount.set(service.engine_account_id, service);
    const engine = engineById.get(service.engine_account_id);
    if (engine && engine.status !== service.status) {
      blockers.push(blocker(
        "service_status_drift",
        "service",
        service.engine_account_id,
        `service authority status ${service.status} differs from engine ${engine.status}`,
      ));
    }
    const policy = resolvePolicy(buildPolicy({
      policy_id: `release-v2:service:${service.service_id}`,
      policy_version: STAGE5_V2_POLICY_VERSION,
      owner_type: "service",
      owner_id: service.service_id,
      account_class: "service",
      product_id: null,
      billing_mode: "meter_only",
      schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
      capability_generation: MULTI_DISCOUNT_GEN7_CAPABILITY_GENERATION,
      capability_digest: MULTI_DISCOUNT_GEN7_CAPABILITY_DIGEST,
      catalog_generation: null,
      catalog_digest: null,
      switch_generation: null,
      switch_digest: null,
      rules: [],
    }));
    policies.push(policy);
    policyByAccount.set(service.engine_account_id, policy);
  }

  for (const account of openkeysAccounts) {
    const engine = engineById.get(account.account_id);
    const expectedStatus = account.lifecycle === "active" ? "active" : "disabled";
    if (engine && engine.status !== expectedStatus) {
      blockers.push(blocker(
        "openkeys_status_drift",
        "openkeys",
        account.account_id,
        `OpenKeys ${account.lifecycle} account maps to engine ${engine.status}`,
      ));
    }
    policyByAccount.set(account.account_id, openkeysPolicy);
  }

  const sortedPolicies = [...policies].sort((left, right) =>
    compareUtf8(left.policy_id, right.policy_id));
  const duplicatePolicy = sortedPolicies.find((policy, index) =>
    index > 0 && sortedPolicies[index - 1]!.policy_id === policy.policy_id);
  if (duplicatePolicy) {
    blockers.push(blocker(
      "policy_identity_collision",
      "release",
      duplicatePolicy.policy_id,
      "two authoritative owners generated the same immutable policy id",
    ));
  }
  const policyManifestDigest = stage5V2Digest("policy-manifest", sortedPolicies.map((policy) => ({
    policy_id: policy.policy_id,
    policy_version: policy.policy_version,
    content_digest: policy.content_digest,
  })));

  const buildAssignments = (generation: number): Stage5V2PlannedAssignment[] => {
    const planned: Stage5V2PlannedAssignment[] = [];
    for (const engine of engineAccounts) {
      const accountClaims = claims.get(engine.account_id) ?? [];
      const policy = policyByAccount.get(engine.account_id);
      if (accountClaims.length !== 1 || !policy) continue;
      const claim = accountClaims[0]!;
      const service = serviceByAccount.get(engine.account_id);
      planned.push(assignment({
        release_generation: generation,
        engine_account_id: engine.account_id,
        account_class: policy.account_class === "open_keys" ? "openkeys" : policy.account_class,
        owner_context: claim.context,
        owner_id: claim.owner_id,
        policy_id: policy.policy_id,
        policy_version: policy.policy_version,
        policy_digest: policy.content_digest,
        billing_mode: policy.billing_mode,
        funding_generation: null,
        purpose: service?.purpose ?? null,
        responsible: service?.responsible ?? null,
      }));
    }
    return planned;
  };
  const commonRelease = {
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    commerce_inventory_digest: commerceInventoryDigest,
    engine_inventory_digest: input.engine_second.identity_digest,
    openkeys_inventory_digest: input.openkeys_second.inventory_digest,
    service_inventory_digest: input.service.inventory_digest,
    policy_manifest_digest: policyManifestDigest,
    funding_manifest_digest: null,
    engine_release_digest: null,
  };
  const target = releasePlan({
    ...commonRelease,
    generation: input.target_generation,
    release_kind: "target",
    assignments: buildAssignments(input.target_generation),
  });
  const recovery = releasePlan({
    ...commonRelease,
    generation: input.recovery_generation,
    release_kind: "recovery",
    assignments: buildAssignments(input.recovery_generation),
  });
  const fundingPlanDigest = stage5V2Digest("funding-plan", {
    target: target.assignments.filter((item) => item.billing_mode === "balance")
      .map(({ engine_account_id, funding_generation }) => ({ engine_account_id, funding_generation })),
    recovery: recovery.assignments.filter((item) => item.billing_mode === "balance")
      .map(({ engine_account_id, funding_generation }) => ({ engine_account_id, funding_generation })),
  });
  const sortedBlockers = [...blockers].sort((left, right) =>
    compareUtf8(left.blocker_context, right.blocker_context)
    || compareUtf8(left.subject_id, right.subject_id)
    || compareUtf8(left.blocker_code, right.blocker_code));
  const inventoryArtifact = {
    commerce: input.commerce,
    engine: {
      first_identity_digest: input.engine_first.identity_digest,
      first_full_digest: input.engine_first.full_digest,
      second_identity_digest: input.engine_second.identity_digest,
      second_full_digest: input.engine_second.full_digest,
      accounts: engineAccounts,
    },
    openkeys: {
      first_inventory_digest: input.openkeys_first.inventory_digest,
      second_inventory_digest: input.openkeys_second.inventory_digest,
      accounts: openkeysAccounts,
    },
    service: input.service,
    release_head: input.head_second,
  };
  const planBase = {
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    commerce_inventory_digest: commerceInventoryDigest,
    engine_scan_first_digest: input.engine_first.identity_digest,
    engine_scan_second_digest: input.engine_second.identity_digest,
    openkeys_scan_first_digest: input.openkeys_first.inventory_digest,
    openkeys_scan_second_digest: input.openkeys_second.inventory_digest,
    service_inventory_digest: input.service.inventory_digest,
    funding_plan_digest: fundingPlanDigest,
    target_generation: input.target_generation,
    target_digest: null,
    recovery_generation: input.recovery_generation,
    recovery_digest: null,
    capability,
    catalogs,
    switches,
    policies: sortedPolicies,
    invitation_snapshots: invitationSnapshots,
    target,
    recovery,
    blockers: sortedBlockers,
    inventory_artifact: inventoryArtifact,
  };
  const { inventory_artifact: _movingEvidence, ...stablePlanIdentity } = planBase;
  return {
    ...planBase,
    plan_digest: stage5V2Digest("plan", {
      ...stablePlanIdentity,
      release_head: input.head_second,
    }),
  };
}

export function stage5V2OpenKeysAccountMap(
  plan: Stage5V2Plan,
): ReadonlyMap<string, OpenKeysPricingInventoryAccountV2> {
  const inventory = plan.inventory_artifact.openkeys as { accounts: OpenKeysPricingInventoryAccountV2[] };
  return new Map(inventory.accounts.map((account) => [account.account_id, account]));
}
