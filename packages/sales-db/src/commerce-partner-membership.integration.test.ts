import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("Commerce partner membership and conserved commission schema", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(async () => {
    await truncate();
  });

  async function truncate(): Promise<void> {
    await database.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function partner(input: {
    code: string;
    commerceUserId?: string;
    programEnabled?: boolean;
    parentPartnerId?: string;
    parentOverrideBps?: number;
  }): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(
        referral_code, status, commission_bps, sub_commission_bps,
        team_override_max_bps, parent_partner_id, parent_override_bps,
        commerce_user_id, program_enabled, program_started_at
      )
      VALUES($1, 'active', 1000, 1000, 2000, $2, $3, $4, $5, $6)
      RETURNING id
    `, [
      input.code,
      input.parentPartnerId ?? null,
      input.parentOverrideBps ?? null,
      input.commerceUserId ?? null,
      input.programEnabled ?? false,
      input.programEnabled ? new Date("2026-08-22T12:00:00.000Z") : null,
    ]);
    return result.rows[0]!.id;
  }

  it("leaves legacy identities outside the new program and uniquely binds enabled Commerce accounts", async () => {
    const legacy = await partner({ code: "legacy-disabled" });
    const legacyRow = await database.pool.query<{
      commerce_user_id: string | null;
      program_enabled: boolean;
      program_started_at: Date | null;
    }>(`
      SELECT commerce_user_id, program_enabled, program_started_at
      FROM partners WHERE id = $1
    `, [legacy]);
    expect(legacyRow.rows[0]).toEqual({
      commerce_user_id: null,
      program_enabled: false,
      program_started_at: null,
    });

    const commerceUserId = randomUUID();
    await expect(partner({
      code: "commerce-enabled",
      commerceUserId,
      programEnabled: true,
    })).resolves.toBeTypeOf("string");
    await expect(partner({
      code: "commerce-duplicate",
      commerceUserId,
      programEnabled: true,
    })).rejects.toMatchObject({ code: "23505" });
    await expect(database.pool.query(`
      INSERT INTO partners(referral_code, status, program_enabled)
      VALUES('missing-membership', 'active', true)
    `)).rejects.toMatchObject({ code: "23514" });
  });

  it("allows only one open Commerce invite per account while preserving legacy invites", async () => {
    const commerceUserId = randomUUID();
    await database.pool.query(`
      INSERT INTO partner_invites(code, telegram_username)
      VALUES('legacy-telegram-invite', 'legacy_user')
    `);
    await database.pool.query(`
      INSERT INTO partner_invites(code, commerce_user_id)
      VALUES('commerce-invite-one', $1)
    `, [commerceUserId]);
    await expect(database.pool.query(`
      INSERT INTO partner_invites(code, commerce_user_id)
      VALUES('commerce-invite-two', $1)
    `, [commerceUserId])).rejects.toMatchObject({ code: "23505" });
    await database.pool.query(`
      UPDATE partner_invites SET revoked_at = now()
      WHERE code = 'commerce-invite-one'
    `);
    await expect(database.pool.query(`
      INSERT INTO partner_invites(code, commerce_user_id)
      VALUES('commerce-invite-after-revoke', $1)
    `, [commerceUserId])).resolves.toBeDefined();
  });

  it("conserves one direct gross commission across a two-level Commerce team", async () => {
    const root = await partner({
      code: "conserved-root",
      commerceUserId: randomUUID(),
      programEnabled: true,
    });
    const child = await partner({
      code: "conserved-child",
      commerceUserId: randomUUID(),
      programEnabled: true,
      parentPartnerId: root,
      parentOverrideBps: 2_000,
    });
    const usage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26001, $1, $2, 100000, '2026-08-22T12:10:00.000Z')
      RETURNING id::text
    `, [randomUUID(), child]);

    await database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 2, 10000, 2000, 8000)
    `, [usage.rows[0]!.id, child]);
    await database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 1, 2000, 2, 2000, 0, 2000)
    `, [usage.rows[0]!.id, root]);

    const total = await database.pool.query<{ total: string }>(`
      SELECT sum(amount_nano)::text AS total
      FROM commission_entries WHERE usage_event_id = $1
    `, [usage.rows[0]!.id]);
    expect(total.rows[0]!.total).toBe("10000");

    const invalidUsage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26002, $1, $2, 100000, '2026-08-22T12:11:00.000Z')
      RETURNING id::text
    `, [randomUUID(), child]);
    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 2, 10000, 0, 10000)
    `, [invalidUsage.rows[0]!.id, child])).rejects.toMatchObject({ code: "23514" });
  });

  it("does not withhold for a parent outside the Commerce program and keeps version 1 compatible", async () => {
    const legacyRoot = await partner({ code: "legacy-parent" });
    const child = await partner({
      code: "commerce-only-child",
      commerceUserId: randomUUID(),
      programEnabled: true,
      parentPartnerId: legacyRoot,
      parentOverrideBps: 2_000,
    });
    const usage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26003, $1, $2, 100000, '2026-08-22T12:12:00.000Z')
      RETURNING id::text
    `, [randomUUID(), child]);
    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 2, 10000, 0, 10000)
    `, [usage.rows[0]!.id, child])).resolves.toBeDefined();

    const legacyUsage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26004, $1, $2, 100000, '2026-08-22T12:13:00.000Z')
      RETURNING id::text
    `, [randomUUID(), legacyRoot]);
    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, amount_nano
      ) VALUES($1, $2, 0, 1000, 10000)
    `, [legacyUsage.rows[0]!.id, legacyRoot])).resolves.toBeDefined();
  });

  it("never pays or withholds commission for membership that started after the usage", async () => {
    const root = await partner({
      code: "late-root",
      commerceUserId: randomUUID(),
      programEnabled: true,
    });
    const child = await partner({
      code: "timely-child",
      commerceUserId: randomUUID(),
      programEnabled: true,
      parentPartnerId: root,
      parentOverrideBps: 2_000,
    });
    await database.pool.query(`
      UPDATE partners
      SET program_started_at = '2026-08-22T12:30:00.000Z'
      WHERE id = $1
    `, [root]);
    const usage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26006, $1, $2, 100000, '2026-08-22T12:15:00.000Z')
      RETURNING id::text
    `, [randomUUID(), child]);

    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 2, 10000, 0, 10000)
    `, [usage.rows[0]!.id, child])).resolves.toBeDefined();
    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 1, 2000, 2, 2000, 0, 2000)
    `, [usage.rows[0]!.id, root])).rejects.toMatchObject({ code: "23514" });

    await database.pool.query(`
      UPDATE partners
      SET program_started_at = '2026-08-22T12:30:00.000Z'
      WHERE id = $1
    `, [child]);
    const preMembershipUsage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events(
        commerce_event_id, commerce_user_id, partner_id, amount_nano, occurred_at
      ) VALUES(26007, $1, $2, 100000, '2026-08-22T12:20:00.000Z')
      RETURNING id::text
    `, [randomUUID(), child]);
    await expect(database.pool.query(`
      INSERT INTO commission_entries(
        usage_event_id, partner_id, level, applied_bps, calculation_version,
        gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 2, 10000, 0, 10000)
    `, [preMembershipUsage.rows[0]!.id, child])).rejects.toMatchObject({ code: "23514" });
  });

  it("applies the same conserved evidence guard to the historical release-v2 ledger", async () => {
    const root = await partner({
      code: "v2-root",
      commerceUserId: randomUUID(),
      programEnabled: true,
    });
    const child = await partner({
      code: "v2-child",
      commerceUserId: randomUUID(),
      programEnabled: true,
      parentPartnerId: root,
      parentOverrideBps: 2_000,
    });
    const usage = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events_v2(
        commerce_event_id, commerce_user_id, partner_id, provider_id, account_class,
        official_nano, charged_nano, paid_funded_nano, bonus_funded_nano,
        other_funded_nano, commission_eligible, release_generation, release_digest,
        snapshot_digest, occurred_at
      ) VALUES(
        26005, $1, $2, 'anthropic', 'b2c', 100000, 100000, 100000, 0, 0,
        true, 1, 'release-digest', 'snapshot-digest', '2026-08-22T12:14:00.000Z'
      ) RETURNING id::text
    `, [randomUUID(), child]);
    await expect(database.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id, partner_id, level, applied_bps, base_paid_funded_nano,
        calculation_version, gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 0, 1000, 100000, 2, 10000, 2000, 8000)
    `, [usage.rows[0]!.id, child])).resolves.toBeDefined();
    await expect(database.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id, partner_id, level, applied_bps, base_paid_funded_nano,
        calculation_version, gross_amount_nano, withheld_amount_nano, amount_nano
      ) VALUES($1, $2, 1, 2000, 100000, 2, 2000, 0, 2000)
    `, [usage.rows[0]!.id, root])).resolves.toBeDefined();
  });
});
