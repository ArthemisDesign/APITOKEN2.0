import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
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
  listApplications,
  setPromoPermissions,
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
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { AdminKeyGuard } from "./admin.guard.js";
import { generateCode } from "./auth.service.js";
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
} from "./schemas.js";

const uuidSchema = z.string().uuid();

@Controller("admin")
@UseGuards(AdminKeyGuard)
export class AdminController {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  @Get("overview")
  @Header("Cache-Control", "no-store")
  async overview(): Promise<unknown> {
    const overview = await getSalesOverview(this.database);
    return {
      partners: overview.partners,
      activePartners: overview.activePartners,
      referredUsers: overview.referredUsers,
      totalDepositNano: overview.totalDepositNano.toString(),
      totalCommissionsNano: overview.totalCommissionsNano.toString(),
      pendingPayoutsNano: overview.pendingPayoutsNano.toString(),
      paidPayoutsNano: overview.paidPayoutsNano.toString(),
    };
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
        referralDiscountBps: partner.referralDiscountBps,
        referralDiscountEnabled: partner.referralDiscountEnabled,
        parentPartnerId: partner.parentPartnerId,
        parentEmail: partner.parentEmail,
        parentTelegramUsername: partner.parentTelegramUsername,
        referredUsers: partner.referredUsers,
        teamSize: partner.teamSize,
        earnedNano: partner.earnedNano.toString(),
        paidNano: partner.paidNano.toString(),
        promoEnabled: partner.promoEnabled,
        promoMaxValueNano: partner.promoMaxValueNano.toString(),
        promoMaxCount: partner.promoMaxCount,
        promoUsed: partner.promoUsed,
        createdAt: partner.createdAt.toISOString(),
      })),
    };
  }

  /** Включить/выключить промокоды партнёру и задать лимиты (номинал USD, количество). */
  @Post("partners/:id/promo")
  @HttpCode(200)
  async setPromo(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const parsed = adminPromoSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid promo settings");
    const ok = await setPromoPermissions(this.database, id, {
      enabled: parsed.data.enabled,
      maxValueNano: BigInt(parsed.data.maxValueUsd) * 1_000_000_000n,
      maxCount: parsed.data.maxCount,
      actorId: "sales-admin-key",
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
  async decideApplicationEndpoint(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
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
          adminNote: parsed.data.note ?? null,
          actorId: "sales-admin-key",
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
  async createInvite(@Body() body: unknown): Promise<unknown> {
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
    for (let attempt = 0; ; attempt += 1) {
      try {
        const invite = await createPartnerInvite(this.database, {
          partnerId: null,
          code: generateCode(12),
          telegramUsername,
          commissionBps: parsed.data.commissionBps ?? null,
          subCommissionBps: parsed.data.subCommissionBps ?? null,
          promoEnabled,
          promoMaxValueNano: BigInt(promoMaxValueUsd) * 1_000_000_000n,
          promoMaxCount,
          referralDiscountBps: parsed.data.referralDiscountBps ?? 0,
          referralDiscountEnabled,
          expiresAt,
        });
        return {
          code: invite.code,
          inviteUrl: `${new URL(this.config.get("PUBLIC_SALES_BASE_URL", { infer: true })).origin}/register?invite=${invite.code}`,
          telegramUsername: invite.telegramUsername,
          commissionBps: invite.commissionBps,
          subCommissionBps: invite.subCommissionBps,
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
        referralDiscountBps: invite.referralDiscountBps,
        referralDiscountEnabled: invite.referralDiscountEnabled,
        promoEnabled: invite.promoEnabled,
        promoMaxCount: invite.promoMaxCount,
        promoMaxValueNano: invite.promoMaxValueNano.toString(),
        expiresAt: invite.expiresAt?.toISOString() ?? null,
        consumedAt: invite.consumedAt?.toISOString() ?? null,
      })),
    };
  }

  @Patch("partners/:id")
  async patchPartner(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    const parsed = adminPatchPartnerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner update");
    const updated = await updatePartnerAdmin(this.database, id, {
      ...(parsed.data.commissionBps !== undefined ? { commissionBps: parsed.data.commissionBps } : {}),
      ...(parsed.data.subCommissionBps !== undefined ? { subCommissionBps: parsed.data.subCommissionBps } : {}),
      ...(parsed.data.referralDiscountBps !== undefined ? { referralDiscountBps: parsed.data.referralDiscountBps } : {}),
      ...(parsed.data.referralDiscountEnabled !== undefined ? { referralDiscountEnabled: parsed.data.referralDiscountEnabled } : {}),
      ...(parsed.data.status !== undefined ? { status: parsed.data.status } : {}),
      actorId: "sales-admin-key",
    });
    if (!updated) throw new NotFoundException("partner not found");
    return { updated: true };
  }

  /** Полное удаление возможно только без истории; иначе 422 → suspend. */
  @Delete("partners/:id")
  @HttpCode(200)
  async deletePartner(@Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid partner id");
    try {
      const deleted = await deletePartnerAdmin(this.database, id, "sales-admin-key");
      if (!deleted) throw new NotFoundException("partner not found");
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
  async decide(@Param("id") id: string, @Body() body: unknown): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("invalid payout id");
    const parsed = adminPayoutDecisionSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid payout decision");
    try {
      const payout = await decidePayout(this.database, {
        payoutId: id,
        decision: parsed.data.action,
        note: parsed.data.note ?? null,
        actorId: "sales-admin-key",
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
      if (error instanceof InvalidPayoutTransitionError) throw new UnprocessableEntityException(error.message);
      throw error;
    }
  }
}
