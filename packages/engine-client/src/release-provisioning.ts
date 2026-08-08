import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
  PRICING_RELEASE_SCHEMA_VERSION_V2,
  pricingReleaseAssignmentExtensionV2Schema,
  pricingReleasePolicyV2Schema,
  type PricingReleaseAssignmentExtensionV2,
  type PricingReleasePolicyV2,
  type PricingReleaseProvisioningContextV2,
  type PricingReleaseProvisioningReleaseV2,
  type PricingReleaseV2,
} from "@claude-api/contracts";
import type { EngineClient, TypedPricingMutationAck } from "./index.js";

export type PricingReleaseProvisioningTransportV2 = Pick<
  EngineClient,
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
  account_class: "service";
  policy_id: string;
  policy_version: number;
  policy_digest: string;
  billing_mode: "meter_only";
  funding_generation: null;
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

/**
 * Completes the post-cutover release-v2 chain for one meter-only service account: the exact
 * rule-free service policy and the account-local assignment extension for the active head (and
 * its paired recovery). Service accounts are the one account class that stays on the release
 * path in the retirement phase: the engine has no meter-only lane outside release-v2 (a
 * rule-free strict policy admits nothing, managed discounts cap at 9500 bps, and the legacy
 * scalar lane cannot express meter-only), so they are never opted out. An exact replay returns
 * the stored base assignment or extension; every success is backed by an exact GET readback.
 */
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
  const assignment = (policy: PricingReleasePolicyV2): AssignmentSemanticsV2 => ({
    account_id: input.accountId,
    account_class: "service",
    policy_id: policy.policy_id,
    policy_version: policy.policy_version,
    policy_digest: policy.content_digest,
    billing_mode: "meter_only",
    funding_generation: null,
    purpose: input.purpose,
    responsible: input.responsible,
  });
  let observedContext = input.releaseRequired ?? false;
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
    const policy = buildServicePricingReleasePolicyV2(context, input.serviceId);
    const fullRelease = await engine.getPricingReleaseV2(context.active_release.generation);
    if (!fullRelease || !projectionMatchesRelease(context.active_release, fullRelease)) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "context_changed",
        "full release readback differs from the provisioning context",
      );
    }
    const base = fullRelease.assignments.find((item) => item.account_id === input.accountId);
    if (base) {
      const expected = assignment(policy);
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

    const policyAck = await engine.preparePricingReleasePolicyV2(policy);
    if (!mutationAccepted(policyAck, "policy")) continue;
    const policyReadback = await engine.getPricingReleasePolicyV2(policy.policy_id, policy.policy_version);
    if (!policyReadback || !sameCanonical(policyReadback, policy)) {
      throw new PricingReleaseAccountProvisioningV2Error(
        "policy_not_ready",
        "pricing release policy readback differs from the requested policy",
      );
    }
    const extension = buildPricingReleaseAssignmentExtensionV2(context, assignment(policy));
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
