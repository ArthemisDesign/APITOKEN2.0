import { describe, expect, it } from "vitest";
import { payoutGate } from "./payout-safety";
import type { PayoutEngineState, PayoutReport } from "./types";

const hotWallet = "0x1111111111111111111111111111111111111111";

function fixture(): { engine: PayoutEngineState; report: PayoutReport } {
  return {
    engine: { configured: true, window: { open: true, enforced: true, opensAt: null, closesAt: null } },
    report: {
      batch: {
        id: "batch", status: "prepared", hotWalletAddress: hotWallet, totalNano: "3000000000",
        recipientCount: 2, gasPriceGwei: "0.1", minNano: "1", earnedBefore: null, note: null,
        createdBy: "admin", error: null, createdAt: "2026-08-22T00:00:00Z", preparedAt: "2026-08-22T00:00:00Z",
        sentAt: null, completedAt: null,
      },
      rows: [
        { id: "one", partnerId: "p1", partner: "one@example.com", amountNano: "1000000000", status: "requested", walletAddress: "0x2222222222222222222222222222222222222222", txHash: null, chainStatus: null, chainError: null, paidAt: null },
        { id: "two", partnerId: "p2", partner: "two@example.com", amountNano: "2000000000", status: "requested", walletAddress: "0x3333333333333333333333333333333333333333", txHash: null, chainStatus: "failed", chainError: "reverted", paidAt: null },
      ],
      window: { open: true, enforced: true, opensAt: null, closesAt: null },
      chain: {
        configured: true, hotWalletAddress: hotWallet, currentHotWalletAddress: hotWallet,
        configurationMatchesBatch: true, requiredUsdtNano: "3000000000", requiredBnbWei: "1000000000000000",
        usdtBalanceNano: "4000000000", bnbBalanceWei: "2000000000000000", sufficientUsdt: true,
        sufficientBnb: true, gasPriceGwei: "0.1",
      },
      invalidAddresses: [],
      accounting: {
        ready: true, reasons: [], usageCursor: "10", usageSourceHead: "10", fundingLotCursor: "20",
        fundingLotSourceHead: "20", paymentReversalCursor: "30", paymentReversalSourceHead: "30",
        incompleteUsageCount: "0", missingCommissionSliceCount: "0", incompleteReversalCount: "0",
        reversalCount: "0", adjustmentCount: "0", adjustmentNano: "0",
      },
    },
  };
}

describe("Partner Admin payout safety gate", () => {
  it("allows only a complete, internally consistent prepared report", () => {
    const { engine, report } = fixture();
    expect(payoutGate(engine, report)).toEqual({ allowed: true, sendableCount: 2 });
  });

  it.each([
    ["accounting lag", (value: ReturnType<typeof fixture>) => { value.report.accounting!.usageSourceHead = "11"; }, "accounting_incomplete"],
    ["wallet drift", (value: ReturnType<typeof fixture>) => { value.report.chain.currentHotWalletAddress = "0x4444444444444444444444444444444444444444"; }, "wallet_mismatch"],
    ["non-canonical money", (value: ReturnType<typeof fixture>) => { value.report.rows[0]!.amountNano = "01"; }, "row_invalid"],
    ["wrong total", (value: ReturnType<typeof fixture>) => { value.report.batch.totalNano = "3000000001"; }, "totals_inconsistent"],
    ["unknown USDT proof", (value: ReturnType<typeof fixture>) => { value.report.chain.sufficientUsdt = null; }, "usdt_insufficient"],
    ["open broadcast", (value: ReturnType<typeof fixture>) => { value.report.rows[0]!.chainStatus = "broadcast"; }, "broadcast_unresolved"],
  ] as const)("fails closed for %s", (_name, mutate, reason) => {
    const value = fixture();
    mutate(value);
    expect(payoutGate(value.engine, value.report)).toEqual({ allowed: false, reason });
  });
});
