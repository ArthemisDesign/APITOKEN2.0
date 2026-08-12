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
  unique,
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
  // Legacy partner-attribution marker ceiling. The field and permission remain for expand-only
  // contracts and historical rows, but they do not change Commerce/engine pricing.
  referralDiscountBps: integer("referral_discount_bps").notNull().default(0),
  // Legacy writer permission. Current product UI does not grant or market it as a discount.
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
  check("partners_referral_discount_check", sql`${table.referralDiscountBps} BETWEEN 0 AND 9500`),
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
  // Promo access plus retained referral-marker permission/ceiling copied into a new partner.
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
  check("partner_invites_referral_discount_check", sql`${table.referralDiscountBps} BETWEEN 0 AND 9500`),
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
  // Legacy attribution marker carried by a promo. It never changes the redeemer's price.
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

// Legacy one-time attribution links. The first matching user consumes the code and the stored bps
// is retained as audit/display metadata only; it never changes Commerce/engine pricing.
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
  check("partner_discount_links_discount_check", sql`${table.discountBps} BETWEEN 0 AND 9500`),
]);

export const syncCursors = pgTable("sync_cursors", {
  feed: text("feed").primaryKey(),
  lastId: bigint("last_id", { mode: "bigint" }).notNull().default(sql`0`),
  updatedAt,
}, (table) => [
  check("sync_cursors_feed_v3_check", sql`${table.feed} IN ('attributions', 'usage_events', 'topups', 'topups_v2', 'topup_funding_lots', 'payment_reversals')`),
]);

