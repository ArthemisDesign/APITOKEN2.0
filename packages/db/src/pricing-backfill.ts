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
 * One pass over one account does exactly four things, in order:
 *
 * 1. align — the dormant engine scalar (`accounts.mult_bp`, never read by the release path
 *    for billing, but the strict admission's fallback for rule-less scopes) is set to the
 *    fallback DERIVED from the account's live release policy (`deriveReleaseFallbackBp`:
 *    the global rule's payable, else full price), and the commerce mirrors
 *    (`customer_profiles`/`engine_accounts`) are synced in the materialization transaction;
 * 2. materialize — the account's managed policy is re-pinned and materialized at the live
 *    catalog head through the SAME writer registration provisioning uses
 *    (`materializeProvisionedUserPolicy` with the arming step disabled): B2C takes the
 *    current head of `policy:main:global-b2c`, B2B the account's own `b2b_client` policy,
 *    with per-model/provider scopes preserved exactly by the materializer. Strict bindings
 *    run it too: the reuse branch is a no-op for a current pin, and a stale catalog pin
 *    (whose strict delivery can never confirm) is re-pinned to the live head.
 * 3. equivalence — the account's release-side resolution (assignment extension over base,
 *    pinned policy, model → provider → global precedence — mirroring the engine's
 *    `pricing_release_resolution_v2_in_transaction`) must resolve every scope to exactly
 *    the payable multiplier the strict policy charges (`assertReleaseStrictEquivalence`),
 *    and the engine scalar must observably equal the aligned fallback — the proof that the
 *    alignment landed BEFORE the chain proceeds. For B2C this is the 5000-global identity;
 *    for B2B the two rule sets were built from the same `pricing_policy_rules` source, so a
 *    scope-walk comparison is exact. A mismatch is never forced: the account keeps its
 *    release coverage, the reason lands on the binding's last_error, and the next pass
 *    re-evaluates — an operator fix (e.g. a corrected assignment extension) is picked up
 *    automatically.
 * 4. arm — `strict_chain_pending` is armed, handing the account to the UNMODIFIED
 *    new-account chain (`packages/db/src/strict-chain.ts`): the fast-tick flush drives the
 *    shared preflight, the durable strict staging, and the one-way engine opt-out marker,
 *    which disarms the flag and records the durable `pricing_release.opt_out` audit entry
 *    that removes the account from this lane's candidate set.
 *
 * Candidate selection is fail-closed on every exclusion: commerce-known customer bindings
 * only (`account_class IN ('b2c','b2b') AND user_id IS NOT NULL`), service accounts doubly
 * excluded (by the binding identity CHECK they never carry a user binding, and by an
 * explicit `service_account_inventory_v2` probe), terminal skips excluded (the hot-loop
 * guard), and accounts with a recorded opt-out audit entry excluded (done). Accounts ARMED
 * in the pre-verification wave (`strict_chain_pending` already true) are included exactly
 * when the chain structurally cannot advance them — `reconciliation_state = 'pending'`:
 * for those the sweep runs the same idempotent steps (materialize/re-pin → account-local
 * verification) and hands them BACK to the chain instead of re-arming; armed accounts the
 * chain can already drive ('verified') stay exclusively with the fast lane. The
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
  /** Already armed (pre-verification wave): the sweep materializes + verifies, the chain owns. */
  strictChainPending: boolean;
};

export type PricingBackfillAdvanceResult =
  // The chain flag is armed; the existing fast-tick strict chain owns the account from here.
  // `noReleaseCoverage` marks accounts that had no release assignment/extension at all (the
  // broken-window cohort): their equivalence gate was skipped by design — the release
  // resolver already fails closed on them today, exactly like phase-2.1 new accounts.
  | { status: "armed"; noReleaseCoverage: boolean }
  // The binding's delivery has not converged to a verifiable state yet (desired≠applied, or
  // the engine cross-check disagrees): left un-armed and quiet, rotated to the back of the
  // queue, re-evaluated on a later pass. Arming earlier would hand the chain a binding it
  // cannot stage (it requires reconciliation 'verified') — the deadlock this state avoids.
  | { status: "pending" }
  // A precondition failed loudly; last_error is recorded and the next pass re-evaluates.
  | { status: "failed"; error: string };

