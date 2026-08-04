import {
  BadGatewayException,
  BadRequestException,
  Body,
  Controller,
  Get,
  Header,
  Headers,
  HttpCode,
  Logger,
  NotFoundException,
  Param,
  Post,
  Req,
  ServiceUnavailableException,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { createCheckoutSchema } from "@claude-api/contracts";
import { CryptomusError, PlategaError } from "@claude-api/payments";
import { z } from "zod";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { CheckoutAmountError, CheckoutService, PlategaWebhookAuthError } from "./checkout.service.js";
import { AccountService, describeEngineFailure, isRetryableEngineFailure } from "./account.service.js";

const uuidSchema = z.string().uuid();

@Controller()
export class PaymentsController {
  private readonly logger = new Logger(PaymentsController.name);

  constructor(
    private readonly checkouts: CheckoutService,
    private readonly accounts: AccountService,
  ) {}

  @Post("checkouts")
  @Header("Cache-Control", "no-store")
  @UseGuards(SessionAuthGuard)
  async createCheckout(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = createCheckoutSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      await this.accounts.ensureEngineAccount(current.user.id);
      return await this.checkouts.create(current.user.id, parsed.data.amountUsd, parsed.data.provider, parsed.data.paymentMethod);
    } catch (error) {
      if (error instanceof CheckoutAmountError) throw new BadRequestException(error.message);
      if (error instanceof CryptomusError || error instanceof PlategaError) throw new BadGatewayException(error.message);
      if (error instanceof Error && error.message.includes("active user")) throw new NotFoundException(error.message);
      if (error instanceof Error && error.message.includes("unsupported payment provider")) {
        throw new ServiceUnavailableException("payment provider is not configured");
      }
      if (isRetryableEngineFailure(error)) {
        // Same contract as AccountController: generic public 503, original failure in logs.
        this.logger.warn(`engine request failed, responding 503: ${describeEngineFailure(error)}`);
        throw new ServiceUnavailableException("engine is temporarily unavailable");
      }
      throw error;
    }
  }

  @Get("checkouts/:id")
  @Header("Cache-Control", "no-store")
  @UseGuards(SessionAuthGuard)
  async getCheckout(@CurrentAuth() current: RequestAuth, @Param("id") id: string): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("checkout ID must be a UUID");
    const checkout = await this.checkouts.get(current.user.id, id);
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

  @Post("payments/platega/webhook")
  @HttpCode(200)
  async plategaWebhook(
    @Req() request: { rawBody?: Buffer },
    @Headers("x-secret") secret?: string,
    // Platega sends "X-MerchantId" which normalizes to "x-merchantid" (no dash).
    @Headers("x-merchantid") merchantId?: string,
  ): Promise<unknown> {
    if (!request.rawBody) throw new BadRequestException("raw webhook body is unavailable");
    try {
      const result = await this.checkouts.processPlategaWebhook(request.rawBody, { secret, merchantId });
      return { accepted: true, duplicate: result.duplicateEvent, status: result.checkoutStatus };
    } catch (error) {
      if (error instanceof PlategaWebhookAuthError) throw new UnauthorizedException(error.message);
      if (error instanceof PlategaError) throw new BadGatewayException(error.message);
      throw error;
    }
  }
}