export const partnerUsageEvents = pgTable("partner_usage_events", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commerceEventId: bigint("commerce_event_id", { mode: "bigint" }).notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  providerId: text("provider_id"),
  accountClass: text("account_class"),
  pricingMode: text("pricing_mode"),
  paidFundedNano: bigint("paid_funded_nano", { mode: "bigint" }),
  commissionEligible: boolean("commission_eligible"),
  snapshotDigest: text("snapshot_digest"),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  importedAt: timestamp("imported_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  uniqueIndex("partner_usage_events_commerce_event_uidx").on(table.commerceEventId),
  index("partner_usage_events_partner_time_idx").on(table.partnerId, table.occurredAt),
  index("partner_usage_events_user_idx").on(table.commerceUserId),
  check("partner_usage_events_amount_check", sql`${table.amountNano} > 0`),
  check("partner_usage_events_multi_discount_check", sql`
    (
      ${table.providerId} IS NULL
      AND ${table.accountClass} IS NULL
      AND ${table.pricingMode} IS NULL
      AND ${table.paidFundedNano} IS NULL
      AND ${table.commissionEligible} IS NULL
      AND ${table.snapshotDigest} IS NULL
    )
    OR (
      ${table.providerId} IS NOT NULL
      AND ${table.providerId} <> ''
      AND ${table.accountClass} IN ('b2c', 'b2b', 'open_keys', 'service')
      AND ${table.pricingMode} IN ('track', 'discount')
      AND ${table.paidFundedNano} IS NOT NULL
      AND ${table.paidFundedNano} > 0
      AND ${table.amountNano} = ${table.paidFundedNano}
      AND ${table.commissionEligible} IS NOT NULL
      AND (NOT ${table.commissionEligible} OR (
        ${table.pricingMode} = 'track'
        AND ${table.accountClass} = 'b2c'
      ))
      AND ${table.snapshotDigest} IS NOT NULL
      AND ${table.snapshotDigest} <> ''
    )
  `),
  check("partner_usage_events_commission_authority_check", sql`
    ${table.providerId} IS NULL
    OR (
      ${table.accountClass} = 'b2c'
      AND ${table.pricingMode} = 'track'
      AND ${table.commissionEligible} IS TRUE
      AND ${table.paidFundedNano} = ${table.amountNano}
    )
  `),
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
  // Текущий источник комиссии = eligible paid-funded usage_event. topup_id сохраняется для
  // исторической совместимости; ровно один источник у строки (см. one_source CHECK ниже).
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

// Schema-v2 paid-funding authority. The migration checkpoint declares the shape only; the
// dual-schema feed consumer is delivered after this expansion is green in production.
export const partnerUsageEventsV2 = pgTable("partner_usage_events_v2", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commerceEventId: bigint("commerce_event_id", { mode: "bigint" }).notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  providerId: text("provider_id").notNull(),
  accountClass: text("account_class").notNull(),
  officialNano: bigint("official_nano", { mode: "bigint" }).notNull(),
  chargedNano: bigint("charged_nano", { mode: "bigint" }).notNull(),
  paidFundedNano: bigint("paid_funded_nano", { mode: "bigint" }).notNull(),
  bonusFundedNano: bigint("bonus_funded_nano", { mode: "bigint" }).notNull(),
  otherFundedNano: bigint("other_funded_nano", { mode: "bigint" }).notNull(),
  commissionEligible: boolean("commission_eligible").notNull(),
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  releaseDigest: text("release_digest").notNull(),
  snapshotDigest: text("snapshot_digest").notNull(),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  importedAt: timestamp("imported_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  unique("partner_usage_events_v2_commerce_event_id_key").on(table.commerceEventId),
  index("partner_usage_events_v2_partner_time_idx").on(table.partnerId, table.occurredAt),
  index("partner_usage_events_v2_user_idx").on(table.commerceUserId, table.commerceEventId),
  check("partner_usage_events_v2_shape_check", sql`
    ${table.providerId} <> ''
    AND ${table.accountClass} IN ('b2c', 'b2b', 'openkeys', 'service')
    AND ${table.officialNano} >= 0
    AND ${table.chargedNano} >= 0
    AND ${table.paidFundedNano} >= 0
    AND ${table.bonusFundedNano} >= 0
    AND ${table.otherFundedNano} >= 0
    AND ${table.paidFundedNano} + ${table.bonusFundedNano} + ${table.otherFundedNano} = ${table.chargedNano}
    AND ${table.releaseGeneration} > 0
    AND ${table.releaseDigest} <> ''
    AND ${table.snapshotDigest} <> ''
    AND (
      NOT ${table.commissionEligible}
      OR (${table.accountClass} = 'b2c' AND ${table.paidFundedNano} > 0)
    )
  `),
]);

export const pendingReferralUsageEventsV2 = pgTable("pending_referral_usage_events_v2", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commerceRef: text("commerce_ref").notNull(),
  commerceEventId: bigint("commerce_event_id", { mode: "bigint" }).notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  providerId: text("provider_id").notNull(),
  accountClass: text("account_class").notNull(),
  officialNano: bigint("official_nano", { mode: "bigint" }).notNull(),
  chargedNano: bigint("charged_nano", { mode: "bigint" }).notNull(),
  paidFundedNano: bigint("paid_funded_nano", { mode: "bigint" }).notNull(),
  bonusFundedNano: bigint("bonus_funded_nano", { mode: "bigint" }).notNull(),
  otherFundedNano: bigint("other_funded_nano", { mode: "bigint" }).notNull(),
  commissionEligible: boolean("commission_eligible").notNull(),
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  releaseDigest: text("release_digest").notNull(),
  snapshotDigest: text("snapshot_digest").notNull(),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  unique("pending_referral_usage_events_v2_commerce_ref_key").on(table.commerceRef),
  unique("pending_referral_usage_events_v2_commerce_event_id_key").on(table.commerceEventId),
  index("pending_referral_usage_events_v2_user_idx").on(table.commerceUserId, table.commerceEventId),
  check("pending_referral_usage_events_v2_shape_check", sql`
    ${table.commerceRef} <> ''
    AND ${table.providerId} <> ''
    AND ${table.accountClass} IN ('b2c', 'b2b', 'openkeys', 'service')
    AND ${table.officialNano} >= 0
    AND ${table.chargedNano} >= 0
    AND ${table.paidFundedNano} >= 0
    AND ${table.bonusFundedNano} >= 0
    AND ${table.otherFundedNano} >= 0
    AND ${table.paidFundedNano} + ${table.bonusFundedNano} + ${table.otherFundedNano} = ${table.chargedNano}
    AND ${table.releaseGeneration} > 0
    AND ${table.releaseDigest} <> ''
    AND ${table.snapshotDigest} <> ''
    AND (
      NOT ${table.commissionEligible}
      OR (${table.accountClass} = 'b2c' AND ${table.paidFundedNano} > 0)
    )
  `),
]);

export const commissionEntriesV2 = pgTable("commission_entries_v2", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  usageEventId: bigint("usage_event_id", { mode: "bigint" }).notNull()
    .references(() => partnerUsageEventsV2.id, { onDelete: "restrict" }),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  level: integer("level").notNull(),
  appliedBps: integer("applied_bps").notNull(),
  basePaidFundedNano: bigint("base_paid_funded_nano", { mode: "bigint" }).notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  createdAt,
}, (table) => [
  unique("commission_entries_v2_source_partner_unique")
    .on(table.usageEventId, table.partnerId),
  unique("commission_entries_v2_source_level_unique")
    .on(table.usageEventId, table.level),
  index("commission_entries_v2_partner_time_idx").on(table.partnerId, table.createdAt),
  check("commission_entries_v2_shape_check", sql`
    ${table.level} BETWEEN 0 AND 10
    AND ${table.appliedBps} BETWEEN 0 AND 10000
    AND ${table.basePaidFundedNano} > 0
    AND ${table.amountNano} > 0
    AND ${table.amountNano} <= ${table.basePaidFundedNano}
  `),
]);

