import "server-only";
import { openkeysBatches, openkeysKeys } from "@claude-api/openkeys-db";
import { and, asc, count, eq, isNull, sql } from "drizzle-orm";
import { getDatabase } from "./db";
import { getEngineClient } from "./engine";

/**
 * Массовое управление ключами одного админа-издателя. Админов как таблицы не
 * существует: издатель — это `openkeys_batches.created_by`, имя из его собственной
 * сессии на openkeys.apitoken.sale. Поэтому и сводка, и действие идут по этому полю.
 *
 * Действия:
 *   pause  — обратимо: ключи переводятся в disabled (в движке и у нас);
 *   resume — обратная операция, поднимает только те, что стоят на паузе;
 *   revoke — необратимо: ключ отключается в движке и помечается removed
 *            (removed_by = проверенный actor единой админки), складской шифротекст
 *            стирается. Выданные покупателям ключи тоже аннулируются — сценарий
 *            «издатель скомпрометирован» иначе не закрывается.
 */
export const SELLER_ACTIONS = ["pause", "resume", "revoke"] as const;
export type SellerAction = (typeof SELLER_ACTIONS)[number];

/**
 * Потолок ключей на один вызов. Каждый ключ — отдельный вызов Control API, поэтому
 * без границы запрос жил бы минутами и упирался бы в таймаут прокси. Остаток
 * возвращается в `remaining`: повторное нажатие продолжает с того же места.
 */
export const SELLER_ACTION_LIMIT = 500;

/** Движок обрабатывает ключи параллельно, но без всплеска — как batch-чтение аккаунтов. */
const SELLER_ACTION_CONCURRENCY = 4;

/** Верхняя граница списка издателей: их единицы, лимит защищает от вырожденной базы. */
const SELLER_LIST_LIMIT = 200;

export interface AdminSellerSummary {
  createdBy: string;
  batches: number;
  /** Живые (не аннулированные) ключи издателя. */
  keys: number;
  active: number;
  disabled: number;
  delivered: number;
  stock: number;
  /** Уже аннулированные ключи — в каталоге их не видно, в сводке видно. */
  revoked: number;
  faceValueNano: string;
  lastIssuedAt: string | null;
}

export interface AdminSellerActionResult {
  createdBy: string;
  action: SellerAction;
  /** Сколько ключей подходило под действие на момент запроса. */
  matched: number;
  changed: number;
  failed: number;
  /** Осталось после потолка и ошибок — столько же обработает следующий клик. */
  remaining: number;
}

/** Секунда PostgreSQL и секунда Node — разные часы; наружу отдаём одинаковый ISO. */
function isoOrNull(value: unknown): string | null {
  if (value instanceof Date) return value.toISOString();
  if (typeof value === "string" && value) return new Date(value).toISOString();
  return null;
}

/**
 * Сводка по издателям. Считается одним SQL: без live-балансов, потому что решение
 * «поставить на паузу или аннулировать» принимается по составу выпуска, а не по
 * остатку на каждом ключе (остатки и так видны в каталоге ниже).
 */
export async function listAdminSellers(): Promise<AdminSellerSummary[]> {
  const { db } = getDatabase();
  const live = sql`${openkeysKeys.removedAt} is null`;
  const rows = await db
    .select({
      createdBy: openkeysBatches.createdBy,
      batches: sql<number>`count(distinct ${openkeysBatches.id})`.mapWith(Number),
      keys: sql<number>`count(${openkeysKeys.id}) filter (where ${live})`.mapWith(Number),
      active: sql<number>`count(${openkeysKeys.id}) filter (where ${live} and ${openkeysKeys.status} = 'active')`.mapWith(Number),
      disabled: sql<number>`count(${openkeysKeys.id}) filter (where ${live} and ${openkeysKeys.status} = 'disabled')`.mapWith(Number),
      delivered: sql<number>`count(${openkeysKeys.id}) filter (where ${live} and ${openkeysKeys.deliveredAt} is not null)`.mapWith(Number),
      stock: sql<number>`count(${openkeysKeys.id}) filter (where ${live} and ${openkeysKeys.deliveredAt} is null)`.mapWith(Number),
      revoked: sql<number>`count(${openkeysKeys.id}) filter (where ${openkeysKeys.removedAt} is not null)`.mapWith(Number),
      faceValueNano: sql<string>`coalesce(sum(${openkeysKeys.faceValueNano}) filter (where ${live}), 0)::text`,
      lastIssuedAt: sql<Date | string | null>`max(${openkeysKeys.createdAt})`,
    })
    .from(openkeysBatches)
    .leftJoin(openkeysKeys, eq(openkeysKeys.batchId, openkeysBatches.id))
    .groupBy(openkeysBatches.createdBy)
    .orderBy(asc(openkeysBatches.createdBy))
    .limit(SELLER_LIST_LIMIT);

  return rows.map((row) => ({
    createdBy: row.createdBy,
    batches: row.batches,
    keys: row.keys,
    active: row.active,
    disabled: row.disabled,
    delivered: row.delivered,
    stock: row.stock,
    revoked: row.revoked,
    faceValueNano: row.faceValueNano,
    lastIssuedAt: isoOrNull(row.lastIssuedAt),
  }));
}

