import type {
  PricingReleaseAssignmentV2,
  PricingReleasePolicyV2,
} from "@claude-api/contracts";
import type { EngineClient } from "@claude-api/engine-client";
import type { Database } from "./client.js";
import {
  materializeProvisionedUserPolicy,
  PricingPolicyWriteError,
} from "./pricing-policy-write.js";
import { PRICING_RELEASE_OPT_OUT_AUDIT_ACTION } from "./strict-chain.js";

/**
 * The existing-account backfill of the release-v2 retirement (phase 2.2,
 * docs/commerce/PRICING.md "Existing-account backfill", runbook
 * docs/ops/PRICING_RELEASE_BACKFILL.md). Phase 2.1 graduates only NEW accounts through the
 * direct strict chain; this lane moves the pre-existing fleet, canary-first and resumable.
 *
 * One pass over one account does exactly three things, in order:
 *
 * 1. materialize — the account's managed policy is re-pinned and materialized at the live
 *    catalog head through the SAME writer registration provisioning uses
 *    (`materializeProvisionedUserPolicy` with the arming step disabled): B2C takes the
 *    current head of `policy:main:global-b2c`, B2B the account's own `b2b_client` policy,
 *    with per-model/provider scopes preserved exactly by the materializer. A binding that
 *    is already strict skips this step — its enforced policy is already the desired one.
 * 2. equivalence — the account's release-side resolution (assignment extension over base,
 *    pinned policy, model → provider → global precedence — mirroring the engine's
 *    `pricing_release_resolution_v2_in_transaction`) must resolve every scope to exactly
 *    the payable multiplier the strict policy charges (`assertReleaseStrictEquivalence`).
 *    For B2C this is the 5000-global identity; for B2B the two rule sets were built from
 *    the same `pricing_policy_rules` source, so a scope-walk comparison is exact. A
 *    mismatch is never forced: the account keeps its release coverage, the reason lands on
 *    the binding's last_error, and the next pass re-evaluates — an operator fix
 *    (e.g. a corrected assignment extension) is picked up automatically.
 * 3. arm — `strict_chain_pending` is armed, handing the account to the UNMODIFIED
 *    new-account chain (`packages/db/src/strict-chain.ts`): the fast-tick flush drives the
 *    shared preflight, the durable strict staging, and the one-way engine opt-out marker,
 *    which disarms the flag and records the durable `pricing_release.opt_out` audit entry
 *    that removes the account from this lane's candidate set.
 *
 * Candidate selection is fail-closed on every exclusion: commerce-known customer bindings
 * only (`account_class IN ('b2c','b2b') AND user_id IS NOT NULL`), service accounts doubly
 * excluded (by the binding identity CHECK they never carry a user binding, and by an
 * explicit `service_account_inventory_v2` probe), already-armed chains excluded (the fast
 * lane owns them), and accounts with a recorded opt-out audit entry excluded (done). The
 * engine itself remains the ultimate arbiter: its opt-out replay is `unchanged`, so a
 * lost audit write can never double-apply the marker.
 *
 * In-flight release reservations are NOT special-cased here: the migration-0016 strict
 * trigger rejects the engine-side flip while they drain, which surfaces as a retryable 503
 * through the control-job lane's exponential backoff — calm retries, never a hot loop.
 */

export type PricingBackfillCandidate = {
  bindingId: string;
  userId: string;
  engineAccountId: string;
  accountClass: "b2c" | "b2b";
  policyEnforcement: string;
  multiplierBp: number;
};

export type PricingBackfillAdvanceResult =
  // The chain flag is armed; the existing fast-tick strict chain owns the account from here.
  | { status: "armed" }
  // A precondition failed loudly; last_error is recorded and the next pass re-evaluates.
  | { status: "failed"; error: string };

export type PricingBackfillSweepSummary = {
  examined: number;
  /** Engine account ids whose strict chain was armed during this pass. */
  armed: string[];
  /** Per-account failures, isolated: one account never blocks the others. */
  failed: Array<{ engineAccountId: string; error: string }>;
};

export class PricingBackfillEquivalenceError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PricingBackfillEquivalenceError";
  }
}

