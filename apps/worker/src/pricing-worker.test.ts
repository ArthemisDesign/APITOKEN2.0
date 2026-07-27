import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

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
