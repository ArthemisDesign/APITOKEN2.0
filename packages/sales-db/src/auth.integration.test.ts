import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createHash, randomBytes } from "node:crypto";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { createPartnerSession, resolvePartnerSession, revokePartnerSession } from "./auth.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

function hashOf(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

describe.runIf(Boolean(connectionString))("partner session resolution", () => {
  let db: SalesDatabase;
  beforeAll(async () => { db = createSalesDatabase(connectionString!); await db.pool.query("SELECT 1"); });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, partner_sessions RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, partner_sessions RESTART IDENTITY CASCADE");
  });

  async function partnerWithSession(status = "active", b2b = false): Promise<{ partnerId: string; sessionId: string; tokenHash: string }> {
    const partner = await db.pool.query<{ id: string }>(
      "INSERT INTO partners (referral_code, status, telegram_username, b2b_enabled, b2b_max_discount_bps) VALUES ($1,$2,$1,$3,$4) RETURNING id",
      [randomBytes(4).toString("hex"), status, b2b, b2b ? 4_000 : 0],
    );
    const tokenHash = hashOf(randomBytes(32).toString("base64url"));
    const sessionId = await createPartnerSession(db, {
      partnerId: partner.rows[0]!.id,
      tokenHash,
      expiresAt: new Date(Date.now() + 3_600_000),
      userAgent: null,
      ipAddress: null,
    });
    return { partnerId: partner.rows[0]!.id, sessionId, tokenHash };
  }

  async function lastSeenAt(sessionId: string): Promise<Date> {
    const result = await db.pool.query<{ last_seen_at: Date }>(
      "SELECT last_seen_at FROM partner_sessions WHERE id = $1", [sessionId]);
    return result.rows[0]!.last_seen_at;
  }

  it("resolves an active session and rejects revoked, expired and suspended ones", async () => {
    const { partnerId, sessionId, tokenHash } = await partnerWithSession();
    expect((await resolvePartnerSession(db, tokenHash))?.sessionId).toBe(sessionId);
    await revokePartnerSession(db, sessionId, partnerId);
    expect(await resolvePartnerSession(db, tokenHash)).toBeNull();

    const expired = await partnerWithSession();
    await db.pool.query("UPDATE partner_sessions SET expires_at = now() - interval '1 second' WHERE id = $1", [expired.sessionId]);
    expect(await resolvePartnerSession(db, expired.tokenHash)).toBeNull();

    const suspended = await partnerWithSession("suspended");
    expect(await resolvePartnerSession(db, suspended.tokenHash)).toBeNull();
  });

  it("preserves the B2B grant in the session partner payload", async () => {
    const granted = await partnerWithSession("active", true);
    const resolved = await resolvePartnerSession(db, granted.tokenHash);
    expect(resolved?.partner.b2bEnabled).toBe(true);
    expect(resolved?.partner.b2bMaxDiscountBps).toBe(4_000);
  });

  it("writes last_seen_at at most once per interval, not on every request", async () => {
    const { sessionId, tokenHash } = await partnerWithSession();
    await db.pool.query("UPDATE partner_sessions SET last_seen_at = now() - interval '2 hours' WHERE id = $1", [sessionId]);
    await resolvePartnerSession(db, tokenHash);
    const bumped = await lastSeenAt(sessionId);
    expect(Date.now() - bumped.getTime()).toBeLessThan(60_000);
    // A repeat resolution inside the interval must not issue the write at all.
    await resolvePartnerSession(db, tokenHash);
    expect((await lastSeenAt(sessionId)).getTime()).toBe(bumped.getTime());
    // An explicit zero interval restores a write on every resolution.
    await resolvePartnerSession(db, tokenHash, 0);
    expect((await lastSeenAt(sessionId)).getTime()).toBeGreaterThan(bumped.getTime());
  });
});