export type PricingBackfillStrictRule = {
  scopeType: "provider" | "model";
  providerId: string;
  canonicalModelId: string | null;
  payableMultiplierBp: number;
};

export type PricingBackfillReleaseTransport = Pick<
  EngineClient,
  | "getPricingReleaseAssignmentExtensionV2"
  | "getPricingReleaseHeadV2"
  | "getPricingReleasePolicyV2"
  | "getPricingReleaseV2"
>;

const B2C_GLOBAL_PAYABLE_BP = 5_000;
const STRICT_B2B_FALLBACK_BP = 10_000;

/**
 * One bounded page of backfill candidates, oldest-touch first so a persistently failing
 * account (its last_error bumps updated_at) rotates to the back of the queue instead of
 * monopolizing the batch. `allowlist` restricts the page to exactly the listed engine
 * account ids (canary mode); `undefined` sweeps the whole eligible set.
 */
export async function listPricingBackfillCandidates(
  database: Database,
  options: { limit: number; allowlist?: readonly string[] },
): Promise<PricingBackfillCandidate[]> {
  const result = await database.pool.query<{
    binding_id: string;
    user_id: string;
    engine_account_id: string;
    account_class: "b2c" | "b2b";
    policy_enforcement: string;
    multiplier_bp: number;
  }>(
    `
    SELECT binding.id::text AS binding_id, binding.user_id::text,
           binding.engine_account_id, binding.account_class,
           binding.policy_enforcement, profile.multiplier_bp
    FROM account_policy_bindings binding
    JOIN customer_profiles profile ON profile.user_id = binding.user_id
    WHERE binding.user_id IS NOT NULL
      AND binding.account_class IN ('b2c', 'b2b')
      AND NOT binding.strict_chain_pending
      AND NOT EXISTS (
        SELECT 1 FROM service_account_inventory_v2 service
        WHERE service.engine_account_id = binding.engine_account_id
      )
      AND NOT EXISTS (
        SELECT 1 FROM audit_log opt_out
        WHERE opt_out.action = $2
          AND opt_out.target_type = 'engine_account'
          AND opt_out.target_id = binding.engine_account_id
      )
      AND ($3::text[] IS NULL OR binding.engine_account_id = ANY($3))
    ORDER BY binding.updated_at, binding.id
    LIMIT $1
  `,
    [options.limit, PRICING_RELEASE_OPT_OUT_AUDIT_ACTION, options.allowlist ?? null],
  );
  return result.rows.map((row) => ({
    bindingId: row.binding_id,
    userId: row.user_id,
    engineAccountId: row.engine_account_id,
    accountClass: row.account_class,
    policyEnforcement: row.policy_enforcement,
    multiplierBp: row.multiplier_bp,
  }));
}

type ScopeMap = ReadonlyMap<string, number>;

function strictScopeKey(rule: PricingBackfillStrictRule): string {
  return rule.scopeType === "provider"
    ? `provider:${rule.providerId}`
    : `model:${rule.providerId}/${rule.canonicalModelId ?? ""}`;
}

function releaseScopeKey(rule: PricingReleasePolicyV2["rules"][number]): string {
  if (rule.scope.scope === "global") return "global";
  if (rule.scope.scope === "provider") return `provider:${rule.scope.provider_id}`;
  return `model:${rule.scope.provider_id}/${rule.scope.canonical_model_id}`;
}

function scopeKeyParts(key: string): { providerId: string; canonicalModelId: string | null } {
  if (key.startsWith("model:")) {
    const separator = key.indexOf("/", "model:".length);
    return {
      providerId: key.slice("model:".length, separator),
      canonicalModelId: key.slice(separator + 1),
    };
  }
  return { providerId: key.slice("provider:".length), canonicalModelId: null };
}

/** Engine precedence (postgres.rs pricing_release_resolution_v2_in_transaction): model → provider → global. */
function resolveReleasePayable(release: ScopeMap, scope: { providerId: string; canonicalModelId: string | null }): number | null {
  if (scope.canonicalModelId !== null) {
    const model = release.get(`model:${scope.providerId}/${scope.canonicalModelId}`);
    if (model !== undefined) return model;
  }
  const provider = release.get(`provider:${scope.providerId}`);
  if (provider !== undefined) return provider;
  return release.get("global") ?? null;
}

