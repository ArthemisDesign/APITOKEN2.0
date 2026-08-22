import { createHash, randomBytes } from "node:crypto";
import {
  BadRequestException,
  Body,
  Controller,
  ConflictException,
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
  ServiceUnavailableException,
  UnauthorizedException,
  UnprocessableEntityException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  countPartnerTeam,
  countReferredUsers,
  createPartnerInvite,
  getPartnerDailyEarnings,
  getPartnerEarningsByProvider,
  getPartnerDailyEarningsByProvider,
  getPartnerEarningsTotals,
  listPartnerInvites,
  listPartnerPayouts,
  listPartnerTeam,
  updateDirectTeamMemberAuthority,
  listReferredUsers,
  resolveReferredUserByPrefix,
  getPartnerPeriodState,
  getPartnerPeriodHistory,
  updatePartnerSettings,
  createPromoCode,
  listPartnerPromoCodes,
  disablePromoCode,
  createDiscountLink,
  insertSalesAudit,
  listDiscountLinks,
  deleteDiscountLink,
  DiscountLinkCollisionError,
  DiscountLinkNotAllowedError,
  InviteCodeCollisionError,
  PromoNotAllowedError,
  PromoLimitError,
  PromoCodeCollisionError,
  TeamMemberNotFoundError,
  TeamOverrideLimitError,
  PartnerB2BAuthorityError,
  PartnerTeamAuthorityError,
  PartnerRequestConflictError,
  PartnerRequestNotFoundError,
  PartnerRequestValidationError,
  createB2BPartnerRequest,
  createCommissionChangeRequest,
  listPartnerRequests,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { AuthService, generateCode, partnerView } from "./auth.service.js";
import { normalizeTelegramUsername } from "./telegram.js";
import { CommercePartnerPricingError, CommerceService } from "./commerce.service.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import {
  createDiscountLinkSchema,
  b2bPartnerRequestSchema,
  commissionChangeRequestSchema,
  createInviteSchema,
  createPromoSchema,
  earningsQuerySchema,
  idempotencyKeySchema,
  partnerBusinessPricingSchema,
  partnerRequestsQuerySchema,
  referralUserRefSchema,
  setReferralDiscountSchema,
  teamMemberControlsSchema,
  updateSettingsSchema,
  walletSchema,
} from "./schemas.js";
import {
  decodePartnerRequestCursor,
  encodePartnerRequestCursor,
  partnerRequestView as serializePartnerRequest,
} from "./partner-request-view.js";

const INVITE_TTL_DAYS = 30;
const uuidSchema = z.string().uuid();
const PAYOUT_METHOD = "usdt-bep20";
const PROMO_ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // без похожих 0O1I

function generatePromoCode(): string {
  const bytes = randomBytes(8);
  let code = "";
  for (let i = 0; i < 8; i += 1) code += PROMO_ALPHABET[bytes[i]! % PROMO_ALPHABET.length];
  return code;
}

/** Адрес из привязанного кошелька партнёра (payout_details.address, сеть BSC). */
function boundWallet(partner: { payoutMethod: string | null; payoutDetails: unknown }): string | null {
  if (partner.payoutMethod !== PAYOUT_METHOD) return null;
  const details = partner.payoutDetails;
  if (details === null || typeof details !== "object" || !("address" in details)) return null;
  const address = (details as { address: unknown }).address;
  return typeof address === "string" && /^0x[a-fA-F0-9]{40}$/.test(address) ? address : null;
}

