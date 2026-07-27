import "server-only";
import { randomBytes } from "node:crypto";
import { openkeysBatches, openkeysKeys } from "@claude-api/openkeys-db";
import { desc, eq } from "drizzle-orm";
import { loadConfig } from "./config";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";
import { balanceToOfficialNano, officialNanoToBalance } from "./money";

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
  /** Полный секрет. Отдаётся админу ОДИН раз и нигде не сохраняется. */
  secret: string;
  viewToken: string;
  viewUrl: string;
  keyMasked: string;
}

function maskKey(secret: string): string {
  return `${secret.slice(0, 12)}…${secret.slice(-4)}`;
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

    await db.insert(openkeysKeys).values({
      batchId: batch.id,
      viewToken,
      engineAccountId: account.account,
      engineKeyId: key.key_id,
      keyMasked: maskKey(key.key),
      faceValueNano: input.faceValueNano,
      multBp: input.multBp,
    });

    issued.push({
      secret: key.key,
      viewToken,
      viewUrl: `${config.publicBaseUrl}/u/${viewToken}`,
      keyMasked: maskKey(key.key),
    });
  }

  return { batchId: batch.id, keys: issued };
}

export interface KeyUsageView {
  viewToken: string;
  keyMasked: string;
  status: "active" | "disabled";
  createdAt: Date;
  faceValueNano: bigint;
  multBp: number;
  balanceNano: bigint;
  spentNano: bigint;
  /** Остаток и расход, пересчитанные в официальный прайс Anthropic. */
  officialRemainingNano: bigint;
  officialSpentNano: bigint;
}

export async function loadUsageByViewToken(viewToken: string): Promise<KeyUsageView | null> {
  const { db } = getDatabase();
  const [row] = await db.select().from(openkeysKeys).where(eq(openkeysKeys.viewToken, viewToken)).limit(1);
  if (!row) return null;

  const account = await getEngineClient().getAccount(row.engineAccountId);
  const balanceNano = BigInt(account.balance_nano);
  const spentNano = BigInt(account.spent_nano);

  return {
    viewToken: row.viewToken,
    keyMasked: row.keyMasked,
    status: account.status === "disabled" ? "disabled" : row.status,
    createdAt: row.createdAt,
    faceValueNano: row.faceValueNano,
    multBp: row.multBp,
    balanceNano,
    spentNano,
    officialRemainingNano: balanceToOfficialNano(balanceNano, row.multBp),
    officialSpentNano: balanceToOfficialNano(spentNano, row.multBp),
  };
}

/**
 * Вход по самому ключу: спрашиваем движок его же публичным /balance, чтобы
 * секрет не сверялся с нашей БД (мы его не храним) и не попадал в логи.
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
