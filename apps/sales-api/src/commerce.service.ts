import { Injectable, Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import type { Environment } from "./config.js";

// Клиент к commerce internal API (единственная граница sales→commerce, серверный SALES_CONTROL_KEY).
// Используется витриной партнёра, чтобы обогатить рефералов авторитетными данными коммерции/движка:
// тип (b2b/b2c), фактическая скидка, legacy referral marker, пополнения и живой баланс.

const nanoStringSchema = z.string().regex(/^\d{1,27}$/);

const referralProfileSchema = z.object({
  userId: z.string().uuid(),
  email: z.string().email().max(320),
  customerType: z.enum(["b2c", "b2b"]),
  multiplierBp: z.number().int().nonnegative(),
  discountPercent: z.number(),
  referralFloorBps: z.number().int().nonnegative(),
  cumulativeTopupNano: nanoStringSchema,
  balanceNano: nanoStringSchema.nullable(),
  status: z.string().nullable(),
});

export type ReferralProfile = z.infer<typeof referralProfileSchema>;

@Injectable()
export class CommerceService {
  private readonly logger = new Logger(CommerceService.name);

  constructor(private readonly config: ConfigService<Environment, true>) {}

  /**
   * Профили рефералов из commerce. Best-effort: при недоступности commerce возвращаем пустую карту —
   * витрина партнёра деградирует до локальных полей (траты/комиссия), а не падает целиком.
   */
  async referralProfiles(userIds: readonly string[]): Promise<Map<string, ReferralProfile>> {
    const map = new Map<string, ReferralProfile>();
    if (userIds.length === 0) return map;
    const base = this.config.get("COMMERCE_BASE_URL", { infer: true });
    try {
      const response = await fetch(new URL("/v1/internal/sales/referral-profiles", base), {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "x-api-key": this.config.get("SALES_CONTROL_KEY", { infer: true }),
        },
        body: JSON.stringify({ userIds: [...userIds] }),
        signal: AbortSignal.timeout(15_000),
      });
      if (!response.ok) {
        this.logger.warn(`referral-profiles responded ${response.status}`);
        return map;
      }
      const body: unknown = await response.json();
      const items = (body as { items?: unknown }).items;
      if (!Array.isArray(items)) return map;
      for (const item of items) {
        const parsed = referralProfileSchema.safeParse(item);
        if (parsed.success) map.set(parsed.data.userId, parsed.data);
      }
    } catch (error) {
      this.logger.warn(`referral-profiles fetch failed: ${error instanceof Error ? error.message : "unknown"}`);
    }
    return map;
  }

  /**
   * Expand-only writer for the legacy referral marker. It never changes scalar/provider pricing.
   * override=true на стороне commerce: абсолютная запись, разрешено понижение и сброс (0).
   * НЕ best-effort: вызывающий должен знать результат, поэтому ошибки транспорта пробрасываем.
   * applied=false for a non-B2C/missing profile or when the marker is unchanged.
   */
  async setReferralDiscount(
    userId: string,
    floorBps: number,
    actorId: "sales-partner" | "sales-admin",
  ): Promise<{ applied: boolean; multiplierBp: number | null }> {
    const base = this.config.get("COMMERCE_BASE_URL", { infer: true });
    const response = await fetch(new URL("/v1/internal/sales/referral-discount", base), {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-api-key": this.config.get("SALES_CONTROL_KEY", { infer: true }),
      },
      body: JSON.stringify({ userId, floorBps, override: true, actorId }),
      signal: AbortSignal.timeout(15_000),
    });
    if (!response.ok) throw new Error(`referral-discount responded ${response.status}`);
    const body = referralDiscountResultSchema.safeParse(await response.json());
    if (!body.success) throw new Error("referral-discount returned an unexpected payload");
    return body.data;
  }

  async setPartnerBusinessPricing(input: {
    operationRef?: string;
    userId: string;
    referralCode: string;
    ceilingPercent: number;
    discountPercent?: number;
    providers?: Record<string, number | null>;
    actorId?: string;
    reason?: string;
  }): Promise<PartnerBusinessPricingResult> {
    const base = this.config.get("COMMERCE_BASE_URL", { infer: true });
    const response = await fetch(new URL("/v1/internal/sales/partner-business-pricing", base), {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-api-key": this.config.get("SALES_CONTROL_KEY", { infer: true }),
      },
      body: JSON.stringify(input),
      signal: AbortSignal.timeout(20_000),
    });
    if (!response.ok) {
      // 403 is the meaningful one: commerce disagreed about ownership or the ceiling.
      throw new CommercePartnerPricingError(response.status, `partner-business-pricing responded ${response.status}`);
    }
    const body = partnerBusinessPricingResultSchema.safeParse(await response.json());
    if (!body.success) throw new Error("partner-business-pricing returned an unexpected payload");
    return body.data;
  }
}

export class CommercePartnerPricingError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "CommercePartnerPricingError";
  }
}

/**
 * The partner-driven B2B write. Commerce re-checks the ceiling and proves the customer is this
 * partner's referral, so a defect on this side cannot reprice someone else's customer. Errors are
 * propagated, never swallowed: a partner must learn whether their pricing change actually landed.
 */
const partnerBusinessPricingResultSchema = z.object({
  operationRef: z.string().min(8).max(200),
  idempotentReplay: z.boolean(),
  userId: z.string(),
  converted: z.boolean(),
  customerType: z.literal("b2b"),
  discountPercent: z.number(),
  providers: z.record(z.string(), z.number()),
});

export type PartnerBusinessPricingResult = z.infer<typeof partnerBusinessPricingResultSchema>;

const referralDiscountResultSchema = z.object({
  applied: z.boolean(),
  multiplierBp: z.number().int().nullable(),
});