@Controller("partner")
@UseGuards(SessionAuthGuard)
export class PartnerController {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
    private readonly commerce: CommerceService,
    private readonly auth: AuthService,
  ) {}

  @Get("overview")
  @Header("Cache-Control", "no-store")
  async overview(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const partnerId = current.partner.id;
    const [totals, referredUsers, teamSize] = await Promise.all([
      getPartnerEarningsTotals(this.database, partnerId),
      countReferredUsers(this.database, partnerId),
      countPartnerTeam(this.database, partnerId),
    ]);
    const mainSite = this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true });
    return {
      referralCode: current.partner.referralCode,
      // Ведём на главную, а не в /register: код ловится глобально (apps/web RefCapture)
      // и доживает до регистрации 30 дней.
      referralUrl: `${new URL(mainSite).origin}/?ref=${current.partner.referralCode}`,
      commissionBps: current.partner.commissionBps,
      // Legacy marker fields remain additive API history. The explicit boolean prevents a client
      // from mistaking them for a price authority.
      referralDiscountEnabled: current.partner.referralDiscountEnabled,
      referralDiscountBps: current.partner.referralDiscountBps,
      referralPricingAffected: false,
      subCommissionBps: current.partner.subCommissionBps,
      teamOverrideMaxBps: current.partner.teamOverrideMaxBps,
      teamInvitesEnabled: current.partner.teamInvitesEnabled,
      b2bEnabled: current.partner.b2bEnabled,
      b2bMaxDiscountBps: current.partner.b2bMaxDiscountBps,
      b2bCanDelegate: current.partner.b2bCanDelegate,
      referredUsers,
      teamSize,
      totals: {
        earnedNano: totals.earnedNano.toString(),
        directNano: totals.directNano.toString(),
        overrideNano: totals.overrideNano.toString(),
        adjustmentNano: totals.adjustmentNano.toString(),
        directAdjustmentNano: totals.directAdjustmentNano.toString(),
        overrideAdjustmentNano: totals.overrideAdjustmentNano.toString(),
        netNano: totals.netNano.toString(),
        directNetNano: totals.directNetNano.toString(),
        overrideNetNano: totals.overrideNetNano.toString(),
        paidNano: totals.paidNano.toString(),
        pendingPayoutNano: totals.pendingPayoutNano.toString(),
        debtNano: totals.debtNano.toString(),
        availableNano: totals.availableNano.toString(),
      },
      last30d: {
        spendNano: totals.last30dSpendNano.toString(),
        earnedNano: totals.last30dEarnedNano.toString(),
        adjustmentNano: totals.last30dAdjustmentNano.toString(),
        netNano: totals.last30dNetNano.toString(),
      },
    };
  }

  @Get("referrals")
  @Header("Cache-Control", "no-store")
  async referrals(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const referrals = await listReferredUsers(this.database, current.partner.id);
    // Enrich with authoritative Commerce/engine type, actual discount and balance. The separate
    // referral marker is legacy metadata and never changes the actual discount. Best-effort:
    // при недоступности commerce карта пуста и строки показывают только локальные поля.
    const profiles = await this.commerce.referralProfiles(referrals.map((r) => r.commerceUserId));
    return {
      items: referrals.map((referral) => {
        const profile = profiles.get(referral.commerceUserId);
        return {
          // Commerce identities stay masked: only a short uuid prefix is exposed to partners.
          userMask: `user-${referral.commerceUserId.slice(0, 8)}…`,
          // Stable masked machine reference retained for the expand-only API.
          userRef: referral.commerceUserId.slice(0, 8),
          email: profile?.email ?? null,
          attributedAt: referral.attributedAt.toISOString(),
          spendNano: referral.spendNano.toString(),
          earnedNano: referral.earnedNano.toString(),
          adjustmentNano: referral.adjustmentNano.toString(),
          netNano: referral.netNano.toString(),
          topupNano: referral.topupNano.toString(),
          // Коммерческие поля (могут отсутствовать, если commerce недоступен).
          customerType: profile?.customerType ?? null,
          discountPercent: profile?.discountPercent ?? null,
          referralFloorBps: profile?.referralFloorBps ?? null,
          balanceNano: profile?.balanceNano ?? null,
          status: profile?.status ?? null,
        };
      }),
    };
  }

  @Post("requests/commission")
  @HttpCode(201)
  async requestCommissionChange(
    @CurrentAuth() current: RequestAuth,
    @Headers("idempotency-key") idempotencyHeader: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const parsed = commissionChangeRequestSchema.safeParse(body ?? {});
    const idempotency = idempotencyKeySchema.safeParse(idempotencyHeader);
    if (!parsed.success || !idempotency.success) {
      throw new BadRequestException("valid request data and Idempotency-Key are required");
    }
    try {
      const request = await createCommissionChangeRequest(this.database, {
        requesterPartnerId: current.partner.id,
        requestedCommissionBps: parsed.data.requestedCommissionBps,
        reason: parsed.data.reason,
        idempotencyKey: idempotency.data,
      });
      return { request: serializePartnerRequest(request, null) };
    } catch (error) {
      throwPartnerRequestError(error);
    }
  }

  @Post("referrals/:userRef/b2b-requests")
  @HttpCode(201)
  async requestReferralB2B(
    @CurrentAuth() current: RequestAuth,
    @Param("userRef") userRef: string,
    @Headers("idempotency-key") idempotencyHeader: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    const ref = referralUserRefSchema.safeParse(userRef);
    const parsed = b2bPartnerRequestSchema.safeParse(body ?? {});
    const idempotency = idempotencyKeySchema.safeParse(idempotencyHeader);
    if (!ref.success || !parsed.success || !idempotency.success) {
      throw new BadRequestException("valid request data and Idempotency-Key are required");
    }
    const commerceUserId = await resolveReferredUserByPrefix(this.database, current.partner.id, ref.data);
    if (commerceUserId === null) throw new NotFoundException("referral not found");
    if (commerceUserId === "ambiguous") throw new UnprocessableEntityException("ambiguous referral reference");
    const profile = (await this.commerce.referralProfiles([commerceUserId])).get(commerceUserId);
    if (!profile) {
      throw new ServiceUnavailableException("Commerce profile is temporarily unavailable");
    }
    try {
      const request = await createB2BPartnerRequest(this.database, {
        requesterPartnerId: current.partner.id,
        commerceUserId,
        requestType: profile.customerType === "b2b" ? "b2b_pricing" : "b2b_conversion",
        requestedDiscountBps: parsed.data.discountPercent * 100,
        providers: Object.fromEntries(Object.entries(parsed.data.providers ?? {})
          .map(([providerId, percent]) => [providerId, percent === null ? null : percent * 100])),
        reason: parsed.data.reason,
        stateSnapshot: {
          customerType: profile.customerType,
          discountPercent: profile.discountPercent,
        },
        idempotencyKey: idempotency.data,
      });
      return { request: serializePartnerRequest(request, profile.email) };
    } catch (error) {
      throwPartnerRequestError(error);
    }
  }

  @Get("requests")
  @Header("Cache-Control", "no-store")
  async requests(@CurrentAuth() current: RequestAuth, @Query() query: unknown): Promise<unknown> {
    const parsed = partnerRequestsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid requests query");
    const before = decodePartnerRequestCursor(parsed.data.cursor);
    if (parsed.data.cursor !== undefined && before === undefined) {
      throw new BadRequestException("invalid requests cursor");
    }
    const page = await listPartnerRequests(this.database, {
      requesterPartnerId: current.partner.id,
      ...(parsed.data.status === undefined ? {} : { status: parsed.data.status }),
      ...(parsed.data.requestType === undefined ? {} : { requestType: parsed.data.requestType }),
      ...(before === undefined ? {} : { before }),
      limit: parsed.data.limit,
    });
    const profiles = await this.commerce.referralProfiles(page.items.flatMap((request) =>
      request.commerceUserId === null ? [] : [request.commerceUserId]));
    return {
      items: page.items.map((request) => serializePartnerRequest(
        request,
        request.commerceUserId === null ? null : (profiles.get(request.commerceUserId)?.email ?? null),
      )),
      nextCursor: encodePartnerRequestCursor(page.nextCursor),
    };
  }

  /**
   * Expand-only legacy marker writer. It never changes the referral's scalar/provider price;
   * `pricingAffected:false` is returned explicitly so API consumers cannot infer otherwise.
   */
  @Post("referrals/:userRef/discount")
  @HttpCode(200)
  async setReferralDiscount(
    @CurrentAuth() current: RequestAuth,
    @Param("userRef") userRef: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    if (!current.partner.referralDiscountEnabled) {
      throw new ForbiddenException("legacy referral marker is not enabled for this account");
    }
    const ref = referralUserRefSchema.safeParse(userRef);
    if (!ref.success) throw new BadRequestException("invalid referral reference");
    const parsed = setReferralDiscountSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid legacy referral marker");
    // Preserve the historical per-partner marker ceiling.
    if (parsed.data.discountBps > current.partner.referralDiscountBps) {
      throw new UnprocessableEntityException("marker exceeds your allowed maximum");
    }
    const commerceUserId = await resolveReferredUserByPrefix(this.database, current.partner.id, ref.data);
    if (commerceUserId === null) throw new NotFoundException("referral not found");
    if (commerceUserId === "ambiguous") throw new UnprocessableEntityException("ambiguous referral reference");
    const result = await this.commerce.setReferralDiscount(commerceUserId, parsed.data.discountBps, "sales-partner");
    // applied=false with multiplierBp=null means a non-B2C or missing profile.
    if (!result.applied && result.multiplierBp === null) {
      throw new UnprocessableEntityException("this referral cannot store the legacy B2C marker");
    }
    await insertSalesAudit(this.database, {
      actorType: "partner", actorId: current.partner.id,
      action: "referral.discount_set", targetType: "referred_user", targetId: commerceUserId,
      metadata: { discountBps: parsed.data.discountBps, multiplierBp: result.multiplierBp },
    });
    return {
      userRef: ref.data,
      discountBps: parsed.data.discountBps,
      multiplierBp: result.multiplierBp,
      pricingAffected: false,
    };
  }

  /**
   * Price one of MY referrals as a B2B customer — available only to a partner an admin granted the
   * right, and never deeper than the granted ceiling.
   *
   * Three things are checked here, and commerce independently re-checks the last two: the grant
   * exists, every requested discount is within the ceiling, and the referral is actually this
   * partner's. Deeper discounts are margin the company gives away, so the ceiling is the whole
   * safety property — it is enforced server-side and is never taken from the request.
   *
   * Commission is unaffected: a B2B referral earns the partner the same percentage of the
   * customer's own money, so a deeper discount simply means a smaller absolute commission.
   */
  @Post("referrals/:userRef/business-pricing")
  @HttpCode(200)
  async setReferralBusinessPricing(
    @CurrentAuth() current: RequestAuth,
    @Param("userRef") userRef: string,
    @Body() body: unknown,
    @Headers("idempotency-key") idempotencyHeader?: string,
  ): Promise<unknown> {
    if (!current.partner.b2bEnabled) {
      throw new ForbiddenException("your account is not allowed to create B2B customers");
    }
    const ref = referralUserRefSchema.safeParse(userRef);
    if (!ref.success) throw new BadRequestException("invalid referral reference");
    const parsed = partnerBusinessPricingSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid business pricing request");
    const idempotency = idempotencyHeader === undefined
      ? undefined
      : idempotencyKeySchema.safeParse(idempotencyHeader);
    if (idempotency !== undefined && !idempotency.success) {
      throw new BadRequestException("invalid Idempotency-Key");
    }

    const ceilingPercent = Math.floor(current.partner.b2bMaxDiscountBps / 100);
    const requested = [
      ...(parsed.data.discountPercent === undefined ? [] : [parsed.data.discountPercent]),
      ...Object.values(parsed.data.providers ?? {}).filter((value): value is number => value !== null),
    ];
    if (requested.some((value) => value > ceilingPercent)) {
      throw new UnprocessableEntityException(`discount exceeds your maximum of ${ceilingPercent}%`);
    }

    const commerceUserId = await resolveReferredUserByPrefix(this.database, current.partner.id, ref.data);
    if (commerceUserId === null) throw new NotFoundException("referral not found");
    if (commerceUserId === "ambiguous") throw new UnprocessableEntityException("ambiguous referral reference");

    let result;
    try {
      result = await this.commerce.setPartnerBusinessPricing({
        ...(idempotency?.success ? {
          operationRef: `partner-direct:${createHash("sha256")
            .update(`${current.partner.id}:${idempotency.data}`)
            .digest("hex")}`,
        } : {}),
        userId: commerceUserId,
        referralCode: current.partner.referralCode,
        ceilingPercent,
        ...(parsed.data.discountPercent === undefined ? {} : { discountPercent: parsed.data.discountPercent }),
        ...(parsed.data.providers === undefined ? {} : { providers: parsed.data.providers }),
        actorId: current.partner.id,
        reason: "Partner self-service B2B pricing",
      });
    } catch (error) {
      if (error instanceof CommercePartnerPricingError && error.status === 403) {
        // Commerce disagreed about ownership or the ceiling. Surface it rather than retrying:
        // the two sides must not quietly settle on the more generous reading.
        throw new ForbiddenException("this referral cannot be priced by your account");
      }
      if (error instanceof CommercePartnerPricingError && error.status === 400) {
        throw new UnprocessableEntityException("this referral cannot be converted yet");
      }
      if (error instanceof CommercePartnerPricingError && error.status === 409) {
        throw new ConflictException("Idempotency-Key was already used for another pricing request");
      }
      throw error;
    }

    if (!result.idempotentReplay) {
      await insertSalesAudit(this.database, {
        actorType: "partner", actorId: current.partner.id,
        action: "referral.business_pricing_set", targetType: "referred_user", targetId: commerceUserId,
        metadata: {
          operationRef: result.operationRef,
          ceilingPercent,
          converted: result.converted,
          discountPercent: result.discountPercent,
          providers: result.providers,
        },
      });
    }

    return {
      userRef: ref.data,
      converted: result.converted,
      customerType: result.customerType,
      discountPercent: result.discountPercent,
      providers: result.providers,
      operationRef: result.operationRef,
      idempotentReplay: result.idempotentReplay,
      ceilingPercent,
    };
  }

  /** Same window and same recorded commission as /earnings, re-grouped by serving provider. */
  @Get("earnings/providers")
  @Header("Cache-Control", "no-store")
  async earningsByProvider(@CurrentAuth() current: RequestAuth, @Query() query: unknown): Promise<unknown> {
    const parsed = earningsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid earnings query");
    const [rows, daily] = await Promise.all([
      getPartnerEarningsByProvider(this.database, current.partner.id, parsed.data.days),
      getPartnerDailyEarningsByProvider(this.database, current.partner.id, parsed.data.days),
    ]);
    return {
      days: parsed.data.days,
      items: rows.map((row) => ({
        providerId: row.providerId,
        events: row.events,
        spendNano: row.spendNano.toString(),
        earnedNano: row.earnedNano.toString(),
      })),
      daily: daily.map((point) => ({
        date: point.date,
        providers: point.providers.map((row) => ({
          providerId: row.providerId,
          events: row.events,
          spendNano: row.spendNano.toString(),
          earnedNano: row.earnedNano.toString(),
        })),
      })),
    };
  }

  @Get("earnings")
  @Header("Cache-Control", "no-store")
  async earnings(@CurrentAuth() current: RequestAuth, @Query() query: unknown): Promise<unknown> {
    const parsed = earningsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid earnings query");
    const series = await getPartnerDailyEarnings(this.database, current.partner.id, parsed.data.days);
    return {
      days: parsed.data.days,
      items: series.map((point) => ({
        date: point.date,
        spendNano: point.spendNano.toString(),
        earnedNano: point.earnedNano.toString(),
        adjustmentNano: point.adjustmentNano.toString(),
        netNano: point.netNano.toString(),
      })),
    };
  }

  @Get("team")
  @Header("Cache-Control", "no-store")
  async team(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const team = await listPartnerTeam(this.database, current.partner.id);
    return {
      platformCommissionBps: this.config.get("DEFAULT_COMMISSION_BPS", { infer: true }),
      teamOverrideMaxBps: current.partner.teamOverrideMaxBps,
      teamInvitesEnabled: current.partner.teamInvitesEnabled,
      b2bEnabled: current.partner.b2bEnabled,
      b2bMaxDiscountBps: current.partner.b2bMaxDiscountBps,
      b2bCanDelegate: current.partner.b2bCanDelegate,
      items: team.map((member) => ({
        id: member.id,
        email: member.email,
        telegramUsername: member.telegramUsername,
        displayName: member.displayName,
        status: member.status,
        commissionBps: member.commissionBps,
        overrideBps: member.overrideBps,
        teamOverrideMaxBps: member.teamOverrideMaxBps,
        teamInvitesEnabled: member.teamInvitesEnabled,
        b2bEnabled: member.b2bEnabled,
        b2bMaxDiscountBps: member.b2bMaxDiscountBps,
        b2bCanDelegate: member.b2bCanDelegate,
        b2bGrantSourcePartnerId: member.b2bGrantSourcePartnerId,
        referredUsers: member.referredUsers,
        earnedNano: member.theirEarnedNano.toString(),
        adjustmentNano: member.theirAdjustmentNano.toString(),
        netNano: member.theirNetNano.toString(),
        myOverrideNano: member.myOverrideNano.toString(),
        myOverrideAdjustmentNano: member.myOverrideAdjustmentNano.toString(),
        myOverrideNetNano: member.myOverrideNetNano.toString(),
      })),
    };
  }

  @Post("invites")
  async createInvite(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    if (!current.partner.teamInvitesEnabled) {
      throw new ForbiddenException("team invitations are disabled for this partner");
    }
    const parsed = createInviteSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid invite data: telegram username is required");
    const telegramUsername = normalizeTelegramUsername(parsed.data.telegramUsername);
    if (!telegramUsername) throw new BadRequestException("invalid telegram username");
    // Expand-only compatibility for the deployed endpoint. The new storefront uses
    // POST /partner/team/invites; this route keeps its previous direct-rate semantics until its
    // consumer-retirement release.
    const cap = current.partner.commissionBps;
    const commissionBps = Math.min(parsed.data.commissionBps ?? cap, cap);
    const canGrantDiscount = current.partner.referralDiscountEnabled;
    const subDiscountEnabled = canGrantDiscount && (parsed.data.referralDiscountEnabled ?? false);
    const subDiscountBps = subDiscountEnabled
      ? Math.min(parsed.data.referralDiscountBps ?? current.partner.referralDiscountBps, current.partner.referralDiscountBps)
      : 0;
    const expiresAt = new Date(Date.now() + INVITE_TTL_DAYS * 24 * 3600 * 1000);
    for (let attempt = 0; ; attempt += 1) {
      try {
        const invite = await createPartnerInvite(this.database, {
          partnerId: current.partner.id,
          code: generateCode(12),
          telegramUsername,
          commissionBps,
          subCommissionBps: null,
          teamOverrideMaxBps: null,
          parentOverrideBps: null,
          promoEnabled: false,
          promoMaxValueNano: 0n,
          promoMaxCount: 0,
          referralDiscountBps: subDiscountBps,
          referralDiscountEnabled: subDiscountEnabled,
          expiresAt,
        });
        return {
          code: invite.code,
          inviteUrl: this.inviteUrl(invite.code),
          telegramUsername: invite.telegramUsername,
          commissionBps: invite.commissionBps,
          overrideBps: current.partner.subCommissionBps,
          expiresAt: invite.expiresAt?.toISOString() ?? null,
        };
      } catch (error) {
        if (error instanceof InviteCodeCollisionError && attempt < 5) continue;
        throw error;
      }
    }
  }

  @Post("team/invites")
  async createTeamInvite(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    if (!current.partner.teamInvitesEnabled) {
      throw new ForbiddenException("team invitations are disabled for this partner");
    }
    const parsed = createInviteSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid invite data: telegram username is required");
    const telegramUsername = normalizeTelegramUsername(parsed.data.telegramUsername);
    if (!telegramUsername) throw new BadRequestException("invalid telegram username");
    const cap = current.partner.teamOverrideMaxBps;
    const overrideBps = parsed.data.overrideBps ?? Math.min(current.partner.subCommissionBps, cap);
    const teamOverrideMaxBps = parsed.data.teamOverrideMaxBps ?? cap;
    if (overrideBps > cap || teamOverrideMaxBps > cap) {
      throw new UnprocessableEntityException(`team controls exceed your maximum of ${cap} bps`);
    }
    const b2bEnabled = parsed.data.b2bEnabled ?? false;
    const b2bMaxDiscountBps = b2bEnabled ? (parsed.data.b2bMaxDiscountBps ?? 0) : 0;
    const b2bCanDelegate = b2bEnabled ? (parsed.data.b2bCanDelegate ?? false) : false;
    if (b2bEnabled && (!current.partner.b2bEnabled || !current.partner.b2bCanDelegate)) {
      throw new ForbiddenException("your B2B authority cannot be delegated");
    }
    if (b2bMaxDiscountBps > current.partner.b2bMaxDiscountBps) {
      throw new UnprocessableEntityException(
        `B2B maximum discount exceeds your limit of ${current.partner.b2bMaxDiscountBps} bps`,
      );
    }
    // The inviter cannot choose the member's platform-funded direct rate. Keeping this value on
    // the invite snapshots the configured 10% default for deterministic onboarding.
    const commissionBps = this.config.get("DEFAULT_COMMISSION_BPS", { infer: true });
    // Preserve the legacy marker permission through existing invites without presenting it as a
    // price capability. Current UI does not grant or expose this field.
    const canGrantDiscount = current.partner.referralDiscountEnabled;
    const subDiscountEnabled = canGrantDiscount && (parsed.data.referralDiscountEnabled ?? false);
    const subDiscountBps = subDiscountEnabled
      ? Math.min(parsed.data.referralDiscountBps ?? current.partner.referralDiscountBps, current.partner.referralDiscountBps)
      : 0;
    const expiresAt = new Date(Date.now() + INVITE_TTL_DAYS * 24 * 3600 * 1000);
    for (let attempt = 0; ; attempt += 1) {
      try {
        const invite = await createPartnerInvite(this.database, {
          partnerId: current.partner.id,
          code: generateCode(12),
          telegramUsername,
          commissionBps,
          subCommissionBps: null,
          teamOverrideMaxBps,
          parentOverrideBps: overrideBps,
          // Промо-доступ остаётся под контролем админа (по умолчанию выключен).
          promoEnabled: false,
          promoMaxValueNano: 0n,
          promoMaxCount: 0,
          referralDiscountBps: subDiscountBps,
          referralDiscountEnabled: subDiscountEnabled,
          teamInvitesEnabled: parsed.data.teamInvitesEnabled ?? true,
          b2bEnabled,
          b2bMaxDiscountBps,
          b2bCanDelegate,
          expiresAt,
        });
        return {
          code: invite.code,
          inviteUrl: this.inviteUrl(invite.code),
          telegramUsername: invite.telegramUsername,
          commissionBps: invite.commissionBps,
          overrideBps: invite.parentOverrideBps,
          teamOverrideMaxBps: invite.teamOverrideMaxBps,
          teamInvitesEnabled: invite.teamInvitesEnabled,
          b2bEnabled: invite.b2bEnabled,
          b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
          b2bCanDelegate: invite.b2bCanDelegate,
          expiresAt: invite.expiresAt?.toISOString() ?? null,
        };
      } catch (error) {
        if (error instanceof InviteCodeCollisionError && attempt < 5) continue;
        throw error;
      }
    }
  }

  // Legacy marker endpoints remain for expand-only compatibility. Current UI does not market or
  // expose them as a discount because they do not reach the pricing authority.

  @Get("invites")
  @Header("Cache-Control", "no-store")
  async listInvites(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const invites = await listPartnerInvites(this.database, current.partner.id);
    return {
      items: invites.map((invite) => ({
        code: invite.code,
        inviteUrl: this.inviteUrl(invite.code),
        telegramUsername: invite.telegramUsername,
        commissionBps: invite.commissionBps,
        overrideBps: invite.parentOverrideBps ?? Math.min(current.partner.subCommissionBps, current.partner.teamOverrideMaxBps),
        teamOverrideMaxBps: invite.teamOverrideMaxBps ?? current.partner.teamOverrideMaxBps,
        teamInvitesEnabled: invite.teamInvitesEnabled,
        b2bEnabled: invite.b2bEnabled,
        b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
        b2bCanDelegate: invite.b2bCanDelegate,
        expiresAt: invite.expiresAt?.toISOString() ?? null,
        consumedAt: invite.consumedAt?.toISOString() ?? null,
        createdAt: invite.createdAt.toISOString(),
      })),
    };
  }

  @Patch("team/:memberId")
  async updateTeamMember(
    @CurrentAuth() current: RequestAuth,
    @Param("memberId") memberId: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(memberId).success) throw new BadRequestException("invalid team member id");
    const parsed = teamMemberControlsSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid team controls");
    try {
      return await updateDirectTeamMemberAuthority(this.database, {
        parentPartnerId: current.partner.id,
        memberId,
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
      });
    } catch (error) {
      if (error instanceof TeamMemberNotFoundError) throw new NotFoundException(error.message);
      if (error instanceof TeamOverrideLimitError) throw new UnprocessableEntityException(error.message);
      if (error instanceof PartnerTeamAuthorityError) throw new ForbiddenException(error.message);
      if (error instanceof PartnerB2BAuthorityError) throw new UnprocessableEntityException(error.message);
      throw error;
    }
  }

  @Get("payouts")
  @Header("Cache-Control", "no-store")
  async listPayouts(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const payouts = await listPartnerPayouts(this.database, current.partner.id);
    return { items: payouts.map(payoutView) };
  }

  /**
   * Периодная модель выплат: текущий период (накопление), лок, дата следующей выплаты и история
   * по полумесячным периодам. Ручного запроса вывода нет — платим по расписанию на кошелёк.
   */
  @Get("periods")
  @Header("Cache-Control", "no-store")
  async periods(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const now = new Date();
    const [state, history] = await Promise.all([
      getPartnerPeriodState(this.database, current.partner.id, now),
      getPartnerPeriodHistory(this.database, current.partner.id, now),
    ]);
    return {
      ...state,
      wallet: boundWallet(current.partner),
      minPayoutNano: (BigInt(this.config.get("SALES_MIN_PAYOUT_USD", { infer: true })) * 1_000_000_000n).toString(),
      lockDays: 7,
      windowDays: 3,
      history,
    };
  }

  /** Промокоды партнёра. Доступно, только если админ включил промо и задал лимиты. */
  @Get("promo-codes")
  @Header("Cache-Control", "no-store")
  async listPromo(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const codes = await listPartnerPromoCodes(this.database, current.partner.id);
    return {
      enabled: current.partner.promoEnabled,
      maxValueNano: current.partner.promoMaxValueNano.toString(),
      maxCount: current.partner.promoMaxCount,
      redeemUrl: `${new URL(this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true })).origin}/dashboard?promo=`,
      // Retained marker permission/ceiling for old clients. It is not a price capability.
      discountAllowed: current.partner.referralDiscountEnabled,
      maxDiscountBps: current.partner.referralDiscountBps,
      pricingAffected: false,
      items: codes.map((c) => ({
        id: c.id,
        code: c.code,
        valueNano: c.valueNano.toString(),
        discountBps: c.discountBps,
        status: c.status,
        redeemedAt: c.redeemedAt?.toISOString() ?? null,
        createdAt: c.createdAt.toISOString(),
      })),
    };
  }

  @Post("promo-codes")
  @HttpCode(201)
  async createPromo(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = createPromoSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid promo value");
    const valueNano = BigInt(parsed.data.valueUsd) * 1_000_000_000n;
    for (let attempt = 0; ; attempt += 1) {
      try {
        const promo = await createPromoCode(this.database, {
          partnerId: current.partner.id,
          code: generatePromoCode(),
          valueNano,
          ...(parsed.data.discountBps !== undefined ? { discountBps: parsed.data.discountBps } : {}),
        });
        return {
          id: promo.id,
          code: promo.code,
          valueNano: promo.valueNano.toString(),
          discountBps: promo.discountBps,
          pricingAffected: false,
          status: promo.status,
        };
      } catch (error) {
        if (error instanceof PromoCodeCollisionError && attempt < 5) continue;
        if (error instanceof PromoNotAllowedError) throw new ForbiddenException(error.message);
        if (error instanceof PromoLimitError) throw new UnprocessableEntityException(error.message);
        throw error;
      }
    }
  }

  /** Legacy one-time attribution links; their bps metadata does not affect pricing. */
  @Get("discount-links")
  @Header("Cache-Control", "no-store")
  async listLinks(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const links = await listDiscountLinks(this.database, current.partner.id);
    const origin = new URL(this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true })).origin;
    return {
      enabled: current.partner.referralDiscountEnabled,
      maxDiscountBps: current.partner.referralDiscountBps,
      pricingAffected: false,
      items: links.map((l) => ({
        id: l.id,
        code: l.code,
        url: `${origin}/?ref=${l.code}`,
        discountBps: l.discountBps,
        note: l.note,
        consumed: l.consumedByCommerceUserId !== null,
        consumedAt: l.consumedAt?.toISOString() ?? null,
        createdAt: l.createdAt.toISOString(),
      })),
    };
  }

  @Post("discount-links")
  @HttpCode(201)
  async createLink(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    if (!current.partner.referralDiscountEnabled) {
      throw new ForbiddenException("legacy referral marker is not enabled for this account");
    }
    const parsed = createDiscountLinkSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid legacy marker (1–95%)");
    const origin = new URL(this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true })).origin;
    for (let attempt = 0; ; attempt += 1) {
      try {
        const link = await createDiscountLink(this.database, {
          partnerId: current.partner.id,
          code: generateCode(12),
          discountBps: parsed.data.discountBps,
          note: parsed.data.note ?? null,
        });
        return {
          id: link.id,
          code: link.code,
          url: `${origin}/?ref=${link.code}`,
          discountBps: link.discountBps,
          note: link.note,
          pricingAffected: false,
        };
      } catch (error) {
        if (error instanceof DiscountLinkCollisionError && attempt < 5) continue;
        if (error instanceof DiscountLinkNotAllowedError) throw new ForbiddenException(error.message);
        throw error;
      }
    }
  }

  @Delete("discount-links/:id")
  @HttpCode(200)
  async deleteLink(@CurrentAuth() current: RequestAuth, @Param("id") id: string): Promise<unknown> {
    if (!/^[0-9a-f-]{36}$/.test(id)) throw new BadRequestException("invalid link id");
    const ok = await deleteDiscountLink(this.database, current.partner.id, id);
    if (!ok) throw new NotFoundException("discount link not found");
    return { deleted: true };
  }

  @Post("promo-codes/:id/disable")
  @HttpCode(200)
  async disablePromo(@CurrentAuth() current: RequestAuth, @Param("id") id: string): Promise<unknown> {
    if (!/^[0-9a-f-]{36}$/.test(id)) throw new BadRequestException("invalid promo id");
    const ok = await disablePromoCode(this.database, current.partner.id, id);
    if (!ok) throw new NotFoundException("promo code not found or already used");
    return { disabled: true };
  }

  /** Привязка/смена кошелька: единственная поддерживаемая сеть — BSC (BEP-20). */
  @Patch("wallet")
  async updateWallet(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = walletSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid BSC address: expected 0x + 40 hex characters");
    const updated = await updatePartnerSettings(this.database, current.partner.id, {
      payoutMethod: PAYOUT_METHOD,
      payoutDetails: { network: "BSC", asset: "USDT (BEP-20)", address: parsed.data.address },
    });
    if (!updated) throw new UnauthorizedException("partner account is unavailable");
    this.auth.invalidatePartnerSessions(current.partner.id);
    // Смена адреса выплат — самое чувствительное действие партнёра: пишем в audit-trail (виден в
    // админ-ленте активности), чтобы подмена адреса перед выплатой не оставалась без следа.
    await insertSalesAudit(this.database, {
      actorType: "partner", actorId: current.partner.id,
      action: "partner.wallet_changed", targetType: "partner", targetId: current.partner.id,
      metadata: { method: PAYOUT_METHOD, address: parsed.data.address },
    });
    return { partner: partnerView(updated) };
  }

  @Patch("settings")
  async updateSettings(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = updateSettingsSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid settings data");
    const updated = await updatePartnerSettings(this.database, current.partner.id, {
      ...(parsed.data.displayName !== undefined ? { displayName: parsed.data.displayName } : {}),
    });
    if (!updated) throw new UnauthorizedException("partner account is unavailable");
    this.auth.invalidatePartnerSessions(current.partner.id);
    return { partner: partnerView(updated) };
  }

  private inviteUrl(code: string): string {
    const base = this.config.get("PUBLIC_SALES_BASE_URL", { infer: true });
    return `${new URL(base).origin}/register?invite=${code}`;
  }
}

function throwPartnerRequestError(error: unknown): never {
  if (error instanceof PartnerRequestConflictError) throw new ConflictException(error.message);
  if (error instanceof PartnerRequestNotFoundError) throw new NotFoundException(error.message);
  if (error instanceof PartnerRequestValidationError) {
    throw new UnprocessableEntityException(error.message);
  }
  throw error;
}

function payoutView(payout: {
  id: string; amountNano: bigint; status: string; method: string; details: unknown;
  requestedAt: Date; decidedAt: Date | null; paidAt: Date | null; adminNote: string | null;
  txHash: string | null; chainStatus: string | null;
}): unknown {
  return {
    id: payout.id,
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
    // Ссылка на транзакцию в обозревателе BNB Chain.
    explorerUrl: payout.txHash ? `https://bscscan.com/tx/${payout.txHash}` : null,
  };
}