// Reversal-accounting authority from migrations 0017/0018. The consumer/backfill writes this
// immutable evidence; signed earnings and payout readers ship in the next explicit checkpoint.
export const partnerPaidFundingLots = pgTable("partner_paid_funding_lots", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  referredTopupId: bigint("referred_topup_id", { mode: "bigint" }).notNull()
    .references(() => referredTopups.id, { onDelete: "restrict" }),
  commerceTopupId: bigint("commerce_topup_id", { mode: "bigint" }).notNull(),
  commercePaymentId: text("commerce_payment_id").notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  originalAmountNano: bigint("original_amount_nano", { mode: "bigint" }).notNull(),
  paidAt: timestamp("paid_at", { withTimezone: true }).notNull(),
  importedAt: timestamp("imported_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  unique("partner_paid_funding_lots_referred_topup_id_key").on(table.referredTopupId),
  unique("partner_paid_funding_lots_commerce_topup_id_key").on(table.commerceTopupId),
  unique("partner_paid_funding_lots_commerce_payment_id_key").on(table.commercePaymentId),
  index("partner_paid_funding_lots_user_fifo_idx")
    .on(table.commerceUserId, table.commerceTopupId),
  check("partner_paid_funding_lots_shape_check", sql`
    ${table.commerceTopupId} > 0
    AND ${table.commercePaymentId} <> ''
    AND ${table.originalAmountNano} > 0
  `),
]);

export const partnerUsageFundingAllocations = pgTable("partner_usage_funding_allocations", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  fundingLotId: bigint("funding_lot_id", { mode: "bigint" }).notNull()
    .references(() => partnerPaidFundingLots.id, { onDelete: "restrict" }),
  usageEventId: bigint("usage_event_id", { mode: "bigint" })
    .references(() => partnerUsageEvents.id, { onDelete: "restrict" }),
  usageEventV2Id: bigint("usage_event_v2_id", { mode: "bigint" })
    .references(() => partnerUsageEventsV2.id, { onDelete: "restrict" }),
  allocatedPaidNano: bigint("allocated_paid_nano", { mode: "bigint" }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("partner_usage_funding_alloc_v1_uidx")
    .on(table.fundingLotId, table.usageEventId)
    .where(sql`${table.usageEventId} IS NOT NULL`),
  uniqueIndex("partner_usage_funding_alloc_v2_uidx")
    .on(table.fundingLotId, table.usageEventV2Id)
    .where(sql`${table.usageEventV2Id} IS NOT NULL`),
  index("partner_usage_funding_alloc_lot_idx").on(table.fundingLotId),
  check("partner_usage_funding_alloc_one_source_check", sql`
    ((${table.usageEventId} IS NOT NULL)::int
      + (${table.usageEventV2Id} IS NOT NULL)::int) = 1
  `),
  check("partner_usage_funding_alloc_amount_check", sql`${table.allocatedPaidNano} > 0`),
]);

