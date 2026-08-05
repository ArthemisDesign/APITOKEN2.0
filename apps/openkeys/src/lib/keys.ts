import "server-only";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import type { EngineAccount, EngineUsage } from "@claude-api/contracts";
import { openkeysBatches, openkeysIssuanceJobs, openkeysKeys } from "@claude-api/openkeys-db";
import { and, asc, count, desc, eq, ilike, inArray, isNotNull, isNull, lt, or, sql } from "drizzle-orm";
import { apiTypeOf, type ApiType } from "./api-product";
import {
  adminUsagePercent,
  classifyAdminUsage,
  matchesAdminUsage,
  type AdminUsageFilter,
  type AdminUsageState,
} from "./admin-directory";
import { loadConfig } from "./config";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";
import {
  balanceToOfficialNano,
  formatUsd,
  officialBalanceBreakdown,
  officialRemainingNano,
} from "./money";
import {
  assertNoOpenKeysPricingOverride,
  assertOfficialEngineAccount,
  OFFICIAL_ONE_TO_ONE_CONTRACT,
  OFFICIAL_ONE_TO_ONE_MULT_BP,
  provisionOfficialOpenKeysCredential,
  resolveOpenKeysPricingAuthority,
} from "./openkeys-pricing";
import { openSecret, sealSecret } from "./secret-box";
import { USAGE_REPORT_CACHE_TTL_MS } from "./usage-refresh-timing";

export const MAX_BATCH_QUANTITY = 100;

export interface IssueBatchInput {
  faceValueNano: bigint;
  quantity: number;
  label: string | null;
  note: string | null;
  apiType: ApiType;
  createdBy: string;
}

export interface IssuedKey {
  /** Полный секрет. Лежит на складе в шифрованном виде, пока ключ не выдан. */
  secret: string;
  viewToken: string;
  viewUrl: string;
  keyMasked: string;
}

export class BatchIssuanceError extends Error {
  constructor(readonly issuedCount: number, options?: ErrorOptions) {
    super(`Выпуск прерван после ${issuedCount} из запрошенных ключей`, options);
    this.name = "BatchIssuanceError";
  }
}

const UNFINISHED_ISSUANCE = ["pending", "account_created", "credited", "key_issued"] as const;

/** Disable accounts left mid-flight by a crashed process; safe to call from every admin read. */
export async function reconcileIssuanceJobs(staleBefore = new Date(Date.now() - 60_000)): Promise<number> {
  const { db } = getDatabase();
  const jobs = await db
    .select()
    .from(openkeysIssuanceJobs)
    .where(and(inArray(openkeysIssuanceJobs.status, [...UNFINISHED_ISSUANCE]), lt(openkeysIssuanceJobs.updatedAt, staleBefore)))
    .limit(100);
  const engine = getEngineClient();
  let reconciled = 0;
  for (const job of jobs) {
    try {
      if (job.engineKeyId) {
        const [persisted] = await db
          .select({ id: openkeysKeys.id })
          .from(openkeysKeys)
          .where(eq(openkeysKeys.engineKeyId, job.engineKeyId))
          .limit(1);
        if (persisted) {
          await db.update(openkeysIssuanceJobs).set({ status: "completed", updatedAt: new Date(), lastError: null })
            .where(eq(openkeysIssuanceJobs.id, job.id));
          reconciled += 1;
          continue;
        }
      }
      if (job.engineAccountId) await engine.setAccountStatus(job.engineAccountId, "disabled");
      await db
        .update(openkeysIssuanceJobs)
        .set({ status: "compensated", updatedAt: new Date(), lastError: "reconciled after interrupted issuance" })
        .where(and(eq(openkeysIssuanceJobs.id, job.id), inArray(openkeysIssuanceJobs.status, [...UNFINISHED_ISSUANCE])));
      reconciled += 1;
    } catch (error) {
      console.error("openkeys issuance reconciliation failed", { jobId: job.id, accountId: job.engineAccountId });
    }
  }
  return reconciled;
}

const RECONCILER_STATE = Symbol.for("openkeys.issuance-reconciler");

/** Keep compensation independent of an operator opening the admin page after a crash. */
export function startIssuanceReconciler(): void {
  const state = globalThis as typeof globalThis & { [RECONCILER_STATE]?: NodeJS.Timeout };
  if (state[RECONCILER_STATE]) return;
  const run = () => void reconcileIssuanceJobs().catch((error) => {
    console.error("openkeys issuance reconciliation cycle failed", {
      error: error instanceof Error ? error.name : "UnknownError",
    });
  });
  run();
  const timer = setInterval(run, 60_000);
  timer.unref();
  state[RECONCILER_STATE] = timer;
}

function maskKey(secret: string): string {
  return `${secret.slice(0, 12)}…${secret.slice(-4)}`;
}

function secretContext(input: { id: string; batchId: string; viewToken: string; engineAccountId: string }): string {
  return `openkeys:v2:${input.id}:${input.batchId}:${input.viewToken}:${input.engineAccountId}`;
}

/**
 * Отпечаток ключа. Переживает выдачу, когда шифротекст уже стёрт: по хешу мы
 * узнаем ключ, если покупатель придёт продлевать, и сможем привязать его к
 * обычному аккаунту на основном сайте.
 */
export function keyDigest(secret: string): string {
  return createHash("sha256").update(secret).digest("hex");
}

/**
 * Один ключ = один аккаунт движка. Так баланс принадлежит ровно этому ключу, и
 * страница расхода может показывать остаток без всякой авторизации.
 */
