import { sql } from "drizzle-orm";
import {
  bigint,
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
export const openkeysBatches = pgTable("openkeys_batches", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  label: text("label"),
  faceValueNano: bigint("face_value_nano", { mode: "bigint" }).notNull(),
  multBp: integer("mult_bp").notNull(),
  quantity: integer("quantity").notNull(),
  note: text("note"),
  createdBy: text("created_by").notNull(),
  createdAt,
});

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
    createdAt,
    disabledAt: timestamp("disabled_at", { withTimezone: true }),
  },
  (table) => [
    uniqueIndex("openkeys_keys_view_token_key").on(table.viewToken),
    uniqueIndex("openkeys_keys_engine_key_id_key").on(table.engineKeyId),
    uniqueIndex("openkeys_keys_key_sha256_key").on(table.keySha256),
    index("openkeys_keys_batch_id_idx").on(table.batchId),
  ],
);

export type OpenkeysBatchRow = typeof openkeysBatches.$inferSelect;
export type OpenkeysKeyRow = typeof openkeysKeys.$inferSelect;
