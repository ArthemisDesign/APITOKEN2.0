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
  redeemPromoCode,
  PromoAlreadyRedeemedError,
  PromoNotFoundError,
  UserAlreadyRedeemedError,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";

// Внутренний контур: commerce (apps/api) вызывает sales-api для погашения промокода.
// Гейт — общий SALES_CONTROL_KEY (тот же, что у фида sales→commerce), заголовок x-api-key.

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

// commerce спрашивает при OAuth-регистрации: этот ?ref= — код активного партнёра? и его B2B-скидка.
// По ней реф партнёра переводится в B2B ДО welcome-бонуса (бонус ему не выдаётся).
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
    // Только активный партнёр триггерит B2B; иначе код трактуется как обычный (реф получит бонус).
    if (!partner || partner.status !== "active") return { found: false };
    return { found: true, partnerId: partner.id, referralDiscountBps: partner.referralDiscountBps };
  }
}

function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left).digest();
  const b = createHash("sha256").update(right).digest();
  return timingSafeEqual(a, b);
}