/** Издатель существует, пока у него есть хотя бы одна партия — даже без живых ключей. */
export async function sellerExists(createdBy: string): Promise<boolean> {
  const { db } = getDatabase();
  const [row] = await db
    .select({ id: openkeysBatches.id })
    .from(openkeysBatches)
    .where(eq(openkeysBatches.createdBy, createdBy))
    .limit(1);
  return row !== undefined;
}

/** Под действие попадают только ключи в подходящем состоянии — повтор клика идемпотентен. */
function eligibility(action: SellerAction, createdBy: string) {
  return and(
    eq(openkeysBatches.createdBy, createdBy),
    isNull(openkeysKeys.removedAt),
    action === "pause" ? eq(openkeysKeys.status, "active") : undefined,
    action === "resume" ? eq(openkeysKeys.status, "disabled") : undefined,
  );
}

/** Один ключ: сначала движок (иначе отключённый у нас ключ продолжил бы тратить деньги), потом наша запись. */
async function applyOne(
  action: SellerAction,
  key: { id: string; engineKeyId: string },
  actor: string,
): Promise<void> {
  const { db } = getDatabase();
  const engine = getEngineClient();

  if (action === "resume") {
    await engine.setKeyStatus(key.engineKeyId, "active");
    await db
      .update(openkeysKeys)
      .set({ status: "active", disabledAt: null })
      .where(and(eq(openkeysKeys.id, key.id), isNull(openkeysKeys.removedAt)));
    return;
  }

  if (action === "pause") {
    await engine.setKeyStatus(key.engineKeyId, "disabled");
    await db
      .update(openkeysKeys)
      .set({ status: "disabled", disabledAt: new Date() })
      .where(and(eq(openkeysKeys.id, key.id), isNull(openkeysKeys.removedAt)));
    return;
  }

  await engine.disableKey(key.engineKeyId);
  await db
    .update(openkeysKeys)
    .set({
      status: "disabled",
      // Момент первого отключения сохраняем: аннулирование не переписывает историю паузы.
      disabledAt: sql`coalesce(${openkeysKeys.disabledAt}, now())`,
      removedAt: new Date(),
      removedBy: actor,
      removalReason: "bulk seller revoke",
      secretCiphertext: null,
      secretNonce: null,
    })
    .where(and(eq(openkeysKeys.id, key.id), isNull(openkeysKeys.removedAt)));
}

/**
 * Массовое действие по издателю. Частичный успех лучше отказа целиком: ключ, который
 * движок не принял, остаётся в прежнем состоянии и попадёт в следующую попытку, а
 * счётчики честно показывают, что произошло.
 */
export async function applySellerKeyAction(input: {
  createdBy: string;
  action: SellerAction;
  actor: string;
}): Promise<AdminSellerActionResult> {
  const { db } = getDatabase();
  const where = eligibility(input.action, input.createdBy);

  const [totals] = await db
    .select({ value: count() })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(where);
  const matched = totals?.value ?? 0;

  const targets = await db
    .select({ id: openkeysKeys.id, engineKeyId: openkeysKeys.engineKeyId })
    .from(openkeysKeys)
    .innerJoin(openkeysBatches, eq(openkeysKeys.batchId, openkeysBatches.id))
    .where(where)
    .orderBy(asc(openkeysKeys.createdAt))
    .limit(SELLER_ACTION_LIMIT);

  let changed = 0;
  let failed = 0;
  let cursor = 0;
  const workers = Array.from(
    { length: Math.min(SELLER_ACTION_CONCURRENCY, targets.length) },
    async () => {
      for (let index = cursor++; index < targets.length; index = cursor++) {
        try {
          await applyOne(input.action, targets[index], input.actor);
          changed += 1;
        } catch {
          // Ключ остаётся как был: обещать «аннулировано», не отключив его в движке,
          // опаснее, чем показать ошибку и дать нажать ещё раз.
          failed += 1;
        }
      }
    },
  );
  await Promise.all(workers);

  return {
    createdBy: input.createdBy,
    action: input.action,
    matched,
    changed,
    failed,
    remaining: Math.max(0, matched - changed),
  };
}