/** Strict/policy_v1 precedence: model rule → provider rule → the account's effective multiplier. */
function resolveStrictPayable(
  strict: ScopeMap,
  fallbackBp: number,
  scope: { providerId: string; canonicalModelId: string | null },
): number {
  if (scope.canonicalModelId !== null) {
    const model = strict.get(`model:${scope.providerId}/${scope.canonicalModelId}`);
    if (model !== undefined) return model;
  }
  const provider = strict.get(`provider:${scope.providerId}`);
  if (provider !== undefined) return provider;
  return fallbackBp;
}

/**
 * The mechanical pre-opt-out equivalence proof: the release policy the account resolves
 * under today must charge exactly what the strict policy it is about to be opted into
 * charges, scope by scope. Both rule sets are normalized to `scope → payable_multiplier_bp`
 * maps and compared over their union with each side's own precedence walk, so the check is
 * exact for the B2C 5000-global identity and for B2B rule sets built from the same managed
 * source. The two sides' stored digests live in different frozen domains (`sha256:v1:` vs
 * `sha256:v2:`) and are never compared — only normalized values are. Any divergence throws
 * a PricingBackfillEquivalenceError; the caller skips the account.
 */
export function assertReleaseStrictEquivalence(input: {
  accountClass: "b2c" | "b2b";
  strictFallbackBp: number;
  strictRules: readonly PricingBackfillStrictRule[];
  releasePolicy: Pick<
    PricingReleasePolicyV2,
    "policy_id" | "policy_version" | "account_class" | "billing_mode" | "rules"
  >;
}): void {
  const policy = input.releasePolicy;
  if (policy.account_class !== input.accountClass) {
    throw new PricingBackfillEquivalenceError(
      `release policy ${policy.policy_id} v${policy.policy_version} has account_class ` +
      `${policy.account_class}, expected ${input.accountClass}`,
    );
  }
  if (policy.billing_mode !== "balance") {
    throw new PricingBackfillEquivalenceError(
      `release policy ${policy.policy_id} v${policy.policy_version} is ${policy.billing_mode}, ` +
      "only balance-mode accounts are backfilled",
    );
  }

  const strict: Map<string, number> = new Map();
  for (const rule of input.strictRules) {
    const key = strictScopeKey(rule);
    if (strict.has(key)) {
      throw new PricingBackfillEquivalenceError(`strict policy has a duplicate ${key} rule`);
    }
    strict.set(key, rule.payableMultiplierBp);
  }
  const release: Map<string, number> = new Map();
  for (const rule of policy.rules) {
    const key = releaseScopeKey(rule);
    if (release.has(key)) {
      throw new PricingBackfillEquivalenceError(`release policy has a duplicate ${key} rule`);
    }
    release.set(key, rule.payable_multiplier_bp);
  }

  const releaseGlobal = release.get("global");
  if (input.accountClass === "b2c") {
    if (releaseGlobal !== B2C_GLOBAL_PAYABLE_BP) {
      throw new PricingBackfillEquivalenceError(
        `B2C release policy ${policy.policy_id} global rule resolves to ${releaseGlobal ?? "nothing"}, ` +
        `expected the ${B2C_GLOBAL_PAYABLE_BP} bp identity`,
      );
    }
    if (input.strictFallbackBp !== B2C_GLOBAL_PAYABLE_BP) {
      throw new PricingBackfillEquivalenceError(
        `B2C account effective multiplier is ${input.strictFallbackBp} bp, expected the ` +
        `${B2C_GLOBAL_PAYABLE_BP} bp identity`,
      );
    }
  } else if (releaseGlobal !== undefined) {
    throw new PricingBackfillEquivalenceError(
      `B2B release policy ${policy.policy_id} carries a global rule, which the strict path cannot express`,
    );
  }

  const scopes = new Set([...strict.keys(), ...release.keys()]);
  scopes.delete("global");
  for (const key of scopes) {
    const scope = scopeKeyParts(key);
    const releasePayable = resolveReleasePayable(release, scope);
    const strictPayable = resolveStrictPayable(strict, input.strictFallbackBp, scope);
    if (releasePayable === null) {
      throw new PricingBackfillEquivalenceError(
        `release policy ${policy.policy_id} does not cover ${key} (strict would charge ${strictPayable} bp)`,
      );
    }
    if (releasePayable !== strictPayable) {
      throw new PricingBackfillEquivalenceError(
        `${key}: release resolves to ${releasePayable} bp, strict charges ${strictPayable} bp`,
      );
    }
  }
}

