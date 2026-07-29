import "server-only";
import { createHash, randomBytes, randomUUID } from "node:crypto";
import type { EngineUsage } from "@claude-api/contracts";
import { openkeysBatches, openkeysIssuanceJobs, openkeysKeys } from "@claude-api/openkeys-db";
import { and, desc, eq, inArray, isNull, lt, or } from "drizzle-orm";
import { apiTypeOf, type ApiType } from "./api-product";
import { loadConfig } from "./config";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";
import {
  balanceToOfficialNano,
  formatUsd,
  officialBalanceBreakdown,
  officialNanoToBalance,
  officialRemainingNano,
} from "./money";
import { openSecret, sealSecret } from "./secret-box";

export const MAX_BATCH_QUANTITY = 100;

export interface IssueBatchInput {
  faceValueNano: bigint;
  quantity: number;
  multBp: number;
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
  if (!Number.isInteger(input.quantity) || input.quantity < 1 || input.quantity > MAX_BATCH_QUANTITY) {
    throw new Error(`Количество ключей должно быть от 1 до ${MAX_BATCH_QUANTITY}`);
  }
  if (!Number.isInteger(input.multBp) || input.multBp < 1 || input.multBp > 10_000) {
    throw new Error("Множитель должен быть от 1 до 10000 basis points");
  }

  await reconcileIssuanceJobs();

  const config = loadConfig();
  const { db } = getDatabase();
  const engine = getEngineClient();
  const balanceNano = officialNanoToBalance(input.faceValueNano, input.multBp);
  if (balanceNano <= 0n) throw new Error("Номинал слишком мал для выбранного множителя");

  const [batch] = await db
    .insert(openkeysBatches)
    .values({
      label: input.label,
      faceValueNano: input.faceValueNano,
      multBp: input.multBp,
      quantity: input.quantity,
      note: input.note,
      apiType: input.apiType,
      createdBy: input.createdBy,
    })
    .returning({ id: openkeysBatches.id });

  if (!batch) throw new Error("Не удалось создать партию");

  const issued: IssuedKey[] = [];
  for (let index = 0; index < input.quantity; index += 1) {
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
        multBp: input.multBp,
      });
      accountId = account.account;
      await db.update(openkeysIssuanceJobs).set({
        status: "account_created", engineAccountId: account.account, updatedAt: new Date(),
      }).where(eq(openkeysIssuanceJobs.id, job.id));

      // ref идемпотентен на стороне движка: повторная попытка не задвоит зачисление.
      await engine.creditAccount(account.account, balanceNano, `openkeys:${batch.id}:${index}`);
      await db.update(openkeysIssuanceJobs).set({ status: "credited", updatedAt: new Date() })
        .where(eq(openkeysIssuanceJobs.id, job.id));
      const key = await engine.issueKey(account.account, {
        label: input.apiType === "anthropic"
          ? `openkeys ${viewToken.slice(0, 8)}`
          : `openkeys openai ${viewToken.slice(0, 8)}`,
      });
      await db.update(openkeysIssuanceJobs).set({
        status: "key_issued", engineKeyId: key.key_id, updatedAt: new Date(),
      }).where(eq(openkeysIssuanceJobs.id, job.id));
      const id = randomUUID();
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
        multBp: input.multBp,
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
const usageLoads = new Map<string, { expiresAt: number; promise: Promise<KeyUsageView | null> }>();

export async function loadUsageByViewToken(
  viewToken: string,
  window = "30d",
): Promise<KeyUsageView | null> {
  if (!/^[A-Za-z0-9_-]{22}$/.test(viewToken)) return null;
  const cacheKey = `${viewToken}:${window}`;
  const now = Date.now();
  const cached = usageLoads.get(cacheKey);
  if (cached && cached.expiresAt > now) return cached.promise;
  if (cached) usageLoads.delete(cacheKey);
  if (usageLoads.size >= 1_000) usageLoads.delete(usageLoads.keys().next().value as string);

  const promise = loadUsageUncached(viewToken, window).catch((error) => {
    usageLoads.delete(cacheKey);
    throw error;
  });
  usageLoads.set(cacheKey, { expiresAt: now + 5_000, promise });
  return promise;
}

async function loadUsageUncached(viewToken: string, window: string): Promise<KeyUsageView | null> {
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
    usage = await engine.getUsage(row.engineAccountId, window);
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
  /** Номинал строкой нанодолларов — по нему группируем склад. */
  faceValueNano: string;
  apiType: ApiType;
  label: string | null;
  createdAt: string;
  deliveredAt: string | null;
}

function stockStatusOf(row: { deliveredAt: Date | null }): StockStatus {
  return row.deliveredAt ? "delivered" : "stock";
}

