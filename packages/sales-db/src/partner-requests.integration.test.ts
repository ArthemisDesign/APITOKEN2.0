import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  claimPartnerRequestEffect,
  createB2BPartnerRequest,
  createCommissionChangeRequest,
  decidePartnerRequest,
  getPartnerRequest,
  listPartnerRequests,
  markPartnerRequestEffectApplied,
  markPartnerRequestEffectFailed,
  PartnerRequestConflictError,
  PartnerRequestDecisionError,
  PartnerRequestNotFoundError,
} from "./partner-requests.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("partner authority requests and Commerce effects", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(truncate);

  async function truncate(): Promise<void> {
    await db.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function partner(email = "partner@example.test", commissionBps = 1_000): Promise<{
    id: string;
    referralCode: string;
  }> {
    const referralCode = `ref${randomUUID().replaceAll("-", "").slice(0, 12)}`;
    const result = await db.pool.query<{ id: string }>(`
      INSERT INTO partners (email, email_verified, status, referral_code, commission_bps)
      VALUES ($1, true, 'active', $2, $3)
      RETURNING id
    `, [email, referralCode, commissionBps]);
    return { id: result.rows[0]!.id, referralCode };
  }

  async function referral(partnerId: string): Promise<string> {
    const commerceUserId = randomUUID();
    await db.pool.query(`
      INSERT INTO referred_users (commerce_user_id, partner_id, referral_code)
      VALUES ($1, $2, 'request-test')
    `, [commerceUserId, partnerId]);
    return commerceUserId;
  }

  it("idempotently records and atomically applies an approved commission increase", async () => {
    const owner = await partner();
    const input = {
      requesterPartnerId: owner.id,
      requestedCommissionBps: 1_500,
      reason: "Consistent qualified volume",
      idempotencyKey: `commission:${randomUUID()}`,
    };
    const created = await createCommissionChangeRequest(db, input);
    const replay = await createCommissionChangeRequest(db, input);
    expect(replay.id).toBe(created.id);
    await expect(createCommissionChangeRequest(db, {
      ...input,
      requestedCommissionBps: 1_600,
    })).rejects.toBeInstanceOf(PartnerRequestConflictError);
    await expect(createCommissionChangeRequest(db, {
      ...input,
      idempotencyKey: `commission:${randomUUID()}`,
    })).rejects.toBeInstanceOf(PartnerRequestConflictError);

    const decided = await decidePartnerRequest(db, {
      requestId: created.id,
      action: "approve",
      approvedCommissionBps: 1_400,
      reviewerActor: "admin:user@example.test",
      reviewerNote: "Approved after volume review",
    });
    expect(decided).toMatchObject({ status: "applied", approvedCommissionBps: 1_400 });
    const stored = await db.pool.query<{ commission_bps: number }>(
      "SELECT commission_bps FROM partners WHERE id = $1",
      [owner.id],
    );
    expect(stored.rows[0]?.commission_bps).toBe(1_400);
    await expect(decidePartnerRequest(db, {
      requestId: created.id,
      action: "reject",
      reviewerActor: "admin:user@example.test",
      reviewerNote: "second decision",
    })).rejects.toBeInstanceOf(PartnerRequestDecisionError);
  });

  it("scopes a public idempotency key to the authenticated partner", async () => {
    const first = await partner("first@example.test");
    const second = await partner("second@example.test");
    const publicKey = `commission:${randomUUID()}`;
    const [left, right] = await Promise.all([
      createCommissionChangeRequest(db, {
        requesterPartnerId: first.id,
        requestedCommissionBps: 1_100,
        reason: "first partner request",
        idempotencyKey: publicKey,
      }),
      createCommissionChangeRequest(db, {
        requesterPartnerId: second.id,
        requestedCommissionBps: 1_100,
        reason: "second partner request",
        idempotencyKey: publicKey,
      }),
    ]);
    expect(left.id).not.toBe(right.id);
  });

  it("proves referral ownership before accepting a B2B request", async () => {
    const owner = await partner("owner@example.test");
    const other = await partner("other@example.test");
    const commerceUserId = await referral(owner.id);
    await expect(createB2BPartnerRequest(db, {
      requesterPartnerId: other.id,
      commerceUserId,
      requestType: "b2b_conversion",
      requestedDiscountBps: 4_000,
      providers: {},
      reason: "Customer requested business terms",
      stateSnapshot: { customerType: "b2c" },
      idempotencyKey: `b2b:${randomUUID()}`,
    })).rejects.toBeInstanceOf(PartnerRequestNotFoundError);
  });

  it("keeps provider evidence immutable and delivers one fenced retryable Commerce effect", async () => {
    const owner = await partner();
    const commerceUserId = await referral(owner.id);
    const created = await createB2BPartnerRequest(db, {
      requesterPartnerId: owner.id,
      commerceUserId,
      requestType: "b2b_conversion",
      requestedDiscountBps: 5_000,
      providers: { anthropic: 4_500, openai: null },
      reason: "Business customer needs negotiated provider terms",
      stateSnapshot: { customerType: "b2c", discountPercent: 50 },
      idempotencyKey: `b2b:${randomUUID()}`,
    });
    const approved = await decidePartnerRequest(db, {
      requestId: created.id,
      action: "approve",
      approvedDiscountBps: 4_000,
      providers: { anthropic: 3_500, openai: null },
      reviewerActor: "admin:pricing@example.test",
      reviewerNote: "Margin checked",
    });
    expect(approved).toMatchObject({ status: "approved", approvedDiscountBps: 4_000 });
    expect(approved.providerTerms).toEqual([
      { providerId: "anthropic", requestedDiscountBps: 4_500, approvedDiscountBps: 3_500 },
      { providerId: "openai", requestedDiscountBps: null, approvedDiscountBps: null },
    ]);
    await expect(createB2BPartnerRequest(db, {
      requesterPartnerId: owner.id,
      commerceUserId,
      requestType: "b2b_pricing",
      requestedDiscountBps: 3_000,
      providers: {},
      reason: "A second request must wait for the active effect",
      stateSnapshot: { customerType: "b2b" },
      idempotencyKey: `b2b:${randomUUID()}`,
    })).rejects.toBeInstanceOf(PartnerRequestConflictError);

    const first = await claimPartnerRequestEffect(db, "worker-a");
    expect(first).not.toBeNull();
    expect(first!.payload).toMatchObject({
      operationRef: `partner-effect:${created.id}`,
      userId: commerceUserId,
      referralCode: owner.referralCode,
      ceilingPercent: 40,
      discountPercent: 40,
      providers: { anthropic: 35, openai: null },
    });
    expect(await markPartnerRequestEffectFailed(db, {
      effectId: first!.effectId,
      requestId: first!.requestId,
      leaseToken: `${first!.leaseToken}:stale`,
      error: "stale worker",
      retryAfterSeconds: 1,
      terminal: false,
    })).toBe(false);
    expect(await markPartnerRequestEffectFailed(db, {
      effectId: first!.effectId,
      requestId: first!.requestId,
      leaseToken: first!.leaseToken,
      error: "temporary Commerce timeout",
      retryAfterSeconds: 1,
      terminal: false,
    })).toBe(true);
    await db.pool.query(`
      UPDATE partner_request_effects SET next_attempt_at = now() WHERE id = $1
    `, [first!.effectId]);
    const retry = await claimPartnerRequestEffect(db, "worker-b");
    expect(retry?.effectId).toBe(first!.effectId);
    expect(retry?.leaseToken).not.toBe(first!.leaseToken);
    expect(await markPartnerRequestEffectApplied(db, {
      effectId: retry!.effectId,
      requestId: retry!.requestId,
      leaseToken: retry!.leaseToken,
      commerceOperationRef: retry!.payload.operationRef,
      idempotentReplay: true,
    })).toBe(true);
    expect(await getPartnerRequest(db, created.id)).toMatchObject({
      status: "applied",
      applyAttempts: 2,
      lastApplyError: null,
      effect: { status: "applied", attempts: 2 },
    });
    await expect(db.pool.query(`
      UPDATE partner_request_provider_terms SET requested_discount_bps = 100 WHERE request_id = $1
    `, [created.id])).rejects.toMatchObject({ code: "23514" });
  });

  it("makes terminal Commerce conflicts visible and non-claimable", async () => {
    const owner = await partner();
    const commerceUserId = await referral(owner.id);
    const request = await createB2BPartnerRequest(db, {
      requesterPartnerId: owner.id,
      commerceUserId,
      requestType: "b2b_pricing",
      requestedDiscountBps: 3_000,
      providers: {},
      reason: "Adjust existing business pricing",
      stateSnapshot: { customerType: "b2b" },
      idempotencyKey: `b2b:${randomUUID()}`,
    });
    await decidePartnerRequest(db, {
      requestId: request.id,
      action: "approve",
      approvedDiscountBps: 3_000,
      providers: {},
      reviewerActor: "admin:pricing@example.test",
      reviewerNote: "Approved",
    });
    const effect = await claimPartnerRequestEffect(db, "worker-terminal");
    await markPartnerRequestEffectFailed(db, {
      effectId: effect!.effectId,
      requestId: effect!.requestId,
      leaseToken: effect!.leaseToken,
      error: "operation ref conflict",
      retryAfterSeconds: 1,
      terminal: true,
    });
    expect(await claimPartnerRequestEffect(db, "worker-terminal-2")).toBeNull();
    expect(await getPartnerRequest(db, request.id)).toMatchObject({
      status: "apply_failed",
      lastApplyError: "operation ref conflict",
      effect: {
        status: "failed",
        terminal: true,
        nextAttemptAt: null,
        lastError: "operation ref conflict",
      },
    });
    await expect(createB2BPartnerRequest(db, {
      requesterPartnerId: owner.id,
      commerceUserId,
      requestType: "b2b_pricing",
      requestedDiscountBps: 2_500,
      providers: {},
      reason: "Supersede the closed terminal request",
      stateSnapshot: { customerType: "b2b" },
      idempotencyKey: `b2b:${randomUUID()}`,
    })).resolves.toMatchObject({ status: "pending" });
  });

  it("uses stable keyset pagination and scopes partner reads", async () => {
    const owner = await partner();
    const other = await partner("other@example.test");
    await createCommissionChangeRequest(db, {
      requesterPartnerId: owner.id,
      requestedCommissionBps: 1_100,
      reason: "first",
      idempotencyKey: `commission:${randomUUID()}`,
    });
    const first = await listPartnerRequests(db, { requesterPartnerId: owner.id, limit: 1 });
    expect(first.items).toHaveLength(1);
    expect(first.items[0]?.requesterEmail).toBe("partner@example.test");
    expect(await getPartnerRequest(db, first.items[0]!.id, other.id)).toBeNull();
  });
});
