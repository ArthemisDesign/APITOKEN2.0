import { randomUUID } from "node:crypto";
import { createSalesDatabase, type SalesDatabase } from "@claude-api/sales-db";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SyncService } from "./sync.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const PAGE = 1_000;

/**
 * Money integrity of the referral path against real SQL: every billable event becomes exactly one
 * commission, a replayed page pays nothing twice, a backlog is drained inside one tick instead of
 * one page per interval, and nothing is written for a user nobody referred.
 */
describe.runIf(Boolean(connectionString))("referral money integrity under load", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-financial-integrity-test");
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
      TRUNCATE sync_cursors, commission_entries, partner_usage_events, pending_referral_events,
        referred_users, partners RESTART IDENTITY CASCADE
    `);
  }

  async function referredUser(commissionBps = 1_000): Promise<{ partnerId: string; userId: string }> {
    const userId = randomUUID();
    const code = `p_${randomUUID().replace(/-/g, "").slice(0, 24)}`;
    const partner = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps, team_override_max_bps,
        commerce_user_id, program_enabled, program_started_at)
      VALUES ($1, 'active', $2, $2, 2000, $3, true, now() - interval '30 days')
      RETURNING id
    `, [code, commissionBps, randomUUID()]);
    const partnerId = partner.rows[0]!.id;
    await database.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, referral_code, attributed_at)
      VALUES ($1, $2, $3, now() - interval '20 days')
    `, [userId, partnerId, code]);
    return { partnerId, userId };
  }

  /** The live scalar wire shape: provider set, attribution absent (tests/contracts golden). */
  function usageRow(id: number, userId: string, amountNano: bigint) {
    return {
      id: String(id),
      userId,
      amountNano: amountNano.toString(),
      providerId: "anthropic",
      accountClass: null,
      pricingMode: null,
      paidFundedNano: null,
      commissionEligible: null,
      snapshotDigest: null,
      officialNano: null,
      chargedNano: null,
      bonusFundedNano: null,
      otherFundedNano: null,
      releaseGeneration: null,
      releaseDigest: null,
      occurredAt: new Date().toISOString(),
    };
  }

  /** Serves usage pages of at most PAGE rows and empty pages for the other feeds. */
  function serveUsage(rows: ReturnType<typeof usageRow>[]): void {
    const head = rows.length === 0 ? "0" : rows[rows.length - 1]!.id;
    vi.stubGlobal("fetch", vi.fn(async (input: URL | string) => {
      const url = new URL(String(input));
      const after = BigInt(url.searchParams.get("after_id") ?? "0");
      if (url.pathname.endsWith("/usage-events")) {
        const page = rows.filter((row) => BigInt(row.id) > after).slice(0, PAGE);
        const nextCursor = page.length === 0 ? after.toString() : page[page.length - 1]!.id;
        return new Response(JSON.stringify({ items: page, nextCursor, sourceHead: head }), { status: 200 });
      }
      const empty = { items: [], nextCursor: after.toString(), sourceHead: after.toString() };
      return new Response(JSON.stringify(empty), { status: 200 });
    }));
  }

  function service(): SyncService {
    const config = {
      get: (key: string) => ({
        COMMERCE_BASE_URL: "http://127.0.0.1:8791",
        SALES_CONTROL_KEY: "test-key",
        SYNC_INTERVAL_MS: 60_000,
        SYNC_MAX_CATCHUP_PASSES: 50,
      })[key],
    };
    return new SyncService(database, config as never);
  }

  async function commissionTotals(): Promise<{ rows: number; total: string }> {
    const result = await database.pool.query<{ rows: string; total: string }>(`
      SELECT count(*)::text AS rows, COALESCE(sum(amount_nano), 0)::text AS total FROM commission_entries
    `);
    return { rows: Number(result.rows[0]!.rows) as unknown as number, total: result.rows[0]!.total };
  }

  it("turns every billable event into exactly one commission and loses nothing on replay", async () => {
    const { userId } = await referredUser();
    const rows = Array.from({ length: 250 }, (_, index) => usageRow(index + 1, userId, 1_000_000_000n));
    serveUsage(rows);
    const sync = service();

    await sync.syncOnce();
    const first = await commissionTotals();
    // 250 × $1 of real spend at 10% = $25 in nanoUSD.
    expect(first).toEqual({ rows: 250, total: "25000000000" });

    // Crash between the commission write and the cursor advance: the page is served again.
    await database.pool.query("UPDATE sync_cursors SET last_id = 0 WHERE feed = 'usage_events'");
    await sync.syncOnce();
    expect(await commissionTotals()).toEqual(first);
  });

  it("drains a multi-page backlog inside one tick instead of one page per interval", async () => {
    const { userId } = await referredUser();
    const rows = Array.from({ length: PAGE * 2 + 137 }, (_, index) => usageRow(index + 1, userId, 1_000_000n));
    serveUsage(rows);
    const sync = service();

    // The service loop keeps pulling while a tick reports it is still behind.
    const started = Date.now();
    let passes = 0;
    let caughtUp = false;
    while (!caughtUp && passes < 50) {
      caughtUp = await sync.syncOnce();
      passes += 1;
    }
    const elapsedMs = Date.now() - started;

    expect(caughtUp).toBe(true);
    expect(passes).toBe(3);
    const totals = await commissionTotals();
    expect(totals.rows).toBe(rows.length);
    expect(totals.total).toBe((BigInt(rows.length) * 100_000n).toString());
    process.stdout.write(`referral money throughput: ${rows.length} events in ${elapsedMs}ms (${Math.round(rows.length / (elapsedMs / 1000))}/s)\n`);
  }, 20_000);

  it("writes no commission for a user nobody referred", async () => {
    serveUsage([usageRow(1, randomUUID(), 5_000_000_000n)]);

    await service().syncOnce();

    expect(await commissionTotals()).toEqual({ rows: 0, total: "0" });
  });

  it("pays each partner from their own referral only", async () => {
    const first = await referredUser(1_000);
    const second = await referredUser(2_000);
    serveUsage([
      usageRow(1, first.userId, 10_000_000_000n),
      usageRow(2, second.userId, 10_000_000_000n),
    ]);

    await service().syncOnce();

    const perPartner = await database.pool.query<{ partner_id: string; total: string }>(`
      SELECT partner_id, sum(amount_nano)::text AS total FROM commission_entries GROUP BY partner_id
    `);
    const byPartner = new Map(perPartner.rows.map((row) => [row.partner_id, row.total]));
    expect(byPartner.get(first.partnerId)).toBe("1000000000");
    expect(byPartner.get(second.partnerId)).toBe("2000000000");
  });
});
