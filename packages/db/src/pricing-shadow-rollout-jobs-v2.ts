import { Buffer } from "node:buffer";
import {
  accountPolicyBindingSchema,
  accountPolicySpecSchema,
  activePolicyTargetSchema,
  canonicalSha256V2Schema,
  openKeysPricingInventoryAccountV2Schema,
  pricingCatalogSpecSchema,
  pricingReleaseActivationOperatorV2Schema,
  pricingStageControlMutationReasonV2Schema,
  providerSwitchSpecSchema,
  type AccountPolicyBinding,
  type AccountPolicySpec,
  type OpenKeysPricingInventoryAccountV2,
  type PricingCatalogSpec,
  type ProviderSwitchSpec,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import { z } from "zod";
import type { Database } from "./client.js";
import {
  Stage5MaterializerV2Error,
  scanStage5EngineInventoryV2,
  stage5V2CanonicalJson,
  stage5V2Digest,
} from "./pricing-stage5-materializer-v2.js";

export const PRICING_SHADOW_ROLLOUT_BINDING_V2: AccountPolicyBinding = {
  policy_enforcement: "shadow",
  funding_enforcement: "legacy_single",
  reconciliation_state: "verified",
};

const OPENKEYS_TRANSITION_PROVIDERS = ["anthropic", "openai"] as const;

export const pricingShadowPolicyRequestPayloadV2Schema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("policy_shadow"),
    policy: accountPolicySpecSchema,
    binding: accountPolicyBindingSchema,
  }).strict(),
  z.object({
    kind: z.literal("locked_openkeys_transition"),
    policy: accountPolicySpecSchema,
    expected_active: activePolicyTargetSchema,
  }).strict(),
]);
export type PricingShadowPolicyRequestPayloadV2 =
  z.infer<typeof pricingShadowPolicyRequestPayloadV2Schema>;

export interface StagePricingShadowRolloutV2Input {
  idempotencyKey: string;
  stage5RunId: string;
  actorId: string;
  reason: string;
}

export interface StagedPricingShadowRolloutV2 {
  rolloutId: string;
  rolloutDigest: string;
  jobCount: number;
  idempotentReplay: boolean;
}

export interface ClaimedPricingShadowPolicyJobV2 {
  id: string;
  rolloutId: string;
  engineAccountId: string;
  accountClass: "b2c" | "b2b" | "openkeys" | "service";
  ownerContext: "commerce" | "openkeys" | "service";
  attempts: number;
  requestDigest: string;
  payload: PricingShadowPolicyRequestPayloadV2;
}

export type PricingShadowPolicyJobDispositionV2 = "retry" | "blocked" | "dead";

export class PricingShadowRolloutV2Error extends Error {
  constructor(message: string, readonly permanent: boolean) {
    super(message);
    this.name = "PricingShadowRolloutV2Error";
  }
}

function permanent(message: string): PricingShadowRolloutV2Error {
  return new PricingShadowRolloutV2Error(message, true);
}

function transient(message: string): PricingShadowRolloutV2Error {
  return new PricingShadowRolloutV2Error(message, false);
}

function assertPositiveDuration(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(`${label} must be a positive safe integer`);
  }
}

function compareUtf8(left: string, right: string): number {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

const planArtifactSchema = z.object({
  catalogs: z.tuple([pricingCatalogSpecSchema, pricingCatalogSpecSchema]),
  switches: providerSwitchSpecSchema,
}).passthrough();

const inventoryArtifactSchema = z.object({
  openkeys: z.object({
    accounts: z.array(openKeysPricingInventoryAccountV2Schema),
  }).passthrough(),
}).passthrough();

interface StoredRunRow {
  run_id: string;
  plan_digest: string;
  engine_scan_first_digest: string;
  engine_scan_second_digest: string;
  target_generation: string;
  recovery_generation: string;
  status: string;
  inventory_artifact: unknown;
  plan_artifact: unknown;
}

interface StoredReleasePlanRow {
  generation: string;
  release_kind: "target" | "recovery";
  content_digest: string;
  status: string;
  engine_inventory_digest: string;
  assignment_manifest_digest: string;
  policy_manifest_digest: string;
  funding_manifest_digest: string | null;
  engine_release_digest: string | null;
}

interface StoredAssignmentRow {
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  owner_id: string;
  policy_id: string;
  policy_version: string;
  policy_digest: string;
}

interface StoredPolicyDocumentRow {
  policy_id: string;
  policy_version: string;
  owner_type: "global_b2c" | "b2b_client" | "openkeys" | "service";
  owner_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  product_id: string | null;
  billing_mode: "balance" | "meter_only";
  catalog_generation: string | null;
  switch_generation: string | null;
  content_digest: string;
}

interface StoredPolicyRuleRow {
  policy_id: string;
  policy_version: string;
  rule_id: string;
  scope_type: "global" | "provider" | "model";
  provider_id: string | null;
  canonical_model_id: string | null;
  discount_bps: string;
  payable_multiplier_bp: string;
}

interface JobDraft {
  engine_account_id: string;
  account_status: "active" | "disabled";
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  release_policy_id: string;
  release_policy_version: string;
  release_policy_digest: string;
  effective_version: number;
  content_digest: string;
  expected_active_version: number | null;
  expected_active_digest: string | null;
  request_digest: string;
  request_payload: PricingShadowPolicyRequestPayloadV2;
}

function positiveSafeNumber(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw permanent(`${label} is not a positive safe integer`);
  }
  return parsed;
}

function nonNegativeSafeNumber(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0 || String(parsed) !== value) {
    throw permanent(`${label} is not a non-negative safe integer`);
  }
  return parsed;
}

function sortedShadowRules(rules: AccountPolicySpec["rules"]): AccountPolicySpec["rules"] {
  return [...rules].sort((left, right) => {
    const leftProvider = "provider" in left.scope ? left.scope.provider.provider_id : left.scope.model.provider_id;
    const rightProvider = "provider" in right.scope ? right.scope.provider.provider_id : right.scope.model.provider_id;
    const leftScope = "provider" in left.scope ? "provider" : "model";
    const rightScope = "provider" in right.scope ? "provider" : "model";
    const leftModel = "model" in left.scope ? left.scope.model.canonical_model_id : "";
    const rightModel = "model" in right.scope ? right.scope.model.canonical_model_id : "";
    return compareUtf8(leftProvider, rightProvider)
      || compareUtf8(leftScope, rightScope)
      || compareUtf8(leftModel, rightModel)
      || compareUtf8(left.rule_id, right.rule_id);
  });
}

