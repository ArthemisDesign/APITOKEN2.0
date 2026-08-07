import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  OPENKEYS_PRICING_PRODUCT_ID,
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  pricingReleaseAssignmentExtensionV2Schema,
  pricingReleasePolicyV2Schema,
  type FundingNormalizationPlanV2,
  type PricingReleaseAssignmentExtensionV2,
  type PricingReleasePolicyV2,
  type PricingReleaseProvisioningContextV2,
  type PricingReleaseProvisioningReleaseV2,
  type PricingReleaseV2,
} from "@claude-api/contracts";
import type { EngineClient, TypedPricingMutationAck } from "./index.js";

export type PricingReleaseProvisioningTransportV2 = Pick<
  EngineClient,
  | "applyFundingNormalizationV2"
  | "getFundingNormalizationPlanV2"
  | "getPricingReleaseAssignmentExtensionV2"
  | "getPricingReleasePolicyV2"
  | "getPricingReleaseProvisioningContextV2"
  | "getPricingReleaseV2"
  | "preparePricingReleaseAssignmentExtensionV2"
  | "preparePricingReleasePolicyV2"
>;

export type PricingReleaseAccountProvisioningResultV2 =
  | { status: "pre_cutover"; headVersion: null; releaseGeneration: null }
  | { status: "base_assignment" | "extension"; headVersion: number; releaseGeneration: number };