export async function issueBatch(input: IssueBatchInput): Promise<{ batchId: string; keys: IssuedKey[] }> {
  assertNoOpenKeysPricingOverride(input);
  if (!Number.isInteger(input.quantity) || input.quantity < 1 || input.quantity > MAX_BATCH_QUANTITY) {
    throw new Error(`Количество ключей должно быть от 1 до ${MAX_BATCH_QUANTITY}`);
  }
  if (input.faceValueNano <= 0n) throw new Error("Номинал должен быть положительным");

  await reconcileIssuanceJobs();

  const config = loadConfig();
  const { db } = getDatabase();
  const engine = getEngineClient();
  // Read and validate the already-reviewed product authority before creating a batch or account.
  // OpenKeys never mutates global catalogs/switches and cannot issue while they are absent/drifted.
  const releaseContext = await engine.getPricingReleaseProvisioningContextV2();
  const pricingAuthority = releaseContext === null
    ? await resolveOpenKeysPricingAuthority(engine)
    : null;

  const [batch] = await db
    .insert(openkeysBatches)
    .values({
      label: input.label,
      faceValueNano: input.faceValueNano,
      multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
      pricingContract: OFFICIAL_ONE_TO_ONE_CONTRACT,
      quantity: input.quantity,
      note: input.note,
      apiType: input.apiType,
      createdBy: input.createdBy,
    })
    .returning({ id: openkeysBatches.id });

  if (!batch) throw new Error("Не удалось создать партию");

  const issued: IssuedKey[] = [];
  for (let index = 0; index < input.quantity; index += 1) {
    const id = randomUUID();
    const viewToken = randomBytes(16).toString("base64url");
    let accountId: string | null = null;
    const [job] = await db
      .insert(openkeysIssuanceJobs)
      .values({ batchId: batch.id, itemIndex: index })
      .returning({ id: openkeysIssuanceJobs.id });
    if (!job) throw new Error("Не удалось создать журнал выпуска");
    try {
      const account = await engine.createAccount({
        handle: input.apiType === "anthropic"
          ? `openkeys-${viewToken.slice(0, 16)}`
          : `openkeys-openai-${viewToken.slice(0, 16)}`,
        multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
      });
      accountId = account.account;
      assertOfficialEngineAccount(account);
      await db.update(openkeysIssuanceJobs).set({
        status: "account_created", engineAccountId: account.account, updatedAt: new Date(),
      }).where(eq(openkeysIssuanceJobs.id, job.id));

      // Pre-cutover policy ACK remains policy-before-credit. Post-cutover the helper credits the
      // exact face value, normalizes funding and completes release-v2 policy/extension readback;
      // the usable secret is issued last in both paths.
      const key = await provisionOfficialOpenKeysCredential(engine, {
        accountId: account.account,
        authority: pricingAuthority,
        releaseRequired: releaseContext !== null,
        faceValueNano: input.faceValueNano,
        creditReference: `openkeys:${batch.id}:${index}`,
        keyLabel: input.apiType === "anthropic"
          ? `openkeys ${viewToken.slice(0, 8)}`
          : `openkeys openai ${viewToken.slice(0, 8)}`,
        onCredited: async () => {
          await db.update(openkeysIssuanceJobs).set({ status: "credited", updatedAt: new Date() })
            .where(eq(openkeysIssuanceJobs.id, job.id));
        },
      });
      await db.update(openkeysIssuanceJobs).set({
        status: "key_issued", engineKeyId: key.key_id, updatedAt: new Date(),
      }).where(eq(openkeysIssuanceJobs.id, job.id));
      const sealed = sealSecret(
        key.key,
        secretContext({ id, batchId: batch.id, viewToken, engineAccountId: account.account }),
      );

      await db.insert(openkeysKeys).values({
        id,
        batchId: batch.id,
        viewToken,
        engineAccountId: account.account,
        engineKeyId: key.key_id,
        keyMasked: maskKey(key.key),
        keySha256: keyDigest(key.key),
        secretCiphertext: sealed.ciphertext,
        secretNonce: sealed.nonce,
        secretVersion: 2,
        secretKeyId: sealed.keyId,
        faceValueNano: input.faceValueNano,
        multBp: OFFICIAL_ONE_TO_ONE_MULT_BP,
        pricingContract: OFFICIAL_ONE_TO_ONE_CONTRACT,
      });
      issued.push({
        secret: key.key,
        viewToken,
        viewUrl: `${config.publicBaseUrl}/profile/${viewToken}`,
        keyMasked: maskKey(key.key),
      });
      try {
        await db.update(openkeysIssuanceJobs).set({ status: "completed", updatedAt: new Date() })
          .where(eq(openkeysIssuanceJobs.id, job.id));
      } catch {
        // The durable key row is authoritative; reconciliation will recognize it and complete the job.
        console.error("openkeys issuance journal completion failed", { jobId: job.id, batchId: batch.id, index });
      }
    } catch (error) {
      // Never leave funded credentials usable when their secret/local mapping was not durably stored.
      let compensated = accountId === null;
      if (accountId) {
        try {
          await engine.setAccountStatus(accountId, "disabled");
          compensated = true;
        } catch {
          console.error("openkeys issuance compensation failed", { accountId, batchId: batch.id, index });
        }
      }
      try {
        await db.update(openkeysIssuanceJobs).set({
          status: compensated ? "compensated" : undefined,
          updatedAt: new Date(),
          lastError: error instanceof Error ? error.message.slice(0, 500) : "issuance failed",
        }).where(eq(openkeysIssuanceJobs.id, job.id));
      } catch {
        console.error("openkeys issuance journal failure update failed", { jobId: job.id, batchId: batch.id, index });
      }
      if (issued.length > 0) {
        await db.update(openkeysBatches).set({ quantity: issued.length }).where(eq(openkeysBatches.id, batch.id));
      }
      throw new BatchIssuanceError(issued.length, { cause: error });
    }
  }

  return { batchId: batch.id, keys: issued };
}

