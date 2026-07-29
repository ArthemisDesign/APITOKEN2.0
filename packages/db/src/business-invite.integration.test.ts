import { createHash, randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  BusinessInvitationNotFoundError,
  createBusinessInvite,
  getBusinessInvitePreview,
  getBusinessInviteToken,
  InvalidBusinessInvitationError,
  revokeBusinessInvite,
  rotateBusinessInvite,
} from "./pricing.js";
import { createEmailUser } from "./auth.js";
import { createDatabase, type Database } from "./client.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("business invitation lifecycle", () => {
  let database: Database;

  beforeAll(() => {
    database = createDatabase(connectionString!);
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE audit_log, email_outbox, auth_tokens, auth_sessions, auth_identities,
               oauth_transactions, engine_pricing_jobs, pricing_months, business_invites,
               customer_profiles, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("creates an email-bound invite atomically with its delivery job and replays by idempotency key", async () => {
    const token = "a".repeat(43);
    const input = {
      email: "Buyer@Example.com",
      tokenHash: tokenHash(token),
      encryptedToken: "encrypted-token",
      multiplierBp: 2500,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "admin@example.com",
      reason: "negotiated enterprise terms",
    };
    const created = await createBusinessInvite(database, input);
    const replay = await createBusinessInvite(database, {
      ...input,
      tokenHash: tokenHash("b".repeat(43)),
      encryptedToken: "different-encrypted-token",
    });

    expect(created).toMatchObject({
      email: "buyer@example.com",
      deliveryStatus: "pending",
      idempotentReplay: false,
    });
    expect(replay).toMatchObject({
      id: created.id,
      encryptedToken: "encrypted-token",
      idempotentReplay: true,
    });
    const outbox = await database.pool.query(`
      SELECT recipient, template, status, business_invite_id
      FROM email_outbox
    `);
    expect(outbox.rows).toEqual([{
      recipient: "buyer@example.com",
      template: "business_invite",
      status: "pending",
      business_invite_id: created.id,
    }]);
    const audit = await database.pool.query(`
      SELECT actor_id, metadata FROM audit_log
      WHERE action = 'business_invite.created' AND target_id = $1
    `, [created.id]);
    expect(audit.rows).toEqual([{
      actor_id: "admin@example.com",
      metadata: expect.objectContaining({
        reason: "negotiated enterprise terms",
        delivery: "email",
      }),
    }]);
  });

  it("allows an unbound link for any email but consumes it exactly once under concurrency", async () => {
    const token = "c".repeat(43);
    const created = await createBusinessInvite(database, {
      tokenHash: tokenHash(token),
      encryptedToken: "encrypted-token",
      multiplierBp: 3000,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "admin@example.com",
      reason: "shareable business onboarding link",
    });
    expect(created.deliveryStatus).toBe("copy_only");
    await expect(getBusinessInvitePreview(database, tokenHash(token))).resolves.toMatchObject({
      email: null,
      multiplierBp: 3000,
    });

    const attempts = await Promise.allSettled([
      createEmailUser(database, "one@example.com", "password-hash", tokenHash(token)),
      createEmailUser(database, "two@example.com", "password-hash", tokenHash(token)),
    ]);
    expect(attempts.filter((result) => result.status === "fulfilled")).toHaveLength(1);
    const rejected = attempts.find((result) => result.status === "rejected");
    expect(rejected).toMatchObject({ reason: expect.any(InvalidBusinessInvitationError) });
    const stored = await database.pool.query(`
      SELECT consumed_at IS NOT NULL AS consumed, encrypted_token
      FROM business_invites WHERE id = $1
    `, [created.id]);
    expect(stored.rows[0]).toEqual({ consumed: true, encrypted_token: null });
  });

  it("enforces a bound recipient and makes revoked links unavailable for preview or copying", async () => {
    const token = "d".repeat(43);
    const created = await createBusinessInvite(database, {
      email: "bound@example.com",
      tokenHash: tokenHash(token),
      encryptedToken: "encrypted-token",
      multiplierBp: 2000,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "admin@example.com",
      reason: "bound enterprise invitation",
    });
    await expect(
      createEmailUser(database, "other@example.com", "password-hash", tokenHash(token)),
    ).rejects.toBeInstanceOf(InvalidBusinessInvitationError);

    await revokeBusinessInvite(database, {
      inviteId: created.id,
      actorId: "admin@example.com",
      reason: "recipient changed",
    });
    await expect(getBusinessInvitePreview(database, tokenHash(token))).resolves.toBeNull();
    await expect(getBusinessInviteToken(database, created.id))
      .rejects.toBeInstanceOf(BusinessInvitationNotFoundError);
    const delivery = await database.pool.query(`
      SELECT status, last_error FROM email_outbox WHERE business_invite_id = $1
    `, [created.id]);
    expect(delivery.rows).toEqual([{
      status: "canceled",
      last_error: "business invitation revoked",
    }]);
  });

  it("rotates an emailed invite idempotently and cancels the old delivery", async () => {
    const created = await createBusinessInvite(database, {
      email: "buyer@example.com",
      tokenHash: tokenHash("e".repeat(43)),
      encryptedToken: "old-encrypted-token",
      multiplierBp: 2500,
      expiresAt: new Date(Date.now() + 86_400_000),
      idempotencyKey: randomUUID(),
      actorId: "admin@example.com",
      reason: "initial terms",
    });
    const idempotencyKey = randomUUID();
    const input = {
      inviteId: created.id,
      tokenHash: tokenHash("f".repeat(43)),
      encryptedToken: "new-encrypted-token",
      expiresAt: new Date(Date.now() + 172_800_000),
      idempotencyKey,
      actorId: "admin@example.com",
      reason: "recipient requested another email",
    };
    const rotated = await rotateBusinessInvite(database, input);
    const replay = await rotateBusinessInvite(database, {
      ...input,
      tokenHash: tokenHash("g".repeat(43)),
      encryptedToken: "wrong-token-on-retry",
    });

    expect(replay).toMatchObject({
      id: rotated.id,
      encryptedToken: "new-encrypted-token",
      idempotentReplay: true,
    });
    const invitations = await database.pool.query(`
      SELECT id, revoked_at IS NOT NULL AS revoked, superseded_by_invite_id
      FROM business_invites ORDER BY CASE WHEN id = $1 THEN 0 ELSE 1 END
    `, [created.id]);
    expect(invitations.rows).toEqual([
      { id: created.id, revoked: true, superseded_by_invite_id: rotated.id },
      { id: rotated.id, revoked: false, superseded_by_invite_id: null },
    ]);
    const deliveries = await database.pool.query(`
      SELECT business_invite_id, status FROM email_outbox
      ORDER BY CASE WHEN business_invite_id = $1 THEN 0 ELSE 1 END
    `, [created.id]);
    expect(deliveries.rows).toEqual([
      { business_invite_id: created.id, status: "canceled" },
      { business_invite_id: rotated.id, status: "pending" },
    ]);
  });
});

function tokenHash(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}
