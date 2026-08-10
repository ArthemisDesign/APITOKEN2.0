import { randomUUID } from "node:crypto";
import {
  paymentProviderSchema,
  type EngineLedgerEntry,
} from "@claude-api/contracts";
import type { PoolClient } from "pg";
import type { Database } from "./client.js";
import { applyProviderDiscountTx, enqueuePricingJob } from "./pricing-discounts.js";

export class InvalidBusinessInvitationError extends Error {}
export class BusinessInvitationNotFoundError extends Error {}
export class BusinessInvitationConflictError extends Error {}
export class BusinessCustomerNotFoundError extends Error {}
export class CustomerProfileNotFoundError extends Error {}
export interface PricingSyncTarget {
  userId: string;
  engineAccountId: string;
}

export interface ClaimedPricingJob {
  id: string;
  userId: string;
  engineAccountId: string;
  /** `null` targets the account default; a provider id targets that provider's override. */
  providerId: string | null;
  /** `null` is only produced by a provider job and means "remove the override". */
  multiplierBp: number | null;
  attempts: number;
}

export function utcMonthStart(value = new Date()): Date {
  return new Date(Date.UTC(value.getUTCFullYear(), value.getUTCMonth(), 1));
}

// РЕАЛЬНЫМИ (комиссионируемыми) считаем ТОЛЬКО депозиты через платёжные провайдеры:
// engine ref = `${provider}:${providerPaymentId}` (см. payments.ts). Это whitelist, а не blacklist:
// welcome-бонус (`signup-bonus:`), промо (`promo:`), админ-кредит (`admin-credit:`), пустой или
// неизвестный ref — «бесплатное», комиссия по нему НЕ идёт. Так любой новый нереальный источник
// денег по умолчанию бесплатный, и мы никогда случайно не выплатим комиссию с подарка.
const REAL_MONEY_REF_PREFIXES: readonly string[] = paymentProviderSchema.options.map((p) => `${p}:`);

export function isFreeCreditRef(ref: string | null | undefined): boolean {
  if (typeof ref !== "string") return true;
  return !REAL_MONEY_REF_PREFIXES.some((prefix) => ref.startsWith(prefix));
}

/** Подарочные источники денег: welcome-бонус, промо и восстановление бонуса. */
const BONUS_REF_PREFIXES: readonly string[] = ["signup-bonus:", "promo:", "bonus-restore", "welcome"];

export type PricingTopupSource = "payment" | "bonus" | "manual";

/**
 * Классификация пополнения ДЛЯ ОТЧЁТНОСТИ (не для комиссии — та живёт в isFreeCreditRef и
 * остаётся whitelist-строгой). `payment` — депозит через платёжного провайдера, `bonus` —
 * известный подарок, `manual` — всё прочее: админ-кредит и ручные зачисления, то есть реальные
 * деньги, полученные мимо платёжной системы. Неизвестный ref безопаснее считать ручным
 * пополнением (он попадает в отчёт как «оплачено вручную» и виден оператору), а не подарком.
 */
export function classifyTopupRef(ref: string | null | undefined): PricingTopupSource {
  if (typeof ref !== "string" || ref.trim() === "") return "manual";
  if (REAL_MONEY_REF_PREFIXES.some((prefix) => ref.startsWith(prefix))) return "payment";
  if (BONUS_REF_PREFIXES.some((prefix) => ref.startsWith(prefix))) return "bonus";
  return "manual";
}

/** Raised when the engine's own ledger evidence contradicts what was already recorded. */
export class PricingLedgerEvidenceError extends Error {}

const PRICING_LEDGER_PAGE_SIZE = 1000;
const POSTGRES_INTEGER_MAX = 2_147_483_647n;
const SUPPORTED_LEDGER_ATTRIBUTION_SCHEMA_VERSIONS: ReadonlySet<bigint> = new Set([1n, 2n]);
const PROVIDER_BACKFILL_WINDOW_DAYS = 30;
const UNATTRIBUTED_PROVIDER_ID = "unattributed";
const UNAVAILABLE_PROVIDER_ID = "unavailable";
const PROVIDER_RECOVERY_VERSION = 2;

function normalizedProviderId(value: string | null | undefined): string | null {
  if (
    typeof value !== "string"
    || value.length === 0
    || value.length > 200
    || value.trim() !== value
    || /[\u0000-\u001f\u007f]/.test(value)
  ) return null;
  return value;
}

function isProviderRecoverySentinel(value: string | null): boolean {
  return value === UNATTRIBUTED_PROVIDER_ID || value === UNAVAILABLE_PROVIDER_ID;
}

/**
 * The provider of a charge is the engine's own top-level ledger column. The retired attribution
 * record carried a second copy of it; there is no second copy to disagree with any more.
 */
function ledgerProviderEvidence(entry: EngineLedgerEntry): string {
  const providerId = normalizedProviderId(entry.provider);
  return providerId === null || isProviderRecoverySentinel(providerId)
    ? UNATTRIBUTED_PROVIDER_ID
    : providerId;
}

function epochSecondsDate(value: number | string, label: string): Date {
  const seconds = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(seconds) || !Number.isSafeInteger(seconds) || seconds <= 0) {
    throw new Error(`invalid ${label}`);
  }
  return new Date(seconds * 1000);
}

export interface BusinessInviteRecord {
  id: string;
  email: string | null;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  idempotentReplay: boolean;
  deliveryStatus: string;
}

