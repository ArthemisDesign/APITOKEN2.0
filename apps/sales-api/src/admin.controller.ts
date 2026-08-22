import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  Headers,
  HttpCode,
  Inject,
  NotFoundException,
  Param,
  Delete,
  Patch,
  Post,
  Query,
  UnprocessableEntityException,
  UseGuards,
} from "@nestjs/common";
import { z } from "zod";
import { ConfigService } from "@nestjs/config";
import {
  createPartnerInvite,
  decideApplication,
  decidePayout,
  deletePartnerAdmin,
  getDuePayoutList,
  getPartnerActivity,
  getPartnerAnalytics,
  getPartnerDailyEarnings,
  listApplications,
  listDiscountLinks,
  listPartnerAnalytics,
  listPartnerPayouts,
  listPartnerPromoCodes,
  listPartnerTeam,
  listReferredUsers,
  resolveReferredUserByPrefix,
  insertSalesAudit,
  PARTNER_ANALYTICS_SORTS,
  setPromoPermissions,
  type PartnerAnalyticsSort,
  ApplicationAlreadyDecidedError,
  PartnerHasHistoryError,
  ReferralCodeCollisionError,
  getSalesOverview,
  listPartnersWithAggregates,
  listPayouts,
  listRootInvites,
  updatePartnerAdmin,
  InvalidPayoutTransitionError,
  InviteCodeCollisionError,
  PartnerB2BAuthorityError,
  PartnerRequestDecisionError,
  PartnerRequestNotFoundError,
  decidePartnerRequest,
  getPartnerRequest,
  listPartnerRequests,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { AdminKeyGuard } from "./admin.guard.js";
import { CommerceService } from "./commerce.service.js";
import { AuthService, generateCode } from "./auth.service.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import { normalizeTelegramUsername } from "./telegram.js";
import {
  adminApplicationDecisionSchema,
  adminApplicationsQuerySchema,
  adminCreateInviteSchema,
  adminPatchPartnerSchema,
  adminPayoutDecisionSchema,
  adminPayoutsQuerySchema,
  adminPromoSchema,
  adminPartnerRequestDecisionSchema,
  partnerRequestsQuerySchema,
  referralUserRefSchema,
  setReferralDiscountSchema,
} from "./schemas.js";
import {
  decodePartnerRequestCursor,
  encodePartnerRequestCursor,
  partnerRequestView as serializePartnerRequest,
} from "./partner-request-view.js";

const uuidSchema = z.string().uuid();

function adminActorId(value: string | undefined): string {
  const actor = value?.trim();
  return actor && actor.length <= 200 ? actor : "legacy-sales-admin";
}

const analyticsQuerySchema = z.object({
  sort: z.enum(PARTNER_ANALYTICS_SORTS).optional(),
  dir: z.enum(["asc", "desc"]).optional(),
  status: z.enum(["all", "active", "suspended", "pending"]).optional(),
  q: z.string().max(120).optional(),
  limit: z.coerce.number().int().min(1).max(500).optional(),
  offset: z.coerce.number().int().min(0).optional(),
});

// NestJS сериализует ответ в JSON, а bigint кидает исключение — рекурсивно приводим bigint→строка,
// Date→ISO. nano-поля из analytics уже строки; это для переиспользуемых функций (team/links/…).
function jsonSafe<T>(value: T): unknown {
  if (typeof value === "bigint") return value.toString();
  if (value instanceof Date) return value.toISOString();
  if (Array.isArray(value)) return value.map(jsonSafe);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, jsonSafe(v)]));
  }
  return value;
}

