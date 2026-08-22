import { createHash } from "node:crypto";
import {
  BadRequestException,
  Body,
  ConflictException,
  Controller,
  Delete,
  ForbiddenException,
  Get,
  Header,
  Headers,
  HttpCode,
  Inject,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UnprocessableEntityException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  CommercePartnerAuthorityError,
  CommercePartnerConflictError,
  CommercePartnerNotFoundError,
  createB2BPartnerRequest,
  createCommerceTeamInvite,
  createCommissionChangeRequest,
  decidePartnerRequest,
  decidePayout,
  findPartnerByCommerceUserId,
  getPartnerDailyEarnings,
  getPartnerDailyEarningsByProvider,
  getPartnerEarningsByProvider,
  getPartnerEarningsTotals,
  getPartnerPeriodHistory,
  getPartnerPeriodState,
  getReferredUserPartner,
  insertSalesAudit,
  InvalidPayoutTransitionError,
  listPartnerInvites,
  listPartnerPayouts,
  listPartnerRequests,
  listPartnersWithAggregates,
  listPartnerTeam,
  listPayouts,
  listReferredUsers,
  onboardCommercePartner,
  PartnerB2BAuthorityError,
  PartnerRequestConflictError,
  PartnerRequestDecisionError,
  PartnerRequestNotFoundError,
  PartnerRequestValidationError,
  PartnerTeamAuthorityError,
  resolveCommercePartnerMembership,
  revokeCommerceTeamInvite,
  TeamMemberNotFoundError,
  TeamOverrideLimitError,
  updateCommercePartnerWallet,
  updateDirectTeamMemberAuthority,
  updatePartnerAdmin,
  type Partner,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { CommercePartnerPricingError, CommerceService } from "./commerce.service.js";
import { decodePartnerRequestCursor, encodePartnerRequestCursor, partnerRequestView } from "./partner-request-view.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import { InternalKeyGuard } from "./internal.controller.js";
import { walletSchema } from "./schemas.js";

const commerceUserIdSchema = z.string().uuid();
const uuidSchema = z.string().uuid();
const bps = (maximum: number) => z.number().int().min(0).max(maximum);
const providerIdSchema = z.enum(["anthropic", "openai", "google", "kimi", "glm"]);
const authoritySchema = z.object({
  teamOverrideMaxBps: bps(2_000),
  teamInvitesEnabled: z.boolean(),
  b2bEnabled: z.boolean(),
  b2bMaxDiscountBps: bps(9_500),
  b2bCanDelegate: z.boolean(),
}).strict().refine(
  (value) => value.b2bEnabled || (value.b2bMaxDiscountBps === 0 && !value.b2bCanDelegate),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const resolveSchema = z.object({ commerceUserId: commerceUserIdSchema }).strict();
const onboardingSchema = z.object({
  commerceUserId: commerceUserIdSchema,
  commissionBps: bps(10_000).default(1_000),
  authority: authoritySchema,
}).strict();
const adminPatchSchema = z.object({
  commissionBps: bps(10_000).optional(),
  teamOverrideMaxBps: bps(2_000).optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: bps(9_500).optional(),
  b2bCanDelegate: z.boolean().optional(),
  status: z.enum(["active", "suspended", "pending"]).optional(),
  programEnabled: z.boolean().optional(),
}).strict().refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one field is required",
}).refine(
  (value) => value.b2bEnabled !== false
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const teamInviteSchema = z.object({
  inviteeCommerceUserId: commerceUserIdSchema,
  overrideBps: bps(2_000),
  authority: authoritySchema,
}).strict();
const teamMemberPatchSchema = z.object({
  overrideBps: bps(2_000).optional(),
  teamOverrideMaxBps: bps(2_000).optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: bps(9_500).optional(),
  b2bCanDelegate: z.boolean().optional(),
}).strict().refine((value) => Object.values(value).some((item) => item !== undefined), {
  message: "at least one field is required",
}).refine(
  (value) => value.b2bEnabled !== false
    || ((value.b2bMaxDiscountBps ?? 0) === 0 && value.b2bCanDelegate !== true),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const commissionRequestSchema = z.object({
  requestedCommissionBps: bps(10_000),
  reason: z.string().trim().min(1).max(4_000),
}).strict();
const b2bRequestSchema = z.object({
  referralCommerceUserId: commerceUserIdSchema,
  requestType: z.enum(["b2b_conversion", "b2b_pricing"]),
  requestedDiscountBps: bps(9_500).refine((value) => value % 100 === 0),
  providers: z.record(providerIdSchema, bps(9_500).refine((value) => value % 100 === 0).nullable()).default({}),
  reason: z.string().trim().min(1).max(4_000),
  stateSnapshot: z.object({
    customerType: z.enum(["b2c", "b2b"]),
    discountPercent: z.number().min(0).max(100),
  }).strict(),
}).strict();
const businessPricingSchema = z.object({
  referralCommerceUserId: commerceUserIdSchema,
  discountPercent: z.number().int().min(0).max(95).optional(),
  providers: z.record(providerIdSchema, z.number().int().min(0).max(95).nullable()).optional(),
}).strict().refine(
  (value) => value.discountPercent !== undefined
    || (value.providers !== undefined && Object.keys(value.providers).length > 0),
  "at least one pricing value is required",
);
const requestQuerySchema = z.object({
  status: z.enum(["pending", "approved", "rejected", "applied", "apply_failed"]).optional(),
  requestType: z.enum(["b2b_conversion", "b2b_pricing", "commission_change"]).optional(),
  cursor: z.string().max(512).optional(),
  limit: z.coerce.number().int().min(1).max(100).default(25),
});
const adminDecisionSchema = z.discriminatedUnion("action", [
  z.object({ action: z.literal("reject"), note: z.string().trim().min(1).max(4_000) }).strict(),
  z.object({
    action: z.literal("approve"),
    note: z.string().trim().min(1).max(4_000),
    commissionBps: bps(10_000).optional(),
    discountBps: bps(9_500).optional(),
    providers: z.record(providerIdSchema, bps(9_500).nullable()).optional(),
  }).strict(),
]);
const payoutQuerySchema = z.object({
  status: z.enum(["requested", "approved", "paid", "rejected"]).optional(),
});
const payoutDecisionSchema = z.object({
  action: z.literal("reject"),
  note: z.string().trim().min(1).max(2_000),
}).strict();

function actorId(value: string | undefined): string {
  const actor = value?.trim();
  return actor && actor.length <= 200 ? actor : "commerce-admin";
}

function jsonSafe(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString();
  if (value instanceof Date) return value.toISOString();
  if (Array.isArray(value)) return value.map(jsonSafe);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, jsonSafe(item)]));
  }
  return value;
}

