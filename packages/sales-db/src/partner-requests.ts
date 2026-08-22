import { createHash, randomUUID } from "node:crypto";
import type { PoolClient } from "pg";
import type { SalesDatabase } from "./client.js";

export const PARTNER_REQUEST_PROVIDER_IDS = ["anthropic", "openai", "google", "kimi", "glm"] as const;
export type PartnerRequestProviderId = (typeof PARTNER_REQUEST_PROVIDER_IDS)[number];
export type PartnerRequestType = "b2b_conversion" | "b2b_pricing" | "commission_change";
export type PartnerRequestStatus = "pending" | "approved" | "rejected" | "applied" | "apply_failed";
export type PartnerRequestEffectStatus = "pending" | "processing" | "applied" | "failed";

export class PartnerRequestConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PartnerRequestConflictError";
  }
}

export class PartnerRequestValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PartnerRequestValidationError";
  }
}

export class PartnerRequestNotFoundError extends Error {
  constructor(message = "partner request not found") {
    super(message);
    this.name = "PartnerRequestNotFoundError";
  }
}

export class PartnerRequestDecisionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PartnerRequestDecisionError";
  }
}

export interface PartnerRequestProviderTerm {
  providerId: PartnerRequestProviderId;
  requestedDiscountBps: number | null;
  approvedDiscountBps: number | null | undefined;
}

export interface PartnerRequestEffectView {
  id: string;
  status: PartnerRequestEffectStatus;
  attempts: number;
  nextAttemptAt: Date | null;
  terminal: boolean;
  appliedAt: Date | null;
  lastError: string | null;
}

export interface PartnerRequestView {
  id: string;
  requestType: PartnerRequestType;
  status: PartnerRequestStatus;
  requesterPartnerId: string;
  requesterEmail: string | null;
  requesterDisplayName: string | null;
  subjectPartnerId: string | null;
  commerceUserId: string | null;
  reason: string;
  stateSnapshot: Record<string, unknown>;
  requestedCommissionBps: number | null;
  requestedDiscountBps: number | null;
  approvedCommissionBps: number | null;
  approvedDiscountBps: number | null;
  reviewerActor: string | null;
  reviewerNote: string | null;
  reviewedAt: Date | null;
  appliedAt: Date | null;
  applyAttempts: number;
  lastApplyError: string | null;
  version: number;
  createdAt: Date;
  updatedAt: Date;
  providerTerms: PartnerRequestProviderTerm[];
  effect: PartnerRequestEffectView | null;
}

interface PartnerRequestRow {
  id: string;
  request_type: PartnerRequestType;
  status: PartnerRequestStatus;
  requester_partner_id: string;
  requester_email: string | null;
  requester_display_name: string | null;
  subject_partner_id: string | null;
  commerce_user_id: string | null;
  reason: string;
  state_snapshot: unknown;
  requested_commission_bps: number | null;
  requested_discount_bps: number | null;
  approved_commission_bps: number | null;
  approved_discount_bps: number | null;
  reviewer_actor: string | null;
  reviewer_note: string | null;
  reviewed_at: Date | null;
  applied_at: Date | null;
  apply_attempts: number;
  last_apply_error: string | null;
  version: number;
  created_at: Date;
  updated_at: Date;
}

const REQUEST_COLUMNS = `
  request.id, request.request_type, request.status, request.requester_partner_id,
  requester.email AS requester_email, requester.display_name AS requester_display_name,
  request.subject_partner_id, request.commerce_user_id, request.reason, request.state_snapshot,
  request.requested_commission_bps, request.requested_discount_bps,
  request.approved_commission_bps, request.approved_discount_bps,
  request.reviewer_actor, request.reviewer_note, request.reviewed_at, request.applied_at,
  request.apply_attempts, request.last_apply_error, request.version,
  request.created_at, request.updated_at
`;

