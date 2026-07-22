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
  primaryKey,
  text,
  timestamp,
  uniqueIndex,
  uuid,
} from "drizzle-orm/pg-core";
import { sql } from "drizzle-orm";

const createdAt = timestamp("created_at", { withTimezone: true }).notNull().defaultNow();
const updatedAt = timestamp("updated_at", { withTimezone: true }).notNull().defaultNow();

export const userStatus = pgEnum("user_status", ["active", "disabled"]);
export const engineAccountStatus = pgEnum("engine_account_status", ["pending", "active", "error", "disabled"]);
export const paymentStatus = pgEnum("payment_status", ["pending", "paid", "failed", "refunded", "disputed"]);
export const webhookStatus = pgEnum("webhook_status", ["received", "processed", "ignored", "failed"]);
export const engineCreditStatus = pgEnum("engine_credit_status", ["pending", "processing", "retry", "confirmed", "dead"]);
export const engineAdjustmentKind = pgEnum("engine_adjustment_kind", ["refund", "dispute"]);
export const engineAdjustmentStatus = pgEnum("engine_adjustment_status", ["pending", "processing", "retry", "confirmed", "dead"]);
export const apiKeyStatus = pgEnum("api_key_status", ["active", "disabled"]);
export const checkoutStatus = pgEnum("checkout_status", ["creating", "pending", "paid", "canceled", "refunded", "failed"]);
export const authTokenPurpose = pgEnum("auth_token_purpose", ["verify_email", "reset_password"]);
export const emailOutboxStatus = pgEnum("email_outbox_status", [
  "pending",
  "processing",
  "sent",
  "failed",
  "canceled",
]);
export const customerType = pgEnum("customer_type", ["b2c", "b2b"]);
export const pricingJobStatus = pgEnum("pricing_job_status", ["pending", "processing", "retry", "confirmed"]);
export const oauthProvider = pgEnum("oauth_provider", ["google", "github"]);
export const contentProjectStatus = pgEnum("content_project_status", [
  "imported",
  "brief_ready",
  "drafting",
  "blog_published",
  "distributed",
]);
export const contentDraftStatus = pgEnum("content_draft_status", ["draft", "approved"]);
export const blogPostStatus = pgEnum("blog_post_status", ["draft", "published"]);

export const users = pgTable("users", {
  id: uuid("id").primaryKey(),
  email: text("email").notNull(),
  displayName: text("display_name").notNull(),
  emailVerified: boolean("email_verified").notNull().default(false),
  passwordHash: text("password_hash"),
  status: userStatus("status").notNull().default("active"),
  // TOTP (Google Authenticator) 2FA. Secret stored AES-GCM-encrypted (auth-secrets); "pending"
  // while a secret exists but totp_enabled is false (enrolled, not yet verified). Gates key issuance.
  totpSecret: text("totp_secret"),
  totpEnabled: boolean("totp_enabled").notNull().default(false),
  createdAt,
  updatedAt,
}, (table) => [uniqueIndex("users_email_lower_uidx").on(sql`lower(${table.email})`)]);

