import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { PricingMutationAck } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import { PricingReleaseActivationJobV2Error } from "@claude-api/db";
import {
  pricingControlDisposition,
  pricingReleaseActivationDisposition,
  requirePricingMutation,
} from "./pricing-worker.service.js";

const source = readFileSync(new URL("./pricing-worker.service.ts", import.meta.url), "utf8");

test("delegates pricing money mutations to the canonical database module", () => {
  assert.match(source, /\bcloseElapsedTierWindows\(this\.database, now, syncedUserIds\)/);
  assert.match(source, /\bcompletePricingUsageSync\(this\.database, target\)/);
  assert.match(source, /\bgetPricingProviderBackfillCursor\(/);
  assert.match(source, /\bapplyPricingProviderBackfillPage\(/);
  assert.match(source, /\bcompletePricingProviderBackfill\(/);
  assert.match(source, /await this\.backfillTargetProviders\(target, cursor\)/);
  assert.match(source, /PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4/);
  assert.match(source, /\brefreshTierWindowUsage\(this\.database, syncedUserIds, now\)/);

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
  // The periodic sweep keeps its own flush call as the recovery path.
  assert.match(source, /await this\.flushPricingControlJobs\(\);\s*\n\s*await this\.flushPricingJobs\(\);/);
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

test("retries uncertain activation transport but never retries a typed rejection", () => {
  assert.equal(
    pricingReleaseActivationDisposition(new EngineClientError("timeout", undefined, true)),
    "retry",
  );
  assert.equal(
    pricingReleaseActivationDisposition(new EngineClientError("malformed ACK", 200, false)),
    "dead",
  );
  assert.equal(
    pricingReleaseActivationDisposition(new PricingReleaseActivationJobV2Error("drift", true)),
    "dead",
  );
  assert.equal(
    pricingReleaseActivationDisposition(new PricingReleaseActivationJobV2Error("database", false)),
    "retry",
  );
});
