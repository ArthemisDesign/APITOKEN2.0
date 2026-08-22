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

  it("expands payout batches with their immutable earnings cutoff", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0019_payout_earned_boundary.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 18);
    const current = journal.entries.find((entry) => entry.idx === 19);

    expect(current).toMatchObject({ idx: 19, tag: "0019_payout_earned_boundary" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).toContain('ALTER TABLE "payout_batches" ADD COLUMN "earned_before"');
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|DROP)\b/im);
  });

  it("expands exact immutable reversal accounting before its consumer", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0017_payment_reversal_accounting.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 16);
    const current = journal.entries.find((entry) => entry.idx === 17);

    expect(current).toMatchObject({ idx: 17, tag: "0017_payment_reversal_accounting" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE)\b/im);
    expect(migration).toContain("sync_cursors_feed_v3_check");
    expect(migration).toContain("'topup_funding_lots'");
    expect(migration).toContain("'payment_reversals'");
    expect(migration.indexOf("ADD CONSTRAINT")).toBeLessThan(
      migration.indexOf("DROP CONSTRAINT"),
    );
    expect(migration.indexOf("VALIDATE CONSTRAINT")).toBeLessThan(
      migration.indexOf("DROP CONSTRAINT"),
    );

    const tables = [...migration.matchAll(/^CREATE TABLE "([^"]+)"/gm)]
      .map((match) => match[1])
      .sort();
    expect(tables).toEqual([
      "partner_commission_adjustments",
      "partner_commission_funding_allocations",
      "partner_paid_funding_lots",
      "partner_payment_reversals",
      "partner_usage_funding_allocations",
    ]);
    expect(migration).toContain('"commerce_reversal_id" bigint NOT NULL UNIQUE');
    expect(migration).toContain('"commerce_topup_id" bigint NOT NULL UNIQUE');
    expect(migration).toContain('"commerce_payment_id" text NOT NULL UNIQUE');
    expect(migration).toContain('"amount_nano" < 0');
    expect(migration).toContain("usage funding allocation violates paid-lot FIFO order");
    expect(migration).toContain("commission funding allocation does not match deterministic rounding");
    expect(migration).toContain("payment reversal requires every exact commission adjustment");
    expect(migration).toContain("payment reversal accounting requires SERIALIZABLE isolation");
    expect(migration).toContain("CREATE CONSTRAINT TRIGGER");
    expect(migration).toContain("commission adjustment must negate the exact reversed payment share");
    for (const trigger of [
      "partner_paid_funding_lots_immutable",
      "partner_usage_funding_alloc_immutable",
      "partner_commission_funding_immutable",
      "partner_payment_reversals_immutable",
      "partner_commission_adjustments_immutable",
    ]) {
      expect(migration).toContain(`"${trigger}"`);
    }

    const databaseObjectNames = [
      ...migration.matchAll(/^CREATE TABLE "([^"]+)"/gm),
      ...migration.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migration.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE FUNCTION "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE CONSTRAINT TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });

  it("fences zero-adjustment reversals and late allocations before the consumer", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0018_reversal_completeness_fence.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 17);
    const current = journal.entries.find((entry) => entry.idx === 18);

    expect(current).toMatchObject({ idx: 18, tag: "0018_reversal_completeness_fence" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|ALTER TABLE|DROP)\b/im);
    expect(migration).toContain("partner_reversal_insert_complete_guard");
    expect(migration).toContain("partner_reversed_usage_complete_guard");
    expect(migration).toContain("partner_reversed_commission_complete_guard");
    expect(migration).toContain("payment reversal requires complete prior usage funding allocation");
    expect(migration).toContain("payment reversal requires every exact commission adjustment");
    expect(migration).toContain("payment reversal accounting requires SERIALIZABLE isolation");
    expect(migration).toContain('FOR UPDATE;');

    const databaseObjectNames = [
      ...migration.matchAll(/^CREATE FUNCTION "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE CONSTRAINT TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });

  it("adds the spend provider as a nullable reporting dimension only", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0022_spend_provider_dimension.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 21);
    const current = journal.entries.find((entry) => entry.idx === 22);

    expect(current).toMatchObject({ idx: 22, tag: "0022_spend_provider_dimension" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    // Expand-only: the dimension may never rewrite or delete recorded money history.
    expect(migration).not.toMatch(/\b(?:DROP|UPDATE|DELETE|TRUNCATE)\b/i);
    for (const table of ["partner_usage_events", "pending_referral_events"]) {
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "spend_provider_id" text;`);
    }
    // Nullable and unbackfilled: rows imported earlier honestly have no provider on record.
    expect(migration).not.toMatch(/"spend_provider_id" text NOT NULL/);
    // It must stay outside the retired attribution tuple: the legacy CHECK keeps fencing that
    // tuple untouched, so recording a provider for reporting cannot re-open a commission path.
    const statements = migration
      .split("--> statement-breakpoint")
      .map((part) => part.replace(/^\s*--.*$/gm, "").trim())
      .filter((part) => part.length > 0);
    expect(statements.length).toBeGreaterThan(0);
    for (const statement of statements) {
      expect(statement).not.toContain("multi_discount_check");
      expect(statement).not.toContain("attribution_check");
    }
  });

  it("adds the B2B grant off by default and ties the ceiling to it", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0023_partner_b2b_grant.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 22);
    const current = journal.entries.find((entry) => entry.idx === 23);

    expect(current).toMatchObject({ idx: 23, tag: "0023_partner_b2b_grant" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/\b(?:DROP|UPDATE|DELETE|TRUNCATE)\b/i);
    for (const table of ["partners", "partner_invites"]) {
      // Existing partners must not acquire the right by migrating.
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "b2b_enabled" boolean DEFAULT false NOT NULL;`);
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "b2b_max_discount_bps" integer DEFAULT 0 NOT NULL;`);
    }
    // A ceiling may never outlive the grant, and never exceed the policy maximum.
    const checks = migration.match(/CHECK \("b2b_max_discount_bps"[\s\S]*?\)\)/g) ?? [];
    expect(checks).toHaveLength(2);
    for (const check of checks) {
      expect(check).toContain("BETWEEN 0 AND 9500");
      expect(check).toContain('"b2b_enabled" OR "b2b_max_discount_bps" = 0');
    }
    // The retired marker columns stay retired rather than being overloaded a second time.
    expect(migration).not.toMatch(/ALTER TABLE[\s\S]*referral_discount/);
  });

  it("expands per-edge team overrides under a hard 20 percent ceiling", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0024_team_override_controls.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 23);
    const current = journal.entries.find((entry) => entry.idx === 24);

    expect(current).toMatchObject({ idx: 24, tag: "0024_team_override_controls" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    // Expand-only: no existing partner, invite, or financial row is rewritten or removed.
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|DROP)\b/im);
    for (const table of ["partners", "partner_invites"]) {
      expect(migration).toContain(
        `ALTER TABLE "${table}" ADD COLUMN "team_override_max_bps" integer;`,
      );
      expect(migration).toContain(
        `ALTER TABLE "${table}" ADD COLUMN "parent_override_bps" integer;`,
      );
    }
    expect(migration.match(/"team_override_max_bps" IS NULL OR "team_override_max_bps" BETWEEN 0 AND 2000/g)).toHaveLength(2);
    expect(migration.match(/"parent_override_bps" BETWEEN 0 AND 2000/g)).toHaveLength(2);
    expect(migration).toContain("partners_team_override_bounds_guard");
    expect(migration).toContain("partner_invites_team_override_bounds_guard");
    expect(migration).toContain("partners_team_override_ceiling_update_guard");
    // The v2 immutable guard accepts the additive edge, while NULL preserves the deployed writer.
    expect(migration).toContain('COALESCE(edge_override_bps, parent."sub_commission_bps")');
  });

  it("stages immutable partner authority requests without creating live work", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0025_partner_authority_requests.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 24);
    const current = journal.entries.find((entry) => entry.idx === 25);

    expect(current).toMatchObject({ idx: 25, tag: "0025_partner_authority_requests" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    // Existing rows are neither rewritten nor removed. The migration creates empty authority
    // storage and compatible columns only.
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|DROP)\b/im);
    for (const table of [
      "partner_requests",
      "partner_request_provider_terms",
      "partner_request_provider_decisions",
      "partner_request_effects",
    ]) {
      expect(migration).toContain(`CREATE TABLE "${table}"`);
    }
    expect(migration).toContain('"team_invites_enabled" boolean DEFAULT true NOT NULL');
    expect(migration).toContain('"b2b_can_delegate" boolean DEFAULT false NOT NULL');
    expect(migration).toContain('"b2b_grant_source_partner_id" uuid');
    expect(migration).toContain("partners_b2b_authority_narrowing_guard");
    expect(migration).toContain("partner_requests_transition_guard");
    expect(migration).toContain("partner_request_effects_transition_guard");
    expect(migration).toContain("sales_audit_log_immutable");
    expect(migration).toContain("partner_requests_pending_commission_uidx");
    expect(migration).toContain("partner_requests_pending_b2b_uidx");

    const databaseObjectNames = [
      ...migration.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migration.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE FUNCTION "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });

  it("stages Commerce membership and conserved commission evidence without rewriting history", () => {
    const migration = readFileSync(
      join(MIGRATIONS_FOLDER, "0026_commerce_partner_membership.sql"),
      "utf8",
    );
    const journal = JSON.parse(
      readFileSync(join(MIGRATIONS_FOLDER, "meta", "_journal.json"), "utf8"),
    ) as { entries: Array<{ idx: number; tag: string; when: number }> };
    const previous = journal.entries.find((entry) => entry.idx === 25);
    const current = journal.entries.find((entry) => entry.idx === 26);

    expect(current).toMatchObject({ idx: 26, tag: "0026_commerce_partner_membership" });
    expect(current!.when).toBeGreaterThan(previous!.when);
    expect(migration).not.toMatch(/^(?:UPDATE|DELETE|TRUNCATE|DROP)\b/im);
    expect(migration).toContain('ALTER TABLE "partners" ADD COLUMN "commerce_user_id" uuid');
    expect(migration).toContain('"program_enabled" boolean DEFAULT false NOT NULL');
    expect(migration).toContain('"partners_commerce_user_uidx"');
    expect(migration).toContain('ALTER TABLE "partner_invites" ADD COLUMN "commerce_user_id" uuid');
    expect(migration).toContain('"partner_invites_open_commerce_uidx"');
    for (const table of ["commission_entries", "commission_entries_v2"]) {
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "calculation_version"`);
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "gross_amount_nano"`);
      expect(migration).toContain(`ALTER TABLE "${table}" ADD COLUMN "withheld_amount_nano"`);
    }
    expect(migration).toContain('"amount_nano" = "gross_amount_nano" - "withheld_amount_nano"');
    expect(migration).toContain('current_gross_nano::numeric * next_edge_bps::numeric / 10000');
    expect(migration).toContain("current_level + 1 < 10");
    expect(migration).toContain("current_status <> 'active'");
    expect(migration).toContain("NOT current_program_enabled");
    expect(migration).toContain("current_program_started_at > source_occurred_at");
    expect(migration).toContain("parent_program_started_at <= source_occurred_at");
    expect(migration).toContain("conserved Team share must be between 0 and 2000 bps");
    expect(migration).toContain('IF NEW."calculation_version" = 1 THEN');

    const databaseObjectNames = [
      ...migration.matchAll(/CONSTRAINT "([^"]+)"/g),
      ...migration.matchAll(/^CREATE (?:UNIQUE )?INDEX "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE (?:OR REPLACE )?FUNCTION "([^"]+)"/gm),
      ...migration.matchAll(/^CREATE TRIGGER "([^"]+)"/gm),
    ].map((match) => match[1]).filter((name): name is string => name !== undefined);
    expect(databaseObjectNames.filter((name) => Buffer.byteLength(name, "utf8") > 63)).toEqual([]);
  });
});
