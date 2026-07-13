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

export const users = pgTable("users", {
  id: uuid("id").primaryKey(),
  email: text("email").notNull(),
  emailVerified: boolean("email_verified").notNull().default(false),
  passwordHash: text("password_hash"),
  status: userStatus("status").notNull().default("active"),
  createdAt,
  updatedAt,
}, (table) => [uniqueIndex("users_email_lower_uidx").on(sql`lower(${table.email})`)]);

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

export const payments = pgTable("payments", {
  id: uuid("id").primaryKey(),
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
