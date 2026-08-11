import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("./adjustment-worker.service.ts", import.meta.url), "utf8");
const moduleSource = readFileSync(new URL("./app.module.ts", import.meta.url), "utf8");

test("registers the durable refund compensation worker", () => {
  assert.match(moduleSource, /AdjustmentWorkerService/);
  assert.match(source, /claimNextAdjustment\(this\.database, this\.workerId\)/);
  assert.match(source, /this\.engine\.debitAccount\(/);
  assert.match(source, /confirmAdjustment\(/);
  assert.match(source, /retryAdjustment\(/);
  assert.match(source, /recoverStaleAdjustments\(/);
});

test("keeps refund money state inside the database module", () => {
  assert.doesNotMatch(source, /UPDATE engine_adjustments/);
  assert.doesNotMatch(source, /INSERT INTO engine_adjustments/);
  assert.doesNotMatch(source, /UPDATE payments/);
});
