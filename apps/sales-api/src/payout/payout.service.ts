import { Inject, Injectable, Logger, type OnApplicationShutdown, type OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  cancelPayoutBatch,
  createPayoutBatch,
  getActiveBatch,
  getPayoutBatch,
  getPayoutCandidates,
  listBatchPayouts,
  listBroadcastPayouts,
  listPayoutBatches,
  listSendablePayouts,
  markPayoutBroadcast,
  markPayoutConfirmed,
  markPayoutFailed,
  periodInPayoutWindow,
  setBatchStatus,
  windowEnd,
  windowStart,
  PayoutBatchInProgressError,
  type PayoutBatch,
  type PayoutRow,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { SALES_DATABASE } from "../infrastructure.module.js";
import type { Environment } from "../config.js";
import { PayoutChain, normalizeBscAddress, nanoToUsdtWei } from "./chain.js";

const NANO = 1_000_000_000n;
const SEND_RETRIES = 2;

export class PayoutWindowClosedError extends Error {}
export class PayoutNotConfiguredError extends Error {}

export interface PayoutReport {
  batch: PayoutBatch;
  rows: PayoutRow[];
  window: { open: boolean; opensAt: string | null; closesAt: string | null };
  chain: {
    configured: boolean;
    hotWalletAddress: string | null;
    usdtBalanceNano: string | null;
    bnbBalanceWei: string | null;
    requiredUsdtNano: string;
    requiredBnbWei: string | null;
    sufficientUsdt: boolean | null;
    sufficientBnb: boolean | null;
    gasPriceGwei: string;
  };
  invalidAddresses: { partnerId: string; walletAddress: string; reason: string }[];
}

@Injectable()
export class PayoutService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(PayoutService.name);
  private chain: PayoutChain | null = null;
  private sending = false;
  private stopped = false;

  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  onModuleInit(): void {
    if (!this.isConfigured()) return;
    void this.pollLoop(); // фоновый добой подтверждений для broadcast-строк
  }

  onApplicationShutdown(): void {
    this.stopped = true;
  }

  private async pollLoop(): Promise<void> {
    while (!this.stopped) {
      try {
        const n = await this.confirmBroadcasts();
        if (n > 0) this.logger.log(`payout poller finalized ${n} tx`);
      } catch (err) {
        this.logger.error(`payout poller failed: ${err instanceof Error ? err.message : "unknown"}`);
      }
      await new Promise((r) => setTimeout(r, 15_000));
    }
  }

  private minNano(): bigint {
    return BigInt(Math.round(this.config.get("PAYOUT_MIN_USD", { infer: true }) * 1e9));
  }
  private gasPriceGwei(): string {
    return this.config.get("PAYOUT_GAS_PRICE_GWEI", { infer: true });
  }

  isConfigured(): boolean {
    return Boolean(this.config.get("PAYOUT_HOT_WALLET_KEY", { infer: true }) && this.config.get("PAYOUT_SEND_RPC_URL", { infer: true }));
  }

  private getChain(): PayoutChain {
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured (missing hot-wallet key or send RPC)");
    if (!this.chain) {
      this.chain = new PayoutChain({
        privateKey: this.config.get("PAYOUT_HOT_WALLET_KEY", { infer: true })!,
        sendRpcUrl: this.config.get("PAYOUT_SEND_RPC_URL", { infer: true })!,
        readRpcUrls: this.config.get("PAYOUT_READ_RPC_URLS", { infer: true }).split(",").map((s) => s.trim()).filter(Boolean),
        usdtContract: this.config.get("PAYOUT_USDT_CONTRACT", { infer: true }),
        chainId: this.config.get("PAYOUT_CHAIN_ID", { infer: true }),
        gasPriceGwei: this.gasPriceGwei(),
        confirmations: this.config.get("PAYOUT_CONFIRMATIONS", { infer: true }),
      });
    }
    return this.chain;
  }

  windowInfo(now = new Date()): { open: boolean; opensAt: string | null; closesAt: string | null } {
    const period = periodInPayoutWindow(now);
    if (!period) return { open: false, opensAt: null, closesAt: null };
    return { open: true, opensAt: windowStart(period).toISOString(), closesAt: windowEnd(period).toISOString() };
  }

  /** Жёсткий гейт: отправка возможна ТОЛЬКО в 3-дневное окно выплат (если PAYOUT_ENFORCE_WINDOW). */
  private assertSendAllowed(): void {
    if (!this.config.get("PAYOUT_ENFORCE_WINDOW", { infer: true })) return;
    if (!periodInPayoutWindow(new Date())) {
      throw new PayoutWindowClosedError("payouts can only be sent during the 3-day payout window");
    }
  }

  // --- prepare -------------------------------------------------------------

  async prepare(adminId: string): Promise<PayoutReport> {
    const existing = await getActiveBatch(this.database);
    if (existing) throw new PayoutBatchInProgressError("a payout batch is already in progress");

    const candidates = await getPayoutCandidates(this.database, this.minNano());
    const recipients: { partnerId: string; amountNano: bigint; walletAddress: string }[] = [];
    const invalidAddresses: PayoutReport["invalidAddresses"] = [];
    for (const c of candidates) {
      try {
        recipients.push({ partnerId: c.partnerId, amountNano: c.unpaidNano, walletAddress: normalizeBscAddress(c.walletAddress) });
      } catch (err) {
        invalidAddresses.push({ partnerId: c.partnerId, walletAddress: c.walletAddress, reason: err instanceof Error ? err.message : "invalid address" });
      }
    }
    if (recipients.length === 0) {
      throw new PayoutBatchInProgressError("no eligible recipients (valid address + balance > 0)");
    }
    const hotAddress = this.isConfigured() ? this.getChain().hotAddress : "";
    const batch = await createPayoutBatch(this.database, {
      createdBy: adminId,
      minNano: this.minNano(),
      gasPriceGwei: this.gasPriceGwei(),
      hotWalletAddress: hotAddress,
      recipients,
    });
    const report = await this.report(batch.id, invalidAddresses);
    if (!report) throw new Error("batch vanished after creation");
    return report;
  }

  async report(batchId: string, invalidAddresses: PayoutReport["invalidAddresses"] = []): Promise<PayoutReport | null> {
    const batch = await getPayoutBatch(this.database, batchId);
    if (!batch) return null;
    const rows = await listBatchPayouts(this.database, batchId);
    const pendingRows = rows.filter((r) => r.status !== "paid" && r.chainStatus !== "confirmed");
    const requiredUsdtNano = pendingRows.reduce((s, r) => s + r.amountNano, 0n);

    const chain: PayoutReport["chain"] = {
      configured: this.isConfigured(),
      hotWalletAddress: batch.hotWalletAddress,
      usdtBalanceNano: null,
      bnbBalanceWei: null,
      requiredUsdtNano: requiredUsdtNano.toString(),
      requiredBnbWei: null,
      sufficientUsdt: null,
      sufficientBnb: null,
      gasPriceGwei: batch.gasPriceGwei ?? this.gasPriceGwei(),
    };
    if (this.isConfigured()) {
      try {
        const c = this.getChain();
        const { usdtWei, bnbWei } = await c.balances();
        const usdtBalanceNano = usdtWei / NANO; // 1e18 wei → nano (1e9)
        const requiredBnbWei = c.gasCostPerTransferWei() * BigInt(pendingRows.length);
        chain.hotWalletAddress = c.hotAddress;
        chain.usdtBalanceNano = usdtBalanceNano.toString();
        chain.bnbBalanceWei = bnbWei.toString();
        chain.requiredBnbWei = requiredBnbWei.toString();
        chain.sufficientUsdt = usdtWei >= nanoToUsdtWei(requiredUsdtNano);
        chain.sufficientBnb = bnbWei >= requiredBnbWei;
      } catch (err) {
        this.logger.error(`report balance read failed: ${err instanceof Error ? err.message : "unknown"}`);
      }
    }
    return { batch, rows, window: this.windowInfo(), chain, invalidAddresses };
  }

  async listBatches(): Promise<PayoutBatch[]> {
    return listPayoutBatches(this.database);
  }

  async cancel(batchId: string): Promise<boolean> {
    return cancelPayoutBatch(this.database, batchId);
  }

  // --- send ----------------------------------------------------------------

  /** Отправляет весь батч по очереди: симуляция → бродкаст → подтверждение → следующий. Быстро и безопасно. */
  async send(batchId: string): Promise<PayoutReport | null> {
    this.assertSendAllowed();
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured");
    if (this.sending) throw new PayoutBatchInProgressError("a send is already running");
    const batch = await getPayoutBatch(this.database, batchId);
    if (!batch || (batch.status !== "prepared" && batch.status !== "sending")) return this.report(batchId);

    this.sending = true;
    try {
      const chain = this.getChain();
      await chain.assertNetwork();
      await setBatchStatus(this.database, batchId, "sending");
      const rows = await listSendablePayouts(this.database, batchId);
      let nonce = await chain.pendingNonce();
      for (const row of rows) {
        const result = await this.sendRow(chain, row, nonce);
        if (result.consumedNonce) nonce += 1;
        if (result.stop) break; // таймаут подтверждения — не рискуем разрывом nonce, дожмёт поллер/повтор
      }
      // Итог батча
      const after = await listBatchPayouts(this.database, batchId);
      const allPaid = after.every((r) => r.status === "paid");
      const anyBroadcast = after.some((r) => r.chainStatus === "broadcast");
      await setBatchStatus(this.database, batchId, allPaid ? "sent" : "sending", { sent: true, completed: allPaid });
      if (anyBroadcast) this.logger.log(`batch ${batchId}: some tx still confirming; poller will finalize`);
      return this.report(batchId);
    } finally {
      this.sending = false;
    }
  }

  /** Ручная отправка/повтор ОДНОЙ строки (кнопка в админке). Тоже под гейтом окна. */
  async sendOne(payoutId: string): Promise<{ ok: boolean; row: PayoutRow | null }> {
    this.assertSendAllowed();
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured");
    const chain = this.getChain();
    await chain.assertNetwork();
    const target = await this.findRow(payoutId);
    if (!target || target.status === "paid" || target.chainStatus === "broadcast" || target.chainStatus === "confirmed") {
      return { ok: false, row: target };
    }
    const nonce = await chain.pendingNonce();
    await this.sendRow(chain, target, nonce);
    return { ok: true, row: await this.findRow(payoutId) };
  }

  private async findRow(payoutId: string): Promise<PayoutRow | null> {
    const r = await this.database.pool.query<Record<string, unknown>>(`
      SELECT po.id, po.partner_id, po.amount_nano::text AS amount_nano, po.status, po.wallet_address,
             po.tx_hash, po.chain_status, po.chain_error, po.paid_at,
             p.telegram_username, p.email, p.display_name
      FROM payouts po JOIN partners p ON p.id = po.partner_id WHERE po.id = $1
    `, [payoutId]);
    const row = r.rows[0];
    if (!row) return null;
    return {
      id: row.id as string, partnerId: row.partner_id as string,
      telegramUsername: (row.telegram_username as string) ?? null, email: (row.email as string) ?? null,
      displayName: (row.display_name as string) ?? null, amountNano: BigInt((row.amount_nano as string) ?? "0"),
      status: row.status as string, walletAddress: (row.wallet_address as string) ?? null,
      txHash: (row.tx_hash as string) ?? null, chainStatus: (row.chain_status as PayoutRow["chainStatus"]) ?? null,
      chainError: (row.chain_error as string) ?? null, paidAt: (row.paid_at as Date) ?? null,
    };
  }

  /**
   * Обрабатывает одну строку: до SEND_RETRIES попыток симуляция→бродкаст→подтверждение. Реджект/ревёрт
   * → повтор. Возвращает, был ли израсходован nonce и надо ли остановить очередь (таймаут подтверждения).
   */
  private async sendRow(chain: PayoutChain, row: PayoutRow, startNonce: number): Promise<{ consumedNonce: boolean; stop: boolean }> {
    if (!row.walletAddress) {
      await markPayoutFailed(this.database, row.id, "no wallet address");
      return { consumedNonce: false, stop: false };
    }
    let nonce = startNonce;
    let consumed = false;
    for (let attempt = 0; attempt <= SEND_RETRIES; attempt += 1) {
      try {
        await chain.simulateTransfer(row.walletAddress, row.amountNano); // eth_call, ничего не тратит
      } catch (err) {
        await markPayoutFailed(this.database, row.id, `simulation reverted: ${err instanceof Error ? err.message : "unknown"}`);
        return { consumedNonce: consumed, stop: false }; // симуляция не тратит nonce
      }
      try {
        const { hash } = await chain.sendTransfer(row.walletAddress, row.amountNano, nonce);
        await markPayoutBroadcast(this.database, row.id, hash);
        consumed = true;
        const conf = await chain.waitForConfirmation(hash);
        if (conf?.status === "confirmed") {
          await markPayoutConfirmed(this.database, row.id);
          await this.notifyPaid(row, hash);
          return { consumedNonce: true, stop: false };
        }
        if (conf?.status === "reverted") {
          await markPayoutFailed(this.database, row.id, "transaction reverted on-chain");
          nonce += 1; // ревёрт съел nonce — повтор со следующим
          continue;
        }
        // timeout: транзакция ещё не в блоке — не рвём очередь, дожмёт поллер
        return { consumedNonce: true, stop: true };
      } catch (err) {
        // ошибка бродкаста (реджект relay и т.п.) — повтор с тем же nonce
        await markPayoutFailed(this.database, row.id, `broadcast failed: ${err instanceof Error ? err.message : "unknown"}`);
        if (attempt === SEND_RETRIES) return { consumedNonce: consumed, stop: false };
      }
    }
    return { consumedNonce: consumed, stop: false };
  }

  // --- confirmation poller (safety net for broadcast rows) -----------------

  async confirmBroadcasts(): Promise<number> {
    if (!this.isConfigured()) return 0;
    const rows = await listBroadcastPayouts(this.database, 100);
    if (rows.length === 0) return 0;
    const chain = this.getChain();
    let done = 0;
    for (const row of rows) {
      if (!row.txHash) continue;
      try {
        const conf = await chain.checkReceipt(row.txHash);
        if (conf?.status === "confirmed") {
          await markPayoutConfirmed(this.database, row.id);
          await this.notifyPaid(row, row.txHash);
          done += 1;
        } else if (conf?.status === "reverted") {
          await markPayoutFailed(this.database, row.id, "transaction reverted on-chain");
          done += 1;
        }
      } catch {
        // сеть недоступна — попробуем на следующем тике
      }
      // финализируем батч, если все строки закрыты
      if (row.batchId) await this.finalizeBatchIfDone(row.batchId);
    }
    return done;
  }

  private async finalizeBatchIfDone(batchId: string): Promise<void> {
    const batch = await getPayoutBatch(this.database, batchId);
    if (!batch || batch.status === "sent" || batch.status === "canceled") return;
    const rows = await listBatchPayouts(this.database, batchId);
    const anyBroadcast = rows.some((r) => r.chainStatus === "broadcast");
    const allPaid = rows.every((r) => r.status === "paid");
    if (!anyBroadcast && batch.status === "sending") {
      await setBatchStatus(this.database, batchId, "sent", { sent: true, completed: allPaid });
    }
  }

  // --- partner notification (cabinet + email) ------------------------------

  private async notifyPaid(row: PayoutRow, txHash: string): Promise<void> {
    // Партнёр видит выплату (сумма + хеш + ссылка на BscScan + статус) в кабинете /dashboard/payouts —
    // это и есть уведомление. Отдельная e-mail-рассылка «payout_paid» — следующий шаг (текущий
    // email-воркер заточен под токен-письма). Здесь только несекретный лог факта выплаты.
    this.logger.log(`payout confirmed: partner=${row.partnerId} amountNano=${row.amountNano} tx=${txHash}`);
  }
}