export type PricingBackfillSweepSummary = {
  examined: number;
  /** Engine account ids whose strict chain was armed during this pass. */
  armed: string[];
  /** Armed accounts with NO release coverage under the active head (gate skipped by design). */
  armedWithoutReleaseCoverage: string[];
  /** Candidates whose delivery is still converging (not armed, no error, quiet). */
  pending: string[];
  /** Per-account failures, isolated: one account never blocks the others. */
  failed: Array<{ engineAccountId: string; error: string }>;
};

/**
 * Terminal skip marker (last_error prefix): a failure that cannot self-heal inside the lane
 * (today: no managed pricing policy to materialize) is written ONCE and excluded from
 * candidate selection forever after — the hot-loop guard. The account stays visible in the
 * pipeline-health failed count and recent-failures list; an operator repair clears the
 * marker (`last_error = NULL`) after fixing the policy, returning the account to the sweep.
 */
export const PRICING_BACKFILL_TERMINAL_SKIP_PREFIX = "terminal: ";

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
  | "getAccount"
  | "getAccountPricingState"
  | "getPricingReleaseAssignmentExtensionV2"
  | "getPricingReleaseHeadV2"
  | "getPricingReleasePolicyV2"
  | "getPricingReleaseV2"
  | "setAccountMultiplier"
>;

const B2C_GLOBAL_PAYABLE_BP = 5_000;
const STRICT_B2B_FALLBACK_BP = 10_000;

/**
 * The account's release-resolution fallback for rule-less scopes, DERIVED from the live
 * release policy (never hardcoded): the global rule's payable when the policy carries one
 * (B2C — the engine-validated 5000), else full price. Release B2B policies cannot carry a
 * global rule (engine validation), so their rule-less scopes are simply not served under
 * release and 10000 is the only honest strict fallback. The strict/policy_v1 admission
 * applies `accounts.mult_bp` to exactly those scopes, so the scalar — dormant for current
 * billing — must be aligned to this value before the opt-out marker lands.
 */
