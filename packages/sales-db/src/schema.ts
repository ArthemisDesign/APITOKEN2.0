import {
  bigint,
  bigserial,
  boolean,
  check,
  index,
  integer,
  jsonb,
  pgEnum,
  pgTable,
  text,
  timestamp,
  uniqueIndex,
  uuid,
  type AnyPgColumn,
} from "drizzle-orm/pg-core";
import { sql } from "drizzle-orm";

const createdAt = timestamp("created_at", { withTimezone: true }).notNull().defaultNow();
const updatedAt = timestamp("updated_at", { withTimezone: true }).notNull().defaultNow();

export const partnerStatus = pgEnum("partner_status", ["active", "suspended", "pending"]);
export const partnerAuthTokenPurpose = pgEnum("partner_auth_token_purpose", ["verify_email", "reset_password"]);
export const partnerEmailStatus = pgEnum("partner_email_status", ["pending", "sending", "sent", "failed"]);
export const payoutStatus = pgEnum("payout_status", ["requested", "approved", "paid", "rejected"]);
export const partnerApplicationStatus = pgEnum("partner_application_status", ["pending", "approved", "rejected"]);
export const promoCodeStatus = pgEnum("promo_code_status", ["active", "redeemed", "disabled"]);

export const partners = pgTable("partners", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  // Онбординг через Telegram: identity = telegram_id. email/password_hash — legacy
  // (партнёры первой волны), для новых NULL.
  email: text("email"),
  displayName: text("display_name"),
  passwordHash: text("password_hash"),
  telegramId: bigint("telegram_id", { mode: "bigint" }),
  telegramUsername: text("telegram_username"),
  telegramPhotoUrl: text("telegram_photo_url"),
  status: partnerStatus("status").notNull().default("pending"),
  emailVerified: boolean("email_verified").notNull().default(false),
  referralCode: text("referral_code").notNull(),
  parentPartnerId: uuid("parent_partner_id").references((): AnyPgColumn => partners.id, { onDelete: "restrict" }),
  commissionBps: integer("commission_bps").notNull().default(1000),
  subCommissionBps: integer("sub_commission_bps").notNull().default(1000),
  payoutMethod: text("payout_method"),
  payoutDetails: jsonb("payout_details"),
  // Промокоды: по умолчанию доступа нет. Админ включает и задаёт лимиты — макс. номинал кода
  // и макс. количество кодов, которые партнёр может создать.
  promoEnabled: boolean("promo_enabled").notNull().default(false),
  promoMaxValueNano: bigint("promo_max_value_nano", { mode: "bigint" }).notNull().default(sql`0`),
  promoMaxCount: integer("promo_max_count").notNull().default(0),
  // Скидка, которую сейлз даёт своим рефам как «пол» цены (реф остаётся b2c на обычных тирах, но
  // платит не дороже этой скидки). Действует только при referralDiscountEnabled. Максимум 90%.
  referralDiscountBps: integer("referral_discount_bps").notNull().default(0),
  // Право давать скидку — выдаётся админом (или каскадом от партнёра с правом). По умолчанию нет.
  referralDiscountEnabled: boolean("referral_discount_enabled").notNull().default(false),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("partners_email_lower_uidx").on(sql`lower(${table.email})`).where(sql`${table.email} IS NOT NULL`),
  uniqueIndex("partners_telegram_id_uidx").on(table.telegramId).where(sql`${table.telegramId} IS NOT NULL`),
  uniqueIndex("partners_referral_code_uidx").on(table.referralCode),
  index("partners_parent_idx").on(table.parentPartnerId),
  check("partners_commission_bps_check", sql`${table.commissionBps} BETWEEN 0 AND 10000`),
  check("partners_sub_commission_bps_check", sql`${table.subCommissionBps} BETWEEN 0 AND 10000`),
  check("partners_referral_discount_check", sql`${table.referralDiscountBps} BETWEEN 0 AND 9000`),
]);

