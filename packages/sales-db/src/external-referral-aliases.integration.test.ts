import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import {
  ensureExternalReferralAlias,
  ExternalReferralAliasConflictError,
  ExternalReferralAliasOwnerNotFoundError,
  resolveExternalReferralAlias,
} from "./external-referral-aliases.js";
import { resolveReferralCode } from "./discount-links.js";
import { deletePartnerAdmin, PartnerHasHistoryError } from "./admin.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("external referral aliases", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });
  afterAll(async () => {
    await reset();
    await db.pool.end();
  });
  beforeEach(async () => reset());

  async function reset(): Promise<void> {
    await db.pool.query("TRUNCATE external_referral_aliases, referred_users, partners RESTART IDENTITY CASCADE");
  }

  async function partner(code: string): Promise<string> {
    const result = await db.pool.query<{ id: string }>(
      "INSERT INTO partners (referral_code, status, telegram_username) VALUES ($1, 'active', $1) RETURNING id",
      [code],
    );
    return result.rows[0]!.id;
  }

  it("is idempotent under concurrent issuance and resolves through ordinary attribution", async () => {
    const partnerId = await partner("crm-owner");
    const input = { source: "crm", externalRef: "contact:00000000-0000-4000-8000-000000000001", partnerReferralCode: "crm-owner" };

    const [first, second] = await Promise.all([
      ensureExternalReferralAlias(db, input),
      ensureExternalReferralAlias(db, input),
    ]);

    expect(first.aliasCode).toBe(second.aliasCode);
    expect(first.aliasCode).toMatch(/^r_[a-z0-9_-]{24}$/);
    expect(await resolveExternalReferralAlias(db, first.aliasCode)).toEqual({ partnerId });
    expect(await resolveReferralCode(db, first.aliasCode)).toEqual({
      partnerId,
      discountBps: 0,
      discountLinkId: null,
      discountLinkConsumed: false,
    });
  });

  it("never moves an external reference to another partner", async () => {
    await partner("owner-a");
    await partner("owner-b");
    const identity = { source: "crm", externalRef: "contact:00000000-0000-4000-8000-000000000002" };
    await ensureExternalReferralAlias(db, { ...identity, partnerReferralCode: "owner-a" });

    await expect(ensureExternalReferralAlias(db, { ...identity, partnerReferralCode: "owner-b" }))
      .rejects.toBeInstanceOf(ExternalReferralAliasConflictError);
  });

  it("requires an active canonical partner owner", async () => {
    await expect(ensureExternalReferralAlias(db, {
      source: "crm",
      externalRef: "contact:00000000-0000-4000-8000-000000000003",
      partnerReferralCode: "missing",
    })).rejects.toBeInstanceOf(ExternalReferralAliasOwnerNotFoundError);
  });

  it("keeps the owner immutable by blocking partner deletion", async () => {
    const partnerId = await partner("owner-with-alias");
    await ensureExternalReferralAlias(db, {
      source: "crm",
      externalRef: "contact:00000000-0000-4000-8000-000000000004",
      partnerReferralCode: "owner-with-alias",
    });

    await expect(deletePartnerAdmin(db, partnerId, "test-admin"))
      .rejects.toBeInstanceOf(PartnerHasHistoryError);
  });
});
