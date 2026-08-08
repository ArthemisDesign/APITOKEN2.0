import "server-only";
import type {
  AccountPolicyBinding,
  AccountPolicySpec,
  PolicyActiveExpectation,
} from "@claude-api/contracts";
import {
  AccountStrictCutoverPreflightError,
  buildOfficialOpenKeysPolicy,
  canonicalPricingJson,
  ensureAccountStrictCutoverPreflight,
  officialOpenKeysStrictBinding,
  type EngineClient,
  type OpenKeysPricingAuthority,
} from "@claude-api/engine-client";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";
import { resolveOpenKeysPricingAuthority } from "./openkeys-pricing";

/**
 * Release-v2 retirement (phase 2.2): the bounded, admin-triggered backfill sweep that moves
 * PRE-EXISTING OpenKeys engine accounts onto the same direct strict path new issuance is
 * born on (docs/product/OPENKEYS.md). New accounts never touch the release path; the
 * pre-existing warehouse keeps its release coverage until this sweep opts each account out.
 *
 * Per account the sweep mirrors the issuance order, adapted to an account that already has
 * a live (release-covered) policy binding and keys: class/status exclusion (service and
 * meter-only accounts are never OpenKeys-owned, and the engine funding snapshot must prove
 * `openkeys` before anything mutates) → shared strict-cutover preflight (funding
 * normalization + exact ACK stamps on the CURRENT active policy) → deterministic official
 * 1:1 strict policy prepare → exact-CAS activation over the observed active policy → key
 * re-stamp on the new head (request auth on a strict account admits only the current ACK;
 * the pre-stamp BEFORE the flip is what the migration-0016 cutover trigger requires) → the
 * one-way opt-out marker. Every step is idempotent: prepare/activation replay as
 * `unchanged`, the already-official strict account skips straight to the opt-out, and the
 * opt-out replay returns the stored marker, so re-running the sweep is always safe.
 *
 * In-flight release reservations simply fail the account's activation engine-side (the
 * migration-0016 drain trigger answers 503): the account is recorded as failed for this
 * call and a later admin call retries it — calm, bounded, never a hot loop. One account's
 * failure never blocks the others.
 */

export type OpenKeysStrictBackfillEngine = Pick<
  EngineClient,
  | "activateAccountPolicy"
  | "applyFundingNormalizationV2"
  | "getAccount"
  | "getAccountPricingState"
  | "getActiveAccountPolicy"
  | "getFundingNormalizationPlanV2"
  | "listKeys"
  | "optOutPricingReleaseV2"
  | "prepareAccountPolicy"
  | "setKeyStatus"
>;

export interface OpenKeysStrictBackfillAccountResult {
  accountId: string;
  outcome: "opted_out" | "skipped" | "failed";
  detail?: string;
}

export interface OpenKeysStrictBackfillSummary {
  candidates: number;
  results: OpenKeysStrictBackfillAccountResult[];
  counts: { optedOut: number; skipped: number; failed: number };
}

function mutationRejected(ack: { result: string }, phase: string): string | null {
  if (ack.result === "rejected") return `${phase} rejected`;
  return null;
}

function policyMatches(
  observed: { policy: AccountPolicySpec; binding: AccountPolicyBinding } | null,
  policy: AccountPolicySpec,
  binding: AccountPolicyBinding,
): boolean {
  return observed !== null
    && canonicalPricingJson(observed.policy) === canonicalPricingJson(policy)
    && canonicalPricingJson(observed.binding) === canonicalPricingJson(binding);
}

async function restampActiveKeys(
  engine: OpenKeysStrictBackfillEngine,
  accountId: string,
  ack: { effectivePolicyVersion: number; policyDigest: string },
): Promise<void> {
  const keys = await engine.listKeys(accountId);
  for (const key of keys) {
    if (key.status !== "active") continue;
    await engine.setKeyStatus(key.key_id, "active", ack);
  }
}

