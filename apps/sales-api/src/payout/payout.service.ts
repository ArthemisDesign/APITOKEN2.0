import { Inject, Injectable, Logger, type OnApplicationShutdown, type OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  acquirePayoutSendLock,
  releasePayoutSendLock,
  cancelPayoutBatch,
  acquirePartnerAccountingLock,
  createPayoutBatch,
  finalizeStuckSendingBatches,
  getActiveBatch,
  getMaxOutstandingNonce,
  getPayoutBatch,
  getPayoutCandidates,
  getPreparedPayoutBalanceProofs,
  getPartnerPayoutAccountingProof,
  listBatchPayouts,
  listBroadcastPayouts,
  listPayoutBatches,
  listSendablePayouts,
  markPayoutBroadcast,
  markPayoutConfirmed,
  markPayoutFailed,
  releasePayoutRow,
  releasePartnerAccountingLock,
  lastEndedPeriod,
  periodInPayoutWindow,
  transitionPayoutBatchStatus,
  windowEnd,
  windowStart,
  InvalidPayoutBatchError,
  PayoutBatchInProgressError,
  type PayoutBatch,
  type PayoutRow,
  type PartnerPayoutAccountingProof,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { SALES_DATABASE } from "../infrastructure.module.js";
import type { Environment } from "../config.js";
import {
  PartnerFeedNotReadyError,
  SyncService,
  type PartnerFeedHeads,
} from "../sync.service.js";
import { PayoutChain, normalizeBscAddress, nanoToUsdtWei } from "./chain.js";

const NANO = 1_000_000_000n;
const SEND_RETRIES = 2;
export type AccountingLockClient = Awaited<ReturnType<typeof acquirePartnerAccountingLock>>;

export class PayoutWindowClosedError extends Error {}
export class PayoutNotConfiguredError extends Error {}
export class PayoutConfigurationMismatchError extends Error {}
export class PayoutInsufficientFundsError extends Error {}
export class PayoutAccountingNotReadyError extends Error {
  constructor(public readonly reasons: string[]) {
    super(`partner accounting is not ready: ${reasons.join("; ")}`);
  }
}

export interface PayoutReport {
  batch: PayoutBatch;
  rows: PayoutRow[];
  window: { open: boolean; opensAt: string | null; closesAt: string | null };
  chain: {
    configured: boolean;
    hotWalletAddress: string | null;
    currentHotWalletAddress: string | null;
    configurationMatchesBatch: boolean | null;
    usdtBalanceNano: string | null;
    bnbBalanceWei: string | null;
    requiredUsdtNano: string;
    requiredBnbWei: string | null;
    sufficientUsdt: boolean | null;
    sufficientBnb: boolean | null;
    gasPriceGwei: string;
  };
  invalidAddresses: { partnerId: string; walletAddress: string; reason: string }[];
  accounting: ReturnType<typeof serializeAccountingProof> | null;
}

function serializeAccountingProof(proof: PartnerPayoutAccountingProof) {
  return {
    ready: proof.ready,
    reasons: proof.reasons,
    usageCursor: proof.usageCursor.toString(),
    usageSourceHead: proof.expectedUsageHead.toString(),
    fundingLotCursor: proof.fundingLotCursor.toString(),
    fundingLotSourceHead: proof.expectedFundingLotHead.toString(),
    paymentReversalCursor: proof.paymentReversalCursor.toString(),
    paymentReversalSourceHead: proof.expectedPaymentReversalHead.toString(),
    incompleteUsageCount: proof.incompleteUsageCount.toString(),
    missingCommissionSliceCount: proof.missingCommissionSliceCount.toString(),
    incompleteReversalCount: proof.incompleteReversalCount.toString(),
    reversalCount: proof.reversalCount.toString(),
    adjustmentCount: proof.adjustmentCount.toString(),
    adjustmentNano: proof.adjustmentNano.toString(),
  };
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
    private readonly sync: SyncService,
  ) {}

  private lastAccountingProof: ReturnType<typeof serializeAccountingProof> | null = null;

  protected async assertAccountingReady(): Promise<PartnerPayoutAccountingProof> {
    try {
      return await this.sync.withPayoutFence(async (heads) => {
        const proof = await getPartnerPayoutAccountingProof(this.database, heads);
        this.rememberAccounting(proof);
        if (!proof.ready) throw new PayoutAccountingNotReadyError(proof.reasons);
        return proof;
      });
    } catch (error) {
      if (error instanceof PartnerFeedNotReadyError) {
        throw new PayoutAccountingNotReadyError(error.reasons);
      }
      throw error;
    }
  }

  private rememberAccounting(proof: PartnerPayoutAccountingProof): void {
    this.lastAccountingProof = serializeAccountingProof(proof);
  }

  /**
   * Linearizable money boundary: drain while owning the SyncService mutex, then take the shared
   * Sales accounting lock, re-probe all three Commerce source heads, and keep both fences through
   * the caller's final local proof and payout commitment/signing.
   */
  protected async withCurrentAccounting<T>(
    callback: (client: AccountingLockClient, proof: PartnerPayoutAccountingProof) => Promise<T>,
  ): Promise<T> {
    try {
      return await this.sync.withPayoutFence(async (drainedHeads) => this.withAccountingLock(
        async (accountingClient) => {
          const heads = await this.sync.probePayoutSourceHeads(drainedHeads);
          const proof = await getPartnerPayoutAccountingProof(this.database, heads);
          this.rememberAccounting(proof);
          if (!proof.ready) throw new PayoutAccountingNotReadyError(proof.reasons);
          return callback(accountingClient, proof);
        },
      ));
    } catch (error) {
      if (error instanceof PartnerFeedNotReadyError) {
        throw new PayoutAccountingNotReadyError(error.reasons);
      }
      throw error;
    }
  }

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
    return BigInt(this.config.get("SALES_MIN_PAYOUT_USD", { infer: true })) * NANO;
  }
  private gasPriceGwei(): string {
    return this.config.get("PAYOUT_GAS_PRICE_GWEI", { infer: true });
  }

  isConfigured(): boolean {
    return Boolean(this.config.get("PAYOUT_HOT_WALLET_KEY", { infer: true }) && this.config.get("PAYOUT_SEND_RPC_URL", { infer: true }));
  }

  protected createChain(): PayoutChain {
    return new PayoutChain({
      privateKey: this.config.get("PAYOUT_HOT_WALLET_KEY", { infer: true })!,
      sendRpcUrl: this.config.get("PAYOUT_SEND_RPC_URL", { infer: true })!,
      readRpcUrls: this.config.get("PAYOUT_READ_RPC_URLS", { infer: true }).split(",").map((s) => s.trim()).filter(Boolean),
      usdtContract: this.config.get("PAYOUT_USDT_CONTRACT", { infer: true }),
      chainId: this.config.get("PAYOUT_CHAIN_ID", { infer: true }),
      gasPriceGwei: this.gasPriceGwei(),
      confirmations: this.config.get("PAYOUT_CONFIRMATIONS", { infer: true }),
    });
  }

  private getChain(): PayoutChain {
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured (missing hot-wallet key or send RPC)");
    if (!this.chain) this.chain = this.createChain();
    return this.chain;
  }

  private assertBatchWallet(batch: PayoutBatch, chain: PayoutChain): void {
    if (!batch.hotWalletAddress || batch.hotWalletAddress.toLowerCase() !== chain.hotAddress.toLowerCase()) {
      throw new PayoutConfigurationMismatchError(
        "the current hot wallet does not match the wallet pinned when this batch was prepared",
      );
    }
  }

  private async assertFunds(chain: PayoutChain, rows: PayoutRow[]): Promise<void> {
    const requiredNano = rows.reduce((total, row) => total + row.amountNano, 0n);
    if (requiredNano <= 0n || rows.length === 0) {
      throw new InvalidPayoutBatchError("payout batch has no sendable positive rows");
    }
    const { usdtWei, bnbWei } = await chain.balances();
    if (usdtWei < nanoToUsdtWei(requiredNano)) {
      throw new PayoutInsufficientFundsError("hot wallet has insufficient USDT for this payout batch");
    }
    if (bnbWei < chain.gasCostPerTransferWei() * BigInt(rows.length)) {
      throw new PayoutInsufficientFundsError("hot wallet has insufficient BNB for this payout batch");
    }
  }

  windowInfo(now = new Date()): { open: boolean; opensAt: string | null; closesAt: string | null; enforced: boolean } {
    const enforced = this.config.get("PAYOUT_ENFORCE_WINDOW", { infer: true });
    const period = periodInPayoutWindow(now);
    // Если гейт окна выключен (PAYOUT_ENFORCE_WINDOW=false) — отправка разрешена всегда, показываем open.
    if (!enforced) {
      return { open: true, opensAt: period ? windowStart(period).toISOString() : null, closesAt: period ? windowEnd(period).toISOString() : null, enforced: false };
    }
    if (!period) return { open: false, opensAt: null, closesAt: null, enforced: true };
    return { open: true, opensAt: windowStart(period).toISOString(), closesAt: windowEnd(period).toISOString(), enforced: true };
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
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured");
    const chain = this.getChain();
    await chain.assertReady();
    const prepared = await this.withCurrentAccounting(async (accountingClient) => {
      const existing = await getActiveBatch(this.database);
      if (existing) throw new PayoutBatchInProgressError("a payout batch is already in progress");

      // Граница начислений = конец периода, чьё окно выплат открыто (иначе — последний завершённый).
      // Так батч платит только «разлоченные» деньги и совпадает с превью getDuePayoutList (уважает лок 7д).
      const now = new Date();
      const period = periodInPayoutWindow(now) ?? lastEndedPeriod(now);
      const candidates = await getPayoutCandidates(this.database, this.minNano(), period.end);
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
      const batch = await createPayoutBatch(this.database, {
        createdBy: adminId,
        minNano: this.minNano(),
        gasPriceGwei: this.gasPriceGwei(),
        hotWalletAddress: chain.hotAddress,
        earnedBefore: period.end,
        accountingClient,
        recipients,
      });
      return { batch, invalidAddresses };
    });
    const report = await this.report(prepared.batch.id, prepared.invalidAddresses);
    if (!report) throw new Error("batch vanished after creation");
    return report;
  }

  async report(batchId: string, invalidAddresses: PayoutReport["invalidAddresses"] = []): Promise<PayoutReport | null> {
    const batch = await getPayoutBatch(this.database, batchId);
    if (!batch) return null;
    const rows = await listBatchPayouts(this.database, batchId);
    const pendingRows = rows.filter(
      (row) => row.status !== "paid" && row.status !== "rejected" && row.chainStatus !== "confirmed",
    );
    const requiredUsdtNano = pendingRows.reduce((s, r) => s + r.amountNano, 0n);

    const chain: PayoutReport["chain"] = {
      configured: this.isConfigured(),
      hotWalletAddress: batch.hotWalletAddress,
      currentHotWalletAddress: null,
      configurationMatchesBatch: null,
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
        chain.currentHotWalletAddress = c.hotAddress;
        chain.configurationMatchesBatch = Boolean(
          batch.hotWalletAddress
            && batch.hotWalletAddress.toLowerCase() === c.hotAddress.toLowerCase(),
        );
        const { usdtWei, bnbWei } = await c.balances();
        const usdtBalanceNano = usdtWei / NANO; // 1e18 wei → nano (1e9)
        const requiredBnbWei = c.gasCostPerTransferWei() * BigInt(pendingRows.length);
        chain.usdtBalanceNano = usdtBalanceNano.toString();
        chain.bnbBalanceWei = bnbWei.toString();
        chain.requiredBnbWei = requiredBnbWei.toString();
        chain.sufficientUsdt = usdtWei >= nanoToUsdtWei(requiredUsdtNano);
        chain.sufficientBnb = bnbWei >= requiredBnbWei;
      } catch (err) {
        this.logger.error(`report balance read failed: ${err instanceof Error ? err.message : "unknown"}`);
      }
    }
    return {
      batch, rows, window: this.windowInfo(), chain, invalidAddresses,
      accounting: this.lastAccountingProof,
    };
  }

  async listBatches(): Promise<PayoutBatch[]> {
    return listPayoutBatches(this.database);
  }

  async cancel(batchId: string): Promise<boolean> {
    return this.withSendLock(() => cancelPayoutBatch(this.database, batchId));
  }

  /** Возвращает баланс failed/sim-fail строки в оборот (→ статус 'rejected'). НЕ трогает 'broadcast'
   * (транза могла уйти). Не под гейтом окна — освобождение денег безопасно всегда. */
  async release(payoutId: string): Promise<boolean> {
    const row = await this.findRow(payoutId);
    const released = await releasePayoutRow(this.database, payoutId);
    if (released && row?.batchId) await this.reconcileBatchState(row.batchId);
    return released;
  }

  // --- send ----------------------------------------------------------------

  /**
   * Единственный вход в отправку: держит и in-memory флаг, и КРОСС-ПРОЦЕССНЫЙ pg advisory-lock, чтобы
   * ни второй инстанс (HA), ни параллельный sendOne не считали nonce одновременно.
   */
  /**
   * Стартовый nonce = max(on-chain 'pending' count, максимальный уже отправленный broadcast-nonce + 1).
   * Публичный read-RPC может НЕ видеть in-flight tx BlockRazor, поэтому одного pendingNonce мало —
   * иначе следующая строка возьмёт занятый nonce и застрянет. Персистнутый broadcast-nonce авторитетен.
   */
  private async computeStartNonce(chain: PayoutChain): Promise<number> {
    const [pending, maxOutstanding] = await Promise.all([
      chain.pendingNonce(),
      getMaxOutstandingNonce(this.database),
    ]);
    return maxOutstanding === null ? pending : Math.max(pending, maxOutstanding + 1);
  }

  private async withSendLock<T>(fn: () => Promise<T>): Promise<T> {
    if (this.sending) throw new PayoutBatchInProgressError("a payout send is already running");
    const lock = await acquirePayoutSendLock(this.database);
    if (!lock) throw new PayoutBatchInProgressError("another payout send is in progress");
    this.sending = true;
    try {
      return await fn();
    } finally {
      this.sending = false;
      await releasePayoutSendLock(lock);
    }
  }

  private async withAccountingLock<T>(fn: (client: AccountingLockClient) => Promise<T>): Promise<T> {
    const client = await acquirePartnerAccountingLock(this.database);
    try {
      return await fn(client);
    } finally {
      await releasePartnerAccountingLock(client);
    }
  }

  private async assertPreparedBalances(
    client: AccountingLockClient,
    batch: PayoutBatch,
    rows: PayoutRow[],
  ): Promise<void> {
    if (batch.earnedBefore === null) {
      throw new InvalidPayoutBatchError(
        "payout batch has no pinned earnings boundary; cancel it and prepare a fresh batch",
      );
    }
    const proofs = await getPreparedPayoutBalanceProofs(
      client,
      rows.map((row) => row.id),
      batch.earnedBefore,
    );
    const byPayout = new Map(proofs.map((proof) => [proof.payoutId, proof]));
    for (const row of rows) {
      const proof = byPayout.get(row.id);
      if (!proof || proof.partnerId !== row.partnerId || proof.amountNano !== row.amountNano) {
        throw new InvalidPayoutBatchError("payout row changed before its final accounting proof");
      }
      if (proof.amountNano > proof.allowedNano) {
        throw new InvalidPayoutBatchError(
          "payout exceeds the current signed partner balance; cancel it and prepare a fresh batch",
        );
      }
    }
  }

  /** Отправляет весь батч по очереди: симуляция → подпись → сохранение хеша → бродкаст → подтверждение. */
  async send(batchId: string): Promise<PayoutReport | null> {
    this.assertSendAllowed();
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured");
    await this.withSendLock(async () => {
      await this.withCurrentAccounting(async (accountingClient) => {
        // Re-read only after both cross-process locks. A cancellation, reversal, or earlier sender
        // may have changed state after the HTTP request began; stale state must never be signed.
        const batch = await getPayoutBatch(this.database, batchId);
        if (!batch || (batch.status !== "prepared" && batch.status !== "sending")) return;
        const chain = this.getChain();
        await chain.assertReady();
        this.assertBatchWallet(batch, chain);
        if (await getMaxOutstandingNonce(this.database) !== null) {
          this.logger.log(`batch ${batchId}: an earlier nonce is unresolved; poller owns progress`);
          return;
        }
        const rows = await listSendablePayouts(this.database, batchId);
        if (rows.length === 0) {
          await this.reconcileBatchState(batchId);
          return;
        }
        await this.assertPreparedBalances(accountingClient, batch, rows);
        await this.assertFunds(chain, rows);
        const claimed = await transitionPayoutBatchStatus(
          this.database,
          batchId,
          ["prepared", "sending"],
          "sending",
        );
        if (!claimed) return;
        let nonce = await this.computeStartNonce(chain);
        for (const row of rows) {
          const result = await this.sendRow(chain, row, nonce);
          nonce = result.nextNonce;
          if (result.stop) break;
        }
        await this.reconcileBatchState(batchId);
        if (await getMaxOutstandingNonce(this.database) !== null) {
          this.logger.log(`batch ${batchId}: tx confirming; poller will finalize`);
        }
      });
    });
    return this.report(batchId);
  }

  /** Ручная отправка/повтор ОДНОЙ строки (кнопка в админке). Под гейтом окна И под тем же send-локом. */
  async sendOne(payoutId: string): Promise<{ ok: boolean; row: PayoutRow | null }> {
    this.assertSendAllowed();
    if (!this.isConfigured()) throw new PayoutNotConfiguredError("payout engine is not configured");
    return this.withSendLock(async () => {
      return this.withCurrentAccounting(async (accountingClient) => {
        const chain = this.getChain();
        await chain.assertReady();
        const target = await this.findRow(payoutId);
        // 'broadcast'/'confirmed'/'paid'/'rejected' не пересылаем — иначе rejected→re-send =
        // двойная выплата, а broadcast/confirmed решает поллер по хешу.
        if (!target || target.status === "paid" || target.status === "rejected"
            || target.chainStatus === "broadcast" || target.chainStatus === "confirmed") {
          return { ok: false, row: target };
        }
        if (!target.batchId) return { ok: false, row: target };
        const batch = await getPayoutBatch(this.database, target.batchId);
        if (!batch || (batch.status !== "prepared" && batch.status !== "sending")) {
          return { ok: false, row: target };
        }
        this.assertBatchWallet(batch, chain);
        if (await getMaxOutstandingNonce(this.database) !== null) {
          throw new PayoutBatchInProgressError("an earlier payout transaction is still unresolved");
        }
        await this.assertPreparedBalances(accountingClient, batch, [target]);
        await this.assertFunds(chain, [target]);
        if (!await transitionPayoutBatchStatus(
          this.database,
          batch.id,
          ["prepared", "sending"],
          "sending",
        )) return { ok: false, row: target };
        const nonce = await this.computeStartNonce(chain);
        await this.sendRow(chain, target, nonce);
        await this.reconcileBatchState(target.batchId);
        return { ok: true, row: await this.findRow(payoutId) };
      });
    });
  }

  private async findRow(payoutId: string): Promise<PayoutRow | null> {
    const r = await this.database.pool.query<Record<string, unknown>>(`
      SELECT po.id, po.partner_id, po.amount_nano::text AS amount_nano, po.status, po.wallet_address,
             po.tx_hash, po.nonce, po.raw_tx, po.chain_status, po.chain_error, po.paid_at, po.batch_id,
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
      txHash: (row.tx_hash as string) ?? null, nonce: row.nonce === null || row.nonce === undefined ? null : Number(row.nonce),
      rawTx: (row.raw_tx as string) ?? null,
      chainStatus: (row.chain_status as PayoutRow["chainStatus"]) ?? null,
      chainError: (row.chain_error as string) ?? null, paidAt: (row.paid_at as Date) ?? null,
      batchId: (row.batch_id as string) ?? null,
    };
  }

  /**
   * Безопасная отправка одной строки. Порядок критичен для защиты от ДВОЙНОЙ ВЫПЛАТЫ:
   *   1) симуляция (eth_call) — если ревёртнётся, не отправляем (nonce не тратится) → строка retriable;
   *   2) ОФЛАЙН-подпись → хеш известен ДО бродкаста → сохраняем hash+nonce → только потом бродкаст.
   * При ошибке бродкаста хеш НЕ теряем и nonce НЕ бампаем: строка остаётся 'broadcast', её судьбу
   * решает поллер по сохранённому хешу (ре-бродкаст того же raw идемпотентен). Свежий nonce берём
   * только когда транзакция ДОСТОВЕРНО завершилась без перевода (on-chain revert). Возвращает
   * следующий nonce и надо ли остановить очередь.
   */
  private async sendRow(chain: PayoutChain, row: PayoutRow, startNonce: number): Promise<{ nextNonce: number; stop: boolean }> {
    const msg = (e: unknown): string => (e instanceof Error ? e.message : "unknown");
    if (!row.walletAddress) {
      await markPayoutFailed(this.database, row.id, "no wallet address");
      return { nextNonce: startNonce, stop: false }; // не отправлялось, nonce свободен
    }
    // 1) симуляция — nonce не тратит
    try {
      await chain.simulateTransfer(row.walletAddress, row.amountNano);
    } catch (err) {
      await markPayoutFailed(this.database, row.id, `simulation reverted: ${msg(err)}`);
      return { nextNonce: startNonce, stop: false };
    }
    // 2) офлайн-подпись — nonce ещё не тратится, но хеш уже известен
    let signed;
    try {
      signed = await chain.signTransfer(row.walletAddress, row.amountNano, startNonce);
    } catch (err) {
      await markPayoutFailed(this.database, row.id, `sign failed: ${msg(err)}`);
      return { nextNonce: startNonce, stop: false };
    }
    // сохраняем hash+nonce+raw ДО бродкаста — с этого момента nonce «зарезервирован» этой транзакцией.
    // Если строка уже не 'requested' (её успели release/paid) — НЕ бродкастим (защита от двойной выплаты).
    const reserved = await markPayoutBroadcast(this.database, row.id, signed.hash, signed.nonce, signed.raw);
    if (!reserved) return { nextNonce: startNonce, stop: false };
    // бродкаст с повторами ТОГО ЖЕ raw (никогда не переподписываем с новым nonce на неопределённости)
    let broadcastOk = false;
    for (let attempt = 0; attempt <= SEND_RETRIES; attempt += 1) {
      try { await chain.broadcastRaw(signed.raw); broadcastOk = true; break; }
      catch (err) {
        const m = msg(err).toLowerCase();
        if (m.includes("already known") || m.includes("already imported")) { broadcastOk = true; break; }
        if (attempt === SEND_RETRIES) {
          // Бродкаст не удался: хеш СОХРАНЁН, строка остаётся 'broadcast'. Транза могла уйти в сеть или нет —
          // поллер разберётся/ре-бродкастнет по сохранённому хешу. Очередь СТОП: пока судьба этого nonce
          // неясна, следующий занимать нельзя. Двойной выплаты нет (ре-бродкаст — тот же tx).
          await this.database.pool.query("UPDATE payouts SET chain_error = $2 WHERE id = $1", [row.id, `broadcast error (tx retained, poller will reconcile): ${msg(err)}`.slice(0, 500)]);
          return { nextNonce: startNonce, stop: true };
        }
      }
    }
    if (!broadcastOk) return { nextNonce: startNonce, stop: true };
    // 3) подтверждение
    let conf;
    try {
      conf = await chain.waitForConfirmation(signed.hash);
    } catch (err) {
      await this.database.pool.query(
        "UPDATE payouts SET chain_error = $2 WHERE id = $1 AND chain_status = 'broadcast'",
        [row.id, `confirmation error (poller will reconcile): ${msg(err)}`.slice(0, 500)],
      );
      return { nextNonce: startNonce + 1, stop: true };
    }
    if (conf?.status === "confirmed") {
      if (await markPayoutConfirmed(this.database, row.id)) await this.notifyPaid(row, signed.hash);
      return { nextNonce: startNonce + 1, stop: false };
    }
    if (conf?.status === "reverted") {
      // Достоверно: транза добыта, nonce потрачен, перевода НЕ было → строку можно повторить (новый nonce).
      await markPayoutFailed(this.database, row.id, "transaction reverted on-chain (no transfer)");
      return { nextNonce: startNonce + 1, stop: false };
    }
    // таймаут: ещё не в блоке — оставляем 'broadcast', поллер добьёт; nonce занят → очередь стоп
    return { nextNonce: startNonce + 1, stop: true };
  }

  // --- confirmation poller (safety net for broadcast rows) -----------------

  async confirmBroadcasts(): Promise<number> {
    if (!this.isConfigured()) return 0;
    // Берём ТОТ ЖЕ send-лок, чтобы поллер не гонялся с send()/sendOne() на общих строках/батчах.
    // Занят (идёт отправка) → пропускаем тик.
    const lock = await acquirePayoutSendLock(this.database);
    if (!lock) return 0;
    try {
      const chain = this.getChain();
      const rows = await listBroadcastPayouts(this.database, 100);
      let done = 0;
      for (const row of rows) {
        if (!row.txHash) continue;
        try {
          const conf = await chain.reconcileTransaction(row.txHash, row.nonce);
          if (conf?.status === "confirmed") {
            // claim-once: уведомляем только если ЭТА сторона реально перевела 'broadcast'→'confirmed'
            if (await markPayoutConfirmed(this.database, row.id)) { await this.notifyPaid(row, row.txHash); done += 1; }
          } else if (conf?.status === "reverted") {
            await markPayoutFailed(this.database, row.id, "transaction reverted on-chain");
            done += 1;
          } else if (conf?.status === "nonce_consumed") {
            // A later confirmed account nonce proves this exact hash can no longer land. This is
            // the only safe escape from a retained `nonce too low` transaction: never infer it
            // from the RPC error alone, and never abandon a hash while its nonce is still live.
            await markPayoutFailed(this.database, row.id, "nonce was consumed by a different transaction");
            done += 1;
          } else if (row.rawTx) {
            // ещё не в блоке — ре-бродкастим СОХРАНЁННЫЙ raw (ровно тот же tx, идемпотентно), чтобы
            // разлочить возможный gap после потерянного ответа. Никакой переподписи.
            try { await chain.broadcastRaw(row.rawTx); } catch { /* «already known»/сеть — норм */ }
          }
        } catch {
          // сеть недоступна — попробуем на следующем тике
        }
        if (row.batchId) await this.reconcileBatchState(row.batchId);
      }
      // Разблокируем застрявшие 'sending'-батчи без broadcast-строк (напр. краш во время send).
      await finalizeStuckSendingBatches(this.database);
      return done;
    } finally {
      await releasePayoutSendLock(lock);
    }
  }

  private async reconcileBatchState(batchId: string): Promise<void> {
    const batch = await getPayoutBatch(this.database, batchId);
    if (!batch || batch.status === "sent" || batch.status === "canceled") return;
    const rows = await listBatchPayouts(this.database, batchId);
    const anyBroadcast = rows.some((r) => r.chainStatus === "broadcast");
    const anySendable = rows.some(
      (row) => row.status === "requested"
        && (row.chainStatus === null || ["pending", "simulated", "failed"].includes(row.chainStatus)),
    );
    const allPaid = rows.every((r) => r.status === "paid");
    const next = anyBroadcast ? "sending" : anySendable ? "prepared" : "sent";
    await transitionPayoutBatchStatus(
      this.database,
      batchId,
      ["prepared", "sending"],
      next,
      { sent: next === "sent", completed: next === "sent" && allPaid },
    );
  }

  // --- partner notification (cabinet + email) ------------------------------

  private async notifyPaid(row: PayoutRow, txHash: string): Promise<void> {
    // Партнёр видит выплату (сумма + хеш + ссылка на BscScan + статус) в кабинете /dashboard/payouts —
    // это и есть уведомление. Отдельная e-mail-рассылка «payout_paid» — следующий шаг (текущий
    // email-воркер заточен под токен-письма). Здесь только несекретный лог факта выплаты.
    this.logger.log(`payout confirmed: partner=${row.partnerId} amountNano=${row.amountNano} tx=${txHash}`);
  }
}