export interface KeyUsageView {
  viewToken: string;
  keyMasked: string;
  status: "active" | "disabled";
  createdAt: string;
  faceValueNano: string;
  multBp: number;
  pricingContract: "legacy" | "official_1_to_1";
  apiType: ApiType;
  balanceNano: string;
  reservedNano: string;
  spentNano: string;
  /** Остаток и расход, пересчитанные в официальный прайс выбранного API. */
  officialAvailableNano: string;
  officialReservedNano: string;
  officialRemainingNano: string;
  officialSpentNano: string;
  /** Полная статистика движка за окно — та же, что рисует дашборд. */
  usage: EngineUsage | null;
}

/**
 * Всё, что нужно странице расхода. Статистику берём тем же вызовом Control API,
 * что и дашборд, поэтому цифры совпадают до нанодоллара. bigint приводим к строкам:
 * server component не может передать bigint в client component.
 */
const usageLoads = new Map<string, { expiresAt: number | null; promise: Promise<EngineUsage> }>();

function loadUsageReport(
  engine: ReturnType<typeof getEngineClient>,
  engineAccountId: string,
  window: string,
): Promise<EngineUsage> {
  const cacheKey = `${engineAccountId}:${window}`;
  const cached = usageLoads.get(cacheKey);
  if (cached && (cached.expiresAt === null || cached.expiresAt > Date.now())) return cached.promise;
  if (cached) usageLoads.delete(cacheKey);
  if (usageLoads.size >= 1_000) usageLoads.delete(usageLoads.keys().next().value as string);

  const promise = engine.getUsage(engineAccountId, window).then(
    (usage) => {
      const entry = usageLoads.get(cacheKey);
      if (entry?.promise === promise) entry.expiresAt = Date.now() + USAGE_REPORT_CACHE_TTL_MS;
      return usage;
    },
    (error) => {
      if (usageLoads.get(cacheKey)?.promise === promise) usageLoads.delete(cacheKey);
      throw error;
    },
  );
  usageLoads.set(cacheKey, { expiresAt: null, promise });
  return promise;
}

export type PayingKeysDays = 1 | 7 | 30;

export interface PayingKeysQuery {
  days: PayingKeysDays;
  limit: number;
  offset: number;
  q: string;
  status: AdminKeyStatusFilter;
}

export type PayingKeyUsage =
  | ({ status: "available" } & EngineUsage)
  | { status: "unavailable"; window: string };

export interface PayingKeyRow {
  id: string;
  batchId: string;
  batchLabel: string | null;
  createdBy: string;
  keyMasked: string;
  engineAccountId: string;
  apiType: ApiType;
  enabled: boolean;
  faceValueNano: string;
  pricingContract: "legacy" | "official_1_to_1";
  createdAt: string;
  deliveredAt: string;
  usage: PayingKeyUsage;
}

export interface PayingKeysPage {
  days: PayingKeysDays;
  total: number;
  limit: number;
  offset: number;
  rows: PayingKeyRow[];
}

const PAYING_KEYS_USAGE_CONCURRENCY = 4;

/**
 * Выданные покупателям ключи с DB-пагинацией до live usage. Складские, снятые и
 * соседние страницы не создают Control API вызовов; сбой одного аккаунта не скрывает остальные.
 */
export async function loadPayingKeys(query: PayingKeysQuery): Promise<PayingKeysPage> {
  const { db } = getDatabase();
  const search = query.q.trim().slice(0, 80);
  const where = and(
    isNull(openkeysKeys.removedAt),
    isNotNull(openkeysKeys.deliveredAt),
    query.status === "active" ? eq(openkeysKeys.status, "active") : undefined,
    query.status === "disabled" ? eq(openkeysKeys.status, "disabled") : undefined,
    search
      ? or(
          ilike(openkeysKeys.keyMasked, `%${search}%`),
          ilike(openkeysKeys.engineAccountId, `%${search}%`),
          ilike(openkeysBatches.label, `%${search}%`),
          ilike(openkeysBatches.createdBy, `%${search}%`),
          sql`${openkeysBatches.id}::text ILIKE ${`%${search}%`}`,
        )
      : undefined,
  );

  const [rows, totals] = await Promise.all([
    db
      .select({
        id: openkeysKeys.id,
        batchId: openkeysKeys.batchId,
        batchLabel: openkeysBatches.label,
        createdBy: openkeysBatches.createdBy,
        keyMasked: openkeysKeys.keyMasked,
        engineAccountId: openkeysKeys.engineAccountId,
        apiType: openkeysBatches.apiType,
        enabled: openkeysKeys.status,
        faceValueNano: openkeysKeys.faceValueNano,
        pricingContract: openkeysKeys.pricingContract,
        createdAt: openkeysKeys.createdAt,
        deliveredAt: openkeysKeys.deliveredAt,
      })
      .from(openkeysKeys)
      .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
      .where(where)
      .orderBy(desc(openkeysKeys.deliveredAt), asc(openkeysKeys.id))
      .limit(query.limit)
      .offset(query.offset),
    db
      .select({ value: count() })
      .from(openkeysKeys)
      .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
      .where(where),
  ]);

  const window = `${query.days}d`;
  const engine = getEngineClient();
  const usage = new Map<string, PayingKeyUsage>();
  for (let offset = 0; offset < rows.length; offset += PAYING_KEYS_USAGE_CONCURRENCY) {
    const page = rows.slice(offset, offset + PAYING_KEYS_USAGE_CONCURRENCY);
    const settled = await Promise.allSettled(
      page.map((row) => loadUsageReport(engine, row.engineAccountId, window)),
    );
    settled.forEach((result, index) => {
      const accountId = page[index]!.engineAccountId;
      usage.set(accountId, result.status === "fulfilled"
        ? { status: "available", ...result.value }
        : { status: "unavailable", window });
    });
  }

  return {
    days: query.days,
    total: totals[0]?.value ?? 0,
    limit: query.limit,
    offset: query.offset,
    rows: rows.map((row) => ({
      id: row.id,
      batchId: row.batchId,
      batchLabel: row.batchLabel,
      createdBy: row.createdBy,
      keyMasked: row.keyMasked,
      engineAccountId: row.engineAccountId,
      apiType: apiTypeOf(row.apiType),
      enabled: row.enabled !== "disabled",
      faceValueNano: row.faceValueNano.toString(),
      pricingContract: row.pricingContract as "legacy" | "official_1_to_1",
      createdAt: row.createdAt.toISOString(),
      deliveredAt: row.deliveredAt!.toISOString(),
      usage: usage.get(row.engineAccountId) ?? { status: "unavailable", window },
    })),
  };
}