export async function createBusinessInvite(database: Database, input: {
  email?: string;
  tokenHash: string;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  idempotencyKey: string;
  actorId: string;
  reason: string;
}): Promise<BusinessInviteRecord> {
  const client = await database.pool.connect();
  const email = input.email?.toLowerCase() ?? null;
  try {
    await client.query("BEGIN");
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [input.idempotencyKey]);
    const existing = await client.query<{
      id: string; email: string | null; encrypted_token: string | null;
      multiplier_bp: number; expires_at: Date; delivery_status: string | null;
    }>(`
      SELECT bi.id, bi.email, bi.encrypted_token, bi.multiplier_bp, bi.expires_at,
             eo.status AS delivery_status
      FROM business_invites bi
      LEFT JOIN LATERAL (
        SELECT status::text AS status FROM email_outbox
        WHERE business_invite_id = bi.id ORDER BY created_at DESC LIMIT 1
      ) eo ON TRUE
      WHERE bi.idempotency_key = $1
      FOR UPDATE OF bi
    `, [input.idempotencyKey]);
    const prior = existing.rows[0];
    if (prior) {
      if (prior.email !== email || prior.multiplier_bp !== input.multiplierBp) {
        throw new BusinessInvitationConflictError("idempotency key was already used for another invitation");
      }
      if (!prior.encrypted_token) {
        throw new BusinessInvitationConflictError("the invitation token is no longer available");
      }
      await client.query("COMMIT");
      return {
        id: prior.id,
        email: prior.email,
        encryptedToken: prior.encrypted_token,
        multiplierBp: prior.multiplier_bp,
        expiresAt: prior.expires_at,
        idempotentReplay: true,
        deliveryStatus: prior.delivery_status ?? "copy_only",
      };
    }

    const id = randomUUID();
    await client.query(`
      INSERT INTO business_invites (
        id, email, token_hash, encrypted_token, multiplier_bp, expires_at,
        idempotency_key, created_by_actor
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    `, [
      id, email, input.tokenHash, input.encryptedToken, input.multiplierBp,
      input.expiresAt, input.idempotencyKey, input.actorId,
    ]);
    if (email) {
      await queueBusinessInviteEmail(client, {
        inviteId: id,
        recipient: email,
        encryptedToken: input.encryptedToken,
        multiplierBp: input.multiplierBp,
        expiresAt: input.expiresAt,
      });
    }
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.created', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, id, JSON.stringify({
      email,
      multiplierBp: input.multiplierBp,
      expiresAt: input.expiresAt.toISOString(),
      delivery: email ? "email" : "copy_only",
      reason: input.reason,
    })]);
    await client.query("COMMIT");
    return {
      id,
      email,
      encryptedToken: input.encryptedToken,
      multiplierBp: input.multiplierBp,
      expiresAt: input.expiresAt,
      idempotentReplay: false,
      deliveryStatus: email ? "pending" : "copy_only",
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function lockBusinessInvite(
  client: PoolClient,
  input: { email: string; tokenHash: string },
): Promise<{ id: string; multiplierBp: number }> {
  const result = await client.query<{ id: string; multiplier_bp: number }>(`
    SELECT id, multiplier_bp
    FROM business_invites
    WHERE token_hash = $1 AND (email IS NULL OR lower(email) = lower($2))
      AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
    FOR UPDATE
  `, [input.tokenHash, input.email]);
  const invite = result.rows[0];
  if (!invite) throw new InvalidBusinessInvitationError("invalid, expired, or email-mismatched business invitation");
  return { id: invite.id, multiplierBp: invite.multiplier_bp };
}

export async function getBusinessInvitePreview(
  database: Database,
  tokenHash: string,
): Promise<{
  email: string | null;
  multiplierBp: number;
  expiresAt: Date;
} | null> {
  const result = await database.pool.query<{
    id: string; email: string | null; multiplier_bp: number; expires_at: Date;
  }>(`
    SELECT id::text, email, multiplier_bp, expires_at
    FROM business_invites
    WHERE token_hash = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
  `, [tokenHash]);
  const row = result.rows[0];
  if (!row) return null;
  return {
    email: row.email,
    multiplierBp: row.multiplier_bp,
    expiresAt: row.expires_at,
  };
}

export async function getBusinessInviteToken(
  database: Database,
  inviteId: string,
): Promise<{ encryptedToken: string; email: string | null; expiresAt: Date }> {
  const result = await database.pool.query<{
    encrypted_token: string | null; email: string | null; expires_at: Date;
  }>(`
    SELECT encrypted_token, email, expires_at
    FROM business_invites
    WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at > now()
  `, [inviteId]);
  const row = result.rows[0];
  if (!row?.encrypted_token) throw new BusinessInvitationNotFoundError("active invitation not found");
  return { encryptedToken: row.encrypted_token, email: row.email, expiresAt: row.expires_at };
}

export async function revokeBusinessInvite(database: Database, input: {
  inviteId: string;
  actorId: string;
  reason: string;
}): Promise<void> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query(`
      UPDATE business_invites
      SET revoked_at = now(), revoked_by_actor = $2, encrypted_token = NULL
      WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
      RETURNING id
    `, [input.inviteId, input.actorId]);
    if (!result.rows[0]) throw new BusinessInvitationNotFoundError("active invitation not found");
    await client.query(`
      UPDATE email_outbox
      SET status = 'canceled', locked_at = NULL, locked_by = NULL,
          last_error = 'business invitation revoked', updated_at = now()
      WHERE business_invite_id = $1 AND status IN ('pending', 'processing')
    `, [input.inviteId]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.revoked', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, input.inviteId, JSON.stringify({ reason: input.reason })]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function rotateBusinessInvite(database: Database, input: {
  inviteId: string;
  tokenHash: string;
  encryptedToken: string;
  expiresAt: Date;
  idempotencyKey: string;
  actorId: string;
  reason: string;
}): Promise<BusinessInviteRecord> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query("SELECT pg_advisory_xact_lock(hashtext($1))", [input.idempotencyKey]);
    const replayResult = await client.query<{
      id: string; email: string | null; encrypted_token: string | null;
      multiplier_bp: number; expires_at: Date; delivery_status: string | null;
    }>(`
      SELECT replacement.id, replacement.email, replacement.encrypted_token,
             replacement.multiplier_bp, replacement.expires_at,
             eo.status::text AS delivery_status
      FROM business_invites original
      JOIN business_invites replacement ON replacement.id = original.superseded_by_invite_id
      LEFT JOIN LATERAL (
        SELECT status FROM email_outbox
        WHERE business_invite_id = replacement.id ORDER BY created_at DESC LIMIT 1
      ) eo ON TRUE
      WHERE original.id = $1 AND replacement.idempotency_key = $2
      FOR UPDATE OF replacement
    `, [input.inviteId, input.idempotencyKey]);
    const replay = replayResult.rows[0];
    if (replay) {
      if (!replay.encrypted_token) {
        throw new BusinessInvitationConflictError("the replacement invitation token is no longer available");
      }
      await client.query("COMMIT");
      return {
        id: replay.id,
        email: replay.email,
        encryptedToken: replay.encrypted_token,
        multiplierBp: replay.multiplier_bp,
        expiresAt: replay.expires_at,
        idempotentReplay: true,
        deliveryStatus: replay.delivery_status ?? "pending",
      };
    }
    const keyInUse = await client.query(
      "SELECT 1 FROM business_invites WHERE idempotency_key = $1",
      [input.idempotencyKey],
    );
    if (keyInUse.rows[0]) {
      throw new BusinessInvitationConflictError("idempotency key was already used for another invitation");
    }
    const oldResult = await client.query<{
      email: string | null; multiplier_bp: number;
    }>(`
      SELECT invitation.email, invitation.multiplier_bp
      FROM business_invites invitation
      WHERE invitation.id = $1 AND invitation.consumed_at IS NULL AND invitation.revoked_at IS NULL
      FOR UPDATE OF invitation
    `, [input.inviteId]);
    const old = oldResult.rows[0];
    if (!old) throw new BusinessInvitationNotFoundError("active invitation not found");
    if (!old.email) throw new BusinessInvitationConflictError("copy-only invitations cannot be emailed; copy the existing link");
    const id = randomUUID();
    const replacementMultiplierBp = old.multiplier_bp;
    await client.query(`
      INSERT INTO business_invites (
        id, email, token_hash, encrypted_token, multiplier_bp, expires_at,
        idempotency_key, created_by_actor
      ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    `, [
      id, old.email, input.tokenHash, input.encryptedToken, replacementMultiplierBp,
      input.expiresAt, input.idempotencyKey, input.actorId,
    ]);
    await client.query(`
      UPDATE business_invites
      SET revoked_at = now(), revoked_by_actor = $2, encrypted_token = NULL,
          superseded_by_invite_id = $3
      WHERE id = $1
    `, [input.inviteId, input.actorId, id]);
    await client.query(`
      UPDATE email_outbox
      SET status = 'canceled', locked_at = NULL, locked_by = NULL,
          last_error = 'superseded by a new business invitation', updated_at = now()
      WHERE business_invite_id = $1 AND status IN ('pending', 'processing')
    `, [input.inviteId]);
    await queueBusinessInviteEmail(client, {
      inviteId: id,
      recipient: old.email,
      encryptedToken: input.encryptedToken,
      multiplierBp: replacementMultiplierBp,
      expiresAt: input.expiresAt,
      policyBased: false,
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'business_invite.resent', 'business_invite', $2, $3::jsonb)
    `, [input.actorId, id, JSON.stringify({
      supersedesInviteId: input.inviteId,
      reason: input.reason,
    })]);
    await client.query("COMMIT");
    return {
      id,
      email: old.email,
      encryptedToken: input.encryptedToken,
      multiplierBp: replacementMultiplierBp,
      expiresAt: input.expiresAt,
      idempotentReplay: false,
      deliveryStatus: "pending",
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

async function queueBusinessInviteEmail(client: PoolClient, input: {
  inviteId: string;
  recipient: string;
  encryptedToken: string;
  multiplierBp: number;
  expiresAt: Date;
  policyBased?: boolean;
}): Promise<void> {
  const pricingPayload = input.policyBased
    ? { pricingPolicy: "provider_model" }
    : { discountPercent: 100 - input.multiplierBp / 100 };
  await client.query(`
    INSERT INTO email_outbox (id, business_invite_id, recipient, template, payload)
    VALUES ($1, $2, $3, 'business_invite', $4::jsonb)
  `, [randomUUID(), input.inviteId, input.recipient, JSON.stringify({
    encryptedToken: input.encryptedToken,
    ...pricingPayload,
    expiresAt: input.expiresAt.toISOString(),
  })]);
}

export async function getPricingView(database: Database, userId: string): Promise<Record<string, unknown> | null> {
  const result = await database.pool.query<{
    customer_type: "b2c" | "b2b";
    multiplier_bp: number;
  }>(`
    SELECT cp.customer_type, cp.multiplier_bp
    FROM customer_profiles cp
    WHERE cp.user_id = $1
  `, [userId]);
  const row = result.rows[0];
  if (!row) return null;
  if (row.customer_type === "b2b") {
    return {
      customerType: "b2b",
      pricingMode: "manual",
      discountPercent: 100 - row.multiplier_bp / 100,
      multiplierBp: row.multiplier_bp,
    };
  }
  // Post-cutover B2C is the flat global policy: 50% off every catalog model, with any
  // provider/model overrides resolved by the release authority. No tiers, thresholds or
  // retention windows exist to display anymore.
  return {
    customerType: "b2c",
    pricingMode: "flat",
    discountPercent: 50,
    multiplierBp: 5_000,
  };
}

/**
 * One negotiated B2B change: the account default and any per-provider overrides, committed
 * together. An admin edit is a single set of terms, so it must land as a single fact — writing the
 * default in one transaction and each provider in its own left a window where the customer was
 * priced by half of the new deal, and a failure partway made that window permanent.
 *
 * Delivery to the engine is still the durable job queue: commerce records what was agreed, the
 * engine is the authority that prices a request, and the queue is what makes them converge.
 */
export async function setBusinessPricingBundle(database: Database, input: {
  userId: string;
  multiplierBp?: number;
  providers?: Record<string, number | null>;
  actorId: string;
  reason: string;
}): Promise<{ engineAccountId: string; jobIds: string[] }> {
  const providers = Object.entries(input.providers ?? {});
  if (input.multiplierBp === undefined && providers.length === 0) {
    throw new Error("business pricing mutation is empty");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{ engine_account_id: string }>(`
      SELECT ea.engine_account_id
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1 AND cp.customer_type = 'b2b'
        AND ea.engine_account_id IS NOT NULL
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    if (!row) throw new BusinessCustomerNotFoundError("business customer not found");
    const jobIds: string[] = [];
    if (input.multiplierBp !== undefined) {
      jobIds.push(await applyBusinessDefaultTx(client, {
        userId: input.userId,
        engineAccountId: row.engine_account_id,
        multiplierBp: input.multiplierBp,
        actorId: input.actorId,
        reason: input.reason,
      }));
    }
    for (const [providerId, multiplierBp] of providers) {
      jobIds.push(await applyProviderDiscountTx(client, {
        userId: input.userId,
        engineAccountId: row.engine_account_id,
        providerId,
        multiplierBp,
        actorId: input.actorId,
        reason: input.reason,
      }));
    }
    await client.query("COMMIT");
    return { engineAccountId: row.engine_account_id, jobIds };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** The account default applied inside a caller's transaction. See setBusinessPricingBundle. */
async function applyBusinessDefaultTx(client: PoolClient, input: {
  userId: string;
  engineAccountId: string;
  multiplierBp: number;
  actorId: string;
  reason: string;
}): Promise<string> {
  await client.query(`
    UPDATE customer_profiles SET multiplier_bp = $2, updated_at = now() WHERE user_id = $1;
  `, [input.userId, input.multiplierBp]);
  await client.query(`
    UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
  `, [input.userId, input.multiplierBp]);
  const jobId = await enqueuePricingJob(client, {
    userId: input.userId,
    engineAccountId: input.engineAccountId,
    multiplierBp: input.multiplierBp,
    reason: "b2b_manual",
  });
  await client.query(`
    INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
    VALUES ('admin', $1, 'pricing.b2b_changed', 'user', $2, $3::jsonb)
  `, [input.actorId, input.userId, JSON.stringify({
    multiplierBp: input.multiplierBp,
    reason: input.reason,
    jobId,
  })]);
  return jobId;
}

export async function setBusinessPricing(database: Database, input: {
  userId: string;
  multiplierBp: number;
  actorId: string;
  reason: string;
}): Promise<{ engineAccountId: string; jobId: string }> {
  const { engineAccountId, jobIds } = await setBusinessPricingBundle(database, {
    userId: input.userId,
    multiplierBp: input.multiplierBp,
    actorId: input.actorId,
    reason: input.reason,
  });
  return { engineAccountId, jobId: jobIds[0]! };
}

/**
 * Converts an existing B2C customer to a supplied negotiated B2B multiplier atomically. B2C
 * progress remains historical data, while the live tier/window/floor controls are cleared so no
 * later B2C reconciliation can change the negotiated rate. The conversion also provisions the
 * managed b2b_client policy (single Anthropic discount rule mirroring the multiplier), its
 * account binding, and a staged engine delivery job — the same end state invitation redemption
 * reaches; without it the admin policy editor has nothing to manage. Re-running the conversion
 * on an already-B2B customer repairs a missing policy (customers converted before this
 * provisioning existed) and is otherwise an unchanged no-op.
 */
export async function convertCustomerToBusiness(database: Database, input: {
  userId: string;
  actorId: string;
  reason: string;
  multiplierBp: number;
}): Promise<{ converted: boolean; multiplierBp: number; engineAccountId: string; jobId: string | null }> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      customer_type: "b2c" | "b2b";
      current_tier: number | null;
      multiplier_bp: number;
      referral_floor_bps: number;
      engine_account_record_id: string;
      engine_account_id: string | null;
    }>(`
      SELECT cp.customer_type, cp.current_tier, cp.multiplier_bp, cp.referral_floor_bps,
             ea.id::text AS engine_account_record_id, ea.engine_account_id
      FROM customer_profiles cp
      JOIN engine_accounts ea ON ea.user_id = cp.user_id
      WHERE cp.user_id = $1
      FOR UPDATE OF cp, ea
    `, [input.userId]);
    const row = result.rows[0];
    if (!row) throw new CustomerProfileNotFoundError("customer profile or engine account not found");
    if (!row.engine_account_id) throw new BusinessCustomerNotFoundError("customer engine account is not provisioned");
    if (row.customer_type === "b2b") {
      // Already B2B: the negotiated default is changed through setBusinessPricing and per-provider
      // terms through setCustomerProviderDiscount. There is no policy document to repair.
      await client.query("ROLLBACK");
      return {
        converted: false,
        multiplierBp: row.multiplier_bp,
        engineAccountId: row.engine_account_id,
        jobId: null,
      };
    }

    await client.query(`
      UPDATE customer_profiles
      SET customer_type = 'b2b', current_tier = NULL,
          tier_window_start = NULL, tier_window_spent_nano = 0,
          referral_floor_bps = 0, multiplier_bp = $2, updated_at = now()
      WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    await client.query(`
      UPDATE engine_accounts SET mult_bp = $2, updated_at = now() WHERE user_id = $1
    `, [input.userId, input.multiplierBp]);
    const jobId = await enqueuePricingJob(client, {
      userId: input.userId,
      engineAccountId: row.engine_account_id,
      multiplierBp: input.multiplierBp,
      reason: "b2b_conversion",
    });
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'pricing.b2b_converted', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({
      reason: input.reason,
      previousMultiplierBp: row.multiplier_bp,
      negotiatedMultiplierBp: input.multiplierBp,
      previousTier: row.current_tier,
      previousReferralFloorBps: row.referral_floor_bps,
    })]);
    await client.query("COMMIT");
    return {
      converted: true,
      multiplierBp: input.multiplierBp,
      engineAccountId: row.engine_account_id,
      jobId,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Устанавливает «пол» скидки от сейлза для реферала ПАРТНЁРА. Клиент ОСТАЁТСЯ b2c и идёт по
 * обычным тир-правилам (тир, бонус, промо — как у всех); floor лишь гарантирует, что цена не хуже
 * скидки сейлза: эффективный mult = min(тир-mult, 10000 - floorBps). floorBps=0 снимает пол
 * (возврат к чистому тиру). Разграничение b2c/b2b сохранено: для business-b2b пол не применяется.
 * Мультипликатор доставляется в движок через durable engine_pricing_jobs. Идемпотентно.
 * Вызывается ТОЛЬКО для партнёрских реф-кодов (обычная сайтовая рефка сюда не идёт).
 */
export async function setReferralFloor(database: Database, input: {
  userId: string;
  floorBps: number; // 0..9500 (скидка ≤ 95%); 0 = снять пол
  actorId: string;
  // Явная АБСОЛЮТНАЯ установка пола (партнёр/админ меняет процент действующему рефералу):
  // обходит монотонный GREATEST и позволяет ПОНИЗИТЬ floor. Автоматические пути (signup,
  // sales-фид, промо) обязаны оставлять override=false — иначе replay затрёт лучшую скидку.
  override?: boolean;
}): Promise<{ applied: boolean; multiplierBp: number | null }> {
  if (!Number.isInteger(input.floorBps) || input.floorBps < 0 || input.floorBps > 9500) {
    throw new RangeError("referral floor must be an integer between 0 and 9500 bps (≤95%)");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      customer_type: "b2c" | "b2b";
      referral_floor_bps: number;
    }>(`
      SELECT cp.customer_type, cp.referral_floor_bps
      FROM customer_profiles cp
      WHERE cp.user_id = $1
      FOR UPDATE OF cp
    `, [input.userId]);
    const row = result.rows[0];
    // Пол применяется только к обычным b2c-аккаунтам; business-b2b не трогаем (своя прайс-логика).
    if (!row || row.customer_type !== "b2c") {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp: null };
    }
    // Пост-катовер: «пол» — это партнёрская АТРИБУЦИЯ обещанной ставки, а не цена. Цена B2C
    // задаётся только активным pricing release (flat 50% + overrides), поэтому scalar/jobs
    // здесь больше не двигаются. Колонка одна, но пишут в неё три независимых источника
    // (промо, партнёрская ссылка, sales-фид) — берём максимум (лучшее клиенту), кроме явного
    // сброса/override. floorBps===0 — единственный путь ЯВНОГО сброса (напр. отзыв).
    const effectiveFloor = input.override === true
      ? input.floorBps
      : input.floorBps === 0 ? 0 : Math.max(row.referral_floor_bps, input.floorBps);
    if (row.referral_floor_bps === effectiveFloor) {
      await client.query("ROLLBACK");
      return { applied: false, multiplierBp: null };
    }
    await client.query(`
      UPDATE customer_profiles
      SET referral_floor_bps = $2, updated_at = now()
      WHERE user_id = $1
    `, [input.userId, effectiveFloor]);
    await client.query(`
      INSERT INTO audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', $1, 'pricing.referral_floor', 'user', $2, $3::jsonb)
    `, [input.actorId, input.userId, JSON.stringify({ requestedFloorBps: input.floorBps, effectiveFloorBps: effectiveFloor, override: input.override === true })]);
    await client.query("COMMIT");
    return { applied: true, multiplierBp: null };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function listPricingSyncTargets(database: Database): Promise<PricingSyncTarget[]> {
  // И b2c, и b2b: расход обязан попадать в immutable pricing_usage_events для обеих сегментов.
  // Прогрессивные эффекты (free-first, месяцы, тир-окна) применяются только к b2c внутри
  // applyPricingLedgerPage и в b2c-фильтрованных функциях тир-модели.
  const result = await database.pool.query<{ user_id: string; engine_account_id: string }>(`
    SELECT cp.user_id, ea.engine_account_id
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    JOIN users u ON u.id = cp.user_id
    WHERE cp.customer_type IN ('b2c', 'b2b') AND ea.status = 'active'
      AND ea.engine_account_id IS NOT NULL AND u.status = 'active'
    ORDER BY cp.user_id
  `);
  return result.rows.map((row) => ({ userId: row.user_id, engineAccountId: row.engine_account_id }));
}

export async function getPricingUsageCursor(
  database: Database,
  target: PricingSyncTarget,
): Promise<bigint> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`
      DELETE FROM pricing_usage_cursors WHERE user_id = $1 AND engine_account_id <> $2
    `, [target.userId, target.engineAccountId]);
    // Invalidate the completion marker before network I/O. Only a terminal short page restores it;
    // a thrown/failed sync therefore cannot authorize window closure with a previous cycle's marker.
    const result = await client.query<{ last_ledger_id: string }>(`
      INSERT INTO pricing_usage_cursors (engine_account_id, user_id, updated_at)
      VALUES ($1, $2, '-infinity')
      ON CONFLICT (engine_account_id) DO UPDATE SET updated_at = '-infinity'
      RETURNING last_ledger_id
    `, [target.engineAccountId, target.userId]);
    // Reconcile durable credit accrual markers on every pricing poll. This catches a missed
    // post-credit call and reverses markers whose payment has since been refunded/disputed.
    await reconcileTopupTier(client, target);
    await client.query("COMMIT");
    return BigInt(result.rows[0]?.last_ledger_id ?? "0");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * Marks an empty engine-ledger page as a completed scan. The cursor is invalidated before network
 * I/O by getPricingUsageCursor, so callers must invoke this only after the engine returns a genuine
 * terminal empty page.
 */
export async function completePricingUsageSync(
  database: Database,
  target: PricingSyncTarget,
): Promise<void> {
  await database.pool.query(`
    UPDATE pricing_usage_cursors
    SET updated_at = now()
    WHERE engine_account_id = $1 AND user_id = $2
  `, [target.engineAccountId, target.userId]);
}

/**
 * Returns the ledger cursor immediately before the oldest recent usage row whose provider has not
 * completed the current evidence algorithm and cannot already be proven by immutable pricing
 * attribution. NULL and both historical sentinels are eligible only below the current version, so
 * a stronger producer can retry old terminal rows once without creating an idle polling loop. The
 * engine retains charge detail for the same 30-day horizon used by the paying-users control room.
 */
export async function getPricingProviderBackfillCursor(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<bigint | null> {
  if (throughLedgerId < 0n) throw new RangeError("provider backfill cursor must not be negative");
  const result = await database.pool.query<{ first_ledger_id: string | null }>(`
    SELECT min(event.ledger_entry_id)::text AS first_ledger_id
    FROM pricing_usage_events event
    WHERE event.user_id = $1 AND event.engine_account_id = $2
      AND event.ledger_entry_id <= $3
      AND event.occurred_at >= now() - make_interval(days => $4)
      AND event.provider_recovery_version < $5
      AND (event.provider_id IS NULL OR event.provider_id IN ($6, $7))
  `, [
    target.userId,
    target.engineAccountId,
    throughLedgerId.toString(),
    PROVIDER_BACKFILL_WINDOW_DAYS,
    PROVIDER_RECOVERY_VERSION,
    UNATTRIBUTED_PROVIDER_ID,
    UNAVAILABLE_PROVIDER_ID,
  ]);
  const firstLedgerId = result.rows[0]?.first_ledger_id;
  if (firstLedgerId === null || firstLedgerId === undefined) return null;
  const first = BigInt(firstLedgerId);
  return first > 0n ? first - 1n : 0n;
}

/**
 * Copies provider evidence from a retained engine ledger page onto matching immutable commerce
 * events. Amount and any existing attribution are locked and compared before the nullable field is
 * filled; conflicting evidence aborts the page instead of silently relabelling spend.
 */
export async function applyPricingProviderBackfillPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
): Promise<number> {
  const evidence = new Map<string, { amountNano: string; providerId: string }>();
  for (const entry of entries) {
    const amount = BigInt(entry.amount_nano);
    if (entry.kind !== "charge" || amount <= 0n) continue;
    const ledgerId = BigInt(entry.id).toString();
    const candidate = {
      amountNano: amount.toString(),
      providerId: ledgerProviderEvidence(entry),
    };
    const previous = evidence.get(ledgerId);
    if (
      previous !== undefined
      && (previous.amountNano !== candidate.amountNano || previous.providerId !== candidate.providerId)
    ) {
      throw new PricingLedgerEvidenceError(
        `engine provider backfill repeated ledger ${ledgerId} with conflicting evidence`,
      );
    }
    evidence.set(ledgerId, candidate);
  }
  if (evidence.size === 0) return 0;

  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const ledgerIds = [...evidence.keys()];
    const existing = await client.query<{
      ledger_entry_id: string;
      amount_nano: string;
      provider_id: string | null;
      provider_recovery_version: number;
    }>(`
      SELECT event.ledger_entry_id::text, event.amount_nano::text,
             event.provider_id, event.provider_recovery_version
      FROM pricing_usage_events event
      WHERE event.user_id = $1 AND event.engine_account_id = $2
        AND event.ledger_entry_id = ANY($3::bigint[])
      FOR UPDATE OF event
    `, [target.userId, target.engineAccountId, ledgerIds]);

    const updateLedgerIds: string[] = [];
    const updateProviderIds: string[] = [];
    for (const row of existing.rows) {
      const candidate = evidence.get(row.ledger_entry_id)!;
      if (row.amount_nano !== candidate.amountNano) {
        throw new PricingLedgerEvidenceError(
          `engine provider backfill amount differs for ledger ${row.ledger_entry_id}`,
        );
      }
      const exactProviders = [row.provider_id]
        .map(normalizedProviderId)
        .filter((value): value is string => value !== null && !isProviderRecoverySentinel(value));
      if (
        candidate.providerId !== UNATTRIBUTED_PROVIDER_ID
        && exactProviders.some((providerId) => providerId !== candidate.providerId)
      ) {
        throw new PricingLedgerEvidenceError(
          `engine provider backfill identity differs for ledger ${row.ledger_entry_id}`,
        );
      }
      if (
        candidate.providerId === UNATTRIBUTED_PROVIDER_ID
        && exactProviders.length > 0
      ) continue;
      const currentProviderId = normalizedProviderId(row.provider_id);
      const needsCurrentRecovery = row.provider_recovery_version < PROVIDER_RECOVERY_VERSION
        && (currentProviderId === null || isProviderRecoverySentinel(currentProviderId));
      if (needsCurrentRecovery && candidate.providerId !== UNATTRIBUTED_PROVIDER_ID) {
        updateLedgerIds.push(row.ledger_entry_id);
        updateProviderIds.push(candidate.providerId);
      }
    }

    let updated = 0;
    if (updateLedgerIds.length > 0) {
      const result = await client.query(`
        UPDATE pricing_usage_events event
        SET provider_id = evidence.provider_id,
            provider_recovery_version = $5
        FROM unnest($3::bigint[], $4::text[]) AS evidence(ledger_entry_id, provider_id)
        WHERE event.user_id = $1 AND event.engine_account_id = $2
          AND event.ledger_entry_id = evidence.ledger_entry_id
      `, [
        target.userId,
        target.engineAccountId,
        updateLedgerIds,
        updateProviderIds,
        PROVIDER_RECOVERY_VERSION,
      ]);
      updated = result.rowCount ?? 0;
    }
    await client.query("COMMIT");
    return updated;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/** Terminalizes one attempted recovery range whose retained ledger evidence remains unavailable. */
export async function completePricingProviderBackfill(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<number> {
  if (throughLedgerId < 0n) throw new RangeError("provider backfill cursor must not be negative");
  const result = await database.pool.query(`
    UPDATE pricing_usage_events event
    SET provider_id = $4, provider_recovery_version = $5
    WHERE event.user_id = $1 AND event.engine_account_id = $2
      AND event.ledger_entry_id <= $3
      AND event.occurred_at >= now() - make_interval(days => $6)
      AND event.provider_recovery_version < $5
      AND (event.provider_id IS NULL OR event.provider_id IN ($7, $8))
  `, [
    target.userId,
    target.engineAccountId,
    throughLedgerId.toString(),
    UNAVAILABLE_PROVIDER_ID,
    PROVIDER_RECOVERY_VERSION,
    PROVIDER_BACKFILL_WINDOW_DAYS,
    UNATTRIBUTED_PROVIDER_ID,
    UNAVAILABLE_PROVIDER_ID,
  ]);
  return result.rowCount ?? 0;
}

/**
 * Пишет одно движковое пополнение в иммутабельную отчётную таблицу. Идемпотентна по
 * (engine_account_id, ledger_entry_id): повторная подача той же страницы ничего не двоит.
 */
async function recordPricingTopup(
  client: PoolClient,
  target: PricingSyncTarget,
  entry: EngineLedgerEntry,
  amount: bigint,
): Promise<void> {
  await client.query(`
    INSERT INTO pricing_usage_topups (
      id, user_id, engine_account_id, ledger_entry_id, ref, source, amount_nano, occurred_at
    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
  `, [
    randomUUID(),
    target.userId,
    target.engineAccountId,
    BigInt(entry.id).toString(),
    entry.ref,
    classifyTopupRef(entry.ref),
    amount.toString(),
    epochSecondsDate(entry.ts, "topup timestamp"),
  ]);
}

/**
 * Курсор догоняющего скана пополнений. Обычный курсор расхода уже стоит выше исторических
 * топапов, поэтому у отчётной таблицы свой маркер: пока он ниже основного курсора, воркер
 * ограниченными страницами перечитывает леджер с начала и заполняет историю ровно один раз.
 * NULL — история уже покрыта, скан не нужен.
 */
export async function getPricingTopupBackfillCursor(
  database: Database,
  target: PricingSyncTarget,
  throughLedgerId: bigint,
): Promise<bigint | null> {
  if (throughLedgerId < 0n) throw new RangeError("topup backfill cursor must not be negative");
  const result = await database.pool.query<{ scanned: string }>(`
    SELECT topups_scanned_through_ledger_id::text AS scanned
    FROM pricing_usage_cursors
    WHERE engine_account_id = $1 AND user_id = $2
  `, [target.engineAccountId, target.userId]);
  const scanned = result.rows[0]?.scanned;
  if (scanned === undefined) return null;
  const from = BigInt(scanned);
  return from >= throughLedgerId ? null : from;
}

/** Заполняет отчётные пополнения по одной странице леджера и двигает маркер скана. */
export async function applyPricingTopupBackfillPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
  scannedThroughLedgerId: bigint,
): Promise<number> {
  if (scannedThroughLedgerId < 0n) throw new RangeError("topup backfill cursor must not be negative");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    let inserted = 0;
    for (const entry of entries) {
      const amount = BigInt(entry.amount_nano);
      if (entry.kind !== "topup" || amount <= 0n) continue;
      const before = await client.query(`
        SELECT 1 FROM pricing_usage_topups
        WHERE engine_account_id = $1 AND ledger_entry_id = $2
      `, [target.engineAccountId, BigInt(entry.id).toString()]);
      if (before.rowCount) continue;
      await recordPricingTopup(client, target, entry, amount);
      inserted += 1;
    }
    await client.query(`
      UPDATE pricing_usage_cursors
      SET topups_scanned_through_ledger_id = GREATEST(topups_scanned_through_ledger_id, $3)
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, scannedThroughLedgerId.toString()]);
    await client.query("COMMIT");
    return inserted;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function applyPricingLedgerPage(
  database: Database,
  target: PricingSyncTarget,
  entries: readonly EngineLedgerEntry[],
): Promise<void> {
  if (entries.length === 0) return;
  // Legacy free-first требует ХРОНОЛОГИЧЕСКОГО порядка: топап должен фондировать последующие charge.
  // Леджер-id движка монотонны по времени создания, поэтому сортируем страницу по id по возрастанию —
  // иначе pre-attribution charge, пришедший раньше своего фондирующего топапа, завысил бы legacy
  // real_funded. Для policy rows деньги берутся из immutable funding evidence. Порядок также делает
  // продвижение курсора детерминированным.
  const ordered = [...entries].sort((a, b) => {
    const da = BigInt(a.id);
    const db = BigInt(b.id);
    return da < db ? -1 : da > db ? 1 : 0;
  });
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // B2B-профиль тоже лочится и синкается: его расход обязан попадать в immutable
    // pricing_usage_events — иначе конвертация клиента в B2B навсегда замораживает его курсор
    // и админка недосчитывает реальные списания. The lock also serializes cursor advancement.
    const profileResult = await client.query<{
      customer_type: "b2c" | "b2b";
    }>(`
      SELECT customer_type
      FROM customer_profiles
      WHERE user_id = $1 FOR UPDATE
    `, [target.userId]);
    const profile = profileResult.rows[0];
    if (!profile) {
      await client.query("ROLLBACK");
      return;
    }
    // Курсор на старте: применяем эффекты (commission basis) ТОЛЬКО к записям выше него —
    // это делает применение страницы идемпотентным к повторной подаче тех же записей.
    // customer_profiles залочена → обработка юзера сериализована.
    const cursorRow = await client.query<{ last_ledger_id: string }>(
      "SELECT last_ledger_id::text AS last_ledger_id FROM pricing_usage_cursors WHERE engine_account_id = $1 AND user_id = $2",
      [target.engineAccountId, target.userId],
    );
    const startCursor = BigInt(cursorRow.rows[0]?.last_ledger_id ?? "0");
    const freeRow = await client.query<{ free_balance_nano: string }>(
      "SELECT free_balance_nano::text AS free_balance_nano FROM customer_profiles WHERE user_id = $1 FOR UPDATE",
      [target.userId],
    );
    let freeBalance = BigInt(freeRow.rows[0]?.free_balance_nano ?? "0");
    let freeBalanceChanged = false;
    let lastLedgerId = 0n;
    for (const entry of ordered) {
      const ledgerId = BigInt(entry.id);
      if (ledgerId > lastLedgerId) lastLedgerId = ledgerId;
      if (ledgerId <= startCursor) continue; // уже обработано ранее — не двоим эффекты
      const amount = BigInt(entry.amount_nano);
      // Иммутабельная копия пополнения для отчётности: движковые топапы (админ-кредит, ручные)
      // не создают строки в payments, поэтому иначе реальные деньги клиента невидимы админке.
      // Деньгами и балансом эта таблица не управляет; вставка идемпотентна по (аккаунт, ledger id).
      if (entry.kind === "topup" && amount > 0n) {
        await recordPricingTopup(client, target, entry, amount);
        // Free credit (welcome bonus, promo, admin credit) tops up the free balance instead of
        // the commissionable one. The whitelist of real-money refs is the single place that
        // decides which is which — see isFreeCreditRef.
        if (isFreeCreditRef(entry.ref)) {
          freeBalance += amount;
          freeBalanceChanged = true;
        }
        continue;
      }
      // Only a positive charge creates real_funded/commission. A negative `adjust` (refund,
      // chargeback, admin clawback) deliberately does not reverse commission already accrued:
      // ignoring it can only underpay, never overpay.
      if (entry.kind !== "charge" || amount <= 0n) continue;
      const occurredAt = epochSecondsDate(entry.ts, "event timestamp");
      // Free money is spent first, so real_funded is the part covered by the customer's own
      // money. This is the whole funding model: one durable free balance per customer, no
      // per-request funding evidence, nothing to disagree with the engine about.
      const fromFree = amount < freeBalance ? amount : freeBalance;
      const realFunded = amount - fromFree;
      const eventId = randomUUID();
      const providerId = ledgerProviderEvidence(entry);
      const inserted = await client.query<{ id: string }>(`
        INSERT INTO pricing_usage_events (
          id, user_id, engine_account_id, ledger_entry_id, provider_id,
          amount_nano, real_funded_nano, occurred_at, provider_recovery_version
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (engine_account_id, ledger_entry_id) DO NOTHING
        RETURNING id
      `, [
        eventId,
        target.userId,
        target.engineAccountId,
        ledgerId.toString(),
        providerId,
        entry.amount_nano,
        realFunded.toString(),
        occurredAt,
        providerId === UNATTRIBUTED_PROVIDER_ID ? 0 : PROVIDER_RECOVERY_VERSION,
      ]);
      if (!inserted.rows[0]) continue; // already processed — never spend the free balance twice
      if (fromFree > 0n) {
        freeBalance -= fromFree;
        freeBalanceChanged = true;
      }
    }
    if (freeBalanceChanged) {
      await client.query(
        "UPDATE customer_profiles SET free_balance_nano = $2, updated_at = now() WHERE user_id = $1",
        [target.userId, freeBalance.toString()],
      );
    }
    const reachedStablePageEnd = entries.length < PRICING_LEDGER_PAGE_SIZE;
    await client.query(`
      UPDATE pricing_usage_cursors
      SET last_ledger_id = GREATEST(last_ledger_id, $3),
          updated_at = CASE WHEN $4 THEN now() ELSE updated_at END
      WHERE engine_account_id = $1 AND user_id = $2
    `, [target.engineAccountId, target.userId, lastLedgerId.toString(), reachedStablePageEnd]);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function claimNextPricingJob(
  database: Database,
  workerId: string,
): Promise<ClaimedPricingJob | null> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    // Lease recovery is part of normal claiming, not a startup-only maintenance step. A failed
    // retryPricingJob write therefore delays a job by at most one lease interval instead of
    // stranding it in processing until process restart.
    await client.query(`
      UPDATE engine_pricing_jobs
      SET status = 'retry', locked_at = NULL, locked_by = NULL, next_attempt_at = now(),
          last_error = COALESCE(last_error, 'recovered expired pricing lease'), updated_at = now()
      WHERE status = 'processing'
        AND (locked_at IS NULL OR locked_at < now() - interval '5 minutes')
    `);
    const result = await client.query<{
      id: string; user_id: string; engine_account_id: string;
      provider_id: string | null; multiplier_bp: number | null; attempts: number;
    }>(`
      SELECT id, user_id, engine_account_id, provider_id, multiplier_bp, attempts
      FROM engine_pricing_jobs
      WHERE status IN ('pending', 'retry') AND next_attempt_at <= now()
      ORDER BY next_attempt_at, created_at
      FOR UPDATE SKIP LOCKED LIMIT 1
    `);
    const row = result.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }
    await client.query(`
      UPDATE engine_pricing_jobs SET status = 'processing', locked_at = now(), locked_by = $2,
        attempts = attempts + 1, updated_at = now() WHERE id = $1
    `, [row.id, workerId]);
    await client.query("COMMIT");
    return {
      id: row.id,
      userId: row.user_id,
      engineAccountId: row.engine_account_id,
      providerId: row.provider_id,
      multiplierBp: row.multiplier_bp,
      attempts: row.attempts + 1,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

/**
 * The value commerce currently wants delivered for this job's target: the customer's default
 * multiplier for an account job, the stored override (or its absence) for a provider job. A job
 * that delivered a now-stale value is requeued instead of confirmed, so an edit made during an
 * in-flight delivery is never lost.
 */
async function desiredPricingJobValue(
  database: Database,
  job: ClaimedPricingJob,
): Promise<{ engineAccountId: string | null; multiplierBp: number | null }> {
  const result = await database.pool.query<{
    engine_account_id: string | null; multiplier_bp: number | null;
  }>(
    job.providerId === null
      ? `SELECT ea.engine_account_id, cp.multiplier_bp
           FROM customer_profiles cp
           LEFT JOIN engine_accounts ea ON ea.user_id = cp.user_id
          WHERE cp.user_id = $1`
      : `SELECT ea.engine_account_id, d.multiplier_bp
           FROM engine_accounts ea
           LEFT JOIN customer_provider_discounts d
             ON d.user_id = ea.user_id AND d.provider_id = $2
          WHERE ea.user_id = $1`,
    job.providerId === null ? [job.userId] : [job.userId, job.providerId],
  );
  const row = result.rows[0];
  return {
    engineAccountId: row?.engine_account_id ?? null,
    multiplierBp: row?.multiplier_bp ?? null,
  };
}

function pricingJobIsCurrent(
  job: ClaimedPricingJob,
  desired: { engineAccountId: string | null; multiplierBp: number | null },
): boolean {
  return desired.multiplierBp === job.multiplierBp
    && (desired.engineAccountId ?? job.engineAccountId) === job.engineAccountId;
}

export async function confirmPricingJob(database: Database, job: ClaimedPricingJob): Promise<void> {
  const desired = await desiredPricingJobValue(database, job);
  if (pricingJobIsCurrent(job, desired)) {
    await database.pool.query(`
      UPDATE engine_pricing_jobs
      SET status = 'confirmed', confirmed_at = now(),
          locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
      WHERE id = $1 AND status = 'processing'
    `, [job.id]);
    return;
  }
  await database.pool.query(`
    UPDATE engine_pricing_jobs
    SET engine_account_id = COALESCE($2, engine_account_id), multiplier_bp = $3,
        reason = 'superseded_after_processing', status = 'pending', attempts = 0,
        next_attempt_at = now(), confirmed_at = NULL,
        locked_at = NULL, locked_by = NULL, last_error = NULL, updated_at = now()
    WHERE id = $1 AND status = 'processing'
  `, [job.id, desired.engineAccountId, desired.multiplierBp]);
}

export async function retryPricingJob(database: Database, job: ClaimedPricingJob, error: string): Promise<void> {
  const desired = await desiredPricingJobValue(database, job);
  const delaySeconds = Math.min(3600, Math.max(5, 2 ** Math.min(job.attempts, 10)));
  if (pricingJobIsCurrent(job, desired)) {
    await database.pool.query(`
      UPDATE engine_pricing_jobs
      SET status = 'retry', next_attempt_at = now() + ($3 * interval '1 second'),
          locked_at = NULL, locked_by = NULL, last_error = $2, updated_at = now()
      WHERE id = $1 AND status = 'processing'
    `, [job.id, error.slice(0, 2000), delaySeconds]);
    return;
  }
  // The desired value moved while this attempt was in flight: deliver the new one immediately
  // rather than backing off on a value nobody wants any more.
  await database.pool.query(`
    UPDATE engine_pricing_jobs
    SET engine_account_id = COALESCE($2, engine_account_id), multiplier_bp = $3,
        reason = 'superseded_after_processing', status = 'retry', attempts = 0,
        next_attempt_at = now(), locked_at = NULL, locked_by = NULL,
        last_error = NULL, updated_at = now()
    WHERE id = $1 AND status = 'processing'
  `, [job.id, desired.engineAccountId, desired.multiplierBp]);
}

export async function recoverStalePricingJobs(database: Database): Promise<number> {
  const result = await database.pool.query(`
    UPDATE engine_pricing_jobs SET status = 'retry', locked_at = NULL, locked_by = NULL,
      next_attempt_at = now(), last_error = 'recovered stale worker lease', updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - interval '5 minutes'
  `);
  return result.rowCount ?? 0;
}

async function reconcileTopupTier(
  client: PoolClient,
  target: { engineAccountId: string; userId?: string },
): Promise<void> {
  const profileResult = await client.query<{
    user_id: string; cumulative_topup_nano: string;
  }>(`
    SELECT cp.user_id, cp.cumulative_topup_nano
    FROM customer_profiles cp
    JOIN engine_accounts ea ON ea.user_id = cp.user_id
    WHERE ea.engine_account_id = $1 AND cp.customer_type = 'b2c'
      AND ($2::uuid IS NULL OR cp.user_id = $2::uuid)
    FOR UPDATE OF cp
  `, [target.engineAccountId, target.userId ?? null]);
  const profile = profileResult.rows[0];
  if (!profile) return;

  // The unique credit marker and the aggregate update stay in this transaction, so confirmed
  // top-ups and later refund/dispute reversals are applied exactly once across worker retries.
  // Post-cutover this maintains only the cumulative top-up total used by finance reporting;
  // no tier, window or multiplier state is advanced anymore.
  const appliedResult = await client.query<{ amount_nano: string }>(`
    WITH eligible AS (
      SELECT ec.id AS credit_id
      FROM engine_credits ec
      JOIN payments p ON p.id = ec.payment_id
      LEFT JOIN pricing_credit_accruals pca ON pca.credit_id = ec.id
      WHERE ec.engine_account_id = $1 AND ec.status = 'confirmed'
        AND p.user_id = $2 AND p.status = 'paid' AND pca.credit_id IS NULL
    ), inserted AS (
      INSERT INTO pricing_credit_accruals (credit_id)
      SELECT credit_id FROM eligible
      ON CONFLICT (credit_id) DO NOTHING
      RETURNING credit_id
    )
    SELECT COALESCE(SUM(ec.amount_nano), 0)::text AS amount_nano
    FROM inserted i
    JOIN engine_credits ec ON ec.id = i.credit_id
  `, [target.engineAccountId, profile.user_id]);
  const reversedResult = await client.query<{ amount_nano: string }>(`
    WITH removed AS (
      DELETE FROM pricing_credit_accruals pca
      USING engine_credits ec, payments p
      WHERE pca.credit_id = ec.id AND ec.payment_id = p.id
        AND ec.engine_account_id = $1 AND p.user_id = $2
        AND p.status IN ('refunded', 'disputed')
      RETURNING ec.amount_nano
    )
    SELECT COALESCE(SUM(amount_nano), 0)::text AS amount_nano FROM removed
  `, [target.engineAccountId, profile.user_id]);

  const applied = BigInt(appliedResult.rows[0]?.amount_nano ?? "0");
  const reversed = BigInt(reversedResult.rows[0]?.amount_nano ?? "0");
  if (applied === 0n && reversed === 0n) return;
  const currentCumulative = BigInt(profile.cumulative_topup_nano);
  const cumulative = currentCumulative + applied > reversed
    ? currentCumulative + applied - reversed
    : 0n;
  await client.query(`
    UPDATE customer_profiles SET cumulative_topup_nano = $2, updated_at = now() WHERE user_id = $1
  `, [profile.user_id, cumulative.toString()]);
}