export const partnerSessions = pgTable("partner_sessions", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  tokenHash: text("token_hash").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  lastSeenAt: timestamp("last_seen_at", { withTimezone: true }).notNull().defaultNow(),
  revokedAt: timestamp("revoked_at", { withTimezone: true }),
  userAgent: text("user_agent"),
  ipAddress: text("ip_address"),
  createdAt,
}, (table) => [
  uniqueIndex("partner_sessions_token_hash_uidx").on(table.tokenHash),
  index("partner_sessions_partner_idx").on(table.partnerId, table.createdAt),
  index("partner_sessions_expiry_idx").on(table.expiresAt),
]);

export const partnerAuthTokens = pgTable("partner_auth_tokens", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  purpose: partnerAuthTokenPurpose("purpose").notNull(),
  tokenHash: text("token_hash").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  usedAt: timestamp("used_at", { withTimezone: true }),
  createdAt,
}, (table) => [
  uniqueIndex("partner_auth_tokens_token_hash_uidx").on(table.tokenHash),
  index("partner_auth_tokens_partner_purpose_idx").on(table.partnerId, table.purpose, table.createdAt),
]);

export const partnerRateLimits = pgTable("partner_rate_limits", {
  keyHash: text("key_hash").primaryKey(),
  attempts: integer("attempts").notNull().default(0),
  windowStartedAt: timestamp("window_started_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt,
});

export const partnerEmailOutbox = pgTable("partner_email_outbox", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  recipient: text("recipient").notNull(),
  template: text("template").notNull(),
  payload: jsonb("payload").notNull().default({}),
  status: partnerEmailStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  sentAt: timestamp("sent_at", { withTimezone: true }),
  lastError: text("last_error"),
  createdAt,
  updatedAt,
}, (table) => [index("partner_email_outbox_claim_idx").on(table.status, table.nextAttemptAt)]);

export const partnerInvites = pgTable("partner_invites", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  // NULL partner_id = корневой инвайт из админки (партнёр без родителя).
  partnerId: uuid("partner_id").references(() => partners.id, { onDelete: "restrict" }),
  code: text("code").notNull(),
  // Инвайт привязан к Telegram-юзернейму (нормализован: без @, lower). Регистрация — только
  // если username вошедшего совпал.
  telegramUsername: text("telegram_username"),
  commissionBps: integer("commission_bps"),
  subCommissionBps: integer("sub_commission_bps"),
  // Промо-доступ и B2B-скидка рефа, зашитые в инвайт — применяются при создании партнёра.
  promoEnabled: boolean("promo_enabled").notNull().default(false),
  promoMaxValueNano: bigint("promo_max_value_nano", { mode: "bigint" }).notNull().default(sql`0`),
  promoMaxCount: integer("promo_max_count").notNull().default(0),
  referralDiscountBps: integer("referral_discount_bps").notNull().default(0),
  referralDiscountEnabled: boolean("referral_discount_enabled").notNull().default(false),
  expiresAt: timestamp("expires_at", { withTimezone: true }),
  consumedAt: timestamp("consumed_at", { withTimezone: true }),
  consumedByPartnerId: uuid("consumed_by_partner_id").references(() => partners.id, { onDelete: "restrict" }),
  createdAt,
}, (table) => [
  uniqueIndex("partner_invites_code_uidx").on(table.code),
  index("partner_invites_partner_idx").on(table.partnerId, table.createdAt),
  check("partner_invites_commission_bps_check", sql`${table.commissionBps} IS NULL OR ${table.commissionBps} BETWEEN 0 AND 10000`),
  check("partner_invites_sub_commission_bps_check", sql`${table.subCommissionBps} IS NULL OR ${table.subCommissionBps} BETWEEN 0 AND 10000`),
  check("partner_invites_referral_discount_check", sql`${table.referralDiscountBps} BETWEEN 0 AND 9000`),
]);

// Заявки «с улицы»: подписанный Telegram-вход без аккаунта и инвайта → заявка на
// рассмотрение. Approve создаёт партнёра сразу (telegram_id уже проверен подписью).
export const partnerApplications = pgTable("partner_applications", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  telegramId: bigint("telegram_id", { mode: "bigint" }).notNull(),
  telegramUsername: text("telegram_username"),
  displayName: text("display_name"),
  telegramPhotoUrl: text("telegram_photo_url"),
  note: text("note"),
  status: partnerApplicationStatus("status").notNull().default("pending"),
  adminNote: text("admin_note"),
  createdPartnerId: uuid("created_partner_id").references(() => partners.id, { onDelete: "restrict" }),
  createdAt,
  decidedAt: timestamp("decided_at", { withTimezone: true }),
}, (table) => [
  uniqueIndex("partner_applications_pending_tg_uidx").on(table.telegramId).where(sql`${table.status} = 'pending'`),
  index("partner_applications_status_idx").on(table.status, table.createdAt),
]);