export class PricingReleaseAccountProvisioningV2Error extends Error {
  constructor(
    readonly code:
      | "assignment_conflict"
      | "context_disappeared"
      | "context_changed"
      | "funding_not_ready"
      | "policy_not_ready",
    message: string,
  ) {
    super(message);
    this.name = "PricingReleaseAccountProvisioningV2Error";
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

/** Canonical JSON shared with the Stage 5 release materializer. */
export function canonicalPricingReleaseV2Json(value: unknown): string {
  return JSON.stringify(canonicalValue(value));
}

/** The one Stage 5/release-v2 content-addressing domain used by all later writers. */
export function pricingReleaseV2Digest(label: string, value: unknown): string {
  const hex = createHash("sha256")
    .update(`pricing-stage5-v2:${label}\n`, "utf8")
    .update(canonicalPricingReleaseV2Json(value), "utf8")
    .digest("hex");
  return `sha256:v2:${hex}`;
}

function policyRule(input: {
  rule_id: string;
  scope: PricingReleasePolicyV2["rules"][number]["scope"];
  discount_bps: number;
}): PricingReleasePolicyV2["rules"][number] {
  const base = {
    ...input,
    payable_multiplier_bp: 10_000 - input.discount_bps,
  };
  return { ...base, rule_digest: pricingReleaseV2Digest("policy-rule", base) };
}

function buildPolicy(input: Omit<PricingReleasePolicyV2, "content_digest">): PricingReleasePolicyV2 {
  const normalized = {
    ...input,
    rules: [...input.rules].sort((left, right) =>
      compareUtf8(canonicalPricingReleaseV2Json(left.scope), canonicalPricingReleaseV2Json(right.scope))
      || compareUtf8(left.rule_id, right.rule_id)),
  };
  return pricingReleasePolicyV2Schema.parse({
    ...normalized,
    content_digest: pricingReleaseV2Digest("policy", normalized),
  });
}

function customerLineage(release: PricingReleaseProvisioningReleaseV2) {
  return {
    billing_mode: "balance" as const,
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    capability_generation: release.capability_generation,
    capability_digest: release.capability_digest,
    catalog_generation: release.openkeys_catalog_generation,
    catalog_digest: release.openkeys_catalog_digest,
    switch_generation: release.switch_generation,
    switch_digest: release.switch_digest,
  };
}

function externalOwnerPolicyVersion(release: PricingReleaseProvisioningReleaseV2): number {
  if (release.capability_generation <= 3) return 1;
  if (release.capability_generation === 4) {
    throw new Error("pricing_release_rejected_capability_generation");
  }
  if (release.capability_generation === 5) return 2;
  if (release.capability_generation === 6) return 3;
  if (release.capability_generation === 7) return 4;
  throw new Error("pricing_release_unsupported_capability_generation");
}

export function buildOpenKeysPricingReleasePolicyV2(
  context: PricingReleaseProvisioningContextV2,
): PricingReleasePolicyV2 {
  return buildPolicy({
    policy_id: "release-v2:openkeys:global",
    policy_version: externalOwnerPolicyVersion(context.active_release),
    owner_type: "open_keys",
    owner_id: "openkeys",
    account_class: "open_keys",
    product_id: OPENKEYS_PRICING_PRODUCT_ID,
    ...customerLineage(context.active_release),
    rules: [policyRule({
      rule_id: "global-one-to-one",
      scope: { scope: "global" },
      discount_bps: 0,
    })],
  });
}

export function buildServicePricingReleasePolicyV2(
  context: PricingReleaseProvisioningContextV2,
  serviceId: string,
): PricingReleasePolicyV2 {
  return buildPolicy({
    policy_id: `release-v2:service:${serviceId}`,
    policy_version: externalOwnerPolicyVersion(context.active_release),
    owner_type: "service",
    owner_id: serviceId,
    account_class: "service",
    product_id: null,
    billing_mode: "meter_only",
    schema_version: PRICING_RELEASE_SCHEMA_VERSION_V2,
    capability_generation: context.active_release.capability_generation,
    capability_digest: context.active_release.capability_digest,
    catalog_generation: null,
    catalog_digest: null,
    switch_generation: null,
    switch_digest: null,
    rules: [],
  });
}

interface AssignmentSemanticsV2 {
  account_id: string;
  account_class: "open_keys" | "service";
  policy_id: string;
  policy_version: number;
  policy_digest: string;
  billing_mode: "balance" | "meter_only";
  funding_generation: number | null;
  purpose: string | null;
  responsible: string | null;
}

export function buildPricingReleaseAssignmentExtensionV2(
  context: PricingReleaseProvisioningContextV2,
  assignmentSemantics: AssignmentSemanticsV2,
): PricingReleaseAssignmentExtensionV2 {
  const generations = context.paired_recovery === null
    ? [context.active_release.generation]
    : [context.active_release.generation, context.paired_recovery.release.generation];
  const members = generations.map((releaseGeneration) => {
    const assignment = {
      ...assignmentSemantics,
      assignment_digest: pricingReleaseV2Digest("assignment-extension-assignment", {
        release_generation: releaseGeneration,
        ...assignmentSemantics,
      }),
    };
    return {
      release_generation: releaseGeneration,
      assignment,
      extension_digest: pricingReleaseV2Digest("assignment-extension-member", {
        release_generation: releaseGeneration,
        assignment,
      }),
    };
  });
  const group = {
    provisioning_head_generation: context.head.active_generation,
    provisioning_head_digest: context.head.active_digest,
    provisioning_head_version: context.head.head_version,
    paired_recovery_generation: context.paired_recovery?.release.generation ?? null,
    paired_recovery_digest: context.paired_recovery?.release.content_digest ?? null,
    members,
  };
  return pricingReleaseAssignmentExtensionV2Schema.parse({
    ...group,
    extension_group_digest: pricingReleaseV2Digest("assignment-extension-group", group),
  });
}

function sameCanonical(left: unknown, right: unknown): boolean {
  return canonicalPricingReleaseV2Json(left) === canonicalPricingReleaseV2Json(right);
}

function mutationAccepted(
  ack: TypedPricingMutationAck<unknown>,
  label: string,
): boolean {
  if (ack.result === "stored" || ack.result === "unchanged") return true;
  if (ack.result === "rejected" && (ack.code === "stale" || ack.code === "missing_dependency")) return false;
  const result = ack.result === "rejected" ? ack.code : ack.result;
  throw new PricingReleaseAccountProvisioningV2Error(
    label === "policy" ? "policy_not_ready" : "assignment_conflict",
    `engine rejected ${label} prepare with ${result}`,
  );
}

function extensionCoversContext(
  extension: PricingReleaseAssignmentExtensionV2,
  context: PricingReleaseProvisioningContextV2,
  accountId: string,
): boolean {
  return extension.members.some((member) =>
    member.release_generation === context.head.active_generation
    && member.assignment.account_id === accountId)
    && (extension.provisioning_head_generation === context.head.active_generation
      ? extension.provisioning_head_digest === context.head.active_digest
      : extension.paired_recovery_generation === context.head.active_generation
        && extension.paired_recovery_digest === context.head.active_digest);
}

function projectionMatchesRelease(
  projection: PricingReleaseProvisioningReleaseV2,
  release: PricingReleaseV2,
): boolean {
  return projection.generation === release.generation
    && projection.release_kind === release.release_kind
    && projection.schema_version === release.schema_version
    && projection.capability_generation === release.capability_generation
    && projection.capability_digest === release.capability_digest
    && projection.main_catalog_generation === release.main_catalog_generation
    && projection.main_catalog_digest === release.main_catalog_digest
    && projection.openkeys_catalog_generation === release.openkeys_catalog_generation
    && projection.openkeys_catalog_digest === release.openkeys_catalog_digest
    && projection.switch_generation === release.switch_generation
    && projection.switch_digest === release.switch_digest
    && projection.inventory_digest === release.inventory_digest
    && projection.funding_manifest_digest === release.funding_manifest_digest
    && projection.minimum_runtime_schema_version === release.minimum_runtime_schema_version
    && projection.content_digest === release.content_digest;
}

async function normalizedFundingGeneration(
  engine: PricingReleaseProvisioningTransportV2,
  accountId: string,
): Promise<number> {
  const complete = (plan: FundingNormalizationPlanV2): number | null =>
    plan.status === "normalized" && plan.funding_generation !== null && plan.funding_head_version !== null
      ? plan.funding_generation
      : null;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const plan = await engine.getFundingNormalizationPlanV2(accountId);
    if (!plan) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "funding_not_ready",
        "engine account disappeared during funding normalization",
      );
    }
    const existing = complete(plan);
    if (existing !== null) return existing;
    if (plan.status === "blocked" || plan.normalization_digest === null || plan.funding_generation === null) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "funding_not_ready",
        `funding normalization is ${plan.status}${plan.blockers[0] ? `: ${plan.blockers[0].code}` : ""}`,
      );
    }
    const result = await engine.applyFundingNormalizationV2(accountId, {
      expected_source_state_digest: plan.source_state_digest,
      expected_normalization_digest: plan.normalization_digest,
    });
    if (!result) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "funding_not_ready",
        "engine account disappeared during funding apply",
      );
    }
    const applied = complete(result.normalization);
    if ((result.status === "stored" || result.status === "unchanged") && applied !== null) return applied;
    if (result.status === "blocked") {
      throw new PricingReleaseAccountProvisioningV2Error(
        "funding_not_ready",
        "engine rejected the account-local funding plan as blocked",
      );
    }
  }
  throw new PricingReleaseAccountProvisioningV2Error(
    "funding_not_ready",
    "funding state kept changing during normalization",
  );
}

