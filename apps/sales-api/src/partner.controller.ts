import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  HttpCode,
  Inject,
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
  requestPayout,
  updatePartnerSettings,
  InsufficientEarningsError,
  InviteCodeCollisionError,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { generateCode, partnerView } from "./auth.service.js";
import { normalizeTelegramUsername } from "./telegram.js";
import { SALES_DATABASE } from "./infrastructure.module.js";
import {
  createInviteSchema,
  createPayoutSchema,
  earningsQuerySchema,
  updateSettingsSchema,
  walletSchema,
} from "./schemas.js";

const INVITE_TTL_DAYS = 30;
const PAYOUT_METHOD = "usdt-bep20";

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
    const expiresAt = new Date(Date.now() + INVITE_TTL_DAYS * 24 * 3600 * 1000);
    for (let attempt = 0; ; attempt += 1) {
      try {
        const invite = await createPartnerInvite(this.database, {
          partnerId: current.partner.id,
          code: generateCode(12),
          telegramUsername,
          commissionBps,
          subCommissionBps: null,
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

  /** Выплата уходит ТОЛЬКО на привязанный BSC-кошелёк (USDT BEP-20). */
  @Post("payouts")
  @HttpCode(201)
  async createPayout(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = createPayoutSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid payout request");
    const wallet = boundWallet(current.partner);
    if (!wallet) throw new UnprocessableEntityException("bind your BSC wallet before requesting a payout");
    try {
      const payout = await requestPayout(this.database, {
        partnerId: current.partner.id,
        amountNano: BigInt(parsed.data.amountNano),
        method: PAYOUT_METHOD,
        details: { network: "BSC", asset: "USDT (BEP-20)", address: wallet },
      });
      return { payout: payoutView(payout) };
    } catch (error) {
      if (error instanceof InsufficientEarningsError) throw new UnprocessableEntityException(error.message);
      throw error;
    }
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
