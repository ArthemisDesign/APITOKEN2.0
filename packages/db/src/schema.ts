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
} from "drizzle-orm/pg-core";
import { sql } from "drizzle-orm";

const createdAt = timestamp("created_at", { withTimezone: true }).notNull().defaultNow();
const updatedAt = timestamp("updated_at", { withTimezone: true }).notNull().defaultNow();

export const userStatus = pgEnum("user_status", ["active", "disabled"]);
export const engineAccountStatus = pgEnum("engine_account_status", ["pending", "active", "error", "disabled"]);
export const paymentStatus = pgEnum("payment_status", ["pending", "paid", "failed", "refunded", "disputed"]);
export const webhookStatus = pgEnum("webhook_status", ["received", "processed", "ignored", "failed"]);
export const engineCreditStatus = pgEnum("engine_credit_status", ["pending", "processing", "retry", "confirmed", "dead"]);
export const apiKeyStatus = pgEnum("api_key_status", ["active", "disabled"]);
export const checkoutStatus = pgEnum("checkout_status", ["creating", "pending", "paid", "canceled", "refunded", "failed"]);
export const authTokenPurpose = pgEnum("auth_token_purpose", ["verify_email", "reset_password"]);
export const emailOutboxStatus = pgEnum("email_outbox_status", ["pending", "processing", "sent", "failed"]);
export const customerType = pgEnum("customer_type", ["b2c", "b2b"]);
export const pricingJobStatus = pgEnum("pricing_job_status", ["pending", "processing", "retry", "confirmed"]);
export const oauthProvider = pgEnum("oauth_provider", ["google", "github"]);

export const users = pgTable("users", {
  id: uuid("id").primaryKey(),
  email: text("email").notNull(),
  displayName: text("display_name").notNull(),
  emailVerified: boolean("email_verified").notNull().default(false),
  passwordHash: text("password_hash"),
  status: userStatus("status").notNull().default("active"),
  createdAt,
  updatedAt,
}, (table) => [uniqueIndex("users_email_lower_uidx").on(sql`lower(${table.email})`)]);

export const customerProfiles = pgTable("customer_profiles", {
  userId: uuid("user_id").primaryKey().references(() => users.id, { onDelete: "restrict" }),
  customerType: customerType("customer_type").notNull().default("b2c"),
  currentTier: integer("current_tier"),
  multiplierBp: integer("multiplier_bp").notNull().default(4000),
  pricingMonthStart: timestamp("pricing_month_start", { withTimezone: true }).notNull(),
  createdAt,
  updatedAt,
}, (table) => [
  check("customer_profiles_multiplier_check", sql`${table.multiplierBp} BETWEEN 0 AND 10000`),
  check("customer_profiles_tier_check", sql`${table.currentTier} IS NULL OR ${table.currentTier} BETWEEN 0 AND 4`),
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
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("pricing_usage_events_engine_ledger_uidx").on(table.engineAccountId, table.ledgerEntryId),
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

export type EngineCreditRow = typeof engineCredits.$inferSelect;