interface ProvisioningInputV2 {
  accountId: string;
  releaseRequired: boolean;
  allowBaseAssignment: boolean;
  policy(context: PricingReleaseProvisioningContextV2): PricingReleasePolicyV2;
  assignment(policy: PricingReleasePolicyV2, fundingGeneration: number | null): AssignmentSemanticsV2;
  normalizeFunding: boolean;
}

async function ensurePricingReleaseAccountProvisioningV2(
  engine: PricingReleaseProvisioningTransportV2,
  input: ProvisioningInputV2,
): Promise<PricingReleaseAccountProvisioningResultV2> {
  let observedContext = input.releaseRequired;
  let fundingGeneration: number | null = null;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const context = await engine.getPricingReleaseProvisioningContextV2();
    if (context === null) {
      if (observedContext) {
        throw new PricingReleaseAccountProvisioningV2Error(
          "context_disappeared",
          "pricing release provisioning context disappeared after it was observed",
        );
      }
      return { status: "pre_cutover", headVersion: null, releaseGeneration: null };
    }
    observedContext = true;
    const policy = input.policy(context);
    if (input.allowBaseAssignment) {
      const fullRelease = await engine.getPricingReleaseV2(context.active_release.generation);
      if (!fullRelease || !projectionMatchesRelease(context.active_release, fullRelease)) {
        throw new PricingReleaseAccountProvisioningV2Error(
          "context_changed",
          "full release readback differs from the provisioning context",
        );
      }
      const base = fullRelease.assignments.find((assignment) => assignment.account_id === input.accountId);
      if (base) {
        const expected = input.assignment(policy, base.funding_generation);
        const comparable = {
          account_id: base.account_id,
          account_class: base.account_class,
          policy_id: base.policy_id,
          policy_version: base.policy_version,
          policy_digest: base.policy_digest,
          billing_mode: base.billing_mode,
          funding_generation: base.funding_generation,
          purpose: base.purpose,
          responsible: base.responsible,
        };
        if (!sameCanonical(comparable, expected)) {
          throw new PricingReleaseAccountProvisioningV2Error(
            "assignment_conflict",
            "immutable base assignment conflicts with the requested account owner",
          );
        }
        return {
          status: "base_assignment",
          headVersion: context.head.head_version,
          releaseGeneration: context.head.active_generation,
        };
      }
    }

    if (input.normalizeFunding && fundingGeneration === null) {
      fundingGeneration = await normalizedFundingGeneration(engine, input.accountId);
    }
    const policyAck = await engine.preparePricingReleasePolicyV2(policy);
    if (!mutationAccepted(policyAck, "policy")) continue;
    const policyReadback = await engine.getPricingReleasePolicyV2(policy.policy_id, policy.policy_version);
    if (!policyReadback || !sameCanonical(policyReadback, policy)) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "policy_not_ready",
        "pricing release policy readback differs from the requested policy",
      );
    }
    const extension = buildPricingReleaseAssignmentExtensionV2(
      context,
      input.assignment(policy, fundingGeneration),
    );
    const extensionAck = await engine.preparePricingReleaseAssignmentExtensionV2(extension);
    if (!mutationAccepted(extensionAck, "assignment extension")) continue;
    const readback = await engine.getPricingReleaseAssignmentExtensionV2(
      extension.provisioning_head_version,
      input.accountId,
    );
    if (!readback || !sameCanonical(readback, extension)) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "assignment_conflict",
        "pricing assignment extension readback differs from the requested extension",
      );
    }
    const finalContext = await engine.getPricingReleaseProvisioningContextV2();
    if (finalContext !== null && extensionCoversContext(readback, finalContext, input.accountId)) {
      return {
        status: "extension",
        headVersion: finalContext.head.head_version,
        releaseGeneration: finalContext.head.active_generation,
      };
    }
  }
  throw new PricingReleaseAccountProvisioningV2Error(
    "context_changed",
    "pricing release provisioning context kept changing",
  );
}

