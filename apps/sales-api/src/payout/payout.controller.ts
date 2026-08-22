import { Controller, Get, Header, HttpCode, HttpException, HttpStatus, NotFoundException, Param, Post, UseGuards } from "@nestjs/common";
import { z } from "zod";
import { InvalidPayoutBatchError, PayoutBatchInProgressError, type PayoutBatch, type PayoutRow } from "@claude-api/sales-db";
import { AdminKeyGuard } from "../admin.guard.js";
import {
  PayoutConfigurationMismatchError,
  PayoutAccountingNotReadyError,
  PayoutInsufficientFundsError,
  PayoutNotConfiguredError,
  PayoutService,
  PayoutWindowClosedError,
  type PayoutReport as ServiceReport,
} from "./payout.service.js";

const uuidSchema = z.string().uuid();

function serializeBatch(b: PayoutBatch): Record<string, unknown> {
  return {
    id: b.id, status: b.status, hotWalletAddress: b.hotWalletAddress,
    totalNano: b.totalNano.toString(), recipientCount: b.recipientCount,
    gasPriceGwei: b.gasPriceGwei, minNano: b.minNano.toString(),
    earnedBefore: b.earnedBefore?.toISOString() ?? null, note: b.note,
    createdBy: b.createdBy, error: b.error,
    createdAt: b.createdAt.toISOString(),
    preparedAt: b.preparedAt?.toISOString() ?? null,
    sentAt: b.sentAt?.toISOString() ?? null,
    completedAt: b.completedAt?.toISOString() ?? null,
  };
}

export function payoutRowIdentity(r: Pick<PayoutRow, "partnerId" | "email" | "displayName" | "telegramUsername">): string {
  return r.email ?? r.displayName ?? (r.telegramUsername ? `@${r.telegramUsername}` : r.partnerId.slice(0, 8));
}

function serializeRow(r: PayoutRow): Record<string, unknown> {
  return {
    id: r.id, partnerId: r.partnerId,
    partner: payoutRowIdentity(r),
    amountNano: r.amountNano.toString(), status: r.status, walletAddress: r.walletAddress,
    txHash: r.txHash, chainStatus: r.chainStatus, chainError: r.chainError,
    paidAt: r.paidAt?.toISOString() ?? null,
  };
}

function serializeReport(report: ServiceReport): Record<string, unknown> {
  return {
    batch: serializeBatch(report.batch),
    rows: report.rows.map(serializeRow),
    window: report.window,
    chain: report.chain,
    invalidAddresses: report.invalidAddresses,
    accounting: report.accounting,
  };
}

@Controller("admin/payouts")
@UseGuards(AdminKeyGuard)
export class PayoutController {
  constructor(private readonly payouts: PayoutService) {}

  private wrap<T>(fn: () => Promise<T>): Promise<T> {
    return fn().catch((err: unknown) => {
      if (err instanceof PayoutWindowClosedError) throw new HttpException(err.message, HttpStatus.LOCKED); // 423
      if (err instanceof PayoutAccountingNotReadyError) {
        throw new HttpException({ error: err.message, reasons: err.reasons }, HttpStatus.LOCKED); // 423
      }
      if (err instanceof PayoutNotConfiguredError) throw new HttpException(err.message, HttpStatus.SERVICE_UNAVAILABLE); // 503
      if (err instanceof PayoutBatchInProgressError
        || err instanceof InvalidPayoutBatchError
        || err instanceof PayoutConfigurationMismatchError
        || err instanceof PayoutInsufficientFundsError) {
        throw new HttpException(err.message, HttpStatus.CONFLICT); // 409
      }
      throw err;
    });
  }

  /** Состояние движка выплат: настроен ли, адрес горячего кошелька, открыто ли окно. */
  @Get("engine")
  @Header("Cache-Control", "no-store")
  async engine(): Promise<unknown> {
    return this.payouts.engineState();
  }

  @Get("batches")
  @Header("Cache-Control", "no-store")
  async batches(): Promise<unknown> {
    return { items: (await this.payouts.listBatches()).map(serializeBatch) };
  }

  @Get("batches/:id")
  @Header("Cache-Control", "no-store")
  async batch(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new NotFoundException("invalid batch id");
    const report = await this.payouts.report(id);
    if (!report) throw new NotFoundException("batch not found");
    return serializeReport(report);
  }

  /** Фаза 1: подготовка. Собирает кандидатов, создаёт батч 'prepared', отдаёт отчёт для просмотра. */
  @Post("prepare")
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  async prepare(): Promise<unknown> {
    const report = await this.wrap(() => this.payouts.prepare("sales-admin-key"));
    return serializeReport(report);
  }

  /** Фаза 2: отправка всего батча. Возможна ТОЛЬКО в окно выплат (иначе 423). */
  @Post("batches/:id/send")
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  async send(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new NotFoundException("invalid batch id");
    const report = await this.wrap(() => this.payouts.send(id));
    if (!report) throw new NotFoundException("batch not found");
    return serializeReport(report);
  }

  /** Ручная отправка/повтор одной строки. Тоже под гейтом окна (423 вне окна). */
  @Post("rows/:payoutId/send")
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  async sendOne(@Param("payoutId") payoutId: string): Promise<unknown> {
    if (!uuidSchema.safeParse(payoutId).success) throw new NotFoundException("invalid payout id");
    const result = await this.wrap(() => this.payouts.sendOne(payoutId));
    return { ok: result.ok, row: result.row ? serializeRow(result.row) : null };
  }

  @Post("batches/:id/cancel")
  @HttpCode(200)
  async cancel(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new NotFoundException("invalid batch id");
    return { canceled: await this.payouts.cancel(id) };
  }

  /** Освободить баланс failed-строки обратно в оборот (не под гейтом окна). */
  @Post("rows/:payoutId/release")
  @HttpCode(200)
  async release(@Param("payoutId") payoutId: string): Promise<unknown> {
    if (!uuidSchema.safeParse(payoutId).success) throw new NotFoundException("invalid payout id");
    return { released: await this.payouts.release(payoutId) };
  }
}
