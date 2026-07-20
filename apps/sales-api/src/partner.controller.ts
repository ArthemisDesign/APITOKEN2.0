import { randomBytes } from "node:crypto";
import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Get,
  Header,
  HttpCode,
  Inject,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  UnauthorizedException,
  UnprocessableEntityException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  countPartnerTeam,
  countReferredUsers,
  createPartnerInvite,
  getPartnerDailyEarnings,
  getPartnerEarningsTotals,
  listPartnerInvites,
  listPartnerPayouts,
  listPartnerTeam,
  listReferredUsers,
  getPartnerPeriodState,
  getPartnerPeriodHistory,
  updatePartnerSettings,
  createPromoCode,
  listPartnerPromoCodes,
  disablePromoCode,
  setPartnerReferralDiscount,
  createDiscountLink,
  listDiscountLinks,
  DiscountLinkCollisionError,
  DiscountLinkNotAllowedError,
  InviteCodeCollisionError,
  PromoNotAllowedError,
  PromoLimitError,
  PromoCodeCollisionError,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { generateCode, partnerView } from "./auth.service.js";
import { normalizeTelegramUsername } from "./telegram.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import {
  createDiscountLinkSchema,
  createInviteSchema,
  createPromoSchema,
  earningsQuerySchema,
  partnerSetDiscountSchema,
  updateSettingsSchema,
  walletSchema,
} from "./schemas.js";

