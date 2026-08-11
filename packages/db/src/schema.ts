import {
  bigint,
  bigserial,
  boolean,
  check,
  foreignKey,
  index,
  integer,
  jsonb,
  pgEnum,
  pgTable,
  primaryKey,
  text,
  timestamp,
  unique,
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
  // Локальная проекция welcome/промо. Legacy rows списывают её free-first; attributed policy rows
  // вычитают exact bonus+other evidence. Комиссия использует immutable eligible paid funding.
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
  email: text("email"),
  tokenHash: text("token_hash").notNull(),
  encryptedToken: text("encrypted_token"),
  multiplierBp: integer("multiplier_bp").notNull(),
  expiresAt: timestamp("expires_at", { withTimezone: true }).notNull(),
  consumedAt: timestamp("consumed_at", { withTimezone: true }),
  consumedByUserId: uuid("consumed_by_user_id").references(() => users.id, { onDelete: "restrict" }),
  revokedAt: timestamp("revoked_at", { withTimezone: true }),
  revokedByActor: text("revoked_by_actor"),
  supersededByInviteId: uuid("superseded_by_invite_id"),
  idempotencyKey: uuid("idempotency_key"),
  createdByActor: text("created_by_actor"),
  createdAt,
}, (table) => [
  uniqueIndex("business_invites_token_hash_uidx").on(table.tokenHash),
  uniqueIndex("business_invites_idempotency_key_uidx").on(table.idempotencyKey)
    .where(sql`${table.idempotencyKey} IS NOT NULL`),
  index("business_invites_email_idx").on(table.email, table.createdAt),
  foreignKey({
    columns: [table.supersededByInviteId],
    foreignColumns: [table.id],
    name: "business_invites_superseded_by_invite_id_fk",
  }).onDelete("restrict"),
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
  // Отдельный догоняющий маркер для истории пополнений: обычный курсор уже стоит выше старых
  // топапов, поэтому без него движковые пополнения до этой правки остались бы неизвестны коммерции.
  topupsScannedThroughLedgerId: bigint("topups_scanned_through_ledger_id", { mode: "bigint" })
    .notNull().default(sql`0`),
  updatedAt,
}, (table) => [uniqueIndex("pricing_usage_cursors_user_uidx").on(table.userId)]);

/**
 * Иммутабельная копия ПОПОЛНЕНИЙ из леджера движка. Балансом не является и им не управляет:
 * нужна отчётности, которая обязана видеть реальные деньги клиента целиком, включая
 * пополнения, сделанные напрямую в движке (`admin-credit:`, ручные) — их нет в `payments`.
 * `source`: `payment` — депозит через платёжного провайдера (тот же ref, что в payments),
 * `bonus` — подарочные кредиты (welcome/промо), `manual` — всё остальное (админ-кредит и т.п.).
 */
export const pricingUsageTopups = pgTable("pricing_usage_topups", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  ledgerEntryId: bigint("ledger_entry_id", { mode: "bigint" }).notNull(),
  ref: text("ref"),
  source: text("source").notNull(),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  occurredAt: timestamp("occurred_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("pricing_usage_topups_engine_ledger_uidx").on(table.engineAccountId, table.ledgerEntryId),
  index("pricing_usage_topups_user_time_idx").on(table.userId, table.occurredAt),
  check("pricing_usage_topups_amount_check", sql`${table.amountNano} > 0`),
  check("pricing_usage_topups_source_check", sql`${table.source} IN ('payment', 'bonus', 'manual')`),
]);

export const pricingUsageEvents = pgTable("pricing_usage_events", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  ledgerEntryId: bigint("ledger_entry_id", { mode: "bigint" }).notNull(),
  // Exact top-level provider copied from the immutable engine charge ledger. Historical sentinel
  // values remain readable while provider_recovery_version records which evidence algorithm made
  // the latest bounded attempt; provider identity is never inferred from a model name.
  providerId: text("provider_id"),
  providerRecoveryVersion: integer("provider_recovery_version").notNull().default(0),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
  // Legacy: free-first paid projection. Attributed: exact paid_funded only when commission-eligible;
  // static/ineligible rows store 0 while true paid evidence remains in pricing_usage_attributions.
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
  check("pricing_usage_events_provider_recovery_version_check", sql`${table.providerRecoveryVersion} >= 0`),
]);

export const enginePricingJobs = pgTable("engine_pricing_jobs", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "restrict" }),
  engineAccountId: text("engine_account_id").notNull(),
  /** NULL targets the account default; a provider id targets that provider's override. */
  providerId: text("provider_id"),
  /** NULL is only valid with a provider id and removes that override. */
  multiplierBp: integer("multiplier_bp"),
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
  uniqueIndex("engine_pricing_jobs_user_provider_uidx").on(table.userId, table.providerId),
  index("engine_pricing_jobs_claim_idx").on(table.status, table.nextAttemptAt),
  check("engine_pricing_jobs_multiplier_check",
    sql`${table.multiplierBp} IS NULL OR ${table.multiplierBp} BETWEEN 0 AND 10000`),
  check("engine_pricing_jobs_target_check",
    sql`${table.providerId} IS NOT NULL OR ${table.multiplierBp} IS NOT NULL`),
]);

/**
 * A B2B customer's per-provider discount. Absent row = the customer's default multiplier applies
 * to that provider. This is the whole per-provider pricing surface: no versions, no catalog, no
 * eligibility rules — the engine resolves `override ?? default` on every request.
 */
export const customerProviderDiscounts = pgTable("customer_provider_discounts", {
  userId: uuid("user_id").notNull().references(() => users.id, { onDelete: "cascade" }),
  providerId: text("provider_id").notNull(),
  multiplierBp: integer("multiplier_bp").notNull(),
  createdAt,
  updatedAt,
}, (table) => [
  primaryKey({ columns: [table.userId, table.providerId] }),
  check("customer_provider_discounts_multiplier_check",
    sql`${table.multiplierBp} BETWEEN 0 AND 10000`),
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
  // Nullable during the expand checkpoint: the previously deployed writer knows only the
  // boolean claim. Migration 0034 backfills every already-granted bonus to its historical $4
  // nominal; the follow-up consumer records the exact amount for every new claim.
  bonusAmountNano: bigint("bonus_amount_nano", { mode: "bigint" }),
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
  userId: uuid("user_id").references(() => users.id, { onDelete: "restrict" }),
  businessInviteId: uuid("business_invite_id").references(() => businessInvites.id, { onDelete: "restrict" }),
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
}, (table) => [
  index("email_outbox_claim_idx").on(table.status, table.nextAttemptAt),
  index("email_outbox_business_invite_idx").on(table.businessInviteId, table.createdAt),
  check("email_outbox_owner_check", sql`
    num_nonnulls(${table.userId}, ${table.businessInviteId}) = 1
  `),
]);

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
  check("engine_accounts_mult_bp_upper_check", sql`${table.multBp} <= 10000`),
]);

// Multi-provider pricing is introduced as empty, versioned streams. The existing scalar
// customer_profiles/engine_accounts/engine_pricing_jobs path remains authoritative until a later
// dual-write, reconciliation, and strict-cutover sequence.
export const providerCapabilityVersions = pgTable("provider_capability_versions", {
  generation: bigint("generation", { mode: "bigint" }).primaryKey(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  sourceRuntime: text("source_runtime"),
  sourceRevision: text("source_revision"),
  observedAt: timestamp("observed_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  unique("provider_capability_versions_digest_unique").on(table.generation, table.contentDigest),
  check("provider_capability_versions_generation_check", sql`${table.generation} > 0`),
  check("provider_capability_versions_schema_check", sql`${table.schemaVersion} > 0`),
  check("provider_capability_versions_digest_check", sql`${table.contentDigest} <> ''`),
]);

export const providerCapabilityEntries = pgTable("provider_capability_entries", {
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  providerId: text("provider_id").notNull(),
  canonicalModelId: text("canonical_model_id").notNull(),
  entryDigest: text("entry_digest").notNull(),
  capabilityData: jsonb("capability_data").notNull(),
}, (table) => [
  primaryKey({
    name: "provider_capability_entries_pk",
    columns: [table.generation, table.providerId, table.canonicalModelId],
  }),
  foreignKey({
    columns: [table.generation],
    foreignColumns: [providerCapabilityVersions.generation],
    name: "provider_capability_entries_version_fk",
  }).onDelete("cascade"),
  check("provider_capability_entries_identity_check", sql`
    ${table.providerId} <> ''
    AND ${table.canonicalModelId} <> ''
    AND ${table.entryDigest} <> ''
  `),
  check("provider_capability_entries_data_check", sql`
    jsonb_typeof(${table.capabilityData}) = 'object'
  `),
]);

export const providerCapabilityAliases = pgTable("provider_capability_aliases", {
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  providerId: text("provider_id").notNull(),
  aliasModelId: text("alias_model_id").notNull(),
  canonicalModelId: text("canonical_model_id").notNull(),
}, (table) => [
  primaryKey({
    name: "provider_capability_aliases_pk",
    columns: [table.generation, table.providerId, table.aliasModelId],
  }),
  foreignKey({
    columns: [table.generation, table.providerId, table.canonicalModelId],
    foreignColumns: [
      providerCapabilityEntries.generation,
      providerCapabilityEntries.providerId,
      providerCapabilityEntries.canonicalModelId,
    ],
    name: "provider_capability_aliases_entry_fk",
  }).onDelete("cascade"),
  check("provider_capability_aliases_identity_check", sql`
    ${table.providerId} <> ''
    AND ${table.aliasModelId} <> ''
    AND ${table.canonicalModelId} <> ''
    AND ${table.aliasModelId} <> ${table.canonicalModelId}
  `),
]);

export const providerCapabilityHead = pgTable("provider_capability_head", {
  singleton: integer("singleton").primaryKey(),
  activeGeneration: bigint("active_generation", { mode: "bigint" }).notNull(),
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.activeGeneration],
    foreignColumns: [providerCapabilityVersions.generation],
    name: "provider_capability_head_version_fk",
  }).onDelete("restrict"),
  check("provider_capability_head_singleton_check", sql`${table.singleton} = 1`),
]);

export const productCatalogVersions = pgTable("product_catalog_versions", {
  productId: text("product_id").notNull(),
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  capabilityGeneration: bigint("capability_generation", { mode: "bigint" }).notNull(),
  capabilityDigest: text("capability_digest").notNull(),
  contentDigest: text("content_digest").notNull(),
  actorType: text("actor_type").notNull(),
  actorId: text("actor_id"),
  reason: text("reason").notNull(),
  createdAt,
}, (table) => [
  primaryKey({ columns: [table.productId, table.generation] }),
  unique("product_catalog_versions_capability_unique")
    .on(table.productId, table.generation, table.capabilityGeneration),
  unique("product_catalog_versions_digest_unique")
    .on(table.productId, table.generation, table.contentDigest),
  unique("product_catalog_versions_job_target_unique")
    .on(table.productId, table.generation, table.schemaVersion, table.contentDigest),
  foreignKey({
    columns: [table.capabilityGeneration, table.capabilityDigest],
    foreignColumns: [
      providerCapabilityVersions.generation,
      providerCapabilityVersions.contentDigest,
    ],
    name: "product_catalog_versions_capability_fk",
  }).onDelete("restrict"),
  check("product_catalog_versions_identity_check", sql`
    ${table.productId} <> ''
    AND ${table.generation} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.capabilityGeneration} > 0
    AND ${table.capabilityDigest} <> ''
    AND ${table.contentDigest} <> ''
    AND ${table.actorType} <> ''
    AND ${table.reason} <> ''
  `),
]);

export const productCatalogEntries = pgTable("product_catalog_entries", {
  productId: text("product_id").notNull(),
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  capabilityGeneration: bigint("capability_generation", { mode: "bigint" }).notNull(),
  providerId: text("provider_id").notNull(),
  canonicalModelId: text("canonical_model_id").notNull(),
  enabled: boolean("enabled").notNull(),
}, (table) => [
  primaryKey({
    name: "product_catalog_entries_pk",
    columns: [table.productId, table.generation, table.providerId, table.canonicalModelId],
  }),
  foreignKey({
    columns: [table.productId, table.generation, table.capabilityGeneration],
    foreignColumns: [
      productCatalogVersions.productId,
      productCatalogVersions.generation,
      productCatalogVersions.capabilityGeneration,
    ],
    name: "product_catalog_entries_version_fk",
  }).onDelete("cascade"),
  foreignKey({
    columns: [table.capabilityGeneration, table.providerId, table.canonicalModelId],
    foreignColumns: [
      providerCapabilityEntries.generation,
      providerCapabilityEntries.providerId,
      providerCapabilityEntries.canonicalModelId,
    ],
    name: "product_catalog_entries_capability_fk",
  }).onDelete("restrict"),
  index("product_catalog_entries_enabled_idx")
    .on(table.productId, table.generation, table.providerId)
    .where(sql`${table.enabled}`),
  check("product_catalog_entries_identity_check", sql`
    ${table.productId} <> ''
    AND ${table.providerId} <> ''
    AND ${table.canonicalModelId} <> ''
  `),
]);

export const productCatalogHeads = pgTable("product_catalog_heads", {
  productId: text("product_id").primaryKey(),
  activeGeneration: bigint("active_generation", { mode: "bigint" }).notNull(),
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.productId, table.activeGeneration],
    foreignColumns: [productCatalogVersions.productId, productCatalogVersions.generation],
    name: "product_catalog_heads_version_fk",
  }).onDelete("restrict"),
  check("product_catalog_heads_product_check", sql`${table.productId} <> ''`),
]);