export const customerProfiles = pgTable("customer_profiles", {
  userId: uuid("user_id").primaryKey().references(() => users.id, { onDelete: "restrict" }),
  customerType: customerType("customer_type").notNull().default("b2c"),
  currentTier: integer("current_tier"),
  multiplierBp: integer("multiplier_bp").notNull().default(4000),
  pricingMonthStart: timestamp("pricing_month_start", { withTimezone: true }).notNull(),
  // Prepay-тир: накопленные пополнения (суммируются, пока не слетел) + скользящее 30-дневное окно удержания.
  cumulativeTopupNano: bigint("cumulative_topup_nano", { mode: "bigint" }).notNull().default(sql`0`),
  tierWindowStart: timestamp("tier_window_start", { withTimezone: true }),
  tierWindowSpentNano: bigint("tier_window_spent_nano", { mode: "bigint" }).notNull().default(sql`0`),
  // Скидка сейлза как «пол»: эффективный mult = min(тир-mult, 10000 - referral_floor_bps). 0 = нет.
  referralFloorBps: integer("referral_floor_bps").notNull().default(0),
  // Бесплатный баланс (welcome-бонус/промо), ещё не израсходованный списаниями. Бесплатное тратится
  // первым: комиссия рефа идёт только с части списания, покрытой РЕАЛЬНЫМИ деньгами (см. real_funded).
  freeBalanceNano: bigint("free_balance_nano", { mode: "bigint" }).notNull().default(sql`0`),
  createdAt,
  updatedAt,
}, (table) => [
  check("customer_profiles_multiplier_check", sql`${table.multiplierBp} BETWEEN 0 AND 10000`),
  check("customer_profiles_referral_floor_check", sql`${table.referralFloorBps} BETWEEN 0 AND 9500`),
  // Expanded in 0008. Contract only in a later release after no deployed writer can emit tier 5.
  check("customer_profiles_tier_check", sql`${table.currentTier} IS NULL OR ${table.currentTier} BETWEEN 0 AND 5`),
  check("customer_profiles_type_tier_check", sql`
    (${table.customerType} = 'b2c' AND ${table.currentTier} IS NOT NULL)
    OR (${table.customerType} = 'b2b' AND ${table.currentTier} IS NULL)
  `),
]);

export const businessInvites = pgTable("business_invites", {
  id: uuid("id").primaryKey(),
  email: text("email").notNull(),
  tokenHash: text("token_hash").notNull(),
  multiplierBp: integer("multiplier_bp").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  consumedAt: timestamp("consumed_at", { withTimezone: true }),
  consumedByUserId: uuid("consumed_by_user_id").references(() => users.id, { onDelete: "restrict" }),
  createdAt,
}, (table) => [
  uniqueIndex("business_invites_token_hash_uidx").on(table.tokenHash),
  index("business_invites_email_idx").on(table.email, table.createdAt),
  check("business_invites_multiplier_check", sql`${table.multiplierBp} BETWEEN 0 AND 10000`),
]);

// Атрибуция регистрации к партнёрскому реф-коду (sales bounded context читает это через
// internal-фид в apps/api; сама таблица не знает о партнёрах — только код из ссылки ?ref=).
export const referralAttributions = pgTable("referral_attributions", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  code: text("code").notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("referral_attributions_user_uidx").on(table.userId),
  index("referral_attributions_code_idx").on(table.code),
]);

export const pricingMonths = pgTable("pricing_months", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  monthStart: timestamp("month_start", { withTimezone: true }).notNull(),
  openingTier: integer("opening_tier").notNull(),
  highestTier: integer("highest_tier").notNull(),
  spentNano: bigint("spent_nano", { mode: "bigint" }).notNull().default(sql`0`),
  closedAt: timestamp("closed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("pricing_months_user_month_uidx").on(table.userId, table.monthStart),
  check("pricing_months_opening_tier_check", sql`${table.openingTier} BETWEEN 0 AND 4`),
  check("pricing_months_highest_tier_check", sql`${table.highestTier} BETWEEN 0 AND 4`),
  check("pricing_months_spent_check", sql`${table.spentNano} >= 0`),
]);