function shadowRuleDigest(base: Omit<AccountPolicySpec["rules"][number], "rule_digest">): string {
  return stage5V2Digest("shadow-rollout-rule", base);
}

/**
 * Builds the only successor the engine transition accepts: the exact live lineage identity
 * advanced exactly once, managed provider-only 1:1 rules and no replacement lock. The source is
 * the actual engine policy read at staging, never a reconstructed historical derivation.
 * Digests use the canonical `sha256:v2` Stage 5 domain.
 */
export function buildLockedOpenkeysSuccessorPolicyV1(input: {
  source: AccountPolicySpec;
  catalogGeneration: number;
  switchGeneration: number;
}): AccountPolicySpec {
  const source = input.source;
  const rules = OPENKEYS_TRANSITION_PROVIDERS.map((providerId) => {
    const base = {
      rule_id: `openkeys-${providerId}-1to1`,
      scope: { provider: { provider_id: providerId } },
      pricing_mode: "discount" as const,
      rule_origin: "managed" as const,
      discount_bps: 0,
      payable_multiplier_bp: 10_000,
      track_eligible: false,
      retention_eligible: false,
      commission_eligible: false,
    };
    return { ...base, rule_digest: shadowRuleDigest(base) };
  });
  const base = {
    account_id: source.account_id,
    effective_version: source.effective_version + 1,
    policy_id: source.policy_id,
    policy_version: source.policy_version + 1,
    source_policy_digest: source.content_digest,
    owner_type: source.owner_type,
    owner_id: source.owner_id,
    account_class: source.account_class,
    product_id: source.product_id,
    schema_version: 1,
    catalog_generation: input.catalogGeneration,
    switch_generation: input.switchGeneration,
    replacement_locked: false,
    rules,
  };
  return { ...base, content_digest: stage5V2Digest("shadow-rollout-locked-successor", base) };
}

type SwitchScope = ProviderSwitchSpec["entries"][number]["scope"];

function sameScope(left: SwitchScope, right: SwitchScope): boolean {
  return stage5V2CanonicalJson(left) === stage5V2CanonicalJson(right);
}

function requiredSwitchScope(accountClass: string, productId: string): SwitchScope {
  if (accountClass === "b2c") return { segment: { product_id: productId, segment: "b2c" } };
  if (accountClass === "b2b") return { segment: { product_id: productId, segment: "b2b" } };
  return { product: { product_id: productId } };
}

function scopedProviders(
  switches: ProviderSwitchSpec,
  scope: SwitchScope,
  catalogGeneration: number,
): string[] {
  return switches.entries
    .filter((entry) =>
      entry.enabled
      && sameScope(entry.scope, scope)
      && entry.catalog_generation === catalogGeneration)
    .map((entry) => entry.provider_id)
    .sort(compareUtf8);
}

function convertReleaseRulesV1(
  document: StoredPolicyDocumentRow,
  rules: StoredPolicyRuleRow[],
  providers: readonly string[],
): AccountPolicySpec["rules"] {
  const converted: AccountPolicySpec["rules"] = [];
  for (const rule of rules) {
    const discountBps = nonNegativeSafeNumber(rule.discount_bps, "release policy rule discount");
    const multiplier = positiveSafeNumber(rule.payable_multiplier_bp, "release policy rule multiplier");
    const base = {
      pricing_mode: "discount" as const,
      rule_origin: "managed" as const,
      discount_bps: discountBps,
      payable_multiplier_bp: multiplier,
      track_eligible: false,
      retention_eligible: false,
      commission_eligible: false,
    };
    if (rule.scope_type === "global") {
      if (providers.length === 0) {
        throw permanent(
          `release policy ${document.policy_id} has a global rule but no enabled scoped providers`,
        );
      }
      for (const providerId of providers) {
        const v1 = {
          ...base,
          rule_id: `${rule.rule_id}:provider:${providerId}`,
          scope: { provider: { provider_id: providerId } },
        };
        converted.push({ ...v1, rule_digest: shadowRuleDigest(v1) });
      }
      continue;
    }
    if (rule.scope_type === "provider") {
      if (rule.provider_id === null) throw permanent("provider rule is missing its provider");
      const v1 = { ...base, rule_id: rule.rule_id, scope: { provider: { provider_id: rule.provider_id } } };
      converted.push({ ...v1, rule_digest: shadowRuleDigest(v1) });
      continue;
    }
    if (rule.provider_id === null || rule.canonical_model_id === null) {
      throw permanent("model rule is missing its provider or model");
    }
    const v1 = {
      ...base,
      rule_id: rule.rule_id,
      scope: {
        model: {
          provider_id: rule.provider_id,
          canonical_model_id: rule.canonical_model_id,
        },
      },
    };
    converted.push({ ...v1, rule_digest: shadowRuleDigest(v1) });
  }
  return sortedShadowRules(converted);
}

function buildGenericShadowPolicyV1(input: {
  accountId: string;
  lineage: {
    policy_id: string;
    policy_version: number;
    owner_type: "global_b2c" | "b2b_client" | "open_keys" | "service";
    owner_id: string;
    account_class: "b2c" | "b2b" | "open_keys" | "service";
    product_id: string;
  };
  effectiveVersion: number;
  document: StoredPolicyDocumentRow;
  rules: StoredPolicyRuleRow[];
  mainCatalog: PricingCatalogSpec;
  openkeysCatalog: PricingCatalogSpec;
  switches: ProviderSwitchSpec;
}): AccountPolicySpec {
  const document = input.document;
  const productId = input.lineage.product_id;
  const catalog = productId === "openkeys" ? input.openkeysCatalog : input.mainCatalog;
  const catalogGeneration = document.catalog_generation === null
    ? catalog.generation
    : positiveSafeNumber(document.catalog_generation, "release policy catalog generation");
  const switchGeneration = document.switch_generation === null
    ? input.switches.generation
    : positiveSafeNumber(document.switch_generation, "release policy switch generation");
  if (catalogGeneration !== catalog.generation || switchGeneration !== input.switches.generation) {
    throw permanent(
      `release policy ${document.policy_id} pins a catalog or switch generation outside the Stage 5 plan`,
    );
  }
  const providers = scopedProviders(
    input.switches,
    requiredSwitchScope(input.lineage.account_class, productId),
    catalogGeneration,
  );
  const rules = convertReleaseRulesV1(document, input.rules, providers);
  if (rules.length === 0) {
    throw permanent(
      `release policy ${document.policy_id} converts to an empty shadow ruleset`,
    );
  }
  const base = {
    account_id: input.accountId,
    effective_version: input.effectiveVersion,
    policy_id: input.lineage.policy_id,
    policy_version: input.lineage.policy_version,
    source_policy_digest: stage5V2Digest("shadow-rollout-source-policy", {
      policy_id: document.policy_id,
      policy_version: document.policy_version,
      content_digest: document.content_digest,
    }),
    owner_type: input.lineage.owner_type,
    owner_id: input.lineage.owner_id,
    account_class: input.lineage.account_class,
    product_id: productId,
    schema_version: 1,
    catalog_generation: catalogGeneration,
    switch_generation: switchGeneration,
    replacement_locked: false,
    rules,
  };
  return { ...base, content_digest: stage5V2Digest("shadow-rollout-effective-policy", base) };
}

