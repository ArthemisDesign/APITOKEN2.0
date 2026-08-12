import { Inject, Injectable, Logger, OnApplicationShutdown, OnModuleInit } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  advanceSyncCursor,
  claimReferralDiscount,
  getSyncCursor,
  hasIncompletePartnerFundingEvidence,
  reconcilePartnerFundingEvidence,
  recordPaidFundingLot,
  recordPaymentReversalPage,
  recordReferredDeposit,
  recordReferredSpend,
  recordReferredSpendV2,
  reconcilePendingReferralEvents,
  reconcilePendingReferralUsageEventsV2,
  resolveReferralCode,
  upsertReferredUser,
  getReferredUserPartner,
  type ReferredSpendAttribution,
  type ReferredSpendV2Event,
  type SalesDatabase,
  type SyncFeed,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";

const POSTGRES_BIGINT_MAX = 9_223_372_036_854_775_807n;
const canonicalPostgresBigintStringSchema = z.string()
  .max(19)
  .regex(/^(0|[1-9]\d*)$/)
  .transform(BigInt)
  .refine((value) => value <= POSTGRES_BIGINT_MAX, "value exceeds PostgreSQL bigint");
const nanoStringSchema = canonicalPostgresBigintStringSchema;
const feedIdSchema = z.union([
  z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER).transform(BigInt),
  canonicalPostgresBigintStringSchema,
]);

const attributionSchema = z.object({
  id: feedIdSchema,
  userId: z.string().uuid(),
  code: z.string().min(1),
  createdAt: z.coerce.date(),
});

export const topupV2Schema = z.object({
  id: canonicalPostgresBigintStringSchema.refine((value) => value > 0n, "topup id must be positive"),
  paymentId: z.string().uuid(),
  userId: z.string().uuid(),
  amountNano: nanoStringSchema.refine((value) => value > 0n, "topup amount must be positive"),
  paidAt: z.string().datetime({ offset: true }).transform((value) => new Date(value)),
});

export const paymentReversalSchema = z.object({
  id: canonicalPostgresBigintStringSchema.refine(
    (value) => value > 0n,
    "payment reversal id must be positive",
  ),
  paymentId: z.string().uuid(),
  userId: z.string().uuid(),
  kind: z.enum(["refund", "dispute"]),
  amountNano: nanoStringSchema.refine(
    (value) => value > 0n,
    "payment reversal amount must be positive",
  ),
  reversedAt: z.string().datetime({ offset: true }).transform((value) => new Date(value)),
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
const nullableNanoSchema = nanoStringSchema.nullable().optional()
  .transform((value): bigint | null => value ?? null);
const nullableDigestSchema = z.string().min(1).nullable().optional()
  .transform((value): string | null => value ?? null);

// Фид эмитит три формы usage-строки (expand-only, неизвестные поля игнорируются):
//   scalar (в коде — "legacy") — ЖИВАЯ форма: комиссия считается от amountNano, который producer
//     уже сузил до real_funded_nano (деньги самого клиента). providerId информативен и может быть
//     как заполнен, так и null — он НЕ часть attribution и сам по себе комиссию не образует;
//     остальные attribution-поля и v2-lineage — null;
//   v1 (policy_v1) — complete B2C track authority, amountNano === paidFundedNano > 0;
//   v2 (release_v2) — pricingMode null, eligibility независима от pricing mode, полная release
//     lineage (official/charged/bonus/other nano, releaseGeneration, releaseDigest) не-null,
//     amountNano === exact paidFundedNano > 0.
// Частичная или смешанная форма — ошибка страницы ДО продвижения курсора (fail closed).
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
  officialNano: nullableNanoSchema,
  chargedNano: nullableNanoSchema,
  bonusFundedNano: nullableNanoSchema,
  otherFundedNano: nullableNanoSchema,
  releaseGeneration: nullableNanoSchema,
  releaseDigest: nullableDigestSchema,
  occurredAt: z.coerce.date(),
}).superRefine((row, context) => {
  // providerId намеренно вне кортежа: живой producer заполняет его на scalar-строке, где всё
  // остальное null. Его наличие не делает форму attribution-полной и не открывает путь к
  // v1/v2-комиссии — этот путь стережёт accountClass.
  const attribution = [
    row.accountClass,
    row.paidFundedNano,
    row.commissionEligible,
    row.snapshotDigest,
  ];
  const lineage = [
    row.officialNano,
    row.chargedNano,
    row.bonusFundedNano,
    row.otherFundedNano,
    row.releaseGeneration,
    row.releaseDigest,
  ];
  const attributionComplete = attribution.every((field) => field !== null);
  const attributionNull = attribution.every((field) => field === null);
  const lineageComplete = lineage.every((field) => field !== null);
  const lineageNull = lineage.every((field) => field === null);
  const fail = (message: string) => context.addIssue({ code: z.ZodIssueCode.custom, message });

  if (attributionNull && lineageNull && row.pricingMode === null) {
    return; // живая scalar-форма (и старая all-null legacy): комиссия от amountNano
  }
  if (!attributionComplete) return fail("usage attribution must be entirely null or complete");
  if (row.providerId === null) return fail("attributed usage must carry its provider");
  if (row.paidFundedNano! <= 0n || row.amountNano !== row.paidFundedNano) {
    return fail("usage amount must equal positive attributed paid funding");
  }
  if (row.pricingMode === "track") {
    // schema v1: lineage-поля обязаны отсутствовать — иначе это битый v2, а не v1.
    if (!lineageNull) fail("release-v2 lineage is incompatible with track pricing mode");
    return;
  }
  // schema v2: pricingMode всегда null; lineage обязана быть полной и согласованной.
  if (!lineageComplete) return fail("release-v2 usage lineage must be entirely null or complete");
  if (row.paidFundedNano! + row.bonusFundedNano! + row.otherFundedNano! !== row.chargedNano!
    || row.releaseGeneration! <= 0n) {
    fail("release-v2 funding buckets must sum to the charged amount");
  }
}).transform((row) => ({
  ...row,
  form: (row.accountClass === null ? "legacy"
    : row.pricingMode === "track" ? "v1"
    : "v2") as "legacy" | "v1" | "v2",
}));