export const pricingUsageCursors = pgTable("pricing_usage_cursors", {
  engineAccountId: text("engine_account_id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  lastLedgerId: bigint("last_ledger_id", { mode: "bigint" }).notNull().default(sql`0`),
  updatedAt,
}, (table) => [uniqueIndex("pricing_usage_cursors_user_uidx").on(table.userId)]);

export const pricingUsageEvents = pgTable("pricing_usage_events", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  ledgerEntryId: bigint("ledger_entry_id", { mode: "bigint" }).notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  // Часть списания, покрытая реальными деньгами (free-first). Реф-комиссия идёт только с неё.
  realFundedNano: bigint("real_funded_nano", { mode: "bigint" }).notNull().default(sql`0`),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  // Монотонный курсор для внешних читателей (sales-фид). Порядок ~= порядок вставки; читатели
  // обязаны скрывать свежие строки (created_at близко к now), чтобы не терять in-flight вставки.
  feedSeq: bigserial("feed_seq", { mode: "bigint" }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("pricing_usage_events_engine_ledger_uidx").on(table.engineAccountId, table.ledgerEntryId),
  uniqueIndex("pricing_usage_events_feed_seq_uidx").on(table.feedSeq),
  index("pricing_usage_events_user_time_idx").on(table.userId, table.occurredAt),
  check("pricing_usage_events_amount_check", sql`${table.amountNano} > 0`),
]);

export const enginePricingJobs = pgTable("engine_pricing_jobs", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  multiplierBp: integer("multiplier_bp").notNull(),
  reason: text("reason").notNull(),
  status: pricingJobStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_pricing_jobs_user_uidx").on(table.userId),
  index("engine_pricing_jobs_claim_idx").on(table.status, table.nextAttemptAt),
  check("engine_pricing_jobs_multiplier_check", sql`${table.multiplierBp} BETWEEN 0 AND 10000`),
]);

export const authIdentities = pgTable("auth_identities", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  provider: text("provider").notNull(),
  subject: text("subject").notNull(),
  email: text("email"),
  emailVerified: boolean("email_verified").notNull().default(false),
  metadata: jsonb("metadata").notNull().default({}),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("auth_identities_provider_subject_uidx").on(table.provider, table.subject),
  uniqueIndex("auth_identities_user_provider_uidx").on(table.userId, table.provider),
]);

export const authSessions = pgTable("auth_sessions", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  tokenHash: text("token_hash").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  lastSeenAt: timestamp("last_seen_at", { withTimezone: true }).notNull().defaultNow(),
  revokedAt: timestamp("revoked_at", { withTimezone: true }),
  userAgent: text("user_agent"),
  ipAddress: text("ip_address"),
  createdAt,
}, (table) => [
  uniqueIndex("auth_sessions_token_hash_uidx").on(table.tokenHash),
  index("auth_sessions_user_idx").on(table.userId, table.createdAt),
  index("auth_sessions_expiry_idx").on(table.expiresAt),
]);

export const signupProfiles = pgTable("signup_profiles", {
  userId: uuid("user_id").primaryKey().references(() => users.id, { onDelete: "cascade" }),
  emailCanonical: text("email_canonical").notNull(),
  ipAddress: text("ip_address"),
  ipSubnet: text("ip_subnet"),
  userAgent: text("user_agent"),
  deviceHash: text("device_hash"),
  bonusGranted: boolean("bonus_granted").notNull().default(false),
  flaggedReason: text("flagged_reason"),
  createdAt,
}, (table) => [
  // Один welcome-бонус на устройство/подсеть/канонический email — атомарно на уровне БД.
  uniqueIndex("signup_bonus_device_uidx").on(table.deviceHash).where(sql`${table.bonusGranted}`),
  uniqueIndex("signup_bonus_subnet_uidx").on(table.ipSubnet).where(sql`${table.bonusGranted}`),
  uniqueIndex("signup_bonus_email_uidx").on(table.emailCanonical).where(sql`${table.bonusGranted}`),
  index("signup_profiles_subnet_idx").on(table.ipSubnet, table.createdAt),
]);

export const deviceSightings = pgTable("device_sightings", {
  deviceHash: text("device_hash").notNull(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "cascade" }),
  firstSeenAt: timestamp("first_seen_at", { withTimezone: true }).notNull().defaultNow(),
  lastSeenAt: timestamp("last_seen_at", { withTimezone: true }).notNull().defaultNow(),
}, (table) => [
  primaryKey({ columns: [table.deviceHash, table.userId] }),
  index("device_sightings_user_idx").on(table.userId),
]);

export const authRateLimits = pgTable("auth_rate_limits", {
  keyHash: text("key_hash").primaryKey(),
  attempts: integer("attempts").notNull().default(0),
  windowStartedAt: timestamp("window_started_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt,
});

export const authTokens = pgTable("auth_tokens", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  purpose: authTokenPurpose("purpose").notNull(),
  tokenHash: text("token_hash").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  usedAt: timestamp("used_at", { withTimezone: true }),
  createdAt,
}, (table) => [
  uniqueIndex("auth_tokens_token_hash_uidx").on(table.tokenHash),
  index("auth_tokens_user_purpose_idx").on(table.userId, table.purpose, table.createdAt),
]);

