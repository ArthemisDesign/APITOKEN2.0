import {
  AccountStrictCutoverPreflightError,
  ensureAccountStrictCutoverPreflight,
  type AccountStrictCutoverPreflightTransport,
  type EngineClient,
} from "@claude-api/engine-client";
import type { Database } from "./client.js";
import {
  AccountStrictCutoverError,
  stageProvisionedAccountStrictJob,
} from "./pricing-control-jobs.js";

/**
 * The new-account direct strict chain of the release-v2 retirement (docs/commerce/PRICING.md,
 * docs/commerce/MULTI-DISCOUNT.md). Registration provisioning arms `strict_chain_pending` on the
 * fresh binding; the pricing worker then drives the account to its retirement target state, one
 * durable step per pass:
 *
 * 1. once the shadow delivery of the exact materialized policy version confirms
 *    (shadow/verified/confirmed, desired = applied), the shared engine preflight runs (funding
 *    normalization + active-key ACK stamps) and the atomic strict/strict/verified staging is
 *    written for that same version;
 * 2. while the strict delivery is in flight the preflight is re-run on every pass, so a key
 *    created between staging and the engine flip is stamped before the flip requires it;
 * 3. once the strict delivery confirms, the engine `POST /admin/pricing/v2/opt-out` marker is
 *    written and the flag is disarmed. The marker is one-way and replay-safe (`unchanged`),
 *    so worker redelivery and the synchronous opt-out in key issuance can never double-apply.
 *
 * Failure discipline is fail-closed: a precondition that cannot advance is recorded on the
 * binding's last_error and retried on the next pass; the account is never opted out — it keeps
 * working on the release path exactly as before (new accounts have no release coverage, so a
 * stuck chain simply leaves the account in its pre-strict provisioning state, visible in the
 * admin panel and the worker error log). An opt-out rejected with `missing_dependency` is the
 * normal "no active ACKed key yet" waiting state (the guard requires at least one): it stays
 * armed quietly until the first key is issued with its activation ACK.
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
  // The engine opt-out marker landed (applied or exact replay); the chain is complete and the
  // flag was disarmed.
  | { status: "opted_out" }
  // The shadow delivery is still in flight, the strict delivery is still in flight, or the
  // account is waiting for its first ACKed key; the next pass re-evaluates quietly.
  | { status: "pending" }
  // A precondition failed loudly; last_error is recorded and the next pass retries.
  | { status: "failed"; error: string };

export type StrictChainOptOutTransport = Pick<EngineClient, "optOutPricingReleaseV2">;

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

/**
 * Writes the one-way engine opt-out marker for one strict-chain account and disarms the flag.
 * Idempotent: an already-marked account is an `unchanged` replay and also disarms. A guard
 * rejection maps to `awaiting_key` (`missing_dependency` — no active ACKed key yet, a normal
 * waiting state) or a recorded `failed`; transport errors propagate to the caller's retry.
 */
export async function optOutStrictChainAccount(
  database: Database,
  engine: StrictChainOptOutTransport,
  input: { bindingId: string; engineAccountId: string; createdBy: string; reason: string },
): Promise<"opted_out" | "awaiting_key" | "failed"> {
  const ack = await engine.optOutPricingReleaseV2({
    accountId: input.engineAccountId,
    createdBy: input.createdBy,
    reason: input.reason,
  });
  if (ack.result !== "rejected") {
    await clearStrictChainPending(database, input.bindingId);
    return "opted_out";
  }
  if (ack.code === "missing_dependency") return "awaiting_key";
  await noteStrictChainFailure(
    database,
    input.bindingId,
    `pricing release opt-out rejected with ${ack.code}`,
  );
  return "failed";
}

export async function advanceAccountStrictChain(
  database: Database,
  engine: AccountStrictCutoverPreflightTransport & StrictChainOptOutTransport,
  candidate: PendingStrictChainAccount,
): Promise<AccountStrictChainAdvanceResult> {
  const strictConfirmed = candidate.policyEnforcement === "strict"
    && candidate.reconciliationState === "verified"
    && candidate.syncState === "confirmed"
    && candidate.appliedEffectiveVersion !== null
    && candidate.desiredEffectiveVersion === candidate.appliedEffectiveVersion;
  if (strictConfirmed) {
    const result = await optOutStrictChainAccount(database, engine, {
      bindingId: candidate.bindingId,
      engineAccountId: candidate.engineAccountId,
      createdBy: "pricing-worker",
      reason: "new-account direct strict chain",
    });
    if (result === "opted_out") return { status: "opted_out" };
    if (result === "awaiting_key") return { status: "pending" };
    return { status: "failed", error: "pricing release opt-out was rejected by the engine guard" };
  }

  if (candidate.policyEnforcement === "strict") {
    // The strict delivery is staged but not yet confirmed. Keep the engine-side preconditions
    // converged so a key created between staging and the engine flip is stamped before the
    // strict trigger requires it; the delivery itself is owned by the control-job lane.
    try {
      await ensureAccountStrictCutoverPreflight(engine, candidate.engineAccountId);
    } catch (error) {
      if (error instanceof AccountStrictCutoverPreflightError) {
        await noteStrictChainFailure(database, candidate.bindingId, error.message);
        return { status: "failed", error: error.message };
      }
      throw error;
    }
    return { status: "pending" };
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
    const staged = await stageProvisionedAccountStrictJob(database, { userId: candidate.userId });
    if (staged.status === "already_strict") return { status: "pending" };
    return {
      status: "staged",
      jobId: staged.jobId,
      funding: preflight.funding,
      keysStamped: preflight.keysStamped,
    };
  } catch (error) {
    if (error instanceof AccountStrictCutoverError) {
      // no_binding means the intent outlived its binding: record it and disarm. The other codes
      // (not_shadow/no_confirmed_version/unverified) mean the delivery moved under the sweep —
      // retried next pass.
      if (error.code === "no_binding") {
        await noteStrictChainFailure(database, candidate.bindingId, error.message);
        await clearStrictChainPending(database, candidate.bindingId);
        return { status: "failed", error: error.message };
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
