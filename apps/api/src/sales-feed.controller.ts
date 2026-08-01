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
  setReferralFloor,
  listPaidTopupsAfter,
  listReferralAttributionsAfter,
  listReferralProfiles,
  listUsageEventsAfter,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";
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
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
  ) {}

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
    const page = await listUsageEventsAfter(this.database, parseCursor(afterId), parseLimit(limit, 1000, 2000));
    return {
      items: page.items.map((row) => ({
        id: row.id.toString(),
        userId: row.userId,
        amountNano: row.amountNano.toString(),
        providerId: row.providerId,
        accountClass: row.accountClass,
        pricingMode: row.pricingMode,
        paidFundedNano: row.paidFundedNano?.toString() ?? null,
        commissionEligible: row.commissionEligible,
        snapshotDigest: row.snapshotDigest,
        occurredAt: row.occurredAt.toISOString(),
      })),
      nextCursor: page.nextCursor.toString(),
    };
  }

  @Get("topups")
  async topups(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const page = await listPaidTopupsAfter(this.database, parseCursor(afterId), parseLimit(limit, 500, 1000));
    return {
      items: page.items.map((row) => ({
        id: row.id.toString(),
        paymentId: row.paymentId,
        userId: row.userId,
        amountNano: row.amountNano.toString(),
        paidAt: row.paidAt.toISOString(),
      })),
      nextCursor: page.nextCursor.toString(),
    };
  }

  /**
   * Устанавливает «пол» скидки сейлза для реферала партнёра. Клиент остаётся b2c и идёт по обычным
   * тир-правилам; пол лишь гарантирует цену не хуже скидки сейлза (эффект = min(тир, 100−floor)).
   * floorBps=0 снимает пол. Вызывает sales-api при атрибуции. Идемпотентно. Только партнёрские коды.
   */
  @Post("referral-discount")
  @HttpCode(200)
  async referralDiscount(@Body() body: unknown) {
    const parsed = referralDiscountSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid referral discount payload");
    const result = await setReferralFloor(this.database, {
      userId: parsed.data.userId,
      floorBps: parsed.data.floorBps,
      actorId: parsed.data.actorId ?? "sales-referral",
      // Явная смена процента действующему рефералу (партнёр/админ из sales-кабинета): абсолютная
      // запись, разрешает понижение. Автоматические пути поле не шлют — монотонность сохранена.
      override: parsed.data.override ?? false,
    });
    return { applied: result.applied, multiplierBp: result.multiplierBp };
  }

  /**
   * Профили рефералов для витрины партнёра: тип (b2b/b2c), эффективная скидка, партнёрский floor,
   * накопленные пополнения и ЖИВОЙ баланс из движка. Только по явному списку user_id, который
   * sales-api формирует из закреплённых за партнёром рефералов — partner видит лишь своих.
   */
  @Post("referral-profiles")
  @HttpCode(200)
  async referralProfiles(@Body() body: unknown) {
    const parsed = referralProfilesSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid referral profiles payload");
    const rows = await listReferralProfiles(this.database, parsed.data.userIds);
    // Баланс/эффективный mult берём из движка (авторитет денег). Ограничиваем параллелизм,
    // чтобы страница партнёра со многими рефералами не устраивала движку всплеск запросов.
    const items = await mapWithConcurrency(rows, 8, async (row) => {
      let balanceNano: string | null = null;
      let engineMultBp: number | null = null;
      let engineStatus = row.engineStatus;
      if (row.engineAccountId) {
        try {
          const account = await this.engine.getAccount(row.engineAccountId);
          balanceNano = account.balance_nano;
          engineMultBp = account.mult_bp;
          engineStatus = account.status;
        } catch {
          // Движок недоступен для этого аккаунта — отдаём профиль без живого баланса, не роняя всю страницу.
        }
      }
      const multiplierBp = engineMultBp ?? row.multiplierBp;
      return {
        userId: row.userId,
        customerType: row.customerType,
        multiplierBp,
        discountPercent: 100 - multiplierBp / 100,
        referralFloorBps: row.referralFloorBps,
        cumulativeTopupNano: row.cumulativeTopupNano.toString(),
        balanceNano,
        status: engineStatus,
      };
    });
    return { items };
  }
}

async function mapWithConcurrency<T, R>(items: readonly T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const results = new Array<R>(items.length);
  let cursor = 0;
  const workers = Array.from({ length: Math.min(limit, items.length) }, async () => {
    for (let index = cursor++; index < items.length; index = cursor++) {
      results[index] = await fn(items[index]!);
    }
  });
  await Promise.all(workers);
  return results;
}

const referralDiscountSchema = z.object({
  userId: z.string().uuid(),
  floorBps: z.number().int().min(0).max(9500),
  override: z.boolean().optional(),
  actorId: z.enum(["sales-referral", "sales-partner", "sales-admin"]).optional(),
});

const referralProfilesSchema = z.object({
  // Партнёр не может иметь тысячи рефералов на одной странице; жёсткий потолок бережёт движок.
  userIds: z.array(z.string().uuid()).max(500),
});
