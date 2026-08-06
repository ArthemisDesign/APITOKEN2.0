import { Buffer } from "node:buffer";
import { isDeepStrictEqual } from "node:util";
import type {
  PricingReleaseAssignmentExtensionV2,
  PricingReleaseHeadV2,
  PricingReleaseInventoryAccountV2,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import type { PoolClient } from "pg";
import {
  scanStage5EngineInventoryV2,
  scanStage5OpenKeysInventoryV2,
  stage5V2CanonicalJson,
  stage5V2CommerceInventoryDigest,
  stage5V2Digest,
  type Stage5V2OpenKeysReader,
} from "./pricing-stage5-materializer-v2.js";
import { readStage5V2CommerceAndServiceSnapshot } from "./pricing-stage5-materializer-v2-store.js";

export type PricingReleaseActivationAuthorityEngineV2 = Pick<
  EngineClient,
  | "getPricingReleaseInventoryV2"
  | "getPricingReleaseHeadV2"
  | "getPricingReleaseAssignmentExtensionV2"
  | "getFundingNormalizationPlanV2"
>;

export interface PricingReleaseActivationAuthorityReadersV2 {
  engine: PricingReleaseActivationAuthorityEngineV2;
  openkeys: Stage5V2OpenKeysReader;
}

export interface PricingReleaseActivationAuthorityExpectationV2 {
  activationKind: "cutover" | "recovery" | "successor";
  targetGeneration: string;
  targetEngineDigest: string;
  recoveryGeneration: string;
  recoveryEngineDigest: string;
  targetCommerceInventoryDigest: string;
  targetEngineInventoryDigest: string;
  targetOpenkeysInventoryDigest: string;
  targetServiceInventoryDigest: string;
  expectedHead: PricingReleaseHeadV2 | null;
}

export interface PricingReleaseActivationAuthorityBlockerV2 {
  code: string;
  count: number;
  subjectDigests: string[];
}

export interface PricingReleaseActivationAuthorityCaptureV2 {
  commerceInventoryDigest: string;
  engineInventoryDigest: string;
  openkeysInventoryDigest: string;
  serviceInventoryDigest: string;
  blockers: PricingReleaseActivationAuthorityBlockerV2[];
}

interface AssignmentRow {
  release_generation: string;
  engine_account_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  owner_context: "commerce" | "openkeys" | "service";
  owner_id: string;
  policy_id: string;
  policy_version: string;
  policy_digest: string;
  billing_mode: "balance" | "meter_only";
  funding_generation: string | null;
  purpose: string | null;
  responsible: string | null;
}

interface PolicyRow {
  policy_id: string;
  policy_version: string;
  owner_type: "global_b2c" | "b2b_client" | "openkeys" | "service";
  owner_id: string;
  account_class: "b2c" | "b2b" | "openkeys" | "service";
  product_id: string | null;
  billing_mode: "balance" | "meter_only";
  content_digest: string;
  rule_count: string;
  global_rule_count: string;
  non_one_to_one_rule_count: string;
}

interface AuthorityClaim {
  context: "commerce" | "openkeys" | "service";
  ownerId: string;
  accountClass: "b2c" | "b2b" | "openkeys" | "service";
  status: "active" | "disabled" | "invalid";
  profileMultiplierBp: number | null;
  commerceMultiplierBp: number | null;
  purpose: string | null;
  responsible: string | null;
}

function blocker(
  blockers: PricingReleaseActivationAuthorityBlockerV2[],
  code: string,
  subjects: readonly string[],
): void {
  const unique = [...new Set(subjects)].sort((left, right) => Buffer.compare(
    Buffer.from(left, "utf8"),
    Buffer.from(right, "utf8"),
  ));
  if (unique.length === 0) return;
  const digests = unique.map((subject) => stage5V2Digest("activation-authority-subject", subject));
  const existing = blockers.find((candidate) => candidate.code === code);
  if (!existing) {
    blockers.push({ code, count: digests.length, subjectDigests: digests });
    return;
  }
  existing.subjectDigests = [...new Set([...existing.subjectDigests, ...digests])]
    .sort((left, right) => Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")));
  existing.count = existing.subjectDigests.length;
}

function sameHead(left: PricingReleaseHeadV2 | null, right: PricingReleaseHeadV2 | null): boolean {
  return isDeepStrictEqual(left, right);
}

function assignmentSemantics(row: AssignmentRow): string {
  return stage5V2CanonicalJson({
    engine_account_id: row.engine_account_id,
    account_class: row.account_class,
    owner_context: row.owner_context,
    owner_id: row.owner_id,
    policy_id: row.policy_id,
    policy_version: row.policy_version,
    policy_digest: row.policy_digest,
    billing_mode: row.billing_mode,
    funding_generation: row.funding_generation,
    purpose: row.purpose,
    responsible: row.responsible,
  });
}

function extensionAssignment(
  extension: PricingReleaseAssignmentExtensionV2,
  generation: number,
): PricingReleaseAssignmentExtensionV2["members"][number]["assignment"] | null {
  return extension.members.find((member) => member.release_generation === generation)?.assignment ?? null;
}

function positiveSafeInteger(value: string, label: string): number {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0 || String(parsed) !== value) {
    throw new RangeError(`${label} is not a positive safe integer`);
  }
  return parsed;
}

function sameAssignmentSemantics(
  left: PricingReleaseAssignmentExtensionV2["members"][number]["assignment"],
  right: PricingReleaseAssignmentExtensionV2["members"][number]["assignment"],
): boolean {
  const semantics = (assignment: typeof left): string => stage5V2CanonicalJson({
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
  return semantics(left) === semantics(right);
}

function expectedPolicyOwner(claim: AuthorityClaim): Pick<PolicyRow, "owner_type" | "owner_id"> {
  if (claim.accountClass === "b2c") return { owner_type: "global_b2c", owner_id: "global" };
  if (claim.accountClass === "b2b") return { owner_type: "b2b_client", owner_id: claim.ownerId };
  if (claim.accountClass === "openkeys") return { owner_type: "openkeys", owner_id: "openkeys" };
  return { owner_type: "service", owner_id: claim.ownerId };
}

interface PolicyAssignmentIdentity {
  policy_id: string;
  policy_version: string | number;
  policy_digest: string;
  billing_mode: "balance" | "meter_only";
}

type ExtensionAssignment = PricingReleaseAssignmentExtensionV2["members"][number]["assignment"];

function policyIdentity(policyId: string, policyVersion: string | number): string {
  return `${policyId}\0${policyVersion}`;
}

async function readPolicyAuthorityRows(
  client: PoolClient,
  assignments: readonly PolicyAssignmentIdentity[],
): Promise<Map<string, PolicyRow>> {
  const identities = [...new Map(assignments.map((assignment) => [
    policyIdentity(assignment.policy_id, assignment.policy_version),
    [assignment.policy_id, String(assignment.policy_version)] as const,
  ])).values()];
  if (identities.length === 0) return new Map();
  const policies = await client.query<PolicyRow>(`
    SELECT policy.policy_id, policy.policy_version::text, policy.owner_type, policy.owner_id,
           policy.account_class, policy.product_id, policy.billing_mode, policy.content_digest,
           count(rule.rule_id)::text AS rule_count,
           count(rule.rule_id) FILTER (WHERE rule.scope_type = 'global')::text AS global_rule_count,
           count(rule.rule_id) FILTER (
             WHERE rule.discount_bps <> 0 OR rule.payable_multiplier_bp <> 10000
           )::text AS non_one_to_one_rule_count
    FROM pricing_policy_documents_v2 policy
    LEFT JOIN pricing_policy_rules_v2 rule
      ON rule.policy_id = policy.policy_id AND rule.policy_version = policy.policy_version
    WHERE (policy.policy_id, policy.policy_version) IN (
      SELECT * FROM unnest($1::text[], $2::bigint[])
    )
    GROUP BY policy.policy_id, policy.policy_version, policy.owner_type, policy.owner_id,
             policy.account_class, policy.product_id, policy.billing_mode, policy.content_digest
  `, [identities.map(([id]) => id), identities.map(([, version]) => version)]);
  return new Map(policies.rows.map((policy) => [
    policyIdentity(policy.policy_id, policy.policy_version),
    policy,
  ]));
}

function validatePolicyIdentity(
  assignment: PolicyAssignmentIdentity,
  claim: AuthorityClaim,
  policy: PolicyRow | undefined,
): string[] {
  if (!policy) return ["missing"];
  const failures: string[] = [];
  const owner = expectedPolicyOwner(claim);
  if (policy.policy_id !== assignment.policy_id
      || policy.policy_version !== String(assignment.policy_version)
      || policy.content_digest !== assignment.policy_digest) failures.push("identity");
  if (policy.owner_type !== owner.owner_type || policy.owner_id !== owner.owner_id) {
    failures.push("owner");
  }
  if (policy.account_class !== claim.accountClass || policy.billing_mode !== assignment.billing_mode) {
    failures.push("class");
  }
  if (claim.accountClass === "openkeys" && (
    policy.product_id !== "openkeys"
    || policy.rule_count !== "1"
    || policy.global_rule_count !== "1"
    || policy.non_one_to_one_rule_count !== "0"
  )) failures.push("openkeys-1-to-1");
  if (claim.accountClass === "service" && (
    policy.product_id !== null || policy.rule_count !== "0"
  )) failures.push("service-meter-only");
  return failures;
}

function validateExtensionIdentity(input: {
  extension: PricingReleaseAssignmentExtensionV2;
  account: PricingReleaseInventoryAccountV2;
  claim: AuthorityClaim;
  policy: PolicyRow | undefined;
  expectation: PricingReleaseActivationAuthorityExpectationV2;
  fundingGeneration: number | null;
  fundingHeadVersion: number | null;
}): string[] {
  const { extension, account, claim, expectation } = input;
  const targetGeneration = positiveSafeInteger(expectation.targetGeneration, "target generation");
  const recoveryGeneration = positiveSafeInteger(expectation.recoveryGeneration, "recovery generation");
  const target = extensionAssignment(extension, targetGeneration);
  const recovery = extensionAssignment(extension, recoveryGeneration);
  const active = target;
  const failures: string[] = [];
  if (
    extension.provisioning_head_generation !== targetGeneration
    || extension.provisioning_head_digest !== expectation.targetEngineDigest
    || extension.provisioning_head_version !== expectation.expectedHead?.head_version
    || extension.paired_recovery_generation !== recoveryGeneration
    || extension.paired_recovery_digest !== expectation.recoveryEngineDigest
    || target === null
    || recovery === null
  ) failures.push("pair");
  if (target !== null && recovery !== null && !sameAssignmentSemantics(target, recovery)) {
    failures.push("pair-semantics");
  }
  if (!active || active.account_id !== account.account_id || active.account_class !== claim.accountClass) {
    failures.push("class");
  }
  const service = claim.accountClass === "service";
  if (active) {
    if (service) {
      if (active.billing_mode !== "meter_only" || active.funding_generation !== null
          || active.purpose !== claim.purpose || active.responsible !== claim.responsible) {
        failures.push("service");
      }
    } else if (
      active.billing_mode !== "balance"
      || active.funding_generation === null
      || active.funding_generation !== account.funding_generation
      || active.funding_generation !== input.fundingGeneration
      || account.funding_head_version !== input.fundingHeadVersion
    ) {
      failures.push("funding");
    }
    failures.push(...validatePolicyIdentity(active, claim, input.policy).map((failure) =>
      `policy-${failure}`));
  }
  return failures;
}

/**
 * Re-reads all mutable ownership authorities and the engine inventory under a stable commerce
 * snapshot. Before cutover it requires the exact immutable base inventory. After cutover it keeps
 * that base immutable and accepts only accounts backed by the exact target/recovery extension pair.
 */
export async function capturePricingReleaseActivationAuthorityV2(
  client: PoolClient,
  readers: PricingReleaseActivationAuthorityReadersV2,
  expectation: PricingReleaseActivationAuthorityExpectationV2,
): Promise<PricingReleaseActivationAuthorityCaptureV2> {
  const blockers: PricingReleaseActivationAuthorityBlockerV2[] = [];
  const headFirst = await readers.engine.getPricingReleaseHeadV2();
  const [engineFirst, openkeysFirst] = await Promise.all([
    scanStage5EngineInventoryV2(readers.engine),
    scanStage5OpenKeysInventoryV2(readers.openkeys),
  ]);
  const current = await readStage5V2CommerceAndServiceSnapshot(client);
  const [engineSecond, openkeysSecond] = await Promise.all([
    scanStage5EngineInventoryV2(readers.engine),
    scanStage5OpenKeysInventoryV2(readers.openkeys),
  ]);
  const headSecond = await readers.engine.getPricingReleaseHeadV2();
  const commerceInventoryDigest = stage5V2CommerceInventoryDigest(current.commerce);
  const serviceInventoryDigest = current.service.inventory_digest;

  if (engineFirst.identity_digest !== engineSecond.identity_digest
      || stage5V2CanonicalJson(engineFirst.accounts.map(({ account_id, status, multiplier_bp }) => ({
        account_id,
        status,
        multiplier_bp,
      }))) !== stage5V2CanonicalJson(engineSecond.accounts.map(({ account_id, status, multiplier_bp }) => ({
        account_id,
        status,
        multiplier_bp,
      })))) {
    blocker(blockers, "engine_inventory_changed_between_scans", [
      engineFirst.identity_digest,
      engineSecond.identity_digest,
    ]);
  }
  if (openkeysFirst.inventory_digest !== openkeysSecond.inventory_digest
      || stage5V2CanonicalJson(openkeysFirst.accounts) !== stage5V2CanonicalJson(openkeysSecond.accounts)) {
    blocker(blockers, "openkeys_inventory_changed_between_scans", [
      openkeysFirst.inventory_digest,
      openkeysSecond.inventory_digest,
    ]);
  }
  if (!sameHead(headFirst, headSecond) || !sameHead(headSecond, expectation.expectedHead)) {
    blocker(blockers, "release_head_changed_or_unexpected", [
      stage5V2CanonicalJson(headFirst),
      stage5V2CanonicalJson(headSecond),
      stage5V2CanonicalJson(expectation.expectedHead),
    ]);
  }

  const assignmentResult = await client.query<AssignmentRow>(`
    SELECT release_generation::text, engine_account_id, account_class, owner_context,
           owner_id, policy_id, policy_version::text, policy_digest, billing_mode,
           funding_generation::text, purpose, responsible
    FROM pricing_release_assignments_v2
    WHERE release_generation IN ($1, $2)
    ORDER BY release_generation, engine_account_id COLLATE "C"
  `, [expectation.targetGeneration, expectation.recoveryGeneration]);
  const targetAssignments = assignmentResult.rows.filter(
    (row) => row.release_generation === expectation.targetGeneration,
  );
  const recoveryAssignments = assignmentResult.rows.filter(
    (row) => row.release_generation === expectation.recoveryGeneration,
  );
  const basePolicyByIdentity = await readPolicyAuthorityRows(client, targetAssignments);
  const recoveryByAccount = new Map(recoveryAssignments.map((row) => [row.engine_account_id, row]));
  for (const target of targetAssignments) {
    const recovery = recoveryByAccount.get(target.engine_account_id);
    if (!recovery || assignmentSemantics(target) !== assignmentSemantics(recovery)) {
      blocker(blockers, "target_recovery_base_assignment_mismatch", [target.engine_account_id]);
    }
  }
  if (targetAssignments.length !== recoveryAssignments.length) {
    blocker(blockers, "target_recovery_base_assignment_count_mismatch", [
      `${targetAssignments.length}:${recoveryAssignments.length}`,
    ]);
  }

  const claims = new Map<string, AuthorityClaim[]>();
  const addClaim = (accountId: string, claim: AuthorityClaim): void => {
    claims.set(accountId, [...(claims.get(accountId) ?? []), claim]);
  };
  for (const account of current.commerce.accounts) {
    addClaim(account.engine_account_id, {
      context: "commerce",
      ownerId: account.user_id,
      accountClass: account.account_class,
      status: account.commerce_status === "active" || account.commerce_status === "disabled"
        ? account.commerce_status
        : "invalid",
      profileMultiplierBp: account.profile_multiplier_bp,
      commerceMultiplierBp: account.commerce_multiplier_bp,
      purpose: null,
      responsible: null,
    });
  }
  for (const account of openkeysSecond.accounts) {
    addClaim(account.account_id, {
      context: "openkeys",
      ownerId: account.source_id,
      accountClass: "openkeys",
      status: account.lifecycle === "active" ? "active" : "disabled",
      profileMultiplierBp: null,
      commerceMultiplierBp: account.source_multiplier_bp,
      purpose: null,
      responsible: null,
    });
  }
  for (const account of current.service.accounts) {
    addClaim(account.engine_account_id, {
      context: "service",
      ownerId: account.service_id,
      accountClass: "service",
      status: account.status,
      profileMultiplierBp: null,
      commerceMultiplierBp: null,
      purpose: account.purpose,
      responsible: account.responsible,
    });
  }

  const engineById = new Map(engineSecond.accounts.map((account) => [account.account_id, account]));
  for (const account of engineSecond.accounts) {
    const owners = claims.get(account.account_id) ?? [];
    if (owners.length === 0) blocker(blockers, "engine_account_missing_owner", [account.account_id]);
    if (owners.length > 1) blocker(blockers, "engine_account_owner_collision", [account.account_id]);
    const claim = owners.length === 1 ? owners[0]! : null;
    if (claim && (claim.status === "invalid" || claim.status !== account.status)) {
      blocker(blockers, "account_status_authority_drift", [account.account_id]);
    }
    if (claim?.accountClass === "b2b" && (
      claim.profileMultiplierBp !== claim.commerceMultiplierBp
      || claim.commerceMultiplierBp !== account.multiplier_bp
    )) blocker(blockers, "b2b_multiplier_authority_drift", [account.account_id]);
  }
  for (const accountId of claims.keys()) {
    if (!engineById.has(accountId)) blocker(blockers, "owner_account_missing_from_engine", [accountId]);
  }

  const baseByAccount = new Map(targetAssignments.map((row) => [row.engine_account_id, row]));
  const postCutover = expectation.activationKind === "recovery";
  if (!postCutover) {
    if (commerceInventoryDigest !== expectation.targetCommerceInventoryDigest) {
      blocker(blockers, "commerce_inventory_drift", [commerceInventoryDigest]);
    }
    if (engineSecond.identity_digest !== expectation.targetEngineInventoryDigest) {
      blocker(blockers, "engine_inventory_drift", [engineSecond.identity_digest]);
    }
    if (openkeysSecond.inventory_digest !== expectation.targetOpenkeysInventoryDigest) {
      blocker(blockers, "openkeys_inventory_drift", [openkeysSecond.inventory_digest]);
    }
    if (serviceInventoryDigest !== expectation.targetServiceInventoryDigest) {
      blocker(blockers, "service_inventory_drift", [serviceInventoryDigest]);
    }
  }

  const extensions: Array<{
    account: PricingReleaseInventoryAccountV2;
    claim: AuthorityClaim;
    extension: PricingReleaseAssignmentExtensionV2;
    fundingGeneration: number | null;
    fundingHeadVersion: number | null;
  }> = [];
  for (const account of engineSecond.accounts) {
    const claim = claims.get(account.account_id)?.[0];
    if (!claim) continue;
    const base = baseByAccount.get(account.account_id);
    if (base) {
      if (base.account_class !== claim.accountClass
          || base.owner_context !== claim.context
          || base.owner_id !== claim.ownerId
          || (claim.accountClass === "service" && (
            base.billing_mode !== "meter_only"
            || base.purpose !== claim.purpose
            || base.responsible !== claim.responsible
          ))) blocker(blockers, "base_assignment_authority_drift", [account.account_id]);
      const policy = basePolicyByIdentity.get(policyIdentity(base.policy_id, base.policy_version));
      const policyFailures = validatePolicyIdentity(base, claim, policy);
      if (policyFailures.length > 0) {
        blocker(blockers, "base_assignment_policy_authority_drift", [
          `${account.account_id}:${policyFailures.join(",")}`,
        ]);
      }
      continue;
    }
    if (!postCutover || expectation.expectedHead === null) {
      blocker(blockers, "account_absent_from_base_manifest", [account.account_id]);
      continue;
    }
    const extension = await readers.engine.getPricingReleaseAssignmentExtensionV2(
      expectation.expectedHead.head_version,
      account.account_id,
    );
    if (!extension) {
      blocker(blockers, "post_cutover_assignment_extension_missing", [account.account_id]);
      continue;
    }
    let fundingGeneration: number | null = null;
    let fundingHeadVersion: number | null = null;
    if (claim.accountClass !== "service") {
      const funding = await readers.engine.getFundingNormalizationPlanV2(account.account_id);
      if (funding?.status === "normalized") {
        fundingGeneration = funding.funding_generation;
        fundingHeadVersion = funding.funding_head_version;
        if (funding.balance_nano !== account.balance_nano
            || funding.reserved_nano !== account.reserved_nano
            || funding.spent_nano !== account.spent_nano) {
          blocker(blockers, "post_cutover_funding_aggregate_drift", [account.account_id]);
        }
      }
    }
    extensions.push({ account, claim, extension, fundingGeneration, fundingHeadVersion });
  }

  if (extensions.length > 0) {
    const extensionAssignments = extensions.map(({ extension }) => extensionAssignment(
        extension,
        positiveSafeInteger(expectation.targetGeneration, "target generation"),
      )).filter((assignment): assignment is ExtensionAssignment => assignment !== null);
    const policyByIdentity = await readPolicyAuthorityRows(client, extensionAssignments);
    for (const extension of extensions) {
      const assignment = extensionAssignment(
        extension.extension,
        positiveSafeInteger(expectation.targetGeneration, "target generation"),
      );
      const policy = assignment
        ? policyByIdentity.get(policyIdentity(assignment.policy_id, assignment.policy_version))
        : undefined;
      const failures = validateExtensionIdentity({ ...extension, policy, expectation });
      if (failures.length > 0) {
        blocker(blockers, "post_cutover_assignment_extension_drift", [
          `${extension.account.account_id}:${failures.join(",")}`,
        ]);
      }
    }
  }

  const headFinal = await readers.engine.getPricingReleaseHeadV2();
  if (!sameHead(headFinal, expectation.expectedHead)) {
    blocker(blockers, "release_head_changed_during_authority_capture", [
      stage5V2CanonicalJson(headFinal),
    ]);
  }
  blockers.sort((left, right) => Buffer.compare(
    Buffer.from(left.code, "utf8"),
    Buffer.from(right.code, "utf8"),
  ));
  return {
    commerceInventoryDigest,
    engineInventoryDigest: postCutover
      ? expectation.targetEngineInventoryDigest
      : engineSecond.identity_digest,
    openkeysInventoryDigest: openkeysSecond.inventory_digest,
    serviceInventoryDigest,
    blockers,
  };
}