function requestSnapshot(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

async function hydrateRequestRows(client: PoolClient, rows: PartnerRequestRow[]): Promise<PartnerRequestView[]> {
  if (rows.length === 0) return [];
  const ids = rows.map((row) => row.id);
  const [terms, effects] = await Promise.all([
    client.query<{
      request_id: string;
      provider_id: PartnerRequestProviderId;
      requested_discount_bps: number | null;
      approved_discount_bps: number | null;
      has_decision: boolean;
    }>(`
      SELECT term.request_id, term.provider_id, term.requested_discount_bps,
             decision.approved_discount_bps, decision.request_id IS NOT NULL AS has_decision
      FROM partner_request_provider_terms term
      LEFT JOIN partner_request_provider_decisions decision
        ON decision.request_id = term.request_id AND decision.provider_id = term.provider_id
      WHERE term.request_id = ANY($1::uuid[])
      ORDER BY term.request_id, term.provider_id
    `, [ids]),
    client.query<{
      request_id: string;
      id: string;
      status: PartnerRequestEffectStatus;
      attempts: number;
      next_attempt_at: Date | null;
      terminal: boolean;
      applied_at: Date | null;
      last_error: string | null;
    }>(`
      SELECT request_id, id, status, attempts,
             CASE WHEN next_attempt_at = 'infinity'::timestamptz THEN NULL ELSE next_attempt_at END
               AS next_attempt_at,
             next_attempt_at = 'infinity'::timestamptz AS terminal,
             applied_at, last_error
      FROM partner_request_effects
      WHERE request_id = ANY($1::uuid[])
    `, [ids]),
  ]);
  const termsByRequest = new Map<string, PartnerRequestProviderTerm[]>();
  for (const row of terms.rows) {
    const values = termsByRequest.get(row.request_id) ?? [];
    values.push({
      providerId: row.provider_id,
      requestedDiscountBps: row.requested_discount_bps,
      approvedDiscountBps: row.has_decision ? row.approved_discount_bps : undefined,
    });
    termsByRequest.set(row.request_id, values);
  }
  const effectByRequest = new Map(effects.rows.map((row) => [row.request_id, {
    id: row.id,
    status: row.status,
    attempts: row.attempts,
    nextAttemptAt: row.next_attempt_at,
    terminal: row.terminal,
    appliedAt: row.applied_at,
    lastError: row.last_error,
  }]));
  return rows.map((row) => ({
    id: row.id,
    requestType: row.request_type,
    status: row.status,
    requesterPartnerId: row.requester_partner_id,
    requesterEmail: row.requester_email,
    requesterDisplayName: row.requester_display_name,
    subjectPartnerId: row.subject_partner_id,
    commerceUserId: row.commerce_user_id,
    reason: row.reason,
    stateSnapshot: requestSnapshot(row.state_snapshot),
    requestedCommissionBps: row.requested_commission_bps,
    requestedDiscountBps: row.requested_discount_bps,
    approvedCommissionBps: row.approved_commission_bps,
    approvedDiscountBps: row.approved_discount_bps,
    reviewerActor: row.reviewer_actor,
    reviewerNote: row.reviewer_note,
    reviewedAt: row.reviewed_at,
    appliedAt: row.applied_at,
    applyAttempts: row.apply_attempts,
    lastApplyError: row.last_apply_error,
    version: row.version,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    providerTerms: termsByRequest.get(row.id) ?? [],
    effect: effectByRequest.get(row.id) ?? null,
  }));
}

async function loadRequest(
  client: PoolClient,
  requestId: string,
  requesterPartnerId?: string,
): Promise<PartnerRequestView | null> {
  const values: unknown[] = [requestId];
  const requesterFilter = requesterPartnerId === undefined
    ? ""
    : `AND request.requester_partner_id = $${values.push(requesterPartnerId)}`;
  const result = await client.query<PartnerRequestRow>(`
    SELECT ${REQUEST_COLUMNS}
    FROM partner_requests request
    JOIN partners requester ON requester.id = request.requester_partner_id
    WHERE request.id = $1 ${requesterFilter}
  `, values);
  return (await hydrateRequestRows(client, result.rows))[0] ?? null;
}

async function lockIdempotencyKey(client: PoolClient, key: string): Promise<void> {
  await client.query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))", [key]);
}

