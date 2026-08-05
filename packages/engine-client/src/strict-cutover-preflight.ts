import type { EngineClient } from "./index.js";

/**
 * Shared engine-side preflight of the per-account shadow→strict cutover lane
 * (docs/commerce/PRICING.md "Per-account strict cutover"). Both triggers of the lane — the
 * manual admin endpoint and the automatic chain behind a B2C→B2B conversion / b2b_client
 * policy save — must arrange the exact same preconditions before the durable strict control
 * job is staged, because the engine enforces them atomically at the flip:
 *
 * 1. funding buckets are normalized to equal the account aggregates (the strict trigger
 *    requires bucket/aggregate parity);
 * 2. every active API key is stamped with the exact active-policy ACK (the cutover trigger
 *    rejects the flip over an unstamped key).
 *
 * The function is idempotent: a replay after a partial failure re-reads the current plan and
 * re-stamps the same ACK, and an already-normalized/already-strict state is reported, not an
 * error. A typed AccountStrictCutoverPreflightError marks the flow loudly instead of producing
 * a partial state; the caller maps it to an HTTP 409 (admin endpoint) or to the binding's
 * last_error + retry (worker chain).
 */
export type AccountStrictCutoverPreflightTransport = Pick<
  EngineClient,
  | "applyFundingNormalizationV2"
  | "getAccountPricingState"
  | "getFundingNormalizationPlanV2"
  | "listKeys"
  | "setKeyStatus"
>;

export type AccountStrictCutoverPreflight = {
  funding: "normalized" | "already_normalized" | "nothing_to_normalize";
  keysStamped: number;
};

export class AccountStrictCutoverPreflightError extends Error {
  constructor(
    readonly code:
      | "funding_blocked"
      | "funding_target_missing"
      | "funding_vanished"
      | "no_active_policy",
    message: string,
  ) {
    super(message);
    this.name = "AccountStrictCutoverPreflightError";
  }
}

export async function ensureAccountStrictCutoverPreflight(
  engine: AccountStrictCutoverPreflightTransport,
  accountId: string,
): Promise<AccountStrictCutoverPreflight> {
  const plan = await engine.getFundingNormalizationPlanV2(accountId);
  let funding: AccountStrictCutoverPreflight["funding"];
  if (plan === null) {
    funding = "nothing_to_normalize";
  } else if (plan.status === "normalized") {
    funding = "already_normalized";
  } else if (plan.status === "blocked") {
    const blockers = plan.blockers.map((blocker) => `${blocker.code}: ${blocker.detail}`).join("; ");
    throw new AccountStrictCutoverPreflightError(
      "funding_blocked",
      `funding normalization is blocked: ${blockers}`,
    );
  } else {
    if (plan.normalization_digest === null) {
      throw new AccountStrictCutoverPreflightError(
        "funding_target_missing",
        "engine account has no funding normalization target",
      );
    }
    const applied = await engine.applyFundingNormalizationV2(accountId, {
      expected_source_state_digest: plan.source_state_digest,
      expected_normalization_digest: plan.normalization_digest,
    });
    if (applied === null) {
      throw new AccountStrictCutoverPreflightError(
        "funding_vanished",
        "funding normalization target vanished",
      );
    }
    funding = "normalized";
  }

  const state = await engine.getAccountPricingState(accountId);
  if (typeof state !== "object" || state === null || !("active" in state)) {
    throw new AccountStrictCutoverPreflightError(
      "no_active_policy",
      "account has no active engine policy to cut over",
    );
  }
  let keysStamped = 0;
  // Request auth on an already-strict account admits only keys stamped by the strict delivery
  // re-stamp; stamping them again here would pin them to the outgoing head.
  if (state.active.binding.policy_enforcement !== "strict") {
    const ack = {
      effectivePolicyVersion: state.active.policy.effective_version,
      policyDigest: state.active.policy.content_digest,
    };
    const keys = await engine.listKeys(accountId);
    for (const key of keys) {
      if (key.status !== "active") continue;
      await engine.setKeyStatus(key.key_id, "active", ack);
      keysStamped += 1;
    }
  }
  return { funding, keysStamped };
}
