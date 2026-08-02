import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("paid-funded commission v2 schema", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await db.pool.query(`
      TRUNCATE commission_entries_v2, partner_usage_events_v2,
        pending_referral_usage_events_v2, partners RESTART IDENTITY CASCADE
    `);
    await db.pool.end();
  });

  beforeEach(async () => {
    await db.pool.query(`
      TRUNCATE commission_entries_v2, partner_usage_events_v2,
        pending_referral_usage_events_v2, partners RESTART IDENTITY CASCADE
    `);
  });

  async function seedChain(): Promise<{ direct: string; parent: string }> {
    const parent = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(referral_code,status,commission_bps,sub_commission_bps)
      VALUES('v2-parent','active',1000,500)
      RETURNING id
    `);
    const direct = await db.pool.query<{ id: string }>(`
      INSERT INTO partners(
        referral_code,status,parent_partner_id,commission_bps,sub_commission_bps
      ) VALUES('v2-direct','active',$1,1000,1000)
      RETURNING id
    `, [parent.rows[0]!.id]);
    return { direct: direct.rows[0]!.id, parent: parent.rows[0]!.id };
  }

  async function seedUsage(partnerId: string): Promise<bigint> {
    const event = await db.pool.query<{ id: string }>(`
      INSERT INTO partner_usage_events_v2(
        commerce_event_id,commerce_user_id,partner_id,provider_id,account_class,
        official_nano,charged_nano,paid_funded_nano,bonus_funded_nano,
        other_funded_nano,commission_eligible,release_generation,release_digest,
        snapshot_digest,occurred_at
      ) VALUES(101,$1,$2,'google','b2c',20000,12000,10000,2000,0,true,7,
        'release-v2','snapshot-v2',now())
      RETURNING id::text
    `, [randomUUID(), partnerId]);
    return BigInt(event.rows[0]!.id);
  }

  it("binds every level to exact paid funding and the active referral chain", async () => {
    const chain = await seedChain();
    const usageEventId = await seedUsage(chain.direct);

    await db.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id,partner_id,level,applied_bps,base_paid_funded_nano,amount_nano
      ) VALUES($1,$2,0,1000,10000,1000)
    `, [usageEventId.toString(), chain.direct]);
    await db.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id,partner_id,level,applied_bps,base_paid_funded_nano,amount_nano
      ) VALUES($1,$2,1,500,10000,50)
    `, [usageEventId.toString(), chain.parent]);

    const rows = await db.pool.query<{
      level: number;
      base_paid_funded_nano: string;
      amount_nano: string;
    }>(`
      SELECT level,base_paid_funded_nano::text,amount_nano::text
      FROM commission_entries_v2
      ORDER BY level
    `);
    expect(rows.rows).toEqual([
      { level: 0, base_paid_funded_nano: "10000", amount_nano: "1000" },
      { level: 1, base_paid_funded_nano: "10000", amount_nano: "50" },
    ]);
  });

  it("rejects an invented base, a skipped chain level, and mutation of immutable evidence", async () => {
    const chain = await seedChain();
    const usageEventId = await seedUsage(chain.direct);

    await expect(db.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id,partner_id,level,applied_bps,base_paid_funded_nano,amount_nano
      ) VALUES($1,$2,0,1000,9999,999)
    `, [usageEventId.toString(), chain.direct])).rejects.toMatchObject({ code: "23514" });

    await expect(db.pool.query(`
      INSERT INTO commission_entries_v2(
        usage_event_id,partner_id,level,applied_bps,base_paid_funded_nano,amount_nano
      ) VALUES($1,$2,1,500,10000,50)
    `, [usageEventId.toString(), chain.parent])).rejects.toMatchObject({ code: "23514" });

    await expect(db.pool.query(`
      UPDATE partner_usage_events_v2 SET paid_funded_nano=9000 WHERE id=$1
    `, [usageEventId.toString()])).rejects.toMatchObject({ code: "23514" });
  });
});
