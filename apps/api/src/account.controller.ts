import {
  BadGatewayException,
  BadRequestException,
  Body,
  Controller,
  Delete,
  Get,
  Header,
  HttpCode,
  Logger,
  NotFoundException,
  Param,
  Patch,
  Post,
  Query,
  ServiceUnavailableException,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { createApiKeySchema, renameApiKeySchema, updateApiKeyPolicySchema } from "@claude-api/contracts";
import { EngineClientError } from "@claude-api/engine-client";
import { z } from "zod";
import { AccountService, describeEngineFailure, isRetryableEngineFailure } from "./account.service.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { TotpService } from "./totp.service.js";

const uuidSchema = z.string().uuid();
const ledgerLimitSchema = z.coerce.number().int().min(1).max(1000).default(50);
const usageWindowSchema = z.string().regex(/^(all|\d+[dh])$/).default("30d");

@Controller()
@UseGuards(SessionAuthGuard)
export class AccountController {
  private readonly logger = new Logger(AccountController.name);

  constructor(private readonly accounts: AccountService, private readonly totp: TotpService) {}

  @Get("account")
  @Header("Cache-Control", "no-store")
  async getAccount(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.withEngineErrors(() => this.accounts.getAccount(current.user.id));
  }

  @Get("account/ledger")
  @Header("Cache-Control", "no-store")
  async getLedger(@CurrentAuth() current: RequestAuth, @Query("limit") value?: string): Promise<unknown> {
    const parsed = ledgerLimitSchema.safeParse(value);
    if (!parsed.success) throw new BadRequestException("limit must be an integer from 1 to 1000");
    return this.withEngineErrors(() => this.accounts.getLedger(current.user.id, parsed.data));
  }

  @Get("account/usage")
  @Header("Cache-Control", "no-store")
  async getUsage(@CurrentAuth() current: RequestAuth, @Query("window") value?: string): Promise<unknown> {
    const parsed = usageWindowSchema.safeParse(value ?? undefined);
    if (!parsed.success) throw new BadRequestException("window must be like 30d, 7d, 24h, or all");
    return this.withEngineErrors(() => this.accounts.getUsage(current.user.id, parsed.data));
  }

  @Get("api-keys")
  @Header("Cache-Control", "no-store")
  async listApiKeys(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.withEngineErrors(() => this.accounts.listApiKeys(current.user.id));
  }

  @Post("api-keys")
  @Header("Cache-Control", "no-store")
  async createApiKey(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<unknown> {
    const parsed = createApiKeySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    // Гейт 2FA: если у пользователя включена TOTP — выпуск ключа только с действующим кодом.
    if (current.user.totpEnabled) {
      if (!parsed.data.totpCode) throw new UnauthorizedException("2fa_required");
      const ok = await this.totp.verify(current.user.id, parsed.data.totpCode);
      if (!ok) throw new UnauthorizedException("2fa_invalid");
    }
    return this.withEngineErrors(() => this.accounts.createApiKey(current.user.id, parsed.data));
  }

  @Patch("api-keys/:id")
  @Header("Cache-Control", "no-store")
  async renameApiKey(
    @CurrentAuth() current: RequestAuth,
    @Param("id") id: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("API key ID must be a UUID");
    const parsed = renameApiKeySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    return this.withEngineErrors(() => this.accounts.renameApiKey(current.user.id, id, parsed.data.label));
  }

  @Patch("api-keys/:id/policy")
  @Header("Cache-Control", "no-store")
  async updateApiKeyPolicy(
    @CurrentAuth() current: RequestAuth,
    @Param("id") id: string,
    @Body() body: unknown,
  ): Promise<unknown> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("API key ID must be a UUID");
    const parsed = updateApiKeyPolicySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    if (current.user.totpEnabled) {
      if (!parsed.data.totpCode) throw new UnauthorizedException("2fa_required");
      const ok = await this.totp.verify(current.user.id, parsed.data.totpCode);
      if (!ok) throw new UnauthorizedException("2fa_invalid");
    }
    return this.withEngineErrors(() => this.accounts.updateApiKeyPolicy(current.user.id, id, parsed.data));
  }

  @Delete("api-keys/:id")
  @HttpCode(204)
  @Header("Cache-Control", "no-store")
  async disableApiKey(@CurrentAuth() current: RequestAuth, @Param("id") id: string): Promise<void> {
    if (!uuidSchema.safeParse(id).success) throw new BadRequestException("API key ID must be a UUID");
    const disabled = await this.withEngineErrors(() => this.accounts.disableApiKey(current.user.id, id));
    if (!disabled) throw new NotFoundException("API key not found");
  }

  private async withEngineErrors<T>(action: () => Promise<T>): Promise<T> {
    try {
      return await action();
    } catch (error) {
      if (isRetryableEngineFailure(error)) {
        // The public 503 text stays generic; the original engine failure must stay
        // diagnosable from logs (blue-green slot cutovers cause transient bursts).
        this.logger.warn(`engine request failed, responding 503: ${describeEngineFailure(error)}`);
        throw new ServiceUnavailableException("engine is temporarily unavailable");
      }
      if (error instanceof EngineClientError || error instanceof z.ZodError ||
          (error instanceof Error && error.message.includes("invalid"))) {
        throw new BadGatewayException("engine returned an invalid response");
      }
      throw error;
    }
  }
}
