import type { PayoutEngineState, PayoutReport, PayoutRow } from "./types";

export type PayoutGate = { allowed: true; sendableCount: number } | { allowed: false; reason: string };
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const HASH = /^0x[0-9a-fA-F]{64}$/;
const ROW_STATES = new Set(["requested", "paid", "rejected"]);
const CHAIN_STATES = new Set<string | null>([null, "pending", "simulated", "broadcast", "confirmed", "failed"]);
const integer = (value: unknown): value is string => typeof value === "string" && /^(0|[1-9]\d*)$/.test(value);
const positive = (value: unknown): value is string => integer(value) && value !== "0";
const deny = (reason: string): PayoutGate => ({ allowed: false, reason });

export function payoutRowSendable(row: PayoutRow): boolean {
  return row.status === "requested" && (row.chainStatus === null || row.chainStatus === "pending" || row.chainStatus === "simulated" || row.chainStatus === "failed");
}

function sameAddress(left: string | null, right: string | null): boolean {
  return left !== null && right !== null && ADDRESS.test(left) && ADDRESS.test(right) && left.toLowerCase() === right.toLowerCase();
}

/** Fail closed before an irreversible send. The backend re-runs the same authoritative checks. */
export function payoutGate(engine: PayoutEngineState | null, report: PayoutReport | null): PayoutGate {
  if (!engine || !report) return deny("state_unavailable");
  if (!engine.configured || !report.chain.configured) return deny("not_configured");
  if (!engine.window.open || !report.window.open) return deny("window_closed");
  if (report.batch.status !== "prepared") return deny("batch_not_prepared");
  const accounting = report.accounting;
  if (!accounting || !accounting.ready || accounting.reasons.length) return deny("accounting_not_ready");
  if (!integer(accounting.usageCursor) || accounting.usageCursor !== accounting.usageSourceHead
    || !integer(accounting.fundingLotCursor) || accounting.fundingLotCursor !== accounting.fundingLotSourceHead
    || !integer(accounting.paymentReversalCursor) || accounting.paymentReversalCursor !== accounting.paymentReversalSourceHead
    || accounting.incompleteUsageCount !== "0" || accounting.missingCommissionSliceCount !== "0" || accounting.incompleteReversalCount !== "0") return deny("accounting_incomplete");
  if (report.chain.configurationMatchesBatch !== true
    || !sameAddress(report.batch.hotWalletAddress, report.chain.hotWalletAddress)
    || !sameAddress(report.batch.hotWalletAddress, report.chain.currentHotWalletAddress)) return deny("wallet_mismatch");
  if (!report.rows.length || report.batch.recipientCount !== report.rows.length || !positive(report.batch.totalNano)) return deny("batch_inconsistent");

  let all = 0n; let required = 0n; let sendableCount = 0;
  for (const row of report.rows) {
    if (!ROW_STATES.has(row.status) || !CHAIN_STATES.has(row.chainStatus) || !positive(row.amountNano) || !row.walletAddress || !ADDRESS.test(row.walletAddress)) return deny("row_invalid");
    if (row.chainStatus === "broadcast") return deny("broadcast_unresolved");
    if ((row.status === "paid") !== (row.chainStatus === "confirmed")) return deny("row_state_inconsistent");
    if ((row.chainStatus === "confirmed" && (!row.txHash || !HASH.test(row.txHash))) || (row.chainStatus !== "confirmed" && row.txHash !== null)) return deny("transaction_evidence_invalid");
    const amount = BigInt(row.amountNano); all += amount;
    if (row.status !== "paid" && row.status !== "rejected" && row.chainStatus !== "confirmed") required += amount;
    if (payoutRowSendable(row)) sendableCount += 1;
  }
  if (all !== BigInt(report.batch.totalNano) || !positive(report.chain.requiredUsdtNano) || required !== BigInt(report.chain.requiredUsdtNano)) return deny("totals_inconsistent");
  if (!sendableCount) return deny("nothing_sendable");
  if (report.chain.sufficientUsdt !== true || !integer(report.chain.usdtBalanceNano) || BigInt(report.chain.usdtBalanceNano) < required) return deny("usdt_insufficient");
  if (report.chain.sufficientBnb !== true || !positive(report.chain.requiredBnbWei) || !integer(report.chain.bnbBalanceWei) || BigInt(report.chain.bnbBalanceWei) < BigInt(report.chain.requiredBnbWei)) return deny("bnb_insufficient");
  return { allowed: true, sendableCount };
}