export const partnerCommissionFundingAllocations = pgTable("partner_commission_funding_allocations", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  usageFundingAllocationId: bigint("usage_funding_allocation_id", { mode: "bigint" }).notNull()
    .references(() => partnerUsageFundingAllocations.id, { onDelete: "restrict" }),
  commissionEntryId: bigint("commission_entry_id", { mode: "bigint" })
    .references(() => commissionEntries.id, { onDelete: "restrict" }),
  commissionEntryV2Id: bigint("commission_entry_v2_id", { mode: "bigint" })
    .references(() => commissionEntriesV2.id, { onDelete: "restrict" }),
  allocatedCommissionNano: bigint("allocated_commission_nano", { mode: "bigint" }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("partner_commission_funding_v1_uidx")
    .on(table.usageFundingAllocationId, table.commissionEntryId)
    .where(sql`${table.commissionEntryId} IS NOT NULL`),
  uniqueIndex("partner_commission_funding_v2_uidx")
    .on(table.usageFundingAllocationId, table.commissionEntryV2Id)
    .where(sql`${table.commissionEntryV2Id} IS NOT NULL`),
  check("partner_commission_funding_one_source_check", sql`
    ((${table.commissionEntryId} IS NOT NULL)::int
      + (${table.commissionEntryV2Id} IS NOT NULL)::int) = 1
  `),
  check("partner_commission_funding_amount_check", sql`${table.allocatedCommissionNano} >= 0`),
]);

export const partnerPaymentReversals = pgTable("partner_payment_reversals", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  commerceReversalId: bigint("commerce_reversal_id", { mode: "bigint" }).notNull(),
  fundingLotId: bigint("funding_lot_id", { mode: "bigint" }).notNull()
    .references(() => partnerPaidFundingLots.id, { onDelete: "restrict" }),
  commercePaymentId: text("commerce_payment_id").notNull(),
  commerceUserId: uuid("commerce_user_id").notNull(),
  kind: text("kind").notNull(),
  originalAmountNano: bigint("original_amount_nano", { mode: "bigint" }).notNull(),
  reversedAt: timestamp("reversed_at", { withTimezone: true }).notNull(),
  importedAt: timestamp("imported_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  unique("partner_payment_reversals_commerce_reversal_id_key").on(table.commerceReversalId),
  unique("partner_payment_reversals_funding_lot_id_key").on(table.fundingLotId),
  unique("partner_payment_reversals_commerce_payment_id_key").on(table.commercePaymentId),
  index("partner_payment_reversals_time_idx").on(table.reversedAt, table.commerceReversalId),
  check("partner_payment_reversals_shape_check", sql`
    ${table.commerceReversalId} > 0
    AND ${table.commercePaymentId} <> ''
    AND ${table.kind} IN ('refund', 'dispute')
    AND ${table.originalAmountNano} > 0
  `),
]);