export const providerSwitchVersions = pgTable("provider_switch_versions", {
  generation: bigint("generation", { mode: "bigint" }).primaryKey(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  capabilityGeneration: bigint("capability_generation", { mode: "bigint" }).notNull(),
  capabilityDigest: text("capability_digest").notNull(),
  contentDigest: text("content_digest").notNull(),
  actorType: text("actor_type").notNull(),
  actorId: text("actor_id"),
  reason: text("reason").notNull(),
  createdAt,
}, (table) => [
  uniqueIndex("provider_switch_versions_digest_uidx")
    .on(table.generation, table.contentDigest),
  unique("provider_switch_versions_job_target_unique")
    .on(table.generation, table.schemaVersion, table.contentDigest),
  foreignKey({
    columns: [table.capabilityGeneration, table.capabilityDigest],
    foreignColumns: [
      providerCapabilityVersions.generation,
      providerCapabilityVersions.contentDigest,
    ],
    name: "provider_switch_versions_capability_fk",
  }).onDelete("restrict"),
  check("provider_switch_versions_identity_check", sql`
    ${table.generation} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.capabilityGeneration} > 0
    AND ${table.capabilityDigest} <> ''
    AND ${table.contentDigest} <> ''
    AND ${table.actorType} <> ''
    AND ${table.reason} <> ''
  `),
]);

export const providerSwitchEntries = pgTable("provider_switch_entries", {
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  providerId: text("provider_id").notNull(),
  scopeType: text("scope_type").notNull(),
  productId: text("product_id").notNull().default(""),
  segment: text("segment").notNull().default(""),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }),
  enabled: boolean("enabled").notNull(),
}, (table) => [
  primaryKey({
    name: "provider_switch_entries_pk",
    columns: [table.generation, table.providerId, table.scopeType, table.productId, table.segment],
  }),
  foreignKey({
    columns: [table.generation],
    foreignColumns: [providerSwitchVersions.generation],
    name: "provider_switch_entries_version_fk",
  }).onDelete("cascade"),
  foreignKey({
    columns: [table.productId, table.catalogGeneration],
    foreignColumns: [productCatalogVersions.productId, productCatalogVersions.generation],
    name: "provider_switch_entries_catalog_fk",
  }).onDelete("restrict"),
  check("provider_switch_entries_identity_check", sql`${table.providerId} <> ''`),
  check("provider_switch_entries_scope_check", sql`
    (
      ${table.scopeType} = 'master'
      AND ${table.productId} = ''
      AND ${table.segment} = ''
      AND ${table.catalogGeneration} IS NULL
    )
    OR (
      ${table.scopeType} = 'product'
      AND ${table.productId} <> ''
      AND ${table.segment} = ''
      AND ${table.catalogGeneration} IS NOT NULL
      AND ${table.catalogGeneration} > 0
    )
    OR (
      ${table.scopeType} = 'segment'
      AND ${table.productId} = 'main'
      AND ${table.segment} IN ('b2c', 'b2b')
      AND ${table.catalogGeneration} IS NOT NULL
      AND ${table.catalogGeneration} > 0
    )
  `),
]);

export const providerSwitchHead = pgTable("provider_switch_head", {
  singleton: integer("singleton").primaryKey(),
  activeGeneration: bigint("active_generation", { mode: "bigint" }).notNull(),
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.activeGeneration],
    foreignColumns: [providerSwitchVersions.generation],
    name: "provider_switch_head_version_fk",
  }).onDelete("restrict"),
  check("provider_switch_head_singleton_check", sql`${table.singleton} = 1`),
]);

export const pricingPolicies = pgTable("pricing_policies", {
  id: text("id").primaryKey(),
  ownerType: text("owner_type").notNull(),
  ownerId: text("owner_id").notNull(),
  productId: text("product_id").notNull(),
  replacementLocked: boolean("replacement_locked").notNull().default(false),
  status: text("status").notNull().default("active"),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("pricing_policies_owner_uidx").on(table.ownerType, table.ownerId, table.productId),
  uniqueIndex("pricing_policies_global_b2c_uidx")
    .on(table.ownerType, table.productId)
    .where(sql`${table.ownerType} = 'global_b2c'`),
  unique("pricing_policies_product_unique").on(table.id, table.productId),
  check("pricing_policies_identity_check", sql`
    ${table.id} <> '' AND ${table.ownerId} <> '' AND ${table.productId} <> ''
  `),
  check("pricing_policies_owner_check", sql`
    ${table.ownerType} IN ('global_b2c', 'b2b_client', 'b2b_invitation', 'service')
    AND (
      ${table.ownerType} = 'service'
      OR ${table.productId} = 'main'
    )
  `),
  check("pricing_policies_status_check", sql`${table.status} IN ('active', 'archived')`),
]);

export const pricingPolicyVersions = pgTable("pricing_policy_versions", {
  policyId: text("policy_id").notNull(),
  version: bigint("version", { mode: "bigint" }).notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  productId: text("product_id").notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  actorType: text("actor_type").notNull(),
  actorId: text("actor_id"),
  reason: text("reason").notNull(),
  createdAt,
}, (table) => [
  primaryKey({ columns: [table.policyId, table.version] }),
  unique("pricing_policy_versions_product_unique")
    .on(table.policyId, table.version, table.productId),
  unique("pricing_policy_versions_catalog_unique")
    .on(table.policyId, table.version, table.productId, table.catalogGeneration),
  unique("pricing_policy_versions_digest_unique")
    .on(
      table.policyId,
      table.version,
      table.productId,
      table.catalogGeneration,
      table.contentDigest,
    ),
  unique("pricing_policy_versions_head_target_unique")
    .on(table.policyId, table.version, table.contentDigest),
  foreignKey({
    columns: [table.policyId, table.productId],
    foreignColumns: [pricingPolicies.id, pricingPolicies.productId],
    name: "pricing_policy_versions_policy_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.productId, table.catalogGeneration],
    foreignColumns: [productCatalogVersions.productId, productCatalogVersions.generation],
    name: "pricing_policy_versions_catalog_fk",
  }).onDelete("restrict"),
  check("pricing_policy_versions_identity_check", sql`
    ${table.policyId} <> ''
    AND ${table.version} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.productId} <> ''
    AND ${table.catalogGeneration} > 0
    AND ${table.contentDigest} <> ''
    AND ${table.actorType} <> ''
    AND ${table.reason} <> ''
  `),
]);

export const pricingPolicyRules = pgTable("pricing_policy_rules", {
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  productId: text("product_id").notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  ruleId: text("rule_id").notNull(),
  ruleDigest: text("rule_digest").notNull(),
  scopeType: text("scope_type").notNull(),
  providerId: text("provider_id").notNull(),
  canonicalModelId: text("canonical_model_id"),
  pricingMode: text("pricing_mode").notNull(),
  ruleOrigin: text("rule_origin").notNull(),
  discountBps: integer("discount_bps"),
  payableMultiplierBp: integer("payable_multiplier_bp"),
  trackEligible: boolean("track_eligible").notNull(),
  retentionEligible: boolean("retention_eligible").notNull(),
  commissionEligible: boolean("commission_eligible").notNull(),
}, (table) => [
  primaryKey({ columns: [table.policyId, table.policyVersion, table.ruleId] }),
  uniqueIndex("pricing_policy_rules_digest_uidx")
    .on(table.policyId, table.policyVersion, table.ruleId, table.ruleDigest),
  uniqueIndex("pricing_policy_rules_provider_scope_uidx")
    .on(table.policyId, table.policyVersion, table.providerId)
    .where(sql`${table.scopeType} = 'provider'`),
  uniqueIndex("pricing_policy_rules_model_scope_uidx")
    .on(table.policyId, table.policyVersion, table.providerId, table.canonicalModelId)
    .where(sql`${table.scopeType} = 'model'`),
  foreignKey({
    columns: [table.policyId, table.policyVersion, table.productId, table.catalogGeneration],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.productId,
      pricingPolicyVersions.catalogGeneration,
    ],
    name: "pricing_policy_rules_version_fk",
  }).onDelete("cascade"),
  foreignKey({
    columns: [
      table.productId,
      table.catalogGeneration,
      table.providerId,
      table.canonicalModelId,
    ],
    foreignColumns: [
      productCatalogEntries.productId,
      productCatalogEntries.generation,
      productCatalogEntries.providerId,
      productCatalogEntries.canonicalModelId,
    ],
    name: "pricing_policy_rules_model_fk",
  }).onDelete("restrict"),
  check("pricing_policy_rules_identity_check", sql`
    ${table.policyId} <> ''
    AND ${table.productId} <> ''
    AND ${table.ruleId} <> ''
    AND ${table.ruleDigest} <> ''
    AND ${table.providerId} <> ''
  `),
  check("pricing_policy_rules_scope_check", sql`
    (${table.scopeType} = 'provider' AND ${table.canonicalModelId} IS NULL)
    OR (
      ${table.scopeType} = 'model'
      AND ${table.canonicalModelId} IS NOT NULL
      AND ${table.canonicalModelId} <> ''
    )
  `),
  check("pricing_policy_rules_pricing_check", sql`
    (
      ${table.pricingMode} = 'track'
      AND ${table.ruleOrigin} = 'managed'
      AND ${table.discountBps} IS NULL
      AND ${table.payableMultiplierBp} IS NULL
      AND ${table.trackEligible}
      AND ${table.retentionEligible}
    )
    OR (
      ${table.pricingMode} = 'discount'
      AND ${table.ruleOrigin} = 'managed'
      AND ${table.discountBps} IS NOT NULL
      AND ${table.discountBps} BETWEEN 0 AND 9500
      AND ${table.discountBps} % 100 = 0
      AND ${table.payableMultiplierBp} IS NOT NULL
      AND ${table.payableMultiplierBp} = 10000 - ${table.discountBps}
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND NOT ${table.commissionEligible}
    )
    OR (
      ${table.pricingMode} = 'discount'
      AND ${table.ruleOrigin} = 'legacy'
      AND ${table.discountBps} IS NULL
      AND ${table.payableMultiplierBp} IS NOT NULL
      AND ${table.payableMultiplierBp} BETWEEN 1 AND 10000
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND NOT ${table.commissionEligible}
    )
  `),
  check("pricing_policy_rules_commission_check", sql`
    NOT ${table.commissionEligible} OR ${table.pricingMode} = 'track'
  `),
]);

export const pricingPolicyHeads = pgTable("pricing_policy_heads", {
  policyId: text("policy_id").primaryKey(),
  currentVersion: bigint("current_version", { mode: "bigint" }).notNull(),
  currentDigest: text("current_digest").notNull(),
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.policyId, table.currentVersion, table.currentDigest],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.contentDigest,
    ],
    name: "pricing_policy_heads_version_fk",
  }).onDelete("restrict"),
  check("pricing_policy_heads_identity_check", sql`
    ${table.policyId} <> '' AND ${table.currentVersion} > 0 AND ${table.currentDigest} <> ''
  `),
]);

export const accountPolicyVersions = pgTable("account_policy_versions", {
  bindingId: uuid("binding_id").notNull(),
  effectiveVersion: bigint("effective_version", { mode: "bigint" }).notNull(),
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  policyDigest: text("policy_digest").notNull(),
  productId: text("product_id").notNull(),
  accountClass: text("account_class").notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  switchGeneration: bigint("switch_generation", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  replacementLocked: boolean("replacement_locked").notNull().default(false),
  createdAt,
}, (table) => [
  primaryKey({ columns: [table.bindingId, table.effectiveVersion] }),
  unique("account_policy_versions_binding_product_unique")
    .on(table.bindingId, table.effectiveVersion, table.productId),
  unique("account_policy_versions_binding_catalog_unique")
    .on(table.bindingId, table.effectiveVersion, table.productId, table.catalogGeneration),
  unique("account_policy_versions_binding_digest_unique")
    .on(
      table.bindingId,
      table.effectiveVersion,
      table.contentDigest,
    ),
  unique("account_policy_versions_job_target_unique")
    .on(
      table.bindingId,
      table.effectiveVersion,
      table.policyId,
      table.policyVersion,
      table.catalogGeneration,
      table.switchGeneration,
      table.schemaVersion,
      table.contentDigest,
    ),
  foreignKey({
    columns: [
      table.policyId,
      table.policyVersion,
      table.productId,
      table.catalogGeneration,
      table.policyDigest,
    ],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.productId,
      pricingPolicyVersions.catalogGeneration,
      pricingPolicyVersions.contentDigest,
    ],
    name: "account_policy_versions_source_policy_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.productId, table.catalogGeneration],
    foreignColumns: [productCatalogVersions.productId, productCatalogVersions.generation],
    name: "account_policy_versions_catalog_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.switchGeneration],
    foreignColumns: [providerSwitchVersions.generation],
    name: "account_policy_versions_switch_fk",
  }).onDelete("restrict"),
  check("account_policy_versions_identity_check", sql`
    ${table.effectiveVersion} > 0
    AND ${table.policyId} <> ''
    AND ${table.policyVersion} > 0
    AND ${table.policyDigest} <> ''
    AND ${table.productId} <> ''
    AND ${table.accountClass} IN ('b2c', 'b2b', 'service')
    AND ${table.schemaVersion} > 0
    AND ${table.catalogGeneration} > 0
    AND ${table.switchGeneration} > 0
    AND ${table.contentDigest} <> ''
  `),
]);

