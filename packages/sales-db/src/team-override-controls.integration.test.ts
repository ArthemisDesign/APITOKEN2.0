import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createSalesDatabase, type SalesDatabase } from "./client.js";
import { updatePartnerAdmin } from "./admin.js";
import { createTelegramPartner } from "./auth.js";
import { createPartnerInvite } from "./invites.js";
import {
  computeCommissionChain,
  loadCommissionChain,
  TeamMemberNotFoundError,
  TeamOverrideLimitError,
  updateDirectTeamMemberControls,
} from "./commissions.js";

const connectionString = process.env.TEST_SALES_DATABASE_URL;

describe.runIf(Boolean(connectionString))("individual Team override controls", () => {
  let db: SalesDatabase;

  beforeAll(async () => {
    db = createSalesDatabase(connectionString!);
    await db.pool.query("SELECT 1");
  });

  afterAll(async () => {
    await truncate();
    await db.pool.end();
  });

  beforeEach(async () => {
    await truncate();
  });

  async function truncate(): Promise<void> {
    await db.pool.query("TRUNCATE partners RESTART IDENTITY CASCADE");
  }

  async function partner(input: {
    code: string;
    parentId?: string;
    commissionBps?: number;
    subCommissionBps?: number;
    maximumBps?: number | null;
    parentOverrideBps?: number | null;
  }): Promise<string> {
    const result = await db.pool.query<{ id: string }>(`
      INSERT INTO partners (
        referral_code, status, commission_bps, sub_commission_bps,
        parent_partner_id, team_override_max_bps, parent_override_bps
      )
      VALUES ($1, 'active', $2, $3, $4, $5, $6)
      RETURNING id
    `, [
      input.code,
      input.commissionBps ?? 1_000,
      input.subCommissionBps ?? 1_000,
      input.parentId ?? null,
      input.maximumBps ?? null,
      input.parentOverrideBps ?? null,
    ]);
    return result.rows[0]!.id;
  }

  it("accepts 0% and 20%, and rejects 20.01% or a parent-bound violation", async () => {
    const root = await partner({ code: "root-boundary", maximumBps: 1_500 });
    await expect(partner({
      code: "zero-edge", parentId: root, maximumBps: 0, parentOverrideBps: 0,
    })).resolves.toBeTypeOf("string");
    await db.pool.query("UPDATE partners SET team_override_max_bps = 2000 WHERE id = $1", [root]);
    await expect(partner({
      code: "twenty-edge", parentId: root, maximumBps: 2_000, parentOverrideBps: 2_000,
    })).resolves.toBeTypeOf("string");
    await expect(partner({
      code: "too-high-global", parentId: root, maximumBps: 2_001, parentOverrideBps: 2_000,
    })).rejects.toMatchObject({ code: "23514" });

    const narrowRoot = await partner({ code: "root-narrow", maximumBps: 1_500 });
    await expect(partner({
      code: "too-high-parent", parentId: narrowRoot, maximumBps: 1_501, parentOverrideBps: 1_500,
    })).rejects.toMatchObject({ code: "23514" });
  });

  it("copies the exact edge and delegated ceiling when an invite is consumed", async () => {
    const root = await partner({ code: "root-invite", maximumBps: 1_200 });
    const invite = await createPartnerInvite(db, {
      partnerId: root,
      code: "team-invite-copy",
      telegramUsername: "member_copy",
      commissionBps: 1_000,
      subCommissionBps: null,
      teamOverrideMaxBps: 900,
      parentOverrideBps: 700,
      promoEnabled: false,
      promoMaxValueNano: 0n,
      promoMaxCount: 0,
      referralDiscountBps: 0,
      referralDiscountEnabled: false,
      expiresAt: new Date(Date.now() + 60_000),
    });
    const created = await createTelegramPartner(db, {
      telegramId: "987654321",
      telegramUsername: "member_copy",
      telegramPhotoUrl: null,
      displayName: "Member Copy",
      referralCode: "member-copy",
      inviteCode: invite.code,
      defaultCommissionBps: 1_000,
      defaultSubCommissionBps: 1_000,
    });
    expect(created).toMatchObject({
      parentPartnerId: root,
      commissionBps: 1_000,
      teamOverrideMaxBps: 900,
      parentOverrideBps: 700,
    });
  });

  it("updates only a direct member and clamps their dependent subtree atomically", async () => {
    const root = await partner({ code: "root-update", maximumBps: 2_000 });
    const child = await partner({
      code: "child-update", parentId: root, maximumBps: 1_800, parentOverrideBps: 1_600,
    });
    const grandchild = await partner({
      code: "grandchild-update", parentId: child, maximumBps: 1_700, parentOverrideBps: 1_500,
    });
    await createPartnerInvite(db, {
      partnerId: child,
      code: "pending-child-invite",
      telegramUsername: "pending_child",
      commissionBps: 1_000,
      subCommissionBps: null,
      teamOverrideMaxBps: 1_600,
      parentOverrideBps: 1_400,
      promoEnabled: false,
      promoMaxValueNano: 0n,
      promoMaxCount: 0,
      referralDiscountBps: 0,
      referralDiscountEnabled: false,
      expiresAt: new Date(Date.now() + 60_000),
    });

    await expect(updateDirectTeamMemberControls(db, {
      parentPartnerId: root,
      memberId: child,
      overrideBps: 600,
      teamOverrideMaxBps: 700,
    })).resolves.toEqual({ memberId: child, overrideBps: 600, teamOverrideMaxBps: 700 });

    const rows = await db.pool.query<{
      id: string;
      commission_bps: number;
      team_override_max_bps: number;
      parent_override_bps: number;
    }>(`
      SELECT id, commission_bps, team_override_max_bps, parent_override_bps
      FROM partners WHERE id = ANY($1::uuid[]) ORDER BY id
    `, [[child, grandchild]]);
    expect(rows.rows.find((row) => row.id === child)).toMatchObject({
      commission_bps: 1_000, team_override_max_bps: 700, parent_override_bps: 600,
    });
    expect(rows.rows.find((row) => row.id === grandchild)).toMatchObject({
      team_override_max_bps: 700, parent_override_bps: 700,
    });
    const pending = await db.pool.query<{
      team_override_max_bps: number;
      parent_override_bps: number;
    }>("SELECT team_override_max_bps, parent_override_bps FROM partner_invites WHERE code = 'pending-child-invite'");
    expect(pending.rows[0]).toEqual({ team_override_max_bps: 700, parent_override_bps: 700 });

    await expect(updateDirectTeamMemberControls(db, {
      parentPartnerId: root, memberId: child, overrideBps: 2_001,
    })).rejects.toBeInstanceOf(TeamOverrideLimitError);
    await expect(updateDirectTeamMemberControls(db, {
      parentPartnerId: root, memberId: randomUUID(), overrideBps: 100,
    })).rejects.toBeInstanceOf(TeamMemberNotFoundError);
  });

  it("loads explicit edges while preserving the legacy NULL fallback", async () => {
    const root = await partner({ code: "root-chain", subCommissionBps: 650, maximumBps: 2_000 });
    const explicit = await partner({
      code: "explicit-chain", parentId: root, parentOverrideBps: 1_750,
    });
    const legacy = await partner({ code: "legacy-chain", parentId: root, parentOverrideBps: null });
    const client = await db.pool.connect();
    try {
      const explicitPlan = computeCommissionChain(await loadCommissionChain(client, explicit), 10_000n);
      const legacyPlan = computeCommissionChain(await loadCommissionChain(client, legacy), 10_000n);
      expect(explicitPlan[1]?.appliedBps).toBe(1_750);
      expect(legacyPlan[1]?.appliedBps).toBe(650);
    } finally {
      client.release();
    }
  });

  it("lets the admin lower a partner ceiling without leaving an invalid dependent grant", async () => {
    const root = await partner({ code: "root-admin", maximumBps: 2_000 });
    const child = await partner({
      code: "child-admin", parentId: root, maximumBps: 1_800, parentOverrideBps: 1_700,
    });
    await expect(updatePartnerAdmin(db, root, {
      teamOverrideMaxBps: 600,
      actorId: "test-admin",
    })).resolves.toBe(true);
    const rows = await db.pool.query<{
      id: string;
      team_override_max_bps: number;
      parent_override_bps: number | null;
    }>(`
      SELECT id, team_override_max_bps, parent_override_bps
      FROM partners WHERE id = ANY($1::uuid[])
    `, [[root, child]]);
    expect(rows.rows.find((row) => row.id === root)?.team_override_max_bps).toBe(600);
    expect(rows.rows.find((row) => row.id === child)).toMatchObject({
      team_override_max_bps: 600,
      parent_override_bps: 600,
    });
  });
});