export const partnerCommissionAdjustments = pgTable("partner_commission_adjustments", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  reversalId: bigint("reversal_id", { mode: "bigint" }).notNull()
    .references(() => partnerPaymentReversals.id, { onDelete: "restrict" }),
  commissionFundingAllocationId: bigint("commission_funding_allocation_id", { mode: "bigint" })
    .notNull()
    .references(() => partnerCommissionFundingAllocations.id, { onDelete: "restrict" }),
  partnerId: uuid("partner_id").notNull().references(() => partners.id, { onDelete: "restrict" }),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  effectiveAt: timestamp("effective_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  unique("partner_commission_adjustments_funding_allocation_key")
    .on(table.commissionFundingAllocationId),
  unique("partner_commission_adjustments_source_unique")
    .on(table.reversalId, table.commissionFundingAllocationId),
  index("partner_commission_adjustments_partner_time_idx")
    .on(table.partnerId, table.effectiveAt),
  check("partner_commission_adjustments_amount_check", sql`${table.amountNano} < 0`),
]);

// Один прогон пакетных on-chain выплат: prepare → (админ смотрит) → send. Отправка возможна ТОЛЬКО
// в 3-дневное окно выплат (жёсткий гейт на бэкенде).
export const payoutBatches = pgTable("payout_batches", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  // preparing | prepared | sending | sent | failed | canceled
  status: text("status").notNull().default("prepared"),
  hotWalletAddress: text("hot_wallet_address"),
  totalNano: bigint("total_nano", { mode: "bigint" }).notNull().default(0n),
  recipientCount: integer("recipient_count").notNull().default(0),
  gasPriceGwei: text("gas_price_gwei"),
  minNano: bigint("min_nano", { mode: "bigint" }).notNull().default(0n),
  note: text("note"),
  createdBy: text("created_by"),
  error: text("error"),
  createdAt,
  preparedAt: timestamp("prepared_at", { withTimezone: true }),
  sentAt: timestamp("sent_at", { withTimezone: true }),
  completedAt: timestamp("completed_at", { withTimezone: true }),
});

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
  // on-chain поля (пакетная выплата)
  batchId: uuid("batch_id").references(() => payoutBatches.id, { onDelete: "set null" }),
  walletAddress: text("wallet_address"),
  txHash: text("tx_hash"),
  nonce: bigint("nonce", { mode: "number" }),
  rawTx: text("raw_tx"),
  chainStatus: text("chain_status"), // pending | simulated | broadcast | confirmed | failed
  chainError: text("chain_error"),
}, (table) => [
  index("payouts_partner_idx").on(table.partnerId, table.requestedAt),
  index("payouts_status_idx").on(table.status, table.requestedAt),
  index("payouts_batch_idx").on(table.batchId),
  uniqueIndex("payouts_tx_hash_uidx").on(table.txHash).where(sql`${table.txHash} IS NOT NULL`),
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
  providerId: text("provider_id"),
  accountClass: text("account_class"),
  pricingMode: text("pricing_mode"),
  paidFundedNano: bigint("paid_funded_nano", { mode: "bigint" }),
  commissionEligible: boolean("commission_eligible"),
  snapshotDigest: text("snapshot_digest"),
  createdAt,
}, (table) => [
  uniqueIndex("pending_referral_events_kind_ref_uidx").on(table.kind, table.commerceRef),
  index("pending_referral_events_user_idx").on(table.commerceUserId),
  check("pending_referral_events_attribution_check", sql`
    (
      ${table.providerId} IS NULL
      AND ${table.accountClass} IS NULL
      AND ${table.pricingMode} IS NULL
      AND ${table.paidFundedNano} IS NULL
      AND ${table.commissionEligible} IS NULL
      AND ${table.snapshotDigest} IS NULL
    )
    OR (
      ${table.kind} = 'spend'
      AND ${table.providerId} IS NOT NULL
      AND ${table.providerId} <> ''
      AND ${table.accountClass} = 'b2c'
      AND ${table.pricingMode} = 'track'
      AND ${table.paidFundedNano} IS NOT NULL
      AND ${table.paidFundedNano} > 0
      AND ${table.amountNano} = ${table.paidFundedNano}
      AND ${table.commissionEligible} IS TRUE
      AND ${table.snapshotDigest} IS NOT NULL
      AND ${table.snapshotDigest} <> ''
    )
  `),
]);
