import {
  AccountStrictCutoverPreflightError,
  ensureAccountStrictCutoverPreflight,
  type AccountStrictCutoverPreflightTransport,
} from "@claude-api/engine-client";
import type { Database } from "./client.js";
import {
  AccountStrictCutoverError,
  pricingReleaseCutoverCompleted,
  stageAccountStrictCutoverJob,
} from "./pricing-control-jobs.js";

/**
 * Automatic half of the per-account strict cutover lane (docs/commerce/PRICING.md,
 * docs/commerce/MULTI-DISCOUNT.md decisions 13–14). A B2C→B2B conversion and every
 * b2b_client policy save set `strict_chain_pending` on the account binding; the pricing worker
 * sweep then advances the chain account-locally once the exact saved policy version is
 * confirmed under shadow: shared engine preflight (funding normalization + active-key ACK
 * stamps), then the atomic strict+strict+verified staging. The staging transaction clears the
 * flag, so a replay never duplicates the cutover, and a binding that is already strict is just
 * disarmed. A failed precondition is recorded on the binding's last_error and retried on the
 * next sweep; it never produces a partial silent state.
 *
 * The lane is pre-cutover only: once the global release cutover receipt is durable, the
 * release-v2 authority owns admission and pricing, and per-account B2B enforcement moves
 * through the append-only assignment extension lane. The writers stop arming the flag at that
 * point, and the sweep disarms any straggler as `superseded` without touching the engine.
 */

export type PendingStrictChainAccount = {
  bindingId: string;
  userId: string;
  engineAccountId: string;
  policyEnforcement: string;
  reconciliationState: string;
  syncState: string;
  desiredEffectiveVersion: string | null;
  appliedEffectiveVersion: string | null;
};

export type AccountStrictChainAdvanceResult =
  | { status: "staged"; jobId: string | null; funding: string; keysStamped: number }
  | { status: "already_strict" }
  // The global release cutover has completed; the flag was disarmed without engine I/O because
  // the assignment extension lane owns per-account enforcement now.
  | { status: "superseded" }
  // The shadow delivery of the target version is still in flight (or a newer save moved the
  // desired target mid-sweep); the next pass re-evaluates quietly.
  | { status: "pending" }
  // A precondition failed loudly; last_error is recorded and the next pass retries.
  | { status: "failed"; error: string };

export async function listPendingStrictChainAccounts(
  database: Database,
  limit: number,
): Promise<PendingStrictChainAccount[]> {
  const result = await database.pool.query<{
    binding_id: string;
    user_id: string;
    engine_account_id: string;
    policy_enforcement: string;
    reconciliation_state: string;
    sync_state: string;
    desired_effective_version: string | null;
    applied_effective_version: string | null;
  }>(
    `
    SELECT id::text AS binding_id, user_id::text, engine_account_id,
           policy_enforcement, reconciliation_state, sync_state,
           desired_effective_version::text, applied_effective_version::text
    FROM account_policy_bindings
    WHERE strict_chain_pending AND user_id IS NOT NULL
    ORDER BY updated_at, id
    LIMIT $1
  `,
    [limit],
  );
  return result.rows.map((row) => ({
    bindingId: row.binding_id,
    userId: row.user_id,
    engineAccountId: row.engine_account_id,
    policyEnforcement: row.policy_enforcement,
    reconciliationState: row.reconciliation_state,
    syncState: row.sync_state,
    desiredEffectiveVersion: row.desired_effective_version,
    appliedEffectiveVersion: row.applied_effective_version,
  }));
}

export async function advanceAccountStrictChain(
  database: Database,
  engine: AccountStrictCutoverPreflightTransport,
  candidate: PendingStrictChainAccount,
): Promise<AccountStrictChainAdvanceResult> {
  {
    const client = await database.pool.connect();
    try {
      if (await pricingReleaseCutoverCompleted(client)) {
        await clearStrictChainPending(database, candidate.bindingId);
        return { status: "superseded" };
      }
    } finally {
      client.release();
    }
  }
  if (candidate.policyEnforcement === "strict") {
    await clearStrictChainPending(database, candidate.bindingId);
    return { status: "already_strict" };
  }
  // The strict staging targets exactly the engine-confirmed version, so the chain may fire only
  // when the shadow delivery of the desired version has fully landed; anything else is retried.
  const ready = candidate.policyEnforcement === "shadow"
    && candidate.reconciliationState === "verified"
    && candidate.syncState === "confirmed"
    && candidate.appliedEffectiveVersion !== null
    && candidate.desiredEffectiveVersion === candidate.appliedEffectiveVersion;
  if (!ready) return { status: "pending" };

  let preflight: { funding: string; keysStamped: number };
  try {
    preflight = await ensureAccountStrictCutoverPreflight(engine, candidate.engineAccountId);
  } catch (error) {
    if (error instanceof AccountStrictCutoverPreflightError) {
      await noteStrictChainFailure(database, candidate.bindingId, error.message);
      return { status: "failed", error: error.message };
    }
    throw error;
  }

  try {
    const staged = await stageAccountStrictCutoverJob(database, { userId: candidate.userId });
    if (staged.status === "already_strict") return { status: "already_strict" };
    return {
      status: "staged",
      jobId: staged.jobId,
      funding: preflight.funding,
      keysStamped: preflight.keysStamped,
    };
  } catch (error) {
    if (error instanceof AccountStrictCutoverError) {
      // not_shadow/no_confirmed_version/unverified mean the delivery moved under the sweep —
      // retried next pass. A permanently impossible target leaves the chain with a loud error.
      if (error.code === "no_binding" || error.code === "not_b2b") {
        await noteStrictChainFailure(database, candidate.bindingId, error.message);
        await clearStrictChainPending(database, candidate.bindingId);
        return { status: "failed", error: error.message };
      }
      if (error.code === "post_cutover") {
        // The head CAS landed between the sweep's read and staging: disarm, the release
        // authority owns this account now.
        await clearStrictChainPending(database, candidate.bindingId);
        return { status: "superseded" };
      }
      return { status: "pending" };
    }
    if (
      error instanceof Error
      && (error.message === "account policy control target is stale"
        || error.message.startsWith("engine_policy_jobs target has an in-flight"))
    ) {
      // A newer policy save is being delivered right now; the next pass stages against it.
      return { status: "pending" };
    }
    throw error;
  }
}

async function clearStrictChainPending(database: Database, bindingId: string): Promise<void> {
  await database.pool.query(
    `
    UPDATE account_policy_bindings SET strict_chain_pending = false, updated_at = now()
    WHERE id = $1 AND strict_chain_pending
  `,
    [bindingId],
  );
}

async function noteStrictChainFailure(
  database: Database,
  bindingId: string,
  error: string,
): Promise<void> {
  await database.pool.query(
    `
    UPDATE account_policy_bindings SET last_error = $2, updated_at = now()
    WHERE id = $1
  `,
    [bindingId, error.slice(0, 2000)],
  );
}
