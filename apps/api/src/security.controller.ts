import { BadRequestException, Body, Controller, Header, HttpCode, Post, UseGuards } from "@nestjs/common";
import { totpCodeBodySchema } from "@claude-api/contracts";
import { CurrentAuth, type RequestAuth, SessionAuthGuard } from "./auth.guard.js";
import { TotpService } from "./totp.service.js";

// /v1/security/totp/* — самообслуживание 2FA (сессия обязательна). Гейт на выпуск ключа — в AccountController.
@Controller("security")
@UseGuards(SessionAuthGuard)
export class SecurityController {
  constructor(private readonly totp: TotpService) {}

  @Post("totp/setup")
  @Header("Cache-Control", "no-store")
  async setup(@CurrentAuth() current: RequestAuth): Promise<unknown> {
    return this.totp.setup(current.user.id, current.user.email);
  }

  @Post("totp/enable")
  @HttpCode(204)
  @Header("Cache-Control", "no-store")
  async enable(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<void> {
    const parsed = totpCodeBodySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    await this.totp.enable(current.user.id, parsed.data.code);
  }

  @Post("totp/disable")
  @HttpCode(204)
  @Header("Cache-Control", "no-store")
  async disable(@CurrentAuth() current: RequestAuth, @Body() body: unknown): Promise<void> {
    const parsed = totpCodeBodySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    await this.totp.disable(current.user.id, parsed.data.code);
  }
}
