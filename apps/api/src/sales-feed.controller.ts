import {
  BadRequestException,
  Body,
  CanActivate,
  Controller,
  ExecutionContext,
  Get,
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
  flipReferredUserToB2B,
  listPaidTopupsAfter,
  listReferralAttributionsAfter,
  listUsageEventsAfter,
  type Database,
} from "@claude-api/db";
import type { Environment } from "./config.js";
import { DATABASE } from "./infrastructure.module.js";
import { safeEqual } from "./admin.guard.js";

// Internal-фид для sales bounded context (sales.apitoken.sale). Единственная граница
// commerce→sales: курсорные фиды after_id под серверным ключом SALES_CONTROL_KEY.
// Деньги в ответах — только decimal-строки nanoUSD; email пользователей не отдаём.

@Injectable()
export class SalesFeedGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const configured = this.config.get("SALES_CONTROL_KEY", { infer: true });
    if (!configured) throw new NotFoundException();
    const request = context.switchToHttp().getRequest<{ headers: Record<string, string | string[] | undefined> }>();
    const supplied = request.headers["x-api-key"];
    if (typeof supplied !== "string" || !safeEqual(configured, supplied)) {
      throw new UnauthorizedException("sales feed authentication required");
    }
    return true;
  }
}

function parseCursor(value: string | undefined): bigint {
  if (value === undefined || value === "") return 0n;
  if (!/^\d{1,19}$/.test(value)) return 0n;
  return BigInt(value);
}

function parseLimit(value: string | undefined, fallback: number, max: number): number {
  const parsed = Number.parseInt(value ?? "", 10);
  if (!Number.isInteger(parsed) || parsed < 1) return fallback;
  return Math.min(parsed, max);
}

@Controller("internal/sales")
@UseGuards(SalesFeedGuard)
export class SalesFeedController {
  constructor(@Inject(DATABASE) private readonly database: Database) {}

  @Get("attributions")
  async attributions(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const rows = await listReferralAttributionsAfter(this.database, parseCursor(afterId), parseLimit(limit, 500, 1000));
    return {
      items: rows.map((row) => ({
        id: row.id.toString(),
        userId: row.userId,
        code: row.code,
        createdAt: row.createdAt.toISOString(),
      })),
    };
  }

  @Get("usage-events")
  async usageEvents(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const rows = await listUsageEventsAfter(this.database, parseCursor(afterId), parseLimit(limit, 1000, 2000));
    return {
      items: rows.map((row) => ({
        id: row.id.toString(),
        userId: row.userId,
        amountNano: row.amountNano.toString(),
        occurredAt: row.occurredAt.toISOString(),
      })),
    };
  }

  @Get("topups")
  async topups(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const rows = await listPaidTopupsAfter(this.database, parseCursor(afterId), parseLimit(limit, 500, 1000));
    return {
      items: rows.map((row) => ({
        id: row.id.toString(),
        paymentId: row.paymentId,
        userId: row.userId,
        amountNano: row.amountNano.toString(),
        paidAt: row.paidAt.toISOString(),
      })),
    };
  }

  /**
   * Флип пользователя, пришедшего ПО РЕФЕРАЛКЕ ПАРТНЁРА, в B2B: его цену задаёт скидка сейлза
   * (≤90%), обычные тир-скидки/бонусы/промо на него не действуют. Вызывает sales-api, когда
   * атрибуция впервые закрепила юзера за партнёром. Идемпотентно. Только для партнёрских кодов —
   * обычная сайтовая рефка (в будущем) сюда не идёт.
   */
  @Post("referral-b2b")
  @HttpCode(200)
  async referralB2B(@Body() body: unknown) {
    const parsed = referralB2BSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid referral b2b payload");
    const result = await flipReferredUserToB2B(this.database, {
      userId: parsed.data.userId,
      referralDiscountBps: parsed.data.referralDiscountBps,
      actorId: "sales-referral",
    });
    return { flipped: result.flipped, alreadyB2B: result.alreadyB2B, multiplierBp: result.multiplierBp };
  }
}

const referralB2BSchema = z.object({
  userId: z.string().uuid(),
  referralDiscountBps: z.number().int().min(0).max(9000),
});