export async function loadUsageByViewToken(
  viewToken: string,
  window = "30d",
): Promise<KeyUsageView | null> {
  if (!/^[A-Za-z0-9_-]{22}$/.test(viewToken)) return null;
  const { db } = getDatabase();
  const [result] = await db
    .select({ key: openkeysKeys, apiType: openkeysBatches.apiType })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(eq(openkeysKeys.viewToken, viewToken))
    .limit(1);
  const row = result?.key;
  if (!row) return null;

  const engine = getEngineClient();
  const account = await engine.getAccount(row.engineAccountId);
  const balanceNano = BigInt(account.balance_nano);
  const reservedNano = BigInt(account.reserved_nano);
  const spentNano = BigInt(account.spent_nano);
  const officialBalance = officialBalanceBreakdown(balanceNano, reservedNano, spentNano, row.multBp);

  // Аккаунт без единого запроса — нормальное состояние только что проданного ключа,
  // а не ошибка: показываем пустой расход вместо страницы с ошибкой.
  let usage: EngineUsage | null = null;
  try {
    usage = await loadUsageReport(engine, row.engineAccountId, window);
  } catch {
    usage = null;
  }

  return {
    viewToken: row.viewToken,
    keyMasked: row.keyMasked,
    status: account.status === "disabled" ? "disabled" : row.status,
    createdAt: row.createdAt.toISOString(),
    faceValueNano: row.faceValueNano.toString(),
    multBp: row.multBp,
    pricingContract: row.pricingContract as "legacy" | "official_1_to_1",
    apiType: apiTypeOf(result.apiType),
    balanceNano: balanceNano.toString(),
    reservedNano: reservedNano.toString(),
    spentNano: spentNano.toString(),
    officialAvailableNano: officialBalance.available.toString(),
    officialReservedNano: officialBalance.reserved.toString(),
    officialRemainingNano: officialBalance.remaining.toString(),
    officialSpentNano: officialBalance.spent.toString(),
    usage,
  };
}

/**
 * Вход по самому ключу: подлинность проверяет движок своим публичным /balance,
 * поэтому мы не расшифровываем склад ради сравнения и не пишем секрет в логи.
 */
export async function resolveViewTokenByApiKey(apiKey: string): Promise<string | null> {
  if (!/^sk-pool-[A-Za-z0-9_-]{16,128}$/.test(apiKey)) return null;

  const config = loadConfig();
  let accountId: string | null = null;
  const checks: Array<{ baseUrl: string; headers: Record<string, string> }> = [
    { baseUrl: config.enginePublicBaseUrl, headers: { "x-api-key": apiKey } },
    { baseUrl: config.engineOpenAiPublicBaseUrl, headers: { authorization: `Bearer ${apiKey}` } },
  ];
  for (const check of checks) {
    try {
      const response = await fetch(`${check.baseUrl}/balance`, { headers: check.headers, cache: "no-store" });
      if (!response.ok) continue;
      const payload = (await response.json()) as { account?: unknown };
      if (typeof payload.account === "string") {
        accountId = payload.account;
        break;
      }
    } catch {
      // The provider planes are independent; one unavailable host must not block the other.
    }
  }
  if (!accountId) return null;

  const { db } = getDatabase();
  const [row] = await db
    .select({ viewToken: openkeysKeys.viewToken })
    .from(openkeysKeys)
    .where(eq(openkeysKeys.engineAccountId, accountId))
    .limit(1);

  return row?.viewToken ?? null;
}

/** Снятые ключи остаются tombstone-записями, но не входят в склад и историю выдачи. */
export type StockStatus = "stock" | "delivered";

export interface StockKey {
  id: string;
  batchId: string;
  status: StockStatus;
  /** Секрет доступен только пока ключ лежит на складе. */
  secret: string | null;
  keyMasked: string;
  viewUrl: string;
  faceValue: string;
  /** Номинал строкой нанодолларов для точного отображения и расчётов. */
  faceValueNano: string;
  apiType: ApiType;
  pricingContract: "legacy" | "official_1_to_1";
  label: string | null;
  enabled: boolean;
  createdAt: string;
  deliveredAt: string | null;
}

function stockStatusOf(row: { deliveredAt: Date | null }): StockStatus {
  return row.deliveredAt ? "delivered" : "stock";
}

/**
 * Склад и история выбранной партии. Партия содержит не больше 100 ключей, поэтому
 * здесь не нужна ещё одна пагинация; главное — не расшифровывать сотни чужих партий.
 * Видно только то, что выпустил сам админ.
 */
