import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Get,
  Header,
  HttpCode,
  HttpException,
  HttpStatus,
  NotFoundException,
  Param,
  Post,
  Req,
  Res,
  ServiceUnavailableException,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard, sessionCookieName } from "./auth.guard.js";
import {
  ApplicationPendingError,
  AuthRateLimitedError,
  AuthService,
  InvalidInviteError,
  NoAccountError,
  PartnerSuspendedError,
  TelegramAuthDisabledError,
  TelegramSignatureError,
  type PartnerSession,
} from "./auth.service.js";
import { inviteCodeSchema, telegramApplySchema, telegramAuthSchema } from "./schemas.js";

interface ReplyLike { header(name: string, value: string | string[]): void }
interface RequestLike { headers: Record<string, string | string[] | undefined>; ip?: string }

@Controller("auth")
export class AuthController {
  constructor(private readonly auth: AuthService, private readonly config: ConfigService<Environment, true>) {}

  /** Имя бота для Telegram Login Widget (нужно фронту до логина). */
  @Get("telegram/config")
  @Header("Cache-Control", "no-store")
  telegramConfig(): unknown {
    const botUsername = this.auth.telegramBotUsername();
    if (!botUsername) throw new ServiceUnavailableException("telegram login is not configured");
    return { botUsername };
  }

  /** Публичная проверка инвайта для страницы регистрации. */
  @Get("invite/:code")
  @Header("Cache-Control", "no-store")
  async invite(@Param("code") code: string): Promise<unknown> {
    if (!inviteCodeSchema.safeParse(code).success) throw new BadRequestException("invalid invite code");
    const info = await this.auth.inviteInfo(code);
    if (!info) throw new NotFoundException("invite code is invalid or expired");
    return { invite: { telegramUsername: info.telegramUsername } };
  }

  @Post("telegram")
  @HttpCode(200)
  async telegram(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = telegramAuthSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid telegram login payload");
    const { inviteCode, ...payload } = parsed.data;
    try {
      const session = await this.auth.telegramLogin({
        payload,
        inviteCode: inviteCode ?? null,
        ...requestMetadata(request),
      });
      this.setSession(reply, session);
      return { partner: session.partner };
    } catch (error) {
      if (error instanceof TelegramAuthDisabledError) throw new ServiceUnavailableException(error.message);
      if (error instanceof TelegramSignatureError) throw new UnauthorizedException(error.message);
      // code — машиночитаемый маркер для фронта: no_account → предложить заявку,
      // application_pending → показать «на рассмотрении».
      if (error instanceof NoAccountError) {
        throw new ForbiddenException({ code: "no_account", message: error.message });
      }
      if (error instanceof ApplicationPendingError) {
        throw new ForbiddenException({ code: "application_pending", message: error.message });
      }
      if (error instanceof InvalidInviteError) throw new BadRequestException(error.message);
      if (error instanceof PartnerSuspendedError) throw new ForbiddenException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  /** Заявка в программу: тот же подписанный payload виджета + свободный текст. */
  @Post("apply")
  @HttpCode(200)
  async apply(@Body() body: unknown, @Req() request: RequestLike): Promise<unknown> {
    const parsed = telegramApplySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid application payload");
    const { note, ...payload } = parsed.data;
    try {
      const result = await this.auth.apply({ payload, note: note ?? null, ipAddress: request.ip ?? null });
      return result;
    } catch (error) {
      if (error instanceof TelegramAuthDisabledError) throw new ServiceUnavailableException(error.message);
      if (error instanceof TelegramSignatureError) throw new UnauthorizedException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Get("me")
  @Header("Cache-Control", "no-store")
  @UseGuards(SessionAuthGuard)
  me(@CurrentAuth() current: RequestAuth): unknown {
    return { partner: current.partner };
  }

  @Post("logout")
  @HttpCode(204)
  @UseGuards(SessionAuthGuard)
  async logout(@CurrentAuth() current: RequestAuth, @Res({ passthrough: true }) reply: ReplyLike): Promise<void> {
    await this.auth.logout(current.sessionId, current.partner.id);
    reply.header("set-cookie", this.clearCookie());
  }

  private setSession(reply: ReplyLike, session: PartnerSession): void {
    const parts = [
      `${sessionCookieName()}=${session.token}`,
      "Path=/",
      "HttpOnly",
      "SameSite=Lax",
      `Max-Age=${this.config.get("SALES_SESSION_TTL_SECONDS", { infer: true })}`,
    ];
    if (this.secureCookies()) parts.push("Secure");
    reply.header("set-cookie", parts.join("; "));
    reply.header("cache-control", "no-store");
  }

  private clearCookie(): string {
    const secure = this.secureCookies() ? "; Secure" : "";
    return `${sessionCookieName()}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`;
  }

  private secureCookies(): boolean {
    return this.config.get("NODE_ENV", { infer: true }) === "production";
  }
}

function requestMetadata(request: RequestLike): { userAgent: string | null; ipAddress: string | null } {
  return {
    userAgent: typeof request.headers["user-agent"] === "string" ? request.headers["user-agent"] : null,
    ipAddress: request.ip ?? null,
  };
}
