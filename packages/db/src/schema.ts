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

export const users = pgTable("users", {
  id: uuid("id").primaryKey(),
  email: text("email").notNull(),
  emailVerified: boolean("email_verified").notNull().default(false),
  passwordHash: text("password_hash"),
  status: userStatus("status").notNull().default("active"),
  createdAt,
  updatedAt,
}, (table) => [uniqueIndex("users_email_lower_uidx").on(sql`lower(${table.email})`)]);

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

export const emailOutbox = pgTable("email_outbox", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  recipient: text("recipient").notNull(),
  template: text("template").notNull(),
  payload: jsonb("payload").notNull().default({}),
  status: emailOutboxStatus("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lastError: text("last_error"),
  sentAt: timestamp("sent_at", { withTimezone: true }),
  createdAt,
}, (table) => [index("email_outbox_claim_idx").on(table.status, table.nextAttemptAt)]);

export const engineAccounts = pgTable("engine_accounts", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id"),
  multBp: integer("mult_bp").notNull().default(2000),
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
}, (table) => [index("api_keys_user_idx").on(table.userId, table.createdAt)]);

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