export async function listKeys(createdBy: string, batchId: string, limit = MAX_BATCH_QUANTITY): Promise<StockKey[]> {
  await reconcileIssuanceJobs();
  const config = loadConfig();
  const { db } = getDatabase();
  const rows = await db
    .select({
      id: openkeysKeys.id,
      batchId: openkeysKeys.batchId,
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      engineAccountId: openkeysKeys.engineAccountId,
      faceValueNano: openkeysKeys.faceValueNano,
      pricingContract: openkeysKeys.pricingContract,
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,
      enabled: openkeysKeys.status,

      secretCiphertext: openkeysKeys.secretCiphertext,
      secretNonce: openkeysKeys.secretNonce,
      secretVersion: openkeysKeys.secretVersion,
      secretKeyId: openkeysKeys.secretKeyId,
      label: openkeysBatches.label,
      apiType: openkeysBatches.apiType,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(
      eq(openkeysBatches.createdBy, createdBy),
      eq(openkeysBatches.id, batchId),
      isNull(openkeysKeys.removedAt),
    ))
    .orderBy(desc(openkeysKeys.createdAt))
    .limit(limit);

  return rows.map((row) => {
    const status = stockStatusOf(row);
    const secret =
      status === "stock" && row.secretCiphertext && row.secretNonce
        ? openSecret(
            { ciphertext: row.secretCiphertext, nonce: row.secretNonce, keyId: row.secretKeyId },
            row.secretVersion === 2 ? secretContext(row) : undefined,
          )
        : null;

    return {
      id: row.id,
      batchId: row.batchId,
      status,
      secret,
      keyMasked: row.keyMasked,
      viewUrl: `${config.publicBaseUrl}/profile/${row.viewToken}`,
      faceValue: formatUsd(row.faceValueNano, 0),
      faceValueNano: row.faceValueNano.toString(),
      apiType: apiTypeOf(row.apiType),
      pricingContract: row.pricingContract as "legacy" | "official_1_to_1",
      label: row.label,
      enabled: row.enabled !== "disabled",
      createdAt: row.createdAt.toISOString(),
      deliveredAt: row.deliveredAt?.toISOString() ?? null,
    };
  });
}

/**
 * Отметка «выдан»: ключ уходит со склада в историю. Секрет стираем — он уже у
 * покупателя, а хранить его дальше значит держать лишний риск без пользы.
 */
export async function markKeyDelivered(id: string, createdBy: string): Promise<boolean> {
  const { db } = getDatabase();
  if (!(await ownsKey(id, createdBy))) return false;

  const updated = await db
    .update(openkeysKeys)
    .set({ deliveredAt: new Date(), secretCiphertext: null, secretNonce: null })
    .where(and(eq(openkeysKeys.id, id), isNull(openkeysKeys.deliveredAt)))
    .returning({ id: openkeysKeys.id });
  return updated.length > 0;
}

/** Ключ принадлежит тому, кто выпустил его партию: чужой трогать нельзя. */
async function ownsKey(id: string, createdBy: string): Promise<boolean> {
  const { db } = getDatabase();
  const [row] = await db
    .select({ id: openkeysKeys.id })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(eq(openkeysKeys.id, id), eq(openkeysBatches.createdBy, createdBy), isNull(openkeysKeys.removedAt)))
    .limit(1);
  return row !== undefined;
}

/**
 * Удаление ключа. Сначала отключаем его в движке — иначе удалённый из нашей базы
 * ключ остался бы рабочим и раздавал бы деньги, о которых мы уже ничего не знаем, —
 * и только потом стираем запись. Выданные ключи не удаляем: они живут в истории.
 */
export async function removeKey(id: string, createdBy: string): Promise<boolean> {
  const { db } = getDatabase();
  if (!(await ownsKey(id, createdBy))) return false;

  const [row] = await db
    .select({ engineKeyId: openkeysKeys.engineKeyId, deliveredAt: openkeysKeys.deliveredAt })
    .from(openkeysKeys)
    .where(eq(openkeysKeys.id, id))
    .limit(1);
  if (!row || row.deliveredAt) return false;

  await getEngineClient().disableKey(row.engineKeyId);
  await db
    .update(openkeysKeys)
    .set({
      status: "disabled",
      disabledAt: new Date(),
      removedAt: new Date(),
      removedBy: createdBy,
      removalReason: "manual stock removal",
      secretCiphertext: null,
      secretNonce: null,
    })
    .where(and(eq(openkeysKeys.id, id), isNull(openkeysKeys.removedAt)));
  return true;
}

/**
 * Удаление всего склада разом. Ключи отключаются по одному: частичный успех
 * лучше, чем отказ целиком, поэтому счётчик показывает, сколько реально ушло.
 */
