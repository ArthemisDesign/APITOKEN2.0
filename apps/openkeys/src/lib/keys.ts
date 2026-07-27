import "server-only";
import { createHash, randomBytes } from "node:crypto";
import type { EngineUsage } from "@claude-api/contracts";
import { openkeysBatches, openkeysKeys } from "@claude-api/openkeys-db";
import { and, desc, eq, isNull } from "drizzle-orm";
import { loadConfig } from "./config";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";
import { balanceToOfficialNano, formatUsd, officialNanoToBalance } from "./money";
import { openSecret, sealSecret } from "./secret-box";

export const MAX_BATCH_QUANTITY = 100;

export interface IssueBatchInput {
  faceValueNano: bigint;
  quantity: number;
  multBp: number;
  label: string | null;
  note: string | null;
  createdBy: string;
}

export interface IssuedKey {
  /** Полный секрет. Лежит на складе в шифрованном виде, пока ключ не выдан. */
  secret: string;
  viewToken: string;
  viewUrl: string;
  keyMasked: string;
}

function maskKey(secret: string): string {
  return `${secret.slice(0, 12)}…${secret.slice(-4)}`;
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
      createdBy: input.createdBy,
    })
    .returning({ id: openkeysBatches.id });

  if (!batch) throw new Error("Не удалось создать партию");

  const issued: IssuedKey[] = [];
  for (let index = 0; index < input.quantity; index += 1) {
    const viewToken = randomBytes(16).toString("base64url");
    const account = await engine.createAccount({
      handle: `openkeys-${viewToken.slice(0, 16)}`,
      multBp: input.multBp,
    });

    // ref идемпотентен на стороне движка: повторная попытка не задвоит зачисление.
    await engine.creditAccount(account.account, balanceNano, `openkeys:${batch.id}:${index}`);
    const key = await engine.issueKey(account.account, { label: `openkeys ${viewToken.slice(0, 8)}` });
    const sealed = sealSecret(key.key);

    await db.insert(openkeysKeys).values({
      batchId: batch.id,
      viewToken,
      engineAccountId: account.account,
      engineKeyId: key.key_id,
      keyMasked: maskKey(key.key),
      keySha256: keyDigest(key.key),
      secretCiphertext: sealed.ciphertext,
      secretNonce: sealed.nonce,
      faceValueNano: input.faceValueNano,
      multBp: input.multBp,
    });

    issued.push({
      secret: key.key,
      viewToken,
      viewUrl: `${config.publicBaseUrl}/profile/${viewToken}`,
      keyMasked: maskKey(key.key),
    });
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
  balanceNano: string;
  spentNano: string;
  /** Остаток и расход, пересчитанные в официальный прайс Anthropic. */
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
export async function loadUsageByViewToken(
  viewToken: string,
  window = "30d",
): Promise<KeyUsageView | null> {
  const { db } = getDatabase();
  const [row] = await db.select().from(openkeysKeys).where(eq(openkeysKeys.viewToken, viewToken)).limit(1);
  if (!row) return null;

  const engine = getEngineClient();
  const account = await engine.getAccount(row.engineAccountId);
  const balanceNano = BigInt(account.balance_nano);
  const spentNano = BigInt(account.spent_nano);

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
    balanceNano: balanceNano.toString(),
    spentNano: spentNano.toString(),
    officialRemainingNano: balanceToOfficialNano(balanceNano, row.multBp).toString(),
    officialSpentNano: balanceToOfficialNano(spentNano, row.multBp).toString(),
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
  const response = await fetch(`${config.enginePublicBaseUrl}/balance`, {
    headers: { "x-api-key": apiKey },
    cache: "no-store",
  });
  if (!response.ok) return null;

  const payload = (await response.json()) as { account?: unknown };
  if (typeof payload.account !== "string") return null;

  const { db } = getDatabase();
  const [row] = await db
    .select({ viewToken: openkeysKeys.viewToken })
    .from(openkeysKeys)
    .where(eq(openkeysKeys.engineAccountId, payload.account))
    .limit(1);

  return row?.viewToken ?? null;
}

/** Удалённых статусов нет: удаление стирает запись, остаётся склад и выданные. */
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
  const config = loadConfig();
  const { db } = getDatabase();
  const rows = await db
    .select({
      id: openkeysKeys.id,
      batchId: openkeysKeys.batchId,
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      faceValueNano: openkeysKeys.faceValueNano,
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,

      secretCiphertext: openkeysKeys.secretCiphertext,
      secretNonce: openkeysKeys.secretNonce,
      label: openkeysBatches.label,
    })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(eq(openkeysBatches.createdBy, createdBy))
    .orderBy(desc(openkeysKeys.createdAt))
    .limit(limit);

  return rows.map((row) => {
    const status = stockStatusOf(row);
    const secret =
      status === "stock" && row.secretCiphertext && row.secretNonce
        ? openSecret({ ciphertext: row.secretCiphertext, nonce: row.secretNonce })
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
    .where(and(eq(openkeysKeys.id, id), eq(openkeysBatches.createdBy, createdBy)))
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
    .select({ engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .where(eq(openkeysKeys.id, id))
    .limit(1);
  if (!row) return false;

  await getEngineClient().disableKey(row.engineKeyId);
  await db.delete(openkeysKeys).where(eq(openkeysKeys.id, id));
  return true;
}

/**
 * Удаление всего склада разом. Ключи отключаются по одному: частичный успех
 * лучше, чем отказ целиком, поэтому счётчик показывает, сколько реально ушло.
 */
export async function removeAllStock(createdBy: string): Promise<number> {
  const { db } = getDatabase();
  const rows = await db
    .select({ id: openkeysKeys.id, engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(and(eq(openkeysBatches.createdBy, createdBy), isNull(openkeysKeys.deliveredAt)));

  const engine = getEngineClient();
  let removed = 0;
  for (const row of rows) {
    try {
      await engine.disableKey(row.engineKeyId);
      await db.delete(openkeysKeys).where(eq(openkeysKeys.id, row.id));
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
    .where(eq(openkeysBatches.createdBy, createdBy))
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
          remaining = formatUsd(balanceToOfficialNano(BigInt(account.balance_nano), row.multBp));
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
    createdAt: row.createdAt.toISOString(),
  }));
}
