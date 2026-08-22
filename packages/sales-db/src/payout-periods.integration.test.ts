import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { getDuePayoutList, getPartnerPeriodState } from "./payout-periods.js";
import type { PartnerStatus } from "./auth.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

// Сценарий на фиксированной дате. Периоды (UTC): P1=[1,16), P2=[16, 1-е след.мес).
// now = 24 июля 2026 → текущий = 2026-07-P2; последний завершённый = 2026-07-P1 (кончился 16 июля).
// P1: лок до 23 июля, окно выплат 23–26 июля → на 24 июля окно P1 ОТКРЫТО.
const NOW = new Date("2026-07-24T12:00:00.000Z");
const BEFORE_P1_END = new Date("2026-07-10T00:00:00.000Z"); // заработано внутри P1 (< 16 июля)
const JUL23 = "2026-07-23T00:00:00.000Z"; // windowStart(P1)
const MIN_PAYOUT = 10n;

describe.runIf(Boolean(connectionString))("payout-periods due-list & cabinet correctness", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await db.pool.query("TRUNCATE partners, commission_entries, payouts, referred_users, referred_topups RESTART IDENTITY CASCADE");
    await db.pool.end();
  });
  beforeEach(async () => {
    await db.pool.query("TRUNCATE partners, commission_entries, payouts, referred_users, referred_topups RESTART IDENTITY CASCADE");
  });

  async function makePartner(opts: { code: string; email?: string; status?: PartnerStatus; wallet?: string }): Promise<string> {
    const details = opts.wallet ? JSON.stringify({ address: opts.wallet }) : null;
    const method = opts.wallet ? "usdt-bep20" : null;
    const res = await db.pool.query<{ id: string }>(
      `INSERT INTO partners (referral_code, email, status, payout_method, payout_details, telegram_username)
       VALUES ($1, $2, $3, $4, $5::jsonb, $1) RETURNING id`,
      [opts.code, opts.email ?? `${opts.code}@example.test`, opts.status ?? "active", method, details],
    );
    return res.rows[0]!.id;
  }
  let eventSeq = 0;
  async function earn(partnerId: string, amountNano: bigint, when: Date): Promise<void> {
    // commission_entries требует ровно один источник (usage_event_id XOR topup_id) — заводим usage-event.
    eventSeq += 1;
    const src = await db.pool.query<{ id: string }>(
      `INSERT INTO partner_usage_events (commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at)
       VALUES ($1, $2, $3, $4, $5) RETURNING id`,
      [eventSeq, randomUUID(), partnerId, amountNano.toString(), when],
    );
    await db.pool.query(
      "INSERT INTO commission_entries (usage_event_id, partner_id, level, applied_bps, amount_nano, created_at) VALUES ($1, $2, 0, 1000, $3, $4)",
      [src.rows[0]!.id, partnerId, amountNano.toString(), when],
    );
  }
  async function payout(partnerId: string, amountNano: bigint, status: string): Promise<void> {
    await db.pool.query(
      "INSERT INTO payouts (partner_id, amount_nano, status, method) VALUES ($1, $2, $3::payout_status, 'usdt-bep20')",
      [partnerId, amountNano.toString(), status],
    );
  }
  const WALLET = "0x1111111111111111111111111111111111111111";

  it("SEV1: does not re-list a partner whose payout is already in-flight (requested/approved)", async () => {
    const inflight = await makePartner({ code: "inflight1", wallet: WALLET });
    await earn(inflight, 100n, BEFORE_P1_END);
    await payout(inflight, 100n, "requested"); // committed but not yet 'paid'

    const { items } = await getDuePayoutList(db, NOW, MIN_PAYOUT);
    const row = items.find((i) => i.partnerId === inflight);
    // committed (100) fully covers earned (100) → nothing left to pay, must be absent.
    expect(row).toBeUndefined();
  });

  it("SEV1: approved payouts are also treated as committed", async () => {
    const p = await makePartner({ code: "approved1", wallet: WALLET });
    await earn(p, 100n, BEFORE_P1_END);
    await payout(p, 60n, "approved");
    const { items } = await getDuePayoutList(db, NOW, MIN_PAYOUT);
    const row = items.find((i) => i.partnerId === p);
    expect(row?.payableNano).toBe("40"); // 100 earned − 60 committed
  });

  it("rejected payouts do NOT reduce payable (money still owed)", async () => {
    const p = await makePartner({ code: "rejected1", wallet: WALLET });
    await earn(p, 100n, BEFORE_P1_END);
    await payout(p, 100n, "rejected");
    const { items } = await getDuePayoutList(db, NOW, MIN_PAYOUT);
    const row = items.find((i) => i.partnerId === p);
    expect(row?.payableNano).toBe("100");
    expect(row?.eligible).toBe(true);
  });

  it("SEV2: a suspended partner with an unpaid balance stays visible but is not eligible", async () => {
    const p = await makePartner({ code: "suspended1", status: "suspended", wallet: WALLET });
    await earn(p, 50n, BEFORE_P1_END);
    const { items } = await getDuePayoutList(db, NOW, MIN_PAYOUT);
    const row = items.find((i) => i.partnerId === p);
    expect(row).toBeDefined();
    expect(row?.payableNano).toBe("50");
    expect(row?.reason).toBe("inactive");
    expect(row?.eligible).toBe(false);
  });

  it("SEV3: cabinet dates the payout to the OPEN window (now), not a future period", async () => {
    const p = await makePartner({ code: "cabinet1", wallet: WALLET });
    await earn(p, 100n, BEFORE_P1_END);
    const state = await getPartnerPeriodState(db, p, NOW);
    expect(new Date(state.nextPayout.date).toISOString()).toBe(JUL23);
    expect(state.nextPayout.estimatedNano).toBe("100");
    expect(state.unpaidNano).toBe("100");
  });

  it("SEV3: cabinet unpaid excludes in-flight payouts (no phantom double-count)", async () => {
    const p = await makePartner({ code: "cabinet2", wallet: WALLET });
    await earn(p, 100n, BEFORE_P1_END);
    await payout(p, 100n, "requested");
    const state = await getPartnerPeriodState(db, p, NOW);
    expect(state.unpaidNano).toBe("0");
    expect(state.nextPayout.estimatedNano).toBe("0");
    expect(state.lifetimePaidNano).toBe("0"); // requested ≠ paid, so lifetimePaid stays 0
  });

  it("eligible active partner with a wallet and balance is ready to pay", async () => {
    const p = await makePartner({ code: "ready1", email: "ready@example.test", wallet: WALLET });
    await earn(p, 100n, BEFORE_P1_END);
    const { items } = await getDuePayoutList(db, NOW, MIN_PAYOUT);
    const row = items.find((i) => i.partnerId === p);
    expect(row?.email).toBe("ready@example.test");
    expect(row?.eligible).toBe(true);
    expect(row?.reason).toBe("ok");
  });
});