export async function removeAllStock(createdBy: string, apiType?: ApiType, batchId?: string): Promise<number> {
  const { db } = getDatabase();
  const rows = await db
    .select({ id: openkeysKeys.id, engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(
      eq(openkeysBatches.createdBy, createdBy),
      batchId ? eq(openkeysBatches.id, batchId) : undefined,
      isNull(openkeysKeys.deliveredAt),
      isNull(openkeysKeys.removedAt),
      apiType === "anthropic"
        ? or(isNull(openkeysBatches.apiType), eq(openkeysBatches.apiType, "anthropic"))
        : apiType === "openai"
          ? eq(openkeysBatches.apiType, "openai")
          : undefined,
    ));

  const engine = getEngineClient();
  let removed = 0;
  for (const row of rows) {
    try {
      await engine.disableKey(row.engineKeyId);
      await db
        .update(openkeysKeys)
        .set({
          status: "disabled",
          disabledAt: new Date(),
          removedAt: new Date(),
          removedBy: createdBy,
          removalReason: "bulk stock removal",
          secretCiphertext: null,
          secretNonce: null,
        })
        .where(and(eq(openkeysKeys.id, row.id), isNull(openkeysKeys.removedAt)));
      removed += 1;
    } catch {
      // Ключ, который не удалось отключить, оставляем в базе: удалить запись
      // о рабочем ключе значит потерять его из виду навсегда.
    }
  }
  return removed;
}

export interface MonitorRow {
  id: string;
  batchId: string;
  batchLabel: string | null;
  status: StockStatus;
  keyMasked: string;
  label: string | null;
  faceValue: string;
  viewUrl: string;
  createdAt: string;
  deliveredAt: string | null;
  /** Живые данные движка. null, если аккаунт не ответил — строку всё равно показываем. */
  remaining: string | null;
  spent: string | null;
  spentNano: string | null;
  enabled: boolean;
}

const ENGINE_ACCOUNT_BATCH = 500;
const ENGINE_BATCH_CONCURRENCY = 4;

/** Один batch Control API заменяет два запроса на каждый ключ и не создаёт N+1 при росте склада. */
async function loadEngineAccountMap(accountIds: string[]): Promise<Map<string, EngineAccount>> {
  const unique = [...new Set(accountIds)];
  const chunks: string[][] = [];
  for (let offset = 0; offset < unique.length; offset += ENGINE_ACCOUNT_BATCH) {
    chunks.push(unique.slice(offset, offset + ENGINE_ACCOUNT_BATCH));
  }

  const accounts = new Map<string, EngineAccount>();
  const engine = getEngineClient();
  for (let offset = 0; offset < chunks.length; offset += ENGINE_BATCH_CONCURRENCY) {
    const settled = await Promise.allSettled(
      chunks.slice(offset, offset + ENGINE_BATCH_CONCURRENCY).map((ids) => engine.getAccounts(ids)),
    );
    for (const result of settled) {
      if (result.status !== "fulfilled") continue;
      for (const account of result.value) accounts.set(account.account, account);
    }
  }
  return accounts;
}

/**
 * Наблюдение за проданными ключами: остаток и расход берём напрямую у движка,
 * поэтому цифры здесь всегда актуальные, а не снимок на момент выпуска.
 */
export async function loadKeyMonitor(createdBy: string, limit = 300): Promise<MonitorRow[]> {
  const config = loadConfig();
  const { db } = getDatabase();
  const rows = await db
    .select({
      id: openkeysKeys.id,
      batchId: openkeysKeys.batchId,
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      engineAccountId: openkeysKeys.engineAccountId,
      faceValueNano: openkeysKeys.faceValueNano,
      multBp: openkeysKeys.multBp,
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,
      enabled: openkeysKeys.status,
      label: openkeysBatches.label,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(eq(openkeysBatches.createdBy, createdBy), isNull(openkeysKeys.removedAt)))
    .orderBy(desc(openkeysKeys.createdAt))
    .limit(limit);

  const accounts = await loadEngineAccountMap(rows.map((row) => row.engineAccountId));
  return rows.map((row) => {
    let remaining: string | null = null;
    let spent: string | null = null;
    let spentNano: string | null = null;
    const account = accounts.get(row.engineAccountId);

    if (account) {
      remaining = formatUsd(officialRemainingNano(
        BigInt(account.balance_nano),
        BigInt(account.reserved_nano),
        row.multBp,
      ));
      const spentOfficial = balanceToOfficialNano(BigInt(account.spent_nano), row.multBp);
      spent = formatUsd(spentOfficial);
      spentNano = spentOfficial.toString();
    }

    return {
      id: row.id,
      batchId: row.batchId,
      batchLabel: row.label,
      status: stockStatusOf(row),
      keyMasked: row.keyMasked,
      label: row.label,
      faceValue: formatUsd(row.faceValueNano, 0),
      viewUrl: `${config.publicBaseUrl}/profile/${row.viewToken}`,
      createdAt: row.createdAt.toISOString(),
      deliveredAt: row.deliveredAt?.toISOString() ?? null,
      remaining,
      spent,
      spentNano,
      enabled: row.enabled !== "disabled",
    };
  });
}

async function applyKeyEnabled(id: string, enabled: boolean): Promise<boolean> {
  const { db } = getDatabase();
  const [row] = await db
    .select({ engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .where(and(eq(openkeysKeys.id, id), isNull(openkeysKeys.removedAt)))
    .limit(1);
  if (!row) return false;

  await getEngineClient().setKeyStatus(row.engineKeyId, enabled ? "active" : "disabled");
  await db
    .update(openkeysKeys)
    .set({ status: enabled ? "active" : "disabled", disabledAt: enabled ? null : new Date() })
    .where(eq(openkeysKeys.id, id));
  return true;
}

/** Включение и отключение ключа владельцем партии. Запись остаётся — это не удаление. */
export async function setKeyEnabled(id: string, createdBy: string, enabled: boolean): Promise<boolean> {
  if (!(await ownsKey(id, createdBy))) return false;
  return applyKeyEnabled(id, enabled);
}

/** Центральная админка уже прошла managed-admin gate и может управлять любой партией. */
export async function setKeyEnabledForInternalAdmin(id: string, enabled: boolean): Promise<boolean> {
  return applyKeyEnabled(id, enabled);
}

export interface BatchSummary {
  id: string;
  label: string | null;
  faceValue: string;
  quantity: number;
  apiType: ApiType;
  pricingContract: "legacy" | "official_1_to_1";
  stockCount: number;
  deliveredCount: number;
  disabledCount: number;
  createdAt: string;
}

export interface BatchPage {
  batches: BatchSummary[];
  total: number;
  limit: number;
  offset: number;
  totals: {
    stock: number;
    delivered: number;
    disabled: number;
  };
}

/**
 * Партии админа с серверным поиском и пагинацией. Счётчики считаются в PostgreSQL,
 * поэтому список остаётся быстрым и не расшифровывает складские секреты.
 */
export async function listBatches(
  createdBy: string,
  options: { limit?: number; offset?: number; q?: string } = {},
): Promise<BatchPage> {
  const { db } = getDatabase();
  const limit = Number.isInteger(options.limit) ? Math.min(50, Math.max(1, options.limit!)) : 20;
  const offset = Number.isInteger(options.offset) ? Math.max(0, options.offset!) : 0;
  const query = options.q?.trim().slice(0, 80) ?? "";
  const owner = eq(openkeysBatches.createdBy, createdBy);
  const where = query
    ? and(owner, or(
        ilike(openkeysBatches.label, `%${query}%`),
        sql`${openkeysBatches.id}::text ILIKE ${`%${query}%`}`,
      ))
    : owner;

  const [rows, totalRows, keyTotals] = await Promise.all([
    db
      .select({
        id: openkeysBatches.id,
        label: openkeysBatches.label,
        faceValueNano: openkeysBatches.faceValueNano,
        quantity: openkeysBatches.quantity,
        apiType: openkeysBatches.apiType,
        pricingContract: openkeysBatches.pricingContract,
        createdAt: openkeysBatches.createdAt,
        stockCount: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.deliveredAt} is null)`.mapWith(Number),
        deliveredCount: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.deliveredAt} is not null)`.mapWith(Number),
        disabledCount: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.status} = 'disabled')`.mapWith(Number),
      })
      .from(openkeysBatches)
      .leftJoin(openkeysKeys, eq(openkeysKeys.batchId, openkeysBatches.id))
      .where(where)
      .groupBy(openkeysBatches.id)
      .orderBy(desc(openkeysBatches.createdAt))
      .limit(limit)
      .offset(offset),
    db.select({ value: count() }).from(openkeysBatches).where(where),
    db
      .select({
        stock: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.deliveredAt} is null)`.mapWith(Number),
        delivered: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.deliveredAt} is not null)`.mapWith(Number),
        disabled: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is null and ${openkeysKeys.status} = 'disabled')`.mapWith(Number),
      })
      .from(openkeysKeys)
      .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
      .where(owner),
  ]);

  return {
    total: totalRows[0]?.value ?? 0,
    limit,
    offset,
    totals: keyTotals[0] ?? { stock: 0, delivered: 0, disabled: 0 },
    batches: rows.map((row) => ({
      id: row.id,
      label: row.label,
      faceValue: formatUsd(row.faceValueNano, 0),
      quantity: row.quantity,
      apiType: apiTypeOf(row.apiType),
      pricingContract: row.pricingContract as "legacy" | "official_1_to_1",
      stockCount: row.stockCount,
      deliveredCount: row.deliveredCount,
      disabledCount: row.disabledCount,
      createdAt: row.createdAt.toISOString(),
    })),
  };
}