async function loadStoredRun(client: PoolClient, stage5RunId: string): Promise<StoredRunRow> {
  const result = await client.query<StoredRunRow>(`
    SELECT run_id::text, plan_digest, engine_scan_first_digest, engine_scan_second_digest,
           target_generation::text, recovery_generation::text, status,
           inventory_artifact, plan_artifact
    FROM pricing_stage5_runs_v2
    WHERE run_id = $1
    FOR SHARE
  `, [stage5RunId]);
  const row = result.rows[0];
  if (!row) throw permanent("exact Stage 5 run does not exist");
  return row;
}

async function loadReleasePlan(
  client: PoolClient,
  generation: string,
  expectedKind: "target" | "recovery",
): Promise<StoredReleasePlanRow> {
  const result = await client.query<StoredReleasePlanRow>(`
    SELECT generation::text, release_kind, content_digest, status,
           engine_inventory_digest, assignment_manifest_digest, policy_manifest_digest,
           funding_manifest_digest::text, engine_release_digest
    FROM pricing_release_plans_v2
    WHERE generation = $1
    FOR SHARE
  `, [generation]);
  const row = result.rows[0];
  if (!row || row.release_kind !== expectedKind) {
    throw permanent(`exact ${expectedKind} release plan does not exist`);
  }
  return row;
}

