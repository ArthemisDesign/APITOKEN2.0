import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { MIGRATIONS_FOLDER } from "./migrate.js";

describe("sales multi-discount migration", () => {
  it("keeps legacy rows valid while fencing complete new attribution", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0013_multi_discount_attribution.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 12);
    const current = journal.entries.find((entry) => entry.idx === 13);

    expect(current).toMatchObject({ idx: 13, tag: "0013_multi_discount_attribution" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/\b(?:DROP|UPDATE|DELETE|TRUNCATE)\b/i);
    for (const column of [
      "provider_id",
      "account_class",
      "pricing_mode",
      "paid_funded_nano",
      "commission_eligible",
      "snapshot_digest",
    ]) {
      expect(migration).toContain(`ADD COLUMN "${column}"`);
    }
    expect(migration).toContain('"amount_nano" = "paid_funded_nano"');
    expect(migration).toContain('"pricing_mode" = \'track\'');
    expect(migration).toContain('"account_class" = \'b2c\'');
  });

  it("expands the pending usage buffer before the immutable attribution writer", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0014_usage_attribution_buffer.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 13);
    const current = journal.entries.find((entry) => entry.idx === 14);

    expect(current).toMatchObject({ idx: 14, tag: "0014_usage_attribution_buffer" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/\b(?:DROP|UPDATE|DELETE|TRUNCATE)\b/i);
    for (const column of [
      "provider_id",
      "account_class",
      "pricing_mode",
      "paid_funded_nano",
      "commission_eligible",
      "snapshot_digest",
    ]) {
      expect(migration).toContain(
        `ALTER TABLE "pending_referral_events" ADD COLUMN "${column}"`,
      );
    }
    expect(migration).toContain('"pending_referral_events_attribution_check"');
    expect(migration).toContain('"partner_usage_events_commission_authority_check"');
    expect(migration).toContain('"kind" = \'spend\'');
    expect(migration).toContain('"commission_eligible" IS TRUE');
    expect(migration).toContain('"amount_nano" = "paid_funded_nano"');
  });

  it("adds paid-funded schema v2 without pricing-mode or progressive-discount authority", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0015_paid_funded_commission_v2.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 14);
    const current = journal.entries.find((entry) => entry.idx === 15);

    expect(current).toMatchObject({ idx: 15, tag: "0015_paid_funded_commission_v2" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/^(?:DROP|UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migration).not.toMatch(/\b(?:pricing_mode|track|tier|retention)\b/i);

    const tables = [...migration.matchAll(/^CREATE TABLE "([^"]+)"/gm)]
      .map((match) => match[1])
      .sort();
    expect(tables).toEqual([
      "commission_entries_v2",
      "partner_usage_events_v2",
      "pending_referral_usage_events_v2",
    ]);
    expect(migration).toContain('"paid_funded_nano" + "bonus_funded_nano" + "other_funded_nano" = "charged_nano"');
    expect(migration).toContain('event_paid_funded_nano <> NEW."base_paid_funded_nano"');
    expect(migration).toContain('previous."level" = NEW."level" - 1');
    expect(migration).toContain('partner."status" = \'active\'');
    expect(migration).toContain('expected_input_nano::numeric * expected_bps::numeric / 10000');
    expect(migration).toContain('"commission_entries_v2_source_level_unique"');
    expect(migration).toContain('"partner_usage_events_v2_immutable"');
    expect(migration).toContain('"commission_entries_v2_immutable"');

    const databaseObjectNames = [
      ...migration.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migration.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migration.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE FUNCTION "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });

  it("widens the durable cursor namespace before the topups-v2 consumer", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0016_topups_v2_cursor.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 15);
    const current = journal.entries.find((entry) => entry.idx === 16);

    expect(current).toMatchObject({ idx: 16, tag: "0016_topups_v2_cursor" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|CREATE TABLE)\b/im);
    expect(migration).toContain("sync_cursors_feed_v2_check");
    expect(migration).toContain("'attributions', 'usage_events', 'topups', 'topups_v2'");
    expect(migration.indexOf("ADD CONSTRAINT")).toBeLessThan(
      migration.indexOf("DROP CONSTRAINT"),
    );
    expect(migration.indexOf("VALIDATE CONSTRAINT")).toBeLessThan(
      migration.indexOf("DROP CONSTRAINT"),
    );
  });
});
