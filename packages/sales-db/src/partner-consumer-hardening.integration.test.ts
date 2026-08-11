import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  loadCommissionChain,
  PartnerReferralCycleError,
  recordReferredDeposit,
  recordReferredSpend,
  reconcilePendingReferralEvents,
  ReferralEventReplayConflictError,
} from "./commissions.js";
import {
  recordReferredSpendV2,
  reconcilePendingReferralUsageEventsV2,
  ReferredSpendV2ReplayConflictError,
  type ReferredSpendV2Event,
} from "./commissions-v2.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const ATTRIBUTED_AT = new Date("2026-08-01T12:00:00.000Z");
const EVENT_AT = new Date("2026-08-01T12:05:00.000Z");
const BEFORE_ATTRIBUTION = new Date("2026-08-01T11:59:00.000Z");

describe.runIf(Boolean(connectionString))("partner feed consumer hardening", () => {
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
    await db.pool.query(`
      TRUNCATE commission_entries_v2, commission_entries,
        partner_usage_events_v2, partner_usage_events,
        pending_referral_usage_events_v2, pending_referral_events,
        referred_topups, referred_users, partners RESTART IDENTITY CASCADE
    `);
  }

  async function partner(code: string): Promise<string> {
    const result = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code, status, commission_bps, sub_commission_bps)
      VALUES($1, 'active', 1000, 1000)
      RETURNING id
    `, [code]);
    return result.rows[0]!.id;
  }

  async function attribute(userId: string, partnerId: string, attributedAt = ATTRIBUTED_AT): Promise<void> {
    await db.pool.query(`
      INSERT INTO referred_users(commerce_user_id, partner_id, referral_code, attributed_at)
      VALUES($1, $2, 'hardening', $3)
    `, [userId, partnerId, attributedAt]);
  }

  function v2Event(overrides: Partial<ReferredSpendV2Event> = {}): ReferredSpendV2Event {
    return {
      commerceEventId: 91_001n,
      commerceUserId: randomUUID(),
      providerId: "google",
      accountClass: "b2c",
      officialNano: 20_000n,
      chargedNano: 10_000n,
      paidFundedNano: 10_000n,
      bonusFundedNano: 0n,
      otherFundedNano: 0n,
      commissionEligible: true,
      releaseGeneration: 7n,
      releaseDigest: "release-hardening",
      snapshotDigest: "snapshot-hardening",
      occurredAt: EVENT_AT,
      ...overrides,
    };
  }

  async function counts(): Promise<{
    v1: string;
    v2: string;
    pendingV1: string;
    pendingV2: string;
    commissions: string;
  }> {
    const result = await db.pool.query<{
      v1: string;
      v2: string;
      pending_v1: string;
      pending_v2: string;
      commissions: string;
    }>(`
      SELECT (SELECT count(*) FROM partner_usage_events)::text AS v1,
             (SELECT count(*) FROM partner_usage_events_v2)::text AS v2,
             (SELECT count(*) FROM pending_referral_events WHERE kind = 'spend')::text AS pending_v1,
             (SELECT count(*) FROM pending_referral_usage_events_v2)::text AS pending_v2,
             ((SELECT count(*) FROM commission_entries)
               + (SELECT count(*) FROM commission_entries_v2))::text AS commissions
    `);
    const row = result.rows[0]!;
    return {
      v1: row.v1,
      v2: row.v2,
      pendingV1: row.pending_v1,
      pendingV2: row.pending_v2,
      commissions: row.commissions,
    };
  }

  it("treats v1-recorded to v2 and v2-recorded to v1 replays as one event", async () => {
    const direct = await partner("cross-recorded");

    const v1User = randomUUID();
    await attribute(v1User, direct);
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_101n,
      commerceUserId: v1User,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    })).resolves.toBe("recorded");
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_101n,
      commerceUserId: v1User,
    }))).resolves.toBe("duplicate");

    const v2User = randomUUID();
    await attribute(v2User, direct);
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_102n,
      commerceUserId: v2User,
    }))).resolves.toBe("recorded");
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_102n,
      commerceUserId: v2User,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    })).resolves.toBe("duplicate");

    await expect(counts()).resolves.toEqual({
      v1: "1", v2: "1", pendingV1: "0", pendingV2: "0", commissions: "2",
    });
  });

  it("serializes concurrent v1/v2 writers and records exactly one commission", async () => {
    const direct = await partner("cross-concurrent");
    const user = randomUUID();
    await attribute(user, direct);
    const eventId = 91_103n;

    const outcomes = await Promise.all([
      recordReferredSpend(db, {
        commerceEventId: eventId,
        commerceUserId: user,
        amountNano: 10_000n,
        occurredAt: EVENT_AT,
      }),
      recordReferredSpendV2(db, v2Event({ commerceEventId: eventId, commerceUserId: user })),
    ]);

    expect(outcomes.sort()).toEqual(["duplicate", "recorded"]);
    const state = await counts();
    expect(BigInt(state.v1) + BigInt(state.v2)).toBe(1n);
    expect(state.commissions).toBe("1");
  });

  it("keeps one pending owner and replays mixed-schema retries in both directions", async () => {
    const direct = await partner("cross-pending");

    const v1User = randomUUID();
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_201n,
      commerceUserId: v1User,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    })).resolves.toBe("buffered");
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_201n,
      commerceUserId: v1User,
    }))).resolves.toBe("buffered");

    const v2User = randomUUID();
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_202n,
      commerceUserId: v2User,
    }))).resolves.toBe("buffered");
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_202n,
      commerceUserId: v2User,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    })).resolves.toBe("buffered");

    await expect(counts()).resolves.toMatchObject({ pendingV1: "1", pendingV2: "1" });
    await attribute(v1User, direct);
    await attribute(v2User, direct);
    await expect(reconcilePendingReferralEvents(db)).resolves.toBe(1);
    await expect(reconcilePendingReferralUsageEventsV2(db)).resolves.toBe(1);
    await expect(counts()).resolves.toEqual({
      v1: "1", v2: "1", pendingV1: "0", pendingV2: "0", commissions: "2",
    });
  });

  it("fails closed on divergent cross-schema recorded and pending replays", async () => {
    const direct = await partner("cross-conflict");
    const recordedUser = randomUUID();
    await attribute(recordedUser, direct);
    await recordReferredSpend(db, {
      commerceEventId: 91_301n,
      commerceUserId: recordedUser,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    });
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_301n,
      commerceUserId: recordedUser,
      paidFundedNano: 9_999n,
      chargedNano: 9_999n,
    }))).rejects.toBeInstanceOf(ReferredSpendV2ReplayConflictError);

    const pendingUser = randomUUID();
    await recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_302n,
      commerceUserId: pendingUser,
    }));
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_302n,
      commerceUserId: pendingUser,
      amountNano: 10_001n,
      occurredAt: EVENT_AT,
    })).rejects.toBeInstanceOf(ReferralEventReplayConflictError);

    await expect(counts()).resolves.toMatchObject({
      v1: "1", v2: "0", pendingV1: "0", pendingV2: "1", commissions: "1",
    });
  });

  it("rejects pre-attribution spend/topups and drains older buffered events without commission", async () => {
    const direct = await partner("temporal");
    const directUser = randomUUID();
    await attribute(directUser, direct);
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_401n,
      commerceUserId: directUser,
      amountNano: 10_000n,
      occurredAt: BEFORE_ATTRIBUTION,
    })).resolves.toBe("skipped");
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_402n,
      commerceUserId: directUser,
      occurredAt: BEFORE_ATTRIBUTION,
    }))).resolves.toBe("skipped");
    await expect(recordReferredDeposit(db, {
      commercePaymentId: "pay-temporal-direct",
      commerceUserId: directUser,
      amountNano: 10_000n,
      paidAt: BEFORE_ATTRIBUTION,
    })).resolves.toBe("skipped");

    const bufferedUser = randomUUID();
    await expect(recordReferredSpend(db, {
      commerceEventId: 91_403n,
      commerceUserId: bufferedUser,
      amountNano: 10_000n,
      occurredAt: BEFORE_ATTRIBUTION,
    })).resolves.toBe("buffered");
    await expect(recordReferredSpendV2(db, v2Event({
      commerceEventId: 91_404n,
      commerceUserId: bufferedUser,
      occurredAt: BEFORE_ATTRIBUTION,
    }))).resolves.toBe("buffered");
    await expect(recordReferredDeposit(db, {
      commercePaymentId: "pay-temporal-buffered",
      commerceUserId: bufferedUser,
      amountNano: 10_000n,
      paidAt: BEFORE_ATTRIBUTION,
    })).resolves.toBe("buffered");
    await attribute(bufferedUser, direct);

    await expect(reconcilePendingReferralEvents(db)).resolves.toBe(2);
    await expect(reconcilePendingReferralUsageEventsV2(db)).resolves.toBe(1);
    await expect(counts()).resolves.toEqual({
      v1: "0", v2: "0", pendingV1: "0", pendingV2: "0", commissions: "0",
    });
    const topups = await db.pool.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM referred_topups",
    );
    expect(topups.rows[0]?.count).toBe("0");
  });

  it("validates every immutable field of duplicate deposits", async () => {
    const direct = await partner("deposit-replay");
    const user = randomUUID();
    await attribute(user, direct);
    const input = {
      commercePaymentId: "pay-exact-replay",
      commerceUserId: user,
      amountNano: 50_000n,
      paidAt: EVENT_AT,
    };
    await expect(recordReferredDeposit(db, input)).resolves.toBe("recorded");
    await expect(recordReferredDeposit(db, input)).resolves.toBe("duplicate");
    await expect(recordReferredDeposit(db, { ...input, amountNano: 50_001n }))
      .rejects.toBeInstanceOf(ReferralEventReplayConflictError);
    await expect(recordReferredDeposit(db, {
      ...input,
      paidAt: new Date(EVENT_AT.getTime() + 1),
    })).rejects.toBeInstanceOf(ReferralEventReplayConflictError);
    const otherUser = randomUUID();
    await attribute(otherUser, direct);
    await expect(recordReferredDeposit(db, { ...input, commerceUserId: otherUser }))
      .rejects.toBeInstanceOf(ReferralEventReplayConflictError);

    const otherPartner = await partner("deposit-wrong-owner");
    await db.pool.query(
      "UPDATE referred_topups SET partner_id = $2 WHERE commerce_payment_id = $1",
      [input.commercePaymentId, otherPartner],
    );
    await expect(recordReferredDeposit(db, input))
      .rejects.toBeInstanceOf(ReferralEventReplayConflictError);
  });

  it("detects a partner cycle and rolls back all event evidence", async () => {
    const first = await partner("cycle-first");
    const second = await partner("cycle-second");
    await db.pool.query("UPDATE partners SET parent_partner_id = $2 WHERE id = $1", [first, second]);
    await db.pool.query("UPDATE partners SET parent_partner_id = $2 WHERE id = $1", [second, first]);
    const user = randomUUID();
    await attribute(user, first);

    await expect(recordReferredSpend(db, {
      commerceEventId: 91_501n,
      commerceUserId: user,
      amountNano: 10_000n,
      occurredAt: EVENT_AT,
    })).rejects.toBeInstanceOf(PartnerReferralCycleError);
    await expect(counts()).resolves.toEqual({
      v1: "0", v2: "0", pendingV1: "0", pendingV2: "0", commissions: "0",
    });
  });

  it("holds a share lock on every partner used by commission math", async () => {
    const parent = await partner("chain-lock-parent");
    const direct = await partner("chain-lock");
    await db.pool.query("UPDATE partners SET parent_partner_id = $2 WHERE id = $1", [direct, parent]);
    const reader = await db.pool.connect();
    const updater = await db.pool.connect();
    try {
      await reader.query("BEGIN");
      await loadCommissionChain(reader, direct);

      await updater.query("BEGIN");
      await updater.query("SET LOCAL lock_timeout = '100ms'");
      await expect(updater.query(
        "UPDATE partners SET commission_bps = commission_bps + 1 WHERE id = $1",
        [parent],
      )).rejects.toMatchObject({ code: "55P03" });
      await updater.query("ROLLBACK");
      await reader.query("COMMIT");

      await expect(db.pool.query(
        "UPDATE partners SET commission_bps = commission_bps + 1 WHERE id = $1",
        [parent],
      )).resolves.toMatchObject({ rowCount: 1 });
    } finally {
      await updater.query("ROLLBACK").catch(() => undefined);
      await reader.query("ROLLBACK").catch(() => undefined);
      updater.release();
      reader.release();
    }
  });
});
