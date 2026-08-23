import {
  BadRequestException,
  Body,
  Controller,
  Delete,
  Get,
  Header,
  Headers,
  HttpCode,
  Param,
  Patch,
  Post,
  Query,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import { AdminGuard } from "./admin.guard.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { ReferralService } from "./referral.service.js";

const uuid = z.string().uuid();
const email = z.string().trim().email().max(320);
const idempotencyKey = z.string().trim().min(8).max(200);
const bps = (maximum: number) => z.number().int().min(0).max(maximum);
const providerId = z.enum(["anthropic", "openai", "google", "kimi", "glm"]);
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
const onboardingSchema = z.object({
  email,
  commissionBps: bps(10_000).default(1_000),
  authority: authoritySchema,
}).strict();
const onboardingByIdSchema = onboardingSchema.omit({ email: true });
const partnerPatchSchema = z.object({
  email,
  commissionBps: bps(10_000).optional(),
  teamOverrideMaxBps: bps(2_000).optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: bps(9_500).optional(),
  b2bCanDelegate: z.boolean().optional(),
  status: z.enum(["active", "suspended", "pending"]).optional(),
  programEnabled: z.boolean().optional(),
}).strict().refine((value) => Object.keys(value).some((key) => key !== "email"), {
  message: "at least one partner setting is required",
}).refine(
  (value) => value.b2bEnabled !== false
    || (value.b2bMaxDiscountBps === 0 && value.b2bCanDelegate === false),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const teamInvitationSchema = z.object({
  email,
  overrideBps: bps(2_000),
  authority: authoritySchema,
}).strict();
const teamMemberPatchSchema = z.object({
  email,
  overrideBps: bps(2_000).optional(),
  teamOverrideMaxBps: bps(2_000).optional(),
  teamInvitesEnabled: z.boolean().optional(),
  b2bEnabled: z.boolean().optional(),
  b2bMaxDiscountBps: bps(9_500).optional(),
  b2bCanDelegate: z.boolean().optional(),
}).strict().refine((value) => Object.keys(value).some((key) => key !== "email"), {
  message: "at least one Team setting is required",
}).refine(
  (value) => value.b2bEnabled !== false
    || (value.b2bMaxDiscountBps === 0 && value.b2bCanDelegate === false),
  "a disabled B2B grant cannot retain a ceiling or delegation",
);
const commissionRequestSchema = z.object({
  requestedCommissionBps: bps(10_000),
  reason: z.string().trim().min(1).max(4_000),
}).strict();
const b2bRequestSchema = z.object({
  customerEmail: email,
  requestType: z.enum(["b2b_conversion", "b2b_pricing"]),
  requestedDiscountBps: bps(9_500).refine((value) => value % 100 === 0),
  providers: z.record(providerId, bps(9_500).refine((value) => value % 100 === 0).nullable()).default({}),
  reason: z.string().trim().min(1).max(4_000),
}).strict();
const businessPricingSchema = z.object({
  customerEmail: email,
  discountPercent: z.number().int().min(0).max(95).optional(),
  providers: z.record(providerId, z.number().int().min(0).max(95).nullable()).optional(),
}).strict().refine(
  (value) => value.discountPercent !== undefined
    || (value.providers !== undefined && Object.keys(value.providers).length > 0),
  "at least one pricing value is required",
);
const walletSchema = z.object({ address: z.string().regex(/^0x[a-fA-F0-9]{40}$/) }).strict();
const requestQuerySchema = z.object({
  status: z.enum(["pending", "approved", "rejected", "applied", "apply_failed"]).optional(),
  requestType: z.enum(["b2b_conversion", "b2b_pricing", "commission_change"]).optional(),
  cursor: z.string().max(512).optional(),
  limit: z.coerce.number().int().min(1).max(100).default(25),
});
const decisionSchema = z.discriminatedUnion("action", [
  z.object({ action: z.literal("reject"), note: z.string().trim().min(1).max(4_000) }).strict(),
  z.object({
    action: z.literal("approve"),
    note: z.string().trim().min(1).max(4_000),
    commissionBps: bps(10_000).optional(),
    discountBps: bps(9_500).optional(),
    providers: z.record(providerId, bps(9_500).nullable()).optional(),
  }).strict(),
]);
const payoutQuerySchema = z.object({
  status: z.enum(["requested", "approved", "paid", "rejected"]).optional(),
});
const payoutDecisionSchema = z.object({
  action: z.literal("reject"),
  note: z.string().trim().min(1).max(2_000),
}).strict();

@Controller("referral")
@UseGuards(SessionAuthGuard)
export class ReferralController {
  constructor(private readonly referral: ReferralService) {}

  @Get()
  @Header("Cache-Control", "no-store")
  snapshot(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.referral.partnerSnapshot(current.user.id, current.user.email);
  }

  @Post("team-invitations")
  @HttpCode(201)
  inviteTeamMember(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = teamInvitationSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid Team invitation");
    return this.referral.inviteTeamMember(current.user.id, parsed.data);
  }

  @Delete("team-invitations/:inviteId")
  revokeTeamInvitation(@CurrentAuth() current: RequestAuth, @Param("inviteId") inviteId: string): Promise<unknown> {
    if (!uuid.safeParse(inviteId).success) throw new BadRequestException("invalid Team invitation ID");
    return this.referral.revokeTeamInvitation(current.user.id, inviteId);
  }

  @Patch("team")
  updateTeamMember(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = teamMemberPatchSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid Team member update");
    const value = parsed.data;
    return this.referral.updateTeamMember(current.user.id, {
      email: value.email,
      ...(value.overrideBps === undefined ? {} : { overrideBps: value.overrideBps }),
      ...(value.teamOverrideMaxBps === undefined ? {} : { teamOverrideMaxBps: value.teamOverrideMaxBps }),
      ...(value.teamInvitesEnabled === undefined ? {} : { teamInvitesEnabled: value.teamInvitesEnabled }),
      ...(value.b2bEnabled === undefined ? {} : { b2bEnabled: value.b2bEnabled }),
      ...(value.b2bMaxDiscountBps === undefined ? {} : { b2bMaxDiscountBps: value.b2bMaxDiscountBps }),
      ...(value.b2bCanDelegate === undefined ? {} : { b2bCanDelegate: value.b2bCanDelegate }),
    });
  }

  @Post("requests/commission")
  @HttpCode(201)
  requestCommission(
    @CurrentAuth() current: RequestAuth,
    @Headers("idempotency-key") key: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const parsed = commissionRequestSchema.safeParse(body);
    const parsedKey = idempotencyKey.safeParse(key);
    if (!parsed.success || !parsedKey.success) throw new BadRequestException("valid request and Idempotency-Key are required");
    return this.referral.requestCommission(
      current.user.id,
      current.user.email,
      { ...parsed.data, idempotencyKey: parsedKey.data },
    );
  }

  @Post("requests/b2b")
  @HttpCode(201)
  requestB2B(
    @CurrentAuth() current: RequestAuth,
    @Headers("idempotency-key") key: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const parsed = b2bRequestSchema.safeParse(body);
    const parsedKey = idempotencyKey.safeParse(key);
    if (!parsed.success || !parsedKey.success) throw new BadRequestException("valid request and Idempotency-Key are required");
    return this.referral.requestB2B(
      current.user.id,
      current.user.email,
      { ...parsed.data, idempotencyKey: parsedKey.data },
    );
  }

  @Post("referrals/business-pricing")
  @HttpCode(200)
  setBusinessPricing(
    @CurrentAuth() current: RequestAuth,
    @Headers("idempotency-key") key: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const parsed = businessPricingSchema.safeParse(body);
    const parsedKey = idempotencyKey.safeParse(key);
    if (!parsed.success || !parsedKey.success) throw new BadRequestException("valid pricing and Idempotency-Key are required");
    return this.referral.setBusinessPricing(current.user.id, {
      customerEmail: parsed.data.customerEmail,
      idempotencyKey: parsedKey.data,
      ...(parsed.data.discountPercent === undefined ? {} : { discountPercent: parsed.data.discountPercent }),
      ...(parsed.data.providers === undefined ? {} : { providers: parsed.data.providers }),
    });
  }

  @Patch("wallet")
  updateWallet(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = walletSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid BSC wallet address");
    return this.referral.updateWallet(current.user.id, parsed.data.address);
  }
}

@Controller("admin/referral")
@UseGuards(AdminGuard)
export class AdminReferralController {
  constructor(private readonly referral: ReferralService) {}

  @Get("partners")
  @Header("Cache-Control", "no-store")
  partners(): Promise<unknown> {
    return this.referral.adminPartners();
  }

  @Post("partners")
  @HttpCode(200)
  onboard(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const parsed = onboardingSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner onboarding");
    return this.referral.onboardByEmail({ ...parsed.data, actor: adminActor(actorHeader) });
  }

  @Patch("partners")
  updatePartner(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const parsed = partnerPatchSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner update");
    const { email: partnerEmail, ...patch } = parsed.data;
    return this.referral.updatePartner({ email: partnerEmail, patch, actor: adminActor(actorHeader) });
  }

  @Get("requests")
  @Header("Cache-Control", "no-store")
  requests(@Query() query: unknown): Promise<unknown> {
    const parsed = requestQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid partner request filters");
    const params = new URLSearchParams({ limit: String(parsed.data.limit) });
    if (parsed.data.status) params.set("status", parsed.data.status);
    if (parsed.data.requestType) params.set("requestType", parsed.data.requestType);
    if (parsed.data.cursor) params.set("cursor", parsed.data.cursor);
    return this.referral.adminRequests(`?${params}`);
  }

  @Post("requests/:requestId/decision")
  @HttpCode(200)
  decideRequest(
    @Param("requestId") requestId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const id = uuid.safeParse(requestId);
    const parsed = decisionSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid partner request decision");
    return this.referral.decideRequest(id.data, parsed.data, adminActor(actorHeader));
  }

  @Get("payouts")
  @Header("Cache-Control", "no-store")
  payouts(@Query() query: unknown): Promise<unknown> {
    const parsed = payoutQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid payout filters");
    const params = new URLSearchParams();
    if (parsed.data.status) params.set("status", parsed.data.status);
    return this.referral.adminPayouts(params.size ? `?${params}` : "");
  }

  @Post("payouts/:payoutId/decision")
  @HttpCode(200)
  decidePayout(
    @Param("payoutId") payoutId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const id = uuid.safeParse(payoutId);
    const parsed = payoutDecisionSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid payout decision");
    return this.referral.decidePayout(id.data, parsed.data, adminActor(actorHeader));
  }
}

@Controller("admin/users")
@UseGuards(AdminGuard)
export class AdminUserReferralController {
  constructor(private readonly referral: ReferralService) {}

  @Post(":id/referral-partner")
  @HttpCode(200)
  onboardUser(
    @Param("id") userId: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader: string | undefined,
  ): Promise<unknown> {
    const id = uuid.safeParse(userId);
    const parsed = onboardingByIdSchema.safeParse(body);
    if (!id.success || !parsed.success) throw new BadRequestException("invalid partner onboarding");
    return this.referral.onboardByUserId({
      userId: id.data,
      ...parsed.data,
      actor: adminActor(actorHeader),
    });
  }
}

function adminActor(value: string | undefined): string {
  const actor = value?.trim();
  return actor ? actor.slice(0, 200) : "admin-panel";
}
