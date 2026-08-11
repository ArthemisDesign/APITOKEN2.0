import {
  isCanonicalNanoUsd,
  isPositiveNanoUsd,
  type PayoutEngine,
  type PayoutReportDto,
  type PayoutRowDto,
} from "./api";

export type PayoutSendGate =
  | { allowed: true; sendableCount: number }
  | { allowed: false; reason: string };

const BSC_ADDRESS = /^0x[0-9a-fA-F]{40}$/;
const TX_HASH = /^0x[0-9a-fA-F]{64}$/;
const CHAIN_STATUSES = new Set([null, "pending", "simulated", "broadcast", "confirmed", "failed"]);
const ROW_STATUSES = new Set(["requested", "paid", "rejected"]);

function deny(reason: string): PayoutSendGate {
  return { allowed: false, reason };
}

function sameAddress(left: string | null, right: string | null): boolean {
  return left !== null
    && right !== null
    && BSC_ADDRESS.test(left)
    && BSC_ADDRESS.test(right)
    && left.toLowerCase() === right.toLowerCase();
}

function isCanonicalUnsignedInteger(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9]\d*)$/.test(value);
}

export function isSendablePayoutRow(row: PayoutRowDto): boolean {
  return row.status === "requested"
    && (row.chainStatus === null
      || row.chainStatus === "pending"
      || row.chainStatus === "simulated"
      || row.chainStatus === "failed");
}

/**
 * UI safety gate for an irreversible payout. The server remains authoritative; this gate makes the
 * browser refuse ambiguous, stale, malformed, or internally inconsistent reports before it asks
 * the server to send anything.
 */
export function evaluatePayoutSendGate(
  engine: PayoutEngine | null,
  report: PayoutReportDto | null,
): PayoutSendGate {
  if (!engine || !report) return deny("Payout state is unavailable. Refresh before sending.");
  if (engine.configured !== true || report.chain.configured !== true) {
    return deny("The payout engine is not fully configured.");
  }
  if (engine.window.open !== true || report.window.open !== true) {
    return deny("The payout window is closed.");
  }
  if (report.batch.status !== "prepared") {
    return deny("Only a prepared, idle batch can be sent.");
  }
  if (report.chain.configurationMatchesBatch !== true) {
    return deny("The current hot wallet does not match the wallet pinned to this batch.");
  }
  if (
    !sameAddress(report.batch.hotWalletAddress, report.chain.hotWalletAddress)
    || !sameAddress(report.batch.hotWalletAddress, report.chain.currentHotWalletAddress)
  ) {
    return deny("The payout wallet identity is unavailable or inconsistent.");
  }
  if (!Array.isArray(report.rows) || report.rows.length === 0) {
    return deny("The batch has no payout rows.");
  }
  if (
    !Number.isSafeInteger(report.batch.recipientCount)
    || report.batch.recipientCount <= 0
    || report.batch.recipientCount !== report.rows.length
  ) {
    return deny("The batch recipient count is inconsistent.");
  }
  if (!isPositiveNanoUsd(report.batch.totalNano)) {
    return deny("The batch total is unavailable or invalid.");
  }

  let allRowsTotal = 0n;
  let requiredTotal = 0n;
  let sendableCount = 0;
  for (const row of report.rows) {
    if (!ROW_STATUSES.has(row.status) || !CHAIN_STATUSES.has(row.chainStatus)) {
      return deny("A payout row has an unknown state.");
    }
    if (!isPositiveNanoUsd(row.amountNano) || !row.walletAddress || !BSC_ADDRESS.test(row.walletAddress)) {
      return deny("A payout row has invalid money or wallet data.");
    }
    if (row.chainStatus === "broadcast") {
      return deny("A transaction is still broadcast and unresolved. Wait for confirmation.");
    }
    if ((row.status === "paid") !== (row.chainStatus === "confirmed")) {
      return deny("A payout row has inconsistent payment and chain states.");
    }
    if (row.status === "rejected" && row.chainStatus === "confirmed") {
      return deny("A rejected payout row is marked confirmed.");
    }
    if (
      (row.chainStatus === "confirmed" && (!row.txHash || !TX_HASH.test(row.txHash)))
      || (row.chainStatus !== "confirmed" && row.txHash !== null)
    ) {
      return deny("A payout row has inconsistent transaction evidence.");
    }

    const amount = BigInt(row.amountNano);
    allRowsTotal += amount;
    if (row.status !== "paid" && row.status !== "rejected" && row.chainStatus !== "confirmed") {
      requiredTotal += amount;
    }
    if (isSendablePayoutRow(row)) sendableCount += 1;
  }

  if (allRowsTotal !== BigInt(report.batch.totalNano)) {
    return deny("The batch total does not equal its payout rows.");
  }
  if (!isPositiveNanoUsd(report.chain.requiredUsdtNano)
    || requiredTotal !== BigInt(report.chain.requiredUsdtNano)) {
    return deny("The required USDT total is unavailable or inconsistent.");
  }
  if (sendableCount === 0) return deny("The batch has no sendable payout rows.");

  if (
    report.chain.sufficientUsdt !== true
    || !isCanonicalNanoUsd(report.chain.usdtBalanceNano)
    || BigInt(report.chain.usdtBalanceNano) < requiredTotal
  ) {
    return deny("The hot wallet has no verified sufficient USDT balance.");
  }
  if (
    report.chain.sufficientBnb !== true
    || !isCanonicalUnsignedInteger(report.chain.requiredBnbWei)
    || report.chain.requiredBnbWei === "0"
    || !isCanonicalUnsignedInteger(report.chain.bnbBalanceWei)
    || BigInt(report.chain.bnbBalanceWei) < BigInt(report.chain.requiredBnbWei)
  ) {
    return deny("The hot wallet has no verified sufficient BNB gas balance.");
  }

  return { allowed: true, sendableCount };
}
