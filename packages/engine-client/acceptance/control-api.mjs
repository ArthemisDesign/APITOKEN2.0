import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { EngineClient, EngineClientError } from "@claude-api/engine-client";
import { ZodError } from "zod";

const baseUrl = process.env.CONTROL_API_ACCEPTANCE_BASE_URL;
const controlKey = process.env.CONTROL_API_ACCEPTANCE_CONTROL_KEY;
const serverLog = process.env.CONTROL_API_ACCEPTANCE_SERVER_LOG;
assert.ok(baseUrl, "CONTROL_API_ACCEPTANCE_BASE_URL is required");
assert.ok(controlKey, "CONTROL_API_ACCEPTANCE_CONTROL_KEY is required");
assert.ok(serverLog, "CONTROL_API_ACCEPTANCE_SERVER_LOG is required");

const client = new EngineClient({ baseUrl, controlKey, timeoutMs: 5_000 });
assert.equal(await client.health(), true, "built EngineClient must reach real /health");
assert.equal(await client.readiness(), true, "built EngineClient must reach real /ready");

const runId = `${process.pid}-${Date.now()}`;
const handle = `acceptance-${runId}@example.invalid`;
const created = await client.createAccount({ handle, multBp: 2500 });
assert.match(created.account, /^acct_/u);
assert.equal(created.multBp, 2500);
assert.equal(created.handle, handle);
const replayed = await client.createAccount({ handle, multBp: 2500 });
assert.equal(replayed.account, created.account, "stable handle retry must not orphan an account");

let account = await client.getAccount(created.account);
assert.deepEqual(
  {
    balance: account.balance_nano,
    spent: account.spent_nano,
    reserved: account.reserved_nano,
    status: account.status,
    multiplier: account.mult_bp,
    handle: account.handle,
  },
  { balance: "0", spent: "0", reserved: "0", status: "active", multiplier: 2500, handle },
);

const amount = 9_007_199_254_740_993n;
const reference = `acceptance:${runId}`;
const credited = await client.creditAccount(created.account, amount, reference);
assert.equal(credited.balance_nano, amount.toString());
const creditedReplay = await client.creditAccount(created.account, amount, reference);
assert.equal(creditedReplay.balance_nano, amount.toString(), "top-up replay must remain idempotent");

const initialLimit = 9_007_199_254_700_000n;
const issued = await client.issueKey(created.account, {
  label: "assembled acceptance",
  spendLimitNano: initialLimit,
});
assert.match(issued.key, /^sk-pool-/u);
assert.match(issued.key_id, /^key_/u);
assert.equal(issued.account, created.account);
assert.equal(issued.spend_limit_nano, initialLimit.toString());

let keys = await client.listKeys(created.account);
assert.equal(keys.length, 1);
assert.equal(keys[0].key_id, issued.key_id);
assert.equal(keys[0].label, "assembled acceptance");
assert.equal(keys[0].status, "active");
assert.equal(keys[0].spend_limit_nano, initialLimit.toString());
assert.ok(!keys[0].key_masked.includes(issued.key), "key listing must never expose the issued secret");

const replacementLimit = 9_007_199_254_600_000n;
await client.replaceKeyPolicy(created.account, issued.key_id, {
  spendLimitNano: replacementLimit,
  expiresAt: null,
});
keys = await client.listKeys(created.account);
assert.equal(keys[0].spend_limit_nano, replacementLimit.toString());

await client.setAccountMultiplier(created.account, 5000);
await client.setAccountProviderDiscount(created.account, "openai", 2000);
let discounts = await client.getAccountDiscounts(created.account);
assert.equal(discounts.multiplierBp, 5000);
assert.equal(discounts.providers.openai, 2000);
await client.setAccountProviderDiscount(created.account, "openai", null);
discounts = await client.getAccountDiscounts(created.account);
assert.equal(discounts.providers.openai, undefined);

const recent = await client.getLedger(created.account, 50);
assert.equal(recent.length, 1);
assert.equal(recent[0].kind, "topup");
assert.equal(recent[0].amount_nano, amount.toString());
assert.equal(recent[0].ref, reference);
assert.equal(recent[0].uncollected_nano, "0");
const cursor = await client.getLedgerAfter(created.account, 0n, 50);
assert.deepEqual(cursor.map((row) => row.id), [...cursor.map((row) => row.id)].sort((a, b) => Number(BigInt(a) - BigInt(b))));
assert.equal(cursor[0].amount_nano, amount.toString());
await client.acknowledgeLedger(created.account, BigInt(cursor.at(-1).id));

const usage = await client.getUsage(created.account, "30d");
assert.equal(usage.account, created.account);
assert.equal(usage.requests, 0);
assert.equal(usage.total_official_nano, "0");
assert.equal(usage.total_charged_nano, "0");
assert.deepEqual(usage.models, []);

const badAuth = new EngineClient({ baseUrl, controlKey: "wrong-control-key-with-at-least-24-chars", timeoutMs: 5_000 });
await assert.rejects(
  badAuth.getAccount(created.account),
  (error) => error instanceof EngineClientError && error.status === 401 && error.retryable === false,
  "real Control API middleware must reject the wrong key as terminal 401",
);

const corruptingFetch = async (input, init) => {
  const response = await fetch(input, init);
  if (new URL(String(input)).pathname === `/admin/account/${created.account}` && response.ok) {
    const payload = await response.json();
    delete payload.balance_nano;
    return new Response(JSON.stringify(payload), {
      status: response.status,
      statusText: response.statusText,
      headers: { "content-type": "application/json" },
    });
  }
  return response;
};
const schemaGuard = new EngineClient({ baseUrl, controlKey, timeoutMs: 5_000, fetch: corruptingFetch });
await assert.rejects(
  schemaGuard.getAccount(created.account),
  (error) => error instanceof ZodError,
  "built EngineClient must fail closed when the producer omits a required field",
);

await client.disableKey(issued.key_id);
keys = await client.listKeys(created.account);
assert.equal(keys[0].status, "disabled");
await client.setAccountStatus(created.account, "disabled");
account = await client.getAccount(created.account);
assert.equal(account.status, "disabled");
assert.equal(account.balance_nano, amount.toString());

const logText = await readFile(serverLog, "utf8");
assert.ok(!logText.includes(issued.key), "server log must not contain the issued key");
assert.ok(!logText.includes(controlKey), "server log must not contain the Control key");
console.log("control-api acceptance passed: built binary, package exports, HTTP middleware, PostgreSQL, bigint, schema guard");