function partnerAccess(partner: Partner): unknown {
  return {
    partnerId: partner.id,
    commerceUserId: partner.commerceUserId,
    status: partner.status,
    programEnabled: partner.programEnabled,
    programStartedAt: partner.programStartedAt?.toISOString() ?? null,
    referralCode: partner.referralCode,
    parentPartnerId: partner.parentPartnerId,
    commissionBps: partner.commissionBps,
    teamShareBps: partner.parentOverrideBps,
    teamOverrideMaxBps: partner.teamOverrideMaxBps,
    teamInvitesEnabled: partner.teamInvitesEnabled,
    b2bEnabled: partner.b2bEnabled,
    b2bMaxDiscountBps: partner.b2bMaxDiscountBps,
    b2bCanDelegate: partner.b2bCanDelegate,
    b2bGrantSourcePartnerId: partner.b2bGrantSourcePartnerId,
    payoutMethod: partner.payoutMethod,
    payoutDetails: partner.payoutDetails,
    createdAt: partner.createdAt.toISOString(),
  };
}

function payoutView(payout: Awaited<ReturnType<typeof listPayouts>>[number]): unknown {
  return {
    id: payout.id,
    partnerId: payout.partnerId,
    amountNano: payout.amountNano.toString(),
    status: payout.status,
    method: payout.method,
    details: payout.details,
    requestedAt: payout.requestedAt.toISOString(),
    decidedAt: payout.decidedAt?.toISOString() ?? null,
    paidAt: payout.paidAt?.toISOString() ?? null,
    adminNote: payout.adminNote,
    txHash: payout.txHash,
    chainStatus: payout.chainStatus,
  };
}

