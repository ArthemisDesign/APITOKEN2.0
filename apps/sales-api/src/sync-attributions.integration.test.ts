import { randomUUID } from "node:crypto";
import { createSalesDatabase, type SalesDatabase } from "@claude-api/sales-db";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SyncService } from "./sync.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

/**
 * The referral link, end to end on the Sales side: Commerce records `?ref=CODE` as an attribution
 * row, this feed turns it into a referred user owned by exactly one partner, and everything that
 * follows — earnings, the Dashboard list, payouts — hangs off that row.
 */
describe.runIf(Boolean(connectionString))("referral attribution sync", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-attribution-sync-test");
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
  });

  afterEach(() => vi.unstubAllGlobals());

  async function truncate(): Promise<void> {
    await database?.pool.query(`
      TRUNCATE sync_cursors, pending_referral_events, referred_users, partners RESTART IDENTITY CASCADE
    `);
  }

  async function partner(code: string, status: "active" | "suspended" = "active"): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES ($1, $2, 1000, 1000)
      RETURNING id
    `, [code, status]);
    return result.rows[0]!.id;
  }

  function service(): SyncService {
    const config = {
      get: (key: string) => ({
        COMMERCE_BASE_URL: "http://127.0.0.1:8791",
        SALES_CONTROL_KEY: "test-key",
        SYNC_INTERVAL_MS: 60_000,
      })[key],
    };
    return new SyncService(database, config as never);
  }

  function serveAttributions(items: Array<{ id: string; userId: string; code: string; createdAt: string }>): void {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      JSON.stringify({ items }),
      { status: 200, headers: { "content-type": "application/json" } },
    )));
  }

  async function sync(): Promise<void> {
    await (service() as never as { syncAttributions(): Promise<void> }).syncAttributions();
  }

  async function referral(commerceUserId: string): Promise<{ partner_id: string; referral_code: string } | undefined> {
    const result = await database.pool.query<{ partner_id: string; referral_code: string }>(
      "SELECT partner_id, referral_code FROM referred_users WHERE commerce_user_id = $1",
      [commerceUserId],
    );
    return result.rows[0];
  }

  async function cursor(): Promise<string | undefined> {
    const result = await database.pool.query<{ last_id: string }>(
      "SELECT last_id::text FROM sync_cursors WHERE feed = 'attributions'",
    );
    return result.rows[0]?.last_id;
  }

  it("binds a user who arrived through a partner link to that partner", async () => {
    const code = "p_1a2b3c4d5e6f7a8b9c0d1e2f";
    const partnerId = await partner(code);
    const userId = randomUUID();
    serveAttributions([{ id: "10", userId, code, createdAt: "2026-08-23T09:00:00.000Z" }]);

    await sync();

    expect(await referral(userId)).toEqual({ partner_id: partnerId, referral_code: code });
    expect(await cursor()).toBe("10");
  });

  it("replays the same page without duplicating the referral", async () => {
    const code = "p_replay0000000000000000";
    const partnerId = await partner(code);
    const userId = randomUUID();
    serveAttributions([{ id: "11", userId, code, createdAt: "2026-08-23T09:00:00.000Z" }]);

    await sync();
    await database.pool.query("UPDATE sync_cursors SET last_id = 0 WHERE feed = 'attributions'");
    await sync();

    const count = await database.pool.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM referred_users WHERE commerce_user_id = $1",
      [userId],
    );
    expect(count.rows[0]).toEqual({ count: "1" });
    expect(await referral(userId)).toEqual({ partner_id: partnerId, referral_code: code });
  });

  it("keeps the first owner when a later link from another partner arrives", async () => {
    const firstCode = "p_first000000000000000000";
    const secondCode = "p_second00000000000000000";
    const firstPartner = await partner(firstCode);
    await partner(secondCode);
    const userId = randomUUID();

    serveAttributions([{ id: "20", userId, code: firstCode, createdAt: "2026-08-23T09:00:00.000Z" }]);
    await sync();
    serveAttributions([{ id: "21", userId, code: secondCode, createdAt: "2026-08-23T10:00:00.000Z" }]);
    await sync();

    expect(await referral(userId)).toEqual({ partner_id: firstPartner, referral_code: firstCode });
    expect(await cursor()).toBe("21");
  });

  it("passes over an unknown code without stalling the feed", async () => {
    const userId = randomUUID();
    serveAttributions([{ id: "30", userId, code: "p_unknown000000000000000", createdAt: "2026-08-23T09:00:00.000Z" }]);

    await sync();

    expect(await referral(userId)).toBeUndefined();
    expect(await cursor()).toBe("30");
  });

  it("does not attribute to a suspended partner", async () => {
    const code = "p_suspended00000000000000";
    await partner(code, "suspended");
    const userId = randomUUID();
    serveAttributions([{ id: "40", userId, code, createdAt: "2026-08-23T09:00:00.000Z" }]);

    await sync();

    expect(await referral(userId)).toBeUndefined();
    expect(await cursor()).toBe("40");
  });
});
