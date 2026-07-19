import { createHash, timingSafeEqual } from "node:crypto";
import {
  BadRequestException,
  Body,
  CanActivate,
  ConflictException,
  Controller,
  ExecutionContext,
  HttpCode,
  Inject,
  Injectable,
  NotFoundException,
  Post,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
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

function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left).digest();
  const b = createHash("sha256").update(right).digest();
  return timingSafeEqual(a, b);
}