function translateKnownError(error: unknown): never {
  if (error instanceof CommercePartnerConflictError || error instanceof PartnerRequestConflictError) {
    throw new ConflictException(error.message);
  }
  if (error instanceof CommercePartnerNotFoundError
    || error instanceof PartnerRequestNotFoundError
    || error instanceof TeamMemberNotFoundError) {
    throw new NotFoundException(error.message);
  }
  if (error instanceof PartnerTeamAuthorityError) throw new ForbiddenException(error.message);
  if (error instanceof CommercePartnerAuthorityError
    || error instanceof PartnerB2BAuthorityError
    || error instanceof PartnerRequestValidationError
    || error instanceof PartnerRequestDecisionError
    || error instanceof TeamOverrideLimitError
    || error instanceof InvalidPayoutTransitionError
    || error instanceof RangeError) {
    throw new UnprocessableEntityException(error.message);
  }
  throw error;
}

@Controller("internal/referral")
@UseGuards(InternalKeyGuard)
export class CommercePartnerController {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
    private readonly commerce: CommerceService,
  ) {}

  @Post("membership/resolve")
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  async resolve(@Body() body: unknown): Promise<unknown> {
    const parsed = resolveSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid Commerce membership payload");
    const result = await resolveCommercePartnerMembership(this.database, parsed.data);
    return {
      state: result.state,
      activated: result.activated,
      membership: result.partner ? partnerAccess(result.partner) : null,
    };
  }

  @Get("partner/:commerceUserId")
  @Header("Cache-Control", "no-store")
  async partnerSnapshot(
    @Param("commerceUserId") commerceUserId: string,
    @Query("days") daysRaw?: string,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const days = z.coerce.number().int().min(1).max(365).default(30).safeParse(daysRaw);
    if (!id.success || !days.success) throw new BadRequestException("invalid partner snapshot query");
    const resolution = await resolveCommercePartnerMembership(this.database, { commerceUserId: id.data });
    if (resolution.state !== "active") {
      return { state: resolution.state, membership: resolution.partner ? partnerAccess(resolution.partner) : null };
    }
    const partner = resolution.partner;
    const now = new Date();
    const [totals, referrals, team, daily, providers, providerDaily, invites, requests, payouts, period, history] =
      await Promise.all([
        getPartnerEarningsTotals(this.database, partner.id),
        listReferredUsers(this.database, partner.id),
        listPartnerTeam(this.database, partner.id),
        getPartnerDailyEarnings(this.database, partner.id, days.data),
        getPartnerEarningsByProvider(this.database, partner.id, days.data),
        getPartnerDailyEarningsByProvider(this.database, partner.id, days.data),
        listPartnerInvites(this.database, partner.id),
        listPartnerRequests(this.database, { requesterPartnerId: partner.id, limit: 100 }),
        listPartnerPayouts(this.database, partner.id),
        getPartnerPeriodState(this.database, partner.id, now),
        getPartnerPeriodHistory(this.database, partner.id, now),
      ]);
    return jsonSafe({
      state: "active",
      activated: resolution.activated,
      membership: partnerAccess(partner),
      totals,
      referrals: referrals.map((referral) => ({
        ...referral,
        userRef: referral.commerceUserId.slice(0, 8),
      })),
      team: team.filter((member) => member.commerceUserId !== null).map((member) => ({
        ...member,
        email: undefined,
        telegramUsername: undefined,
        displayName: undefined,
      })),
      earnings: { days: days.data, daily, providers, providerDaily },
      invitations: invites
        .filter((invite) => invite.commerceUserId !== null)
        .map((invite) => ({
          id: invite.id,
          commerceUserId: invite.commerceUserId,
          overrideBps: invite.parentOverrideBps,
          teamOverrideMaxBps: invite.teamOverrideMaxBps,
          teamInvitesEnabled: invite.teamInvitesEnabled,
          b2bEnabled: invite.b2bEnabled,
          b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
          b2bCanDelegate: invite.b2bCanDelegate,
          expiresAt: invite.expiresAt,
          consumedAt: invite.consumedAt,
          revokedAt: invite.revokedAt,
          createdAt: invite.createdAt,
        })),
      requests: requests.items.map((request) => partnerRequestView(
        request,
        null,
        { includeCommerceIdentity: true },
      )),
      payouts: payouts.map(payoutView),
      period,
      periodHistory: history,
      payoutPolicy: {
        minPayoutNano: (
          BigInt(this.config.get("SALES_MIN_PAYOUT_USD", { infer: true })) * 1_000_000_000n
        ).toString(),
        lockDays: 7,
        windowDays: 3,
      },
    });
  }

  @Post("partner/:commerceUserId/team-invitations")
  @HttpCode(201)
  async inviteTeamMember(
    @Param("commerceUserId") commerceUserId: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    const inviter = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = teamInviteSchema.safeParse(body);
    if (!inviter.success || !parsed.success) throw new BadRequestException("invalid Team invitation payload");
    try {
      const invitation = await createCommerceTeamInvite(this.database, {
        inviterCommerceUserId: inviter.data,
        inviteeCommerceUserId: parsed.data.inviteeCommerceUserId,
        defaultCommissionBps: this.config.get("DEFAULT_COMMISSION_BPS", { infer: true }),
        defaultSubCommissionBps: this.config.get("DEFAULT_SUB_COMMISSION_BPS", { infer: true }),
        overrideBps: parsed.data.overrideBps,
        authority: parsed.data.authority,
        expiresAt: new Date(Date.now() + 30 * 24 * 60 * 60 * 1_000),
      });
      return jsonSafe({ invitation });
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Delete("partner/:commerceUserId/team-invitations/:inviteId")
  async revokeTeamInvitation(
    @Param("commerceUserId") commerceUserId: string,
    @Param("inviteId") inviteId: string,
  ): Promise<unknown> {
    const inviter = commerceUserIdSchema.safeParse(commerceUserId);
    const invitation = uuidSchema.safeParse(inviteId);
    if (!inviter.success || !invitation.success) {
      throw new BadRequestException("invalid Team invitation revocation");
    }
    try {
      return jsonSafe({
        invitation: await revokeCommerceTeamInvite(this.database, {
          inviterCommerceUserId: inviter.data,
          inviteId: invitation.data,
        }),
      });
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Patch("partner/:commerceUserId/team/:memberCommerceUserId")
  async updateTeamMember(
    @Param("commerceUserId") commerceUserId: string,
    @Param("memberCommerceUserId") memberCommerceUserId: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    const parentId = commerceUserIdSchema.safeParse(commerceUserId);
    const memberId = commerceUserIdSchema.safeParse(memberCommerceUserId);
    const parsed = teamMemberPatchSchema.safeParse(body);
    if (!parentId.success || !memberId.success || !parsed.success) {
      throw new BadRequestException("invalid Team member update");
    }
    try {
      const [parent, member] = await Promise.all([
        findPartnerByCommerceUserId(this.database, parentId.data),
        findPartnerByCommerceUserId(this.database, memberId.data),
      ]);
      if (!parent || !parent.programEnabled || !member) throw new CommercePartnerNotFoundError();
      const authority = await updateDirectTeamMemberAuthority(this.database, {
        parentPartnerId: parent.id,
        memberId: member.id,
        ...(parsed.data.overrideBps === undefined ? {} : { overrideBps: parsed.data.overrideBps }),
        ...(parsed.data.teamOverrideMaxBps === undefined
          ? {}
          : { teamOverrideMaxBps: parsed.data.teamOverrideMaxBps }),
        ...(parsed.data.teamInvitesEnabled === undefined
          ? {}
          : { teamInvitesEnabled: parsed.data.teamInvitesEnabled }),
        ...(parsed.data.b2bEnabled === undefined ? {} : { b2bEnabled: parsed.data.b2bEnabled }),
        ...(parsed.data.b2bMaxDiscountBps === undefined
          ? {}
          : { b2bMaxDiscountBps: parsed.data.b2bMaxDiscountBps }),
        ...(parsed.data.b2bCanDelegate === undefined
          ? {}
          : { b2bCanDelegate: parsed.data.b2bCanDelegate }),
        requireProgramEnabled: true,
      });
      return { authority };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Post("partner/:commerceUserId/requests/commission")
  @HttpCode(201)
  async requestCommission(
    @Param("commerceUserId") commerceUserId: string,
    @Headers("idempotency-key") idempotencyKey: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = commissionRequestSchema.safeParse(body);
    const key = z.string().trim().min(8).max(200).safeParse(idempotencyKey);
    if (!id.success || !parsed.success || !key.success) {
      throw new BadRequestException("valid request payload and Idempotency-Key are required");
    }
    try {
      const partner = await findPartnerByCommerceUserId(this.database, id.data);
      if (!partner || !partner.programEnabled || partner.status !== "active") {
        throw new CommercePartnerNotFoundError();
      }
      const request = await createCommissionChangeRequest(this.database, {
        requesterPartnerId: partner.id,
        requestedCommissionBps: parsed.data.requestedCommissionBps,
        reason: parsed.data.reason,
        idempotencyKey: key.data,
        requireProgramEnabled: true,
      });
      return { request: partnerRequestView(request, null, { includeCommerceIdentity: true }) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Post("partner/:commerceUserId/requests/b2b")
  @HttpCode(201)
  async requestB2B(
    @Param("commerceUserId") commerceUserId: string,
    @Headers("idempotency-key") idempotencyKey: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = b2bRequestSchema.safeParse(body);
    const key = z.string().trim().min(8).max(200).safeParse(idempotencyKey);
    if (!id.success || !parsed.success || !key.success) {
      throw new BadRequestException("valid request payload and Idempotency-Key are required");
    }
    try {
      const partner = await findPartnerByCommerceUserId(this.database, id.data);
      if (!partner || !partner.programEnabled || partner.status !== "active") {
        throw new CommercePartnerNotFoundError();
      }
      const request = await createB2BPartnerRequest(this.database, {
        requesterPartnerId: partner.id,
        commerceUserId: parsed.data.referralCommerceUserId,
        requestType: parsed.data.requestType,
        requestedDiscountBps: parsed.data.requestedDiscountBps,
        providers: parsed.data.providers,
        reason: parsed.data.reason,
        stateSnapshot: parsed.data.stateSnapshot,
        idempotencyKey: key.data,
        requireProgramEnabled: true,
      });
      return { request: partnerRequestView(request, null, { includeCommerceIdentity: true }) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Post("partner/:commerceUserId/referrals/business-pricing")
  @HttpCode(200)
  async setBusinessPricing(
    @Param("commerceUserId") commerceUserId: string,
    @Headers("idempotency-key") idempotencyKey: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = businessPricingSchema.safeParse(body);
    const key = z.string().trim().min(8).max(200).safeParse(idempotencyKey);
    if (!id.success || !parsed.success || !key.success) {
      throw new BadRequestException("valid pricing payload and Idempotency-Key are required");
    }
    const partner = await findPartnerByCommerceUserId(this.database, id.data);
    if (!partner || !partner.programEnabled || partner.status !== "active") {
      throw new NotFoundException("active Commerce partner membership not found");
    }
    if (!partner.b2bEnabled) throw new ForbiddenException("B2B self-service is disabled for this partner");
    const maximumPercent = Math.floor(partner.b2bMaxDiscountBps / 100);
    const requested = [
      ...(parsed.data.discountPercent === undefined ? [] : [parsed.data.discountPercent]),
      ...Object.values(parsed.data.providers ?? {}).filter((value): value is number => value !== null),
    ];
    if (requested.some((value) => value > maximumPercent)) {
      throw new UnprocessableEntityException(`discount exceeds the partner ceiling of ${maximumPercent}%`);
    }
    const owner = await getReferredUserPartner(this.database, parsed.data.referralCommerceUserId);
    if (owner !== partner.id) throw new NotFoundException("referral not found for this partner");
    try {
      const result = await this.commerce.setPartnerBusinessPricing({
        operationRef: `commerce-partner-direct:${createHash("sha256")
          .update(`${partner.id}:${key.data}`)
          .digest("hex")}`,
        userId: parsed.data.referralCommerceUserId,
        referralCode: partner.referralCode,
        ceilingPercent: maximumPercent,
        ...(parsed.data.discountPercent === undefined
          ? {}
          : { discountPercent: parsed.data.discountPercent }),
        ...(parsed.data.providers === undefined ? {} : { providers: parsed.data.providers }),
        actorId: partner.id,
        reason: "Commerce Dashboard partner self-service B2B pricing",
      });
      if (!result.idempotentReplay) {
        await insertSalesAudit(this.database, {
          actorType: "partner",
          actorId: partner.id,
          action: "commerce_referral.business_pricing_set",
          targetType: "referred_user",
          targetId: parsed.data.referralCommerceUserId,
          metadata: {
            operationRef: result.operationRef,
            ceilingPercent: maximumPercent,
            discountPercent: result.discountPercent,
            providers: result.providers,
          },
        });
      }
      return { ...result, ceilingPercent: maximumPercent };
    } catch (error) {
      if (error instanceof CommercePartnerPricingError && error.status === 403) {
        throw new ForbiddenException("Commerce rejected referral ownership or authority");
      }
      if (error instanceof CommercePartnerPricingError && error.status === 400) {
        throw new UnprocessableEntityException("this referral cannot be converted yet");
      }
      if (error instanceof CommercePartnerPricingError && error.status === 409) {
        throw new ConflictException("Idempotency-Key was already used for another pricing request");
      }
      throw error;
    }
  }

  @Patch("partner/:commerceUserId/wallet")
  async updateWallet(
    @Param("commerceUserId") commerceUserId: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = walletSchema.safeParse(body);
    if (!id.success || !parsed.success) {
      throw new BadRequestException("invalid BSC address: expected 0x + 40 hex characters");
    }
    try {
      const updated = await updateCommercePartnerWallet(this.database, {
        commerceUserId: id.data,
        address: parsed.data.address,
      });
      return { membership: partnerAccess(updated) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Post("admin/partners")
  @HttpCode(200)
  async onboard(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const parsed = onboardingSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner onboarding payload");
    try {
      const result = await onboardCommercePartner(this.database, {
        commerceUserId: parsed.data.commerceUserId,
        commissionBps: parsed.data.commissionBps,
        defaultSubCommissionBps: this.config.get("DEFAULT_SUB_COMMISSION_BPS", { infer: true }),
        authority: parsed.data.authority,
        actorId: actorId(actorHeader),
      });
      return { created: result.created, membership: partnerAccess(result.partner) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Patch("admin/partners/:commerceUserId")
  async patchPartner(
    @Param("commerceUserId") commerceUserId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const id = commerceUserIdSchema.safeParse(commerceUserId);
    const parsed = adminPatchSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid partner update");
    try {
      const partner = await findPartnerByCommerceUserId(this.database, id.data);
      if (!partner) throw new CommercePartnerNotFoundError();
      const updated = await updatePartnerAdmin(this.database, partner.id, {
        ...(parsed.data.commissionBps === undefined ? {} : { commissionBps: parsed.data.commissionBps }),
        ...(parsed.data.teamOverrideMaxBps === undefined
          ? {}
          : { teamOverrideMaxBps: parsed.data.teamOverrideMaxBps }),
        ...(parsed.data.teamInvitesEnabled === undefined
          ? {}
          : { teamInvitesEnabled: parsed.data.teamInvitesEnabled }),
        ...(parsed.data.b2bEnabled === undefined ? {} : { b2bEnabled: parsed.data.b2bEnabled }),
        ...(parsed.data.b2bMaxDiscountBps === undefined
          ? {}
          : { b2bMaxDiscountBps: parsed.data.b2bMaxDiscountBps }),
        ...(parsed.data.b2bCanDelegate === undefined
          ? {}
          : { b2bCanDelegate: parsed.data.b2bCanDelegate }),
        ...(parsed.data.status === undefined ? {} : { status: parsed.data.status }),
        ...(parsed.data.programEnabled === undefined
          ? {}
          : { programEnabled: parsed.data.programEnabled }),
        actorId: actorId(actorHeader),
      });
      if (!updated) throw new CommercePartnerNotFoundError();
      const current = await findPartnerByCommerceUserId(this.database, id.data);
      if (!current) throw new CommercePartnerNotFoundError();
      return { membership: partnerAccess(current) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Get("admin/partners")
  @Header("Cache-Control", "no-store")
  async partners(): Promise<unknown> {
    const partners = (await listPartnersWithAggregates(this.database))
      .filter((partner) => partner.commerceUserId !== null);
    return jsonSafe({
      items: partners.map((partner) => ({
        partnerId: partner.id,
        commerceUserId: partner.commerceUserId,
        programEnabled: partner.programEnabled,
        programStartedAt: partner.programStartedAt,
        status: partner.status,
        referralCode: partner.referralCode,
        commissionBps: partner.commissionBps,
        teamOverrideMaxBps: partner.teamOverrideMaxBps,
        teamShareBps: partner.parentOverrideBps,
        parentPartnerId: partner.parentPartnerId,
        referredUsers: partner.referredUsers,
        teamSize: partner.teamSize,
        earnedNano: partner.earnedNano,
        adjustmentNano: partner.adjustmentNano,
        netNano: partner.netNano,
        debtNano: partner.debtNano,
        payableNano: partner.payableNano,
        paidNano: partner.paidNano,
        b2bEnabled: partner.b2bEnabled,
        b2bMaxDiscountBps: partner.b2bMaxDiscountBps,
        teamInvitesEnabled: partner.teamInvitesEnabled,
        b2bCanDelegate: partner.b2bCanDelegate,
        createdAt: partner.createdAt,
      })),
    });
  }

  @Get("admin/requests")
  @Header("Cache-Control", "no-store")
  async adminRequests(@Query() query: unknown): Promise<unknown> {
    const parsed = requestQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid requests query");
    const before = decodePartnerRequestCursor(parsed.data.cursor);
    if (parsed.data.cursor !== undefined && before === undefined) {
      throw new BadRequestException("invalid requests cursor");
    }
    const page = await listPartnerRequests(this.database, {
      commerceProgramOnly: true,
      ...(parsed.data.status ? { status: parsed.data.status } : {}),
      ...(parsed.data.requestType ? { requestType: parsed.data.requestType } : {}),
      ...(before ? { before } : {}),
      limit: parsed.data.limit,
    });
    return {
      items: page.items.map((request) => partnerRequestView(
        request,
        null,
        { includeCommerceIdentity: true },
      )),
      nextCursor: encodePartnerRequestCursor(page.nextCursor),
    };
  }

  @Post("admin/requests/:requestId/decision")
  @HttpCode(200)
  async decideRequest(
    @Param("requestId") requestId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const id = uuidSchema.safeParse(requestId);
    const parsed = adminDecisionSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid request decision");
    try {
      const request = await decidePartnerRequest(this.database, {
        requestId: id.data,
        action: parsed.data.action,
        reviewerActor: actorId(actorHeader),
        reviewerNote: parsed.data.note,
        ...(parsed.data.action === "approve" && parsed.data.commissionBps !== undefined
          ? { approvedCommissionBps: parsed.data.commissionBps }
          : {}),
        ...(parsed.data.action === "approve" && parsed.data.discountBps !== undefined
          ? { approvedDiscountBps: parsed.data.discountBps }
          : {}),
        ...(parsed.data.action === "approve" && parsed.data.providers !== undefined
          ? { providers: parsed.data.providers }
          : {}),
        requireCommerceProgram: true,
      });
      return { request: partnerRequestView(request, null, { includeCommerceIdentity: true }) };
    } catch (error) {
      translateKnownError(error);
    }
  }

  @Get("admin/payouts")
  @Header("Cache-Control", "no-store")
  async adminPayouts(@Query() query: unknown): Promise<unknown> {
    const parsed = payoutQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid payouts query");
    const [payouts, partners] = await Promise.all([
      listPayouts(this.database, parsed.data.status),
      listPartnersWithAggregates(this.database),
    ]);
    const commercePartnerIds = new Set(partners
      .filter((partner) => partner.commerceUserId !== null)
      .map((partner) => partner.id));
    return { items: payouts.filter((payout) => commercePartnerIds.has(payout.partnerId)).map(payoutView) };
  }

  @Post("admin/payouts/:payoutId/decision")
  @HttpCode(200)
  async decideLegacyPayout(
    @Param("payoutId") payoutId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const id = uuidSchema.safeParse(payoutId);
    const parsed = payoutDecisionSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid payout decision");
    try {
      const payout = await decidePayout(this.database, {
        payoutId: id.data,
        decision: parsed.data.action,
        note: parsed.data.note,
        actorId: actorId(actorHeader),
      });
      return { payout: payoutView(payout) };
    } catch (error) {
      translateKnownError(error);
    }
  }
}
