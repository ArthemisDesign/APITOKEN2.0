import { createHash, timingSafeEqual } from "node:crypto";
import {
  BadRequestException,
  Body,
  CanActivate,
  ConflictException,
  Controller,
  ExecutionContext,
  Get,
  Header,
  HttpCode,
  Inject,
  Injectable,
  NotFoundException,
  Post,
  Query,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  findPartnerByReferralCode,
  ensureExternalReferralAlias,
  ExternalReferralAliasConflictError,
  ExternalReferralAliasOwnerNotFoundError,
  redeemPromoCode,
  claimReferralDiscount,
  PromoAlreadyRedeemedError,
  PromoNotFoundError,
  UserAlreadyRedeemedError,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";

// Внутренний Commerce→Sales контур. Гейт — общий SALES_CONTROL_KEY (тот же, что у фида
// Sales→Commerce), заголовок x-api-key; ни один из этих маршрутов не вызывается браузером.

@Injectable()
export class InternalKeyGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}
  canActivate(context: ExecutionContext): boolean {
    const configured = this.config.get("SALES_CONTROL_KEY", { infer: true });
    if (!configured) throw new UnauthorizedException("internal API disabled");
    const request = context.switchToHttp().getRequest<{ headers: Record<string, string | string[] | undefined> }>();
    const supplied = request.headers["x-api-key"];
    if (typeof supplied !== "string" || !safeEqual(configured, supplied)) {
      throw new UnauthorizedException("internal authentication required");
    }
    return true;
  }
}

const redeemSchema = z.object({
  code: z.string().trim().regex(/^[A-Za-z0-9]{4,32}$/),
  commerceUserId: z.string().uuid(),
});

@Controller("internal/promo")
@UseGuards(InternalKeyGuard)
export class InternalController {
  constructor(@Inject(SALES_DATABASE) private readonly database: SalesDatabase) {}

  @Post("redeem")
  @HttpCode(200)
  async redeem(@Body() body: unknown): Promise<unknown> {
    const parsed = redeemSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid redeem payload");
    try {
      const result = await redeemPromoCode(this.database, {
        code: parsed.data.code,
        commerceUserId: parsed.data.commerceUserId,
      });
      return {
        valueNano: result.valueNano.toString(),
        partnerId: result.partnerId,
        referralCode: result.referralCode,
        redemptionRef: result.redemptionRef,
        discountBps: result.discountBps,
        pricingAffected: false,
        alreadyRedeemed: result.alreadyRedeemed,
      };
    } catch (error) {
      if (error instanceof PromoNotFoundError) throw new NotFoundException("promo code not found");
      if (error instanceof PromoAlreadyRedeemedError) throw new ConflictException("promo code already used");
      if (error instanceof UserAlreadyRedeemedError) throw new ConflictException("account already used a promo code");
      throw error;
    }
  }
}

const resolveSchema = z.object({
  code: z.string().trim().regex(/^[A-Za-z0-9_-]{3,32}$/),
});

// Резервный read-only endpoint: сейчас НИКЕМ не вызывается — claim через POST
// `referral-discount` (ниже) заменил прежнюю пару resolve+consume. Оставлен как expand-only
// контракт: отвечает, является ли ?ref= кодом активного партнёра, и возвращает legacy marker.
@Controller("internal/partners")
@UseGuards(InternalKeyGuard)
export class InternalPartnersController {
  constructor(@Inject(SALES_DATABASE) private readonly database: SalesDatabase) {}

  @Get("resolve")
  @Header("Cache-Control", "no-store")
  async resolve(@Query("code") code?: string): Promise<unknown> {
    const parsed = resolveSchema.safeParse({ code });
    if (!parsed.success) return { found: false };
    const partner = await findPartnerByReferralCode(this.database, parsed.data.code.toLowerCase());
    // Only an active partner owns a resolvable referral code. Program-membership gating is a later
    // consumer-retirement change; this existing expand-only contract keeps its rollout semantics.
    if (!partner || partner.status !== "active") return { found: false };
    return { found: true, partnerId: partner.id, referralDiscountBps: partner.referralDiscountBps };
  }

  /**
   * Producer for trusted server-side acquisition tools. The alias contains no customer identity,
   * does not change pricing, and resolves through the ordinary partner attribution path.
   */
  @Post("external-referral-alias")
  @HttpCode(200)
  async externalReferralAlias(@Body() body: unknown): Promise<unknown> {
    const parsed = externalReferralAliasSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid external referral alias payload");
    try {
      const alias = await ensureExternalReferralAlias(this.database, {
        source: parsed.data.source,
        externalRef: parsed.data.externalRef,
        partnerReferralCode: parsed.data.partnerCode.toLowerCase(),
      });
      return {
        source: alias.source,
        externalRef: alias.externalRef,
        code: alias.aliasCode,
        partnerId: alias.partnerId,
        createdAt: alias.createdAt.toISOString(),
      };
    } catch (error) {
      if (error instanceof ExternalReferralAliasOwnerNotFoundError) {
        throw new NotFoundException("active referral owner not found");
      }
      if (error instanceof ExternalReferralAliasConflictError) {
        throw new ConflictException("external referral reference already has another owner");
      }
      throw error;
    }
  }

  // Atomically consumes a legacy one-time attribution link and returns its marker only to the
  // winner. The marker does not change pricing; ordinary/already-consumed codes return zero.
  // POST, т.к. мутирует (consume). Идемпотентно по (code,user).
  @Post("referral-discount")
  @HttpCode(200)
  async referralDiscount(@Body() body: unknown): Promise<unknown> {
    const parsed = claimSchema.safeParse(body);
    if (!parsed.success) return { discountBps: 0, pricingAffected: false };
    const { discountBps } = await claimReferralDiscount(this.database, parsed.data.code.toLowerCase(), parsed.data.commerceUserId);
    return { discountBps, pricingAffected: false };
  }
}

const claimSchema = z.object({
  code: z.string().trim().regex(/^[A-Za-z0-9_-]{3,32}$/),
  commerceUserId: z.string().uuid(),
});

const externalReferralAliasSchema = z.object({
  source: z.string().regex(/^[a-z][a-z0-9_-]{1,31}$/),
  externalRef: z.string().min(1).max(128).regex(/^[A-Za-z0-9:_-]+$/),
  partnerCode: z.string().trim().regex(/^[A-Za-z0-9_-]{3,32}$/),
}).strict();

function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left).digest();
  const b = createHash("sha256").update(right).digest();
  return timingSafeEqual(a, b);
}