export const oauthTransactions = pgTable("oauth_transactions", {
  stateHash: text("state_hash").primaryKey(),
  provider: oauthProvider("provider").notNull(),
  nonce: text("nonce"),
  codeVerifier: text("code_verifier").notNull(),
  inviteTokenHash: text("invite_token_hash"),
  // Партнёрский реф-код (?ref=), протянутый через OAuth: реф партнёра станет B2B до бонуса.
  referralCode: text("referral_code"),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  consumedAt: timestamp("consumed_at", { withTimezone: true }),
  createdAt,
}, (table) => [index("oauth_transactions_expiry_idx").on(table.expiresAt)]);

export const emailOutbox = pgTable("email_outbox", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  recipient: text("recipient").notNull(),
  template: text("template").notNull(),
  payload: jsonb("payload").notNull().default({}),
  status: emailOutboxStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  providerMessageId: text("provider_message_id"),
  sentAt: timestamp("sent_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [index("email_outbox_claim_idx").on(table.status, table.nextAttemptAt)]);

export const engineAccounts = pgTable("engine_accounts", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id"),
  multBp: integer("mult_bp").notNull().default(4000),
  status: engineAccountStatus("status").notNull().default("pending"),
  lastError: text("last_error"),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_accounts_user_uidx").on(table.userId),
  uniqueIndex("engine_accounts_engine_id_uidx").on(table.engineAccountId)
    .where(sql`${table.engineAccountId} IS NOT NULL`),
  check("engine_accounts_mult_bp_check", sql`${table.multBp} >= 0`),
]);

export const checkoutSessions = pgTable("checkout_sessions", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  provider: text("provider").notNull(),
  amountUsd: bigint("amount_usd", { mode: "bigint" }).notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  providerPaymentId: text("provider_payment_id"),
  checkoutUrl: text("checkout_url"),
  status: checkoutStatus("status").notNull().default("creating"),
  providerState: jsonb("provider_state").notNull().default({}),
  expiresAt: timestamp("expires_at", { withTimezone: true }),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  index("checkout_sessions_user_created_idx").on(table.userId, table.createdAt),
  uniqueIndex("checkout_sessions_provider_payment_uidx").on(table.provider, table.providerPaymentId)
    .where(sql`${table.providerPaymentId} IS NOT NULL`),
  check("checkout_sessions_amount_usd_check", sql`${table.amountUsd} > 0`),
  check("checkout_sessions_amount_exact_check", sql`${table.amountNano} = ${table.amountUsd} * 1000000000`),
]);

export const payments = pgTable("payments", {
  id: uuid("id").primaryKey(),
  checkoutId: uuid("checkout_id").notNull().references(() => checkoutSessions.id, { onDelete: "restrict" }),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  provider: text("provider").notNull(),
  providerPaymentId: text("provider_payment_id").notNull(),
  amountMinor: bigint("amount_minor", { mode: "bigint" }).notNull(),
  currency: text("currency").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  status: paymentStatus("status").notNull().default("pending"),
  providerState: jsonb("provider_state").notNull().default({}),
  paidAt: timestamp("paid_at", { withTimezone: true }),
  // Монотонный курсор вставки для внешних читателей; см. pricing_usage_events.feed_seq.
  feedSeq: bigserial("feed_seq", { mode: "bigint" }).notNull(),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("payments_checkout_uidx").on(table.checkoutId),
  uniqueIndex("payments_provider_payment_uidx").on(table.provider, table.providerPaymentId),
  index("payments_user_created_idx").on(table.userId, table.createdAt),
  check("payments_amount_minor_check", sql`${table.amountMinor} > 0`),
  check("payments_amount_nano_check", sql`${table.amountNano} > 0`),
  check("payments_currency_check", sql`${table.currency} = upper(${table.currency}) AND length(${table.currency}) = 3`),
]);

