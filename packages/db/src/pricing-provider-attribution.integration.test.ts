import { randomUUID } from "node:crypto";
import type { EngineLedgerEntry } from "@claude-api/contracts";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createDatabase, type Database } from "./client.js";
import {
  applyPricingProviderBackfillPage,
  completePricingProviderBackfill,
  getPricingProviderBackfillCursor,
  PricingLedgerEvidenceError,
  type PricingSyncTarget,
} from "./pricing.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("pricing provider recovery uses exact ledger evidence", () => {
  let database: Database;
  let target: PricingSyncTarget;

  beforeAll(async () => {
    database = createDatabase(connectionString!);
    await database.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await database.pool.end();
  });

  beforeEach(async () => {
    await database.pool.query(
      `TRUNCATE pricing_usage_attributions, pricing_usage_events, engine_accounts,
       customer_profiles, users RESTART IDENTITY CASCADE`,
    );
    const userId = randomUUID();
    target = { userId, engineAccountId: "acct_provider_recovery" };
    await database.pool.query(
      "INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Provider recovery')",
      [userId, `${userId}@t.invalid`],
    );
    await database.pool.query(
      `INSERT INTO engine_accounts (id, user_id, engine_account_id, status)
       VALUES ($1, $2, $3, 'active')`,
      [randomUUID(), userId, target.engineAccountId],
    );
  });

  async function seedEvent(input: {
    ledgerId: bigint;
    providerId: string | null;
    recoveryVersion: number;
    amountNano?: bigint;
    occurredAt?: Date;
  }): Promise<void> {
    await database.pool.query(
      `INSERT INTO pricing_usage_events (
         id, user_id, engine_account_id, ledger_entry_id, provider_id,
         provider_recovery_version, amount_nano, real_funded_nano, occurred_at
       ) VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8)`,
      [
        randomUUID(),
        target.userId,
        target.engineAccountId,
        input.ledgerId.toString(),
        input.providerId,
        input.recoveryVersion,
        (input.amountNano ?? 100n).toString(),
        input.occurredAt ?? new Date(),
      ],
    );
  }

  function charge(
    ledgerId: bigint,
    amountNano: bigint,
    provider: string | null | undefined,
  ): EngineLedgerEntry {
    return {
      id: ledgerId.toString(),
      kind: "charge",
      request_id: null,
      amount_nano: amountNano.toString(),
      amount: amountNano.toString(),
      key_masked: null,
      ref: null,
      balance_after_nano: null,
      ts: "1700000000",
      model: null,
      provider,
      official_nano: amountNano.toString(),
    };
  }

  async function states(): Promise<Array<{
    ledger_entry_id: string;
    provider_id: string | null;
    provider_recovery_version: number;
  }>> {
    const result = await database.pool.query<{
      ledger_entry_id: string;
      provider_id: string | null;
      provider_recovery_version: number;
    }>(`
      SELECT ledger_entry_id::text, provider_id, provider_recovery_version
      FROM pricing_usage_events
      WHERE user_id = $1 AND engine_account_id = $2
      ORDER BY ledger_entry_id
    `, [target.userId, target.engineAccountId]);
    return result.rows;
  }

  it("starts immediately before the oldest recent sentinel not tried by the current algorithm", async () => {
    await seedEvent({ ledgerId: 10n, providerId: "unavailable", recoveryVersion: 1 });
    await seedEvent({ ledgerId: 20n, providerId: null, recoveryVersion: 0 });
    await seedEvent({ ledgerId: 30n, providerId: "anthropic", recoveryVersion: 0 });
    await seedEvent({ ledgerId: 40n, providerId: "unavailable", recoveryVersion: 2 });
    await seedEvent({
      ledgerId: 5n,
      providerId: "unavailable",
      recoveryVersion: 0,
      occurredAt: new Date(Date.now() - 31 * 24 * 60 * 60 * 1000),
    });

    await expect(getPricingProviderBackfillCursor(database, target, 45n)).resolves.toBe(9n);
    await expect(getPricingProviderBackfillCursor(database, target, 9n)).resolves.toBeNull();
  });

  it("copies only a matching account-ledger-amount provider and never relabels exact evidence", async () => {
    await seedEvent({ ledgerId: 10n, providerId: "unavailable", recoveryVersion: 1 });
    await seedEvent({ ledgerId: 11n, providerId: null, recoveryVersion: 0 });
    await seedEvent({ ledgerId: 12n, providerId: "anthropic", recoveryVersion: 0 });

    await expect(applyPricingProviderBackfillPage(database, target, [
      charge(10n, 100n, "openai"),
      charge(11n, 100n, null),
      charge(12n, 100n, "anthropic"),
    ])).resolves.toBe(1);
    expect(await states()).toEqual([
      { ledger_entry_id: "10", provider_id: "openai", provider_recovery_version: 2 },
      { ledger_entry_id: "11", provider_id: null, provider_recovery_version: 0 },
      { ledger_entry_id: "12", provider_id: "anthropic", provider_recovery_version: 0 },
    ]);

    await expect(applyPricingProviderBackfillPage(
      database,
      target,
      [charge(12n, 100n, "google")],
    )).rejects.toBeInstanceOf(PricingLedgerEvidenceError);
    await expect(applyPricingProviderBackfillPage(
      database,
      target,
      [charge(10n, 101n, "openai")],
    )).rejects.toBeInstanceOf(PricingLedgerEvidenceError);
  });

  it("terminalizes only exhausted recent sentinels inside the completed ledger range", async () => {
    await seedEvent({ ledgerId: 1n, providerId: null, recoveryVersion: 0 });
    await seedEvent({ ledgerId: 2n, providerId: "unattributed", recoveryVersion: 1 });
    await seedEvent({ ledgerId: 3n, providerId: "unavailable", recoveryVersion: 2 });
    await seedEvent({ ledgerId: 4n, providerId: "openai", recoveryVersion: 0 });
    await seedEvent({ ledgerId: 5n, providerId: "unavailable", recoveryVersion: 0 });
    await seedEvent({
      ledgerId: 6n,
      providerId: null,
      recoveryVersion: 0,
      occurredAt: new Date(Date.now() - 31 * 24 * 60 * 60 * 1000),
    });

    await expect(completePricingProviderBackfill(database, target, 4n)).resolves.toBe(2);
    expect(await states()).toEqual([
      { ledger_entry_id: "1", provider_id: "unavailable", provider_recovery_version: 2 },
      { ledger_entry_id: "2", provider_id: "unavailable", provider_recovery_version: 2 },
      { ledger_entry_id: "3", provider_id: "unavailable", provider_recovery_version: 2 },
      { ledger_entry_id: "4", provider_id: "openai", provider_recovery_version: 0 },
      { ledger_entry_id: "5", provider_id: "unavailable", provider_recovery_version: 0 },
      { ledger_entry_id: "6", provider_id: null, provider_recovery_version: 0 },
    ]);
    await expect(getPricingProviderBackfillCursor(database, target, 6n)).resolves.toBe(4n);
  });
});
