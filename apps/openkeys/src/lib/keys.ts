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

export type StockStatus = "stock" | "delivered" | "removed";

export interface StockKey {
  id: string;
  status: StockStatus;
  /** Секрет доступен только пока ключ лежит на складе. */
  secret: string | null;
  keyMasked: string;
  viewUrl: string;
  faceValue: string;
  label: string | null;
  createdAt: string;
  deliveredAt: string | null;
  removedAt: string | null;
}

function stockStatusOf(row: { deliveredAt: Date | null; removedAt: Date | null }): StockStatus {
  if (row.removedAt) return "removed";
  if (row.deliveredAt) return "delivered";
  return "stock";
}

/** Весь склад и история одним запросом: строк здесь мало, а порядок нужен общий. */
export async function listKeys(limit = 500): Promise<StockKey[]> {
  const config = loadConfig();
  const { db } = getDatabase();
  const rows = await db
    .select({
      id: openkeysKeys.id,
      keyMasked: openkeysKeys.keyMasked,
      viewToken: openkeysKeys.viewToken,
      faceValueNano: openkeysKeys.faceValueNano,
      createdAt: openkeysKeys.createdAt,
      deliveredAt: openkeysKeys.deliveredAt,
      removedAt: openkeysKeys.removedAt,
      secretCiphertext: openkeysKeys.secretCiphertext,
      secretNonce: openkeysKeys.secretNonce,
      label: openkeysBatches.label,
    })
    .from(openkeysKeys)
    .leftJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
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
      status,
      secret,
      keyMasked: row.keyMasked,
      viewUrl: `${config.publicBaseUrl}/profile/${row.viewToken}`,
      faceValue: formatUsd(row.faceValueNano, 0),
      label: row.label,
      createdAt: row.createdAt.toISOString(),
      deliveredAt: row.deliveredAt?.toISOString() ?? null,
      removedAt: row.removedAt?.toISOString() ?? null,
    };
  });
}

/**
 * Отметка «выдан»: ключ уходит со склада в историю. Секрет стираем — он уже у
 * покупателя, а хранить его дальше значит держать лишний риск без пользы.
 */
export async function markKeyDelivered(id: string): Promise<boolean> {
  const { db } = getDatabase();
  const updated = await db
    .update(openkeysKeys)
    .set({ deliveredAt: new Date(), secretCiphertext: null, secretNonce: null })
    .where(and(eq(openkeysKeys.id, id), isNull(openkeysKeys.deliveredAt), isNull(openkeysKeys.removedAt)))
    .returning({ id: openkeysKeys.id });
  return updated.length > 0;
}

/**
 * Снятие со склада: ключ отключается в движке, иначе «удалённый» ключ остался бы
 * рабочим. Деньги остаются на аккаунте — их всегда можно вернуть в оборот.
 */
export async function removeKey(id: string): Promise<boolean> {
  const { db } = getDatabase();
  const [row] = await db
    .select({ engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .where(and(eq(openkeysKeys.id, id), isNull(openkeysKeys.removedAt)))
    .limit(1);
  if (!row) return false;

  await getEngineClient().disableKey(row.engineKeyId);
  await db
    .update(openkeysKeys)
    .set({ removedAt: new Date(), status: "disabled", secretCiphertext: null, secretNonce: null })
    .where(eq(openkeysKeys.id, id));
  return true;
}

export interface BatchSummary {
  id: string;
  label: string | null;
  note: string | null;
  faceValueNano: bigint;
  multBp: number;
  quantity: number;
  createdAt: Date;
  createdBy: string;
}

export async function listBatches(limit = 50): Promise<BatchSummary[]> {
  const { db } = getDatabase();
  return db.select().from(openkeysBatches).orderBy(desc(openkeysBatches.createdAt)).limit(limit);
}
