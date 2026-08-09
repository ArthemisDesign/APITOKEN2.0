import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { PricingMutationAck } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import {
  pricingControlDisposition,
  requirePricingMutation,
  strictActivationNeedsPreRestamp,
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
  // for a missed notification.
  assert.match(source, /this\.requestControlFlush\(\);\s*\n\s*await this\.flushPricingJobs\(\);/);
  assert.match(source, /const dispatchMs = this\.config\.get\("PRICING_DISPATCH_MS", \{ infer: true \}\)/);
  assert.match(source, /if \(Date\.now\(\) < nextSweepAt\)/);
});

test("re-stamps active keys with the exact new ACK after every strict policy activation", () => {
  // Strict request auth admits only keys stamped with the current policy head, so the delivery
  // worker must re-stamp before the job confirms — otherwise each strict activation (the
  // cutover and every later policy save) would break the account's keys. The PRE-activation
  // re-stamp runs only when the strict target is the currently active version (the engine
  // accepts a key-ACK write only when it matches the active policy): the shadow→strict flip.
  // On a strict→strict advance to a new version the post-activation re-stamp converges keys.
  assert.match(source, /strictActivationNeedsPreRestamp\(state, job\.spec, job\.binding\)/);
  assert.match(source, /restampActiveKeysForStrictPolicy\(job\.spec\.account_id, \{/);
  assert.match(source, /effectivePolicyVersion: job\.spec\.effective_version/);
  assert.match(source, /policyDigest: job\.spec\.content_digest/);
  assert.match(source, /if \(key\.status !== "active"\) continue;/);
  assert.match(source, /await this\.engine\.setKeyStatus\(key\.key_id, "active", ack\)/);
});

test("pre-activation re-stamp timing follows the currently active version", () => {
  const strictBinding = {
    policy_enforcement: "strict",
    funding_enforcement: "strict",
    reconciliation_state: "verified",
  } as const;
  const shadowBinding = {
    policy_enforcement: "shadow",
    funding_enforcement: "legacy_single",
    reconciliation_state: "verified",
  } as const;
  const spec = { effective_version: 5, content_digest: "digest-v5" };
  const activeAt = (version: number, digest: string) => ({
    active: {
      policy: { effective_version: version, content_digest: digest },
      binding: shadowBinding,
    },
  });

  // Shadow→strict cutover: the target IS the shadow-confirmed active version — the engine
  // accepts the pre-stamp and the 0016 trigger requires it.
  assert.equal(
    strictActivationNeedsPreRestamp(activeAt(5, "digest-v5") as never, spec, strictBinding),
    true,
  );
  // Strict→strict advance to a NOT-yet-active version: the pre-stamp would be rejected with
  // the 409 ACK conflict; the post-activation re-stamp converges the keys instead.
  assert.equal(
    strictActivationNeedsPreRestamp(activeAt(4, "digest-v4") as never, spec, strictBinding),
    false,
  );
  // Same version but a different digest is not the active policy either.
  assert.equal(
    strictActivationNeedsPreRestamp(activeAt(5, "digest-other") as never, spec, strictBinding),
    false,
  );
  // Non-strict deliveries never re-stamp before activation.
  assert.equal(
    strictActivationNeedsPreRestamp(activeAt(5, "digest-v5") as never, spec, shadowBinding),
    false,
  );
  assert.equal(strictActivationNeedsPreRestamp("unbound" as never, spec, strictBinding), false);
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
  // The key-ACK conflict (a policy advance landed between the state read and the stamp) is a
  // transient ordering artifact by construction — retry, never dead.
  assert.equal(
    pricingControlDisposition(
      new EngineClientError("activation_policy_ack does not match the active policy", 409, false),
    ),
    "retry",
  );
  // Other non-retryable 409s stay terminal.
  assert.equal(pricingControlDisposition(new EngineClientError("key already exists", 409, false)), "dead");
  assert.equal(pricingControlDisposition(new Error("database unavailable")), "retry");
});