export async function stagePricingShadowRolloutV2(
  database: Database,
  engine: Pick<EngineClient, "getPricingReleaseInventoryV2" | "getAccountPricingState">,
  untrustedInput: StagePricingShadowRolloutV2Input,
): Promise<StagedPricingShadowRolloutV2> {
  const input: StagePricingShadowRolloutV2Input = {
    idempotencyKey: untrustedInput.idempotencyKey,
    stage5RunId: untrustedInput.stage5RunId,
    actorId: pricingReleaseActivationOperatorV2Schema.parse(untrustedInput.actorId),
    reason: pricingStageControlMutationReasonV2Schema.parse(untrustedInput.reason),
  };
  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  if (!uuidPattern.test(input.idempotencyKey)) throw new TypeError("idempotencyKey must be a UUID");
  if (!uuidPattern.test(input.stage5RunId)) throw new TypeError("stage5RunId must be a UUID");

  let scan: Awaited<ReturnType<typeof scanStage5EngineInventoryV2>>;
  try {
    scan = await scanStage5EngineInventoryV2(engine);
  } catch (error) {
    if (error instanceof Stage5MaterializerV2Error) {
      throw transient("engine inventory authority is temporarily unavailable");
    }
    throw error;
  }
  const inventoryByAccount = new Map(scan.accounts.map((account) => [account.account_id, account]));

  // Immutable pre-pass: the Stage 5 run row and target assignment manifest are append-only, so
  // reading them outside the staging transaction is safe; the in-transaction FOR SHARE re-read
  // below must reproduce the manifest byte-for-byte or staging fails closed. The pre-pass exists
  // so per-account engine lineage reads (network I/O) never happen inside the SERIALIZABLE
  // transaction.
  const preRun = await database.pool.query<{
    target_generation: string;
    inventory_artifact: unknown;
  }>(`
    SELECT target_generation::text, inventory_artifact
    FROM pricing_stage5_runs_v2 WHERE run_id = $1
  `, [input.stage5RunId]);
  const preRunRow = preRun.rows[0];
  if (!preRunRow) {
    throw permanent("Stage 5 run is not fully prepared for shadow rollout staging");
  }
  const preTargetGeneration = preRunRow.target_generation;
  const preAssignments = await database.pool.query<StoredAssignmentRow>(`
    SELECT engine_account_id, account_class, owner_context, owner_id,
           policy_id, policy_version::text, policy_digest
    FROM pricing_release_assignments_v2
    WHERE release_generation = $1
    ORDER BY engine_account_id COLLATE "C"
  `, [preTargetGeneration]);
  const preOpenkeysAccounts = new Map<string, OpenKeysPricingInventoryAccountV2>(
    inventoryArtifactSchema.parse(preRunRow.inventory_artifact).openkeys.accounts
      .map((account) => [account.account_id, account]),
  );

  // Only OpenKeys accounts are aligned by this lane: commerce and service lineages are advanced
  // by their managed policy writers, and a release-policy identity can never attach to an
  // account whose engine lineage already exists (policy identity is immutable per lineage).
  // Every OpenKeys job builds from the exact live engine lineage read once up front.
  const openkeysAdvance = new Map<string, Awaited<ReturnType<EngineClient["getAccountPricingState"]>>>();
  for (const row of preAssignments.rows) {
    if (row.owner_context !== "openkeys") continue;
    if (preOpenkeysAccounts.get(row.engine_account_id)?.pricing_contract === undefined) continue;
    const state = await engine.getAccountPricingState(row.engine_account_id);
    openkeysAdvance.set(row.engine_account_id, state);
  }

  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL SERIALIZABLE");
    transactionOpen = true;
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended('pricing-shadow-rollout-v2:stage', 0))",
    );

    const run = await loadStoredRun(client, input.stage5RunId);
    if (run.status !== "prepared") {
      throw permanent("Stage 5 run is not fully prepared for shadow rollout staging");
    }
    if (scan.identity_digest !== run.engine_scan_first_digest
        || run.engine_scan_first_digest !== run.engine_scan_second_digest) {
      throw permanent("engine inventory drifted from the exact Stage 5 run");
    }
    const target = await loadReleasePlan(client, run.target_generation, "target");
    const recovery = await loadReleasePlan(client, run.recovery_generation, "recovery");
    for (const [kind, plan] of [["target", target], ["recovery", recovery]] as const) {
      if (plan.status !== "prepared"
          || plan.funding_manifest_digest === null
          || plan.engine_release_digest === null) {
        throw permanent(`exact ${kind} release plan is not prepared`);
      }
      if (plan.engine_inventory_digest !== scan.identity_digest) {
        throw permanent(`exact ${kind} release plan engine inventory digest drifted`);
      }
    }

    const existing = await client.query<{
      id: string;
      stage5_run_id: string;
      rollout_digest: string;
      actor_id: string;
      reason: string;
      job_count: string;
    }>(`
      SELECT id::text, stage5_run_id::text, rollout_digest, actor_id, reason, job_count::text
      FROM pricing_shadow_rollouts_v2
      WHERE idempotency_key = $1
      FOR UPDATE
    `, [input.idempotencyKey]);

    const planArtifact = planArtifactSchema.parse(run.plan_artifact);
    const mainCatalog = planArtifact.catalogs.find((catalog) => catalog.product_id === "main");
    const openkeysCatalog = planArtifact.catalogs.find((catalog) => catalog.product_id === "openkeys");
    if (!mainCatalog || !openkeysCatalog) {
      throw permanent("Stage 5 plan artifact is missing the exact main or OpenKeys catalog");
    }
    if (mainCatalog.generation !== openkeysCatalog.generation) {
      throw permanent("Stage 5 catalogs do not share one exact generation");
    }
    const switches = planArtifact.switches;
    const openkeysAccounts = new Map<string, OpenKeysPricingInventoryAccountV2>(
      inventoryArtifactSchema.parse(run.inventory_artifact).openkeys.accounts
        .map((account) => [account.account_id, account]),
    );

    const assignments = await client.query<StoredAssignmentRow>(`
      SELECT engine_account_id, account_class, owner_context, owner_id,
             policy_id, policy_version::text, policy_digest
      FROM pricing_release_assignments_v2
      WHERE release_generation = $1
      ORDER BY engine_account_id COLLATE "C"
      FOR SHARE
    `, [run.target_generation]);
    if (assignments.rows.length === 0) {
      throw permanent("exact target release has no assignments to align");
    }
    if (preTargetGeneration !== String(run.target_generation)
        || stage5V2CanonicalJson(preAssignments.rows) !== stage5V2CanonicalJson(assignments.rows)) {
      throw permanent("target assignment manifest drifted during shadow rollout staging");
    }

    const policyKeys = [...new Map(assignments.rows.map((row) => [
      `${row.policy_id}${row.policy_version}`,
      { policy_id: row.policy_id, policy_version: row.policy_version },
    ])).values()];
    const documents = new Map<string, StoredPolicyDocumentRow>();
    const documentRules = new Map<string, StoredPolicyRuleRow[]>();
    for (const key of policyKeys) {
      const document = await client.query<StoredPolicyDocumentRow>(`
        SELECT policy_id, policy_version::text, owner_type, owner_id, account_class,
               product_id, billing_mode, catalog_generation::text, switch_generation::text,
               content_digest
        FROM pricing_policy_documents_v2
        WHERE policy_id = $1 AND policy_version = $2
        FOR SHARE
      `, [key.policy_id, key.policy_version]);
      const documentRow = document.rows[0];
      if (!documentRow) {
        throw permanent(`release policy ${key.policy_id}/${key.policy_version} is missing`);
      }
      documents.set(`${key.policy_id}${key.policy_version}`, documentRow);
      const rules = await client.query<StoredPolicyRuleRow>(`
        SELECT policy_id, policy_version::text, rule_id, scope_type,
               provider_id, canonical_model_id, discount_bps::text, payable_multiplier_bp::text
        FROM pricing_policy_rules_v2
        WHERE policy_id = $1 AND policy_version = $2
        ORDER BY rule_id COLLATE "C"
        FOR SHARE
      `, [key.policy_id, key.policy_version]);
      documentRules.set(`${key.policy_id}${key.policy_version}`, rules.rows);
    }

    const jobs: JobDraft[] = [];
    for (const assignment of assignments.rows) {
      const inventoryAccount = inventoryByAccount.get(assignment.engine_account_id);
      if (!inventoryAccount) {
        throw permanent(
          `assignment account ${assignment.engine_account_id} is missing from the exact engine inventory`,
        );
      }
      const document = documents.get(`${assignment.policy_id}${assignment.policy_version}`)!;
      if (document.content_digest !== assignment.policy_digest) {
        throw permanent(
          `release policy ${assignment.policy_id} digest collides with the target assignment`,
        );
      }
      const openkeysAccount = openkeysAccounts.get(assignment.engine_account_id);
      if (assignment.owner_context === "openkeys" && !openkeysAccount) {
        throw permanent(
          `OpenKeys assignment ${assignment.engine_account_id} is missing its exact inventory owner`,
        );
      }
      if (assignment.owner_context !== "openkeys") {
        // Commerce and service lineages are advanced by their managed policy writers; this lane
        // never attaches a release-policy identity to an existing engine lineage.
        continue;
      }
      if (openkeysAccount!.pricing_contract === "legacy") {
        if (openkeysAccount!.source_multiplier_bp !== inventoryAccount.multiplier_bp) {
          throw permanent(
            `legacy OpenKeys multiplier drifted for ${assignment.engine_account_id}`,
          );
        }
        // The exact legacy policy is read from the live engine lineage, never reconstructed:
        // only the actual stored identity/digest can be the transition expectation.
        const lockedState = openkeysAdvance.get(assignment.engine_account_id);
        if (!lockedState || lockedState === "unbound" || "inactive" in lockedState) {
          throw permanent(
            `legacy OpenKeys account ${assignment.engine_account_id} has no active engine policy lineage`,
          );
        }
        const lockedPolicy = lockedState.active.policy;
        if (!lockedPolicy.replacement_locked) {
          throw permanent(
            `legacy OpenKeys account ${assignment.engine_account_id} lineage lost its replacement lock`,
          );
        }
        const successor = buildLockedOpenkeysSuccessorPolicyV1({
          source: lockedPolicy,
          catalogGeneration: openkeysCatalog.generation,
          switchGeneration: switches.generation,
        });
        const payload: PricingShadowPolicyRequestPayloadV2 = {
          kind: "locked_openkeys_transition",
          policy: successor,
          expected_active: {
            target: {
              version: lockedPolicy.effective_version,
              content_digest: lockedPolicy.content_digest,
            },
            binding: lockedState.active.binding,
          },
        };
        jobs.push({
          engine_account_id: assignment.engine_account_id,
          account_status: inventoryAccount.status,
          account_class: assignment.account_class,
          owner_context: assignment.owner_context,
          release_policy_id: assignment.policy_id,
          release_policy_version: assignment.policy_version,
          release_policy_digest: assignment.policy_digest,
          effective_version: successor.effective_version,
          content_digest: successor.content_digest,
          expected_active_version: lockedPolicy.effective_version,
          expected_active_digest: lockedPolicy.content_digest,
          request_digest: stage5V2Digest("pricing-shadow-rollout-request-v2", payload),
          request_payload: payload,
        });
        continue;
      }
      // Already-canonical OpenKeys accounts advance their existing engine lineage in place: the
      // engine never accepts a different policy identity for an account with a lineage, so the
      // shadow successor reuses the live identity at the next monotonic version.
      const lineageState = openkeysAdvance.get(assignment.engine_account_id);
      if (!lineageState || lineageState === "unbound" || "inactive" in lineageState) {
        throw permanent(
          `canonical OpenKeys account ${assignment.engine_account_id} has no active engine policy lineage`,
        );
      }
      const lineage = lineageState.active.policy;
      if (lineage.replacement_locked) {
        throw permanent(
          `canonical OpenKeys account ${assignment.engine_account_id} lineage is unexpectedly locked`,
        );
      }
      const policy = buildGenericShadowPolicyV1({
        accountId: assignment.engine_account_id,
        lineage: {
          policy_id: lineage.policy_id,
          policy_version: lineage.policy_version + 1,
          owner_type: lineage.owner_type,
          owner_id: lineage.owner_id,
          account_class: lineage.account_class,
          product_id: lineage.product_id,
        },
        effectiveVersion: lineage.effective_version + 1,
        document,
        rules: documentRules.get(`${assignment.policy_id}${assignment.policy_version}`)!,
        mainCatalog,
        openkeysCatalog,
        switches,
      });
      const payload: PricingShadowPolicyRequestPayloadV2 = {
        kind: "policy_shadow",
        policy,
        binding: PRICING_SHADOW_ROLLOUT_BINDING_V2,
      };
      jobs.push({
        engine_account_id: assignment.engine_account_id,
        account_status: inventoryAccount.status,
        account_class: assignment.account_class,
        owner_context: assignment.owner_context,
        release_policy_id: assignment.policy_id,
        release_policy_version: assignment.policy_version,
        release_policy_digest: assignment.policy_digest,
        effective_version: policy.effective_version,
        content_digest: policy.content_digest,
        expected_active_version: lineage.effective_version,
        expected_active_digest: lineage.content_digest,
        request_digest: stage5V2Digest("pricing-shadow-rollout-request-v2", payload),
        request_payload: payload,
      });
    }

    const rolloutDigest = stage5V2Digest("pricing-shadow-rollout-v2", {
      stage5_run_id: input.stage5RunId,
      target_generation: target.generation,
      target_digest: target.content_digest,
      recovery_generation: recovery.generation,
      recovery_digest: recovery.content_digest,
      catalog_generation: mainCatalog.generation,
      main_catalog_digest: mainCatalog.content_digest,
      openkeys_catalog_digest: openkeysCatalog.content_digest,
      switch_generation: switches.generation,
      switch_digest: switches.content_digest,
      engine_inventory_digest: scan.identity_digest,
      assignment_manifest_digest: target.assignment_manifest_digest,
      policy_manifest_digest: target.policy_manifest_digest,
      request_digests: jobs.map((job) => job.request_digest),
    });

    const existingRow = existing.rows[0];
    if (existingRow) {
      if (existingRow.stage5_run_id !== input.stage5RunId
          || existingRow.rollout_digest !== rolloutDigest
          || existingRow.actor_id !== input.actorId
          || existingRow.reason !== input.reason) {
        throw permanent("shadow rollout idempotency key has a different immutable request");
      }
      await client.query("COMMIT");
      transactionOpen = false;
      return {
        rolloutId: existingRow.id,
        rolloutDigest,
        jobCount: nonNegativeSafeNumber(existingRow.job_count, "stored rollout job count"),
        idempotentReplay: true,
      };
    }
    const sameDigest = await client.query<{ id: string; stage5_run_id: string; job_count: string }>(`
      SELECT id::text, stage5_run_id::text, job_count::text
      FROM pricing_shadow_rollouts_v2
      WHERE rollout_digest = $1
      FOR UPDATE
    `, [rolloutDigest]);
    if (sameDigest.rows[0]) {
      if (sameDigest.rows[0].stage5_run_id !== input.stage5RunId) {
        throw permanent("shadow rollout digest collides with a different Stage 5 run");
      }
      await client.query("COMMIT");
      transactionOpen = false;
      return {
        rolloutId: sameDigest.rows[0].id,
        rolloutDigest,
        jobCount: nonNegativeSafeNumber(sameDigest.rows[0].job_count, "stored rollout job count"),
        idempotentReplay: true,
      };
    }

    const inserted = await client.query<{ id: string }>(`
      INSERT INTO pricing_shadow_rollouts_v2 (
        idempotency_key, stage5_run_id,
        target_generation, target_digest, recovery_generation, recovery_digest,
        catalog_generation, main_catalog_digest, openkeys_catalog_digest,
        switch_generation, switch_digest,
        engine_inventory_digest, assignment_manifest_digest, policy_manifest_digest,
        rollout_digest, assignment_count, job_count, actor_id, reason
      ) VALUES (
        $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19
      )
      RETURNING id::text
    `, [
      input.idempotencyKey,
      input.stage5RunId,
      target.generation,
      target.content_digest,
      recovery.generation,
      recovery.content_digest,
      mainCatalog.generation,
      mainCatalog.content_digest,
      openkeysCatalog.content_digest,
      switches.generation,
      switches.content_digest,
      scan.identity_digest,
      target.assignment_manifest_digest,
      target.policy_manifest_digest,
      rolloutDigest,
      assignments.rows.length,
      jobs.length,
      input.actorId,
      input.reason,
    ]);
    const rolloutId = inserted.rows[0]!.id;
    for (const job of jobs) {
      await client.query(`
        INSERT INTO pricing_shadow_policy_jobs_v2 (
          rollout_id, engine_account_id, account_status, account_class, owner_context,
          release_policy_id, release_policy_version, release_policy_digest,
          effective_version, content_digest, expected_active_version, expected_active_digest,
          request_digest, request_payload
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14::jsonb)
      `, [
        rolloutId,
        job.engine_account_id,
        job.account_status,
        job.account_class,
        job.owner_context,
        job.release_policy_id,
        job.release_policy_version,
        job.release_policy_digest,
        job.effective_version,
        job.content_digest,
        job.expected_active_version,
        job.expected_active_digest,
        job.request_digest,
        JSON.stringify(job.request_payload),
      ]);
    }
    await client.query(`
      INSERT INTO audit_log (
        actor_type, actor_id, action, target_type, target_id, metadata
      ) VALUES (
        'admin', $1, 'pricing_shadow_rollout_staged',
        'pricing_shadow_rollout_v2', $2,
        jsonb_build_object(
          'stage5_run_id', $3::text,
          'rollout_digest', $4::text,
          'target_generation', $5::text,
          'target_digest', $6::text,
          'recovery_generation', $7::text,
          'recovery_digest', $8::text,
          'assignment_count', $9::text,
          'job_count', $10::text,
          'reason', $11::text
        )
      )
    `, [
      input.actorId,
      rolloutId,
      input.stage5RunId,
      rolloutDigest,
      target.generation,
      target.content_digest,
      recovery.generation,
      recovery.content_digest,
      String(assignments.rows.length),
      String(jobs.length),
      input.reason,
    ]);
    await client.query("COMMIT");
    transactionOpen = false;
    return { rolloutId, rolloutDigest, jobCount: jobs.length, idempotentReplay: false };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