export const accountPolicyRules = pgTable("account_policy_rules", {
  bindingId: uuid("binding_id").notNull(),
  effectiveVersion: bigint("effective_version", { mode: "bigint" }).notNull(),
  productId: text("product_id").notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  ruleId: text("rule_id").notNull(),
  ruleDigest: text("rule_digest").notNull(),
  scopeType: text("scope_type").notNull(),
  providerId: text("provider_id").notNull(),
  canonicalModelId: text("canonical_model_id"),
  pricingMode: text("pricing_mode").notNull(),
  ruleOrigin: text("rule_origin").notNull(),
  discountBps: integer("discount_bps"),
  payableMultiplierBp: integer("payable_multiplier_bp").notNull(),
  trackEligible: boolean("track_eligible").notNull(),
  retentionEligible: boolean("retention_eligible").notNull(),
  commissionEligible: boolean("commission_eligible").notNull(),
}, (table) => [
  primaryKey({ columns: [table.bindingId, table.effectiveVersion, table.ruleId] }),
  uniqueIndex("account_policy_rules_digest_uidx")
    .on(table.bindingId, table.effectiveVersion, table.ruleId, table.ruleDigest),
  uniqueIndex("account_policy_rules_provider_scope_uidx")
    .on(table.bindingId, table.effectiveVersion, table.providerId)
    .where(sql`${table.scopeType} = 'provider'`),
  uniqueIndex("account_policy_rules_model_scope_uidx")
    .on(table.bindingId, table.effectiveVersion, table.providerId, table.canonicalModelId)
    .where(sql`${table.scopeType} = 'model'`),
  foreignKey({
    columns: [
      table.bindingId,
      table.effectiveVersion,
      table.productId,
      table.catalogGeneration,
    ],
    foreignColumns: [
      accountPolicyVersions.bindingId,
      accountPolicyVersions.effectiveVersion,
      accountPolicyVersions.productId,
      accountPolicyVersions.catalogGeneration,
    ],
    name: "account_policy_rules_version_fk",
  }).onDelete("cascade"),
  foreignKey({
    columns: [
      table.productId,
      table.catalogGeneration,
      table.providerId,
      table.canonicalModelId,
    ],
    foreignColumns: [
      productCatalogEntries.productId,
      productCatalogEntries.generation,
      productCatalogEntries.providerId,
      productCatalogEntries.canonicalModelId,
    ],
    name: "account_policy_rules_model_fk",
  }).onDelete("restrict"),
  check("account_policy_rules_identity_check", sql`
    ${table.productId} <> ''
    AND ${table.ruleId} <> ''
    AND ${table.ruleDigest} <> ''
    AND ${table.providerId} <> ''
  `),
  check("account_policy_rules_scope_check", sql`
    (${table.scopeType} = 'provider' AND ${table.canonicalModelId} IS NULL)
    OR (
      ${table.scopeType} = 'model'
      AND ${table.canonicalModelId} IS NOT NULL
      AND ${table.canonicalModelId} <> ''
    )
  `),
  check("account_policy_rules_pricing_check", sql`
    (
      ${table.pricingMode} = 'track'
      AND ${table.ruleOrigin} = 'managed'
      AND ${table.discountBps} IS NULL
      AND ${table.payableMultiplierBp} BETWEEN 0 AND 10000
      AND ${table.trackEligible}
      AND ${table.retentionEligible}
    )
    OR (
      ${table.pricingMode} = 'discount'
      AND ${table.ruleOrigin} = 'managed'
      AND ${table.discountBps} IS NOT NULL
      AND ${table.discountBps} BETWEEN 0 AND 9500
      AND ${table.discountBps} % 100 = 0
      AND ${table.payableMultiplierBp} = 10000 - ${table.discountBps}
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND NOT ${table.commissionEligible}
    )
    OR (
      ${table.pricingMode} = 'discount'
      AND ${table.ruleOrigin} = 'legacy'
      AND ${table.discountBps} IS NULL
      AND ${table.payableMultiplierBp} BETWEEN 1 AND 10000
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND NOT ${table.commissionEligible}
    )
  `),
  check("account_policy_rules_commission_check", sql`
    NOT ${table.commissionEligible} OR ${table.pricingMode} = 'track'
  `),
]);

/**
 * RETIRED PRICING TABLES (2026-08-09).
 *
 * The per-account policy/binding, catalog/switch/release-v2, funding-bucket, shadow-rollout and
 * per-request usage-attribution tables below are no longer read or written by any runtime path.
 * They remain declared because the repository is expand-only and their data is immutable history;
 * a drop migration is a separate, explicit decision.
 *
 * Do not wire anything to them again. Price is `engine_accounts.mult_bp` plus
 * `customer_provider_discounts`; the paid/bonus split of spend is `pricing_usage_events
 * .real_funded_nano` under free-first accounting. Contract: docs/commerce/PRICING_MODEL.md.
 */
export const accountPolicyBindings = pgTable("account_policy_bindings", {
  id: uuid("id").primaryKey(),
  userId: uuid("user_id"),
  engineAccountRecordId: uuid("engine_account_record_id"),
  engineAccountId: text("engine_account_id"),
  accountClass: text("account_class").notNull(),
  productId: text("product_id").notNull(),
  policyId: text("policy_id").notNull(),
  desiredEffectiveVersion: bigint("desired_effective_version", { mode: "bigint" }),
  desiredDigest: text("desired_digest"),
  appliedEffectiveVersion: bigint("applied_effective_version", { mode: "bigint" }),
  appliedDigest: text("applied_digest"),
  policyEnforcement: text("policy_enforcement").notNull().default("legacy_scalar"),
  fundingEnforcement: text("funding_enforcement").notNull().default("legacy_single"),
  reconciliationState: text("reconciliation_state").notNull().default("pending"),
  syncState: text("sync_state").notNull().default("legacy"),
  strictChainPending: boolean("strict_chain_pending").notNull().default(false),
  lastAckAt: timestamp("last_ack_at", { withTimezone: true }),
  lastError: text("last_error"),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("account_policy_bindings_user_uidx")
    .on(table.userId)
    .where(sql`${table.userId} IS NOT NULL`),
  uniqueIndex("account_policy_bindings_engine_record_uidx")
    .on(table.engineAccountRecordId)
    .where(sql`${table.engineAccountRecordId} IS NOT NULL`),
  uniqueIndex("account_policy_bindings_engine_account_uidx")
    .on(table.engineAccountId)
    .where(sql`${table.engineAccountId} IS NOT NULL`),
  unique("account_policy_bindings_engine_target_unique")
    .on(table.id, table.engineAccountId),
  unique("account_policy_bindings_invite_copy_unique")
    .on(table.id, table.userId, table.policyId),
  index("account_policy_bindings_sync_idx")
    .on(table.syncState, table.reconciliationState, table.updatedAt),
  foreignKey({
    columns: [table.userId],
    foreignColumns: [users.id],
    name: "account_policy_bindings_user_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.engineAccountRecordId],
    foreignColumns: [engineAccounts.id],
    name: "account_policy_bindings_engine_record_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.policyId, table.productId],
    foreignColumns: [pricingPolicies.id, pricingPolicies.productId],
    name: "account_policy_bindings_policy_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.id,
      table.desiredEffectiveVersion,
      table.desiredDigest,
    ],
    foreignColumns: [
      accountPolicyVersions.bindingId,
      accountPolicyVersions.effectiveVersion,
      accountPolicyVersions.contentDigest,
    ],
    name: "account_policy_bindings_desired_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.id,
      table.appliedEffectiveVersion,
      table.appliedDigest,
    ],
    foreignColumns: [
      accountPolicyVersions.bindingId,
      accountPolicyVersions.effectiveVersion,
      accountPolicyVersions.contentDigest,
    ],
    name: "account_policy_bindings_applied_fk",
  }).onDelete("restrict"),
  check("account_policy_bindings_identity_check", sql`
    ${table.productId} <> ''
    AND ${table.policyId} <> ''
    AND (
      (
        ${table.accountClass} IN ('b2c', 'b2b')
        AND ${table.userId} IS NOT NULL
        AND ${table.engineAccountRecordId} IS NOT NULL
        AND ${table.engineAccountId} IS NOT NULL
        AND ${table.engineAccountId} <> ''
      )
      OR (
        ${table.accountClass} = 'service'
        AND ${table.userId} IS NULL
        AND ${table.engineAccountRecordId} IS NULL
        AND ${table.engineAccountId} IS NOT NULL
        AND ${table.engineAccountId} <> ''
      )
    )
  `),
  check("account_policy_bindings_desired_shape_check", sql`
    (${table.desiredEffectiveVersion} IS NULL AND ${table.desiredDigest} IS NULL)
    OR (
      ${table.desiredEffectiveVersion} IS NOT NULL
      AND
      ${table.desiredEffectiveVersion} > 0
      AND ${table.desiredDigest} IS NOT NULL
      AND ${table.desiredDigest} <> ''
    )
  `),
  check("account_policy_bindings_applied_shape_check", sql`
    (
      ${table.appliedEffectiveVersion} IS NULL
      AND ${table.appliedDigest} IS NULL
      AND ${table.lastAckAt} IS NULL
    )
    OR (
      ${table.appliedEffectiveVersion} IS NOT NULL
      AND
      ${table.appliedEffectiveVersion} > 0
      AND ${table.appliedDigest} IS NOT NULL
      AND ${table.appliedDigest} <> ''
      AND ${table.lastAckAt} IS NOT NULL
    )
  `),
  check("account_policy_bindings_enforcement_check", sql`
    ${table.policyEnforcement} IN ('legacy_scalar', 'shadow', 'strict')
    AND ${table.fundingEnforcement} IN ('legacy_single', 'shadow', 'strict')
    AND ${table.reconciliationState} IN ('pending', 'verified', 'exception')
    AND ${table.syncState} IN ('legacy', 'pending', 'confirmed', 'failed')
    AND (
      ${table.policyEnforcement} = 'legacy_scalar'
      OR ${table.desiredEffectiveVersion} IS NOT NULL
    )
    AND (
      ${table.policyEnforcement} <> 'strict'
      OR (
        ${table.appliedEffectiveVersion} IS NOT NULL
        AND ${table.reconciliationState} = 'verified'
      )
    )
    AND (
      ${table.fundingEnforcement} <> 'strict'
      OR ${table.reconciliationState} = 'verified'
    )
    AND (
      ${table.appliedEffectiveVersion} IS NULL
      OR ${table.desiredEffectiveVersion} IS NULL
      OR ${table.appliedEffectiveVersion} <= ${table.desiredEffectiveVersion}
    )
    AND (
      ${table.syncState} <> 'confirmed'
      OR (
        ${table.desiredEffectiveVersion} IS NOT NULL
        AND ${table.appliedEffectiveVersion} IS NOT NULL
        AND ${table.appliedEffectiveVersion} = ${table.desiredEffectiveVersion}
        AND ${table.desiredDigest} IS NOT NULL
        AND ${table.appliedDigest} IS NOT NULL
        AND ${table.appliedDigest} = ${table.desiredDigest}
      )
    )
  `),
]);

export const businessInvitePolicyBindings = pgTable("business_invite_policy_bindings", {
  inviteId: uuid("invite_id").primaryKey(),
  invitationPolicyId: text("invitation_policy_id").notNull(),
  currentPolicyVersion: bigint("current_policy_version", { mode: "bigint" }).notNull(),
  currentPolicyDigest: text("current_policy_digest").notNull(),
  redeemedSourcePolicyVersion: bigint("redeemed_source_policy_version", { mode: "bigint" }),
  redeemedSourcePolicyDigest: text("redeemed_source_policy_digest"),
  copiedToUserId: uuid("copied_to_user_id"),
  copiedToBindingId: uuid("copied_to_binding_id"),
  copiedClientPolicyId: text("copied_client_policy_id"),
  copiedClientPolicyVersion: bigint("copied_client_policy_version", { mode: "bigint" }),
  copiedClientPolicyDigest: text("copied_client_policy_digest"),
  redeemedAt: timestamp("redeemed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.inviteId],
    foreignColumns: [businessInvites.id],
    name: "business_invite_policy_bindings_invite_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.copiedToUserId],
    foreignColumns: [users.id],
    name: "business_invite_policy_bindings_user_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.copiedToBindingId],
    foreignColumns: [accountPolicyBindings.id],
    name: "business_invite_policy_bindings_binding_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.copiedToBindingId,
      table.copiedToUserId,
      table.copiedClientPolicyId,
    ],
    foreignColumns: [
      accountPolicyBindings.id,
      accountPolicyBindings.userId,
      accountPolicyBindings.policyId,
    ],
    name: "business_invite_policy_bindings_copy_target_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.invitationPolicyId,
      table.currentPolicyVersion,
      table.currentPolicyDigest,
    ],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.contentDigest,
    ],
    name: "business_invite_policy_bindings_current_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.invitationPolicyId,
      table.redeemedSourcePolicyVersion,
      table.redeemedSourcePolicyDigest,
    ],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.contentDigest,
    ],
    name: "business_invite_policy_bindings_redeemed_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.copiedClientPolicyId,
      table.copiedClientPolicyVersion,
      table.copiedClientPolicyDigest,
    ],
    foreignColumns: [
      pricingPolicyVersions.policyId,
      pricingPolicyVersions.version,
      pricingPolicyVersions.contentDigest,
    ],
    name: "business_invite_policy_bindings_copied_fk",
  }).onDelete("restrict"),
  check("business_invite_policy_bindings_current_check", sql`
    ${table.invitationPolicyId} <> ''
    AND ${table.currentPolicyVersion} > 0
    AND ${table.currentPolicyDigest} <> ''
  `),
  check("business_invite_policy_bindings_redemption_check", sql`
    (
      ${table.redeemedSourcePolicyVersion} IS NULL
      AND ${table.redeemedSourcePolicyDigest} IS NULL
      AND ${table.copiedToUserId} IS NULL
      AND ${table.copiedToBindingId} IS NULL
      AND ${table.copiedClientPolicyId} IS NULL
      AND ${table.copiedClientPolicyVersion} IS NULL
      AND ${table.copiedClientPolicyDigest} IS NULL
      AND ${table.redeemedAt} IS NULL
    )
    OR (
      ${table.redeemedSourcePolicyVersion} IS NOT NULL
      AND
      ${table.redeemedSourcePolicyVersion} > 0
      AND ${table.redeemedSourcePolicyDigest} IS NOT NULL
      AND ${table.redeemedSourcePolicyDigest} <> ''
      AND ${table.redeemedSourcePolicyVersion} = ${table.currentPolicyVersion}
      AND ${table.redeemedSourcePolicyDigest} = ${table.currentPolicyDigest}
      AND ${table.copiedToUserId} IS NOT NULL
      AND ${table.copiedToBindingId} IS NOT NULL
      AND ${table.copiedClientPolicyId} IS NOT NULL
      AND ${table.copiedClientPolicyId} <> ''
      AND ${table.copiedClientPolicyVersion} IS NOT NULL
      AND ${table.copiedClientPolicyVersion} > 0
      AND ${table.copiedClientPolicyDigest} IS NOT NULL
      AND ${table.copiedClientPolicyDigest} <> ''
      AND ${table.redeemedAt} IS NOT NULL
    )
  `),
]);