function validateReason(reason: string): string {
  const value = reason.trim();
  if (value.length < 1 || value.length > 4000) {
    throw new PartnerRequestValidationError("reason must contain between 1 and 4000 characters");
  }
  return value;
}

function validateIdempotencyKey(key: string): string {
  const value = key.trim();
  if (value.length < 8 || value.length > 200) {
    throw new PartnerRequestValidationError("idempotency key must contain between 8 and 200 characters");
  }
  return value;
}

/** Public idempotency keys are scoped to the authenticated partner before entering the global DB index. */
function scopedIdempotencyKey(partnerId: string, key: string): string {
  return `partner-request:v1:${createHash("sha256")
    .update(partnerId)
    .update("\0")
    .update(key)
    .digest("hex")}`;
}

function uniqueViolation(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "23505";
}

export async function createCommissionChangeRequest(database: SalesDatabase, input: {
  requesterPartnerId: string;
  requestedCommissionBps: number;
  reason: string;
  idempotencyKey: string;
  requireProgramEnabled?: boolean;
}): Promise<PartnerRequestView> {
  const reason = validateReason(input.reason);
  const idempotencyKey = scopedIdempotencyKey(
    input.requesterPartnerId,
    validateIdempotencyKey(input.idempotencyKey),
  );
  if (!Number.isInteger(input.requestedCommissionBps)
    || input.requestedCommissionBps < 0
    || input.requestedCommissionBps > 10_000) {
    throw new PartnerRequestValidationError("requested commission must be between 0 and 10000 bps");
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await lockIdempotencyKey(client, idempotencyKey);
    const replay = await client.query<PartnerRequestRow>(`
      SELECT ${REQUEST_COLUMNS}
      FROM partner_requests request
      JOIN partners requester ON requester.id = request.requester_partner_id
      WHERE request.idempotency_key = $1
    `, [idempotencyKey]);
    if (replay.rows[0]) {
      const existing = (await hydrateRequestRows(client, replay.rows))[0]!;
      if (existing.requestType !== "commission_change"
        || existing.requesterPartnerId !== input.requesterPartnerId
        || existing.requestedCommissionBps !== input.requestedCommissionBps
        || existing.reason !== reason) {
        throw new PartnerRequestConflictError("idempotency key was already used for another request");
      }
      await client.query("COMMIT");
      return existing;
    }
    const partner = await client.query<{ commission_bps: number; program_enabled: boolean }>(`
      SELECT commission_bps, program_enabled FROM partners
      WHERE id = $1 AND status = 'active'
      FOR UPDATE
    `, [input.requesterPartnerId]);
    const current = partner.rows[0]?.commission_bps;
    if (current === undefined) throw new PartnerRequestNotFoundError("active partner not found");
    if (input.requireProgramEnabled === true && partner.rows[0]?.program_enabled !== true) {
      throw new PartnerRequestNotFoundError("active Commerce partner membership not found");
    }
    if (input.requestedCommissionBps <= current) {
      throw new PartnerRequestValidationError("requested commission must be higher than the current commission");
    }
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_requests (
        request_type, requester_partner_id, subject_partner_id, reason, state_snapshot,
        requested_commission_bps, idempotency_key
      )
      VALUES ('commission_change', $1, $1, $2, $3::jsonb, $4, $5)
      RETURNING id
    `, [
      input.requesterPartnerId,
      reason,
      JSON.stringify({ currentCommissionBps: current }),
      input.requestedCommissionBps,
      idempotencyKey,
    ]);
    const requestId = inserted.rows[0]!.id;
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'partner_request.created', 'partner_request', $2, $3::jsonb)
    `, [input.requesterPartnerId, requestId, JSON.stringify({ requestType: "commission_change" })]);
    const request = await loadRequest(client, requestId);
    await client.query("COMMIT");
    return request!;
  } catch (error) {
    await client.query("ROLLBACK");
    if (uniqueViolation(error)) {
      throw new PartnerRequestConflictError("a commission change request is already pending");
    }
    throw error;
  } finally {
    client.release();
  }
}

