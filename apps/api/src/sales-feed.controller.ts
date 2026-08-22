import {
  BadRequestException,
  Body,
  ForbiddenException,
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
  convertCustomerToBusiness,
  getReferralAttributionCode,
  getPricingView,
  listCustomerProviderDiscounts,
  setBusinessPricingBundle,
  isDiscountProviderId,
  BusinessCustomerNotFoundError,
  listPaidTopupsAfter,
  listPaidTopupsV2After,
  listPaymentReversalsAfter,
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
// Деньги в ответах — только decimal-строки nanoUSD. Referral profile email is exposed only on
// the authenticated server-to-server route and only for the explicit user ids supplied by Sales.

const PG_BIGINT_MAX = 9_223_372_036_854_775_807n;

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
  const parsed = BigInt(value);
  return parsed <= PG_BIGINT_MAX ? parsed : 0n;
}

function parseLimit(value: string | undefined, fallback: number, max: number): number {
  // Parse the whole token. Number.parseInt("1junk") used to turn malformed input into a real
  // page size, and converting an unbounded decimal through Number can lose integer precision.
  if (!/^\d{1,20}$/.test(value ?? "")) return fallback;
  const parsed = BigInt(value!);
  if (parsed < 1n) return fallback;
  return parsed > BigInt(max) ? max : Number(parsed);
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
      sourceHead: page.sourceHead.toString(),
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

  @Get("topups-v2")
  async topupsV2(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const page = await listPaidTopupsV2After(this.database, parseCursor(afterId), parseLimit(limit, 500, 1000));
    return {
      items: page.items.map((row) => ({
        id: row.id.toString(),
        paymentId: row.paymentId,
        userId: row.userId,
        amountNano: row.amountNano.toString(),
        paidAt: row.paidAt.toISOString(),
      })),
      nextCursor: page.nextCursor.toString(),
      sourceHead: page.sourceHead.toString(),
    };
  }

  @Get("payment-reversals")
  async paymentReversals(@Query("after_id") afterId?: string, @Query("limit") limit?: string) {
    const page = await listPaymentReversalsAfter(
      this.database,
      parseCursor(afterId),
      parseLimit(limit, 500, 1000),
    );
    return {
      items: page.items.map((row) => ({
        id: row.id.toString(),
        paymentId: row.paymentId,
        userId: row.userId,
        kind: row.kind,
        amountNano: row.amountNano.toString(),
        reversedAt: row.reversedAt.toISOString(),
      })),
      nextCursor: page.nextCursor.toString(),
      sourceHead: page.sourceHead.toString(),
    };
  }

  /**
   * Stores the legacy referral marker as partner attribution, never as a promised or applied price.
   * B2C price authority is the saved scalar plus provider overrides; this route never moves either
   * and returns multiplierBp=null plus pricingAffected=false. Zero clears the marker. Idempotent.
   */
  @Post("referral-discount")
  @HttpCode(200)
  async referralDiscount(@Body() body: unknown) {
    const parsed = referralDiscountSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid legacy referral marker payload");
    const result = await setReferralFloor(this.database, {
      userId: parsed.data.userId,
      floorBps: parsed.data.floorBps,
      actorId: parsed.data.actorId ?? "sales-referral",
      // The retained admin/partner compatibility path may replace/lower the marker explicitly.
      // Automatic replays omit override and remain monotonic.
      override: parsed.data.override ?? false,
    });
    return { applied: result.applied, multiplierBp: result.multiplierBp, pricingAffected: false as const };
  }

  /**
   * A granted partner prices their OWN referral as a B2B customer.
   *
   * Two independent guards, because either one alone is not enough. Sales owns the grant and its
   * ceiling and checks both before calling; commerce re-checks the ceiling AND proves the customer
   * is actually attributed to the calling partner's referral code. Without that second proof, a bug
   * on the sales side would be enough to reprice any customer in the system through a route that is
   * authenticated only as "sales".
   *
   * Conversion is idempotent: an already-B2B customer keeps their class and only the requested
   * values move. Nothing here is best-effort — the partner must learn whether their change landed.
   */
  @Post("partner-business-pricing")
  @HttpCode(200)
  async partnerBusinessPricing(@Body() body: unknown) {
    const parsed = partnerBusinessPricingSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid partner business pricing payload");
    const { userId, referralCode, ceilingPercent, discountPercent, providers } = parsed.data;

    // The customer must be this partner's referral. Attribution is first-touch and immutable, so
    // it is the one fact commerce can verify on its own.
    const attributed = await getReferralAttributionCode(this.database, userId);
    if (attributed === null || attributed !== referralCode) {
      throw new ForbiddenException("this customer is not attributed to the calling partner");
    }

    // Re-enforce the ceiling here. Sales already refused anything deeper, so reaching this branch
    // means the two sides disagree — fail closed rather than pick the more generous reading.
    const requested = [
      ...(discountPercent === undefined ? [] : [discountPercent]),
      ...Object.values(providers ?? {}).filter((value): value is number => value !== null),
    ];
    if (requested.some((value) => value > ceilingPercent)) {
      throw new ForbiddenException("requested discount exceeds the partner ceiling");
    }

    const before = await getPricingView(this.database, userId);
    let converted = false;
    if (before?.customerType !== "b2b") {
      // A partner converting their referral must supply the default the customer starts on;
      // provider overrides alone would leave the rest of the catalog at the B2C rate.
      if (discountPercent === undefined) {
        throw new BadRequestException("converting a referral to B2B requires the default discount");
      }
      const result = await convertCustomerToBusiness(this.database, {
        userId,
        actorId: "sales-partner",
        reason: "partner b2b conversion",
        multiplierBp: 10_000 - discountPercent * 100,
      });
      converted = result.converted;
    }

    const providerEntries = Object.entries(providers ?? {});
    // After a fresh conversion the default is already applied; re-sending it would be an empty
    // change. Skip it so the bundle is never called with nothing to do.
    const defaultChange = converted ? undefined : discountPercent;
    if (defaultChange !== undefined || providerEntries.length > 0) {
      try {
        await setBusinessPricingBundle(this.database, {
          userId,
          ...(defaultChange === undefined ? {} : { multiplierBp: 10_000 - defaultChange * 100 }),
          providers: Object.fromEntries(providerEntries.map(([providerId, percent]) => [
            providerId,
            percent === null ? null : 10_000 - percent * 100,
          ])),
          actorId: "sales-partner",
          reason: "partner b2b pricing",
        });
      } catch (error) {
        if (error instanceof BusinessCustomerNotFoundError) {
          throw new BadRequestException("referral has no provisioned business account yet");
        }
        throw error;
      }
    }

    const [after, overrides] = await Promise.all([
      getPricingView(this.database, userId),
      listCustomerProviderDiscounts(this.database, userId),
    ]);
    return {
      userId,
      converted,
      customerType: after?.customerType ?? null,
      discountPercent: after?.discountPercent ?? null,
      providers: Object.fromEntries(overrides.map((row) => [row.providerId, 100 - row.multiplierBp / 100])),
    };
  }

  /**
   * Referral profiles: account type, actual discount, legacy marker, topups and live engine
   * balance. Only an explicit user_id list supplied from the partner's assignments is accepted,
   * so one partner cannot inspect another partner's referrals.
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
        email: row.email,
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

// Percent-valued because the whole B2B surface (admin editor included) speaks whole percents;
// bps stay an internal representation. Capped at 95 to match the pricing range — the partner's own
// ceiling narrows it further and is enforced on both sides.
const partnerDiscountPercentSchema = z.number().int().min(0).max(95);

const partnerBusinessPricingSchema = z.object({
  userId: z.string().uuid(),
  // The calling partner's referral code, proving the customer is theirs.
  referralCode: z.string().min(1).max(64),
  ceilingPercent: partnerDiscountPercentSchema,
  discountPercent: partnerDiscountPercentSchema.optional(),
  // null drops a provider override back to the customer's default.
  providers: z.record(z.string(), partnerDiscountPercentSchema.nullable()).optional(),
}).superRefine((value, context) => {
  if (value.discountPercent === undefined && (value.providers === undefined || Object.keys(value.providers).length === 0)) {
    context.addIssue({ code: z.ZodIssueCode.custom, message: "nothing to change" });
  }
  for (const providerId of Object.keys(value.providers ?? {})) {
    // Closed provider list: an unknown id would be stored and then silently never match a request.
    if (!isDiscountProviderId(providerId)) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: `unknown provider ${providerId}` });
    }
  }
});

const referralProfilesSchema = z.object({
  // Партнёр не может иметь тысячи рефералов на одной странице; жёсткий потолок бережёт движок.
  userIds: z.array(z.string().uuid()).max(500),
});