export function deriveReleaseFallbackBp(
  policy: Pick<PricingReleasePolicyV2, "rules">,
): number {
  const globalRule = policy.rules.find((rule) => rule.scope.scope === "global");
  return globalRule?.payable_multiplier_bp ?? STRICT_B2B_FALLBACK_BP;
}

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
    strict_chain_pending: boolean;
  }>(
    `
    SELECT binding.id::text AS binding_id, binding.user_id::text,
           binding.engine_account_id, binding.account_class,
           binding.policy_enforcement, binding.strict_chain_pending
    FROM account_policy_bindings binding
    JOIN customer_profiles profile ON profile.user_id = binding.user_id
    WHERE binding.user_id IS NOT NULL
      AND binding.account_class IN ('b2c', 'b2b')
      -- Unarmed accounts, plus armed ones the chain structurally cannot advance
      -- (reconciliation 'pending': unverifiable; sync 'pending': the strict delivery has
      -- not confirmed — the sweep re-materializes a stale catalog pin first, then verifies
      -- account-locally) and hands them back; armed accounts the chain can already drive
      -- stay exclusively with the fast lane.
      AND (NOT binding.strict_chain_pending
           OR binding.reconciliation_state = 'pending'
           OR binding.sync_state = 'pending')
      AND (binding.last_error IS NULL OR binding.last_error NOT LIKE 'terminal: %')
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
    strictChainPending: row.strict_chain_pending,
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

/**
 * The release assignment the account resolves under: the head-pinned extension wins over the base.
 * Returns null ONLY when the account has no release coverage at all under the active head
 * (no extension and no base assignment) — accounts registered in the window between the
 * extension-removal and the backfill have a confirmed commerce binding but were never
 * written into the release authority, and the release resolver already fails closed on them
 * today: there is no release-side resolution to diverge from, exactly like a phase-2.1
 * new account. Every OTHER anomaly (moved head, unreadable release/policy, digest drift,
 * extension contract violation) stays a hard equivalence error — a present-but-divergent
 * assignment must keep blocking.
 */
async function resolveAccountReleasePolicy(
  engine: PricingBackfillReleaseTransport,
  accountId: string,
): Promise<PricingReleasePolicyV2 | null> {
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
  }
  if (assignment === undefined) {
    // Pin the head before concluding "no coverage": a head CAS between the probe reads could
    // have carried the account's first assignment under a newer head.
    await assertReleaseHeadUnchanged(engine, head);
    return null;
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
  await assertReleaseHeadUnchanged(engine, head);
  return policy;
}

async function assertReleaseHeadUnchanged(
  engine: PricingBackfillReleaseTransport,
  head: { active_generation: number; active_digest: string; head_version: number },
): Promise<void> {
  const closingHead = await engine.getPricingReleaseHeadV2();
  if (
    closingHead === null
    || closingHead.active_generation !== head.active_generation
    || closingHead.active_digest !== head.active_digest
    || closingHead.head_version !== head.head_version
  ) {
    throw new PricingBackfillEquivalenceError("pricing release head moved during the equivalence check");
  }
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
 * Terminal variant of notePricingBackfillFailure: writes the failure ONCE under the
 * PRICING_BACKFILL_TERMINAL_SKIP_PREFIX, which removes the account from candidate selection
 * — the deterministic hot-loop guard. (The transient failure path alone could not guarantee
 * quietness: the control-job delivery lane writes binding.last_error/updated_at
 * unconditionally on its own confirm/release cadence, so a plain last_error could be
 * overwritten and the account re-selected and re-logged every pass.)
 */
async function notePricingBackfillTerminalSkip(
  database: Database,
  bindingId: string,
  error: string,
): Promise<{ status: "failed"; error: string }> {
  return notePricingBackfillFailure(
    database,
    bindingId,
    `${PRICING_BACKFILL_TERMINAL_SKIP_PREFIX}${error}`,
  );
}

/**
 * The deleted shadow-rollout lane's verification step, revived account-locally for the
 * backfill: the strict chain (and the engine's own strict triggers and opt-out guard)
 * require reconciliation_state='verified', but nothing in the live system flips a
 * shadow|strict + 'pending' binding to 'verified' any more — the rollout lane that did it
 * was deleted with the release orchestration, and the delivery confirm path carries the
 * job's own ('pending') reconciliation forward.
 *
 * The evidence is exactly the durable ACK proof the rollout used: sync_state='confirmed'
 * with desired_effective_version = applied_effective_version, matching digests and a
 * recorded last_ack_at — the engine already ACKed and applied precisely this immutable
 * version. It is cross-checked cheaply against the engine's active policy state (version +
 * digest must equal the desired head); a mismatch or desired≠applied means the delivery is
 * still converging and the binding is left 'pending'. Works for shadow and strict bindings
 * alike (the opt-out guard needs strict/strict/verified too).
 */
async function verifyBackfillShadowReconciliation(
  database: Database,
  engine: PricingBackfillReleaseTransport,
  candidate: PricingBackfillCandidate,
): Promise<"verified" | "pending"> {
  const binding = await database.pool.query<{
    reconciliation_state: string;
    sync_state: string;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
    desired_digest: string | null;
    applied_digest: string | null;
    last_ack_at: Date | null;
  }>(
    `
    SELECT reconciliation_state, sync_state,
           desired_effective_version::text, applied_effective_version::text,
           desired_digest, applied_digest, last_ack_at
    FROM account_policy_bindings WHERE id = $1
  `,
    [candidate.bindingId],
  );
  const row = binding.rows[0];
  if (!row) return "pending";
  if (row.reconciliation_state === "verified") return "verified";
  const converged = row.sync_state === "confirmed"
    && row.desired_effective_version !== null
    && row.desired_effective_version === row.applied_effective_version
    && row.desired_digest !== null
    && row.desired_digest === row.applied_digest
    && row.last_ack_at !== null;
  if (!converged) return "pending";
  const state = await engine.getAccountPricingState(candidate.engineAccountId);
  if (typeof state !== "object" || state === null || !("active" in state)) return "pending";
  if (
    state.active.policy.effective_version !== Number(row.desired_effective_version)
    || state.active.policy.content_digest !== row.desired_digest
  ) {
    return "pending";
  }
  const updated = await database.pool.query(
    `
    UPDATE account_policy_bindings
    SET reconciliation_state = 'verified', updated_at = now()
    WHERE id = $1 AND reconciliation_state = 'pending'
      AND sync_state = 'confirmed'
      AND desired_effective_version IS NOT NULL
      AND desired_effective_version = applied_effective_version
      AND desired_digest = applied_digest
  `,
    [candidate.bindingId],
  );
  return (updated.rowCount ?? 0) === 1 ? "verified" : "pending";
}

/**
 * Advances one existing account one lane step, in an order that keeps the dormant scalar
 * faithful at every moment:
 *
 * 1. resolve the account's live release policy and DERIVE the rule-less-scope fallback
 *    (`deriveReleaseFallbackBp`);
 * 2. align the ENGINE scalar (`account_set_mult_bp`, idempotent) — under the release path
 *    `accounts.mult_bp` is never read for billing (the reserve multiplier comes from the
 *    release resolution), so this cannot change what the account pays today; it only fixes
 *    the fallback the strict admission will apply to rule-less scopes after the opt-out.
 *    The legacy `engine_pricing_jobs` stream is deliberately not used: `claimNextPricingJob`
 *    drains scalar jobs for strict-bound accounts without an engine write, and this lane is
 *    itself the durable, resumable driver that re-asserts the value before arming;
 * 3. materialize the managed policy at the live catalog head with the commerce scalar
 *    mirrors aligned in the same transaction (already-strict bindings get the standalone
 *    mirror sync instead — their enforced policy is already the desired one);
 * 4. prove release/strict equivalence, now including the engine-side scalar convergence —
 *    this is what makes the alignment provably land BEFORE the chain (and therefore the
 *    opt-out marker) is allowed to proceed. The scope-walk gate is skipped ONLY for
 *    accounts with no release coverage at all under the active head (no extension, no base
 *    assignment): the release resolver already fails closed on them today, so there is no
 *    release-side resolution to diverge from — they proceed exactly like phase-2.1 new
 *    accounts, and the skip is reported (`noReleaseCoverage`). A present-but-divergent
 *    assignment keeps blocking;
 * 5. arm the shared strict chain.
 *
 * Typed precondition failures are recorded on the binding and returned; transport failures
 * propagate to the sweep, which isolates them per account and retries on the next pass.
 */
export async function advancePricingBackfillAccount(
  database: Database,
  engine: PricingBackfillReleaseTransport,
  candidate: PricingBackfillCandidate,
): Promise<PricingBackfillAdvanceResult> {
  let releasePolicy: PricingReleasePolicyV2 | null;
  try {
    releasePolicy = await resolveAccountReleasePolicy(engine, candidate.engineAccountId);
  } catch (error) {
    if (error instanceof PricingBackfillEquivalenceError) {
      return notePricingBackfillFailure(database, candidate.bindingId, error.message);
    }
    throw error;
  }
  // With no release coverage there is no policy to derive from: the class identity is the
  // faithful fallback (B2C 5000, B2B full price — the same default provisioning uses).
  const fallbackBp = releasePolicy === null
    ? (candidate.accountClass === "b2c" ? B2C_GLOBAL_PAYABLE_BP : STRICT_B2B_FALLBACK_BP)
    : deriveReleaseFallbackBp(releasePolicy);
  await engine.setAccountMultiplier(candidate.engineAccountId, fallbackBp);

  // Materialize runs for EVERY candidate class, strict ones included: an armed strict
  // binding whose policy pins a stale catalog generation can never confirm its strict
  // delivery (the engine rejects the activation with missing_dependency on the old pin),
  // and only this step re-pins it to the live head. For a current strict binding the reuse
  // branch makes it a no-op — no new version, no churn.
  try {
    const materialized = await materializeProvisionedUserPolicy(
      database,
      { userId: candidate.userId, engineAccountId: candidate.engineAccountId },
      { armStrictChain: false, alignScalarBp: fallbackBp },
    );
    if (!materialized.policyRequired) {
      // Permanent skip: there is no managed policy to materialize from, so nothing in the
      // lane can ever advance this account — mark it terminal instead of re-logging it
      // every pass. Stays visible in pipeline-health; an operator repair clears the marker.
      return notePricingBackfillTerminalSkip(
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
    // The scope-walk gate applies ONLY to accounts with release coverage: an account with no
    // assignment/extension under the active head is unservable-by-design on the release path
    // today, so there is no release-side resolution to diverge from — it proceeds like a
    // phase-2.1 new account. A present-but-divergent assignment still blocks in here.
    if (releasePolicy !== null) {
      assertReleaseStrictEquivalence({
        accountClass: candidate.accountClass,
        // The strict fallback the engine will apply after the opt-out IS the aligned scalar —
        // use the derived release fallback, not the (just overwritten) commerce mirror.
        strictFallbackBp: fallbackBp,
        strictRules: rules.rows.map((rule) => ({
          scopeType: rule.scope_type,
          providerId: rule.provider_id,
          canonicalModelId: rule.canonical_model_id,
          payableMultiplierBp: rule.payable_multiplier_bp,
        })),
        releasePolicy,
      });
    }
    // The ordering proof: the engine scalar must observably equal the aligned fallback
    // before the chain is armed — a concurrent re-point (e.g. an admin multiplier save)
    // between the write above and this read blocks the arm and is retried next pass.
    const engineAccount = await engine.getAccount(candidate.engineAccountId);
    if (engineAccount.mult_bp !== fallbackBp) {
      throw new PricingBackfillEquivalenceError(
        `engine account scalar is ${engineAccount.mult_bp} bp, expected the aligned ${fallbackBp} bp`,
      );
    }
  } catch (error) {
    if (error instanceof PricingBackfillEquivalenceError) {
      return notePricingBackfillFailure(database, candidate.bindingId, error.message);
    }
    throw error;
  }

  // The deleted rollout lane's verification step, account-locally: the chain can stage (and
  // the engine opt-out guard can pass) only a 'verified' binding, and nothing else in the
  // live system flips 'pending' → 'verified' any more. Converged bindings are verified here
  // with the durable ACK proof + engine cross-check; unconverged ones stay un-armed and
  // rotate quietly — arming them is precisely the deadlock.
  const reconciliation = await verifyBackfillShadowReconciliation(database, engine, candidate);
  if (reconciliation === "pending") {
    await database.pool.query(
      `
      UPDATE account_policy_bindings SET updated_at = now() WHERE id = $1
    `,
      [candidate.bindingId],
    );
    return { status: "pending" };
  }

  if (candidate.strictChainPending) {
    // Armed in the pre-verification wave: the chain already owns this account — the sweep's
    // job was materialize (re-pin included) + verification only. Never re-arm (the arm
    // write below stays idempotent regardless); rotate quietly and let the fast tick stage
    // it from here.
    await database.pool.query(
      `
      UPDATE account_policy_bindings SET updated_at = now() WHERE id = $1
    `,
      [candidate.bindingId],
    );
    return { status: "pending" };
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
  return { status: "armed", noReleaseCoverage: releasePolicy === null };
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
  const summary: PricingBackfillSweepSummary = {
    examined: candidates.length,
    armed: [],
    armedWithoutReleaseCoverage: [],
    pending: [],
    failed: [],
  };
  for (const candidate of candidates) {
    try {
      const result = await advancePricingBackfillAccount(database, engine, candidate);
      if (result.status === "armed") {
        summary.armed.push(candidate.engineAccountId);
        if (result.noReleaseCoverage) {
          summary.armedWithoutReleaseCoverage.push(candidate.engineAccountId);
        }
      } else if (result.status === "pending") {
        summary.pending.push(candidate.engineAccountId);
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
