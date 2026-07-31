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
  assert.match(source, /\bcloseElapsedTierWindows\(this\.database, now, syncedUserIds\)/);
  assert.match(source, /\bcompletePricingUsageSync\(this\.database, target\)/);
  assert.match(source, /\brefreshTierWindowUsage\(this\.database, syncedUserIds, now\)/);

  assert.doesNotMatch(source, /\bB2C_TIER_RULES\b/);
  assert.doesNotMatch(source, /\breverseRefundedTopups\b/);
  assert.doesNotMatch(source, /UPDATE customer_profiles/);
  assert.doesNotMatch(source, /INSERT INTO engine_pricing_jobs/);
  assert.doesNotMatch(source, /INSERT INTO webhook_events/);
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
