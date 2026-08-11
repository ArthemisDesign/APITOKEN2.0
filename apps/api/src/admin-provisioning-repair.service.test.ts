import { randomUUID } from "node:crypto";
import { ConflictException } from "@nestjs/common";
import type { ConfigService } from "@nestjs/config";
import { createDatabase, type Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AdminService } from "./admin.service.js";
import type { Environment } from "./config.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("admin provisioning repair", () => {
  let database: Database;
  let userId: string;
  const engineAccountId = "acct_admin_repair";

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
      INSERT INTO engine_accounts (id, user_id, engine_account_id, status, mult_bp)
      VALUES ($1, $2, $3, 'pending', 3700)
    `, [randomUUID(), userId, engineAccountId]);
  });

  afterAll(async () => {
    await database.pool.end();
  });

  function serviceWithLiveAccount(input: { status?: string; multBp?: number } = {}): {
    service: AdminService;
    getAccount: ReturnType<typeof vi.fn>;
  } {
    const getAccount = vi.fn().mockResolvedValue({
      account: engineAccountId,
      status: input.status ?? "active",
      mult_bp: input.multBp ?? 3_700,
    });
    return {
      service: new AdminService(
        database,
        { getAccount } as unknown as EngineClient,
        {} as ConfigService<Environment, true>,
      ),
      getAccount,
    };
  }

  it("requires and proves the same live engine state around the atomic repair", async () => {
    const { service, getAccount } = serviceWithLiveAccount();
    await expect(service.repairUserProvisioningV2(
      userId,
      "operator-1",
      "engine account already serves traffic",
    )).resolves.toEqual({
      status: "activated",
      job_id: null,
      previous_status: "pending",
      engine_account_id: engineAccountId,
      multiplier_bp: 3_700,
      engine_verified: true,
    });
    expect(getAccount).toHaveBeenCalledTimes(2);

    await expect(service.repairUserProvisioningV2(
      userId,
      "operator-1",
      "idempotent retry",
    )).resolves.toMatchObject({ status: "already_active", engine_verified: true });
    const audit = await database.pool.query<{ count: string }>(`
      SELECT count(*)::text AS count FROM audit_log
      WHERE target_id = $1 AND action = 'user.provisioning_reconciled'
    `, [userId]);
    expect(audit.rows[0]!.count).toBe("1");
  });

  it("rejects an inactive or differently priced engine before commerce changes", async () => {
    const inactive = serviceWithLiveAccount({ status: "disabled" }).service;
    await expect(inactive.repairUserProvisioningV2(
      userId,
      "operator-1",
      "must stay pending",
    )).rejects.toBeInstanceOf(ConflictException);

    const repriced = serviceWithLiveAccount({ multBp: 4_000 }).service;
    await expect(repriced.repairUserProvisioningV2(
      userId,
      "operator-1",
      "must repair pricing first",
    )).rejects.toBeInstanceOf(ConflictException);

    const mapping = await database.pool.query<{ status: string }>(
      "SELECT status FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    expect(mapping.rows[0]!.status).toBe("pending");
  });
});