@Controller("admin")
@UseGuards(AdminKeyGuard)
export class AdminController {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
    private readonly commerce: CommerceService,
    private readonly auth: AuthService,
  ) {}

  @Get("overview")
  @Header("Cache-Control", "no-store")
  async overview(): Promise<unknown> {
    const overview = await getSalesOverview(this.database);
    return {
      partners: overview.partners,
      activePartners: overview.activePartners,
      referredUsers: overview.referredUsers,
      totalSpendNano: overview.totalSpendNano.toString(),
      totalCommissionsNano: overview.totalCommissionsNano.toString(),
      totalAdjustmentsNano: overview.totalAdjustmentsNano.toString(),
      totalNetCommissionsNano: overview.totalNetCommissionsNano.toString(),
      totalDebtNano: overview.totalDebtNano.toString(),
      totalPayableNano: overview.totalPayableNano.toString(),
      pendingPayoutsNano: overview.pendingPayoutsNano.toString(),
      paidPayoutsNano: overview.paidPayoutsNano.toString(),
    };
  }

  @Get("requests")
  @Header("Cache-Control", "no-store")
  async requests(@Query() query: unknown): Promise<unknown> {
    const parsed = partnerRequestsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid requests query");
    const before = decodePartnerRequestCursor(parsed.data.cursor);
    if (parsed.data.cursor !== undefined && before === undefined) {
      throw new BadRequestException("invalid requests cursor");
    }
    const page = await listPartnerRequests(this.database, {
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

  @Get("requests/:id")
  @Header("Cache-Control", "no-store")
  async request(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner request id");
    const request = await getPartnerRequest(this.database, id);
    if (!request) throw new NotFoundException("partner request not found");
    const customerEmail = request.commerceUserId === null
      ? null
      : (await this.commerce.referralProfiles([request.commerceUserId])).get(request.commerceUserId)?.email ?? null;
    return { request: serializePartnerRequest(request, customerEmail) };
  }

  @Post("requests/:id/decision")
  @HttpCode(200)
  async decidePartnerRequestEndpoint(
    @Param("id") id: string,
    @Headers("x-admin-actor") actorHeader: string | undefined,
    @Body() body: unknown,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner request id");
    const parsed = adminPartnerRequestDecisionSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid partner request decision");
    try {
      const decision = parsed.data;
      const request = await decidePartnerRequest(this.database, {
        requestId: id,
        action: decision.action,
        reviewerActor: adminActorId(actorHeader),
        reviewerNote: decision.note,
        ...(decision.action !== "approve" || decision.commissionBps === undefined
          ? {}
          : { approvedCommissionBps: decision.commissionBps }),
        ...(decision.action !== "approve" || decision.discountPercent === undefined
          ? {}
          : { approvedDiscountBps: decision.discountPercent * 100 }),
        ...(decision.action !== "approve" || decision.providers === undefined
          ? {}
          : {
              providers: Object.fromEntries(Object.entries(decision.providers)
                .map(([providerId, percent]) => [providerId, percent === null ? null : percent * 100])),
            }),
      });
      const customerEmail = request.commerceUserId === null
        ? null
        : (await this.commerce.referralProfiles([request.commerceUserId])).get(request.commerceUserId)?.email ?? null;
      return { request: serializePartnerRequest(request, customerEmail) };
    } catch (error) {
      if (error instanceof PartnerRequestNotFoundError) throw new NotFoundException(error.message);
      if (error instanceof PartnerRequestDecisionError) {
        throw new UnprocessableEntityException(error.message);
      }
      throw error;
    }
  }

  @Get("partners")
  @Header("Cache-Control", "no-store")
  async partners(): Promise<unknown> {
    const partners = await listPartnersWithAggregates(this.database);
    return {
      items: partners.map((partner) => ({
        id: partner.id,
        email: partner.email,
        telegramUsername: partner.telegramUsername,
        displayName: partner.displayName,
        status: partner.status,
        emailVerified: partner.emailVerified,
        referralCode: partner.referralCode,
        commissionBps: partner.commissionBps,
        subCommissionBps: partner.subCommissionBps,
        teamOverrideMaxBps: partner.teamOverrideMaxBps,
        parentOverrideBps: partner.parentOverrideBps,
        referralDiscountBps: partner.referralDiscountBps,
        referralDiscountEnabled: partner.referralDiscountEnabled,
        b2bEnabled: partner.b2bEnabled,
        b2bMaxDiscountBps: partner.b2bMaxDiscountBps,
        teamInvitesEnabled: partner.teamInvitesEnabled,
        b2bCanDelegate: partner.b2bCanDelegate,
        b2bGrantSourcePartnerId: partner.b2bGrantSourcePartnerId,
        parentPartnerId: partner.parentPartnerId,
        parentEmail: partner.parentEmail,
        parentTelegramUsername: partner.parentTelegramUsername,
        referredUsers: partner.referredUsers,
        teamSize: partner.teamSize,
        earnedNano: partner.earnedNano.toString(),
        adjustmentNano: partner.adjustmentNano.toString(),
        netNano: partner.netNano.toString(),
        debtNano: partner.debtNano.toString(),
        payableNano: partner.payableNano.toString(),
        paidNano: partner.paidNano.toString(),
        promoEnabled: partner.promoEnabled,
        promoMaxValueNano: partner.promoMaxValueNano.toString(),
        promoMaxCount: partner.promoMaxCount,
        promoUsed: partner.promoUsed,
        createdAt: partner.createdAt.toISOString(),
      })),
    };
  }

  /** Аналитика партнёров: сортируемый/фильтруемый/пагинируемый список для анализа 100+ рефоводов. */
  @Get("partner-analytics")
  @Header("Cache-Control", "no-store")
  async partnerAnalytics(@Query() query: Record<string, string>): Promise<unknown> {
    const parsed = analyticsQuerySchema.safeParse(query);
    if (!parsed.success) throw new BadRequestException("invalid analytics query");
    const q = parsed.data;
    const result = await listPartnerAnalytics(this.database, {
      ...(q.sort ? { sort: q.sort as PartnerAnalyticsSort } : {}),
      ...(q.dir ? { dir: q.dir } : {}),
      ...(q.status ? { status: q.status } : {}),
      ...(q.q ? { search: q.q } : {}),
      ...(q.limit !== undefined ? { limit: q.limit } : {}),
      ...(q.offset !== undefined ? { offset: q.offset } : {}),
    });
    return jsonSafe(result);
  }

  /** Детальная карточка партнёра: агрегаты + графики + подсписки (команда/ссылки/промо/выплаты/рефералы). */
  @Get("partners/:id/analytics")
  @Header("Cache-Control", "no-store")
  async partnerDetail(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const partner = await getPartnerAnalytics(this.database, id);
    if (!partner) throw new NotFoundException("partner not found");
    const [daily, team, discountLinks, promos, payouts, referrals] = await Promise.all([
      getPartnerDailyEarnings(this.database, id, 30),
      listPartnerTeam(this.database, id),
      listDiscountLinks(this.database, id),
      listPartnerPromoCodes(this.database, id),
      listPartnerPayouts(this.database, id),
      listReferredUsers(this.database, id),
    ]);
    // Commerce supplies the actual price discount. referralFloorBps is separate legacy metadata
    // and is never presented as an applied price.
    const profiles = await this.commerce.referralProfiles(referrals.map((r) => r.commerceUserId));
    // Email is authoritative in Commerce and is disclosed only on this managed-admin boundary.
    const maskedReferrals = referrals.map((r) => {
      const profile = profiles.get(r.commerceUserId);
      return {
        userMask: `user-${r.commerceUserId.slice(0, 8)}…`,
        userRef: r.commerceUserId.slice(0, 8),
        email: profile?.email ?? null,
        attributedAt: r.attributedAt,
        spendNano: r.spendNano,
        earnedNano: r.earnedNano,
        adjustmentNano: r.adjustmentNano,
        netNano: r.netNano,
        customerType: profile?.customerType ?? null,
        discountPercent: profile?.discountPercent ?? null,
        referralFloorBps: profile?.referralFloorBps ?? null,
      };
    });
    return jsonSafe({ partner, daily, team, discountLinks, promos, payouts, referrals: maskedReferrals });
  }

  /**
   * Expand-only admin writer for the legacy referral marker. It does not change pricing and the
   * response says so explicitly. Absolute override can lower or clear the stored marker.
   */
  @Post("partners/:id/referrals/:userRef/discount")
  @HttpCode(200)
  async setReferralDiscount(
    @Param("id") id: string,
    @Param("userRef") userRef: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const ref = referralUserRefSchema.safeParse(userRef);
    if (!ref.success) throw new BadRequestException("invalid referral reference");
    const parsed = setReferralDiscountSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid legacy referral marker");
    const commerceUserId = await resolveReferredUserByPrefix(this.database, id, ref.data);
    if (commerceUserId === null) throw new NotFoundException("referral not found");
    if (commerceUserId === "ambiguous") throw new UnprocessableEntityException("ambiguous referral reference");
    const result = await this.commerce.setReferralDiscount(commerceUserId, parsed.data.discountBps, "sales-admin");
    if (!result.applied && result.multiplierBp === null) {
      throw new UnprocessableEntityException("this referral cannot store the legacy B2C marker");
    }
    await insertSalesAudit(this.database, {
      actorType: "admin", actorId: adminActorId(actorHeader),
      action: "referral.discount_set", targetType: "referred_user", targetId: commerceUserId,
      metadata: { partnerId: id, discountBps: parsed.data.discountBps, multiplierBp: result.multiplierBp },
    });
    return {
      userRef: ref.data,
      discountBps: parsed.data.discountBps,
      multiplierBp: result.multiplierBp,
      pricingAffected: false,
    };
  }

  /** Лента действий партнёра (рефералы, депозиты, ссылки, промо, выплаты, входы, админ-действия). */
  @Get("partners/:id/activity")
  @Header("Cache-Control", "no-store")
  async partnerActivity(@Param("id") id: string, @Query("limit") limit?: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const n = limit ? Number.parseInt(limit, 10) : 60;
    const events = await getPartnerActivity(this.database, id, Number.isFinite(n) ? n : 60);
    const userIds = [...new Set(events.flatMap((event) => {
      const userId = event.meta.commerceUserId;
      return typeof userId === "string" && uuidSchema.safeParse(userId).success ? [userId] : [];
    }))];
    const profiles = await this.commerce.referralProfiles(userIds);
    const enriched = events.map((event) => {
      const rawUserId = event.meta.commerceUserId;
      if (typeof rawUserId !== "string" || !uuidSchema.safeParse(rawUserId).success) {
        return { ...event, email: null, userMask: null };
      }
      const { commerceUserId: _commerceUserId, ...safeMeta } = event.meta;
      const email = profiles.get(rawUserId)?.email ?? null;
      const userMask = `user-${rawUserId.slice(0, 8)}…`;
      const prefix = event.type === "referral" ? "New referral" : "Referred deposit";
      return {
        ...event,
        label: `${prefix} ${email ?? userMask}`,
        email,
        userMask,
        meta: { ...safeMeta, email, userMask },
      };
    });
    return jsonSafe({ events: enriched });
  }

  /** Включить/выключить промокоды партнёру и задать лимиты (номинал USD, количество). */
  @Post("partners/:id/promo")
  @HttpCode(200)
  async setPromo(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const parsed = adminPromoSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid promo settings");
    const ok = await setPromoPermissions(this.database, id, {
      enabled: parsed.data.enabled,
      maxValueNano: BigInt(parsed.data.maxValueUsd) * 1_000_000_000n,
      maxCount: parsed.data.maxCount,
      actorId: adminActorId(actorHeader),
    });
    if (!ok) throw new NotFoundException("partner not found");
    return { updated: true };
  }

  /**
   * Авто-список «к выплате» за окно текущего/последнего периода. Считается от подтверждённых
   * комиссий (< конца периода) минус выплаченное; ролловер автоматический. Само отправление
   * (on-chain) — отдельная система; здесь только читаемый список для оператора.
   */
  @Get("payout-list")
  @Header("Cache-Control", "no-store")
  async payoutList(): Promise<unknown> {
    const minPayoutNano = BigInt(this.config.get("SALES_MIN_PAYOUT_USD", { infer: true })) * 1_000_000_000n;
    const list = await getDuePayoutList(this.database, new Date(), minPayoutNano);
    return { ...list, minPayoutNano: minPayoutNano.toString() };
  }

  /** Заявки «с улицы» (вход через Telegram без инвайта). */
  @Get("applications")
  @Header("Cache-Control", "no-store")
  async applications(@Query() query: unknown): Promise<unknown> {
    const parsed = adminApplicationsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid applications query");
    const applications = await listApplications(this.database, parsed.data.status ?? null);
    return {
      items: applications.map((application) => ({
        id: application.id,
        telegramUsername: application.telegramUsername,
        displayName: application.displayName,
        note: application.note,
        status: application.status,
        adminNote: application.adminNote,
        createdAt: application.createdAt.toISOString(),
        decidedAt: application.decidedAt?.toISOString() ?? null,
      })),
    };
  }

  /** Approve создаёт активного партнёра сразу (вход у человека заработает мгновенно). */
  @Post("applications/:id/decision")
  @HttpCode(200)
  async decideApplicationEndpoint(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid application id");
    const parsed = adminApplicationDecisionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid application decision");
    for (let attempt = 0; ; attempt += 1) {
      try {
        const result = await decideApplication(this.database, {
          applicationId: id,
          action: parsed.data.action,
          referralCode: generateCode(8),
          commissionBps: parsed.data.commissionBps ?? this.config.get("DEFAULT_COMMISSION_BPS", { infer: true }),
          subCommissionBps: parsed.data.subCommissionBps ?? this.config.get("DEFAULT_SUB_COMMISSION_BPS", { infer: true }),
          teamOverrideMaxBps: parsed.data.teamOverrideMaxBps ?? 2_000,
          teamInvitesEnabled: parsed.data.teamInvitesEnabled ?? true,
          b2bEnabled: parsed.data.b2bEnabled ?? false,
          b2bMaxDiscountBps: parsed.data.b2bEnabled === true ? (parsed.data.b2bMaxDiscountBps ?? 0) : 0,
          b2bCanDelegate: parsed.data.b2bEnabled === true ? (parsed.data.b2bCanDelegate ?? false) : false,
          adminNote: parsed.data.note ?? null,
          actorId: adminActorId(actorHeader),
        });
        return {
          application: { id: result.application.id, status: result.application.status },
          partnerId: result.partnerId,
        };
      } catch (error) {
        if (error instanceof ReferralCodeCollisionError && attempt < 5) continue;
        if (error instanceof ApplicationAlreadyDecidedError) throw new UnprocessableEntityException(error.message);
        throw error;
      }
    }
  }

  /** Онбординг сейлза верхнего уровня: инвайт без родителя, привязанный к telegram. */
  @Post("invites")
  @HttpCode(201)
  async createInvite(
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    const parsed = adminCreateInviteSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid invite data: telegram username is required");
    const telegramUsername = normalizeTelegramUsername(parsed.data.telegramUsername);
    if (!telegramUsername) throw new BadRequestException("invalid telegram username");
    const expiresAt = new Date(Date.now() + 30 * 24 * 3600 * 1000);
    // Промо-доступ включается только если заданы оба лимита (> 0). Номинал USD → nano.
    const promoMaxCount = parsed.data.promoMaxCount ?? 0;
    const promoMaxValueUsd = parsed.data.promoMaxValueUsd ?? 0;
    const promoEnabled = promoMaxCount > 0 && promoMaxValueUsd > 0;
    const referralDiscountEnabled = parsed.data.referralDiscountEnabled ?? false;
    const b2bEnabled = parsed.data.b2bEnabled ?? false;
    for (let attempt = 0; ; attempt += 1) {
      try {
        const invite = await createPartnerInvite(this.database, {
          partnerId: null,
          code: generateCode(12),
          telegramUsername,
          commissionBps: parsed.data.commissionBps ?? null,
          subCommissionBps: parsed.data.subCommissionBps ?? null,
          teamOverrideMaxBps: parsed.data.teamOverrideMaxBps ?? null,
          parentOverrideBps: null,
          promoEnabled,
          promoMaxValueNano: BigInt(promoMaxValueUsd) * 1_000_000_000n,
          promoMaxCount,
          referralDiscountBps: parsed.data.referralDiscountBps ?? 0,
          referralDiscountEnabled,
          // A ceiling only travels with an explicit grant; without it the invite carries none.
          b2bEnabled,
          b2bMaxDiscountBps: b2bEnabled ? (parsed.data.b2bMaxDiscountBps ?? 0) : 0,
          teamInvitesEnabled: parsed.data.teamInvitesEnabled ?? true,
          b2bCanDelegate: b2bEnabled ? (parsed.data.b2bCanDelegate ?? false) : false,
          actorId: adminActorId(actorHeader),
          expiresAt,
        });
        return {
          code: invite.code,
          inviteUrl: `${new URL(this.config.get("PUBLIC_SALES_BASE_URL", { infer: true })).origin}/register?invite=${invite.code}`,
          telegramUsername: invite.telegramUsername,
          commissionBps: invite.commissionBps,
          subCommissionBps: invite.subCommissionBps,
          teamOverrideMaxBps: invite.teamOverrideMaxBps ?? 2_000,
          teamInvitesEnabled: invite.teamInvitesEnabled,
          b2bEnabled: invite.b2bEnabled,
          b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
          b2bCanDelegate: invite.b2bCanDelegate,
          referralDiscountBps: invite.referralDiscountBps,
          referralDiscountEnabled: invite.referralDiscountEnabled,
          promoEnabled: invite.promoEnabled,
          promoMaxCount: invite.promoMaxCount,
          promoMaxValueNano: invite.promoMaxValueNano.toString(),
          expiresAt: invite.expiresAt?.toISOString() ?? null,
        };
      } catch (error) {
        if (error instanceof InviteCodeCollisionError && attempt < 5) continue;
        throw error;
      }
    }
  }

  @Get("invites")
  @Header("Cache-Control", "no-store")
  async invites(): Promise<unknown> {
    const invites = await listRootInvites(this.database);
    const origin = new URL(this.config.get("PUBLIC_SALES_BASE_URL", { infer: true })).origin;
    return {
      items: invites.map((invite) => ({
        code: invite.code,
        inviteUrl: `${origin}/register?invite=${invite.code}`,
        telegramUsername: invite.telegramUsername,
        commissionBps: invite.commissionBps,
        subCommissionBps: invite.subCommissionBps,
        teamOverrideMaxBps: invite.teamOverrideMaxBps ?? 2_000,
        referralDiscountBps: invite.referralDiscountBps,
        referralDiscountEnabled: invite.referralDiscountEnabled,
        b2bEnabled: invite.b2bEnabled,
        b2bMaxDiscountBps: invite.b2bMaxDiscountBps,
        teamInvitesEnabled: invite.teamInvitesEnabled,
        b2bCanDelegate: invite.b2bCanDelegate,
        promoEnabled: invite.promoEnabled,
        promoMaxCount: invite.promoMaxCount,
        promoMaxValueNano: invite.promoMaxValueNano.toString(),
        expiresAt: invite.expiresAt?.toISOString() ?? null,
        consumedAt: invite.consumedAt?.toISOString() ?? null,
      })),
    };
  }

  @Patch("partners/:id")
  async patchPartner(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const parsed = adminPatchPartnerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner update");
    try {
      const updated = await updatePartnerAdmin(this.database, id, {
        ...(parsed.data.commissionBps !== undefined ? { commissionBps: parsed.data.commissionBps } : {}),
        ...(parsed.data.subCommissionBps !== undefined ? { subCommissionBps: parsed.data.subCommissionBps } : {}),
        ...(parsed.data.teamOverrideMaxBps !== undefined
          ? { teamOverrideMaxBps: parsed.data.teamOverrideMaxBps }
          : {}),
        ...(parsed.data.referralDiscountBps !== undefined ? { referralDiscountBps: parsed.data.referralDiscountBps } : {}),
        ...(parsed.data.referralDiscountEnabled !== undefined ? { referralDiscountEnabled: parsed.data.referralDiscountEnabled } : {}),
        ...(parsed.data.b2bEnabled !== undefined ? { b2bEnabled: parsed.data.b2bEnabled } : {}),
        ...(parsed.data.b2bMaxDiscountBps !== undefined ? { b2bMaxDiscountBps: parsed.data.b2bMaxDiscountBps } : {}),
        ...(parsed.data.teamInvitesEnabled !== undefined
          ? { teamInvitesEnabled: parsed.data.teamInvitesEnabled }
          : {}),
        ...(parsed.data.b2bCanDelegate !== undefined
          ? { b2bCanDelegate: parsed.data.b2bCanDelegate }
          : {}),
        ...(parsed.data.status !== undefined ? { status: parsed.data.status } : {}),
        actorId: adminActorId(actorHeader),
      });
      if (!updated) throw new NotFoundException("partner not found");
    } catch (error) {
      if (error instanceof PartnerB2BAuthorityError) {
        throw new UnprocessableEntityException(error.message);
      }
      throw error;
    }
    this.auth.invalidatePartnerSessions(id);
    return { updated: true };
  }

  /** Полное удаление возможно только без истории; иначе 422 → suspend. */
  @Delete("partners/:id")
  @HttpCode(200)
  async deletePartner(
    @Param("id") id: string,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    try {
      const deleted = await deletePartnerAdmin(this.database, id, adminActorId(actorHeader));
      if (!deleted) throw new NotFoundException("partner not found");
      this.auth.invalidatePartnerSessions(id);
      return { deleted: true };
    } catch (error) {
      if (error instanceof PartnerHasHistoryError) throw new UnprocessableEntityException(error.message);
      throw error;
    }
  }

  @Get("payouts")
  @Header("Cache-Control", "no-store")
  async payouts(@Query() query: unknown): Promise<unknown> {
    const parsed = adminPayoutsQuerySchema.safeParse(query ?? {});
    if (!parsed.success) throw new BadRequestException("invalid payouts query");
    const payouts = await listPayouts(this.database, parsed.data.status);
    return {
      items: payouts.map((payout) => ({
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
      })),
    };
  }

  @Post("payouts/:id/decision")
  @HttpCode(200)
  async decide(
    @Param("id") id: string,
    @Body() body: unknown,
    @Headers("x-admin-actor") actorHeader?: string,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid payout id");
    const parsed = adminPayoutDecisionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid payout decision");
    if (parsed.data.action !== "reject") {
      throw new UnprocessableEntityException(
        "legacy manual payouts can only be rejected; prepare a fenced on-chain payout batch instead",
      );
    }
    try {
      const payout = await decidePayout(this.database, {
        payoutId: id,
        decision: parsed.data.action,
        note: parsed.data.note ?? null,
        actorId: adminActorId(actorHeader),
      });
      return {
        payout: {
          id: payout.id,
          partnerId: payout.partnerId,
          amountNano: payout.amountNano.toString(),
          status: payout.status,
          decidedAt: payout.decidedAt?.toISOString() ?? null,
          paidAt: payout.paidAt?.toISOString() ?? null,
          adminNote: payout.adminNote,
        },
      };
    } catch (error) {
      if (error instanceof InvalidPayoutTransitionError) {
        throw new UnprocessableEntityException(error.message);
      }
      throw error;
    }
  }
}
