import { Injectable, NotFoundException } from "@nestjs/common";
import { z } from "zod";
import { ReferralSalesClient } from "./referral-sales.client.js";

const invitationSchema = z.object({
  id: z.string().uuid(),
  inviterCommerceUserId: z.string().uuid().nullable(),
  commissionBps: z.number().int().min(0).max(10_000),
  parentOverrideBps: z.number().int().min(0).max(2_000),
  teamOverrideMaxBps: z.number().int().min(0).max(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: z.number().int().min(0).max(9_500),
  b2bCanDelegate: z.boolean(),
  expiresAt: z.string().nullable(),
  createdAt: z.string(),
}).strict();
const pendingSchema = z.object({ invitation: invitationSchema.nullable() }).strict();
const acceptSchema = z.object({ state: z.literal("active"), activated: z.boolean() }).passthrough();
const declineSchema = z.object({ declined: z.boolean() }).strict();

/** What the invitee is shown before deciding. Internal Sales identifiers never leave this layer. */
export interface PendingTeamInvitation {
  id: string;
  commissionBps: number;
  retainedShareBps: number;
  teamOverrideMaxBps: number;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  expiresAt: string | null;
  createdAt: string;
}

@Injectable()
export class ReferralInvitationService {
  constructor(private readonly sales: ReferralSalesClient) {}

  async pending(commerceUserId: string): Promise<{ invitation: PendingTeamInvitation | null }> {
    const result = await this.sales.call(`invitations/${encodeURIComponent(commerceUserId)}`, pendingSchema);
    if (!result.invitation) return { invitation: null };
    const invitation = result.invitation;
    return {
      invitation: {
        id: invitation.id,
        commissionBps: invitation.commissionBps,
        retainedShareBps: invitation.parentOverrideBps,
        teamOverrideMaxBps: invitation.teamOverrideMaxBps,
        b2bEnabled: invitation.b2bEnabled,
        b2bMaxDiscountBps: invitation.b2bMaxDiscountBps,
        expiresAt: invitation.expiresAt,
        createdAt: invitation.createdAt,
      },
    };
  }

  /** Accepting is what creates the membership; nothing else in the read path does. */
  async accept(commerceUserId: string): Promise<{ accepted: true }> {
    const pending = await this.pending(commerceUserId);
    if (!pending.invitation) throw new NotFoundException("no invitation to accept");
    await this.sales.call(`invitations/${encodeURIComponent(commerceUserId)}/accept`, acceptSchema, {
      method: "POST",
      body: {},
    });
    return { accepted: true };
  }

  async decline(commerceUserId: string, inviteId: string): Promise<{ declined: boolean }> {
    return this.sales.call(`invitations/${encodeURIComponent(commerceUserId)}/decline`, declineSchema, {
      method: "POST",
      body: { inviteId },
    });
  }
}
