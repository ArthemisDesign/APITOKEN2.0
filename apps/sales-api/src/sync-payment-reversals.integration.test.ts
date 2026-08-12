import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createSalesDatabase,
  recordPaidFundingLot,
  type SalesDatabase,
} from "@claude-api/sales-db";
import { SyncService } from "./sync.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const golden = JSON.parse(readFileSync(
  new URL("../../../tests/contracts/sales-payment-reversals-feed.golden.json", import.meta.url),
  "utf8",
)) as {
  row: {
    id: string;
    paymentId: string;
    userId: string;
    kind: "refund" | "dispute";
    amountNano: string;
    reversedAt: string;
  };
  nextCursor: string;
};

describe.runIf(Boolean(connectionString))("payment reversal golden sync", () => {
  let database: SalesDatabase;
  let userId: string;
  let partnerId: string;
  let paymentId: string;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-payment-reversal-golden-test");
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
    userId = golden.row.userId;
    paymentId = golden.row.paymentId;
    const partner = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES($1, 'active', 2300, 1000)
      RETURNING id
    `, [`golden-${randomUUID()}`]);
    partnerId = partner.rows[0]!.id;
    await database.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, attributed_at)
      VALUES($1, $2, '2026-08-01T00:00:00.000Z')
    `, [userId, partnerId]);
    await database.pool.query(`
      INSERT INTO referred_topups(
        commerce_payment_id, commerce_user_id, partner_id, amount_nano, paid_at
      ) VALUES($1, $2, $3, $4, '2026-08-02T00:00:00.000Z')
    `, [paymentId, userId, partnerId, golden.row.amountNano]);
    await recordPaidFundingLot(database, {
      commerceTopupId: 12n,
      commercePaymentId: paymentId,
      commerceUserId: userId,
      amountNano: BigInt(golden.row.amountNano),
      paidAt: new Date("2026-08-02T00:00:00.000Z"),
    });
  });

  afterEach(() => vi.unstubAllGlobals());

  async function truncate(): Promise<void> {
    await database?.pool.query(`
      TRUNCATE sync_cursors, partner_commission_adjustments,
        partner_commission_funding_allocations, partner_payment_reversals,
        partner_usage_funding_allocations, partner_paid_funding_lots,
        commission_entries_v2, partner_usage_events_v2,
        commission_entries, partner_usage_events,
        referred_topups, referred_users, partners
      RESTART IDENTITY CASCADE
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
    vi.stubGlobal("fetch", vi.fn(async (request: Parameters<typeof fetch>[0]) => {
      const url = request instanceof URL ? request : new URL(String(request));
      if (url.pathname.endsWith("/payment-reversals")) {
        return new Response(JSON.stringify({
          items: [golden.row],
          nextCursor: golden.nextCursor,
          sourceHead: golden.nextCursor,
        }), { status: 200 });
      }
      return new Response(JSON.stringify({ items: [], nextCursor: "0", sourceHead: "0" }), { status: 200 });
    }));
  }

  it("consumes the producer golden exactly and crash-replays cursor plus evidence", async () => {
    serveGolden();
    const sync = service();
    await (sync as never as { syncPaymentReversals(): Promise<void> }).syncPaymentReversals();

    const first = await database.pool.query<{
      reversals: string;
      cursor: string;
    }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals') AS cursor
    `);
    expect(first.rows[0]).toEqual({ reversals: "1", cursor: golden.nextCursor });

    await database.pool.query("UPDATE sync_cursors SET last_id=0 WHERE feed='payment_reversals'");
    await (sync as never as { syncPaymentReversals(): Promise<void> }).syncPaymentReversals();
    const replay = await database.pool.query<{ reversals: string; cursor: string }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals') AS cursor
    `);
    expect(replay.rows[0]).toEqual({ reversals: "1", cursor: golden.nextCursor });
  });

  it("does not advance while either causal feed has not proved its head", async () => {
    vi.stubGlobal("fetch", vi.fn(async (request: Parameters<typeof fetch>[0]) => {
      const url = request instanceof URL ? request : new URL(String(request));
      if (url.pathname.endsWith("/payment-reversals")) {
        return new Response(JSON.stringify({
          items: [golden.row],
          nextCursor: golden.nextCursor,
          sourceHead: golden.nextCursor,
        }), { status: 200 });
      }
      const nextCursor = url.pathname.endsWith("/usage-events") ? "1" : "0";
      return new Response(JSON.stringify({
        items: [],
        nextCursor,
        sourceHead: nextCursor === "1" ? "2" : "0",
      }), { status: 200 });
    }));
    await (service() as never as { syncPaymentReversals(): Promise<void> }).syncPaymentReversals();
    const result = await database.pool.query<{ reversals: string; cursors: string }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT count(*)::text FROM sync_cursors WHERE feed='payment_reversals') AS cursors
    `);
    expect(result.rows[0]).toEqual({ reversals: "0", cursors: "0" });
  });
});
