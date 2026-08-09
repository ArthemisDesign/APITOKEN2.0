import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("./pricing-worker.service.ts", import.meta.url), "utf8");

test("delegates pricing money mutations to the canonical database module", () => {
  assert.match(source, /\bcompletePricingUsageSync\(this\.database, target\)/);
  assert.match(source, /\bgetPricingProviderBackfillCursor\(/);
  assert.match(source, /\bapplyPricingProviderBackfillPage\(/);
  assert.match(source, /\bcompletePricingProviderBackfill\(/);
  assert.match(source, /await this\.backfillTargetProviders\(target, cursor\)/);
  assert.match(source, /PROVIDER_BACKFILL_MAX_PAGES_PER_SYNC = 4/);

  // The worker owns delivery, never the money itself: commerce state is written by the database
  // module inside one transaction with the job row.
  assert.doesNotMatch(source, /UPDATE customer_profiles/);
  assert.doesNotMatch(source, /INSERT INTO engine_pricing_jobs/);
  assert.doesNotMatch(source, /INSERT INTO webhook_events/);
});

test("delivers a job to its own target: the account default or one provider override", () => {
  // One job, one target. A provider job with a null multiplier removes the override and returns
  // that provider to the account default — the engine resolves `override ?? default` per request.
  assert.match(source, /if \(job\.providerId === null\) \{/);
  assert.match(source, /await this\.engine\.setAccountMultiplier\(job\.engineAccountId, job\.multiplierBp\)/);
  assert.match(source, /await this\.engine\.setAccountProviderDiscount\(/);
  assert.match(source, /await confirmPricingJob\(this\.database, job\)/);
  assert.match(source, /await retryPricingJob\(this\.database, job, message\(error\)\)/);
});

test("carries no pricing policy, catalog, switch or strict-chain machinery", () => {
  // The delivery lane is a multiplier and a provider id. Anything versioned, staged or enforced
  // is what made a funded account unable to spend its own balance on 2026-08-09.
  for (const retired of [
    /PricingControlJob/,
    /preparePricingCatalog/,
    /activateProviderSwitches/,
    /activateAccountPolicy/,
    /strictChain/i,
    /restampActiveKeys/,
    /optOutPricingReleaseV2/,
  ]) {
    assert.doesNotMatch(source, retired);
  }
});
