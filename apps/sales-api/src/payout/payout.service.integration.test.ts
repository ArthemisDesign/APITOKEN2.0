import { randomUUID } from "node:crypto";
import type { ConfigService } from "@nestjs/config";
import {
  PayoutBatchInProgressError,
  createPayoutBatch,
  createSalesDatabase,
  getPayoutBatch,
  listBatchPayouts,
  releasePayoutRow,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Environment } from "../config.js";
import type { ConfirmResult, PayoutChain, SignedTransfer } from "./chain.js";
import {
  PayoutConfigurationMismatchError,
  PayoutInsufficientFundsError,
  PayoutNotConfiguredError,
  PayoutService,
} from "./payout.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;
const HOT_WALLET = "0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A";
const OTHER_WALLET = "0x1563915e194D8CfBA1943570603F7606A3115508";
const RECIPIENT = "0x55d398326f99059fF775485246999027B3197955";

type FakeChain = {
  hotAddress: string;
  assertReady: ReturnType<typeof vi.fn>;
  balances: ReturnType<typeof vi.fn>;
  gasCostPerTransferWei: ReturnType<typeof vi.fn>;
  pendingNonce: ReturnType<typeof vi.fn>;
  simulateTransfer: ReturnType<typeof vi.fn>;
  signTransfer: ReturnType<typeof vi.fn>;
  broadcastRaw: ReturnType<typeof vi.fn>;
  waitForConfirmation: ReturnType<typeof vi.fn>;
  reconcileTransaction: ReturnType<typeof vi.fn>;
};

function fakeChain(overrides: Partial<FakeChain> = {}): FakeChain {
  return {
    hotAddress: HOT_WALLET,
    assertReady: vi.fn().mockResolvedValue(undefined),
    balances: vi.fn().mockResolvedValue({ usdtWei: 1_000_000n * USD * USD, bnbWei: 10n ** 18n }),
    gasCostPerTransferWei: vi.fn().mockReturnValue(5_000_000_000_000n),
    pendingNonce: vi.fn().mockResolvedValue(7),
    simulateTransfer: vi.fn().mockResolvedValue(undefined),
    signTransfer: vi.fn().mockImplementation(async (_to: string, _amount: bigint, nonce: number): Promise<SignedTransfer> => ({
      hash: `0x${nonce.toString(16).padStart(64, "0")}`,
      raw: `0x${nonce.toString(16).padStart(2, "0")}feed`,
      nonce,
    })),
    broadcastRaw: vi.fn().mockResolvedValue(undefined),
    waitForConfirmation: vi.fn().mockResolvedValue({ status: "confirmed", blockNumber: 100 } satisfies ConfirmResult),
    reconcileTransaction: vi.fn().mockResolvedValue(null),
    ...overrides,
  };
}

function config(overrides: Record<string, unknown> = {}): ConfigService<Environment, true> {
  const values: Record<string, unknown> = {
    SALES_MIN_PAYOUT_USD: 0,
    PAYOUT_HOT_WALLET_KEY: `0x${"11".repeat(32)}`,
    PAYOUT_SEND_RPC_URL: "https://bsc.invalid",
    PAYOUT_READ_RPC_URLS: "https://bsc.invalid",
    PAYOUT_USDT_CONTRACT: RECIPIENT,
    PAYOUT_CHAIN_ID: 56,
    PAYOUT_GAS_PRICE_GWEI: "0.05",
    PAYOUT_CONFIRMATIONS: 3,
    PAYOUT_ENFORCE_WINDOW: false,
    ...overrides,
  };
  return { get: (key: string) => values[key] } as unknown as ConfigService<Environment, true>;
}

class TestPayoutService extends PayoutService {
  constructor(database: SalesDatabase, cfg: ConfigService<Environment, true>, private readonly fake: FakeChain) {
    super(database, cfg);
  }

  protected override createChain(): PayoutChain {
    return this.fake as unknown as PayoutChain;
  }
}