export async function createB2BPartnerRequest(database: SalesDatabase, input: {
  requesterPartnerId: string;
  commerceUserId: string;
  requestType: "b2b_conversion" | "b2b_pricing";
  requestedDiscountBps: number;
  providers: Partial<Record<PartnerRequestProviderId, number | null>>;
  reason: string;
  stateSnapshot: Record<string, unknown>;
  idempotencyKey: string;
  requireProgramEnabled?: boolean;
}): Promise<PartnerRequestView> {
  const reason = validateReason(input.reason);
  const idempotencyKey = scopedIdempotencyKey(
    input.requesterPartnerId,
    validateIdempotencyKey(input.idempotencyKey),
  );
  if (!Number.isInteger(input.requestedDiscountBps)
    || input.requestedDiscountBps < 0
    || input.requestedDiscountBps > 9_500
    || input.requestedDiscountBps % 100 !== 0) {
    throw new PartnerRequestValidationError("requested discount must be a whole percent between 0 and 95");
  }
  const providerEntries = Object.entries(input.providers)
    .sort(([left], [right]) => left.localeCompare(right)) as Array<[PartnerRequestProviderId, number | null]>;
  for (const [providerId, bps] of providerEntries) {
    if (!PARTNER_REQUEST_PROVIDER_IDS.includes(providerId)
      || (bps !== null && (!Number.isInteger(bps) || bps < 0 || bps > 9_500 || bps % 100 !== 0))) {
      throw new PartnerRequestValidationError(`invalid requested provider discount for ${providerId}`);
    }
  }
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    await lockIdempotencyKey(client, idempotencyKey);
    const replay = await client.query<PartnerRequestRow>(`
      SELECT ${REQUEST_COLUMNS}
      FROM partner_requests request
      JOIN partners requester ON requester.id = request.requester_partner_id
      WHERE request.idempotency_key = $1
    `, [idempotencyKey]);
    if (replay.rows[0]) {
      const existing = (await hydrateRequestRows(client, replay.rows))[0]!;
      const existingProviders = existing.providerTerms.map((term) => [term.providerId, term.requestedDiscountBps]);
      if (existing.requestType !== input.requestType
        || existing.requesterPartnerId !== input.requesterPartnerId
        || existing.commerceUserId !== input.commerceUserId
        || existing.requestedDiscountBps !== input.requestedDiscountBps
        || existing.reason !== reason
        || JSON.stringify(existingProviders) !== JSON.stringify(providerEntries)) {
        throw new PartnerRequestConflictError("idempotency key was already used for another request");
      }
      await client.query("COMMIT");
      return existing;
    }
    await client.query(
      "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
      [`b2b-request:${input.requesterPartnerId}:${input.commerceUserId}`],
    );
    const active = await client.query<{ id: string }>(`
      SELECT request.id
      FROM partner_requests request
      LEFT JOIN partner_request_effects effect ON effect.request_id = request.id
      WHERE request.requester_partner_id = $1 AND request.commerce_user_id = $2
        AND request.request_type IN ('b2b_conversion', 'b2b_pricing')
        AND (
          request.status IN ('pending', 'approved')
          OR (request.status = 'apply_failed'
            AND (effect.id IS NULL OR effect.next_attempt_at <> 'infinity'::timestamptz))
        )
      LIMIT 1
    `, [input.requesterPartnerId, input.commerceUserId]);
    if (active.rows[0]) {
      throw new PartnerRequestConflictError("a B2B request for this referral is already active");
    }
    const owner = await client.query<{ referral_code: string }>(`
      SELECT partner.referral_code
      FROM referred_users referral
      JOIN partners partner ON partner.id = referral.partner_id
      WHERE referral.partner_id = $1 AND referral.commerce_user_id = $2
        AND partner.status = 'active'
        AND ($3::boolean = false OR partner.program_enabled = true)
      FOR SHARE OF partner
    `, [input.requesterPartnerId, input.commerceUserId, input.requireProgramEnabled === true]);
    if (!owner.rows[0]) throw new PartnerRequestNotFoundError("referral not found for this partner");
    const inserted = await client.query<{ id: string }>(`
      INSERT INTO partner_requests (
        request_type, requester_partner_id, commerce_user_id, reason, state_snapshot,
        requested_discount_bps, idempotency_key
      )
      VALUES ($1::partner_request_type, $2, $3, $4, $5::jsonb, $6, $7)
      RETURNING id
    `, [
      input.requestType,
      input.requesterPartnerId,
      input.commerceUserId,
      reason,
      JSON.stringify({ ...input.stateSnapshot, referralCode: owner.rows[0].referral_code }),
      input.requestedDiscountBps,
      idempotencyKey,
    ]);
    const requestId = inserted.rows[0]!.id;
    for (const [providerId, requestedDiscountBps] of providerEntries) {
      await client.query(`
        INSERT INTO partner_request_provider_terms (request_id, provider_id, requested_discount_bps)
        VALUES ($1, $2, $3)
      `, [requestId, providerId, requestedDiscountBps]);
    }
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('partner', $1, 'partner_request.created', 'partner_request', $2, $3::jsonb)
    `, [input.requesterPartnerId, requestId, JSON.stringify({ requestType: input.requestType })]);
    const request = await loadRequest(client, requestId);
    await client.query("COMMIT");
    return request!;
  } catch (error) {
    await client.query("ROLLBACK");
    if (uniqueViolation(error)) {
      throw new PartnerRequestConflictError("a B2B request for this referral is already pending");
    }
    throw error;
  } finally {
    client.release();
  }
}

export interface PartnerRequestPage {
  items: PartnerRequestView[];
  nextCursor: { createdAt: Date; id: string } | null;
}

export async function listPartnerRequests(database: SalesDatabase, input: {
  requesterPartnerId?: string;
  /** Limit the new Commerce Admin queue to account-bound program memberships. */
  commerceProgramOnly?: boolean;
  status?: PartnerRequestStatus;
  requestType?: PartnerRequestType;
  before?: { createdAt: Date; id: string };
  limit: number;
}): Promise<PartnerRequestPage> {
  const limit = Math.min(Math.max(input.limit, 1), 100);
  const conditions: string[] = [];
  const values: unknown[] = [];
  const add = (sql: string, value: unknown): void => {
    values.push(value);
    conditions.push(sql.replace("?", `$${values.length}`));
  };
  if (input.requesterPartnerId !== undefined) add("request.requester_partner_id = ?", input.requesterPartnerId);
  if (input.commerceProgramOnly === true) {
    conditions.push("requester.commerce_user_id IS NOT NULL");
  }
  if (input.status !== undefined) add("request.status = ?::partner_request_status", input.status);
  if (input.requestType !== undefined) add("request.request_type = ?::partner_request_type", input.requestType);
  if (input.before !== undefined) {
    values.push(input.before.createdAt, input.before.id);
    conditions.push(`(request.created_at, request.id) < ($${values.length - 1}, $${values.length})`);
  }
  values.push(limit + 1);
  const result = await database.pool.query<PartnerRequestRow>(`
    SELECT ${REQUEST_COLUMNS}
    FROM partner_requests request
    JOIN partners requester ON requester.id = request.requester_partner_id
    ${conditions.length > 0 ? `WHERE ${conditions.join(" AND ")}` : ""}
    ORDER BY request.created_at DESC, request.id DESC
    LIMIT $${values.length}
  `, values);
  const hasMore = result.rows.length > limit;
  const pageRows = result.rows.slice(0, limit);
  const client = await database.pool.connect();
  try {
    const items = await hydrateRequestRows(client, pageRows);
    const last = pageRows.at(-1);
    return {
      items,
      nextCursor: hasMore && last ? { createdAt: last.created_at, id: last.id } : null,
    };
  } finally {
    client.release();
  }
}

export async function getPartnerRequest(
  database: SalesDatabase,
  requestId: string,
  requesterPartnerId?: string,
): Promise<PartnerRequestView | null> {
  const client = await database.pool.connect();
  try {
    return await loadRequest(client, requestId, requesterPartnerId);
  } finally {
    client.release();
  }
}

export async function decidePartnerRequest(database: SalesDatabase, input: {
  requestId: string;
  action: "approve" | "reject";
  reviewerActor: string;
  reviewerNote: string;
  approvedCommissionBps?: number;
  approvedDiscountBps?: number;
  providers?: Partial<Record<PartnerRequestProviderId, number | null>>;
  /** Keep the Commerce Admin boundary from deciding a legacy Telegram request by raw UUID. */
  requireCommerceProgram?: boolean;
}): Promise<PartnerRequestView> {
  const actor = input.reviewerActor.trim();
  const note = input.reviewerNote.trim();
  if (actor.length < 1 || actor.length > 200) throw new PartnerRequestDecisionError("invalid reviewer actor");
  if (note.length < 1 || note.length > 4000) throw new PartnerRequestDecisionError("reviewer note is required");
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const locked = await client.query<{
      id: string;
      request_type: PartnerRequestType;
      status: PartnerRequestStatus;
      requester_partner_id: string;
      commerce_user_id: string | null;
      requested_commission_bps: number | null;
      requested_discount_bps: number | null;
      reason: string;
      state_snapshot: unknown;
      referral_code: string;
      current_commission_bps: number;
    }>(`
      SELECT request.id, request.request_type, request.status, request.requester_partner_id,
             request.commerce_user_id, request.requested_commission_bps,
             request.requested_discount_bps, request.reason, request.state_snapshot,
             requester.referral_code, requester.commission_bps AS current_commission_bps
      FROM partner_requests request
      JOIN partners requester ON requester.id = request.requester_partner_id
      WHERE request.id = $1
        AND ($2::boolean = false OR requester.commerce_user_id IS NOT NULL)
      FOR UPDATE OF request, requester
    `, [input.requestId, input.requireCommerceProgram === true]);
    const request = locked.rows[0];
    if (!request) throw new PartnerRequestNotFoundError();
    if (request.status !== "pending") throw new PartnerRequestDecisionError("partner request was already decided");
    if (input.action === "reject") {
      await client.query(`
        UPDATE partner_requests
        SET status = 'rejected', reviewer_actor = $2, reviewer_note = $3, reviewed_at = now()
        WHERE id = $1
      `, [input.requestId, actor, note]);
    } else if (request.request_type === "commission_change") {
      const approved = input.approvedCommissionBps;
      if (!Number.isInteger(approved)
        || approved! <= request.current_commission_bps
        || approved! > (request.requested_commission_bps ?? -1)) {
        throw new PartnerRequestDecisionError(
          "approved commission must be above the current rate and no higher than requested",
        );
      }
      await client.query(`
        UPDATE partners SET commission_bps = $2, updated_at = now() WHERE id = $1
      `, [request.requester_partner_id, approved]);
      await client.query(`
        UPDATE partner_requests
        SET status = 'applied', approved_commission_bps = $2,
            reviewer_actor = $3, reviewer_note = $4, reviewed_at = now(), applied_at = now()
        WHERE id = $1
      `, [input.requestId, approved, actor, note]);
    } else {
      const approved = input.approvedDiscountBps;
      if (!Number.isInteger(approved)
        || approved! < 0
        || approved! > (request.requested_discount_bps ?? -1)
        || approved! % 100 !== 0) {
        throw new PartnerRequestDecisionError(
          "approved discount must be a whole percent no higher than requested",
        );
      }
      const terms = await client.query<{
        provider_id: PartnerRequestProviderId;
        requested_discount_bps: number | null;
      }>(`
        SELECT provider_id, requested_discount_bps
        FROM partner_request_provider_terms
        WHERE request_id = $1
        ORDER BY provider_id
        FOR SHARE
      `, [input.requestId]);
      const decisions = Object.entries(input.providers ?? {})
        .sort(([left], [right]) => left.localeCompare(right)) as Array<[PartnerRequestProviderId, number | null]>;
      const requestedIds = terms.rows.map((term) => term.provider_id);
      if (JSON.stringify(decisions.map(([providerId]) => providerId)) !== JSON.stringify(requestedIds)) {
        throw new PartnerRequestDecisionError("every requested provider term needs an explicit decision");
      }
      for (const term of terms.rows) {
        const decision = decisions.find(([providerId]) => providerId === term.provider_id)![1];
        if (term.requested_discount_bps === null ? decision !== null : (
          decision === null
          || !Number.isInteger(decision)
          || decision < 0
          || decision > term.requested_discount_bps
          || decision % 100 !== 0
        )) {
          throw new PartnerRequestDecisionError(`invalid decision for provider ${term.provider_id}`);
        }
      }
      await client.query(`
        UPDATE partner_requests
        SET status = 'approved', approved_discount_bps = $2,
            reviewer_actor = $3, reviewer_note = $4, reviewed_at = now()
        WHERE id = $1
      `, [input.requestId, approved, actor, note]);
      for (const [providerId, approvedDiscountBps] of decisions) {
        await client.query(`
          INSERT INTO partner_request_provider_decisions
            (request_id, provider_id, approved_discount_bps)
          VALUES ($1, $2, $3)
        `, [input.requestId, providerId, approvedDiscountBps]);
      }
      const providerPercents = Object.fromEntries(decisions.map(([providerId, bps]) => [
        providerId,
        bps === null ? null : bps / 100,
      ]));
      const ceilingPercent = Math.max(
        approved! / 100,
        ...decisions.flatMap(([, bps]) => bps === null ? [] : [bps / 100]),
      );
      const payload = {
        operationRef: `partner-effect:${input.requestId}`,
        userId: request.commerce_user_id,
        referralCode: request.referral_code,
        ceilingPercent,
        discountPercent: approved! / 100,
        providers: providerPercents,
        actorId: actor,
        reason: `Approved partner request ${input.requestId}: ${note}`.slice(0, 4000),
      };
      await client.query(`
        INSERT INTO partner_request_effects (request_id, payload, idempotency_key)
        VALUES ($1, $2::jsonb, $3)
      `, [input.requestId, JSON.stringify(payload), payload.operationRef]);
    }
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('admin', $1, 'partner_request.decided', 'partner_request', $2, $3::jsonb)
    `, [actor, input.requestId, JSON.stringify({ action: input.action, reviewerNote: note })]);
    const decided = await loadRequest(client, input.requestId);
    await client.query("COMMIT");
    return decided!;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export interface PartnerRequestCommerceEffect {
  effectId: string;
  requestId: string;
  leaseToken: string;
  attempts: number;
  payload: {
    operationRef: string;
    userId: string;
    referralCode: string;
    ceilingPercent: number;
    discountPercent: number;
    providers: Record<string, number | null>;
    actorId: string;
    reason: string;
  };
}