// Промокоды партнёра. Одноразовые: код погашается один раз, кредитует юзеру наш баланс на
// value_nano. Погашение — через internal-эндпоинт (commerce → sales-api). Один юзер может
// погасить не более одного промо (redeemed_by уникален).
export const promoCodes = pgTable("promo_codes", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  code: text("code").notNull(),
  valueNano: bigint("value_nano", { mode: "bigint" }).notNull(),
  status: promoCodeStatus("status").notNull().default("active"),
  redeemedByCommerceUserId: uuid("redeemed_by_commerce_user_id"),
  redeemedAt: timestamp("redeemed_at", { withTimezone: true }),
  // Идемпотентный ref для кредита движка на стороне commerce.
  redemptionRef: text("redemption_ref"),
  // Спец-скидка промокода: >0 → редимщик получает скидку-«пол» (b2c + floor). 0 = обычный промо.
  discountBps: integer("discount_bps").notNull().default(0),
  createdAt,
}, (table) => [
  uniqueIndex("promo_codes_code_uidx").on(sql`upper(${table.code})`),
  uniqueIndex("promo_codes_redemption_ref_uidx").on(table.redemptionRef).where(sql`${table.redemptionRef} IS NOT NULL`),
  uniqueIndex("promo_codes_redeemed_by_uidx").on(table.redeemedByCommerceUserId).where(sql`${table.redeemedByCommerceUserId} IS NOT NULL`),
  index("promo_codes_partner_idx").on(table.partnerId, table.createdAt),
  check("promo_codes_value_check", sql`${table.valueNano} > 0`),
]);

export const referredUsers = pgTable("referred_users", {
  commerceUserId: uuid("commerce_user_id").primaryKey(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  referralCode: text("referral_code"),
  attributedAt: timestamp("attributed_at", { withTimezone: true }).notNull().defaultNow(),
  sourceAttributionId: bigint("source_attribution_id", { mode: "bigint" }),
}, (table) => [
  uniqueIndex("referred_users_source_attribution_uidx").on(table.sourceAttributionId)
    .where(sql`${table.sourceAttributionId} IS NOT NULL`),
  index("referred_users_partner_idx").on(table.partnerId, table.attributedAt),
]);

// Персональные одноразовые ссылки со скидкой. Партнёр (с правом) выпускает под конкретного клиента;
// первый привязанный по коду пользователь получает discount_bps как «пол» цены и гасит ссылку.
export const partnerDiscountLinks = pgTable("partner_discount_links", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  code: text("code").notNull(),
  discountBps: integer("discount_bps").notNull(),
  // Подпись «для кого» — необязательная заметка партнёра.
  note: text("note"),
  consumedByCommerceUserId: uuid("consumed_by_commerce_user_id"),
  consumedAt: timestamp("consumed_at", { withTimezone: true }),
  createdAt,
}, (table) => [
  uniqueIndex("partner_discount_links_code_uidx").on(table.code),
  index("partner_discount_links_partner_idx").on(table.partnerId, table.createdAt),
  check("partner_discount_links_discount_check", sql`${table.discountBps} BETWEEN 0 AND 9000`),
]);

export const syncCursors = pgTable("sync_cursors", {
  feed: text("feed").primaryKey(),
  lastId: bigint("last_id", { mode: "bigint" }).notNull().default(sql`0`),
  updatedAt,
}, (table) => [
  check("sync_cursors_feed_check", sql`${table.feed} IN ('attributions', 'usage_events', 'topups')`),
]);