export const engineCatalogJobs = pgTable("engine_catalog_jobs", {
  id: uuid("id").primaryKey(),
  productId: text("product_id").notNull(),
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  payload: jsonb("payload").notNull(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  ackGeneration: bigint("ack_generation", { mode: "bigint" }),
  ackSchemaVersion: bigint("ack_schema_version", { mode: "bigint" }),
  ackContentDigest: text("ack_content_digest"),
  ackPayload: jsonb("ack_payload"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_catalog_jobs_target_uidx").on(table.productId, table.generation),
  index("engine_catalog_jobs_claim_idx")
    .on(table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  foreignKey({
    columns: [table.productId, table.generation, table.schemaVersion, table.contentDigest],
    foreignColumns: [
      productCatalogVersions.productId,
      productCatalogVersions.generation,
      productCatalogVersions.schemaVersion,
      productCatalogVersions.contentDigest,
    ],
    name: "engine_catalog_jobs_target_fk",
  }).onDelete("restrict"),
  check("engine_catalog_jobs_target_check", sql`
    ${table.productId} <> ''
    AND ${table.generation} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.contentDigest} <> ''
    AND jsonb_typeof(${table.payload}) = 'object'
  `),
  check("engine_catalog_jobs_state_check", sql`
    ${table.status} IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND ${table.attempts} >= 0
    AND (
      (${table.status} = 'processing' AND ${table.lockedAt} IS NOT NULL AND ${table.lockedBy} IS NOT NULL)
      OR (${table.status} <> 'processing' AND ${table.lockedAt} IS NULL AND ${table.lockedBy} IS NULL)
    )
  `),
  check("engine_catalog_jobs_ack_check", sql`
    (
      ${table.status} <> 'confirmed'
      AND ${table.ackGeneration} IS NULL
      AND ${table.ackSchemaVersion} IS NULL
      AND ${table.ackContentDigest} IS NULL
      AND ${table.ackPayload} IS NULL
      AND ${table.confirmedAt} IS NULL
    )
    OR (
      ${table.status} = 'confirmed'
      AND ${table.ackGeneration} IS NOT NULL
      AND ${table.ackGeneration} = ${table.generation}
      AND ${table.ackSchemaVersion} IS NOT NULL
      AND ${table.ackSchemaVersion} = ${table.schemaVersion}
      AND ${table.ackContentDigest} IS NOT NULL
      AND ${table.ackContentDigest} = ${table.contentDigest}
      AND ${table.ackPayload} IS NOT NULL
      AND jsonb_typeof(${table.ackPayload}) = 'object'
      AND ${table.confirmedAt} IS NOT NULL
    )
  `),
]);

export const engineSwitchJobs = pgTable("engine_switch_jobs", {
  id: uuid("id").primaryKey(),
  generation: bigint("generation", { mode: "bigint" }).notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  payload: jsonb("payload").notNull(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  ackGeneration: bigint("ack_generation", { mode: "bigint" }),
  ackSchemaVersion: bigint("ack_schema_version", { mode: "bigint" }),
  ackContentDigest: text("ack_content_digest"),
  ackPayload: jsonb("ack_payload"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_switch_jobs_target_uidx").on(table.generation),
  index("engine_switch_jobs_claim_idx")
    .on(table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  foreignKey({
    columns: [table.generation, table.schemaVersion, table.contentDigest],
    foreignColumns: [
      providerSwitchVersions.generation,
      providerSwitchVersions.schemaVersion,
      providerSwitchVersions.contentDigest,
    ],
    name: "engine_switch_jobs_target_fk",
  }).onDelete("restrict"),
  check("engine_switch_jobs_target_check", sql`
    ${table.generation} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.contentDigest} <> ''
    AND jsonb_typeof(${table.payload}) = 'object'
  `),
  check("engine_switch_jobs_state_check", sql`
    ${table.status} IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND ${table.attempts} >= 0
    AND (
      (${table.status} = 'processing' AND ${table.lockedAt} IS NOT NULL AND ${table.lockedBy} IS NOT NULL)
      OR (${table.status} <> 'processing' AND ${table.lockedAt} IS NULL AND ${table.lockedBy} IS NULL)
    )
  `),
  check("engine_switch_jobs_ack_check", sql`
    (
      ${table.status} <> 'confirmed'
      AND ${table.ackGeneration} IS NULL
      AND ${table.ackSchemaVersion} IS NULL
      AND ${table.ackContentDigest} IS NULL
      AND ${table.ackPayload} IS NULL
      AND ${table.confirmedAt} IS NULL
    )
    OR (
      ${table.status} = 'confirmed'
      AND ${table.ackGeneration} IS NOT NULL
      AND ${table.ackGeneration} = ${table.generation}
      AND ${table.ackSchemaVersion} IS NOT NULL
      AND ${table.ackSchemaVersion} = ${table.schemaVersion}
      AND ${table.ackContentDigest} IS NOT NULL
      AND ${table.ackContentDigest} = ${table.contentDigest}
      AND ${table.ackPayload} IS NOT NULL
      AND jsonb_typeof(${table.ackPayload}) = 'object'
      AND ${table.confirmedAt} IS NOT NULL
    )
  `),
]);

export const enginePolicyJobs = pgTable("engine_policy_jobs", {
  id: uuid("id").primaryKey(),
  bindingId: uuid("binding_id").notNull(),
  effectiveVersion: bigint("effective_version", { mode: "bigint" }).notNull(),
  engineAccountId: text("engine_account_id").notNull(),
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  switchGeneration: bigint("switch_generation", { mode: "bigint" }).notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  payload: jsonb("payload").notNull(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  ackEffectiveVersion: bigint("ack_effective_version", { mode: "bigint" }),
  ackPolicyVersion: bigint("ack_policy_version", { mode: "bigint" }),
  ackCatalogGeneration: bigint("ack_catalog_generation", { mode: "bigint" }),
  ackSwitchGeneration: bigint("ack_switch_generation", { mode: "bigint" }),
  ackSchemaVersion: bigint("ack_schema_version", { mode: "bigint" }),
  ackContentDigest: text("ack_content_digest"),
  ackPayload: jsonb("ack_payload"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("engine_policy_jobs_target_uidx").on(table.bindingId, table.effectiveVersion),
  index("engine_policy_jobs_claim_idx")
    .on(table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  foreignKey({
    columns: [table.bindingId, table.engineAccountId],
    foreignColumns: [accountPolicyBindings.id, accountPolicyBindings.engineAccountId],
    name: "engine_policy_jobs_binding_target_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.bindingId,
      table.effectiveVersion,
      table.policyId,
      table.policyVersion,
      table.catalogGeneration,
      table.switchGeneration,
      table.schemaVersion,
      table.contentDigest,
    ],
    foreignColumns: [
      accountPolicyVersions.bindingId,
      accountPolicyVersions.effectiveVersion,
      accountPolicyVersions.policyId,
      accountPolicyVersions.policyVersion,
      accountPolicyVersions.catalogGeneration,
      accountPolicyVersions.switchGeneration,
      accountPolicyVersions.schemaVersion,
      accountPolicyVersions.contentDigest,
    ],
    name: "engine_policy_jobs_target_fk",
  }).onDelete("restrict"),
  check("engine_policy_jobs_target_check", sql`
    ${table.effectiveVersion} > 0
    AND ${table.engineAccountId} <> ''
    AND ${table.policyId} <> ''
    AND ${table.policyVersion} > 0
    AND ${table.catalogGeneration} > 0
    AND ${table.switchGeneration} > 0
    AND ${table.schemaVersion} > 0
    AND ${table.contentDigest} <> ''
    AND jsonb_typeof(${table.payload}) = 'object'
  `),
  check("engine_policy_jobs_state_check", sql`
    ${table.status} IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND ${table.attempts} >= 0
    AND (
      (${table.status} = 'processing' AND ${table.lockedAt} IS NOT NULL AND ${table.lockedBy} IS NOT NULL)
      OR (${table.status} <> 'processing' AND ${table.lockedAt} IS NULL AND ${table.lockedBy} IS NULL)
    )
  `),
  check("engine_policy_jobs_ack_check", sql`
    (
      ${table.status} <> 'confirmed'
      AND ${table.ackEffectiveVersion} IS NULL
      AND ${table.ackPolicyVersion} IS NULL
      AND ${table.ackCatalogGeneration} IS NULL
      AND ${table.ackSwitchGeneration} IS NULL
      AND ${table.ackSchemaVersion} IS NULL
      AND ${table.ackContentDigest} IS NULL
      AND ${table.ackPayload} IS NULL
      AND ${table.confirmedAt} IS NULL
    )
    OR (
      ${table.status} = 'confirmed'
      AND ${table.ackEffectiveVersion} IS NOT NULL
      AND ${table.ackEffectiveVersion} = ${table.effectiveVersion}
      AND ${table.ackPolicyVersion} IS NOT NULL
      AND ${table.ackPolicyVersion} = ${table.policyVersion}
      AND ${table.ackCatalogGeneration} IS NOT NULL
      AND ${table.ackCatalogGeneration} = ${table.catalogGeneration}
      AND ${table.ackSwitchGeneration} IS NOT NULL
      AND ${table.ackSwitchGeneration} = ${table.switchGeneration}
      AND ${table.ackSchemaVersion} IS NOT NULL
      AND ${table.ackSchemaVersion} = ${table.schemaVersion}
      AND ${table.ackContentDigest} IS NOT NULL
      AND ${table.ackContentDigest} = ${table.contentDigest}
      AND ${table.ackPayload} IS NOT NULL
      AND jsonb_typeof(${table.ackPayload}) = 'object'
      AND ${table.confirmedAt} IS NOT NULL
    )
  `),
]);

export const pricingUsageAttributions = pgTable("pricing_usage_attributions", {
  pricingUsageEventId: uuid("pricing_usage_event_id").primaryKey(),
  attributionSchemaVersion: bigint("attribution_schema_version", { mode: "bigint" }).notNull(),
  snapshotKind: text("snapshot_kind").notNull(),
  engineRequestId: text("engine_request_id"),
  providerId: text("provider_id"),
  productId: text("product_id"),
  accountClass: text("account_class"),
  bindingId: uuid("binding_id"),
  requestedModelId: text("requested_model_id"),
  canonicalModelId: text("canonical_model_id"),
  servedModelId: text("served_model_id"),
  servedCanonicalModelId: text("served_canonical_model_id"),
  billingInvariantCode: text("billing_invariant_code"),
  aliasGeneration: bigint("alias_generation", { mode: "bigint" }),
  ruleId: text("rule_id"),
  ruleDigest: text("rule_digest"),
  ruleScope: text("rule_scope"),
  pricingMode: text("pricing_mode"),
  ruleOrigin: text("rule_origin"),
  discountBps: integer("discount_bps"),
  payableMultiplierBp: integer("payable_multiplier_bp"),
  policyId: text("policy_id"),
  policyVersion: bigint("policy_version", { mode: "bigint" }),
  effectivePolicyVersion: bigint("effective_policy_version", { mode: "bigint" }),
  effectivePolicyDigest: text("effective_policy_digest"),
  policyDigest: text("policy_digest"),
  sourcePolicyDigest: text("source_policy_digest"),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }),
  switchGeneration: bigint("switch_generation", { mode: "bigint" }),
  admissionCatalogGeneration: bigint("admission_catalog_generation", { mode: "bigint" }),
  admissionCatalogDigest: text("admission_catalog_digest"),
  admissionSwitchGeneration: bigint("admission_switch_generation", { mode: "bigint" }),
  admissionSwitchDigest: text("admission_switch_digest"),
  runtimeManifestGeneration: bigint("runtime_manifest_generation", { mode: "bigint" }),
  runtimeManifestDigest: text("runtime_manifest_digest"),
  tariffScheduleId: text("tariff_schedule_id"),
  tariffPricedAt: timestamp("tariff_priced_at", { withTimezone: true }),
  officialNano: bigint("official_nano", { mode: "bigint" }),
  chargedNano: bigint("charged_nano", { mode: "bigint" }).notNull(),
  officialCostJson: jsonb("official_cost_json"),
  paidFundedNano: bigint("paid_funded_nano", { mode: "bigint" }),
  bonusFundedNano: bigint("bonus_funded_nano", { mode: "bigint" }),
  otherFundedNano: bigint("other_funded_nano", { mode: "bigint" }),
  fundingAllocationJson: jsonb("funding_allocation_json"),
  trackEligible: boolean("track_eligible").notNull(),
  retentionEligible: boolean("retention_eligible").notNull(),
  commissionEligible: boolean("commission_eligible").notNull(),
  snapshotDigest: text("snapshot_digest").notNull(),
  releaseSchemaVersion: bigint("release_schema_version", { mode: "bigint" }),
  releaseGeneration: bigint("release_generation", { mode: "bigint" }),
  releaseDigest: text("release_digest"),
  releaseBillingMode: text("release_billing_mode"),
  releaseFundingGeneration: bigint("release_funding_generation", { mode: "bigint" }),
  createdAt,
}, (table) => [
  // The atomic ledger consumer stores immutable raw engine evidence here and treats normalized
  // pricing_usage_funding_allocations rows as query authority after exact local reconciliation.
  index("pricing_usage_attributions_policy_idx")
    .on(table.policyId, table.policyVersion)
    .where(sql`${table.policyId} IS NOT NULL`),
  index("pricing_usage_attributions_provider_model_idx")
    .on(table.providerId, table.canonicalModelId)
    .where(sql`${table.providerId} IS NOT NULL`),
  foreignKey({
    columns: [table.pricingUsageEventId],
    foreignColumns: [pricingUsageEvents.id],
    name: "pricing_usage_attributions_event_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [
      table.bindingId,
      table.effectivePolicyVersion,
      table.effectivePolicyDigest,
    ],
    foreignColumns: [
      accountPolicyVersions.bindingId,
      accountPolicyVersions.effectiveVersion,
      accountPolicyVersions.contentDigest,
    ],
    name: "pricing_usage_attributions_effective_fk",
  }).onDelete("restrict"),
  check("pricing_usage_attributions_base_check", sql`
    ${table.attributionSchemaVersion} > 0
    AND ${table.snapshotKind} IN ('policy_v1', 'legacy_scalar', 'legacy_b2c_track', 'release_v2')
    AND (${table.pricingMode} IS NULL OR ${table.pricingMode} IN ('track', 'discount', 'legacy_scalar'))
    AND (${table.ruleOrigin} IS NULL OR ${table.ruleOrigin} IN ('managed', 'legacy'))
    AND ${table.chargedNano} > 0
    AND ${table.snapshotDigest} <> ''
    AND (${table.engineRequestId} IS NULL OR ${table.engineRequestId} <> '')
    AND (${table.servedModelId} IS NULL OR ${table.servedModelId} <> '')
    AND (
      ${table.servedCanonicalModelId} IS NULL
      OR ${table.servedCanonicalModelId} <> ''
    )
    AND (
      ${table.billingInvariantCode} IS NULL
      OR ${table.billingInvariantCode} <> ''
    )
    AND (${table.officialNano} IS NULL OR ${table.officialNano} >= 0)
    AND (
      ${table.payableMultiplierBp} IS NULL
      OR ${table.payableMultiplierBp} BETWEEN 0 AND 10000
    )
    AND (
      ${table.discountBps} IS NULL
      OR (
        ${table.discountBps} BETWEEN 0 AND 9500
        AND ${table.discountBps} % 100 = 0
      )
    )
    AND (
      ${table.officialCostJson} IS NULL
      OR jsonb_typeof(${table.officialCostJson}) = 'object'
    )
    AND (NOT ${table.commissionEligible} OR ${table.trackEligible}
      OR ${table.snapshotKind} = 'release_v2')
  `),
  check("pricing_usage_attributions_funding_check", sql`
    (
      ${table.paidFundedNano} IS NULL
      AND ${table.bonusFundedNano} IS NULL
      AND ${table.otherFundedNano} IS NULL
      AND ${table.fundingAllocationJson} IS NULL
    )
    OR (
      ${table.paidFundedNano} IS NOT NULL
      AND ${table.paidFundedNano} >= 0
      AND ${table.bonusFundedNano} IS NOT NULL
      AND ${table.bonusFundedNano} >= 0
      AND ${table.otherFundedNano} IS NOT NULL
      AND ${table.otherFundedNano} >= 0
      AND ${table.paidFundedNano} + ${table.bonusFundedNano} + ${table.otherFundedNano}
        = ${table.chargedNano}
      AND ${table.fundingAllocationJson} IS NOT NULL
      AND jsonb_typeof(${table.fundingAllocationJson}) = 'array'
    )
  `),
  check("pricing_usage_attributions_snapshot_check", sql`
    (
      ${table.snapshotKind} = 'policy_v1'
      AND ${table.engineRequestId} IS NOT NULL AND ${table.engineRequestId} <> ''
      AND ${table.providerId} IS NOT NULL AND ${table.providerId} <> ''
      AND ${table.productId} IS NOT NULL AND ${table.productId} <> ''
      AND ${table.accountClass} IS NOT NULL
      AND ${table.accountClass} IN ('b2c', 'b2b', 'service')
      AND ${table.requestedModelId} IS NOT NULL AND ${table.requestedModelId} <> ''
      AND ${table.canonicalModelId} IS NOT NULL AND ${table.canonicalModelId} <> ''
      AND (${table.servedModelId} IS NULL OR ${table.servedModelId} <> '')
      AND (
        ${table.servedCanonicalModelId} IS NULL
        OR ${table.servedCanonicalModelId} <> ''
      )
      AND (
        ${table.billingInvariantCode} IS NULL
        OR ${table.billingInvariantCode} <> ''
      )
      AND ${table.aliasGeneration} IS NOT NULL
      AND ${table.aliasGeneration} > 0
      AND ${table.ruleId} IS NOT NULL AND ${table.ruleId} <> ''
      AND ${table.ruleDigest} IS NOT NULL AND ${table.ruleDigest} <> ''
      AND ${table.ruleScope} IS NOT NULL
      AND ${table.ruleScope} IN ('provider', 'model')
      AND ${table.policyId} IS NOT NULL AND ${table.policyId} <> ''
      AND ${table.policyVersion} IS NOT NULL
      AND ${table.policyVersion} > 0
      AND ${table.effectivePolicyVersion} IS NOT NULL
      AND ${table.effectivePolicyVersion} > 0
      AND ${table.policyDigest} IS NOT NULL AND ${table.policyDigest} <> ''
      AND ${table.catalogGeneration} IS NOT NULL
      AND ${table.catalogGeneration} > 0
      AND ${table.switchGeneration} IS NOT NULL
      AND ${table.switchGeneration} > 0
      AND ${table.tariffScheduleId} IS NOT NULL AND ${table.tariffScheduleId} <> ''
      AND ${table.tariffPricedAt} IS NOT NULL
      AND ${table.officialNano} IS NOT NULL
      AND ${table.officialCostJson} IS NOT NULL
      AND ${table.payableMultiplierBp} IS NOT NULL
      AND (
        (
          ${table.pricingMode} = 'track'
          AND ${table.ruleOrigin} = 'managed'
          AND ${table.discountBps} IS NULL
          AND ${table.trackEligible}
          AND ${table.retentionEligible}
        )
        OR (
          ${table.pricingMode} = 'discount'
          AND ${table.ruleOrigin} = 'managed'
          AND ${table.discountBps} IS NOT NULL
          AND ${table.payableMultiplierBp} = 10000 - ${table.discountBps}
          AND NOT ${table.trackEligible}
          AND NOT ${table.retentionEligible}
          AND NOT ${table.commissionEligible}
        )
        OR (
          ${table.pricingMode} = 'discount'
          AND ${table.ruleOrigin} = 'legacy'
          AND ${table.discountBps} IS NULL
          AND ${table.payableMultiplierBp} BETWEEN 1 AND 10000
          AND NOT ${table.trackEligible}
          AND NOT ${table.retentionEligible}
          AND NOT ${table.commissionEligible}
        )
      )
    )
    OR (
      ${table.snapshotKind} = 'legacy_scalar'
      AND ${table.pricingMode} = 'legacy_scalar'
      AND ${table.ruleOrigin} = 'legacy'
      AND ${table.discountBps} IS NULL
      AND ${table.payableMultiplierBp} IS NOT NULL
      AND ${table.payableMultiplierBp} BETWEEN 0 AND 10000
      AND ${table.policyId} IS NULL
      AND ${table.policyVersion} IS NULL
      AND ${table.effectivePolicyVersion} IS NULL
      AND ${table.policyDigest} IS NULL
      AND ${table.catalogGeneration} IS NULL
      AND ${table.switchGeneration} IS NULL
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND NOT ${table.commissionEligible}
    )
    OR (
      ${table.snapshotKind} = 'legacy_b2c_track'
      AND ${table.pricingMode} = 'track'
      AND ${table.ruleOrigin} = 'legacy'
      AND ${table.discountBps} IS NULL
      AND ${table.policyId} IS NULL
      AND ${table.policyVersion} IS NULL
      AND ${table.effectivePolicyVersion} IS NULL
      AND ${table.policyDigest} IS NULL
      AND ${table.catalogGeneration} IS NULL
      AND ${table.switchGeneration} IS NULL
      AND ${table.trackEligible}
      AND ${table.retentionEligible}
    )
    OR (
      ${table.snapshotKind} = 'release_v2'
      AND ${table.engineRequestId} IS NOT NULL AND ${table.engineRequestId} <> ''
      AND ${table.providerId} IS NOT NULL AND ${table.providerId} <> ''
      AND ${table.accountClass} IS NOT NULL
      AND ${table.accountClass} IN ('b2c', 'b2b', 'openkeys', 'service')
      AND ${table.requestedModelId} IS NOT NULL AND ${table.requestedModelId} <> ''
      AND ${table.canonicalModelId} IS NOT NULL AND ${table.canonicalModelId} <> ''
      AND (${table.servedModelId} IS NULL OR ${table.servedModelId} <> '')
      AND (
        ${table.servedCanonicalModelId} IS NULL
        OR ${table.servedCanonicalModelId} <> ''
      )
      AND (
        ${table.ruleId} IS NULL
        OR (
          ${table.ruleId} <> ''
          AND ${table.ruleDigest} IS NOT NULL AND ${table.ruleDigest} <> ''
          AND ${table.ruleScope} IS NOT NULL
          AND ${table.ruleScope} IN ('global', 'provider', 'model')
          AND ${table.payableMultiplierBp} IS NOT NULL
          AND (
            ${table.discountBps} IS NULL
            OR ${table.payableMultiplierBp} = 10000 - ${table.discountBps}
          )
        )
      )
      AND ${table.policyId} IS NOT NULL AND ${table.policyId} <> ''
      AND ${table.policyVersion} IS NOT NULL
      AND ${table.policyVersion} > 0
      AND ${table.policyDigest} IS NOT NULL AND ${table.policyDigest} <> ''
      AND ${table.tariffScheduleId} IS NOT NULL AND ${table.tariffScheduleId} <> ''
      AND ${table.tariffPricedAt} IS NOT NULL
      AND ${table.officialNano} IS NOT NULL
      AND ${table.officialCostJson} IS NOT NULL
      AND ${table.pricingMode} IS NULL
      AND ${table.ruleOrigin} IS NULL
      AND NOT ${table.trackEligible}
      AND NOT ${table.retentionEligible}
      AND ${table.releaseSchemaVersion} IS NOT NULL
      AND ${table.releaseSchemaVersion} >= 2
      AND ${table.releaseGeneration} IS NOT NULL
      AND ${table.releaseGeneration} > 0
      AND ${table.releaseDigest} IS NOT NULL AND ${table.releaseDigest} <> ''
      AND ${table.releaseBillingMode} IS NOT NULL
      AND ${table.releaseBillingMode} IN ('balance', 'meter_only')
      AND (
        (
          ${table.releaseBillingMode} = 'balance'
          AND ${table.releaseFundingGeneration} IS NOT NULL
          AND ${table.releaseFundingGeneration} > 0
        )
        OR (
          ${table.releaseBillingMode} = 'meter_only'
          AND ${table.releaseFundingGeneration} IS NULL
        )
      )
    )
  `),
  check("pricing_usage_attributions_effective_check", sql`
    (
      ${table.snapshotKind} = 'policy_v1'
      AND ${table.bindingId} IS NOT NULL
      AND ${table.effectivePolicyVersion} IS NOT NULL
      AND ${table.effectivePolicyDigest} IS NOT NULL
      AND ${table.effectivePolicyDigest} <> ''
    )
    OR (
      ${table.snapshotKind} IN ('legacy_scalar', 'legacy_b2c_track', 'release_v2')
      AND ${table.bindingId} IS NULL
      AND ${table.effectivePolicyDigest} IS NULL
    )
  `),
  check("pricing_usage_attributions_policy_funding_check", sql`
    ${table.snapshotKind} NOT IN ('policy_v1', 'release_v2')
    OR (
      ${table.paidFundedNano} IS NOT NULL
      AND ${table.bonusFundedNano} IS NOT NULL
      AND ${table.otherFundedNano} IS NOT NULL
      AND ${table.fundingAllocationJson} IS NOT NULL
    )
  `),
]);

export const pricingUsageFundingAllocations = pgTable("pricing_usage_funding_allocations", {
  pricingUsageEventId: uuid("pricing_usage_event_id").notNull(),
  ordinal: integer("ordinal").notNull(),
  engineBucketId: text("engine_bucket_id").notNull(),
  bucketVersion: bigint("bucket_version", { mode: "bigint" }).notNull(),
  sourceType: text("source_type").notNull(),
  sourceRef: text("source_ref").notNull().default(""),
  amountNano: bigint("amount_nano", { mode: "bigint" }).notNull(),
}, (table) => [
  primaryKey({
    name: "pricing_usage_funding_allocations_pk",
    columns: [table.pricingUsageEventId, table.ordinal],
  }),
  index("pricing_usage_funding_allocations_source_idx")
    .on(table.sourceType, table.sourceRef),
  foreignKey({
    columns: [table.pricingUsageEventId],
    foreignColumns: [pricingUsageAttributions.pricingUsageEventId],
    name: "pricing_usage_funding_allocations_attribution_fk",
  }).onDelete("restrict"),
  check("pricing_usage_funding_allocations_shape_check", sql`
    ${table.ordinal} >= 0
    AND ${table.bucketVersion} > 0
    AND ${table.sourceType} <> ''
    AND ${table.amountNano} > 0
    AND ${table.engineBucketId} <> ''
  `),
]);

export const accountPolicyReconciliations = pgTable("account_policy_reconciliations", {
  id: uuid("id").primaryKey(),
  bindingId: uuid("binding_id").notNull(),
  effectiveVersion: bigint("effective_version", { mode: "bigint" }),
  scope: text("scope").notNull(),
  status: text("status").notNull().default("pending"),
  legacyAccountClass: text("legacy_account_class"),
  legacyMultiplierBp: integer("legacy_multiplier_bp"),
  observedBalanceNano: bigint("observed_balance_nano", { mode: "bigint" }),
  observedReservedNano: bigint("observed_reserved_nano", { mode: "bigint" }),
  observedSpentNano: bigint("observed_spent_nano", { mode: "bigint" }),
  expectedDigest: text("expected_digest"),
  observedDigest: text("observed_digest"),
  exceptionCode: text("exception_code"),
  details: jsonb("details").notNull(),
  startedAt: timestamp("started_at", { withTimezone: true }).notNull().defaultNow(),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
}, (table) => [
  index("account_policy_reconciliations_binding_idx")
    .on(table.bindingId, table.createdAt),
  index("account_policy_reconciliations_status_idx")
    .on(table.status, table.createdAt),
  foreignKey({
    columns: [table.bindingId],
    foreignColumns: [accountPolicyBindings.id],
    name: "account_policy_reconciliations_binding_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.bindingId, table.effectiveVersion],
    foreignColumns: [accountPolicyVersions.bindingId, accountPolicyVersions.effectiveVersion],
    name: "account_policy_reconciliations_version_fk",
  }).onDelete("restrict"),
  check("account_policy_reconciliations_shape_check", sql`
    ${table.scope} IN ('classification', 'policy', 'funding', 'history')
    AND ${table.status} IN ('pending', 'verified', 'exception')
    AND (${table.effectiveVersion} IS NULL OR ${table.effectiveVersion} > 0)
    AND (
      ${table.legacyAccountClass} IS NULL
      OR ${table.legacyAccountClass} IN ('b2c', 'b2b', 'service')
    )
    AND (
      ${table.legacyMultiplierBp} IS NULL
      OR ${table.legacyMultiplierBp} BETWEEN 0 AND 10000
    )
    AND (
      ${table.observedReservedNano} IS NULL
      OR ${table.observedReservedNano} >= 0
    )
    AND (
      ${table.observedSpentNano} IS NULL
      OR ${table.observedSpentNano} >= 0
    )
    AND jsonb_typeof(${table.details}) = 'object'
    AND (
      (${table.status} = 'pending' AND ${table.completedAt} IS NULL)
      OR (${table.status} <> 'pending' AND ${table.completedAt} IS NOT NULL)
    )
    AND (
      (
        ${table.status} = 'exception'
        AND ${table.exceptionCode} IS NOT NULL
        AND ${table.exceptionCode} <> ''
      )
      OR (${table.status} <> 'exception' AND ${table.exceptionCode} IS NULL)
    )
  `),
]);

// Schema-v2 authority for the one-head zero-downtime pricing/funding release. Stage 5 persists the
// immutable source/policy/assignment plan first; balance funding identities and the engine release
// digest are finalized later from Stage 6 account-local evidence. These declarations intentionally
// have no new runtime consumer in the migration checkpoint; the PostgreSQL expansion must be green
// in production before the producer-first application release uses the two-phase shape.
export const pricingPolicyDocumentsV2 = pgTable("pricing_policy_documents_v2", {
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  ownerType: text("owner_type").notNull(),
  ownerId: text("owner_id").notNull(),
  accountClass: text("account_class").notNull(),
  productId: text("product_id"),
  billingMode: text("billing_mode").notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  capabilityGeneration: bigint("capability_generation", { mode: "bigint" }).notNull(),
  capabilityDigest: text("capability_digest").notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }),
  catalogDigest: text("catalog_digest"),
  switchGeneration: bigint("switch_generation", { mode: "bigint" }),
  switchDigest: text("switch_digest"),
  contentDigest: text("content_digest").notNull(),
  createdAt,
}, (table) => [
  primaryKey({
    columns: [table.policyId, table.policyVersion],
    name: "pricing_policy_documents_v2_pk",
  }),
  unique("pricing_policy_documents_v2_digest_unique")
    .on(table.policyId, table.policyVersion, table.contentDigest),
  check("pricing_policy_documents_v2_identity_check", sql`
    ${table.policyId} <> ''
    AND ${table.policyVersion} > 0
    AND ${table.ownerId} <> ''
    AND ${table.schemaVersion} >= 2
    AND ${table.capabilityGeneration} > 0
    AND ${table.capabilityDigest} <> ''
    AND ${table.contentDigest} <> ''
    AND (
      (${table.ownerType} = 'global_b2c' AND ${table.accountClass} = 'b2c')
      OR (${table.ownerType} = 'b2b_client' AND ${table.accountClass} = 'b2b')
      OR (${table.ownerType} = 'openkeys' AND ${table.accountClass} = 'openkeys')
      OR (${table.ownerType} = 'service' AND ${table.accountClass} = 'service')
    )
    AND (
      (
        ${table.accountClass} = 'service'
        AND ${table.billingMode} = 'meter_only'
        AND ${table.productId} IS NULL
        AND ${table.catalogGeneration} IS NULL
        AND ${table.catalogDigest} IS NULL
        AND ${table.switchGeneration} IS NULL
        AND ${table.switchDigest} IS NULL
      )
      OR (
        ${table.accountClass} <> 'service'
        AND ${table.billingMode} = 'balance'
        AND ${table.productId} IS NOT NULL AND ${table.productId} <> ''
        AND ${table.catalogGeneration} > 0
        AND ${table.catalogDigest} IS NOT NULL AND ${table.catalogDigest} <> ''
        AND ${table.switchGeneration} > 0
        AND ${table.switchDigest} IS NOT NULL AND ${table.switchDigest} <> ''
      )
    )
  `),
]);

export const pricingPolicyRulesV2 = pgTable("pricing_policy_rules_v2", {
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  ruleId: text("rule_id").notNull(),
  ruleDigest: text("rule_digest").notNull(),
  scopeType: text("scope_type").notNull(),
  providerId: text("provider_id"),
  canonicalModelId: text("canonical_model_id"),
  discountBps: bigint("discount_bps", { mode: "bigint" }).notNull(),
  payableMultiplierBp: bigint("payable_multiplier_bp", { mode: "bigint" }).notNull(),
}, (table) => [
  primaryKey({
    columns: [table.policyId, table.policyVersion, table.ruleId],
    name: "pricing_policy_rules_v2_pk",
  }),
  unique("pricing_policy_rules_v2_digest_unique")
    .on(table.policyId, table.policyVersion, table.ruleId, table.ruleDigest),
  foreignKey({
    columns: [table.policyId, table.policyVersion],
    foreignColumns: [pricingPolicyDocumentsV2.policyId, pricingPolicyDocumentsV2.policyVersion],
    name: "pricing_policy_rules_v2_policy_fk",
  }).onDelete("restrict"),
  uniqueIndex("pricing_policy_rules_v2_global_uidx")
    .on(table.policyId, table.policyVersion)
    .where(sql`${table.scopeType} = 'global'`),
  uniqueIndex("pricing_policy_rules_v2_provider_uidx")
    .on(table.policyId, table.policyVersion, table.providerId)
    .where(sql`${table.scopeType} = 'provider'`),
  uniqueIndex("pricing_policy_rules_v2_model_uidx")
    .on(table.policyId, table.policyVersion, table.providerId, table.canonicalModelId)
    .where(sql`${table.scopeType} = 'model'`),
  check("pricing_policy_rules_v2_shape_check", sql`
    ${table.ruleId} <> ''
    AND ${table.ruleDigest} <> ''
    AND ${table.discountBps} BETWEEN 0 AND 10000
    AND ${table.payableMultiplierBp} = 10000 - ${table.discountBps}
    AND (
      (${table.scopeType} = 'global' AND ${table.providerId} IS NULL AND ${table.canonicalModelId} IS NULL)
      OR (
        ${table.scopeType} = 'provider'
        AND ${table.providerId} IS NOT NULL AND ${table.providerId} <> ''
        AND ${table.canonicalModelId} IS NULL
      )
      OR (
        ${table.scopeType} = 'model'
        AND ${table.providerId} IS NOT NULL AND ${table.providerId} <> ''
        AND ${table.canonicalModelId} IS NOT NULL AND ${table.canonicalModelId} <> ''
      )
    )
  `),
]);

export const businessInvitePolicySnapshotsV2 = pgTable("business_invite_policy_snapshots_v2", {
  inviteId: uuid("invite_id").primaryKey(),
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  policyDigest: text("policy_digest").notNull(),
  snapshotDigest: text("snapshot_digest").notNull().unique(),
  createdAt,
}, (table) => [
  foreignKey({
    columns: [table.inviteId],
    foreignColumns: [businessInvites.id],
    name: "business_invite_policy_snapshots_v2_invite_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.policyId, table.policyVersion, table.policyDigest],
    foreignColumns: [
      pricingPolicyDocumentsV2.policyId,
      pricingPolicyDocumentsV2.policyVersion,
      pricingPolicyDocumentsV2.contentDigest,
    ],
    name: "business_invite_policy_snapshots_v2_policy_fk",
  }).onDelete("restrict"),
  check("business_invite_policy_snapshots_v2_digest_check", sql`
    ${table.policyDigest} <> '' AND ${table.snapshotDigest} <> ''
  `),
]);

export const serviceAccountInventoryV2 = pgTable("service_account_inventory_v2", {
  serviceId: text("service_id").primaryKey(),
  engineAccountId: text("engine_account_id").notNull().unique(),
  purpose: text("purpose").notNull(),
  responsible: text("responsible").notNull(),
  status: text("status").notNull(),
  sourceVersion: bigint("source_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  createdAt,
  updatedAt,
}, (table) => [
  check("service_account_inventory_v2_shape_check", sql`
    ${table.serviceId} <> ''
    AND ${table.engineAccountId} <> ''
    AND ${table.purpose} <> ''
    AND ${table.responsible} <> ''
    AND ${table.status} IN ('active', 'disabled')
    AND ${table.sourceVersion} > 0
    AND ${table.contentDigest} <> ''
  `),
]);

/**
 * Immutable Stage 5 source/plan evidence. The JSON artifacts contain the exact
 * validated inventories and plan; searchable blockers and prepare/read ACKs
 * live in child tables below. Target/recovery generations are reserved in
 * Stage 5, while their release digests remain null until Stage 6 funding and
 * engine prepare/readback finish. Updating status never permits source identity
 * to be replaced because the consumer uses the plan digest as its exact CAS.
 */
export const pricingStage5RunsV2 = pgTable("pricing_stage5_runs_v2", {
  runId: uuid("run_id").primaryKey().default(sql`gen_random_uuid()`),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  planDigest: text("plan_digest").notNull().unique(),
  commerceInventoryDigest: text("commerce_inventory_digest").notNull(),
  engineScanFirstDigest: text("engine_scan_first_digest").notNull(),
  engineScanSecondDigest: text("engine_scan_second_digest").notNull(),
  openkeysScanFirstDigest: text("openkeys_scan_first_digest").notNull(),
  openkeysScanSecondDigest: text("openkeys_scan_second_digest").notNull(),
  serviceInventoryDigest: text("service_inventory_digest").notNull(),
  fundingPlanDigest: text("funding_plan_digest").notNull(),
  targetGeneration: bigint("target_generation", { mode: "bigint" }).notNull(),
  targetDigest: text("target_digest"),
  recoveryGeneration: bigint("recovery_generation", { mode: "bigint" }).notNull(),
  recoveryDigest: text("recovery_digest"),
  inventoryArtifact: jsonb("inventory_artifact").$type<Record<string, unknown>>().notNull(),
  planArtifact: jsonb("plan_artifact").$type<Record<string, unknown>>().notNull(),
  blockerCount: bigint("blocker_count", { mode: "bigint" }).notNull(),
  status: text("status").notNull().default("planned"),
  createdAt,
  updatedAt,
}, (table) => [
  unique("pricing_stage5_runs_v2_target_unique")
    .on(table.targetGeneration, table.targetDigest),
  unique("pricing_stage5_runs_v2_recovery_unique")
    .on(table.recoveryGeneration, table.recoveryDigest),
  check("pricing_stage5_runs_v2_shape_check", sql`
    ${table.schemaVersion} = 2
    AND ${table.planDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.commerceInventoryDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.engineScanFirstDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.engineScanSecondDigest} = ${table.engineScanFirstDigest}
    AND ${table.openkeysScanFirstDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.openkeysScanSecondDigest} = ${table.openkeysScanFirstDigest}
    AND ${table.serviceInventoryDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.fundingPlanDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.targetGeneration} > 0
    AND ${table.recoveryGeneration} > ${table.targetGeneration}
    AND (${table.targetDigest} IS NULL OR ${table.targetDigest} ~ '^sha256:v2:[0-9a-f]{64}$')
    AND (${table.recoveryDigest} IS NULL OR ${table.recoveryDigest} ~ '^sha256:v2:[0-9a-f]{64}$')
    AND ((${table.targetDigest} IS NULL) = (${table.recoveryDigest} IS NULL))
    AND jsonb_typeof(${table.inventoryArtifact}) = 'object'
    AND jsonb_typeof(${table.planArtifact}) = 'object'
    AND ${table.blockerCount} >= 0
    AND ${table.status} IN ('blocked', 'planned', 'materializing', 'prepared', 'failed')
    AND (
      (${table.status} = 'blocked' AND ${table.blockerCount} > 0)
      OR (${table.status} <> 'blocked' AND ${table.blockerCount} = 0)
    )
    AND (
      ${table.status} <> 'prepared'
      OR (${table.targetDigest} IS NOT NULL AND ${table.recoveryDigest} IS NOT NULL)
    )
  `),
]);

export const pricingStage5BlockersV2 = pgTable("pricing_stage5_blockers_v2", {
  runId: uuid("run_id").notNull(),
  blockerDigest: text("blocker_digest").notNull(),
  blockerCode: text("blocker_code").notNull(),
  blockerContext: text("blocker_context").notNull(),
  subjectId: text("subject_id").notNull(),
  detail: text("detail").notNull(),
  createdAt,
}, (table) => [
  primaryKey({
    columns: [table.runId, table.blockerDigest],
    name: "pricing_stage5_blockers_v2_pk",
  }),
  foreignKey({
    columns: [table.runId],
    foreignColumns: [pricingStage5RunsV2.runId],
    name: "pricing_stage5_blockers_v2_run_fk",
  }).onDelete("restrict"),
  index("pricing_stage5_blockers_v2_subject_idx")
    .on(table.runId, table.blockerContext, table.subjectId),
  check("pricing_stage5_blockers_v2_shape_check", sql`
    ${table.blockerDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.blockerCode} <> ''
    AND ${table.blockerContext} IN ('commerce', 'engine', 'openkeys', 'service', 'funding', 'release')
    AND ${table.subjectId} <> ''
    AND ${table.detail} <> ''
  `),
]);

export const pricingStage5PrepareAcksV2 = pgTable("pricing_stage5_prepare_acks_v2", {
  runId: uuid("run_id").notNull(),
  artifactKind: text("artifact_kind").notNull(),
  artifactId: text("artifact_id").notNull(),
  artifactVersion: bigint("artifact_version", { mode: "bigint" }).notNull(),
  expectedDigest: text("expected_digest").notNull(),
  mutationResult: text("mutation_result").notNull(),
  readbackDigest: text("readback_digest").notNull(),
  ackDigest: text("ack_digest").notNull().unique(),
  createdAt,
}, (table) => [
  primaryKey({
    columns: [table.runId, table.artifactKind, table.artifactId, table.artifactVersion],
    name: "pricing_stage5_prepare_acks_v2_pk",
  }),
  foreignKey({
    columns: [table.runId],
    foreignColumns: [pricingStage5RunsV2.runId],
    name: "pricing_stage5_prepare_acks_v2_run_fk",
  }).onDelete("restrict"),
  check("pricing_stage5_prepare_acks_v2_shape_check", sql`
    ${table.artifactKind} IN (
      'capability', 'main_catalog', 'openkeys_catalog', 'switches', 'policy',
      'target_release', 'recovery_release', 'recovery_link'
    )
    AND ${table.artifactId} <> ''
    AND ${table.artifactVersion} > 0
    AND ${table.expectedDigest} ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND ${table.mutationResult} IN ('stored', 'unchanged')
    AND ${table.readbackDigest} = ${table.expectedDigest}
    AND ${table.ackDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
  `),
]);

export const pricingReleasePlansV2 = pgTable("pricing_release_plans_v2", {
  generation: bigint("generation", { mode: "bigint" }).primaryKey(),
  releaseKind: text("release_kind").notNull(),
  schemaVersion: bigint("schema_version", { mode: "bigint" }).notNull(),
  commerceInventoryDigest: text("commerce_inventory_digest").notNull(),
  engineInventoryDigest: text("engine_inventory_digest").notNull(),
  openkeysInventoryDigest: text("openkeys_inventory_digest").notNull(),
  serviceInventoryDigest: text("service_inventory_digest").notNull(),
  policyManifestDigest: text("policy_manifest_digest").notNull(),
  assignmentManifestDigest: text("assignment_manifest_digest").notNull(),
  fundingManifestDigest: text("funding_manifest_digest"),
  engineReleaseDigest: text("engine_release_digest"),
  contentDigest: text("content_digest").notNull(),
  status: text("status").notNull().default("planned"),
  createdAt,
  updatedAt,
}, (table) => [
  unique("pricing_release_plans_v2_digest_unique").on(table.generation, table.contentDigest),
  check("pricing_release_plans_v2_identity_check", sql`
    ${table.generation} > 0
    AND ${table.releaseKind} IN ('target', 'recovery')
    AND ${table.schemaVersion} >= 2
    AND ${table.commerceInventoryDigest} <> ''
    AND ${table.engineInventoryDigest} <> ''
    AND ${table.openkeysInventoryDigest} <> ''
    AND ${table.serviceInventoryDigest} <> ''
    AND ${table.policyManifestDigest} <> ''
    AND ${table.assignmentManifestDigest} <> ''
    AND (${table.fundingManifestDigest} IS NULL OR ${table.fundingManifestDigest} <> '')
    AND (${table.engineReleaseDigest} IS NULL OR ${table.engineReleaseDigest} <> '')
    AND ${table.contentDigest} <> ''
    AND ${table.status} IN ('planned', 'materializing', 'prepared', 'active', 'superseded', 'failed')
    AND (
      ${table.status} NOT IN ('prepared', 'active', 'superseded')
      OR (${table.fundingManifestDigest} IS NOT NULL AND ${table.engineReleaseDigest} IS NOT NULL)
    )
  `),
]);

export const pricingReleaseAssignmentsV2 = pgTable("pricing_release_assignments_v2", {
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  engineAccountId: text("engine_account_id").notNull(),
  accountClass: text("account_class").notNull(),
  ownerContext: text("owner_context").notNull(),
  ownerId: text("owner_id").notNull(),
  policyId: text("policy_id").notNull(),
  policyVersion: bigint("policy_version", { mode: "bigint" }).notNull(),
  policyDigest: text("policy_digest").notNull(),
  billingMode: text("billing_mode").notNull(),
  fundingGeneration: bigint("funding_generation", { mode: "bigint" }),
  purpose: text("purpose"),
  responsible: text("responsible"),
  assignmentDigest: text("assignment_digest").notNull(),
}, (table) => [
  primaryKey({
    columns: [table.releaseGeneration, table.engineAccountId],
    name: "pricing_release_assignments_v2_pk",
  }),
  unique("pricing_release_assignments_v2_digest_unique")
    .on(table.releaseGeneration, table.engineAccountId, table.assignmentDigest),
  foreignKey({
    columns: [table.releaseGeneration],
    foreignColumns: [pricingReleasePlansV2.generation],
    name: "pricing_release_assignments_v2_release_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.policyId, table.policyVersion, table.policyDigest],
    foreignColumns: [
      pricingPolicyDocumentsV2.policyId,
      pricingPolicyDocumentsV2.policyVersion,
      pricingPolicyDocumentsV2.contentDigest,
    ],
    name: "pricing_release_assignments_v2_policy_fk",
  }).onDelete("restrict"),
  index("pricing_release_assignments_v2_class_idx")
    .on(table.releaseGeneration, table.accountClass, table.engineAccountId),
  check("pricing_release_assignments_v2_shape_check", sql`
    ${table.engineAccountId} <> ''
    AND ${table.ownerId} <> ''
    AND ${table.policyDigest} <> ''
    AND ${table.assignmentDigest} <> ''
    AND ${table.ownerContext} IN ('commerce', 'openkeys', 'service')
    AND ${table.accountClass} IN ('b2c', 'b2b', 'openkeys', 'service')
    AND (
      (
        ${table.accountClass} = 'service'
        AND ${table.ownerContext} = 'service'
        AND ${table.billingMode} = 'meter_only'
        AND ${table.fundingGeneration} IS NULL
        AND ${table.purpose} IS NOT NULL AND ${table.purpose} <> ''
        AND ${table.responsible} IS NOT NULL AND ${table.responsible} <> ''
      )
      OR (
        ${table.accountClass} = 'openkeys'
        AND ${table.ownerContext} = 'openkeys'
        AND ${table.billingMode} = 'balance'
        AND (${table.fundingGeneration} IS NULL OR ${table.fundingGeneration} > 0)
        AND ${table.purpose} IS NULL
        AND ${table.responsible} IS NULL
      )
      OR (
        ${table.accountClass} IN ('b2c', 'b2b')
        AND ${table.ownerContext} = 'commerce'
        AND ${table.billingMode} = 'balance'
        AND (${table.fundingGeneration} IS NULL OR ${table.fundingGeneration} > 0)
        AND ${table.purpose} IS NULL
        AND ${table.responsible} IS NULL
      )
    )
  `),
]);

export const pricingFundingNormalizationsV2 = pgTable("pricing_funding_normalizations_v2", {
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  engineAccountId: text("engine_account_id").notNull(),
  fundingGeneration: bigint("funding_generation", { mode: "bigint" }),
  expectedSourceDigest: text("expected_source_digest").notNull(),
  targetFundingDigest: text("target_funding_digest"),
  appliedFundingDigest: text("applied_funding_digest"),
  normalizationSource: text("normalization_source"),
  blockers: jsonb("blockers").$type<Array<{ code: string; detail: string }>>(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  createdAt,
  updatedAt,
}, (table) => [
  primaryKey({
    columns: [table.releaseGeneration, table.engineAccountId],
    name: "pricing_funding_normalizations_v2_pk",
  }),
  foreignKey({
    columns: [table.releaseGeneration],
    foreignColumns: [pricingReleasePlansV2.generation],
    name: "pricing_funding_normalizations_v2_release_fk",
  }).onDelete("restrict"),
  index("pricing_funding_normalizations_v2_claim_idx")
    .on(table.status, table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  check("pricing_funding_normalizations_v2_shape_check", sql`
    ${table.engineAccountId} <> ''
    AND (${table.fundingGeneration} IS NULL OR ${table.fundingGeneration} > 0)
    AND ${table.expectedSourceDigest} <> ''
    AND (${table.targetFundingDigest} IS NULL OR ${table.targetFundingDigest} <> '')
    AND (
      ${table.normalizationSource} IS NULL
      OR ${table.normalizationSource} IN (
        'aggregate_paid_only', 'ledger_replay', 'legacy_buckets', 'stored_generation'
      )
    )
    AND (${table.blockers} IS NULL OR jsonb_typeof(${table.blockers}) = 'array')
    AND ${table.attempts} >= 0
    AND ${table.status} IN ('pending', 'processing', 'retry', 'ready', 'blocker')
    AND (
      (
        ${table.status} = 'ready'
        AND ${table.fundingGeneration} IS NOT NULL
        AND ${table.targetFundingDigest} IS NOT NULL
        AND ${table.appliedFundingDigest} = ${table.targetFundingDigest}
        AND ${table.blockers} IS NULL
      )
      OR (${table.status} <> 'ready' AND ${table.appliedFundingDigest} IS NULL)
    )
    AND (
      ${table.status} <> 'blocker'
      OR ${table.blockers} IS NULL
      OR jsonb_array_length(${table.blockers}) > 0
    )
  `),
]);

export const pricingStage8EvidenceV2 = pgTable("pricing_stage8_evidence_v2", {
  evidenceDigest: text("evidence_digest").primaryKey(),
  engineEvidenceDigest: text("engine_evidence_digest"),
  engineCapturedAt: timestamp("engine_captured_at", { withTimezone: true }),
  targetGeneration: bigint("target_generation", { mode: "bigint" }).notNull(),
  targetDigest: text("target_digest").notNull(),
  recoveryGeneration: bigint("recovery_generation", { mode: "bigint" }).notNull(),
  recoveryDigest: text("recovery_digest").notNull(),
  commerceInventoryDigest: text("commerce_inventory_digest").notNull(),
  engineInventoryDigest: text("engine_inventory_digest").notNull(),
  openkeysInventoryDigest: text("openkeys_inventory_digest").notNull(),
  serviceInventoryDigest: text("service_inventory_digest"),
  salesContractDigest: text("sales_contract_digest").notNull(),
  fundingDigest: text("funding_digest").notNull(),
  shadowDigest: text("shadow_digest").notNull(),
  runtimeFloorDigest: text("runtime_floor_digest").notNull(),
  legacyInflightCount: bigint("legacy_inflight_count", { mode: "bigint" }).notNull(),
  blockerCount: bigint("blocker_count", { mode: "bigint" }).notNull(),
  passed: boolean("passed").notNull(),
  observedAt: timestamp("observed_at", { withTimezone: true }).notNull(),
  validUntil: timestamp("valid_until", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  foreignKey({
    columns: [table.targetGeneration, table.targetDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_stage8_evidence_v2_target_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.recoveryGeneration, table.recoveryDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_stage8_evidence_v2_recovery_fk",
  }).onDelete("restrict"),
  check("pricing_stage8_evidence_v2_shape_check", sql`
    ${table.evidenceDigest} <> ''
    AND ${table.commerceInventoryDigest} <> ''
    AND ${table.engineInventoryDigest} <> ''
    AND ${table.openkeysInventoryDigest} <> ''
    AND ${table.salesContractDigest} <> ''
    AND ${table.fundingDigest} <> ''
    AND ${table.shadowDigest} <> ''
    AND ${table.runtimeFloorDigest} <> ''
    AND ${table.legacyInflightCount} >= 0
    AND ${table.blockerCount} >= 0
    AND ${table.validUntil} > ${table.observedAt}
    AND ((${table.passed} AND ${table.blockerCount} = 0) OR NOT ${table.passed})
  `),
]);

export const pricingStage8CaptureJobsV2 = pgTable("pricing_stage8_capture_jobs_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  idempotencyKey: text("idempotency_key").notNull().unique(),
  requestDigest: text("request_digest").notNull(),
  targetGeneration: bigint("target_generation", { mode: "bigint" }).notNull(),
  recoveryGeneration: bigint("recovery_generation", { mode: "bigint" }).notNull(),
  windowStartAt: timestamp("window_start_at", { withTimezone: true }).notNull(),
  windowEndAt: timestamp("window_end_at", { withTimezone: true }).notNull(),
  minSamplesPerProvider: bigint("min_samples_per_provider", { mode: "bigint" }).notNull(),
  financialSampleSize: integer("financial_sample_size").notNull(),
  geminiClientAdmissions: bigint("gemini_client_admissions", { mode: "bigint" }).notNull(),
  operatorId: text("operator_id").notNull(),
  reason: text("reason").notNull(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  resultEngineEvidenceDigest: text("result_engine_evidence_digest"),
  resultCombinedEvidenceDigest: text("result_combined_evidence_digest"),
  resultPassed: boolean("result_passed"),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  index("pricing_stage8_capture_jobs_v2_claim_idx")
    .on(table.status, table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  check("pricing_stage8_capture_jobs_v2_shape_check", sql`
    ${table.idempotencyKey} <> ''
    AND ${table.requestDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.targetGeneration} > 0
    AND ${table.recoveryGeneration} > ${table.targetGeneration}
    AND ${table.windowEndAt} > ${table.windowStartAt}
    AND ${table.minSamplesPerProvider} BETWEEN 1 AND 1000000
    AND ${table.financialSampleSize} BETWEEN 1 AND 1000
    AND ${table.geminiClientAdmissions} >= 0
    AND ${table.operatorId} <> ''
    AND ${table.reason} <> ''
    AND ${table.attempts} >= 0
    AND ${table.status} IN ('pending', 'processing', 'retry', 'passed', 'blocked', 'dead')
    AND (
      (
        ${table.resultEngineEvidenceDigest} IS NULL
        AND ${table.resultCombinedEvidenceDigest} IS NULL
        AND ${table.resultPassed} IS NULL
      )
      OR (
        ${table.resultEngineEvidenceDigest} IS NOT NULL
        AND ${table.resultEngineEvidenceDigest} <> ''
        AND ${table.resultCombinedEvidenceDigest} IS NOT NULL
        AND ${table.resultCombinedEvidenceDigest} <> ''
        AND ${table.resultPassed} IS NOT NULL
      )
    )
    AND (
      (
        ${table.status} IN ('pending', 'processing', 'retry')
        AND ${table.completedAt} IS NULL
        AND ${table.resultEngineEvidenceDigest} IS NULL
      )
      OR (
        ${table.status} = 'passed'
        AND ${table.completedAt} IS NOT NULL
        AND ${table.resultPassed} = true
      )
      OR (
        ${table.status} = 'blocked'
        AND ${table.completedAt} IS NOT NULL
        AND ${table.resultPassed} = false
      )
      OR (
        ${table.status} = 'dead'
        AND ${table.completedAt} IS NOT NULL
        AND ${table.lastError} IS NOT NULL
      )
    )
  `),
]);

export const pricingStage8CaptureArtifactsV2 = pgTable("pricing_stage8_capture_artifacts_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  jobId: uuid("job_id").notNull(),
  attempt: integer("attempt").notNull(),
  engineEvidenceDigest: text("engine_evidence_digest").notNull(),
  engineCapturedAt: timestamp("engine_captured_at", { withTimezone: true }).notNull(),
  enginePayloadJson: text("engine_payload_json").notNull(),
  combinedEvidenceDigest: text("combined_evidence_digest"),
  combinedPayloadJson: text("combined_payload_json"),
  combinedPassed: boolean("combined_passed"),
  combinedWriteResult: text("combined_write_result"),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
}, (table) => [
  foreignKey({
    columns: [table.jobId],
    foreignColumns: [pricingStage8CaptureJobsV2.id],
    name: "pricing_stage8_capture_artifacts_v2_job_fk",
  }).onDelete("restrict"),
  unique("pricing_stage8_capture_artifacts_v2_job_attempt_unique")
    .on(table.jobId, table.attempt),
  index("pricing_stage8_capture_artifacts_v2_job_idx")
    .on(table.jobId, table.createdAt),
  check("pricing_stage8_capture_artifacts_v2_shape_check", sql`
    ${table.attempt} > 0
    AND ${table.engineEvidenceDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.enginePayloadJson} <> ''
    AND (
      (
        ${table.combinedEvidenceDigest} IS NULL
        AND ${table.combinedPayloadJson} IS NULL
        AND ${table.combinedPassed} IS NULL
        AND ${table.combinedWriteResult} IS NULL
        AND ${table.completedAt} IS NULL
      )
      OR (
        ${table.combinedEvidenceDigest} IS NOT NULL
        AND ${table.combinedEvidenceDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
        AND ${table.combinedPayloadJson} IS NOT NULL
        AND ${table.combinedPayloadJson} <> ''
        AND ${table.combinedPassed} IS NOT NULL
        AND ${table.combinedWriteResult} IS NOT NULL
        AND ${table.combinedWriteResult} IN ('stored', 'unchanged', 'not_persisted')
        AND ${table.completedAt} IS NOT NULL
      )
    )
  `),
]);

/**
 * Dormant parent identity for one full-inventory pre-cutover shadow alignment.
 * The later protected producer binds the exact prepared target/recovery pair to
 * one reviewed catalog/switch lineage and creates child delivery jobs. Merely
 * creating these tables cannot move a pricing head or contact the engine.
 */
export const pricingShadowRolloutsV2 = pgTable("pricing_shadow_rollouts_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  idempotencyKey: uuid("idempotency_key").notNull().unique(),
  stage5RunId: uuid("stage5_run_id").notNull(),
  targetGeneration: bigint("target_generation", { mode: "bigint" }).notNull(),
  targetDigest: text("target_digest").notNull(),
  recoveryGeneration: bigint("recovery_generation", { mode: "bigint" }).notNull(),
  recoveryDigest: text("recovery_digest").notNull(),
  catalogGeneration: bigint("catalog_generation", { mode: "bigint" }).notNull(),
  mainCatalogDigest: text("main_catalog_digest").notNull(),
  openkeysCatalogDigest: text("openkeys_catalog_digest").notNull(),
  switchGeneration: bigint("switch_generation", { mode: "bigint" }).notNull(),
  switchDigest: text("switch_digest").notNull(),
  engineInventoryDigest: text("engine_inventory_digest").notNull(),
  assignmentManifestDigest: text("assignment_manifest_digest").notNull(),
  policyManifestDigest: text("policy_manifest_digest").notNull(),
  rolloutDigest: text("rollout_digest").notNull().unique(),
  assignmentCount: bigint("assignment_count", { mode: "bigint" }).notNull(),
  jobCount: bigint("job_count", { mode: "bigint" }).notNull(),
  actorId: text("actor_id").notNull(),
  reason: text("reason").notNull(),
  status: text("status").notNull().default("pending"),
  lastError: text("last_error"),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.stage5RunId],
    foreignColumns: [pricingStage5RunsV2.runId],
    name: "pricing_shadow_rollouts_v2_stage5_run_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.targetGeneration, table.targetDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_shadow_rollouts_v2_target_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.recoveryGeneration, table.recoveryDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_shadow_rollouts_v2_recovery_fk",
  }).onDelete("restrict"),
  index("pricing_shadow_rollouts_v2_status_idx").on(table.status, table.createdAt),
  check("pricing_shadow_rollouts_v2_shape_check", sql`
    ${table.targetGeneration} > 0
    AND ${table.recoveryGeneration} > ${table.targetGeneration}
    AND ${table.targetDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.recoveryDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.catalogGeneration} > 0
    AND ${table.mainCatalogDigest} ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND ${table.openkeysCatalogDigest} ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND ${table.switchGeneration} > 0
    AND ${table.switchDigest} ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND ${table.engineInventoryDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.assignmentManifestDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.policyManifestDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.rolloutDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.assignmentCount} > 0
    AND ${table.jobCount} >= 0
    AND ${table.jobCount} <= ${table.assignmentCount}
    AND ${table.actorId} <> ''
    AND ${table.reason} <> ''
    AND ${table.status} IN ('pending', 'processing', 'confirmed', 'blocked', 'dead')
    AND (
      (${table.status} IN ('pending', 'processing') AND ${table.completedAt} IS NULL)
      OR (${table.status} = 'confirmed' AND ${table.completedAt} IS NOT NULL AND ${table.lastError} IS NULL)
      OR (${table.status} IN ('blocked', 'dead') AND ${table.completedAt} IS NOT NULL AND ${table.lastError} IS NOT NULL)
    )
  `),
]);

/**
 * Exact engine mutation requests for every account that is not already aligned
 * with the parent rollout. Payloads include the policy, shadow binding and CAS
 * expectation so retries never rebuild a request from moving engine state.
 */
export const pricingShadowPolicyJobsV2 = pgTable("pricing_shadow_policy_jobs_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  rolloutId: uuid("rollout_id").notNull(),
  engineAccountId: text("engine_account_id").notNull(),
  accountStatus: text("account_status").notNull(),
  accountClass: text("account_class").notNull(),
  ownerContext: text("owner_context").notNull(),
  releasePolicyId: text("release_policy_id").notNull(),
  releasePolicyVersion: bigint("release_policy_version", { mode: "bigint" }).notNull(),
  releasePolicyDigest: text("release_policy_digest").notNull(),
  effectiveVersion: bigint("effective_version", { mode: "bigint" }).notNull(),
  contentDigest: text("content_digest").notNull(),
  expectedActiveVersion: bigint("expected_active_version", { mode: "bigint" }),
  expectedActiveDigest: text("expected_active_digest"),
  requestDigest: text("request_digest").notNull(),
  requestPayload: jsonb("request_payload").$type<Record<string, unknown>>().notNull(),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  ackDigest: text("ack_digest"),
  ackPayload: jsonb("ack_payload").$type<Record<string, unknown>>(),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  completedAt: timestamp("completed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.rolloutId],
    foreignColumns: [pricingShadowRolloutsV2.id],
    name: "pricing_shadow_policy_jobs_v2_rollout_fk",
  }).onDelete("restrict"),
  unique("pricing_shadow_policy_jobs_v2_account_unique")
    .on(table.rolloutId, table.engineAccountId),
  unique("pricing_shadow_policy_jobs_v2_request_unique")
    .on(table.rolloutId, table.requestDigest),
  index("pricing_shadow_policy_jobs_v2_claim_idx")
    .on(table.status, table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  check("pricing_shadow_policy_jobs_v2_shape_check", sql`
    ${table.engineAccountId} <> ''
    AND ${table.accountStatus} IN ('active', 'disabled')
    AND ${table.accountClass} IN ('b2c', 'b2b', 'openkeys', 'service')
    AND ${table.ownerContext} IN ('commerce', 'openkeys', 'service')
    AND ${table.releasePolicyId} <> ''
    AND ${table.releasePolicyVersion} > 0
    AND ${table.releasePolicyDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ${table.effectiveVersion} > 0
    AND ${table.contentDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND ((${table.expectedActiveVersion} IS NULL) = (${table.expectedActiveDigest} IS NULL))
    AND (${table.expectedActiveVersion} IS NULL OR ${table.expectedActiveVersion} > 0)
    AND (${table.expectedActiveDigest} IS NULL OR ${table.expectedActiveDigest} ~ '^sha256:v[12]:[0-9a-f]{64}$')
    AND ${table.requestDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
    AND jsonb_typeof(${table.requestPayload}) = 'object'
    AND ${table.attempts} >= 0
    AND ${table.status} IN ('pending', 'processing', 'retry', 'confirmed', 'blocked', 'dead')
    AND (
      (${table.status} = 'processing' AND ${table.lockedAt} IS NOT NULL AND ${table.lockedBy} IS NOT NULL)
      OR (${table.status} <> 'processing' AND ${table.lockedAt} IS NULL AND ${table.lockedBy} IS NULL)
    )
    AND (
      (${table.status} = 'confirmed'
        AND ${table.ackDigest} ~ '^sha256:v2:[0-9a-f]{64}$'
        AND ${table.ackPayload} IS NOT NULL
        AND jsonb_typeof(${table.ackPayload}) = 'object'
        AND ${table.confirmedAt} IS NOT NULL
        AND ${table.completedAt} IS NOT NULL
        AND ${table.lastError} IS NULL)
      OR (${table.status} IN ('blocked', 'dead')
        AND ${table.ackDigest} IS NULL
        AND ${table.ackPayload} IS NULL
        AND ${table.confirmedAt} IS NULL
        AND ${table.completedAt} IS NOT NULL
        AND ${table.lastError} IS NOT NULL)
      OR (${table.status} IN ('pending', 'processing', 'retry')
        AND ${table.ackDigest} IS NULL
        AND ${table.ackPayload} IS NULL
        AND ${table.confirmedAt} IS NULL
        AND ${table.completedAt} IS NULL)
    )
  `),
]);

export const pricingReleaseControlJobsV2 = pgTable("pricing_release_control_jobs_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  jobKind: text("job_kind").notNull(),
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  releaseDigest: text("release_digest").notNull(),
  idempotencyKey: text("idempotency_key").notNull().unique(),
  payloadDigest: text("payload_digest").notNull(),
  expectedHeadVersion: bigint("expected_head_version", { mode: "bigint" }),
  stage8EvidenceDigest: text("stage8_evidence_digest"),
  activationPayload: jsonb("activation_payload"),
  status: text("status").notNull().default("pending"),
  attempts: integer("attempts").notNull().default(0),
  nextAttemptAt: timestamp("next_attempt_at", { withTimezone: true }).notNull().defaultNow(),
  lockedAt: timestamp("locked_at", { withTimezone: true }),
  lockedBy: text("locked_by"),
  lastError: text("last_error"),
  resultDigest: text("result_digest"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  foreignKey({
    columns: [table.releaseGeneration, table.releaseDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_release_control_jobs_v2_release_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.stage8EvidenceDigest],
    foreignColumns: [pricingStage8EvidenceV2.evidenceDigest],
    name: "pricing_release_control_jobs_v2_evidence_fk",
  }).onDelete("restrict"),
  index("pricing_release_control_jobs_v2_claim_idx")
    .on(table.status, table.nextAttemptAt, table.createdAt)
    .where(sql`${table.status} IN ('pending', 'retry')`),
  check("pricing_release_control_jobs_v2_shape_check", sql`
    ${table.releaseDigest} <> ''
    AND ${table.idempotencyKey} <> ''
    AND ${table.payloadDigest} <> ''
    AND ${table.attempts} >= 0
    AND (${table.expectedHeadVersion} IS NULL OR ${table.expectedHeadVersion} >= 0)
    AND ${table.jobKind} IN (
      'materialize_release',
      'normalize_funding',
      'collect_stage8',
      'activate_release',
      'activate_recovery',
      'activate_successor'
    )
    AND ${table.status} IN ('pending', 'processing', 'retry', 'confirmed', 'dead')
    AND (
      ${table.jobKind} NOT IN ('activate_release', 'activate_recovery', 'activate_successor')
      OR (${table.expectedHeadVersion} IS NOT NULL AND ${table.stage8EvidenceDigest} IS NOT NULL)
    )
    AND (
      (${table.status} = 'confirmed' AND ${table.resultDigest} IS NOT NULL AND ${table.confirmedAt} IS NOT NULL)
      OR (${table.status} <> 'confirmed' AND ${table.confirmedAt} IS NULL)
    )
  `),
]);

export const pricingReleaseActivationReceiptsV2 = pgTable("pricing_release_activation_receipts_v2", {
  activationId: text("activation_id").primaryKey(),
  activationKind: text("activation_kind").notNull(),
  releaseGeneration: bigint("release_generation", { mode: "bigint" }).notNull(),
  releaseDigest: text("release_digest").notNull(),
  evidenceDigest: text("evidence_digest").notNull(),
  headVersion: bigint("head_version", { mode: "bigint" }).notNull(),
  receiptDigest: text("receipt_digest").notNull().unique(),
  receiptPayload: jsonb("receipt_payload"),
  activatedAt: timestamp("activated_at", { withTimezone: true }).notNull(),
  createdAt,
}, (table) => [
  unique("pricing_release_activation_receipts_v2_head_unique").on(table.headVersion),
  foreignKey({
    columns: [table.releaseGeneration, table.releaseDigest],
    foreignColumns: [pricingReleasePlansV2.generation, pricingReleasePlansV2.contentDigest],
    name: "pricing_release_activation_receipts_v2_release_fk",
  }).onDelete("restrict"),
  foreignKey({
    columns: [table.evidenceDigest],
    foreignColumns: [pricingStage8EvidenceV2.evidenceDigest],
    name: "pricing_release_activation_receipts_v2_evidence_fk",
  }).onDelete("restrict"),
  check("pricing_release_activation_receipts_v2_shape_check", sql`
    ${table.activationId} <> ''
    AND ${table.activationKind} IN ('cutover', 'recovery', 'successor')
    AND ${table.headVersion} > 0
    AND ${table.receiptDigest} <> ''
  `),
]);

export const pricingReleaseOrchestrationsV2 = pgTable("pricing_release_orchestrations_v2", {
  id: uuid("id").primaryKey().default(sql`gen_random_uuid()`),
  idempotencyKey: text("idempotency_key").notNull().unique(),
  capabilityGeneration: bigint("capability_generation", { mode: "bigint" }).notNull(),
  step: text("step").notNull(),
  status: text("status").notNull().default("active"),
  cycle: integer("cycle").notNull().default(1),
  stage5RunId: uuid("stage5_run_id"),
  targetGeneration: bigint("target_generation", { mode: "bigint" }),
  recoveryGeneration: bigint("recovery_generation", { mode: "bigint" }),
  evidenceDigest: text("evidence_digest"),
  activationKind: text("activation_kind"),
  operatorId: text("operator_id").notNull(),
  reason: text("reason").notNull(),
  lastError: text("last_error"),
  resultDigest: text("result_digest"),
  confirmedAt: timestamp("confirmed_at", { withTimezone: true }),
  createdAt,
  updatedAt,
}, (table) => [
  uniqueIndex("pricing_release_orchestrations_v2_active_uq")
    .on(sql`(1)`)
    .where(sql`${table.status} = 'active'`),
  check("pricing_release_orchestrations_v2_shape_check", sql`
    ${table.capabilityGeneration} > 0
    AND ${table.step} IN (
      'materialize_pair',
      'deliver_catalogs',
      'normalize_funding',
      'rollout',
      'capture',
      'activate',
      'verify'
    )
    AND ${table.status} IN ('active', 'confirmed', 'dead')
    AND ${table.cycle} BETWEEN 1 AND 3
    AND (${table.activationKind} IS NULL OR ${table.activationKind} IN ('cutover', 'recovery', 'successor'))
    AND ${table.operatorId} <> ''
    AND ${table.reason} <> ''
    AND (
      (${table.status} = 'confirmed' AND ${table.resultDigest} IS NOT NULL
        AND ${table.confirmedAt} IS NOT NULL AND ${table.step} = 'verify'
        AND ${table.targetGeneration} IS NOT NULL)
      OR (${table.status} <> 'confirmed' AND ${table.confirmedAt} IS NULL)
    )
  `),
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