export interface FeedPage<T extends { id: bigint }> {
  items: T[];
  nextCursor: bigint;
  sourceHead: bigint | null;
}

export type PartnerFeedHeads = {
  usageEvents: bigint;
  fundingLots: bigint;
  paymentReversals: bigint;
};

export class PartnerFeedNotReadyError extends Error {
  constructor(public readonly reasons: string[]) {
    super(`partner accounting feeds are not ready: ${reasons.join("; ")}`);
  }
}

@Injectable()
export class SyncService implements OnModuleInit, OnApplicationShutdown {
  private readonly logger = new Logger(SyncService.name);
  private stopped = false;
  private loop: Promise<void> | undefined;
  private stopSleep!: () => void;
  private readonly stopSignal = new Promise<void>((resolve) => { this.stopSleep = resolve; });
  private readonly missingFeedLogged = new Set<SyncFeed>();
  private readonly sourceHeads: PartnerFeedHeads = {
    usageEvents: 0n,
    fundingLots: 0n,
    paymentReversals: 0n,
  };
  private syncWork: Promise<void> = Promise.resolve();

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
        await this.syncOnce();
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
          // Side effects run only for this attribution's owner. A different owner must not consume
          // somebody else's one-time link. Both the claim and marker replay are idempotent.
          const ownerPartnerId = won ? resolved.partnerId : await getReferredUserPartner(this.database, row.userId);
          if (ownerPartnerId === resolved.partnerId && resolved.discountLinkId) {
            // One UPDATE claims the legacy link for its first user. The winner receives its marker
            // on first processing or retry; a loser receives zero. Regular referral codes have no
            // discountLinkId. The marker is audit metadata and never changes pricing.
            const { discountBps } = await claimReferralDiscount(this.database, row.code, row.userId);
            if (discountBps > 0) {
              await this.replayReferralMarker(row.userId, discountBps);
            }
          }
        }
        lastOk = row.id;
      }
    } finally {
      if (lastOk !== after) await advanceSyncCursor(this.database, "attributions", lastOk);
    }
  }

  /** Testable single-iteration boundary; production run() uses the same ordered pipeline. */
  async syncOnce(): Promise<void> {
    const previous = this.syncWork;
    let release!: () => void;
    this.syncWork = new Promise<void>((resolve) => { release = resolve; });
    await previous;
    try {
      await this.syncPipelineOnce();
    } finally {
      release();
    }
  }

  private async syncPipelineOnce(): Promise<void> {
    await this.syncAttributions();
    await this.syncTopups();
    await this.syncUsageEvents();
    const replayed = await reconcilePendingReferralEvents(this.database);
    if (replayed > 0) this.logger.log(`reconciled ${replayed} buffered referral events`);
    const replayedV2 = await reconcilePendingReferralUsageEventsV2(this.database);
    if (replayedV2 > 0) this.logger.log(`reconciled ${replayedV2} buffered release-v2 usage events`);
    await this.syncFundingLots();
    const funding = await reconcilePartnerFundingEvidence(this.database);
    if (funding.completed > 0) {
      this.logger.log(`completed funding evidence for ${funding.completed} usage events`);
    }
    await this.syncPaymentReversals();
  }

  /** Last successfully parsed source watermarks. Payouts also refresh these through drainForPayout. */
  getPartnerFeedHeads(): PartnerFeedHeads {
    return { ...this.sourceHeads };
  }

  /**
   * Payout-time proof, not a passive health read. Repeatedly drains the three causal feeds until
   * each returns a no-advance page, then leaves exact source heads available to the DB gate.
   */
  async drainForPayout(maxPasses = 100): Promise<PartnerFeedHeads> {
    return this.withPayoutFence((heads) => Promise.resolve(heads), maxPasses);
  }

  /**
   * Serializes the whole payout visibility fence with the background consumer. The callback runs
   * before the mutex is released, so callers can take the database money lock, re-probe Commerce,
   * and sign without an in-process sync iteration changing the local proof underneath them.
   */
  async withPayoutFence<T>(
    callback: (heads: PartnerFeedHeads) => Promise<T>,
    maxPasses = 100,
  ): Promise<T> {
    const previous = this.syncWork;
    let release!: () => void;
    this.syncWork = new Promise<void>((resolve) => { release = resolve; });
    await previous;
    try {
      const heads = await this.drainForPayoutExclusive(maxPasses);
      return await callback(heads);
    } finally {
      release();
    }
  }

  private async drainForPayoutExclusive(maxPasses: number): Promise<PartnerFeedHeads> {
    for (let pass = 0; pass < maxPasses; pass += 1) {
      const before = await Promise.all([
        getSyncCursor(this.database, "usage_events"),
        getSyncCursor(this.database, "topup_funding_lots"),
        getSyncCursor(this.database, "payment_reversals"),
      ]);
      // Funding-lot snapshots reference the durable referred_topups analytics row. Keep that
      // ordinary consumer in the payout drain even though only the independent lot cursor gates.
      await this.syncTopups();
      const usageAtHead = await this.syncUsageEvents();
      const fundingAtHead = await this.syncFundingLots();
      await reconcilePartnerFundingEvidence(this.database);
      await this.syncPaymentReversals();
      const after = await Promise.all([
        getSyncCursor(this.database, "usage_events"),
        getSyncCursor(this.database, "topup_funding_lots"),
        getSyncCursor(this.database, "payment_reversals"),
      ]);
      const usageAtSourceHead = usageAtHead === true && after[0] === this.sourceHeads.usageEvents;
      const fundingAtSourceHead = fundingAtHead === true && after[1] === this.sourceHeads.fundingLots;
      const reversalAtHead = after[2] === before[2]
        && after[2] === this.sourceHeads.paymentReversals;
      if (usageAtSourceHead && fundingAtSourceHead && reversalAtHead) {
        return this.getPartnerFeedHeads();
      }
      const advanced = after.some((cursor, index) => cursor > before[index]!);
      if (!advanced) {
        throw new PartnerFeedNotReadyError([
          "Commerce has committed partner-accounting rows that are not visible to the feed consumer yet",
        ]);
      }
    }
    throw new PartnerFeedNotReadyError([
      `partner accounting backlog exceeded the ${maxPasses}-page payout drain limit`,
    ]);
  }

  /**
   * Final read-only visibility probe. Callers invoke it while holding both `withPayoutFence` and
   * the Sales accounting advisory lock. A remote commit before these reads changes `sourceHead`
   * and blocks the payout; a commit after them is ordered after the payout and becomes explicit
   * signed debt when its reversal is consumed.
   */
  async probePayoutSourceHeads(expected: PartnerFeedHeads): Promise<PartnerFeedHeads> {
    const [usageCursor, fundingCursor, reversalCursor] = await Promise.all([
      getSyncCursor(this.database, "usage_events"),
      getSyncCursor(this.database, "topup_funding_lots"),
      getSyncCursor(this.database, "payment_reversals"),
    ]);
    const [usage, funding, reversals] = await Promise.all([
      this.fetchFeed(
        "usage_events",
        `usage-events?after_id=${usageCursor}&limit=1000`,
        usageEventSchema,
      ),
      this.fetchFeed(
        "topup_funding_lots",
        `topups-v2?after_id=${fundingCursor}&limit=500`,
        topupV2Schema,
        fundingCursor,
      ),
      this.fetchFeed(
        "payment_reversals",
        `payment-reversals?after_id=${reversalCursor}&limit=500`,
        paymentReversalSchema,
        reversalCursor,
      ),
    ]);
    const reasons: string[] = [];
    const check = (
      label: string,
      cursor: bigint,
      expectedHead: bigint,
      page: FeedPage<{ id: bigint }> | null,
    ): bigint => {
      if (page === null) {
        reasons.push(`${label} feed is unavailable`);
        return expectedHead;
      }
      if (page.sourceHead === null) {
        reasons.push(`${label} feed omitted its committed source head`);
        return expectedHead;
      }
      if (page.items.length !== 0 || page.nextCursor !== cursor || page.sourceHead !== cursor) {
        reasons.push(`${label} feed advanced after the payout drain`);
      }
      if (page.sourceHead !== expectedHead) {
        reasons.push(`${label} source head changed during the payout fence`);
      }
      return page.sourceHead;
    };
    const observed = {
      usageEvents: check("usage", usageCursor, expected.usageEvents, usage),
      fundingLots: check("funding-lot", fundingCursor, expected.fundingLots, funding),
      paymentReversals: check("payment-reversal", reversalCursor, expected.paymentReversals, reversals),
    };
    if (reasons.length > 0) throw new PartnerFeedNotReadyError([...new Set(reasons)]);
    return observed;
  }

  /** Replays the legacy B2C referral marker into Commerce. It does not change pricing. */
  private async replayReferralMarker(userId: string, floorBps: number): Promise<void> {
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
    const after = await getSyncCursor(this.database, "topups_v2");
    const page = await this.fetchFeed(
      "topups_v2",
      `topups-v2?after_id=${after}&limit=500`,
      topupV2Schema,
      after,
    );
    if (!page) return;
    for (const row of page.items) {
      await recordReferredDeposit(this.database, {
        commercePaymentId: row.paymentId,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt,
      });
    }
    if (page.nextCursor > after) {
      await advanceSyncCursor(this.database, "topups_v2", page.nextCursor);
    }
  }

  /**
   * Independent replay of the same commit-ordered topup stream. It starts at zero and snapshots
   * immutable funding lots without resetting or coupling the partner-visible topups-v2 cursor.
   */
  private async syncFundingLots(): Promise<boolean | undefined> {
    const after = await getSyncCursor(this.database, "topup_funding_lots");
    const page = await this.fetchFeed(
      "topup_funding_lots",
      `topups-v2?after_id=${after}&limit=500`,
      topupV2Schema,
      after,
    );
    if (!page) return undefined;
    if (page.sourceHead === null) {
      throw new Error("commerce feed topup_funding_lots did not expose its committed source head");
    }
    this.sourceHeads.fundingLots = page.sourceHead;
    for (const row of page.items) {
      await recordPaidFundingLot(this.database, {
        commerceTopupId: row.id,
        commercePaymentId: row.paymentId,
        commerceUserId: row.userId,
        amountNano: row.amountNano,
        paidAt: row.paidAt,
      });
    }
    if (page.nextCursor > after) {
      await advanceSyncCursor(this.database, "topup_funding_lots", page.nextCursor);
    }
    return page.nextCursor === page.sourceHead;
  }

  /**
   * Reversals are admitted only after both causal source feeds have reached the reversal page and
   * every locally stored usage/commission row has complete funding evidence. The page writer owns
   * the cursor transaction; no generic post-write cursor advance is allowed here.
   */
  private async syncPaymentReversals(): Promise<void> {
    const after = await getSyncCursor(this.database, "payment_reversals");
    const page = await this.fetchFeed(
      "payment_reversals",
      `payment-reversals?after_id=${after}&limit=500`,
      paymentReversalSchema,
      after,
    );
    if (!page) return;
    if (page.sourceHead === null) {
      throw new Error("commerce feed payment_reversals did not expose its committed source head");
    }
    this.sourceHeads.paymentReversals = page.sourceHead;
    if (page.nextCursor === after) return;
    // Feed identifiers live in independent PostgreSQL sequences and must never be compared. These
    // post-page requests prove both older causal streams are drained through a visibility cutoff
    // no earlier than the one that exposed this reversal.
    const usageAtHead = await this.syncUsageEvents();
    const fundingLotsAtHead = await this.syncFundingLots();
    if (usageAtHead !== true || fundingLotsAtHead !== true) {
      this.logger.warn(`payment reversal page ${page.nextCursor} waits for causal source feed heads`);
      return;
    }
    await reconcilePartnerFundingEvidence(this.database);
    if (await hasIncompletePartnerFundingEvidence(this.database)) {
      this.logger.warn(`payment reversal page ${page.nextCursor} waits for complete funding evidence`);
      return;
    }
    await recordPaymentReversalPage(this.database, page.items.map((row) => ({
      commerceReversalId: row.id,
      commercePaymentId: row.paymentId,
      commerceUserId: row.userId,
      kind: row.kind,
      amountNano: row.amountNano,
      reversedAt: row.reversedAt,
    })), page.nextCursor);
  }

  /**
   * Маршрутизация по форме строки: schema v2 (release_v2, pricingMode null + полная lineage) —
   * в recordReferredSpendV2 (basis = exact paidFundedNano; bonus-funded часть НИКОГДА не
   * комиссионируется); schema v1 (B2C track) и legacy all-null — в recordReferredSpend.
   * Одно событие обрабатывается ровно одним writer'ом; курсор продвигается только когда вся
   * страница обработана (at-least-once).
   */
  private async syncUsageEvents(): Promise<boolean | undefined> {
    const after = await getSyncCursor(this.database, "usage_events");
    const page = await this.fetchFeed("usage_events", `usage-events?after_id=${after}&limit=1000`, usageEventSchema);
    if (!page) return undefined;
    if (page.sourceHead === null) {
      throw new Error("commerce feed usage_events did not expose its committed source head");
    }
    this.sourceHeads.usageEvents = page.sourceHead;
    for (const row of page.items) {
      if (row.form === "v2") {
        await recordReferredSpendV2(this.database, toReferredSpendV2Event(row));
        continue;
      }
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
    return page.nextCursor === page.sourceHead;
  }

  private async fetchFeed<T extends { id: bigint }>(
    feed: SyncFeed,
    pathAndQuery: string,
    schema: z.ZodType<T, z.ZodTypeDef, unknown>,
    canonicalAfter?: bigint,
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
    const body = await response.json();
    return canonicalAfter !== undefined
      ? parseCanonicalFeedPage(
        body,
        schema,
        feed,
        canonicalAfter,
        feed === "topup_funding_lots" || feed === "payment_reversals",
      )
      : parseFeedPage(body, schema, feed);
  }

  private async sleep(milliseconds: number): Promise<void> {
    await Promise.race([new Promise((resolve) => setTimeout(resolve, milliseconds)), this.stopSignal]);
  }
}