export const partnerUsageEvents = pgTable("partner_usage_events", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commerceEventId: bigint("commerce_event_id", { mode: "bigint" }).notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  importedAt: timestamp("imported_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  uniqueIndex("partner_usage_events_commerce_event_uidx").on(table.commerceEventId),
  index("partner_usage_events_partner_time_idx").on(table.partnerId, table.occurredAt),
  index("partner_usage_events_user_idx").on(table.commerceUserId),
  check("partner_usage_events_amount_check", sql`${table.amountNano} > 0`),
]);

export const referredTopups = pgTable("referred_topups", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commercePaymentId: text("commerce_payment_id").notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  paidAt: timestamp("paid_at", { withTimezone: true }).notNull(),
}, (table) => [
  uniqueIndex("referred_topups_commerce_payment_uidx").on(table.commercePaymentId),
  index("referred_topups_partner_idx").on(table.partnerId, table.paidAt),
  check("referred_topups_amount_check", sql`${table.amountNano} > 0`),
]);

export const commissionEntries = pgTable("commission_entries", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  // Источник комиссии = депозит (topup). usage_event_id — legacy-путь (списания), больше не
  // пишется; ровно один источник у строки (см. one_source CHECK ниже).
  usageEventId: bigint("usage_event_id", { mode: "bigint" })
    .references(() => partnerUsageEvents.id, { onDelete: "restrict" }),
  topupId: bigint("topup_id", { mode: "bigint" })
    .references(() => referredTopups.id, { onDelete: "restrict" }),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  level: integer("level").notNull(),
  appliedBps: integer("applied_bps").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("commission_entries_usage_partner_uidx").on(table.usageEventId, table.partnerId)
    .where(sql`${table.usageEventId} IS NOT NULL`),
  uniqueIndex("commission_entries_topup_partner_uidx").on(table.topupId, table.partnerId)
    .where(sql`${table.topupId} IS NOT NULL`),
  index("commission_entries_partner_time_idx").on(table.partnerId, table.createdAt),
  check("commission_entries_level_check", sql`${table.level} BETWEEN 0 AND 10`),
  check("commission_entries_applied_bps_check", sql`${table.appliedBps} BETWEEN 0 AND 10000`),
  check("commission_entries_amount_check", sql`${table.amountNano} > 0`),
  check("commission_entries_one_source_check",
    sql`((${table.usageEventId} IS NOT NULL)::int + (${table.topupId} IS NOT NULL)::int) = 1`),
]);

export const payouts = pgTable("payouts", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  status: payoutStatus("status").notNull().default("requested"),
  method: text("method").notNull(),
  details: jsonb("details").notNull().default({}),
  requestedAt: timestamp("requested_at", { withTimezone: true }).notNull().defaultNow(),
  decidedAt: timestamp("decided_at", { withTimezone: true }),
  paidAt: timestamp("paid_at", { withTimezone: true }),
  adminNote: text("admin_note"),
}, (table) => [
  index("payouts_partner_idx").on(table.partnerId, table.requestedAt),
  index("payouts_status_idx").on(table.status, table.requestedAt),
  check("payouts_amount_check", sql`${table.amountNano} > 0`),
]);

export const salesAuditLog = pgTable("sales_audit_log", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  actorType: text("actor_type").notNull(),
  actorId: text("actor_id"),
  action: text("action").notNull(),
  targetType: text("target_type").notNull(),
  targetId: text("target_id").notNull(),
  metadata: jsonb("metadata").notNull().default({}),
  createdAt,
}, (table) => [index("sales_audit_log_target_idx").on(table.targetType, table.targetId, table.createdAt)]);

// Буфер спенд/депозит-событий, пришедших РАНЬШЕ атрибуции их пользователя (кросс-фидовая задержка
// видимости). Записываются здесь вместо «тихо потерять»; reconcile проигрывает их (идемпотентно),
// как только юзер появляется в referred_users, и удаляет строку. См. аудит 2026-07-20 (D1).
export const pendingReferralEvents = pgTable("pending_referral_events", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  kind: text("kind").notNull(), // 'spend' | 'deposit'
  commerceRef: text("commerce_ref").notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("pending_referral_events_kind_ref_uidx").on(table.kind, table.commerceRef),
  index("pending_referral_events_user_idx").on(table.commerceUserId),
]);