/** Advances one OpenKeys-owned engine account to the strict 1:1 path + opt-out. Never throws. */
export async function backfillOpenKeysAccount(
  engine: OpenKeysStrictBackfillEngine,
  authority: OpenKeysPricingAuthority,
  accountId: string,
): Promise<OpenKeysStrictBackfillAccountResult> {
  const fail = (detail: string): OpenKeysStrictBackfillAccountResult =>
    ({ accountId, outcome: "failed", detail });
  const skip = (detail: string): OpenKeysStrictBackfillAccountResult =>
    ({ accountId, outcome: "skipped", detail });
  try {
    const account = await engine.getAccount(accountId);
    if (account.status !== "active") return skip("engine account is disabled");
    if (account.funding?.account_class !== "openkeys") {
      // Service/meter-only accounts stay on the release path by design (no engine meter-only
      // lane outside release-v2 yet); anything that is not provably OpenKeys-owned is left
      // untouched.
      return skip("engine account is not openkeys-class (service/meter-only or unknown)");
    }
    const state = await engine.getAccountPricingState(accountId);
    if (typeof state !== "object" || state === null || !("active" in state)) {
      return skip("account has no active policy binding to cut over");
    }

    const policy = buildOfficialOpenKeysPolicy(accountId, authority);
    const binding = officialOpenKeysStrictBinding();
    const observed = await engine.getActiveAccountPolicy(accountId);
    if (!policyMatches(observed, policy, binding)) {
      await ensureAccountStrictCutoverPreflight(engine, accountId);
      const prepared = await engine.prepareAccountPolicy(policy);
      const prepareRejected = mutationRejected(prepared, "prepare");
      if (prepareRejected !== null) return fail(prepareRejected);
      // Pre-stamp every active key with the NEW strict ACK: the migration-0016 cutover
      // trigger admits the flip only when all active keys already carry it. While the
      // account is still shadow/release-covered the stamp is informational, so the
      // in-flight traffic is unaffected — exactly the commerce delivery worker's order.
      await restampActiveKeys(engine, accountId, {
        effectivePolicyVersion: policy.effective_version,
        policyDigest: policy.content_digest,
      });
      const expectation: PolicyActiveExpectation = {
        exact: {
          target: {
            version: state.active.policy.effective_version,
            content_digest: state.active.policy.content_digest,
          },
          binding: state.active.binding,
        },
      };
      const activated = await engine.activateAccountPolicy(policy, binding, expectation);
      const activationRejected = mutationRejected(activated, "activation");
      if (activationRejected !== null) return fail(activationRejected);
      const readback = await engine.getActiveAccountPolicy(accountId);
      if (!policyMatches(readback, policy, binding)) {
        return fail("strict policy activation did not read back with the exact requested identity");
      }
      // Converge any key created in the race between the pre-stamp and the flip.
      await restampActiveKeys(engine, accountId, {
        effectivePolicyVersion: policy.effective_version,
        policyDigest: policy.content_digest,
      });
    }

    const optOut = await engine.optOutPricingReleaseV2({
      accountId,
      createdBy: "openkeys",
      reason: "OpenKeys existing-account strict backfill (release-v2 retirement)",
    });
    if (optOut.result === "rejected") {
      return fail(`pricing release opt-out rejected with ${optOut.code}`);
    }
    return { accountId, outcome: "opted_out" };
  } catch (error) {
    if (error instanceof AccountStrictCutoverPreflightError) {
      return fail(error.message);
    }
    return fail(error instanceof Error ? error.message : "openkeys strict backfill failed");
  }
}

/**
 * The candidate set is the local warehouse: every OpenKeys-owned engine account has exactly
 * one `openkeys_keys` row, which by construction excludes service/meter-only accounts.
 * `accountIds` restricts the page to the explicit canary list.
 */
export async function listOpenKeysStrictBackfillCandidates(options: {
  limit: number;
  accountIds?: readonly string[];
}): Promise<string[]> {
  const result = await getDatabase().pool.query<{ engine_account_id: string }>(
    `
    SELECT engine_account_id
    FROM openkeys_keys
    WHERE status = 'active' AND removed_at IS NULL
      AND ($2::text[] IS NULL OR engine_account_id = ANY($2))
    ORDER BY engine_account_id
    LIMIT $1
  `,
    [options.limit, options.accountIds ?? null],
  );
  return result.rows.map((row) => row.engine_account_id);
}

/** One bounded sweep pass over the local warehouse candidates. */
export async function runOpenKeysStrictBackfill(options: {
  limit: number;
  accountIds?: readonly string[];
}): Promise<OpenKeysStrictBackfillSummary> {
  const engine = getEngineClient();
  const authority = await resolveOpenKeysPricingAuthority(engine);
  const candidates = await listOpenKeysStrictBackfillCandidates(options);
  const results: OpenKeysStrictBackfillAccountResult[] = [];
  for (const accountId of candidates) {
    results.push(await backfillOpenKeysAccount(engine, authority, accountId));
  }
  return {
    candidates: candidates.length,
    results,
    counts: {
      optedOut: results.filter((result) => result.outcome === "opted_out").length,
      skipped: results.filter((result) => result.outcome === "skipped").length,
      failed: results.filter((result) => result.outcome === "failed").length,
    },
  };
}