export async function ensureOpenKeysPricingReleaseProvisioningV2(
  engine: PricingReleaseProvisioningTransportV2,
  input: { accountId: string; releaseRequired?: boolean },
): Promise<PricingReleaseAccountProvisioningResultV2> {
  return ensurePricingReleaseAccountProvisioningV2(engine, {
    accountId: input.accountId,
    releaseRequired: input.releaseRequired ?? false,
    allowBaseAssignment: false,
    normalizeFunding: true,
    policy: buildOpenKeysPricingReleasePolicyV2,
    assignment: (policy, fundingGeneration) => ({
      account_id: input.accountId,
      account_class: "open_keys",
      policy_id: policy.policy_id,
      policy_version: policy.policy_version,
      policy_digest: policy.content_digest,
      billing_mode: "balance",
      funding_generation: fundingGeneration,
      purpose: null,
      responsible: null,
    }),
  });
}

export async function ensureServicePricingReleaseProvisioningV2(
  engine: PricingReleaseProvisioningTransportV2,
  input: {
    accountId: string;
    serviceId: string;
    purpose: string;
    responsible: string;
    releaseRequired?: boolean;
  },
): Promise<PricingReleaseAccountProvisioningResultV2> {
  return ensurePricingReleaseAccountProvisioningV2(engine, {
    accountId: input.accountId,
    releaseRequired: input.releaseRequired ?? false,
    allowBaseAssignment: true,
    normalizeFunding: false,
    policy: (context) => buildServicePricingReleasePolicyV2(context, input.serviceId),
    assignment: (policy) => ({
      account_id: input.accountId,
      account_class: "service",
      policy_id: policy.policy_id,
      policy_version: policy.policy_version,
      policy_digest: policy.content_digest,
      billing_mode: "meter_only",
      funding_generation: null,
      purpose: input.purpose,
      responsible: input.responsible,
    }),
  });
}