export type AdminKeyStatusFilter = "all" | "active" | "disabled";

export interface AdminKeyDirectoryQuery {
  limit: number;
  offset: number;
  q: string;
  batchId: string | null;
  status: AdminKeyStatusFilter;
  usage: AdminUsageFilter;
}

export interface AdminKeyDirectoryRow {
  id: string;
  batchId: string;
  batchLabel: string | null;
  createdBy: string;
  keyMasked: string;
  engineAccountId: string;
  apiType: ApiType;
  status: StockStatus;
  enabled: boolean;
  usageState: AdminUsageState;
  usagePercent: number | null;
  faceValueNano: string;
  remainingNano: string | null;
  spentNano: string | null;
  viewUrl: string;
  createdAt: string;
  deliveredAt: string | null;
}

export interface AdminKeyDirectory {
  rows: AdminKeyDirectoryRow[];
  total: number;
  limit: number;
  offset: number;
  truncated: boolean;
  summary: {
    active: number;
    disabled: number;
    unused: number;
    used: number;
    exhausted: number;
    unavailable: number;
    spentNano: string;
    remainingNano: string;
  };
  batches: Array<{
    id: string;
    label: string | null;
    createdBy: string;
    createdAt: string;
  }>;
}

const ADMIN_DIRECTORY_SCAN_LIMIT = 10_000;

/**
 * Закрытый каталог для единой админки. Метаданные читаются одним SQL, live-балансы —
 * bounded batch-вызовами по 500 аккаунтов, затем usage-фильтр и пагинация применяются
 * на сервере. Полные ключи и складской шифротекст в этот контракт не попадают.
 */
