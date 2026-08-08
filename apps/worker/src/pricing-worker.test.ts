import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { PricingMutationAck } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import {
  pricingControlDisposition,
  requirePricingMutation,
} from "./pricing-worker.service.js";

const source = readFileSync(new URL("./pricing-worker.service.ts", import.meta.url), "utf8");

test("delegates pricing money mutations to the canonical database module", () => {
  assert.match(source, /\bcompletePricingUsageSync\(this\.database, target\)/);
  assert.match(source, /\bgetPricingProviderBackfillCursor\(/);
  assert.match(source, /\bapplyPricingProviderBackfillPage\(/);
  assert.match(source, /\bcompletePricingProviderBackfill\(/);
  assert.match(source, /await this\.backfillTargetProviders\(target, cursor\)/);
  assert.match(source, /PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4/);

  // The progressive tier machinery is retired: no tier reconciliation, window refresh, or
  // month-close jobs may run anymore (docs/commerce/MULTI-DISCOUNT.md section 6).
  assert.doesNotMatch(source, /\bcloseElapsedTierWindows\b/);
  assert.doesNotMatch(source, /\brefreshTierWindowUsage\b/);
  assert.doesNotMatch(source, /\breconcileTierLadderMultipliers\b/);
  assert.doesNotMatch(source, /\bB2C_TIER_RULES\b/);
  assert.doesNotMatch(source, /\breverseRefundedTopups\b/);
  assert.doesNotMatch(source, /UPDATE customer_profiles/);
  assert.doesNotMatch(source, /INSERT INTO engine_pricing_jobs/);
  assert.doesNotMatch(source, /INSERT INTO webhook_events/);
  assert.doesNotMatch(source, /stagePricingReleaseActivationJobV2/);
});

test("wakes control-job delivery from LISTEN/NOTIFY with the sweep as recovery", () => {
  assert.match(source, /new PricingControlNotifyListener\(/);
  assert.match(source, /onWake: \(\) => this\.requestControlFlush\(\)/);
  assert.match(source, /await this\.controlNotify\?\.stop\(\)/);
  assert.match(source, /private controlFlushRunning = false/);
  assert.match(source, /private controlFlushQueued = false/);
  // A wake during an active flush schedules exactly one follow-up pass.
  assert.match(source, /this\.controlFlushQueued = true/);
  assert.match(source, /while \(this\.controlFlushQueued && !this\.stopped\)/);
  // Delivery runs first and on its own short tick (PRICING_DISPATCH_MS), never behind the
  // sweep: control jobs through the coalescing dispatcher so a tick cannot double a
  // NOTIFY-triggered pass, the multiplier queue directly. The tick is the bounded recovery path
  // for a missed notification; the slow sweep keeps only the strict-chain flush.
  assert.match(source, /this\.requestControlFlush\(\);\s*\n\s*await this\.flushPricingJobs\(\);/);
  assert.match(source, /const dispatchMs = this\.config\.get\("PRICING_DISPATCH_MS", \{ infer: true \}\)/);
  assert.match(source, /if \(Date\.now\(\) < nextSweepAt\)/);
  assert.match(source, /await this\.flushPendingStrictChains\(\);/);
});

test("advances flagged strict chains once the shadow delivery confirms", () => {
  // A conversion/policy save flags the binding strict_chain_pending; the sweep runs the shared
  // preflight + durable staging from the canonical database module and logs failures loudly.
  assert.match(source, /STRICT_CHAIN_MAX_ACCOUNTS_PER_SWEEP = 25/);
  assert.match(source, /listPendingStrictChainAccounts\(\s*this\.database,\s*STRICT_CHAIN_MAX_ACCOUNTS_PER_SWEEP,?\s*\)/);
  assert.match(source, /await advanceAccountStrictChain\(this\.database, this\.engine, candidate\)/);
  assert.match(source, /strict chain for \$\{candidate\.userId\} cannot advance/);
  assert.doesNotMatch(source, /UPDATE account_policy_bindings/);
});

test("runs the existing-account backfill arm lane on the slow sweep with canary knobs", () => {
  // Phase 2.2 of the release-v2 retirement: the slow sweep arms a bounded page of eligible
  // accounts into the SAME direct strict chain (never a fork) through the canonical database
  // module; the enable flag, batch size and allowlist are env-driven for the canary sequence
  // (docs/ops/PRICING_RELEASE_BACKFILL.md), and per-account armed/failed lines are logged.
  assert.match(source, /await this\.flushPricingBackfill\(\);/);
  assert.match(source, /if \(!this\.config\.get\("PRICING_BACKFILL_ENABLED", \{ infer: true \}\)\) return;/);
  assert.match(source, /this\.config\.get\("PRICING_BACKFILL_BATCH_SIZE", \{ infer: true \}\)/);
  assert.match(source, /this\.config\.get\("PRICING_BACKFILL_ACCOUNT_ALLOWLIST", \{ infer: true \}\)/);
  assert.match(source, /runPricingBackfillSweep\(this\.database, this\.engine, \{/);
  assert.match(source, /pricing backfill armed the direct strict chain for \$\{accountId\}/);
  assert.match(source, /pricing backfill for \$\{failure\.engineAccountId\} cannot advance/);
});

test("re-stamps active keys with the exact new ACK after every strict policy activation", () => {
  // Strict request auth admits only keys stamped with the current policy head, so the delivery
  // worker must re-stamp before the job confirms — otherwise each strict activation (the
  // cutover and every later policy save) would break the account's keys.
  assert.match(source, /job\.binding\.policy_enforcement === "strict"/);
  assert.match(source, /restampActiveKeysForStrictPolicy\(job\.spec\.account_id, \{/);
  assert.match(source, /effectivePolicyVersion: job\.spec\.effective_version/);
  assert.match(source, /policyDigest: job\.spec\.content_digest/);
  assert.match(source, /if \(key\.status !== "active"\) continue;/);
  assert.match(source, /await this\.engine\.setKeyStatus\(key\.key_id, "active", ack\)/);
});

test("classifies typed pricing rejections without retrying permanent failures", () => {
  const cases: Array<{ ack: PricingMutationAck; expected: "retry" | "superseded" | "dead" }> = [
    {
      ack: {
        result: "rejected",
        code: "missing_dependency",
        identity: {},
        rejection: { missing_dependency: { dependency: "catalog:main:1" } },
      },
      expected: "retry",
    },
    {
      ack: {
        result: "rejected",
        code: "stale",
        identity: {},
        rejection: { stale: { actual: { version: 2, content_digest: "newer" } } },
      },
      expected: "superseded",
    },
    {
      ack: {
        result: "rejected",
        code: "version_conflict",
        identity: {},
        rejection: "version_conflict",
      },
      expected: "dead",
    },
    {
      ack: {
        result: "rejected",
        code: "locked",
        identity: {},
        rejection: "locked",
      },
      expected: "dead",
    },
  ];

  for (const { ack, expected } of cases) {
    let failure: unknown;
    try {
      requirePricingMutation(ack, ["applied"], "test mutation");
    } catch (error) {
      failure = error;
    }
    assert.equal(pricingControlDisposition(failure), expected);
  }

  assert.equal(pricingControlDisposition(new EngineClientError("bad response", 200, false)), "dead");
  assert.equal(pricingControlDisposition(new EngineClientError("timeout", undefined, true)), "retry");
  assert.equal(pricingControlDisposition(new Error("database unavailable")), "retry");
});
