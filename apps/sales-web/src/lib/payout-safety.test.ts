import { describe, expect, it } from "vitest";
import type { PayoutEngine, PayoutReportDto } from "./api";
import { evaluatePayoutSendGate } from "./payout-safety";

const wallet = "0x1111111111111111111111111111111111111111";
const txHash = `0x${"a".repeat(64)}`;

function fixture(): { engine: PayoutEngine; report: PayoutReportDto } {
  return {
    engine: { configured: true, window: { open: true, opensAt: null, closesAt: null } },
    report: {
      batch: {
        id: "batch",
        status: "prepared",
        hotWalletAddress: wallet,
        totalNano: "3000000000",
        recipientCount: 2,
        gasPriceGwei: "0.1",
        minNano: "0",
        note: null,
        createdBy: "admin",
        error: null,
        createdAt: "2026-08-11T00:00:00.000Z",
        preparedAt: "2026-08-11T00:00:00.000Z",
        sentAt: null,
        completedAt: null,
      },
      rows: [
        {
          id: "one",
          partnerId: "partner-one",
          partner: "one",
          amountNano: "1000000000",
          status: "requested",
          walletAddress: "0x2222222222222222222222222222222222222222",
          txHash: null,
          chainStatus: null,
          chainError: null,
          paidAt: null,
        },
        {
          id: "two",
          partnerId: "partner-two",
          partner: "two",
          amountNano: "2000000000",
          status: "requested",
          walletAddress: "0x3333333333333333333333333333333333333333",
          txHash: null,
          chainStatus: "failed",
          chainError: "reverted",
          paidAt: null,
        },
      ],
      window: { open: true, opensAt: null, closesAt: null },
      chain: {
        configured: true,
        hotWalletAddress: wallet,
        currentHotWalletAddress: wallet,
        configurationMatchesBatch: true,
        usdtBalanceNano: "4000000000",
        bnbBalanceWei: "2000000000000000",
        requiredUsdtNano: "3000000000",
        requiredBnbWei: "1000000000000000",
        sufficientUsdt: true,
        sufficientBnb: true,
        gasPriceGwei: "0.1",
      },
      invalidAddresses: [],
      accounting: {
        ready: true,
        reasons: [],
        usageCursor: "10",
        usageSourceHead: "10",
        fundingLotCursor: "20",
        fundingLotSourceHead: "20",
        paymentReversalCursor: "30",
        paymentReversalSourceHead: "30",
        incompleteUsageCount: "0",
        missingCommissionSliceCount: "0",
        incompleteReversalCount: "0",
        reversalCount: "1",
        adjustmentCount: "1",
        adjustmentNano: "-100",
      },
    },
  };
}

describe("payout send gate", () => {
  it("allows one exact, internally consistent prepared report", () => {
    const { engine, report } = fixture();
    expect(evaluatePayoutSendGate(engine, report)).toEqual({ allowed: true, sendableCount: 2 });
  });

  it.each([
    ["unknown balance", (f: ReturnType<typeof fixture>) => { f.report.chain.sufficientUsdt = null; }],
    ["insufficient gas", (f: ReturnType<typeof fixture>) => { f.report.chain.sufficientBnb = false; }],
    ["wallet drift", (f: ReturnType<typeof fixture>) => { f.report.chain.configurationMatchesBatch = false; }],
    ["malformed amount", (f: ReturnType<typeof fixture>) => { f.report.rows[0].amountNano = "01"; }],
    ["zero amount", (f: ReturnType<typeof fixture>) => { f.report.rows[0].amountNano = "0"; }],
    ["wrong batch total", (f: ReturnType<typeof fixture>) => { f.report.batch.totalNano = "3000000001"; }],
    ["wrong required total", (f: ReturnType<typeof fixture>) => { f.report.chain.requiredUsdtNano = "1"; }],
    ["recipient mismatch", (f: ReturnType<typeof fixture>) => { f.report.batch.recipientCount = 3; }],
    ["sending batch", (f: ReturnType<typeof fixture>) => { f.report.batch.status = "sending"; }],
    ["closed report window", (f: ReturnType<typeof fixture>) => { f.report.window.open = false; }],
    ["unknown row state", (f: ReturnType<typeof fixture>) => { f.report.rows[0].status = "mystery"; }],
    ["accounting lag", (f: ReturnType<typeof fixture>) => { f.report.accounting!.paymentReversalSourceHead = "31"; }],
    ["incomplete reversal", (f: ReturnType<typeof fixture>) => { f.report.accounting!.incompleteReversalCount = "1"; }],
  ])("fails closed for %s", (_name, mutate) => {
    const value = fixture();
    mutate(value);
    expect(evaluatePayoutSendGate(value.engine, value.report).allowed).toBe(false);
  });

  it("blocks the whole batch while any transaction remains broadcast", () => {
    const { engine, report } = fixture();
    report.rows[0].chainStatus = "broadcast";
    report.rows[0].txHash = txHash;
    expect(evaluatePayoutSendGate(engine, report)).toEqual({
      allowed: false,
      reason: "A transaction is still broadcast and unresolved. Wait for confirmation.",
    });
  });

  it("allows paid confirmed rows only when totals still match the full batch", () => {
    const { engine, report } = fixture();
    report.rows[0].status = "paid";
    report.rows[0].chainStatus = "confirmed";
    report.rows[0].txHash = txHash;
    report.chain.requiredUsdtNano = "2000000000";
    expect(evaluatePayoutSendGate(engine, report)).toEqual({ allowed: true, sendableCount: 1 });
  });
});