export const webhookEvents = pgTable("webhook_events", {
  id: uuid("id").primaryKey(),
  provider: text("provider").notNull(),
  providerEventId: text("provider_event_id").notNull(),
  eventType: text("event_type").notNull(),
  payload: jsonb("payload").notNull(),
  status: webhookStatus("status").notNull().default("received"),
  attempts: integer("attempts").notNull().default(0),
  lastError: text("last_error"),
  receivedAt: timestamp("received_at", { withTimezone: true }).notNull().defaultNow(),
  processedAt: timestamp("processed_at", { withTimezone: true }),
}, (table) => [
  uniqueIndex("webhook_events_provider_event_uidx").on(table.provider, table.providerEventId),
  index("webhook_events_status_idx").on(table.status, table.receivedAt),
]);

export const engineCredits = pgTable("engine_credits", {
  id: uuid("id").primaryKey(),
  paymentId: uuid("payment_id").notNull().references(() => payments.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  idempotencyRef: text("idempotency_ref").notNull(),
  status: engineCreditStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  engineBalanceAfterNano: bigint("engine_balance_after_nano", { mode: "bigint" }),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_credits_payment_uidx").on(table.paymentId),
  uniqueIndex("engine_credits_ref_uidx").on(table.idempotencyRef),
  index("engine_credits_claim_idx").on(table.status, table.nextAttemptAt),
  check("engine_credits_amount_check", sql`${table.amountNano} > 0`),
]);

// AUDIT-TODO(C23): run pnpm db:generate + migrate; insert this marker and update the profile aggregate in one transaction.
export const pricingCreditAccruals = pgTable("pricing_credit_accruals", {
  creditId: uuid("credit_id").primaryKey().references(() => engineCredits.id, { onDelete: "restrict" }),
  appliedAt: timestamp("applied_at", { withTimezone: true }).notNull().defaultNow(),
});

// AUDIT-TODO(C24): run pnpm db:generate + migrate; enqueue the adjustment in the same transaction as the refund/dispute state change.
export const engineAdjustments = pgTable("engine_adjustments", {
  id: uuid("id").primaryKey(),
  paymentId: uuid("payment_id").notNull().references(() => payments.id, { onDelete: "restrict" }),
  webhookEventId: uuid("webhook_event_id").notNull().references(() => webhookEvents.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  kind: engineAdjustmentKind("kind").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  idempotencyRef: text("idempotency_ref").notNull(),
  status: engineAdjustmentStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  engineBalanceAfterNano: bigint("engine_balance_after_nano", { mode: "bigint" }),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_adjustments_payment_event_uidx").on(table.paymentId, table.webhookEventId),
  uniqueIndex("engine_adjustments_ref_uidx").on(table.idempotencyRef),
  index("engine_adjustments_payment_idx").on(table.paymentId, table.createdAt),
  index("engine_adjustments_claim_idx").on(table.status, table.nextAttemptAt),
  check("engine_adjustments_amount_check", sql`${table.amountNano} < 0`),
]);

export const apiKeys = pgTable("api_keys", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  engineKeyId: text("engine_key_id"),
  label: text("label"),
  keyMasked: text("key_masked").notNull(),
  status: apiKeyStatus("status").notNull().default("active"),
  createdAt,
  updatedAt,
}, (table) => [
  index("api_keys_user_idx").on(table.userId, table.createdAt),
  uniqueIndex("api_keys_engine_key_uidx").on(table.engineKeyId)
    .where(sql`${table.engineKeyId} IS NOT NULL`),
]);

export const auditLog = pgTable("audit_log", {
  id: bigserial("id", { mode: "bigint" }).primaryKey(),
  actorType: text("actor_type").notNull(),
  actorId: text("actor_id"),
  action: text("action").notNull(),
  targetType: text("target_type").notNull(),
  targetId: text("target_id").notNull(),
  metadata: jsonb("metadata").notNull().default({}),
  createdAt,
}, (table) => [index("audit_log_target_idx").on(table.targetType, table.targetId, table.createdAt)]);

export const adminAccounts = pgTable("admin_accounts", {
  id: uuid("id").primaryKey(),
  username: text("username").notNull(),
  passwordHash: text("password_hash").notNull(),
  status: text("status").notNull().default("active"),
  passwordChangedAt: timestamp("password_changed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("admin_accounts_username_lower_uidx").on(sql`lower(${table.username})`),
  check("admin_accounts_username_check", sql`
    ${table.username} = btrim(${table.username})
    AND length(${table.username}) BETWEEN 1 AND 80
    AND ${table.username} ~ '^[A-Za-z0-9._@-]+$'
  `),
  check("admin_accounts_password_hash_check", sql`length(${table.passwordHash}) BETWEEN 20 AND 512`),
  check("admin_accounts_status_check", sql`${table.status} IN ('active', 'disabled')`),
]);

export const adminAccountDomains = pgTable("admin_account_domains", {
  adminAccountId: uuid("admin_account_id").notNull()
    .references(() => adminAccounts.id, { onDelete: "cascade" }),
  domain: text("domain").notNull(),
  createdAt,
}, (table) => [
  primaryKey({ columns: [table.adminAccountId, table.domain] }),
  index("admin_account_domains_domain_idx").on(table.domain, table.adminAccountId),
  check("admin_account_domains_domain_check", sql`${table.domain} IN (
    'admin.apitoken.sale',
    'admin.partners.apitoken.sale',
    'crm.apitoken.sale',
    'content-studio.apitoken.sale',
    'monitoring.apitoken.sale'
  )`),
]);

export const contentProjects = pgTable("content_projects", {
  id: uuid("id").primaryKey(),
  sourceUrl: text("source_url").notNull(),
  sourcePlatform: text("source_platform").notNull(),
  sourceTitle: text("source_title").notNull().default(""),
  sourceAuthor: text("source_author"),
  sourceContent: text("source_content").notNull().default(""),
  sourcePublishedAt: timestamp("source_published_at", { withTimezone: true }),
  sourceSnapshot: jsonb("source_snapshot").notNull().default({}),
  primaryLocale: text("primary_locale").notNull().default("en"),
  status: contentProjectStatus("status").notNull().default("imported"),
  briefMarkdown: text("brief_markdown").notNull().default(""),
  briefVersion: integer("brief_version").notNull().default(0),
  createdAt,
  updatedAt,
}, (table) => [
  index("content_projects_status_updated_idx").on(table.status, table.updatedAt),
  check("content_projects_locale_check", sql`${table.primaryLocale} IN ('en', 'ru')`),
  check("content_projects_brief_version_check", sql`${table.briefVersion} >= 0`),
]);

export const contentSources = pgTable("content_sources", {
  id: uuid("id").primaryKey(),
  projectId: uuid("project_id").notNull().references(() => contentProjects.id, { onDelete: "cascade" }),
  url: text("url").notNull(),
  title: text("title").notNull().default(""),
  sourceType: text("source_type").notNull().default("reference"),
  publisher: text("publisher"),
  publishedAt: timestamp("published_at", { withTimezone: true }),
  notes: text("notes").notNull().default(""),
  createdAt,
}, (table) => [
  uniqueIndex("content_sources_project_url_uidx").on(table.projectId, table.url),
  index("content_sources_project_idx").on(table.projectId, table.createdAt),
]);

export const platformProfiles = pgTable("platform_profiles", {
  id: uuid("id").primaryKey(),
  key: text("key").notNull(),
  name: text("name").notNull(),
  rules: jsonb("rules").notNull().default({}),
  builtIn: boolean("built_in").notNull().default(false),
  active: boolean("active").notNull().default(true),
  createdAt,
  updatedAt,
}, (table) => [uniqueIndex("platform_profiles_key_uidx").on(table.key)]);

export const contentDrafts = pgTable("content_drafts", {
  id: uuid("id").primaryKey(),
  projectId: uuid("project_id").notNull().references(() => contentProjects.id, { onDelete: "cascade" }),
  profileKey: text("profile_key").notNull(),
  locale: text("locale").notNull().default("en"),
  title: text("title").notNull().default(""),
  excerpt: text("excerpt").notNull().default(""),
  bodyMarkdown: text("body_markdown").notNull().default(""),
  status: contentDraftStatus("status").notNull().default("draft"),
  revision: integer("revision").notNull().default(1),
  briefVersion: integer("brief_version").notNull().default(0),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("content_drafts_project_profile_locale_uidx").on(table.projectId, table.profileKey, table.locale),
  index("content_drafts_project_idx").on(table.projectId, table.updatedAt),
  check("content_drafts_locale_check", sql`${table.locale} IN ('en', 'ru')`),
  check("content_drafts_revision_check", sql`${table.revision} > 0`),
  check("content_drafts_brief_version_check", sql`${table.briefVersion} >= 0`),
]);

export const contentRevisions = pgTable("content_revisions", {
  id: uuid("id").primaryKey(),
  draftId: uuid("draft_id").notNull().references(() => contentDrafts.id, { onDelete: "cascade" }),
  revision: integer("revision").notNull(),
  scope: text("scope").notNull(),
  instruction: text("instruction").notNull(),
  before: jsonb("before").notNull(),
  after: jsonb("after").notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("content_revisions_draft_revision_uidx").on(table.draftId, table.revision),
  check("content_revisions_scope_check", sql`${table.scope} IN ('draft', 'platform', 'project', 'all')`),
  check("content_revisions_revision_check", sql`${table.revision} > 1`),
]);

export const blogPosts = pgTable("blog_posts", {
  id: uuid("id").primaryKey(),
  projectId: uuid("project_id").notNull().references(() => contentProjects.id, { onDelete: "restrict" }),
  draftId: uuid("draft_id").notNull().references(() => contentDrafts.id, { onDelete: "restrict" }),
  slug: text("slug").notNull(),
  locale: text("locale").notNull().default("en"),
  title: text("title").notNull(),
  excerpt: text("excerpt").notNull(),
  bodyMarkdown: text("body_markdown").notNull(),
  authorName: text("author_name").notNull().default("apiToken.sale Editorial"),
  seoTitle: text("seo_title").notNull(),
  seoDescription: text("seo_description").notNull(),
  sourceUrls: jsonb("source_urls").notNull().default([]),
  relatedPaths: jsonb("related_paths").notNull().default([]),
  status: blogPostStatus("status").notNull().default("draft"),
  publishedAt: timestamp("published_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("blog_posts_project_uidx").on(table.projectId),
  uniqueIndex("blog_posts_draft_uidx").on(table.draftId),
  uniqueIndex("blog_posts_slug_locale_uidx").on(table.slug, table.locale),
  index("blog_posts_status_published_idx").on(table.status, table.publishedAt),
  check("blog_posts_locale_check", sql`${table.locale} IN ('en', 'ru')`),
  check("blog_posts_publication_check", sql`
    (${table.status} = 'draft' AND ${table.publishedAt} IS NULL)
    OR (${table.status} = 'published' AND ${table.publishedAt} IS NOT NULL)
  `),
]);

export const externalPublications = pgTable("external_publications", {
  id: uuid("id").primaryKey(),
  projectId: uuid("project_id").notNull().references(() => contentProjects.id, { onDelete: "restrict" }),
  draftId: uuid("draft_id").notNull().references(() => contentDrafts.id, { onDelete: "restrict" }),
  platformKey: text("platform_key").notNull(),
  url: text("url").notNull(),
  publishedAt: timestamp("published_at", { withTimezone: true }).notNull().defaultNow(),
  createdAt,
}, (table) => [
  uniqueIndex("external_publications_draft_uidx").on(table.draftId),
  uniqueIndex("external_publications_url_uidx").on(table.url),
  index("external_publications_project_idx").on(table.projectId, table.publishedAt),
  check("external_publications_not_blog_check", sql`${table.platformKey} <> 'blog'`),
]);

export type EngineCreditRow = typeof engineCredits.$inferSelect;
