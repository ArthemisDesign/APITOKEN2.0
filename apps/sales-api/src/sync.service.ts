import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  advanceSyncCursor,
  claimReferralDiscount,
  getSyncCursor,
  recordReferredDeposit,
  recordReferredSpend,
  reconcilePendingReferralEvents,
  resolveReferralCode,
  upsertReferredUser,
  getReferredUserPartner,
  type ReferredSpendAttribution,
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

const nullableProviderIdSchema = z.string().min(1).nullable().optional()
  .transform((value): string | null => value ?? null);
const nullableAccountClassSchema = z.literal("b2c").nullable().optional()
  .transform((value): "b2c" | null => value ?? null);
const nullablePricingModeSchema = z.literal("track").nullable().optional()
  .transform((value): "track" | null => value ?? null);
const nullablePaidFundedSchema = nanoStringSchema.nullable().optional()
  .transform((value): bigint | null => value ?? null);
const nullableCommissionEligibleSchema = z.literal(true).nullable().optional()
  .transform((value): true | null => value ?? null);
const nullableSnapshotDigestSchema = z.string().min(1).nullable().optional()
  .transform((value): string | null => value ?? null);

// Old producer payloads omit all attribution fields and remain the temporary legacy free-first
// path. Any attributed payload must be the complete B2C track authority; partial or ineligible
// shapes fail the page before cursor advancement.
export const usageEventSchema = z.object({
  id: feedIdSchema,
  userId: z.string().uuid(),
  amountNano: nanoStringSchema,
  providerId: nullableProviderIdSchema,
  accountClass: nullableAccountClassSchema,
  pricingMode: nullablePricingModeSchema,
  paidFundedNano: nullablePaidFundedSchema,
  commissionEligible: nullableCommissionEligibleSchema,
  snapshotDigest: nullableSnapshotDigestSchema,
  occurredAt: z.coerce.date(),
}).superRefine((row, context) => {
  const fields = [
    row.providerId,
    row.accountClass,
    row.pricingMode,
    row.paidFundedNano,
    row.commissionEligible,
    row.snapshotDigest,
  ];
  if (fields.every((field) => field === null)) return;
  if (fields.some((field) => field === null)) {
    context.addIssue({
      code: z.ZodIssueCode.custom,
      message: "usage attribution must be entirely null or complete",
    });
    return;
  }
  if (row.paidFundedNano! <= 0n || row.amountNano !== row.paidFundedNano) {
    context.addIssue({
      code: z.ZodIssueCode.custom,
      message: "usage amount must equal positive attributed paid funding",
    });
  }
});