export async function recoverStalePartnerRequestEffects(
  database: SalesDatabase,
  leaseSeconds: number,
): Promise<number> {
  const result = await database.pool.query(`
    UPDATE partner_request_effects
    SET status = 'pending', locked_at = NULL, locked_by = NULL,
        next_attempt_at = now(), last_error = NULL, updated_at = now()
    WHERE status = 'processing' AND locked_at < now() - ($1 * interval '1 second')
  `, [leaseSeconds]);
  return result.rowCount ?? 0;
}

export async function claimPartnerRequestEffect(
  database: SalesDatabase,
  workerId: string,
): Promise<PartnerRequestCommerceEffect | null> {
  const leaseToken = `${workerId}:${randomUUID()}`;
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const result = await client.query<{
      id: string;
      request_id: string;
      attempts: number;
      payload: PartnerRequestCommerceEffect["payload"];
    }>(`
      WITH candidate AS (
        SELECT effect.id
        FROM partner_request_effects effect
        JOIN partner_requests request ON request.id = effect.request_id
        WHERE effect.status IN ('pending', 'failed')
          AND effect.next_attempt_at <= now()
          AND request.status IN ('approved', 'apply_failed')
        ORDER BY effect.next_attempt_at, effect.created_at, effect.id
        FOR UPDATE OF effect SKIP LOCKED
        LIMIT 1
      )
      UPDATE partner_request_effects effect
      SET status = 'processing', attempts = effect.attempts + 1,
          locked_at = now(), locked_by = $1, updated_at = now()
      FROM candidate
      WHERE effect.id = candidate.id
      RETURNING effect.id, effect.request_id, effect.attempts, effect.payload
    `, [leaseToken]);
    const row = result.rows[0];
    if (!row) {
      await client.query("COMMIT");
      return null;
    }
    const request = await client.query(`
      UPDATE partner_requests SET apply_attempts = apply_attempts + 1
      WHERE id = $1 AND status IN ('approved', 'apply_failed')
    `, [row.request_id]);
    if ((request.rowCount ?? 0) !== 1) throw new Error("claimed effect lost its applicable request");
    await client.query("COMMIT");
    return {
      effectId: row.id,
      requestId: row.request_id,
      leaseToken,
      attempts: row.attempts,
      payload: row.payload,
    };
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function markPartnerRequestEffectApplied(database: SalesDatabase, input: {
  effectId: string;
  requestId: string;
  leaseToken: string;
  commerceOperationRef: string;
  idempotentReplay: boolean;
}): Promise<boolean> {
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const effect = await client.query<{ id: string }>(`
      UPDATE partner_request_effects
      SET status = 'applied', locked_at = NULL, locked_by = NULL,
          applied_at = now(), last_error = NULL, updated_at = now()
      WHERE id = $1 AND request_id = $2 AND status = 'processing' AND locked_by = $3
      RETURNING id
    `, [input.effectId, input.requestId, input.leaseToken]);
    if (!effect.rows[0]) {
      await client.query("ROLLBACK");
      return false;
    }
    const request = await client.query(`
      UPDATE partner_requests
      SET status = 'applied', applied_at = now(), last_apply_error = NULL
      WHERE id = $1 AND status IN ('approved', 'apply_failed')
    `, [input.requestId]);
    if ((request.rowCount ?? 0) !== 1) throw new Error("applied effect lost its request transition");
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', $1, 'partner_request.effect_applied', 'partner_request', $2, $3::jsonb)
    `, [input.leaseToken.split(":", 1)[0] ?? "sales-worker", input.requestId, JSON.stringify({
      commerceOperationRef: input.commerceOperationRef,
      idempotentReplay: input.idempotentReplay,
    })]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

export async function markPartnerRequestEffectFailed(database: SalesDatabase, input: {
  effectId: string;
  requestId: string;
  leaseToken: string;
  error: string;
  retryAfterSeconds: number;
  terminal: boolean;
}): Promise<boolean> {
  const message = input.error.trim().slice(0, 4000) || "unknown Commerce effect failure";
  const client = await database.pool.connect();
  try {
    await client.query("BEGIN");
    const effect = await client.query<{ id: string }>(`
      UPDATE partner_request_effects
      SET status = 'failed', locked_at = NULL, locked_by = NULL,
          next_attempt_at = CASE WHEN $4 THEN 'infinity'::timestamptz
            ELSE now() + ($5 * interval '1 second') END,
          last_error = $6, updated_at = now()
      WHERE id = $1 AND request_id = $2 AND status = 'processing' AND locked_by = $3
      RETURNING id
    `, [
      input.effectId,
      input.requestId,
      input.leaseToken,
      input.terminal,
      Math.max(1, Math.floor(input.retryAfterSeconds)),
      message,
    ]);
    if (!effect.rows[0]) {
      await client.query("ROLLBACK");
      return false;
    }
    const request = await client.query(`
      UPDATE partner_requests
      SET status = 'apply_failed', last_apply_error = $2
      WHERE id = $1 AND status IN ('approved', 'apply_failed')
    `, [input.requestId, message]);
    if ((request.rowCount ?? 0) !== 1) throw new Error("failed effect lost its request transition");
    await client.query(`
      INSERT INTO sales_audit_log (actor_type, actor_id, action, target_type, target_id, metadata)
      VALUES ('system', $1, 'partner_request.effect_failed', 'partner_request', $2, $3::jsonb)
    `, [input.leaseToken.split(":", 1)[0] ?? "sales-worker", input.requestId, JSON.stringify({
      terminal: input.terminal,
      error: message,
    })]);
    await client.query("COMMIT");
    return true;
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}
