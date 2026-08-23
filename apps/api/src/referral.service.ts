import { Inject, Injectable, NotFoundException, UnprocessableEntityException } from "@nestjs/common";
import {
  findActiveReferralCommerceAccountByEmail,
  findActiveReferralCommerceAccountById,
  listReferralCommerceAccountsByIds,
  type Database,
  type ReferralCommerceAccount,
} from "@claude-api/db";
import { DATABASE } from "./infrastructure.module.js";
import {
  adminPartnersSchema,
  adminPayoutsSchema,
  adminRequestsSchema,
  businessPricingMutationSchema,
  onboardingMutationSchema,
  partnerMutationSchema,
  payoutMutationSchema,
  referralSnapshotSchema,
  requestMutationSchema,
  teamInvitationMutationSchema,
  teamInvitationRevocationSchema,
  teamMemberMutationSchema,
  walletMutationSchema,
  ReferralSalesClient,
  type ReferralSalesRequest,
  type ReferralSalesSnapshot,
} from "./referral-sales.client.js";

export interface PartnerAuthorityInput {
  teamOverrideMaxBps: number;
  teamInvitesEnabled: boolean;
  b2bEnabled: boolean;
  b2bMaxDiscountBps: number;
  b2bCanDelegate: boolean;
}

@Injectable()
export class ReferralService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    private readonly sales: ReferralSalesClient,
  ) {}

  async partnerSnapshot(commerceUserId: string, currentEmail: string): Promise<unknown> {
    const snapshot = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}?days=30`,
      referralSnapshotSchema,
    );
    return this.enrichPartnerSnapshot(snapshot, currentEmail);
  }

  async inviteTeamMember(commerceUserId: string, input: {
    email: string;
    overrideBps: number;
    authority: PartnerAuthorityInput;
  }): Promise<unknown> {
    const account = await this.requireActiveEmail(input.email);
    if (account.id === commerceUserId) throw new UnprocessableEntityException("you cannot invite your own account");
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/team-invitations`,
      teamInvitationMutationSchema,
      {
        method: "POST",
        body: {
          inviteeCommerceUserId: account.id,
          overrideBps: input.overrideBps,
          authority: input.authority,
        },
      },
    );
    const { commerceUserId: _commerceUserId, inviterPartnerId: _inviterPartnerId, ...invitation } = result.invitation;
    return { invitation: { ...invitation, email: account.email } };
  }

  async revokeTeamInvitation(commerceUserId: string, inviteId: string): Promise<unknown> {
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/team-invitations/${encodeURIComponent(inviteId)}`,
      teamInvitationRevocationSchema,
      { method: "DELETE" },
    );
    return {
      invitation: {
        id: result.invitation.id,
        revokedAt: result.invitation.revokedAt,
        revoked: result.invitation.revoked,
      },
    };
  }

  async updateTeamMember(commerceUserId: string, input: { email: string } & Partial<PartnerAuthorityInput> & {
    overrideBps?: number;
  }): Promise<unknown> {
    const account = await this.requireActiveEmail(input.email);
    const { email: _email, ...patch } = input;
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/team/${encodeURIComponent(account.id)}`,
      teamMemberMutationSchema,
      { method: "PATCH", body: patch },
    );
    const { memberId: _memberId, ...authority } = result.authority;
    return { authority: { ...authority, email: account.email } };
  }

  async requestCommission(commerceUserId: string, currentEmail: string, input: {
    requestedCommissionBps: number;
    reason: string;
    idempotencyKey: string;
  }): Promise<unknown> {
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/requests/commission`,
      requestMutationSchema,
      {
        method: "POST",
        idempotencyKey: input.idempotencyKey,
        body: { requestedCommissionBps: input.requestedCommissionBps, reason: input.reason },
      },
    );
    return { request: publicRequest(result.request, new Map(), currentEmail) };
  }

  async requestB2B(commerceUserId: string, currentEmail: string, input: {
    customerEmail: string;
    requestType: "b2b_conversion" | "b2b_pricing";
    requestedDiscountBps: number;
    providers: Record<string, number | null>;
    reason: string;
    idempotencyKey: string;
  }): Promise<unknown> {
    const account = await this.requirePricedAccount(input.customerEmail);
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/requests/b2b`,
      requestMutationSchema,
      {
        method: "POST",
        idempotencyKey: input.idempotencyKey,
        body: {
          referralCommerceUserId: account.id,
          requestType: input.requestType,
          requestedDiscountBps: input.requestedDiscountBps,
          providers: input.providers,
          reason: input.reason,
          stateSnapshot: {
            customerType: account.customerType,
            discountPercent: (account.discountBps ?? 0) / 100,
          },
        },
      },
    );
    return { request: publicRequest(result.request, new Map([[account.id, account]]), currentEmail) };
  }

  async setBusinessPricing(commerceUserId: string, input: {
    customerEmail: string;
    discountPercent?: number;
    providers?: Record<string, number | null>;
    idempotencyKey: string;
  }): Promise<unknown> {
    const account = await this.requireActiveEmail(input.customerEmail);
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/referrals/business-pricing`,
      businessPricingMutationSchema,
      {
        method: "POST",
        idempotencyKey: input.idempotencyKey,
        body: {
          referralCommerceUserId: account.id,
          ...(input.discountPercent === undefined ? {} : { discountPercent: input.discountPercent }),
          ...(input.providers === undefined ? {} : { providers: input.providers }),
        },
      },
    );
    const { userId: _userId, ...safe } = result;
    return { ...safe, customerEmail: account.email };
  }

  async updateWallet(commerceUserId: string, address: string): Promise<unknown> {
    const result = await this.sales.call(
      `partner/${encodeURIComponent(commerceUserId)}/wallet`,
      walletMutationSchema,
      { method: "PATCH", body: { address } },
    );
    return { membership: publicMembership(result.membership) };
  }

  async adminPartners(): Promise<{ items: unknown[] }> {
    const result = await this.sales.call("admin/partners", adminPartnersSchema);
    const accounts = await this.accountMap(result.items.map((item) => item.commerceUserId));
    const commerceByPartner = new Map(result.items.map((item) => [item.partnerId, item.commerceUserId]));
    return {
      items: result.items.map((item) => {
        const account = accounts.get(item.commerceUserId);
        const parentCommerceId = item.parentPartnerId ? commerceByPartner.get(item.parentPartnerId) : undefined;
        const parent = parentCommerceId ? accounts.get(parentCommerceId) : undefined;
        const { partnerId: _partnerId, commerceUserId: _commerceUserId, parentPartnerId: _parentPartnerId, ...safe } = item;
        return {
          ...safe,
          email: account?.email ?? null,
          accountStatus: account?.status ?? null,
          parentEmail: parent?.email ?? null,
        };
      }),
    };
  }

  async onboardByEmail(input: {
    email: string;
    commissionBps: number;
    authority: PartnerAuthorityInput;
    actor: string;
  }): Promise<unknown> {
    const account = await this.requireActiveEmail(input.email);
    return this.onboardAccount(account, input.commissionBps, input.authority, input.actor);
  }

  async onboardByUserId(input: {
    userId: string;
    commissionBps: number;
    authority: PartnerAuthorityInput;
    actor: string;
  }): Promise<unknown> {
    const account = await findActiveReferralCommerceAccountById(this.database, input.userId);
    if (!account) throw new NotFoundException("active Commerce account not found");
    return this.onboardAccount(account, input.commissionBps, input.authority, input.actor);
  }

  async updatePartner(input: {
    email: string;
    patch: Record<string, unknown>;
    actor: string;
  }): Promise<unknown> {
    const account = await this.requireActiveEmail(input.email);
    const result = await this.sales.call(
      `admin/partners/${encodeURIComponent(account.id)}`,
      partnerMutationSchema,
      { method: "PATCH", body: input.patch, actor: input.actor },
    );
    return { membership: { ...publicMembership(result.membership), email: account.email } };
  }

  async adminRequests(query: string): Promise<unknown> {
    const [requests, partners] = await Promise.all([
      this.sales.call(`admin/requests${query}`, adminRequestsSchema),
      this.sales.call("admin/partners", adminPartnersSchema),
    ]);
    const requesterCommerce = new Map(partners.items.map((item) => [item.partnerId, item.commerceUserId]));
    const ids = requests.items.flatMap((request) => [
      ...(request.customerCommerceUserId ? [request.customerCommerceUserId] : []),
      ...(requesterCommerce.get(request.requesterPartnerId) ? [requesterCommerce.get(request.requesterPartnerId)!] : []),
    ]);
    const accounts = await this.accountMap(ids);
    return {
      items: requests.items.map((request) => {
        const requesterId = requesterCommerce.get(request.requesterPartnerId);
        return publicRequest(request, accounts, requesterId ? accounts.get(requesterId)?.email ?? null : null);
      }),
      nextCursor: requests.nextCursor,
    };
  }

  async decideRequest(requestId: string, body: unknown, actor: string): Promise<unknown> {
    const result = await this.sales.call(
      `admin/requests/${encodeURIComponent(requestId)}/decision`,
      requestMutationSchema,
      { method: "POST", body, actor },
    );
    return { request: { id: result.request.id, status: result.request.status } };
  }

  async adminPayouts(query: string): Promise<unknown> {
    const [payouts, partners] = await Promise.all([
      this.sales.call(`admin/payouts${query}`, adminPayoutsSchema),
      this.sales.call("admin/partners", adminPartnersSchema),
    ]);
    const commerceByPartner = new Map(partners.items.map((item) => [item.partnerId, item.commerceUserId]));
    const accounts = await this.accountMap([...commerceByPartner.values()]);
    return {
      items: payouts.items.map((payout) => {
        const commerceUserId = commerceByPartner.get(payout.partnerId);
        const { partnerId: _partnerId, ...safe } = payout;
        return { ...safe, email: commerceUserId ? accounts.get(commerceUserId)?.email ?? null : null };
      }),
    };
  }

  async decidePayout(payoutId: string, body: unknown, actor: string): Promise<unknown> {
    const result = await this.sales.call(
      `admin/payouts/${encodeURIComponent(payoutId)}/decision`,
      payoutMutationSchema,
      { method: "POST", body, actor },
    );
    return { payout: { id: result.payout.id, status: result.payout.status } };
  }

  private async onboardAccount(
    account: ReferralCommerceAccount,
    commissionBps: number,
    authority: PartnerAuthorityInput,
    actor: string,
  ): Promise<unknown> {
    const result = await this.sales.call("admin/partners", onboardingMutationSchema, {
      method: "POST",
      body: { commerceUserId: account.id, commissionBps, authority },
      actor,
    });
    return {
      created: result.created,
      membership: { ...publicMembership(result.membership), email: account.email },
    };
  }

  private async enrichPartnerSnapshot(snapshot: ReferralSalesSnapshot, currentEmail: string): Promise<unknown> {
    if (snapshot.state === "unavailable") return snapshot;
    if (snapshot.state === "disabled") {
      return { state: snapshot.state, membership: { ...publicMembership(snapshot.membership), email: currentEmail } };
    }
    const ids = [
      ...snapshot.referrals.map((item) => item.commerceUserId),
      ...snapshot.team.map((item) => item.commerceUserId),
      ...snapshot.invitations.map((item) => item.commerceUserId),
      ...snapshot.requests.flatMap((item) => item.customerCommerceUserId ? [item.customerCommerceUserId] : []),
    ];
    const accounts = await this.accountMap(ids);
    return {
      state: "active",
      activated: snapshot.activated,
      membership: { ...publicMembership(snapshot.membership), email: currentEmail },
      totals: snapshot.totals,
      referrals: snapshot.referrals.map((item) => {
        const account = accounts.get(item.commerceUserId);
        const { commerceUserId: _commerceUserId, userRef: _userRef, ...safe } = item;
        return {
          ...safe,
          email: account?.email ?? null,
          customerType: account?.customerType ?? null,
          discountBps: account?.discountBps ?? null,
          providerDiscounts: account?.providerDiscounts ?? [],
        };
      }),
      team: snapshot.team.map((item) => {
        const account = accounts.get(item.commerceUserId);
        const {
          id: _id,
          commerceUserId: _commerceUserId,
          b2bGrantSourcePartnerId: _b2bGrantSourcePartnerId,
          ...safe
        } = item;
        return { ...safe, email: account?.email ?? null };
      }),
      earnings: snapshot.earnings,
      invitations: snapshot.invitations.map((item) => {
        const account = accounts.get(item.commerceUserId);
        const { commerceUserId: _commerceUserId, ...safe } = item;
        return { ...safe, email: account?.email ?? null };
      }),
      requests: snapshot.requests.map((request) => publicRequest(request, accounts, currentEmail)),
      payouts: snapshot.payouts.map(({ partnerId: _partnerId, ...payout }) => payout),
      period: snapshot.period,
      periodHistory: snapshot.periodHistory,
      payoutPolicy: snapshot.payoutPolicy,
    };
  }

  private async requireActiveEmail(email: string): Promise<ReferralCommerceAccount> {
    const account = await findActiveReferralCommerceAccountByEmail(this.database, email);
    if (!account) throw new NotFoundException("active Commerce account not found for this email");
    return account;
  }

  private async requirePricedAccount(email: string): Promise<ReferralCommerceAccount & {
    customerType: "b2c" | "b2b";
    discountBps: number;
  }> {
    const account = await this.requireActiveEmail(email);
    if (account.customerType === null || account.discountBps === null) {
      throw new UnprocessableEntityException("Commerce pricing profile is not ready for this account");
    }
    return account as ReferralCommerceAccount & { customerType: "b2c" | "b2b"; discountBps: number };
  }

  private async accountMap(ids: readonly string[]): Promise<Map<string, ReferralCommerceAccount>> {
    const accounts = await listReferralCommerceAccountsByIds(this.database, ids);
    return new Map(accounts.map((account) => [account.id, account]));
  }
}

function publicMembership(membership: zMembership): Omit<zMembership,
  "partnerId" | "commerceUserId" | "parentPartnerId" | "b2bGrantSourcePartnerId"
> {
  const {
    partnerId: _partnerId,
    commerceUserId: _commerceUserId,
    parentPartnerId: _parentPartnerId,
    b2bGrantSourcePartnerId: _b2bGrantSourcePartnerId,
    ...safe
  } = membership;
  return safe;
}

type zMembership = Extract<ReferralSalesSnapshot, { state: "active" | "disabled" }>["membership"];

function publicRequest(
  request: ReferralSalesRequest,
  accounts: Map<string, ReferralCommerceAccount>,
  requesterEmail: string | null,
): unknown {
  const customer = request.customerCommerceUserId ? accounts.get(request.customerCommerceUserId) : undefined;
  const {
    requesterPartnerId: _requesterPartnerId,
    requesterDisplayName: _requesterDisplayName,
    subjectPartnerId: _subjectPartnerId,
    customerCommerceUserId: _customerCommerceUserId,
    customerEmail: _customerEmail,
    requesterEmail: _producerRequesterEmail,
    ...safe
  } = request;
  return {
    ...safe,
    requesterEmail,
    customerEmail: customer?.email ?? null,
  };
}
