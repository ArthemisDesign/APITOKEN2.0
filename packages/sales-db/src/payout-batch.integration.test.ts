import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  createPayoutBatch, getActiveBatch, getPayoutCandidates, cancelPayoutBatch,
  listBatchPayouts, listSendablePayouts, listBroadcastPayouts,
  markPayoutBroadcast, markPayoutConfirmed, markPayoutFailed, setBatchStatus,
  releasePayoutRow,
  PayoutBatchInProgressError,
} from "./payout-batch.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;
const W = "0x55d398326f99059fF775485246999027B3197955";

describe.runIf(Boolean(connectionString))("payout batches (on-chain state machine)", () => {
  let db: SalesDatabase;
  beforeAll(async () => { db = createSalesDatabase(connectionString!); await db.pool.query("SELECT 1"); });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, commission_entries, payouts, payout_batches RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, commission_entries, payouts, payout_batches RESTART IDENTITY CASCADE");
  });

  async function partner(code: string, opts: { status?: string; wallet?: string | null } = {}): Promise<string> {
    const details = opts.wallet === null ? null : JSON.stringify({ network: "BSC", address: opts.wallet ?? W });
    const method = opts.wallet === null ? null : "usdt-bep20";
    const r = await db.pool.query<{ id: string }>(
      "INSERT INTO partners (referral_code, status, telegram_username, payout_method, payout_details) VALUES ($1,$2,$1,$3,$4::jsonb) RETURNING id",
      [code, opts.status ?? "active", method, details]);
    return r.rows[0]!.id;
  }
  let ev = 0;
  async function earn(partnerId: string, nano: bigint): Promise<void> {
    ev += 1;
    const src = await db.pool.query<{ id: string }>(
      "INSERT INTO partner_usage_events (commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at) VALUES ($1,$2,$3,$4,now()) RETURNING id",
      [ev, randomUUID(), partnerId, nano.toString()]);
    await db.pool.query("INSERT INTO commission_entries (usage_event_id, partner_id, level, applied_bps, amount_nano) VALUES ($1,$2,0,1000,$3)", [src.rows[0]!.id, partnerId, nano.toString()]);
  }

  it("candidates = active + valid wallet + unpaid > min; excludes others", async () => {
    const a = await partner("aaa"); await earn(a, 100n * USD);
    const noWallet = await partner("bbb", { wallet: null }); await earn(noWallet, 100n * USD);
    const suspended = await partner("ccc", { status: "suspended" }); await earn(suspended, 100n * USD);
    const zero = await partner("ddd"); // no earnings → unpaid 0
    const cands = await getPayoutCandidates(db, 0n);
    expect(cands.map((c) => c.partnerId).sort()).toEqual([a].sort());
    expect(cands[0]!.unpaidNano).toBe(100n * USD);
    void noWallet; void suspended; void zero;
  });

  it("createPayoutBatch inserts rows, blocks a second in-progress batch, and excludes committed balance next time", async () => {
    const a = await partner("aaa"); await earn(a, 60n * USD);
    const cands = await getPayoutCandidates(db, 0n);
    const batch = await createPayoutBatch(db, {
      createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W,
      recipients: cands.map((c) => ({ partnerId: c.partnerId, amountNano: c.unpaidNano, walletAddress: W })),
    });
    expect(batch.recipientCount).toBe(1);
    expect(batch.totalNano).toBe(60n * USD);
    const rows = await listBatchPayouts(db, batch.id);
    expect(rows[0]).toMatchObject({ amountNano: 60n * USD, status: "requested", chainStatus: "pending", walletAddress: W });

    // second prepare must be blocked while one is in progress
    expect(await getActiveBatch(db)).not.toBeNull();
    await expect(createPayoutBatch(db, { createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W, recipients: [{ partnerId: a, amountNano: 1n, walletAddress: W }] }))
      .rejects.toBeInstanceOf(PayoutBatchInProgressError);

    // the requested row now counts as committed → candidate has 0 unpaid left
    const cands2 = await getPayoutCandidates(db, 0n);
    expect(cands2).toHaveLength(0);
  });

  it("chain state transitions: broadcast → confirmed marks paid; failed clears hash for retry", async () => {
    const a = await partner("aaa"); await earn(a, 10n * USD);
    const batch = await createPayoutBatch(db, { createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W, recipients: [{ partnerId: a, amountNano: 10n * USD, walletAddress: W }] });
    const row = (await listBatchPayouts(db, batch.id))[0]!;

    await markPayoutBroadcast(db, row.id, "0xhash1", 7);
    let broad = await listBroadcastPayouts(db);
    expect(broad.map((r) => r.id)).toContain(row.id);
    expect(broad.find((r) => r.id === row.id)!.nonce).toBe(7); // nonce persisted for safe re-broadcast

    await markPayoutConfirmed(db, row.id);
    const confirmed = (await listBatchPayouts(db, batch.id))[0]!;
    expect(confirmed).toMatchObject({ status: "paid", chainStatus: "confirmed", txHash: "0xhash1" });
    expect(await listSendablePayouts(db, batch.id)).toHaveLength(0);
    broad = await listBroadcastPayouts(db);
    expect(broad.map((r) => r.id)).not.toContain(row.id);

    // a failed row drops its hash so it can be retried
    const b = await partner("bbb"); await earn(b, 5n * USD);
    const batch2Rows = [{ partnerId: b, amountNano: 5n * USD, walletAddress: W }];
    await db.pool.query("UPDATE payout_batches SET status='sent' WHERE id=$1", [batch.id]); // free the in-progress lock
    const batch2 = await createPayoutBatch(db, { createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W, recipients: batch2Rows });
    const row2 = (await listBatchPayouts(db, batch2.id))[0]!;
    await markPayoutBroadcast(db, row2.id, "0xhash2", 5);
    await markPayoutFailed(db, row2.id, "reverted");
    const failed = (await listBatchPayouts(db, batch2.id))[0]!;
    expect(failed).toMatchObject({ chainStatus: "failed", txHash: null });
    expect((await listSendablePayouts(db, batch2.id)).map((r) => r.id)).toContain(row2.id); // retriable
  });

  it("cancel removes unsent rows and frees the balance", async () => {
    const a = await partner("aaa"); await earn(a, 20n * USD);
    const batch = await createPayoutBatch(db, { createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W, recipients: [{ partnerId: a, amountNano: 20n * USD, walletAddress: W }] });
    expect(await cancelPayoutBatch(db, batch.id)).toBe(true);
    expect(await listBatchPayouts(db, batch.id)).toHaveLength(0);
    // balance is available again
    expect((await getPayoutCandidates(db, 0n))[0]!.unpaidNano).toBe(20n * USD);
    void setBatchStatus;
  });

  it("release frees a failed row's committed balance (→ rejected); never a broadcast row", async () => {
    const a = await partner("aaa"); await earn(a, 15n * USD);
    const cands = await getPayoutCandidates(db, 0n);
    const batch = await createPayoutBatch(db, { createdBy: "admin", minNano: 0n, gasPriceGwei: "0.05", hotWalletAddress: W, recipients: cands.map((c) => ({ partnerId: c.partnerId, amountNano: c.unpaidNano, walletAddress: W })) });
    const row = (await listBatchPayouts(db, batch.id))[0]!;

    // a broadcast row must NOT be releasable (its tx may have landed)
    await markPayoutBroadcast(db, row.id, "0xabc", 3);
    expect(await releasePayoutRow(db, row.id)).toBe(false);

    // once it's a definitive failure, releasing frees the balance
    await markPayoutFailed(db, row.id, "simulation reverted");
    expect(await getPayoutCandidates(db, 0n)).toHaveLength(0); // still committed as 'requested'
    expect(await releasePayoutRow(db, row.id)).toBe(true);
    expect((await listBatchPayouts(db, batch.id))[0]!.status).toBe("rejected");
    expect((await getPayoutCandidates(db, 0n))[0]!.unpaidNano).toBe(15n * USD); // balance freed
  });
});
