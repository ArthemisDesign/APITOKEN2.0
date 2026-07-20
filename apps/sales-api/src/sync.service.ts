import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  advanceSyncCursor,
  consumeDiscountLink,
  getSyncCursor,
  recordReferredDeposit,
  recordReferredSpend,
  resolveReferralCode,
  upsertReferredUser,
  type SalesDatabase,
  type SyncFeed,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";

const nanoStringSchema = z.string().regex(/^\d{1,27}$/).transform(BigInt);
const feedIdSchema = z.union([z.number().int().nonnegative(), z.string().regex(/^\d+$/)]).transform(BigInt);

const attributionSchema = z.object({
  id: feedIdSchema,
  userId: z.string().uuid(),
  code: z.string().min(1),
  createdAt: z.coerce.date(),
});

const topupSchema = z.object({
  id: feedIdSchema,
  paymentId: z.string().min(1),
  userId: z.string().uuid(),
  amountNano: nanoStringSchema,
  paidAt: z.coerce.date(),
});

// Списание за API. amountNano = real_funded: часть, покрытая реальными деньгами (коммерция уже
// вычла бесплатную часть по принципу «бесплатное тратится первым»). Может быть 0 → комиссии нет.
const usageEventSchema = z.object({
  id: feedIdSchema,
  userId: z.string().uuid(),
  amountNano: nanoStringSchema,
  occurredAt: z.coerce.date(),
});