/** The release assignment the account resolves under: the head-pinned extension wins over the base. */
async function resolveAccountReleasePolicy(
  engine: PricingBackfillReleaseTransport,
  accountId: string,
): Promise<PricingReleasePolicyV2> {
  const head = await engine.getPricingReleaseHeadV2();
  if (head === null) {
    throw new PricingBackfillEquivalenceError("engine has no active pricing release head");
  }
  let assignment: PricingReleaseAssignmentV2 | undefined;
  const extension = await engine.getPricingReleaseAssignmentExtensionV2(head.head_version, accountId);
  if (extension !== null) {
    assignment = extension.members.find((member) => member.assignment.account_id === accountId)?.assignment;
    if (assignment === undefined) {
      throw new PricingBackfillEquivalenceError(
        "assignment extension does not contain the account (engine contract violation)",
      );
    }
  } else {
    const release = await engine.getPricingReleaseV2(head.active_generation);
    if (release === null) {
      throw new PricingBackfillEquivalenceError("active pricing release generation is not readable");
    }
    if (release.content_digest !== head.active_digest) {
      throw new PricingBackfillEquivalenceError("pricing release head moved during the equivalence check");
    }
    assignment = release.assignments.find((candidate) => candidate.account_id === accountId);
    if (assignment === undefined) {
      throw new PricingBackfillEquivalenceError("account has no assignment in the active pricing release");
    }
  }
  const policy = await engine.getPricingReleasePolicyV2(assignment.policy_id, assignment.policy_version);
  if (policy === null) {
    throw new PricingBackfillEquivalenceError(
      `release policy ${assignment.policy_id} v${assignment.policy_version} is not readable`,
    );
  }
  if (policy.content_digest !== assignment.policy_digest) {
    throw new PricingBackfillEquivalenceError(
      `release assignment pins digest ${assignment.policy_digest} but the stored policy has ${policy.content_digest}`,
    );
  }
  // The whole check is pinned to ONE head: a head CAS between the first read and here (an
  // operator event) could have re-pinned the assignment/extension under a newer head while we
  // compared against the old one. Re-read the head and refuse — the next pass re-checks.
  const closingHead = await engine.getPricingReleaseHeadV2();
  if (
    closingHead === null
    || closingHead.active_generation !== head.active_generation
    || closingHead.active_digest !== head.active_digest
    || closingHead.head_version !== head.head_version
  ) {
    throw new PricingBackfillEquivalenceError("pricing release head moved during the equivalence check");
  }
  return policy;
}

async function notePricingBackfillFailure(
  database: Database,
  bindingId: string,
  error: string,
): Promise<{ status: "failed"; error: string }> {
  const truncated = error.slice(0, 2000);
  await database.pool.query(
    `
    UPDATE account_policy_bindings SET last_error = $2, updated_at = now()
    WHERE id = $1
  `,
    [bindingId, truncated],
  );
  return { status: "failed", error: truncated };
}

/**
 * Advances one existing account one lane step: materialize at the live catalog head (unless
 * already strict), prove release/strict equivalence, then arm the shared strict chain.
 * Typed precondition failures are recorded on the binding and returned; transport failures
 * propagate to the sweep, which isolates them per account and retries on the next pass.
 */