export interface FeedPage<T extends { id: bigint }> {
  items: T[];
  nextCursor: bigint;
}

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
        // Догоняем события, пришедшие раньше атрибуции их юзера (буфер pending_referral_events).
        const replayed = await reconcilePendingReferralEvents(this.database);
        if (replayed > 0) this.logger.log(`reconciled ${replayed} buffered referral events`);
      } catch (error) {
        this.logger.error(`sync iteration failed: ${message(error)}`);
      }
      await this.sleep(intervalMs);
    }
  }

  private async syncAttributions(): Promise<void> {
    const after = await getSyncCursor(this.database, "attributions");
    const page = await this.fetchFeed("attributions", `attributions?after_id=${after}&limit=500`, attributionSchema);
    if (!page || page.items.length === 0) return;
    const rows = page.items;
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
          // Побочные эффекты запускаем, если юзер закреплён именно за ЭТИМ партнёром — либо мы только что
          // выиграли атрибуцию (won), либо он уже был закреплён за ним же (идемпотентно к крашу/ретраю
          // между upsert и consume). Если юзер закреплён за ДРУГИМ партнёром — не трогаем (не жжём чужую
          // одноразовую ссылку и не даём незаслуженный floor). consume/apply идемпотентны.
          const ownerPartnerId = won ? resolved.partnerId : await getReferredUserPartner(this.database, row.userId);
          if (ownerPartnerId === resolved.partnerId && resolved.discountLinkId) {
            // Скидочная ссылка: АТОМАРНО закрепляем её за этим юзером (claim = consume+resolve одним
            // UPDATE, идемпотентно по (code,user)). Победитель получает floor — либо впервые, либо как
            // backfill, если синхронное применение при регистрации упало; проигравший (ссылка уже
            // погашена другим) получает строго 0 и floor НЕ применяется. Это закрывает и гонку
            // «одна ссылка → скидка нескольким», и потерю floor при сбое apply. Обычный реф-код
            // (без discountLinkId) сюда не заходит — floor 0, атрибуция/комиссия уже сделаны выше.
            const { discountBps } = await claimReferralDiscount(this.database, row.code, row.userId);
            if (discountBps > 0) {
              await this.applyReferralDiscount(row.userId, discountBps);
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
    const page = await this.fetchFeed("topups", `topups?after_id=${after}&limit=500`, topupSchema);
    if (!page) return;
    for (const row of page.items) {
      if (row.amountNano <= 0n) continue;
      await recordReferredDeposit(this.database, {
        commercePaymentId: row.paymentId,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt,
      });
    }
    if (page.nextCursor > after) await advanceSyncCursor(this.database, "topups", page.nextCursor);
  }

  /**
   * New rows carry immutable B2C track attribution and use exact paid funding as commission basis.
   * All-null/omitted attribution is the bounded rolling-compatibility path for old ledger rows,
   * where amountNano remains the commerce free-first projection.
   */
  private async syncUsageEvents(): Promise<void> {
    const after = await getSyncCursor(this.database, "usage_events");
    const page = await this.fetchFeed("usage_events", `usage-events?after_id=${after}&limit=1000`, usageEventSchema);
    if (!page) return;
    for (const row of page.items) {
      if (row.amountNano <= 0n) continue;
      await recordReferredSpend(this.database, {
        commerceEventId: row.id,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        attribution: toReferredSpendAttribution(row),
        occurredAt: row.occurredAt,
      });
    }
    if (page.nextCursor > after) await advanceSyncCursor(this.database, "usage_events", page.nextCursor);
  }

  private async fetchFeed<T extends { id: bigint }>(
    feed: SyncFeed,
    pathAndQuery: string,
    schema: z.ZodType<T, z.ZodTypeDef, unknown>,
  ): Promise<FeedPage<T> | null> {
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
    return parseFeedPage(await response.json(), schema, feed);
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

function toReferredSpendAttribution(
  row: z.infer<typeof usageEventSchema>,
): ReferredSpendAttribution | null {
  if (row.providerId === null) return null;
  // usageEventSchema has already enforced all-or-none and the exact literal authority. Keep this
  // guard local so future schema edits cannot accidentally turn a partial payload into commission.
  if (
    row.accountClass !== "b2c"
    || row.pricingMode !== "track"
    || row.paidFundedNano === null
    || row.commissionEligible !== true
    || row.snapshotDigest === null
  ) throw new Error("validated usage attribution became incomplete");
  return {
    providerId: row.providerId,
    accountClass: row.accountClass,
    pricingMode: row.pricingMode,
    paidFundedNano: row.paidFundedNano,
    commissionEligible: row.commissionEligible,
    snapshotDigest: row.snapshotDigest,
  };
}

/** Accepts the canonical page object and the pre-page legacy array during rolling deployments. */
export function parseFeedPage<T extends { id: bigint }>(
  body: unknown,
  schema: z.ZodType<T, z.ZodTypeDef, unknown>,
  feed: SyncFeed,
): FeedPage<T> {
  const isObject = body !== null && typeof body === "object" && !Array.isArray(body);
  const rawItems = isObject && Array.isArray((body as { items?: unknown }).items)
    ? (body as { items: unknown[] }).items
    : body;
  if (!Array.isArray(rawItems)) throw new Error(`commerce feed ${feed} returned an unexpected body shape`);
  const items = rawItems.map((item) => schema.parse(item));
  const itemCursor = maxId(items);
  const rawNextCursor = isObject ? (body as { nextCursor?: unknown }).nextCursor : undefined;
  const nextCursor = rawNextCursor === undefined ? itemCursor : feedIdSchema.parse(rawNextCursor);
  if (nextCursor < itemCursor) {
    throw new Error(`commerce feed ${feed} returned a cursor behind its items`);
  }
  return { items, nextCursor };
}

function maxId(rows: readonly { id: bigint }[]): bigint {
  return rows.reduce((maximum, row) => (row.id > maximum ? row.id : maximum), 0n);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "sync failed";
}