interface ClaimedJobRow {
  id: string;
  rollout_id: string;
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  attempts: number;
  request_digest: string;
  request_payload: unknown;
}

async function recoverStaleShadowJobs(
  client: Pick<PoolClient, "query">,
  leaseMs: number,
  maxAttempts: number,
): Promise<number> {
  assertPositiveDuration(leaseMs, "shadow rollout leaseMs");
  assertPositiveDuration(maxAttempts, "shadow rollout maxAttempts");
  const result = await client.query(`
    UPDATE pricing_shadow_policy_jobs_v2
    SET status = CASE WHEN attempts >= $2 THEN 'dead' ELSE 'retry' END,
        next_attempt_at = CASE WHEN attempts >= $2 THEN next_attempt_at ELSE now() END,
        locked_at = NULL, locked_by = NULL,
        last_error = CASE
          WHEN attempts >= $2 THEN 'shadow policy job lease expired at the maximum attempt count'
          ELSE COALESCE(last_error, 'recovered expired shadow policy job lease')
        END,
        completed_at = CASE WHEN attempts >= $2 THEN now() ELSE NULL END,
        updated_at = now()
    WHERE status = 'processing'
      AND (locked_at IS NULL OR locked_at < now() - ($1 * interval '1 millisecond'))
  `, [leaseMs, maxAttempts]);
  return result.rowCount ?? 0;
}