describe.runIf(Boolean(connectionString))("PayoutService composition", () => {
  let database: SalesDatabase;
  let eventId = 0;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-payout-service-test");
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await database.pool.query("TRUNCATE partners, commission_entries, payouts, payout_batches RESTART IDENTITY CASCADE");
    await database.pool.end();
  });

  beforeEach(async () => {
    eventId = 0;
    await database.pool.query("TRUNCATE partners, commission_entries, payouts, payout_batches RESTART IDENTITY CASCADE");
  });

  async function earn(amountNano: bigint, createdAt = new Date()): Promise<string> {
    const partner = await database.pool.query<{ id: string }>(`
      INSERT INTO partners (
        referral_code, status, telegram_username, payout_method, payout_details
      ) VALUES ($1, 'active', $1, 'usdt-bep20', $2::jsonb)
      RETURNING id
    `, [randomUUID(), JSON.stringify({ network: "BSC", address: RECIPIENT })]);
    eventId += 1;
    const usage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events (
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES ($1, $2, $3, $4, $5)
      RETURNING id
    `, [eventId, randomUUID(), partner.rows[0]!.id, amountNano.toString(), createdAt]);
    await database.pool.query(`
      INSERT INTO commission_entries (
        usage_event_id, partner_id, level, applied_bps, amount_nano, created_at
      ) VALUES ($1, $2, 0, 1000, $3, $4)
    `, [usage.rows[0]!.id, partner.rows[0]!.id, amountNano.toString(), createdAt]);
    return partner.rows[0]!.id;
  }

  async function batch(amounts: bigint[], hotWalletAddress = HOT_WALLET): Promise<{ id: string }> {
    const recipients = [];
    for (const amount of amounts) {
      const partnerId = await earn(amount);
      recipients.push({ partnerId, amountNano: amount, walletAddress: RECIPIENT });
    }
    return createPayoutBatch(database, {
      createdBy: "test-admin",
      minNano: 0n,
      gasPriceGwei: "0.05",
      hotWalletAddress,
      recipients,
    });
  }

  it("requires a configured, chain-validated engine before prepare", async () => {
    const chain = fakeChain();
    const disabled = new TestPayoutService(database, config({
      PAYOUT_HOT_WALLET_KEY: undefined,
      PAYOUT_SEND_RPC_URL: undefined,
    }), chain);
    await expect(disabled.prepare("admin")).rejects.toBeInstanceOf(PayoutNotConfiguredError);

    await earn(10n * USD, new Date("2020-01-01T00:00:00Z"));
    chain.assertReady.mockRejectedValueOnce(new Error("wrong token"));
    const enabled = new TestPayoutService(database, config(), chain);
    await expect(enabled.prepare("admin")).rejects.toThrow("wrong token");
    expect(await database.pool.query("SELECT count(*)::int AS n FROM payout_batches").then((r) => r.rows[0]!.n)).toBe(0);
  });

  it("uses the one integer minimum and admits the exact boundary", async () => {
    await earn(10n * USD, new Date("2020-01-01T00:00:00Z"));
    await earn(10n * USD - 1n, new Date("2020-01-01T00:00:00Z"));
    const service = new TestPayoutService(database, config({ SALES_MIN_PAYOUT_USD: 10 }), fakeChain());
    const report = await service.prepare("admin");
    expect(report.rows).toHaveLength(1);
    expect(report.rows[0]!.amountNano).toBe(10n * USD);
    expect(report.batch.minNano).toBe(10n * USD);
  });

  it("fails a simulation without broadcasting and keeps the row reviewable", async () => {
    const created = await batch([10n * USD]);
    const chain = fakeChain({ simulateTransfer: vi.fn().mockRejectedValue(new Error("execution reverted")) });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.broadcastRaw).not.toHaveBeenCalled();
    expect(report!.rows[0]).toMatchObject({ status: "requested", chainStatus: "failed", txHash: null });
    expect(report!.batch.status).toBe("prepared");
    expect(await new TestPayoutService(database, config(), chain).release(report!.rows[0]!.id)).toBe(true);
    expect((await getPayoutBatch(database, created.id))!.status).toBe("sent");
  });

  it("rechecks exact token and gas balances before any signing", async () => {
    for (const balances of [
      { usdtWei: 1n, bnbWei: 10n ** 18n },
      { usdtWei: 100n * USD * USD, bnbWei: 1n },
    ]) {
      await database.pool.query("TRUNCATE partners, commission_entries, payouts, payout_batches RESTART IDENTITY CASCADE");
      const created = await batch([10n * USD]);
      const chain = fakeChain({ balances: vi.fn().mockResolvedValue(balances) });
      const service = new TestPayoutService(database, config(), chain);
      await expect(service.send(created.id)).rejects.toBeInstanceOf(PayoutInsufficientFundsError);
      expect(chain.signTransfer).not.toHaveBeenCalled();
      expect((await getPayoutBatch(database, created.id))!.status).toBe("prepared");
    }
  });

  it("does not broadcast when the row is released between signing and reservation", async () => {
    const created = await batch([10n * USD]);
    const row = (await listBatchPayouts(database, created.id))[0]!;
    const chain = fakeChain();
    chain.signTransfer.mockImplementationOnce(async (_to: string, _amount: bigint, nonce: number) => {
      await releasePayoutRow(database, row.id);
      return { hash: `0x${"1".repeat(64)}`, raw: "0x01feed", nonce };
    });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.broadcastRaw).not.toHaveBeenCalled();
    expect(report!.rows[0]!.status).toBe("rejected");
  });

  it("treats already-known rebroadcast as idempotent and confirms once", async () => {
    const created = await batch([10n * USD]);
    const chain = fakeChain({ broadcastRaw: vi.fn().mockRejectedValue(new Error("already known")) });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.broadcastRaw).toHaveBeenCalledTimes(1);
    expect(report!.rows[0]).toMatchObject({ status: "paid", chainStatus: "confirmed" });
    expect(report!.batch.status).toBe("sent");
  });

  it("retains the exact tx and stops the queue after repeated broadcast uncertainty", async () => {
    const created = await batch([20n * USD, 10n * USD]);
    const chain = fakeChain({ broadcastRaw: vi.fn().mockRejectedValue(new Error("gateway timeout")) });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.broadcastRaw).toHaveBeenCalledTimes(3);
    expect(chain.signTransfer).toHaveBeenCalledTimes(1);
    expect(report!.rows[0]).toMatchObject({ chainStatus: "broadcast", nonce: 7, rawTx: "0x07feed" });
    expect(report!.rows[1]).toMatchObject({ chainStatus: "pending", txHash: null });
    expect(report!.batch.status).toBe("sending");

    await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.signTransfer).toHaveBeenCalledTimes(1);
  });

  it("stops after confirmation timeout and lets the claim-once poller finish idempotently", async () => {
    const created = await batch([20n * USD, 10n * USD]);
    const chain = fakeChain({ waitForConfirmation: vi.fn().mockResolvedValue(null) });
    const service = new TestPayoutService(database, config(), chain);
    let report = await service.send(created.id);
    expect(chain.signTransfer).toHaveBeenCalledTimes(1);
    expect(report!.rows[0]!.chainStatus).toBe("broadcast");
    expect(report!.rows[1]!.chainStatus).toBe("pending");

    chain.reconcileTransaction.mockResolvedValue({ status: "confirmed", blockNumber: 101 });
    expect(await service.confirmBroadcasts()).toBe(1);
    expect(await service.confirmBroadcasts()).toBe(0);
    report = await service.report(created.id);
    expect(report!.rows[0]).toMatchObject({ status: "paid", chainStatus: "confirmed" });
  });

  it("retains a hash on confirmation transport failure instead of advancing the queue", async () => {
    const created = await batch([20n * USD, 10n * USD]);
    const chain = fakeChain({ waitForConfirmation: vi.fn().mockRejectedValue(new Error("all read RPCs failed")) });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(report!.rows[0]!.chainStatus).toBe("broadcast");
    expect(report!.rows[0]!.chainError).toContain("poller will reconcile");
    expect(report!.rows[1]!.chainStatus).toBe("pending");
  });

  it("releases a definitively reverted nonce and proceeds with the next exact nonce", async () => {
    const created = await batch([20n * USD, 10n * USD]);
    const chain = fakeChain();
    chain.waitForConfirmation
      .mockResolvedValueOnce({ status: "reverted", blockNumber: 100 })
      .mockResolvedValueOnce({ status: "confirmed", blockNumber: 101 });
    const report = await new TestPayoutService(database, config(), chain).send(created.id);
    expect(chain.signTransfer.mock.calls.map((call) => call[2])).toEqual([7, 8]);
    expect(report!.rows[0]!.chainStatus).toBe("failed");
    expect(report!.rows[1]).toMatchObject({ status: "paid", chainStatus: "confirmed" });
  });

  it("uses confirmed account nonce to resolve a retained nonce-too-low hash", async () => {
    const created = await batch([10n * USD]);
    const chain = fakeChain({
      broadcastRaw: vi.fn().mockRejectedValue(new Error("nonce too low")),
      reconcileTransaction: vi.fn().mockResolvedValue({ status: "nonce_consumed", blockNumber: null }),
    });
    const service = new TestPayoutService(database, config(), chain);
    let report = await service.send(created.id);
    expect(report!.rows[0]!.chainStatus).toBe("broadcast");
    expect(await service.confirmBroadcasts()).toBe(1);
    report = await service.report(created.id);
    expect(report!.rows[0]).toMatchObject({ status: "requested", chainStatus: "failed", txHash: null });
    expect(report!.batch.status).toBe("prepared");
  });

  it("pins the prepared hot wallet and cannot resurrect a concurrently canceled batch", async () => {
    const wrongWalletBatch = await batch([10n * USD]);
    const mismatch = new TestPayoutService(database, config(), fakeChain({ hotAddress: OTHER_WALLET }));
    await expect(mismatch.send(wrongWalletBatch.id)).rejects.toBeInstanceOf(PayoutConfigurationMismatchError);
    expect((await getPayoutBatch(database, wrongWalletBatch.id))!.status).toBe("prepared");

    expect(await mismatch.cancel(wrongWalletBatch.id)).toBe(true);
    const canceledReport = await mismatch.send(wrongWalletBatch.id);
    expect(canceledReport!.batch.status).toBe("canceled");
  });

  it("serializes send and cancel through the same cross-process lock", async () => {
    const created = await batch([10n * USD]);
    let releaseReady!: () => void;
    const ready = new Promise<void>((resolve) => { releaseReady = resolve; });
    let entered!: () => void;
    const enteredReady = new Promise<void>((resolve) => { entered = resolve; });
    const chain = fakeChain({
      assertReady: vi.fn().mockImplementation(async () => { entered(); await ready; }),
    });
    const service = new TestPayoutService(database, config(), chain);
    const sending = service.send(created.id);
    await enteredReady;
    await expect(service.cancel(created.id)).rejects.toBeInstanceOf(PayoutBatchInProgressError);
    releaseReady();
    await sending;
    expect((await getPayoutBatch(database, created.id))!.status).toBe("sent");
  });
});