/**
 * Склад и история одним запросом: строк мало, а порядок нужен общий.
 * Видно только то, что выпустил сам админ — партии принадлежат тому, кто их создал.
 */
export async function listKeys(createdBy: string, limit = 500): Promise<StockKey[]> {
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
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,

      secretCiphertext: openkeysKeys.secretCiphertext,
      secretNonce: openkeysKeys.secretNonce,
      secretVersion: openkeysKeys.secretVersion,
      secretKeyId: openkeysKeys.secretKeyId,
      label: openkeysBatches.label,
      apiType: openkeysBatches.apiType,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(eq(openkeysBatches.createdBy, createdBy), isNull(openkeysKeys.removedAt)))
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
      label: row.label,
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
export async function removeAllStock(createdBy: string, apiType?: ApiType): Promise<number> {
  const { db } = getDatabase();
  const rows = await db
    .select({ id: openkeysKeys.id, engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(
      eq(openkeysBatches.createdBy, createdBy),
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
  enabled: boolean | null;
}

/** Живой опрос движка идёт пачками: последовательно это минуты на полусотне ключей. */
const MONITOR_CONCURRENCY = 8;

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
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      engineAccountId: openkeysKeys.engineAccountId,
      engineKeyId: openkeysKeys.engineKeyId,
      faceValueNano: openkeysKeys.faceValueNano,
      multBp: openkeysKeys.multBp,
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,
      label: openkeysBatches.label,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(eq(openkeysBatches.createdBy, createdBy), isNull(openkeysKeys.removedAt)))
    .orderBy(desc(openkeysKeys.createdAt))
    .limit(limit);

  const engine = getEngineClient();
  const result: MonitorRow[] = [];

  for (let offset = 0; offset < rows.length; offset += MONITOR_CONCURRENCY) {
    const slice = rows.slice(offset, offset + MONITOR_CONCURRENCY);
    const loaded = await Promise.all(
      slice.map(async (row) => {
        let remaining: string | null = null;
        let spent: string | null = null;
        let spentNano: string | null = null;
        let enabled: boolean | null = null;

        try {
          const [account, keys] = await Promise.all([
            engine.getAccount(row.engineAccountId),
            engine.listKeys(row.engineAccountId),
          ]);
          remaining = formatUsd(officialRemainingNano(
            BigInt(account.balance_nano),
            BigInt(account.reserved_nano),
            row.multBp,
          ));
          const spentOfficial = balanceToOfficialNano(BigInt(account.spent_nano), row.multBp);
          spent = formatUsd(spentOfficial);
          spentNano = spentOfficial.toString();
          enabled = keys.find((key) => key.key_id === row.engineKeyId)?.status !== "disabled";
        } catch {
          // Недоступный аккаунт не должен прятать строку: сам ключ у нас есть,
          // и админ должен видеть его в списке даже без свежих цифр.
        }

        return {
          id: row.id,
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
          enabled,
        };
      }),
    );
    result.push(...loaded);
  }

  return result;
}

/** Включение и отключение ключа. Запись у нас остаётся — это не удаление. */
export async function setKeyEnabled(id: string, createdBy: string, enabled: boolean): Promise<boolean> {
  const { db } = getDatabase();
  if (!(await ownsKey(id, createdBy))) return false;

  const [row] = await db
    .select({ engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .where(eq(openkeysKeys.id, id))
    .limit(1);
  if (!row) return false;

  await getEngineClient().setKeyStatus(row.engineKeyId, enabled ? "active" : "disabled");
  await db
    .update(openkeysKeys)
    .set({ status: enabled ? "active" : "disabled", disabledAt: enabled ? null : new Date() })
    .where(eq(openkeysKeys.id, id));
  return true;
}

export interface BatchSummary {
  id: string;
  label: string | null;
  faceValue: string;
  quantity: number;
  apiType: ApiType;
  createdAt: string;
}

/** Партии этого админа. Состав партии страница берёт из общего списка ключей. */
export async function listBatches(createdBy: string, limit = 200): Promise<BatchSummary[]> {
  const { db } = getDatabase();
  const rows = await db
    .select()
    .from(openkeysBatches)
    .where(eq(openkeysBatches.createdBy, createdBy))
    .orderBy(desc(openkeysBatches.createdAt))
    .limit(limit);

  return rows.map((row) => ({
    id: row.id,
    label: row.label,
    faceValue: formatUsd(row.faceValueNano, 0),
    quantity: row.quantity,
    apiType: apiTypeOf(row.apiType),
    createdAt: row.createdAt.toISOString(),
  }));
}