export async function recoverStalePricingShadowPolicyJobsV2(
  database: Database,
  leaseMs: number,
  maxAttempts: number,
): Promise<number> {
  return recoverStaleShadowJobs(database.pool, leaseMs, maxAttempts);
}

function claimedFromRow(row: ClaimedJobRow): ClaimedPricingShadowPolicyJobV2 {
  const payload = pricingShadowPolicyRequestPayloadV2Schema.parse(row.request_payload);
  if (stage5V2Digest("pricing-shadow-rollout-request-v2", payload) !== row.request_digest) {
    throw permanent("durable shadow policy job payload does not match its request digest");
  }
  return {
    id: row.id,
    rolloutId: row.rollout_id,
    engineAccountId: row.engine_account_id,
    accountClass: row.account_class,
    ownerContext: row.owner_context,
    attempts: row.attempts,
    requestDigest: row.request_digest,
    payload,
  };
}

export async function claimPricingShadowPolicyJobsV2(
  database: Database,
  workerId: string,
  options: { batchSize: number; leaseMs: number; maxAttempts: number },
): Promise<ClaimedPricingShadowPolicyJobV2[]> {
  if (workerId.trim() === "") throw new RangeError("workerId is required");
  if (!Number.isSafeInteger(options.batchSize)
      || options.batchSize < 1
      || options.batchSize > 500) {
    throw new RangeError("shadow rollout batchSize must be an integer from 1 to 500");
  }
  assertPositiveDuration(options.leaseMs, "shadow rollout leaseMs");
  assertPositiveDuration(options.maxAttempts, "shadow rollout maxAttempts");
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    await recoverStaleShadowJobs(client, options.leaseMs, options.maxAttempts);
    const candidates = await client.query<{ id: string }>(`
      SELECT id
      FROM pricing_shadow_policy_jobs_v2
      WHERE status IN ('pending', 'retry') AND next_attempt_at <= now()
      ORDER BY next_attempt_at, created_at, id
      FOR UPDATE SKIP LOCKED
      LIMIT $1
    `, [options.batchSize]);
    if (candidates.rows.length === 0) {
      await client.query("COMMIT");
      transactionOpen = false;
      return [];
    }
    const ids = candidates.rows.map((row) => row.id);
    const claimed = await client.query<ClaimedJobRow>(`
      UPDATE pricing_shadow_policy_jobs_v2
      SET status = 'processing', attempts = attempts + 1,
          locked_at = now(), locked_by = $2, last_error = NULL, updated_at = now()
      WHERE id = ANY($1::uuid[])
      RETURNING id::text, rollout_id::text, engine_account_id, account_class, owner_context,
                attempts, request_digest, request_payload
    `, [ids, workerId]);
    try {
      const jobs = claimed.rows.map(claimedFromRow);
      await client.query(`
        UPDATE pricing_shadow_rollouts_v2
        SET status = 'processing', updated_at = now()
        WHERE status = 'pending'
          AND id IN (
            SELECT DISTINCT rollout_id FROM pricing_shadow_policy_jobs_v2 WHERE id = ANY($1::uuid[])
          )
      `, [ids]);
      await client.query("COMMIT");
      transactionOpen = false;
      return jobs;
    } catch (error) {
      const reason = error instanceof Error ? error.message : "invalid durable shadow policy job";
      await client.query(`
        UPDATE pricing_shadow_policy_jobs_v2
        SET status = 'dead', locked_at = NULL, locked_by = NULL,
            last_error = $2, completed_at = now(), updated_at = now()
        WHERE id = ANY($1::uuid[]) AND status = 'processing' AND locked_by = $3
      `, [ids, reason.slice(0, 2_000), workerId]);
      await client.query("COMMIT");
      transactionOpen = false;
      throw error;
    }
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function finalizeRolloutIfTerminal(client: PoolClient, rolloutId: string): Promise<void> {
  const counts = await client.query<{ status: string; count: string }>(`
    SELECT status, count(*)::text AS count
    FROM pricing_shadow_policy_jobs_v2
    WHERE rollout_id = $1
    GROUP BY status
  `, [rolloutId]);
  const byStatus = new Map(counts.rows.map((row) => [
    row.status,
    nonNegativeSafeNumber(row.count, "shadow rollout job count"),
  ]));
  const unresolved = (byStatus.get("pending") ?? 0)
    + (byStatus.get("processing") ?? 0)
    + (byStatus.get("retry") ?? 0);
  if (unresolved > 0) return;
  const confirmed = byStatus.get("confirmed") ?? 0;
  const blocked = byStatus.get("blocked") ?? 0;
  const dead = byStatus.get("dead") ?? 0;
  const total = confirmed + blocked + dead;
  if (total === 0) return;
  const status = confirmed === total ? "confirmed" : dead > 0 ? "dead" : "blocked";
  const lastError = status === "confirmed"
    ? null
    : status === "dead"
      ? "one or more shadow policy jobs failed closed"
      : "one or more shadow policy jobs were blocked";
  const updated = await client.query(`
    UPDATE pricing_shadow_rollouts_v2
    SET status = $2, last_error = $3, completed_at = now(), updated_at = now()
    WHERE id = $1 AND status IN ('pending', 'processing')
  `, [rolloutId, status, lastError]);
  if (updated.rowCount !== 1) {
    throw transient("shadow rollout changed while its terminal state was finalized");
  }
}

export function pricingShadowPolicyAckDigestV2(ackPayload: unknown): string {
  const parsed = z.record(z.unknown()).parse(ackPayload);
  return stage5V2Digest("pricing-shadow-rollout-ack-v2", parsed);
}

export async function completePricingShadowPolicyJobV2(
  database: Database,
  job: ClaimedPricingShadowPolicyJobV2,
  workerId: string,
  ackPayload: unknown,
): Promise<string> {
  const ackDigest = pricingShadowPolicyAckDigestV2(ackPayload);
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    const updated = await client.query(`
      UPDATE pricing_shadow_policy_jobs_v2
      SET status = 'confirmed', ack_digest = $2, ack_payload = $3::jsonb,
          confirmed_at = now(), completed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'processing' AND locked_by = $4
        AND attempts = $5 AND request_digest = $6
    `, [
      job.id,
      ackDigest,
      JSON.stringify(ackPayload),
      workerId,
      job.attempts,
      job.requestDigest,
    ]);
    if (updated.rowCount !== 1) {
      throw transient(`shadow policy job ${job.id} lost its lease`);
    }
    await finalizeRolloutIfTerminal(client, job.rolloutId);
    await client.query("COMMIT");
    transactionOpen = false;
    return ackDigest;
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function failPricingShadowPolicyJobV2(
  database: Database,
  job: ClaimedPricingShadowPolicyJobV2,
  workerId: string,
  disposition: PricingShadowPolicyJobDispositionV2,
  error: string,
  options: { retryMs: number; maxAttempts: number },
): Promise<"retry" | "blocked" | "dead"> {
  assertPositiveDuration(options.retryMs, "shadow rollout retryMs");
  assertPositiveDuration(options.maxAttempts, "shadow rollout maxAttempts");
  const status = disposition === "retry" && job.attempts < options.maxAttempts
    ? "retry"
    : disposition === "retry" ? "dead" : disposition;
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN");
    transactionOpen = true;
    const updated = await client.query(`
      UPDATE pricing_shadow_policy_jobs_v2
      SET status = $4,
          next_attempt_at = CASE
            WHEN $4 = 'retry' THEN now() + ($5 * interval '1 millisecond')
            ELSE next_attempt_at
          END,
          completed_at = CASE WHEN $4 = 'retry' THEN NULL ELSE now() END,
          locked_at = NULL, locked_by = NULL,
          last_error = $6, updated_at = now()
      WHERE id = $1 AND status = 'processing' AND locked_by = $2
        AND attempts = $3 AND request_digest = $7
    `, [
      job.id,
      workerId,
      job.attempts,
      status,
      options.retryMs,
      error.slice(0, 2_000),
      job.requestDigest,
    ]);
    if (updated.rowCount !== 1) {
      throw transient(`shadow policy job ${job.id} lost its lease`);
    }
    if (status !== "retry") await finalizeRolloutIfTerminal(client, job.rolloutId);
    await client.query("COMMIT");
    transactionOpen = false;
    return status;
  } catch (error2) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error2;
  } finally {
    client.release();
  }
}