export async function loadAdminKeyDirectory(query: AdminKeyDirectoryQuery): Promise<AdminKeyDirectory> {
  const config = loadConfig();
  const { db } = getDatabase();
  const search = query.q.trim().slice(0, 80);
  const where = and(
    isNull(openkeysKeys.removedAt),
    query.batchId ? eq(openkeysBatches.id, query.batchId) : undefined,
    query.status === "active" ? eq(openkeysKeys.status, "active") : undefined,
    query.status === "disabled" ? eq(openkeysKeys.status, "disabled") : undefined,
    search
      ? or(
          ilike(openkeysKeys.keyMasked, `%${search}%`),
          ilike(openkeysKeys.engineAccountId, `%${search}%`),
          ilike(openkeysBatches.label, `%${search}%`),
          ilike(openkeysBatches.createdBy, `%${search}%`),
          sql`${openkeysBatches.id}::text ILIKE ${`%${search}%`}`,
        )
      : undefined,
  );

  const [rawRows, rawBatches] = await Promise.all([
    db
      .select({
        id: openkeysKeys.id,
        batchId: openkeysKeys.batchId,
        batchLabel: openkeysBatches.label,
        createdBy: openkeysBatches.createdBy,
        keyMasked: openkeysKeys.keyMasked,
        viewToken: openkeysKeys.viewToken,
        engineAccountId: openkeysKeys.engineAccountId,
        apiType: openkeysBatches.apiType,
        faceValueNano: openkeysKeys.faceValueNano,
        multBp: openkeysKeys.multBp,
        enabled: openkeysKeys.status,
        createdAt: openkeysKeys.createdAt,
        deliveredAt: openkeysKeys.deliveredAt,
      })
      .from(openkeysKeys)
      .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
      .where(where)
      .orderBy(desc(openkeysKeys.createdAt))
      .limit(ADMIN_DIRECTORY_SCAN_LIMIT + 1),
    db
      .select({
        id: openkeysBatches.id,
        label: openkeysBatches.label,
        createdBy: openkeysBatches.createdBy,
        createdAt: openkeysBatches.createdAt,
      })
      .from(openkeysBatches)
      .orderBy(desc(openkeysBatches.createdAt))
      .limit(2_000),
  ]);

  const truncated = rawRows.length > ADMIN_DIRECTORY_SCAN_LIMIT;
  const sourceRows = truncated ? rawRows.slice(0, ADMIN_DIRECTORY_SCAN_LIMIT) : rawRows;
  const accounts = await loadEngineAccountMap(sourceRows.map((row) => row.engineAccountId));
  const materialized: AdminKeyDirectoryRow[] = sourceRows.map((row) => {
    const account = accounts.get(row.engineAccountId);
    const faceValueNano = row.faceValueNano;
    const spentNano = account === undefined
      ? null
      : balanceToOfficialNano(BigInt(account.spent_nano), row.multBp);
    const remainingNano = account === undefined
      ? null
      : officialRemainingNano(BigInt(account.balance_nano), BigInt(account.reserved_nano), row.multBp);
    const usageState = classifyAdminUsage(spentNano, remainingNano);
    return {
      id: row.id,
      batchId: row.batchId,
      batchLabel: row.batchLabel,
      createdBy: row.createdBy,
      keyMasked: row.keyMasked,
      engineAccountId: row.engineAccountId,
      apiType: apiTypeOf(row.apiType),
      status: stockStatusOf(row),
      // The control here is key-scoped. Account status is a separate engine concern and must not
      // turn the key button into an "enable account" action that can never fulfil its label.
      enabled: row.enabled !== "disabled",
      usageState,
      usagePercent: adminUsagePercent(spentNano, faceValueNano),
      faceValueNano: faceValueNano.toString(),
      remainingNano: remainingNano?.toString() ?? null,
      spentNano: spentNano?.toString() ?? null,
      viewUrl: `${config.publicBaseUrl}/profile/${row.viewToken}`,
      createdAt: row.createdAt.toISOString(),
      deliveredAt: row.deliveredAt?.toISOString() ?? null,
    };
  });
  const filtered = materialized.filter((row) => matchesAdminUsage(row.usageState, query.usage));
  let spentNano = 0n;
  let remainingNano = 0n;
  const summary = {
    active: 0,
    disabled: 0,
    unused: 0,
    used: 0,
    exhausted: 0,
    unavailable: 0,
  };
  for (const row of filtered) {
    summary[row.enabled ? "active" : "disabled"] += 1;
    summary[row.usageState] += 1;
    if (row.spentNano !== null) spentNano += BigInt(row.spentNano);
    if (row.remainingNano !== null) remainingNano += BigInt(row.remainingNano);
  }

  return {
    rows: filtered.slice(query.offset, query.offset + query.limit),
    total: filtered.length,
    limit: query.limit,
    offset: query.offset,
    truncated,
    summary: {
      ...summary,
      spentNano: spentNano.toString(),
      remainingNano: remainingNano.toString(),
    },
    batches: rawBatches.map((batch) => ({
      id: batch.id,
      label: batch.label,
      createdBy: batch.createdBy,
      createdAt: batch.createdAt.toISOString(),
    })),
  };
}

export interface AdminKeyLookupRow {
  engineAccountId: string;
  keyMasked: string;
  batchId: string;
  batchLabel: string | null;
  createdBy: string;
  apiType: ApiType;
  faceValueNano: string;
  enabled: boolean;
  viewUrl: string;
}

/**
 * Лёгкая карта engine-аккаунт → контекст ключа. Один SQL без live-балансов:
 * ей подписываются openkeys-строки в «Кто тратит» и реестрах аккаунтов, где
 * тяжёлый каталог с bounded batch-запросами был бы лишним.
 */
export async function loadAdminKeyLookup(): Promise<{ rows: AdminKeyLookupRow[]; truncated: boolean }> {
  const config = loadConfig();
  const { db } = getDatabase();
  const raw = await db
    .select({
      engineAccountId: openkeysKeys.engineAccountId,
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      batchId: openkeysKeys.batchId,
      batchLabel: openkeysBatches.label,
      createdBy: openkeysBatches.createdBy,
      apiType: openkeysBatches.apiType,
      faceValueNano: openkeysKeys.faceValueNano,
      status: openkeysKeys.status,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(isNull(openkeysKeys.removedAt))
    .orderBy(desc(openkeysKeys.createdAt))
    .limit(ADMIN_DIRECTORY_SCAN_LIMIT + 1);
  const truncated = raw.length > ADMIN_DIRECTORY_SCAN_LIMIT;
  const rows = (truncated ? raw.slice(0, ADMIN_DIRECTORY_SCAN_LIMIT) : raw).map((row) => ({
    engineAccountId: row.engineAccountId,
    keyMasked: row.keyMasked,
    batchId: row.batchId,
    batchLabel: row.batchLabel,
    createdBy: row.createdBy,
    apiType: apiTypeOf(row.apiType),
    faceValueNano: row.faceValueNano.toString(),
    enabled: row.status !== "disabled",
    viewUrl: `${config.publicBaseUrl}/profile/${row.viewToken}`,
  }));
  return { rows, truncated };
}
