import { readFileSync } from "node:fs";
import {
  createSalesDatabase,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SyncService } from "./sync.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const golden = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-topups-v2-feed.golden.json", import.meta.url),
  "utf8",
)) as {
  row: {
    id: string;
    paymentId: string;
    userId: string;
    amountNano: string;
    paidAt: string;
  };
  nextCursor: string;
};

describe.runIf(Boolean(connectionString))("topups-v2 sync replay", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-topups-v2-sync-test");
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
    const partner = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES ('topups-v2-integration', 'active', 1000, 1000)
      RETURNING id
    `);
    await database.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, referral_code, attributed_at)
      VALUES ($1, $2, 'topups-v2-integration', '2026-08-01T00:00:00.000Z')
    `, [golden.row.userId, partner.rows[0]!.id]);
    await database.pool.query(
      "INSERT INTO sync_cursors(feed, last_id) VALUES ('topups', 1786438638715458)",
    );
  });

  afterEach(() => vi.unstubAllGlobals());

  async function truncate(): Promise<void> {
    await database?.pool.query(`
      TRUNCATE sync_cursors, pending_referral_events, referred_topups,
        referred_users, partners RESTART IDENTITY CASCADE
    `);
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

  function serveGolden(): void {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      items: [golden.row],
      nextCursor: golden.nextCursor,
    }), { status: 200 })));
  }

  it("replays sequence zero idempotently and never mutates the legacy timestamp cursor", async () => {
    serveGolden();
    const sync = service();
    await (sync as never as { syncTopups(): Promise<void> }).syncTopups();

    const first = await database.pool.query<{
      row_count: string;
      amount_sum: string;
      topups_v2: string;
      legacy_topups: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM referred_topups) AS row_count,
        (SELECT COALESCE(sum(amount_nano), 0)::text FROM referred_topups) AS amount_sum,
        (SELECT last_id::text FROM sync_cursors WHERE feed='topups_v2') AS topups_v2,
        (SELECT last_id::text FROM sync_cursors WHERE feed='topups') AS legacy_topups
    `);
    expect(first.rows[0]).toEqual({
      row_count: "1",
      amount_sum: golden.row.amountNano,
      topups_v2: golden.nextCursor,
      legacy_topups: "1786438638715458",
    });

    // Simulate a crash after the idempotent deposit commit but before cursor persistence.
    await database.pool.query("UPDATE sync_cursors SET last_id=0 WHERE feed='topups_v2'");
    await (sync as never as { syncTopups(): Promise<void> }).syncTopups();

    const replay = await database.pool.query<{ row_count: string; cursor: string }>(`
      SELECT
        (SELECT count(*)::text FROM referred_topups) AS row_count,
        (SELECT last_id::text FROM sync_cursors WHERE feed='topups_v2') AS cursor
    `);
    expect(replay.rows[0]).toEqual({ row_count: "1", cursor: golden.nextCursor });
  });
});