export interface PricingShadowRolloutControlV2 {
  databaseObservedAt: Date;
  countsByStatus: Record<"pending" | "processing" | "confirmed" | "blocked" | "dead", number>;
  rollouts: Array<{
    id: string;
    idempotencyKey: string;
    stage5RunId: string;
    rolloutDigest: string;
    targetGeneration: string;
    targetDigest: string;
    recoveryGeneration: string;
    recoveryDigest: string;
    catalogGeneration: string;
    mainCatalogDigest: string;
    openkeysCatalogDigest: string;
    switchGeneration: string;
    switchDigest: string;
    engineInventoryDigest: string;
    assignmentManifestDigest: string;
    policyManifestDigest: string;
    assignmentCount: string;
    jobCount: string;
    jobCountsByStatus: Record<string, number>;
    actorId: string;
    reason: string;
    status: string;
    lastError: string | null;
    completedAt: Date | null;
    createdAt: Date;
    updatedAt: Date;
  }>;
  jobs: Array<{
    id: string;
    rolloutId: string;
    subjectDigest: string;
    accountStatus: string;
    accountClass: string;
    ownerContext: string;
    releasePolicyDigest: string;
    contentDigest: string;
    expectedActiveDigest: string | null;
    requestDigest: string;
    status: string;
    attempts: number;
    lastError: string | null;
    ackDigest: string | null;
    confirmedAt: Date | null;
    completedAt: Date | null;
    createdAt: Date;
    updatedAt: Date;
  }>;
}

