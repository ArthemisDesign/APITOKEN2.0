import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  createPromoCode,
  PromoAlreadyRedeemedError,
  PromoCodeCollisionError,
  PromoLimitError,
  PromoNotAllowedError,
  redeemPromoCode,
  UserAlreadyRedeemedError,
} from "./promo.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;
const USD = 1_000_000_000n;

describe.runIf(Boolean(connectionString))("promo creation and atomic redemption", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await db.pool.query(
      "TRUNCATE sales_audit_log, promo_codes, partners RESTART IDENTITY CASCADE",
    );
    await db.pool.end();
  });

  beforeEach(async () => {
    await db.pool.query(
      "TRUNCATE sales_audit_log, promo_codes, partners RESTART IDENTITY CASCADE",
    );
  });

  async function partner(
    code: string,
    options: {
      status?: "active" | "suspended" | "pending";
      promoEnabled?: boolean;
      maxValueNano?: bigint;
      maxCount?: number;
      discountEnabled?: boolean;
      discountBps?: number;
    } = {},
  ): Promise<string> {
    const result = await db.pool.query<{ id: string }>(
      `INSERT INTO partners (
         referral_code, status, telegram_username, promo_enabled, promo_max_value_nano,
         promo_max_count, referral_discount_enabled, referral_discount_bps
       ) VALUES ($1,$2,$1,$3,$4,$5,$6,$7)
       RETURNING id`,
      [
        code,
        options.status ?? "active",
        options.promoEnabled ?? true,
        (options.maxValueNano ?? 10n * USD).toString(),
        options.maxCount ?? 10,
        options.discountEnabled ?? true,
        options.discountBps ?? 4_000,
      ],
    );
    return result.rows[0]!.id;
  }

  it("redeems case-insensitively and replays the same credit reference after partner freeze", async () => {
    const partnerId = await partner("mixed-partner");
    const promo = await createPromoCode(db, {
      partnerId,
      code: "MiXeD-2026",
      valueNano: 5n * USD,
      discountBps: 3_000,
    });
    const commerceUserId = randomUUID();

    const first = await redeemPromoCode(db, {
      code: "mixed-2026",
      commerceUserId,
    });
    expect(first).toEqual({
      valueNano: 5n * USD,
      partnerId,
      referralCode: "mixed-partner",
      redemptionRef: `promo:${promo.id}`,
      discountBps: 3_000,
      alreadyRedeemed: false,
    });

    // A completed redemption remains replayable even if an administrator freezes the partner
    // before commerce retries its engine credit.
    await db.pool.query(
      "UPDATE partners SET status = 'suspended', promo_enabled = false WHERE id = $1",
      [partnerId],
    );
    const replay = await redeemPromoCode(db, {
      code: "MIXED-2026",
      commerceUserId,
    });
    expect(replay).toEqual({ ...first, alreadyRedeemed: true });

    const stored = await db.pool.query<{
      status: string;
      redeemed_by_commerce_user_id: string;
      redemption_ref: string;
    }>(
      `SELECT status, redeemed_by_commerce_user_id, redemption_ref
       FROM promo_codes WHERE id = $1`,
      [promo.id],
    );
    expect(stored.rows[0]).toEqual({
      status: "redeemed",
      redeemed_by_commerce_user_id: commerceUserId,
      redemption_ref: first.redemptionRef,
    });
    const audit = await db.pool.query<{ count: string }>(
      `SELECT count(*)::text AS count FROM sales_audit_log
       WHERE action = 'promo.redeemed' AND target_id = $1`,
      [promo.id],
    );
    expect(audit.rows[0]!.count).toBe("1");
  });

  it("never lets a second user consume an already redeemed code", async () => {
    const partnerId = await partner("single-code");
    await createPromoCode(db, {
      partnerId,
      code: "SINGLE-CODE",
      valueNano: USD,
    });
    await redeemPromoCode(db, {
      code: "SINGLE-CODE",
      commerceUserId: randomUUID(),
    });

    await expect(
      redeemPromoCode(db, {
        code: "single-code",
        commerceUserId: randomUUID(),
      }),
    ).rejects.toBeInstanceOf(PromoAlreadyRedeemedError);
  });

  it("blocks new redemption immediately when promo permission or partner status is frozen", async () => {
    const partnerId = await partner("freeze");
    const disabled = await createPromoCode(db, {
      partnerId,
      code: "FROZEN-BY-PERMISSION",
      valueNano: USD,
    });
    const suspended = await createPromoCode(db, {
      partnerId,
      code: "FROZEN-BY-STATUS",
      valueNano: USD,
    });

    await db.pool.query("UPDATE partners SET promo_enabled = false WHERE id = $1", [
      partnerId,
    ]);
    await expect(
      redeemPromoCode(db, {
        code: disabled.code,
        commerceUserId: randomUUID(),
      }),
    ).rejects.toBeInstanceOf(PromoAlreadyRedeemedError);

    await db.pool.query(
      "UPDATE partners SET promo_enabled = true, status = 'suspended' WHERE id = $1",
      [partnerId],
    );
    await expect(
      redeemPromoCode(db, {
        code: suspended.code,
        commerceUserId: randomUUID(),
      }),
    ).rejects.toBeInstanceOf(PromoAlreadyRedeemedError);

    const rows = await db.pool.query<{ status: string }>(
      "SELECT status FROM promo_codes WHERE partner_id = $1 ORDER BY code",
      [partnerId],
    );
    expect(rows.rows.map((row) => row.status)).toEqual(["active", "active"]);
  });

  it("allows one promo per commerce user across different codes", async () => {
    const partnerId = await partner("one-per-user");
    await createPromoCode(db, {
      partnerId,
      code: "FIRST-PROMO",
      valueNano: USD,
    });
    await createPromoCode(db, {
      partnerId,
      code: "SECOND-PROMO",
      valueNano: USD,
    });
    const commerceUserId = randomUUID();
    await redeemPromoCode(db, { code: "FIRST-PROMO", commerceUserId });

    await expect(
      redeemPromoCode(db, { code: "SECOND-PROMO", commerceUserId }),
    ).rejects.toBeInstanceOf(UserAlreadyRedeemedError);
  });

  it("serializes two different codes raced by one user to exactly one redemption", async () => {
    const partnerId = await partner("user-race");
    const first = await createPromoCode(db, {
      partnerId,
      code: "USER-RACE-A",
      valueNano: USD,
    });
    const second = await createPromoCode(db, {
      partnerId,
      code: "USER-RACE-B",
      valueNano: 2n * USD,
    });
    const commerceUserId = randomUUID();

    const results = await Promise.allSettled([
      redeemPromoCode(db, { code: first.code, commerceUserId }),
      redeemPromoCode(db, { code: second.code, commerceUserId }),
    ]);
    const fulfilled = results.filter(
      (result): result is PromiseFulfilledResult<Awaited<ReturnType<typeof redeemPromoCode>>> =>
        result.status === "fulfilled",
    );
    const rejected = results.filter(
      (result): result is PromiseRejectedResult => result.status === "rejected",
    );
    expect(fulfilled).toHaveLength(1);
    expect(rejected).toHaveLength(1);
    expect(rejected[0]!.reason).toBeInstanceOf(UserAlreadyRedeemedError);

    const redeemed = await db.pool.query<{ count: string }>(
      `SELECT count(*)::text AS count FROM promo_codes
       WHERE redeemed_by_commerce_user_id = $1`,
      [commerceUserId],
    );
    expect(redeemed.rows[0]!.count).toBe("1");
    const audit = await db.pool.query<{ count: string }>(
      `SELECT count(*)::text AS count FROM sales_audit_log
       WHERE action = 'promo.redeemed' AND actor_id = $1`,
      [commerceUserId],
    );
    expect(audit.rows[0]!.count).toBe("1");
  });

  it("serializes two users racing one code to exactly one owner", async () => {
    const partnerId = await partner("code-race");
    const promo = await createPromoCode(db, {
      partnerId,
      code: "CODE-RACE",
      valueNano: USD,
    });
    const users = [randomUUID(), randomUUID()];

    const results = await Promise.allSettled(
      users.map((commerceUserId) =>
        redeemPromoCode(db, { code: promo.code, commerceUserId }),
      ),
    );
    const fulfilled = results.filter((result) => result.status === "fulfilled");
    const rejected = results.filter(
      (result): result is PromiseRejectedResult => result.status === "rejected",
    );
    expect(fulfilled).toHaveLength(1);
    expect(rejected).toHaveLength(1);
    expect(rejected[0]!.reason).toBeInstanceOf(PromoAlreadyRedeemedError);

    const stored = await db.pool.query<{
      redeemed_by_commerce_user_id: string;
      redemption_ref: string;
    }>(
      `SELECT redeemed_by_commerce_user_id, redemption_ref
       FROM promo_codes WHERE id = $1`,
      [promo.id],
    );
    expect(users).toContain(stored.rows[0]!.redeemed_by_commerce_user_id);
    expect(stored.rows[0]!.redemption_ref).toBe(`promo:${promo.id}`);
  });

  it("enforces value, count, case-insensitive collision, and active-partner creation limits", async () => {
    const partnerId = await partner("limits", {
      maxValueNano: 3n * USD,
      maxCount: 1,
    });
    await expect(
      createPromoCode(db, {
        partnerId,
        code: "TOO-VALUABLE",
        valueNano: 3n * USD + 1n,
      }),
    ).rejects.toBeInstanceOf(PromoLimitError);

    await createPromoCode(db, {
      partnerId,
      code: "AT-LIMIT",
      valueNano: 3n * USD,
    });
    await expect(
      createPromoCode(db, {
        partnerId,
        code: "OVER-COUNT",
        valueNano: USD,
      }),
    ).rejects.toBeInstanceOf(PromoLimitError);

    const collisionPartner = await partner("collision");
    await expect(
      createPromoCode(db, {
        partnerId: collisionPartner,
        code: "at-limit",
        valueNano: USD,
      }),
    ).rejects.toBeInstanceOf(PromoCodeCollisionError);

    const suspendedPartner = await partner("creation-suspended", {
      status: "suspended",
    });
    await expect(
      createPromoCode(db, {
        partnerId: suspendedPartner,
        code: "SUSPENDED-CREATE",
        valueNano: USD,
      }),
    ).rejects.toBeInstanceOf(PromoNotAllowedError);
  });

  it("requires explicit discount permission and caps the promo discount", async () => {
    const partnerId = await partner("discount-cap", {
      discountEnabled: false,
      discountBps: 3_000,
    });
    await expect(
      createPromoCode(db, {
        partnerId,
        code: "NO-DISCOUNT-PERMISSION",
        valueNano: USD,
        discountBps: 1_000,
      }),
    ).rejects.toBeInstanceOf(PromoNotAllowedError);

    await db.pool.query(
      `UPDATE partners
       SET referral_discount_enabled = true, referral_discount_bps = 3000
       WHERE id = $1`,
      [partnerId],
    );
    await expect(
      createPromoCode(db, {
        partnerId,
        code: "ABOVE-DISCOUNT-CAP",
        valueNano: USD,
        discountBps: 3_001,
      }),
    ).rejects.toBeInstanceOf(PromoNotAllowedError);

    await expect(
      createPromoCode(db, {
        partnerId,
        code: "AT-DISCOUNT-CAP",
        valueNano: USD,
        discountBps: 3_000,
      }),
    ).resolves.toMatchObject({ discountBps: 3_000 });
  });
});
