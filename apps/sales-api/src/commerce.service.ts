import { Injectable, Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import type { Environment } from "./config.js";

// Клиент к commerce internal API (единственная граница sales→commerce, серверный SALES_CONTROL_KEY).
// Используется витриной партнёра, чтобы обогатить рефералов авторитетными данными коммерции/движка:
// тип (b2b/b2c), эффективная скидка, партнёрский floor, накопленные пополнения и живой баланс.

const nanoStringSchema = z.string().regex(/^\d{1,27}$/);

const referralProfileSchema = z.object({
  userId: z.string().uuid(),
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
}
