import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("terminal Commerce invitation authority", () => {
  let database: SalesDatabase;

  beforeAll(async () => {
    database = createSalesDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await database.pool.end();
  });

  beforeEach(truncate);

  async function truncate(): Promise<void> {
    await database.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function root(code: string): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partners(
        referral_code, status, team_override_max_bps,
        b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      ) VALUES($1, 'active', 2000, true, 2500, true)
      RETURNING id
    `, [code]);
    return result.rows[0]!.id;
  }

  async function commerceInvite(partnerId: string, code: string): Promise<string> {
    const result = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_invites(
        partner_id, code, commerce_user_id,
        team_override_max_bps, parent_override_bps,
        b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      ) VALUES($1, $2, $3, 1500, 1500, true, 1500, false)
      RETURNING id
    `, [partnerId, code, randomUUID()]);
    return result.rows[0]!.id;
  }

  it("ignores revoked grants during narrowing and preserves their terminal evidence", async () => {
    const partnerId = await root("terminal-root");
    const inviteId = await commerceInvite(partnerId, "terminal-invite");
    await database.pool.query("UPDATE partner_invites SET revoked_at = now() WHERE id = $1", [inviteId]);

    await expect(database.pool.query(`
      UPDATE partners
      SET team_override_max_bps = 500, b2b_max_discount_bps = 500
      WHERE id = $1
    `, [partnerId])).resolves.toBeDefined();
    await expect(database.pool.query(`
      UPDATE partner_invites SET expires_at = now() + interval '1 day' WHERE id = $1
    `, [inviteId])).rejects.toMatchObject({
      code: "23514",
      message: expect.stringContaining("terminal Commerce invitation is immutable"),
    });
    await expect(database.pool.query("DELETE FROM partner_invites WHERE id = $1", [inviteId]))
      .rejects.toMatchObject({
        code: "23514",
        message: expect.stringContaining("Commerce invitation evidence cannot be deleted"),
      });

    const stored = await database.pool.query<{
      team_override_max_bps: number;
      b2b_max_discount_bps: number;
      revoked_at: Date;
    }>(`
      SELECT team_override_max_bps, b2b_max_discount_bps, revoked_at
      FROM partner_invites WHERE id = $1
    `, [inviteId]);
    expect(stored.rows[0]).toMatchObject({
      team_override_max_bps: 1_500,
      b2b_max_discount_bps: 1_500,
      revoked_at: expect.any(Date),
    });
  });

  it("continues to block narrowing under a live grant and freezes consumed Commerce evidence", async () => {
    const partnerId = await root("live-root");
    const inviteId = await commerceInvite(partnerId, "live-invite");
    await expect(database.pool.query(`
      UPDATE partners SET team_override_max_bps = 500 WHERE id = $1
    `, [partnerId])).rejects.toMatchObject({
      code: "23514",
      message: expect.stringContaining("dependent grants"),
    });
    await expect(database.pool.query(`
      UPDATE partners SET b2b_max_discount_bps = 500 WHERE id = $1
    `, [partnerId])).rejects.toMatchObject({
      code: "23514",
      message: expect.stringContaining("inherited grants"),
    });

    await database.pool.query(`
      UPDATE partner_invites
      SET consumed_at = now(), consumed_by_partner_id = $2
      WHERE id = $1
    `, [inviteId, partnerId]);
    await expect(database.pool.query(`
      UPDATE partner_invites SET expires_at = now() + interval '1 day' WHERE id = $1
    `, [inviteId])).rejects.toMatchObject({
      code: "23514",
      message: expect.stringContaining("terminal Commerce invitation is immutable"),
    });
  });

  it("does not change the legacy invitation delete contract", async () => {
    const legacy = await database.pool.query<{ id: string }>(`
      INSERT INTO partner_invites(code, telegram_username)
      VALUES('legacy-delete', 'legacy_delete') RETURNING id
    `);
    await expect(database.pool.query(
      "DELETE FROM partner_invites WHERE id = $1",
      [legacy.rows[0]!.id],
    )).resolves.toMatchObject({ rowCount: 1 });
  });
});