export function pricingShadowRolloutSubjectDigestV2(engineAccountId: string): string {
  return stage5V2Digest("pricing-shadow-rollout-subject", engineAccountId);
}

export async function readPricingShadowRolloutControlV2(
  database: Database,
  limit = 20,
): Promise<PricingShadowRolloutControlV2> {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
    throw new RangeError("shadow rollout control limit must be an integer from 1 to 100");
  }
  const client = await database.pool.connect();
  let transactionOpen = false;
  try {
    await client.query("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY");
    transactionOpen = true;
    const observed = await client.query<{ database_now: Date }>(
      "SELECT transaction_timestamp() AS database_now",
    );
    const counts = await client.query<{ status: string; count: string }>(`
      SELECT status, count(*)::text AS count
      FROM pricing_shadow_rollouts_v2
      GROUP BY status
    `);
    const rollouts = await client.query<{
      id: string;
      idempotency_key: string;
      stage5_run_id: string;
      rollout_digest: string;
      target_generation: string;
      target_digest: string;
      recovery_generation: string;
      recovery_digest: string;
      catalog_generation: string;
      main_catalog_digest: string;
      openkeys_catalog_digest: string;
      switch_generation: string;
      switch_digest: string;
      engine_inventory_digest: string;
      assignment_manifest_digest: string;
      policy_manifest_digest: string;
      assignment_count: string;
      job_count: string;
      actor_id: string;
      reason: string;
      status: string;
      last_error: string | null;
      completed_at: Date | null;
      created_at: Date;
      updated_at: Date;
    }>(`
      SELECT id::text, idempotency_key::text, stage5_run_id::text, rollout_digest,
             target_generation::text, target_digest,
             recovery_generation::text, recovery_digest,
             catalog_generation::text, main_catalog_digest, openkeys_catalog_digest,
             switch_generation::text, switch_digest,
             engine_inventory_digest, assignment_manifest_digest, policy_manifest_digest,
             assignment_count::text, job_count::text,
             actor_id, reason, status, last_error, completed_at, created_at, updated_at
      FROM pricing_shadow_rollouts_v2
      ORDER BY created_at DESC, id
      LIMIT $1
    `, [limit]);
    const rolloutIds = rollouts.rows.map((row) => row.id);
    const jobCounts = rolloutIds.length === 0
      ? { rows: [] as Array<{ rollout_id: string; status: string; count: string }> }
      : await client.query<{ rollout_id: string; status: string; count: string }>(`
        SELECT rollout_id::text, status, count(*)::text AS count
        FROM pricing_shadow_policy_jobs_v2
        WHERE rollout_id = ANY($1::uuid[])
        GROUP BY rollout_id, status
      `, [rolloutIds]);
    const jobs = await client.query<{
      id: string;
      rollout_id: string;
      engine_account_id: string;
      account_status: string;
      account_class: string;
      owner_context: string;
      release_policy_digest: string;
      content_digest: string;
      expected_active_digest: string | null;
      request_digest: string;
      status: string;
      attempts: number;
      last_error: string | null;
      ack_digest: string | null;
      confirmed_at: Date | null;
      completed_at: Date | null;
      created_at: Date;
      updated_at: Date;
    }>(`
      SELECT id::text, rollout_id::text, engine_account_id, account_status, account_class,
             owner_context, release_policy_digest, content_digest, expected_active_digest,
             request_digest, status, attempts, last_error, ack_digest,
             confirmed_at, completed_at, created_at, updated_at
      FROM pricing_shadow_policy_jobs_v2
      ORDER BY created_at DESC, id
      LIMIT $1
    `, [limit]);
    await client.query("COMMIT");
    transactionOpen = false;

    const countsByStatus = { pending: 0, processing: 0, confirmed: 0, blocked: 0, dead: 0 };
    for (const row of counts.rows) {
      if (row.status in countsByStatus) {
        countsByStatus[row.status as keyof typeof countsByStatus] =
          nonNegativeSafeNumber(row.count, "shadow rollout count");
      }
    }
    const jobCountsByRollout = new Map<string, Record<string, number>>();
    for (const row of jobCounts.rows) {
      const entry = jobCountsByRollout.get(row.rollout_id) ?? {};
      entry[row.status] = nonNegativeSafeNumber(row.count, "shadow rollout job count");
      jobCountsByRollout.set(row.rollout_id, entry);
    }
    return {
      databaseObservedAt: observed.rows[0]!.database_now,
      countsByStatus,
      rollouts: rollouts.rows.map((row) => ({
        id: row.id,
        idempotencyKey: row.idempotency_key,
        stage5RunId: row.stage5_run_id,
        rolloutDigest: row.rollout_digest,
        targetGeneration: row.target_generation,
        targetDigest: row.target_digest,
        recoveryGeneration: row.recovery_generation,
        recoveryDigest: row.recovery_digest,
        catalogGeneration: row.catalog_generation,
        mainCatalogDigest: row.main_catalog_digest,
        openkeysCatalogDigest: row.openkeys_catalog_digest,
        switchGeneration: row.switch_generation,
        switchDigest: row.switch_digest,
        engineInventoryDigest: row.engine_inventory_digest,
        assignmentManifestDigest: row.assignment_manifest_digest,
        policyManifestDigest: row.policy_manifest_digest,
        assignmentCount: row.assignment_count,
        jobCount: row.job_count,
        jobCountsByStatus: jobCountsByRollout.get(row.id) ?? {},
        actorId: row.actor_id,
        reason: row.reason,
        status: row.status,
        lastError: row.last_error,
        completedAt: row.completed_at,
        createdAt: row.created_at,
        updatedAt: row.updated_at,
      })),
      jobs: jobs.rows.map((row) => ({
        id: row.id,
        rolloutId: row.rollout_id,
        subjectDigest: pricingShadowRolloutSubjectDigestV2(row.engine_account_id),
        accountStatus: row.account_status,
        accountClass: row.account_class,
        ownerContext: row.owner_context,
        releasePolicyDigest: row.release_policy_digest,
        contentDigest: row.content_digest,
        expectedActiveDigest: row.expected_active_digest,
        requestDigest: row.request_digest,
        status: row.status,
        attempts: row.attempts,
        lastError: row.last_error,
        ackDigest: row.ack_digest,
        confirmedAt: row.confirmed_at,
        completedAt: row.completed_at,
        createdAt: row.created_at,
        updatedAt: row.updated_at,
      })),
    };
  } catch (error) {
    if (transactionOpen) await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
