import { randomUUID } from "node:crypto";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "@claude-api/sales-db";
import { SyncService } from "./sync.service.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("commerce feed to reversal ledger E2E", () => {
  let database: SalesDatabase;
  let userId: string;
  let partnerId: string;
  let paymentId: string;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!, "sales-payment-reversal-e2e-test");
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
    userId = randomUUID();
    paymentId = randomUUID();
    const partner = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES($1, 'active', 2300, 1000)
      RETURNING id
    `, [`e2e-${randomUUID()}`]);
    partnerId = partner.rows[0]!.id;
    await database.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, attributed_at)
      VALUES($1, $2, '2026-08-01T00:00:00.000Z')
    `, [userId, partnerId]);
  });

  afterEach(() => vi.unstubAllGlobals());

  async function truncate(): Promise<void> {
    await database?.pool.query(`
      TRUNCATE sync_cursors, partner_commission_adjustments,
        partner_commission_funding_allocations, partner_payment_reversals,
        partner_usage_funding_allocations, partner_paid_funding_lots,
        commission_entries_v2, partner_usage_events_v2,
        commission_entries, partner_usage_events,
        pending_referral_usage_events_v2, pending_referral_events,
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

  function installFeed(): void {
    const topup = {
      id: "10",
      paymentId,
      userId,
      amountNano: "600",
      paidAt: "2026-08-02T00:00:00.000Z",
    };
    const usage = {
      id: "20",
      userId,
      amountNano: "600",
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
      occurredAt: "2026-08-03T00:00:00.000Z",
    };
    const reversal = {
      id: "30",
      paymentId,
      userId,
      kind: "refund",
      amountNano: "600",
      reversedAt: "2026-08-04T00:00:00.000Z",
    };
    vi.stubGlobal("fetch", vi.fn(async (request: Parameters<typeof fetch>[0]) => {
      const url = request instanceof URL ? request : new URL(String(request));
      const after = BigInt(url.searchParams.get("after_id") ?? "0");
      if (url.pathname.endsWith("/attributions")) {
        return new Response(JSON.stringify({ items: [], nextCursor: after.toString() }));
      }
      if (url.pathname.endsWith("/topups-v2")) {
        return new Response(JSON.stringify({
          items: after < 10n ? [topup] : [],
          nextCursor: after < 10n ? "10" : after.toString(),
        }));
      }
      if (url.pathname.endsWith("/usage-events")) {
        return new Response(JSON.stringify({
          items: after < 20n ? [usage] : [],
          nextCursor: after < 20n ? "20" : after.toString(),
        }));
      }
      if (url.pathname.endsWith("/payment-reversals")) {
        return new Response(JSON.stringify({
          items: after < 30n ? [reversal] : [],
          nextCursor: after < 30n ? "30" : after.toString(),
        }));
      }
      throw new Error(`unexpected feed URL ${url}`);
    }));
  }

  it("drains causality before reversal and produces one exact negative ledger row", async () => {
    installFeed();
    const sync = service();

    // The reversal page is fetched only after the first topup/usage/funding pass; its own post-page
    // probes confirm both causal feeds are still at head before the atomic writer commits.
    await sync.syncOnce();
    const intermediate = await database.pool.query<{ reversals: string; adjustments: string }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT count(*)::text FROM partner_commission_adjustments) AS adjustments
    `);
    expect(intermediate.rows[0]).toEqual({ reversals: "1", adjustments: "1" });

    const final = await database.pool.query<{
      gross: string;
      adjustments: string;
      reversals: string;
      cursor: string;
    }>(`
      SELECT
        (SELECT sum(amount_nano)::text FROM commission_entries) AS gross,
        (SELECT sum(amount_nano)::text FROM partner_commission_adjustments) AS adjustments,
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT last_id::text FROM sync_cursors WHERE feed='payment_reversals') AS cursor
    `);
    expect(final.rows[0]).toEqual({
      gross: "138",
      adjustments: "-138",
      reversals: "1",
      cursor: "30",
    });

    await sync.syncOnce();
    const replay = await database.pool.query<{ reversals: string; adjustments: string }>(`
      SELECT
        (SELECT count(*)::text FROM partner_payment_reversals) AS reversals,
        (SELECT count(*)::text FROM partner_commission_adjustments) AS adjustments
    `);
    expect(replay.rows[0]).toEqual({ reversals: "1", adjustments: "1" });
  });
});
