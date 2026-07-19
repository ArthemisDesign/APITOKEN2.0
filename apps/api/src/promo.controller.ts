import { BadRequestException, Body, Controller, HttpCode, HttpException, Post, UseGuards } from "@nestjs/common";
import { z } from "zod";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { PromoError, PromoService } from "./promo.service.js";

const redeemSchema = z.object({ code: z.string().trim().regex(/^[A-Za-z0-9]{4,32}$/) }).strict();

@Controller("promo")
@UseGuards(SessionAuthGuard)
export class PromoController {
  constructor(private readonly promo: PromoService) {}

  @Post("redeem")
  @HttpCode(200)
  async redeem(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = redeemSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid promo code");
    try {
      return await this.promo.redeem(current.user.id, parsed.data.code);
    } catch (error) {
      if (error instanceof PromoError) throw new HttpException(error.message, error.status);
      throw error;
    }
  }
}
