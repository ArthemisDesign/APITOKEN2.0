import assert from "node:assert/strict";
import test from "node:test";
import type { PricingReleaseInventoryAccountV2, PricingReleaseInventoryPageV2 } from "@claude-api/contracts";
import {
  applyFreshFundingNormalizationPlanV2,
  collectFundingNormalizationInventoryV2,
} from "./funding-normalization-worker.service.js";

function account(accountId: string): PricingReleaseInventoryAccountV2 {
  return {
    account_id: accountId,
    status: "active",
    multiplier_bp: 10_000,
    balance_nano: "0",
    reserved_nano: "0",
    spent_nano: "0",
    funding_generation: null,
    funding_head_version: null,
  };
}

test("exhausts the exact monotonic funding inventory cursor", async () => {
  const calls: Array<{ afterAccountId?: string; limit: number }> = [];
  const pages: PricingReleaseInventoryPageV2[] = [
    { accounts: [account("acct_a"), account("acct_b")], next_after_account_id: "acct_b" },
    { accounts: [account("acct_c")], next_after_account_id: null },
  ];
  let heartbeats = 0;
  const result = await collectFundingNormalizationInventoryV2({
    async getPricingReleaseInventoryV2(options) {
      calls.push(options);
      return pages.shift()!;
    },
  }, 2, async () => { heartbeats += 1; });

  assert.deepEqual(result.map((entry) => entry.account_id), ["acct_a", "acct_b", "acct_c"]);
  assert.deepEqual(calls, [{ limit: 2 }, { afterAccountId: "acct_b", limit: 2 }]);
  assert.equal(heartbeats, 1);
});

test("rejects duplicate accounts and a continuation cursor not equal to the last row", async () => {
  await assert.rejects(
    collectFundingNormalizationInventoryV2({
      async getPricingReleaseInventoryV2() {
        return {
          accounts: [account("acct_a"), account("acct_a")],
          next_after_account_id: null,
        };
      },
    }, 500),
    /regressed or duplicated/,
  );

  await assert.rejects(
    collectFundingNormalizationInventoryV2({
      async getPricingReleaseInventoryV2() {
        return {
          accounts: [account("acct_a")],
          next_after_account_id: "acct_z",
        };
      },
    }, 500),
    /non-monotonic continuation cursor/,
  );
});

test("posts only source and target digests from the immediately preceding fresh plan", async () => {
  const sourceDigest = `sha256:v2:${"1".repeat(64)}`;
  const targetDigest = `sha256:v2:${"2".repeat(64)}`;
  const plan = {
    account_id: "acct_fresh",
    account_status: "active" as const,
    status: "ready" as const,
    source: "ledger_replay" as const,
    source_state_digest: sourceDigest,
    normalization_digest: targetDigest,
    funding_generation: 2,
    funding_head_version: 1,
    balance_nano: "0",
    reserved_nano: "0",
    spent_nano: "0",
    lots: [],
    blockers: [],
  };
  let posted: unknown;
  const result = await applyFreshFundingNormalizationPlanV2({
    async getFundingNormalizationPlanV2() {
      return plan;
    },
    async applyFundingNormalizationV2(accountId, input) {
      posted = { accountId, input };
      return { status: "unchanged", normalization: { ...plan, status: "normalized", source: "stored_generation" } };
    },
  }, "acct_fresh");

  assert.deepEqual(posted, {
    accountId: "acct_fresh",
    input: {
      expected_source_state_digest: sourceDigest,
      expected_normalization_digest: targetDigest,
    },
  });
  assert.equal(result.kind, "applied");
});
