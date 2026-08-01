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
});