@Injectable()
export class SyncService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(SyncService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });
  private readonly missingFeedLogged = new Set<SyncFeed>();

  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  onModuleInit(): void {
    this.loop = this.run().catch((error) => {
      this.logger.error(`sync loop terminated unexpectedly: ${message(error)}`);
    });
  }

  async onApplicationShutdown(): Promise<void> {
    this.stopped = true;
    this.stopSleep();
    await this.loop;
  }

  private async run(): Promise<void> {
    const intervalMs = this.config.get("SYNC_INTERVAL_MS", { infer: true });
    this.logger.log("commerce feed sync started");
    while (!this.stopped) {
      // The loop itself is the overlap guard: the next tick starts only after this one ends.
      try {
        await this.syncAttributions();
        await this.syncTopups();
        await this.syncUsageEvents();
      } catch (error) {
        this.logger.error(`sync iteration failed: ${message(error)}`);
      }
      await this.sleep(intervalMs);
    }
  }

  private async syncAttributions(): Promise<void> {
    const after = await getSyncCursor(this.database, "attributions");
    const rows = await this.fetchFeed("attributions", `attributions?after_id=${after}&limit=500`, attributionSchema);
    if (!rows || rows.length === 0) return;
    // Продвигаем курсор только по успешно обработанным строкам (rows идут по возрастанию id).
    // Флип в B2B идемпотентен; сбой останавливает батч (курсор до последней хорошей строки),
    // чтобы упавшая строка повторилась на следующем тике — at-least-once, без head-of-line stall.
    let lastOk = after;
    try {
      for (const row of rows) {
        const resolved = await resolveReferralCode(this.database, row.code);
        if (resolved) {
          const won = await upsertReferredUser(this.database, {
            commerceUserId: row.userId,
            partnerId: resolved.partnerId,
            referralCode: row.code,
            attributedAt: row.createdAt,
            sourceAttributionId: row.id,
          });
          // Только если ЭТА атрибуция реально закрепила юзера (upsert вставил строку). Если юзер уже
          // был привязан к другому партнёру, upsert — no-op: НЕ гасим чужую одноразовую ссылку и НЕ
          // применяем чужую скидку (иначе партнёр терял бы ссылку, а юзер получал незаслуженный floor).
          if (won) {
            // Персональная ОДНОРАЗОВАЯ ссылка со скидкой → гасим её этим пользователем.
            if (resolved.discountLinkId && !resolved.discountLinkConsumed) {
              await consumeDiscountLink(this.database, resolved.discountLinkId, row.userId);
            }
            // Скидочный floor шлём в commerce ТОЛЬКО когда он ненулевой. Обычный реф-код (floor 0,
            // обычные b2c-тиры) не должен дёргать commerce-эндпоинт — иначе его недоступность стопорит
            // вообще всю ленту атрибуции, включая рефералов без скидки (head-of-line stall).
            if (resolved.discountBps > 0) {
              await this.applyReferralDiscount(row.userId, resolved.discountBps);
            }
          }
        }
        lastOk = row.id;
      }
    } finally {
      if (lastOk !== after) await advanceSyncCursor(this.database, "attributions", lastOk);
    }
  }

  /** Просит commerce применить «пол» скидки сейлза рефералу партнёра (b2c + floor). Идемпотентно. */
  private async applyReferralDiscount(userId: string, floorBps: number): Promise<void> {
    const base = this.config.get("COMMERCE_BASE_URL", { infer: true });
    const url = new URL("/v1/internal/sales/referral-discount", base);
    const response = await fetch(url, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-api-key": this.config.get("SALES_CONTROL_KEY", { infer: true }),
      },
      body: JSON.stringify({ userId, floorBps }),
      signal: AbortSignal.timeout(15_000),
    });
    // 404 — эндпоинт ещё не задеплоен в commerce; повторим на следующем тике (курсор не двинется).
    if (!response.ok) throw new Error(`referral-discount responded ${response.status}`);
  }

  /**
   * Депозиты рефералов — ТОЛЬКО для истории/аналитики (referred_topups). Комиссию они НЕ создают:
   * источник комиссии — списания (см. syncUsageEvents/recordReferredSpend), где amountNano уже равен
   * real_funded (реально-оплаченной части по принципу «бесплатное тратится первым»). Здесь же мы просто
   * фиксируем факт пополнения реальными деньгами.
   */
  private async syncTopups(): Promise<void> {
    const after = await getSyncCursor(this.database, "topups");
    const rows = await this.fetchFeed("topups", `topups?after_id=${after}&limit=500`, topupSchema);
    if (!rows || rows.length === 0) return;
    for (const row of rows) {
      if (row.amountNano <= 0n) continue;
      await recordReferredDeposit(this.database, {
        commercePaymentId: row.paymentId,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt,
      });
    }
    await advanceSyncCursor(this.database, "topups", maxId(rows));
  }

  /**
   * Списания за API. amountNano здесь = real_funded (коммерция уже вычла бесплатную часть по
   * принципу «бесплатное тратится первым»), поэтому комиссия капает только с реальных денег.
   */
  private async syncUsageEvents(): Promise<void> {
    const after = await getSyncCursor(this.database, "usage_events");
    const rows = await this.fetchFeed("usage_events", `usage-events?after_id=${after}&limit=1000`, usageEventSchema);
    if (!rows || rows.length === 0) return;
    for (const row of rows) {
      if (row.amountNano <= 0n) continue;
      await recordReferredSpend(this.database, {
        commerceEventId: row.id,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        occurredAt: row.occurredAt,
      });
    }
    await advanceSyncCursor(this.database, "usage_events", maxId(rows));
  }

  private async fetchFeed<T>(feed: SyncFeed, pathAndQuery: string, schema: z.ZodType<T, z.ZodTypeDef, unknown>): Promise<T[] | null> {
    const base = this.config.get("COMMERCE_BASE_URL", { infer: true });
    const url = new URL(`/v1/internal/sales/${pathAndQuery}`, base);
    const response = await fetch(url, {
      headers: { "x-api-key": this.config.get("SALES_CONTROL_KEY", { infer: true }) },
      signal: AbortSignal.timeout(15_000),
    });
    if (response.status === 404) {
      // The commerce side of the feed may not be deployed yet — log once, retry next tick.
      if (!this.missingFeedLogged.has(feed)) {
        this.missingFeedLogged.add(feed);
        this.logger.debug(`commerce feed ${feed} is not available yet (404); will keep retrying`);
      }
      return null;
    }
    if (!response.ok) throw new Error(`commerce feed ${feed} responded ${response.status}`);
    this.missingFeedLogged.delete(feed);
    const body: unknown = await response.json();
    // Канонический формат фида коммерции (apps/api sales-feed.controller): { items: [...] }.
    const items = body !== null && typeof body === "object" && Array.isArray((body as { items?: unknown }).items)
      ? (body as { items: unknown[] }).items
      : body;
    if (!Array.isArray(items)) throw new Error(`commerce feed ${feed} returned an unexpected body shape`);
    return items.map((item) => schema.parse(item));
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

function maxId(rows: readonly { id: bigint }[]): bigint {
  return rows.reduce((maximum, row) => (row.id > maximum ? row.id : maximum), 0n);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "sync failed";
}
