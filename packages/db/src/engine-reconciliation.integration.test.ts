import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  reconcileProvisionedEngineAccount,
} from "./engine.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("provisioned engine-account reconciliation", () => {
  let database: Database;
  let userId: string;
  const engineAccountId = "acct_reconciliation";

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  beforeEach(async () => {
    await database.pool.query(`
      TRUNCATE customer_profiles, engine_accounts, audit_log, users RESTART IDENTITY CASCADE
    `);
    userId = randomUUID();
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Repair')",
      [userId, `${userId}@t.invalid`],
    );
    await database.pool.query(`
      INSERT INTO customer_profiles (user_id, customer_type, multiplier_bp, pricing_month_start)
      VALUES ($1, 'b2b', 3700, date_trunc('month', now()))
    `, [userId]);
    await database.pool.query(`
      INSERT INTO engine_accounts (
        id, user_id, engine_account_id, status, mult_bp, last_error
      ) VALUES ($1, $2, $3, 'pending', 3700, 'legacy policy lane removed')
    `, [randomUUID(), userId, engineAccountId]);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  it("activates only the exact live mapping and makes replay a no-op", async () => {
    await expect(reconcileProvisionedEngineAccount(database, {
      userId,
      engineAccountId,
      multiplierBp: 3_700,
      actorId: "operator-1",
      reason: "engine account is already live",
    })).resolves.toEqual({ status: "activated", previousStatus: "pending" });

    const mapping = await database.pool.query<{ status: string; last_error: string | null }>(
      "SELECT status, last_error FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    expect(mapping.rows[0]).toEqual({ status: "active", last_error: null });
    const audit = await database.pool.query<{ action: string; metadata: Record<string, unknown> }>(
      "SELECT action, metadata FROM audit_log WHERE target_id = $1",
      [userId],
    );
    expect(audit.rows).toEqual([{
      action: "user.provisioning_reconciled",
      metadata: expect.objectContaining({
        engineAccountId,
        multiplierBp: 3_700,
        previousStatus: "pending",
        reason: "engine account is already live",
      }),
    }]);

    await expect(reconcileProvisionedEngineAccount(database, {
      userId,
      engineAccountId,
      multiplierBp: 3_700,
      actorId: "operator-1",
      reason: "safe retry",
    })).resolves.toEqual({ status: "already_active", previousStatus: "active" });
    const auditCount = await database.pool.query<{ count: string }>(
      "SELECT count(*)::text AS count FROM audit_log WHERE target_id = $1",
      [userId],
    );
    expect(auditCount.rows[0]!.count).toBe("1");
  });

  it("rejects mapping and pricing drift without changing readiness", async () => {
    await expect(reconcileProvisionedEngineAccount(database, {
      userId,
      engineAccountId: "acct_other",
      multiplierBp: 3_700,
      actorId: "operator-1",
      reason: "wrong account",
    })).rejects.toMatchObject({ code: "mapping_changed" });

    await database.pool.query(
      "UPDATE customer_profiles SET multiplier_bp = 4000 WHERE user_id = $1",
      [userId],
    );
    await expect(reconcileProvisionedEngineAccount(database, {
      userId,
      engineAccountId,
      multiplierBp: 3_700,
      actorId: "operator-1",
      reason: "stale price",
    })).rejects.toMatchObject({ code: "pricing_drift" });

    const status = await database.pool.query<{ status: string }>(
      "SELECT status FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    expect(status.rows[0]!.status).toBe("pending");
    const audit = await database.pool.query("SELECT 1 FROM audit_log WHERE target_id = $1", [userId]);
    expect(audit.rowCount).toBe(0);
  });

  it("never reactivates a disabled account", async () => {
    await database.pool.query(
      "UPDATE engine_accounts SET status = 'disabled' WHERE user_id = $1",
      [userId],
    );
    await expect(reconcileProvisionedEngineAccount(database, {
      userId,
      engineAccountId,
      multiplierBp: 3_700,
      actorId: "operator-1",
      reason: "must remain disabled",
    })).rejects.toMatchObject({ code: "disabled" });
  });
});