function toReferredSpendAttribution(
  row: z.infer<typeof usageEventSchema>,
): ReferredSpendAttribution | null {
  if (row.accountClass === null) return null;
  // usageEventSchema has already enforced all-or-none and the exact literal authority. Keep this
  // guard local so future schema edits cannot accidentally turn a partial payload into commission.
  if (
    row.providerId === null
    || row.accountClass !== "b2c"
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

function toReferredSpendV2Event(row: z.infer<typeof usageEventSchema>): ReferredSpendV2Event {
  // usageEventSchema уже гарантировал полную и согласованную v2-форму; локальный guard —
  // защита от будущих правок схемы, чтобы частичный payload никогда не стал комиссией.
  if (
    row.form !== "v2"
    || row.accountClass !== "b2c"
    || row.commissionEligible !== true
    || row.paidFundedNano === null
    || row.paidFundedNano <= 0n
    || row.officialNano === null
    || row.chargedNano === null
    || row.bonusFundedNano === null
    || row.otherFundedNano === null
    || row.releaseGeneration === null
    || row.releaseDigest === null
    || row.snapshotDigest === null
    || row.providerId === null
  ) throw new Error("validated release-v2 usage event became incomplete");
  return {
    commerceEventId: row.id,
    commerceUserId: row.userId,
    providerId: row.providerId,
    accountClass: row.accountClass,
    officialNano: row.officialNano,
    chargedNano: row.chargedNano,
    paidFundedNano: row.paidFundedNano,
    bonusFundedNano: row.bonusFundedNano,
    otherFundedNano: row.otherFundedNano,
    commissionEligible: row.commissionEligible,
    releaseGeneration: row.releaseGeneration,
    releaseDigest: row.releaseDigest,
    snapshotDigest: row.snapshotDigest,
    occurredAt: row.occurredAt,
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
  const rawSourceHead = isObject ? (body as { sourceHead?: unknown }).sourceHead : undefined;
  const sourceHead = rawSourceHead === undefined ? null : feedIdSchema.parse(rawSourceHead);
  if (sourceHead !== null && sourceHead < nextCursor) {
    throw new Error(`commerce feed ${feed} returned a source head behind its cursor`);
  }
  return { items, nextCursor, sourceHead };
}

/** Producer-first feeds have no pre-page array form; require their explicit source watermark. */
export function parseCanonicalFeedPage<T extends { id: bigint }>(
  body: unknown,
  schema: z.ZodType<T, z.ZodTypeDef, unknown>,
  feed: SyncFeed,
  afterExclusive?: bigint,
  requireSourceHead = false,
): FeedPage<T> {
  if (body === null || typeof body !== "object" || Array.isArray(body)) {
    throw new Error(`commerce feed ${feed} returned an unexpected body shape`);
  }
  const candidate = body as { items?: unknown; nextCursor?: unknown; sourceHead?: unknown };
  if (!Array.isArray(candidate.items)
      || candidate.nextCursor === undefined
      || (requireSourceHead && candidate.sourceHead === undefined)) {
    throw new Error(`commerce feed ${feed} returned an unexpected body shape`);
  }
  const items = candidate.items.map((item) => schema.parse(item));
  if (items.some((item, index) => (
    (afterExclusive !== undefined && item.id <= afterExclusive)
    || (index > 0 && item.id <= items[index - 1]!.id)
  ))) {
    throw new Error(`commerce feed ${feed} returned non-monotonic items`);
  }
  const itemCursor = maxId(items);
  const nextCursor = canonicalPostgresBigintStringSchema.parse(candidate.nextCursor);
  if (nextCursor < itemCursor || (afterExclusive !== undefined && nextCursor < afterExclusive)) {
    throw new Error(`commerce feed ${feed} returned a cursor behind its items`);
  }
  const sourceHead = candidate.sourceHead === undefined
    ? null
    : canonicalPostgresBigintStringSchema.parse(candidate.sourceHead);
  if (sourceHead !== null && sourceHead < nextCursor) {
    throw new Error(`commerce feed ${feed} returned a source head behind its cursor`);
  }
  return { items, nextCursor, sourceHead };
}

function maxId(rows: readonly { id: bigint }[]): bigint {
  return rows.reduce((maximum, row) => (row.id > maximum ? row.id : maximum), 0n);
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "sync failed";
}
