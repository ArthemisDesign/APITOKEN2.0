import {
  BadGatewayException,
  BadRequestException,
  Body,
  Controller,
  Get,
  Headers,
  HttpCode,
  NotFoundException,
  Param,
  Post,
  Req,
  ServiceUnavailableException,
  UnauthorizedException,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { createCheckoutSchema } from "@claude-api/contracts";
import { CryptomusError } from "@claude-api/payments";
import { z } from "zod";
import type { Environment } from "./config.js";
import { CheckoutAmountError, CheckoutService } from "./checkout.service.js";

const uuidSchema = z.string().uuid();

@Controller()
export class PaymentsController {
  constructor(
    private readonly checkouts: CheckoutService,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  @Post("checkouts")
  async createCheckout(@Headers("x-user-id") userHeader: string | undefined, @Body() body: unknown): Promise<unknown> {
    const userId = this.localUserId(userHeader);
    const parsed = createCheckoutSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.checkouts.create(userId, parsed.data.amountUsd, parsed.data.provider);
    } catch (error) {
      if (error instanceof CheckoutAmountError) throw new BadRequestException(error.message);
      if (error instanceof CryptomusError) throw new BadGatewayException(error.message);
      if (error instanceof Error && error.message.includes("active user")) throw new NotFoundException(error.message);
      if (error instanceof Error && error.message.includes("unsupported payment provider")) {
        throw new ServiceUnavailableException("Cryptomus is not configured");
      }
      throw error;
    }
  }

  @Get("checkouts/:id")
  async getCheckout(@Headers("x-user-id") userHeader: string | undefined, @Param("id") id: string): Promise<unknown> {
    const userId = this.localUserId(userHeader);
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("checkout ID must be a UUID");
    const checkout = await this.checkouts.get(userId, id);
    if (!checkout) throw new NotFoundException("checkout not found");
    return checkout;
  }

  @Post("payments/cryptomus/webhook")
  @HttpCode(200)
  async cryptomusWebhook(@Req() request: { rawBody?: Buffer }): Promise<unknown> {
    if (!request.rawBody) throw new BadRequestException("raw webhook body is unavailable");
    try {
      const result = await this.checkouts.processCryptomusWebhook(request.rawBody);
      return { accepted: true, duplicate: result.duplicateEvent, status: result.checkoutStatus };
    } catch (error) {
      if (error instanceof CryptomusError && error.message.includes("signature")) {
        throw new UnauthorizedException("invalid Cryptomus signature");
      }
      if (error instanceof CryptomusError) throw new BadGatewayException(error.message);
      throw error;
    }
  }

  private localUserId(value: string | undefined): string {
    if (!this.config.get("ALLOW_INSECURE_USER_HEADER", { infer: true })) {
      throw new ServiceUnavailableException("checkout authentication is not enabled yet");
    }
    const parsed = uuidSchema.safeParse(value);
    if (!parsed.success) throw new UnauthorizedException("valid x-user-id header required for local checkout testing");
    return parsed.data;
  }
}