export async function advancePricingBackfillAccount(
  database: Database,
  engine: PricingBackfillReleaseTransport,
  candidate: PricingBackfillCandidate,
): Promise<PricingBackfillAdvanceResult> {
  if (candidate.policyEnforcement !== "strict") {
    try {
      const materialized = await materializeProvisionedUserPolicy(
        database,
        { userId: candidate.userId, engineAccountId: candidate.engineAccountId },
        { armStrictChain: false },
      );
      if (!materialized.policyRequired) {
        return notePricingBackfillFailure(
          database,
          candidate.bindingId,
          "account has no managed pricing policy to materialize",
        );
      }
    } catch (error) {
      if (error instanceof PricingPolicyWriteError) {
        return notePricingBackfillFailure(database, candidate.bindingId, error.message);
      }
      throw error;
    }
  }

  const binding = await database.pool.query<{
    desired_effective_version: string | null;
  }>(
    `
    SELECT desired_effective_version::text
    FROM account_policy_bindings WHERE id = $1
  `,
    [candidate.bindingId],
  );
  const desired = binding.rows[0]?.desired_effective_version;
  if (desired === null || desired === undefined) {
    return notePricingBackfillFailure(
      database,
      candidate.bindingId,
      "binding has no materialized desired policy version",
    );
  }
  const rules = await database.pool.query<{
    scope_type: "provider" | "model";
    provider_id: string;
    canonical_model_id: string | null;
    payable_multiplier_bp: number;
  }>(
    `
    SELECT scope_type, provider_id, canonical_model_id, payable_multiplier_bp
    FROM account_policy_rules
    WHERE binding_id = $1 AND effective_version = $2
    ORDER BY provider_id, scope_type, COALESCE(canonical_model_id, ''), rule_id
  `,
    [candidate.bindingId, desired],
  );
  try {
    const releasePolicy = await resolveAccountReleasePolicy(engine, candidate.engineAccountId);
    assertReleaseStrictEquivalence({
      accountClass: candidate.accountClass,
      strictFallbackBp: candidate.accountClass === "b2c"
        ? candidate.multiplierBp
        : STRICT_B2B_FALLBACK_BP,
      strictRules: rules.rows.map((rule) => ({
        scopeType: rule.scope_type,
        providerId: rule.provider_id,
        canonicalModelId: rule.canonical_model_id,
        payableMultiplierBp: rule.payable_multiplier_bp,
      })),
      releasePolicy,
    });
  } catch (error) {
    if (error instanceof PricingBackfillEquivalenceError) {
      return notePricingBackfillFailure(database, candidate.bindingId, error.message);
    }
    throw error;
  }

  // Hand the account to the unmodified direct strict chain: the fast-tick flush drives the
  // preflight, the durable strict staging and the opt-out marker from here. last_error is
  // cleared so the progress surface reflects the fresh armed state.
  await database.pool.query(
    `
    UPDATE account_policy_bindings
    SET strict_chain_pending = true, last_error = NULL, updated_at = now()
    WHERE id = $1 AND NOT strict_chain_pending
  `,
    [candidate.bindingId],
  );
  return { status: "armed" };
}

/**
 * One bounded backfill pass: every candidate is advanced independently — a single account's
 * failure (typed or transport) is recorded in the summary and never blocks the others. The
 * pass is resumable by construction: armed and completed accounts leave the candidate set,
 * and the remainder is re-listed on the next pass in oldest-touch order.
 */
export async function runPricingBackfillSweep(
  database: Database,
  engine: PricingBackfillReleaseTransport,
  options: { limit: number; allowlist?: readonly string[] },
): Promise<PricingBackfillSweepSummary> {
  const candidates = await listPricingBackfillCandidates(database, options);
  const summary: PricingBackfillSweepSummary = { examined: candidates.length, armed: [], failed: [] };
  for (const candidate of candidates) {
    try {
      const result = await advancePricingBackfillAccount(database, engine, candidate);
      if (result.status === "armed") {
        summary.armed.push(candidate.engineAccountId);
      } else {
        summary.failed.push({ engineAccountId: candidate.engineAccountId, error: result.error });
      }
    } catch (error) {
      summary.failed.push({
        engineAccountId: candidate.engineAccountId,
        error: error instanceof Error ? error.message : "pricing backfill advance failed",
      });
    }
  }
  return summary;
}
