import { Inject, Injectable, Logger } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import {
  getEngineAccountMapping,
  recordReferralAttribution,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

// Погашение партнёрского промокода на стороне commerce. Код живёт в sales-БД — здесь только
// HTTP-вызов sales-api для атомарного погашения, затем идемпотентный кредит движка и атрибуция
// незакреплённого юзера к владельцу кода. sales-БД commerce не открывает.

export class PromoError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "PromoError";
  }
}

interface SalesRedeemResponse {
  valueNano: string;
  partnerId: string;
  referralCode: string;
  redemptionRef: string;
  alreadyRedeemed: boolean;
}

@Injectable()
export class PromoService {
  private readonly logger = new Logger(PromoService.name);

  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async redeem(userId: string, code: string): Promise<Record<string, unknown>> {
    const base = this.config.get("SALES_API_URL", { infer: true });
    const key = this.config.get("SALES_CONTROL_KEY", { infer: true });
    if (!base || !key) throw new PromoError(503, "promo codes are not available right now");

    // 1) Атомарно погасить код в sales (резервирование). Идемпотентно по (code,user).
    const redeemed = await this.consumeInSales(base, key, code, userId);

    // 2) Кредитовать движок идемпотентно по redemptionRef (ретраи безопасны).
    const mapping = await getEngineAccountMapping(this.database, userId);
    if (!mapping?.engineAccountId || mapping.status !== "active") {
      throw new PromoError(409, "your account is not ready to receive a promo credit yet");
    }
    const valueNano = BigInt(redeemed.valueNano);
    let balance: string | undefined;
    let balanceNano: string | undefined;
    for (let attempt = 0; ; attempt += 1) {
      try {
        const result = await this.engine.creditAccount(mapping.engineAccountId, valueNano, redeemed.redemptionRef);
        balance = result.balance;
        balanceNano = result.balance_nano;
        break;
      } catch (error) {
        if (attempt < 2) continue;
        // Код уже погашен в sales; кредит идемпотентен по ref — юзер может повторить позже.
        this.logger.error(`promo credit failed for ${userId} ref=${redeemed.redemptionRef}`);
        throw new PromoError(502, "could not credit your balance — please try again in a minute");
      }
    }

    // 3) Атрибуция: если юзер ещё ни за кем не закреплён — становится рефералом владельца кода.
    try {
      await recordReferralAttribution(this.database, userId, redeemed.referralCode);
    } catch {
      // best-effort: не срываем успешный кредит из-за атрибуции
    }

    return {
      credited_usd: (valueNano / 1_000_000_000n).toString(),
      credited_nano: valueNano.toString(),
      balance,
      balance_nano: balanceNano,
    };
  }

  private async consumeInSales(base: string, key: string, code: string, userId: string): Promise<SalesRedeemResponse> {
    let response: Response;
    try {
      response = await fetch(new URL("/v1/internal/promo/redeem", base), {
        method: "POST",
        headers: { "content-type": "application/json", "x-api-key": key },
        body: JSON.stringify({ code, commerceUserId: userId }),
        signal: AbortSignal.timeout(10_000),
      });
    } catch {
      throw new PromoError(502, "promo service is unreachable — try again");
    }
    if (response.status === 404) throw new PromoError(404, "this promo code does not exist");
    if (response.status === 409) throw new PromoError(409, "this promo code has already been used");
    if (response.status === 400) throw new PromoError(400, "invalid promo code");
    if (!response.ok) throw new PromoError(502, "promo service error — try again");
    return (await response.json()) as SalesRedeemResponse;
  }
}
