import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { updatePartnerAdmin } from "./admin.js";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { createPartnerInvite } from "./invites.js";
import {
  PartnerB2BAuthorityError,
  PartnerTeamAuthorityError,
  updateDirectTeamMemberAuthority,
} from "./partner-authority.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("delegated Partner authority cascade", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(truncate);

  async function truncate(): Promise<void> {
    await db.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function partner(input: {
    parentId?: string;
    sourceId?: string;
    maximumBps?: number;
    canDelegate?: boolean;
  } = {}): Promise<string> {
    const result = await db.pool.query<{ id: string }>(`
      INSERT INTO partners (
        referral_code, status, commission_bps, team_override_max_bps,
        parent_partner_id, b2b_enabled, b2b_max_discount_bps,
        b2b_can_delegate, b2b_grant_source_partner_id
      )
      VALUES ($1, 'active', 1000, 2000, $2, true, $3, $4, $5)
      RETURNING id
    `, [
      `ref${randomUUID().replaceAll("-", "").slice(0, 12)}`,
      input.parentId ?? null,
      input.maximumBps ?? 7_000,
      input.canDelegate ?? true,
      input.sourceId ?? null,
    ]);
    return result.rows[0]!.id;
  }

  it("clamps inherited descendants and pending invites leaf-first, then revokes the whole subtree", async () => {
    const root = await partner({ maximumBps: 7_000 });
    const child = await partner({ parentId: root, sourceId: root, maximumBps: 6_000 });
    const grandchild = await partner({ parentId: child, sourceId: child, maximumBps: 5_000 });
    await createPartnerInvite(db, {
      partnerId: child,
      code: "pending-b2b-authority",
      telegramUsername: "pending_b2b",
      commissionBps: 1_000,
      subCommissionBps: null,
      promoEnabled: false,
      promoMaxValueNano: 0n,
      promoMaxCount: 0,
      referralDiscountBps: 0,
      referralDiscountEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_500,
      b2bCanDelegate: true,
      expiresAt: new Date(Date.now() + 60_000),
    });

    await updatePartnerAdmin(db, root, {
      b2bMaxDiscountBps: 4_000,
      actorId: "admin:authority@example.test",
    });
    const clamped = await db.pool.query<{
      id: string;
      b2b_max_discount_bps: number;
      b2b_enabled: boolean;
    }>(`
      SELECT id, b2b_max_discount_bps, b2b_enabled
      FROM partners WHERE id = ANY($1::uuid[])
    `, [[root, child, grandchild]]);
    expect(clamped.rows.find((row) => row.id === root)?.b2b_max_discount_bps).toBe(4_000);
    expect(clamped.rows.find((row) => row.id === child)?.b2b_max_discount_bps).toBe(4_000);
    expect(clamped.rows.find((row) => row.id === grandchild)?.b2b_max_discount_bps).toBe(4_000);
    expect((await db.pool.query<{ b2b_max_discount_bps: number }>(
      "SELECT b2b_max_discount_bps FROM partner_invites WHERE code = 'pending-b2b-authority'",
    )).rows[0]?.b2b_max_discount_bps).toBe(4_000);

    await updatePartnerAdmin(db, root, {
      b2bEnabled: false,
      actorId: "admin:authority@example.test",
    });
    const revoked = await db.pool.query<{
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
      b2b_grant_source_partner_id: string | null;
    }>(`
      SELECT b2b_enabled, b2b_max_discount_bps, b2b_can_delegate, b2b_grant_source_partner_id
      FROM partners WHERE id = ANY($1::uuid[])
    `, [[root, child, grandchild]]);
    expect(revoked.rows).toHaveLength(3);
    for (const row of revoked.rows) {
      expect(row).toEqual({
        b2b_enabled: false,
        b2b_max_discount_bps: 0,
        b2b_can_delegate: false,
        b2b_grant_source_partner_id: null,
      });
    }
    expect((await db.pool.query<{
      b2b_enabled: boolean;
      b2b_max_discount_bps: number;
      b2b_can_delegate: boolean;
    }>(`
      SELECT b2b_enabled, b2b_max_discount_bps, b2b_can_delegate
      FROM partner_invites WHERE code = 'pending-b2b-authority'
    `)).rows[0]).toEqual({
      b2b_enabled: false,
      b2b_max_discount_bps: 0,
      b2b_can_delegate: false,
    });
  });

  it("protects a direct platform grant from parent edits without blocking Team-only controls", async () => {
    const root = await partner({ maximumBps: 7_000 });
    const child = await partner({ parentId: root, sourceId: root, maximumBps: 5_000 });
    await updatePartnerAdmin(db, child, {
      b2bEnabled: true,
      b2bMaxDiscountBps: 6_000,
      b2bCanDelegate: true,
      actorId: "admin:authority@example.test",
    });
    await expect(updateDirectTeamMemberAuthority(db, {
      parentPartnerId: root,
      memberId: child,
      b2bMaxDiscountBps: 4_000,
    })).rejects.toBeInstanceOf(PartnerB2BAuthorityError);
    await expect(updateDirectTeamMemberAuthority(db, {
      parentPartnerId: root,
      memberId: child,
      overrideBps: 500,
      teamInvitesEnabled: false,
    })).resolves.toMatchObject({
      memberId: child,
      overrideBps: 500,
      teamInvitesEnabled: false,
      b2bEnabled: true,
      b2bMaxDiscountBps: 6_000,
    });
    const stored = await db.pool.query<{ b2b_grant_source_partner_id: string | null }>(
      "SELECT b2b_grant_source_partner_id FROM partners WHERE id = $1",
      [child],
    );
    expect(stored.rows[0]?.b2b_grant_source_partner_id).toBeNull();
  });

  it("returns not-found cleanly when an admin B2B patch targets a missing partner", async () => {
    await expect(updatePartnerAdmin(db, randomUUID(), {
      b2bEnabled: true,
      b2bMaxDiscountBps: 4_000,
      actorId: "admin:authority@example.test",
    })).resolves.toBe(false);
  });

  it("does not let a partner delegate Team invitations after the platform disabled that right", async () => {
    const root = await partner();
    const child = await partner({ parentId: root, sourceId: root });
    await updatePartnerAdmin(db, root, {
      teamInvitesEnabled: false,
      actorId: "admin:authority@example.test",
    });
    await expect(updateDirectTeamMemberAuthority(db, {
      parentPartnerId: root,
      memberId: child,
      teamInvitesEnabled: true,
    })).rejects.toBeInstanceOf(PartnerTeamAuthorityError);
    await expect(updateDirectTeamMemberAuthority(db, {
      parentPartnerId: root,
      memberId: child,
      teamInvitesEnabled: false,
    })).resolves.toMatchObject({ teamInvitesEnabled: false });
  });
});
