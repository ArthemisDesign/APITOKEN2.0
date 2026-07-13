import {
  Body,
  BadRequestException,
  ConflictException,
  Controller,
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
import { loginSchema, registerSchema } from "@claude-api/contracts";
import { EmailAlreadyRegisteredError } from "@claude-api/db";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard, sessionCookieName } from "./auth.guard.js";
import { AuthRateLimitedError, AuthService, EmailVerificationRequiredError, InvalidCredentialsError, type AuthSession } from "./auth.service.js";

interface ReplyLike { header(name: string, value: string): void }
interface RequestLike { headers: Record<string, string | string[] | undefined>; ip?: string }

@Controller("auth")
export class AuthController {
  constructor(private readonly auth: AuthService, private readonly config: ConfigService<Environment, true>) {}

  @Post("register")
  async register(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = registerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid registration data");
    try {
      const result = await this.auth.register({ ...parsed.data, ...requestMetadata(request) });
      if (result.session) this.setSession(reply, result.session);
      return { user: result.user, verificationRequired: result.session === null };
    } catch (error) {
      if (error instanceof EmailAlreadyRegisteredError) throw new ConflictException("email is already registered");
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
      const session = await this.auth.login({ ...parsed.data, ...requestMetadata(request) });
      this.setSession(reply, session);
      return { user: session.user };
    } catch (error) {
      if (error instanceof InvalidCredentialsError) throw new UnauthorizedException("invalid email or password");
      if (error instanceof EmailVerificationRequiredError) throw new UnauthorizedException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Get("me")
  @Header("Cache-Control", "no-store")
  @UseGuards(SessionAuthGuard)
  me(@CurrentAuth() current: RequestAuth): unknown {
    return { user: current.user };
  }

  @Post("logout")
  @HttpCode(204)
  @UseGuards(SessionAuthGuard)
  async logout(@CurrentAuth() current: RequestAuth, @Res({ passthrough: true }) reply: ReplyLike): Promise<void> {
    await this.auth.logout(current.sessionId, current.user.id);
    reply.header("set-cookie", clearCookie());
    reply.header("clear-site-data", '"cache", "cookies", "storage"');
  }

  @Get("providers")
  @Header("Cache-Control", "no-store")
  providers(): unknown {
    return {
      email: { password: true, deliveryConnected: false },
      google: {
        configured: this.config.get("GOOGLE_CLIENT_ID", { infer: true }) !== undefined,
        enabled: false,
      },
    };
  }

  private setSession(reply: ReplyLike, session: AuthSession): void {
    const secure = this.config.get("NODE_ENV", { infer: true }) === "production";
    const parts = [
      `${sessionCookieName()}=${session.token}`,
      "Path=/",
      "HttpOnly",
      "SameSite=Lax",
      `Max-Age=${this.config.get("SESSION_TTL_SECONDS", { infer: true })}`,
    ];
    if (secure) parts.push("Secure");
    reply.header("set-cookie", parts.join("; "));
    reply.header("cache-control", "no-store");
  }
}

function requestMetadata(request: RequestLike): { userAgent: string | null; ipAddress: string | null } {
  return {
    userAgent: typeof request.headers["user-agent"] === "string" ? request.headers["user-agent"] : null,
    ipAddress: request.ip ?? null,
  };
}

function clearCookie(): string {
  const secure = process.env.NODE_ENV === "production" ? "; Secure" : "";
  return `${sessionCookieName()}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`;
}