const INVITE_TTL_DAYS = 30;
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
      // Скидка рефам: право (выдаёт админ/родитель) + текущее значение. Кабинет показывает поле,
      // только если право есть.
      referralDiscountEnabled: current.partner.referralDiscountEnabled,
      referralDiscountBps: current.partner.referralDiscountBps,
      subCommissionBps: current.partner.subCommissionBps,
      referredUsers,
      teamSize,
      totals: {
        earnedNano: totals.earnedNano.toString(),
        directNano: totals.directNano.toString(),
        overrideNano: totals.overrideNano.toString(),
        paidNano: totals.paidNano.toString(),
        pendingPayoutNano: totals.pendingPayoutNano.toString(),
        availableNano: totals.availableNano.toString(),
      },
      last30d: {
        spendNano: totals.last30dSpendNano.toString(),
        earnedNano: totals.last30dEarnedNano.toString(),
      },
    };
  }

  @Get("referrals")
  @Header("Cache-Control", "no-store")
  async referrals(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const referrals = await listReferredUsers(this.database, current.partner.id);
    return {
      items: referrals.map((referral) => ({
        // Commerce identities stay masked: only a short uuid prefix is exposed to partners.
        userMask: `user-${referral.commerceUserId.slice(0, 8)}…`,
        attributedAt: referral.attributedAt.toISOString(),
        spendNano: referral.spendNano.toString(),
        earnedNano: referral.earnedNano.toString(),
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
      })),
    };
  }

  @Get("team")
  @Header("Cache-Control", "no-store")
  async team(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const team = await listPartnerTeam(this.database, current.partner.id);
    return {
      items: team.map((member) => ({
        id: member.id,
        email: member.email,
        telegramUsername: member.telegramUsername,
        displayName: member.displayName,
        status: member.status,
        commissionBps: member.commissionBps,
        referredUsers: member.referredUsers,
        earnedNano: member.theirEarnedNano.toString(),
        myOverrideNano: member.myOverrideNano.toString(),
      })),
    };
  }

  @Post("invites")
  async createInvite(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = createInviteSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid invite data: telegram username is required");
    const telegramUsername = normalizeTelegramUsername(parsed.data.telegramUsername);
    if (!telegramUsername) throw new BadRequestException("invalid telegram username");
    // Партнёр не может подарить суб-партнёру ставку выше собственной (иначе раздаёт маржу
    // платформы). По умолчанию — своя ставка. Более широкий диапазон — только у админа.
    const cap = current.partner.commissionBps;
    const commissionBps = Math.min(parsed.data.commissionBps ?? cap, cap);
    // Каскад права давать скидку: только партнёр, у кого право есть, может включить его суб-сейлзу,
    // и не больше собственной скидки (нельзя раздать больше, чем разрешено самому). Нет права → нет.
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
          // Промо-доступ остаётся под контролем админа (по умолчанию выключен).
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
          expiresAt: invite.expiresAt?.toISOString() ?? null,
        };
      } catch (error) {
        if (error instanceof InviteCodeCollisionError && attempt < 5) continue;
        throw error;
      }
    }
  }

  /**
   * Партнёр с правом ставит скидку своим рефам (≤90%) из кабинета. Применяется как «пол» цены рефа
   * (реф остаётся b2c на обычных тирах). Право проверяется на уровне БД; нет права → 403.
   */
  @Patch("referral-discount")
  @HttpCode(200)
  async setReferralDiscount(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    if (!current.partner.referralDiscountEnabled) {
      throw new ForbiddenException("referral discount is not enabled for this account");
    }
    const parsed = partnerSetDiscountSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid referral discount (0–90%)");
    const ok = await setPartnerReferralDiscount(this.database, current.partner.id, parsed.data.referralDiscountBps);
    if (!ok) throw new ForbiddenException("referral discount is not enabled for this account");
    return { updated: true, referralDiscountBps: parsed.data.referralDiscountBps };
  }

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
        expiresAt: invite.expiresAt?.toISOString() ?? null,
        consumedAt: invite.consumedAt?.toISOString() ?? null,
        createdAt: invite.createdAt.toISOString(),
      })),
    };
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
      // Кабинет показывает, можно ли вешать спец-скидку и её максимум (право + макс партнёра).
      discountAllowed: current.partner.referralDiscountEnabled,
      maxDiscountBps: current.partner.referralDiscountBps,
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
        return { id: promo.id, code: promo.code, valueNano: promo.valueNano.toString(), discountBps: promo.discountBps, status: promo.status };
      } catch (error) {
        if (error instanceof PromoCodeCollisionError && attempt < 5) continue;
        if (error instanceof PromoNotAllowedError) throw new ForbiddenException(error.message);
        if (error instanceof PromoLimitError) throw new UnprocessableEntityException(error.message);
        throw error;
      }
    }
  }

  /** Персональные одноразовые ссылки со скидкой (нужно право давать скидку). */
  @Get("discount-links")
  @Header("Cache-Control", "no-store")
  async listLinks(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    const links = await listDiscountLinks(this.database, current.partner.id);
    const origin = new URL(this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true })).origin;
    return {
      enabled: current.partner.referralDiscountEnabled,
      maxDiscountBps: current.partner.referralDiscountBps,
      items: links.map((l) => ({
        id: l.id,
        code: l.code,
        url: `${origin}/?ref=${l.code}`,
        discountBps: l.discountBps,
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
      throw new ForbiddenException("referral discount is not enabled for this account");
    }
    const parsed = createDiscountLinkSchema.safeParse(body ?? {});
    if (!parsed.success) throw new BadRequestException("invalid discount (0–90%)");
    const origin = new URL(this.config.get("PUBLIC_MAIN_SITE_URL", { infer: true })).origin;
    for (let attempt = 0; ; attempt += 1) {
      try {
        const link = await createDiscountLink(this.database, {
          partnerId: current.partner.id,
          code: generateCode(12),
          discountBps: parsed.data.discountBps,
        });
        return { id: link.id, code: link.code, url: `${origin}/?ref=${link.code}`, discountBps: link.discountBps };
      } catch (error) {
        if (error instanceof DiscountLinkCollisionError && attempt < 5) continue;
        if (error instanceof DiscountLinkNotAllowedError) throw new ForbiddenException(error.message);
        throw error;
      }
    }
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
    return { partner: partnerView(updated) };
  }

  private inviteUrl(code: string): string {
    const base = this.config.get("PUBLIC_SALES_BASE_URL", { infer: true });
    return `${new URL(base).origin}/register?invite=${code}`;
  }
}

function payoutView(payout: {
  id: string; amountNano: bigint; status: string; method: string; details: unknown;
  requestedAt: Date; decidedAt: Date | null; paidAt: Date | null; adminNote: string | null;
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
  };
}
