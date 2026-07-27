import { sql } from "drizzle-orm";
import {
  bigint,
  check,
  index,
  integer,
  pgEnum,
  pgTable,
  text,
  timestamp,
  uniqueIndex,
  uuid,
} from "drizzle-orm/pg-core";

const createdAt = timestamp("created_at", { withTimezone: true }).notNull().defaultNow();

export const openkeysKeyStatus = pgEnum("openkeys_key_status", ["active", "disabled"]);

/**
 * Партия ключей, выпущенная админом под продажу (FunPay и т.п.).
 * face_value_nano — номинал ОДНОГО ключа в эквиваленте официального прайса
 * Anthropic; фактический баланс движка = face_value_nano * mult_bp / 10000.
 */
export const openkeysBatches = pgTable(
  "openkeys_batches",
  {
    id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
    label: text("label"),
    faceValueNano: bigint("face_value_nano", { mode: "bigint" }).notNull(),
    multBp: integer("mult_bp").notNull(),
    quantity: integer("quantity").notNull(),
    note: text("note"),
    createdBy: text("created_by").notNull(),
    createdAt,
  },
  (table) => [
    check("openkeys_batches_face_value_positive", sql`${table.faceValueNano} > 0`),
    check("openkeys_batches_mult_bp_range", sql`${table.multBp} BETWEEN 1 AND 10000`),
    check("openkeys_batches_quantity_range", sql`${table.quantity} BETWEEN 1 AND 100`),
    check("openkeys_batches_label_length", sql`${table.label} IS NULL OR char_length(${table.label}) <= 200`),
    check("openkeys_batches_note_length", sql`${table.note} IS NULL OR char_length(${table.note}) <= 2000`),
    check("openkeys_batches_created_by_length", sql`char_length(${table.createdBy}) BETWEEN 1 AND 128`),
  ],
);

/**
 * Один проданный ключ. Полный секрет `sk-pool-…` НЕ хранится: он показывается
 * админу один раз при выпуске. view_token — публичная непубликуемая ссылка на
 * страницу расхода, работает без авторизации.
 */
export const openkeysKeys = pgTable(
  "openkeys_keys",
  {
    id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
    batchId: uuid("batch_id")
      .notNull()
      .references(() => openkeysBatches.id, { onDelete: "restrict" }),
    viewToken: text("view_token").notNull(),
    engineAccountId: text("engine_account_id").notNull(),
    engineKeyId: text("engine_key_id").notNull(),
    keyMasked: text("key_masked").notNull(),
    /**
     * SHA-256 самого секрета. Сам ключ не храним, но по хешу можем узнать его,
     * когда покупатель придёт продлевать: тогда ключ можно привязать к обычному
     * аккаунту на основном сайте и увести человека в стандартный клиентский путь.
     */
    keySha256: text("key_sha256"),
    faceValueNano: bigint("face_value_nano", { mode: "bigint" }).notNull(),
    multBp: integer("mult_bp").notNull(),
    status: openkeysKeyStatus("status").notNull().default("active"),
    /**
     * Секрет в AES-256-GCM, чтобы ключ можно было выдать покупателю позже, а не
     * только в момент выпуска. Обнуляется, как только ключ помечен выданным или
     * снятым: дальше хранить его незачем.
     */
    secretCiphertext: text("secret_ciphertext"),
    secretNonce: text("secret_nonce"),
    /** Отмечен как переданный покупателю. */
    deliveredAt: timestamp("delivered_at", { withTimezone: true }),
    /** Снят со склада: ключ отключён в движке и больше не выдаётся. */
    removedAt: timestamp("removed_at", { withTimezone: true }),
    createdAt,
    disabledAt: timestamp("disabled_at", { withTimezone: true }),
  },
  (table) => [
    uniqueIndex("openkeys_keys_view_token_key").on(table.viewToken),
    uniqueIndex("openkeys_keys_engine_key_id_key").on(table.engineKeyId),
    uniqueIndex("openkeys_keys_key_sha256_key").on(table.keySha256),
    index("openkeys_keys_batch_id_idx").on(table.batchId),
    check("openkeys_keys_face_value_positive", sql`${table.faceValueNano} > 0`),
    check("openkeys_keys_mult_bp_range", sql`${table.multBp} BETWEEN 1 AND 10000`),
    check("openkeys_keys_view_token_shape", sql`${table.viewToken} ~ '^[A-Za-z0-9_-]{22}$'`),
    check(
      "openkeys_keys_secret_pair",
      sql`(${table.secretCiphertext} IS NULL) = (${table.secretNonce} IS NULL)`,
    ),
    check(
      "openkeys_keys_delivered_secret_cleared",
      sql`${table.deliveredAt} IS NULL OR (${table.secretCiphertext} IS NULL AND ${table.secretNonce} IS NULL)`,
    ),
    check(
      "openkeys_keys_disabled_timestamp",
      sql`${table.status} <> 'disabled' OR ${table.disabledAt} IS NOT NULL`,
    ),
  ],
);

export type OpenkeysBatchRow = typeof openkeysBatches.$inferSelect;
export type OpenkeysKeyRow = typeof openkeysKeys.$inferSelect;
