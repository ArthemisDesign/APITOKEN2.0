import {
  BadRequestException,
  Body,
  ConflictException,
  Controller,
  ForbiddenException,
  Get,
  Header,
  HttpCode,
  HttpException,
  HttpStatus,
  Post,
  Req,
  Res,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard, sessionCookieName } from "./auth.guard.js";
import {
  AuthRateLimitedError,
  AuthService,
  EmailAlreadyRegisteredError,
  InvalidAuthTokenError,
  InvalidCredentialsError,
  InvalidInviteError,
  PartnerSuspendedError,
  type PartnerSession,
} from "./auth.service.js";
import {
  emailOnlySchema,
  loginSchema,
  registerSchema,
  resetPasswordSchema,
  verifyEmailSchema,
} from "./schemas.js";

interface ReplyLike { header(name: string, value: string | string[]): void }
interface RequestLike { headers: Record<string, string | string[] | undefined>; ip?: string }

@Controller("auth")
export class AuthController {
  constructor(private readonly auth: AuthService, private readonly config: ConfigService<Environment, true>) {}

  @Post("register")
  async register(@Body() body: unknown, @Req() request: RequestLike): Promise<unknown> {
    const parsed = registerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid registration data");
    try {
      const partner = await this.auth.register({ ...parsed.data, ipAddress: request.ip ?? null });
      return { partner, verificationRequired: true };
    } catch (error) {
      if (error instanceof EmailAlreadyRegisteredError) throw new ConflictException("email is already registered");
      if (error instanceof InvalidInviteError) throw new BadRequestException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Post("login")
  @HttpCode(200)
  async login(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = loginSchema.safeParse(body);
    if (!parsed.success) throw new UnauthorizedException("invalid email or password");
    try {
      const result = await this.auth.login({ ...parsed.data, ...requestMetadata(request) });
      if (result.kind === "verification_required") return { verificationRequired: true };
      this.setSession(reply, result.session);
      return { partner: result.session.partner };
    } catch (error) {
      if (error instanceof InvalidCredentialsError) throw new UnauthorizedException("invalid email or password");
      if (error instanceof PartnerSuspendedError) throw new ForbiddenException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Post("email/verify")
  @HttpCode(200)
  async verifyEmail(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = verifyEmailSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid verification token");
    try {
      const session = await this.auth.verifyEmail({ ...parsed.data, ...requestMetadata(request) });
      this.setSession(reply, session);
      return { partner: session.partner };
    } catch (error) {
      if (error instanceof InvalidAuthTokenError) throw new BadRequestException(error.message);
      throw error;
    }
  }

  @Post("email/resend")
  @HttpCode(202)
  async resendVerification(@Body() body: unknown, @Req() request: RequestLike): Promise<unknown> {
    const parsed = emailOnlySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid email");
    try {
      await this.auth.resendVerification(parsed.data.email, request.ip ?? null);
      return { accepted: true };
    } catch (error) {
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Post("password/forgot")
  @HttpCode(202)
  async forgotPassword(@Body() body: unknown, @Req() request: RequestLike): Promise<unknown> {
    const parsed = emailOnlySchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid email");
    try {
      await this.auth.requestPasswordReset(parsed.data.email, request.ip ?? null);
      return { accepted: true };
    } catch (error) {
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Post("password/reset")
  @HttpCode(204)
  async resetPassword(@Body() body: unknown): Promise<void> {
    const parsed = resetPasswordSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid password reset data");
    try {
      await this.auth.resetPassword(parsed.data.token, parsed.data.password);
    } catch (error) {
      if (error instanceof InvalidAuthTokenError) throw new BadRequestException(error.message);
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
