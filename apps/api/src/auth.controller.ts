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
  Patch,
  Post,
  Query,
  Redirect,
  Req,
  Res,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { z } from "zod";
import {
  authTokenSchema,
  emailOnlySchema,
  loginSchema,
  registerSchema,
  resetPasswordSchema,
  updateProfileSchema,
  verifyEmailSchema,
} from "@claude-api/contracts";
import {
  EmailAlreadyRegisteredError,
  InvalidBusinessInvitationError,
  type OAuthProvider,
} from "@claude-api/db";
import type { Environment } from "./config.js";
import { CurrentAuth, type RequestAuth, SessionAuthGuard, sessionCookieName } from "./auth.guard.js";
import {
  AuthRateLimitedError,
  AuthService,
  EmailVerificationRequiredError,
  InvalidAuthTokenError,
  InvalidCredentialsError,
  InvalidOAuthTransactionError,
  type AuthSession,
} from "./auth.service.js";
import { OAuthProviderError } from "./auth-providers.js";

import { randomBytes } from "node:crypto";

interface ReplyLike { header(name: string, value: string | string[]): void }
interface RequestLike { headers: Record<string, string | string[] | undefined>; ip?: string }

// Долгоживущая анонимная метка браузера для антифрода (бонус-дедуп + журнал устройств).
// Сервер хранит только SHA-256 от значения. Куки нет/подтёрта → сигнал просто отсутствует.
const DEVICE_COOKIE = "apitoken_device";
const DEVICE_COOKIE_TTL_SECONDS = 2 * 365 * 24 * 3600;
const oauthCallbackSchema = z.object({ code: z.string().min(1), state: authTokenSchema, error: z.string().optional() });

@Controller("auth")
export class AuthController {
  constructor(private readonly auth: AuthService, private readonly config: ConfigService<Environment, true>) {}

  @Post("register")
  async register(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = registerSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid registration data");
    try {
      const result = await this.auth.register({ ...parsed.data, ...requestMetadata(request) });
      const device = issueDeviceCookie(request, this.secureCookies());
      if (result.session) this.setSession(reply, result.session, device ? [device] : []);
      else if (device) reply.header("set-cookie", device);
      return { user: result.user, verificationRequired: result.session === null };
    } catch (error) {
      if (error instanceof EmailAlreadyRegisteredError) throw new ConflictException("email is already registered");
      if (error instanceof InvalidBusinessInvitationError) throw new BadRequestException(error.message);
      if (error instanceof AuthRateLimitedError) throw new HttpException(error.message, HttpStatus.TOO_MANY_REQUESTS);
      throw error;
    }
  }

  @Post("business-invite/preview")
  @HttpCode(200)
  async businessInvitePreview(@Body() body: unknown): Promise<unknown> {
    const parsed = z.object({ token: authTokenSchema }).strict().safeParse(body);
    if (!parsed.success) return { valid: false };
    return this.auth.businessInvitePreview(parsed.data.token);
  }

  @Post("login")
  @HttpCode(200)
  async login(@Body() body: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike): Promise<unknown> {
    const parsed = loginSchema.safeParse(body);
    if (!parsed.success) throw new UnauthorizedException("invalid email or password");
    try {
      const session = await this.auth.login({ ...parsed.data, ...requestMetadata(request) });
      const device = issueDeviceCookie(request, this.secureCookies());
      this.setSession(reply, session, device ? [device] : []);
      return { user: session.user };
    } catch (error) {
      if (error instanceof InvalidCredentialsError) throw new UnauthorizedException("invalid email or password");
      if (error instanceof EmailVerificationRequiredError) throw new UnauthorizedException(error.message);
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
      const device = issueDeviceCookie(request, this.secureCookies());
      this.setSession(reply, session, device ? [device] : []);
      return { user: session.user };
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

  @Get("google")
  @Redirect()
  startGoogle(
    @Query("invite") invite: string | undefined,
    @Query("ref") ref: string | undefined,
    @Req() request: RequestLike,
    @Res({ passthrough: true }) reply: ReplyLike,
  ) {
    return this.startOAuth("google", invite, ref, request, reply);
  }

  @Get("github")
  @Redirect()
  startGitHub(
    @Query("invite") invite: string | undefined,
    @Query("ref") ref: string | undefined,
    @Req() request: RequestLike,
    @Res({ passthrough: true }) reply: ReplyLike,
  ) {
    return this.startOAuth("github", invite, ref, request, reply);
  }

  @Get("google/callback")
  @Redirect()
  completeGoogle(@Query() query: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike) {
    return this.completeOAuth("google", query, request, reply);
  }

  @Get("github/callback")
  @Redirect()
  completeGitHub(@Query() query: unknown, @Req() request: RequestLike, @Res({ passthrough: true }) reply: ReplyLike) {
    return this.completeOAuth("github", query, request, reply);
  }

  @Get("me")
  @Header("Cache-Control", "no-store")
  @UseGuards(SessionAuthGuard)
  me(@CurrentAuth() current: RequestAuth): unknown {
    return { user: current.user };
  }

  @Patch("me")
  @UseGuards(SessionAuthGuard)
  async updateProfile(@Body() body: unknown, @CurrentAuth() current: RequestAuth): Promise<unknown> {
    const parsed = updateProfileSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException("invalid profile data");
    const user = await this.auth.updateProfile(current.user.id, parsed.data.displayName);
    if (!user) throw new UnauthorizedException("account is unavailable");
    return { user };
  }

  @Post("logout")
  @HttpCode(204)
  @UseGuards(SessionAuthGuard)
  async logout(@CurrentAuth() current: RequestAuth, @Res({ passthrough: true }) reply: ReplyLike): Promise<void> {
    await this.auth.logout(current.sessionId, current.user.id);
    reply.header("set-cookie", clearCookie());
    reply.header("clear-site-data", '"cookies", "storage"');
  }

  @Get("providers")
  @Header("Cache-Control", "no-store")
  providers(): unknown {
    const configured = this.auth.providerStatus();
    return {
      email: {
        password: true,
        verificationRequired: this.config.get("EMAIL_VERIFICATION_REQUIRED", { infer: true }) === true,
      },
      google: { configured: configured.google, enabled: configured.google },
      github: { configured: configured.github, enabled: configured.github, emailScope: "user:email" },
    };
  }

  private async startOAuth(
    provider: OAuthProvider,
    invite: string | undefined,
    ref: string | undefined,
    request: RequestLike,
    reply: ReplyLike,
  ) {
    if (invite !== undefined && !authTokenSchema.safeParse(invite).success) throw new BadRequestException("invalid invite token");
    // ref валидируется мягко в auth.service (normalizeReferralCode); мусор просто игнорируется.
    try {
      const result = await this.auth.beginOAuth(provider, invite, ref);
      // Device-метку ставим ДО редиректа в провайдера — callback уже видит её и может
      // сверить бонус-дедуп по устройству.
      const cookies = [oauthCookie(result.state, this.secureCookies())];
      const device = issueDeviceCookie(request, this.secureCookies());
      if (device) cookies.push(device);
      reply.header("set-cookie", cookies);
      reply.header("cache-control", "no-store");
      return { url: result.authorizationUrl, statusCode: 302 };
    } catch (error) {
      if (error instanceof OAuthProviderError) throw new BadRequestException(error.message);
      throw error;
    }
  }

  private async completeOAuth(
    provider: OAuthProvider,
    query: unknown,
    request: RequestLike,
    reply: ReplyLike,
  ) {
    const parsed = oauthCallbackSchema.safeParse(query);
    if (!parsed.success || parsed.data.error) return this.oauthErrorRedirect(reply, provider, "invalid_callback");
    try {
      const session = await this.auth.completeOAuth({
        provider,
        code: parsed.data.code,
        state: parsed.data.state,
        stateCookie: readCookieHeader(request.headers.cookie, "apitoken_oauth_state"),
        ...requestMetadata(request),
      });
      const cookies = [clearOAuthCookie(this.secureCookies()), sessionCookie(session, this.config)];
      const device = issueDeviceCookie(request, this.secureCookies());
      if (device) cookies.push(device);
      reply.header("set-cookie", cookies);
      reply.header("cache-control", "no-store");
      return { url: new URL(`/auth/callback?provider=${provider}`, this.config.get("PUBLIC_APP_BASE_URL", { infer: true })).toString(), statusCode: 302 };
    } catch (error) {
      const code = error instanceof InvalidOAuthTransactionError ? "invalid_state" :
        error instanceof OAuthProviderError ? "provider_error" : "authentication_failed";
      return this.oauthErrorRedirect(reply, provider, code);
    }
  }

  private oauthErrorRedirect(reply: ReplyLike, provider: OAuthProvider, code: string) {
    reply.header("set-cookie", clearOAuthCookie(this.secureCookies()));
    reply.header("cache-control", "no-store");
    const url = new URL("/auth/callback", this.config.get("PUBLIC_APP_BASE_URL", { infer: true }));
    url.searchParams.set("provider", provider);
    url.searchParams.set("error", code);
    return { url: url.toString(), statusCode: 302 };
  }

  private setSession(reply: ReplyLike, session: AuthSession, extraCookies: string[] = []): void {
    const secure = this.config.get("NODE_ENV", { infer: true }) === "production";
    const parts = [
      `${sessionCookieName()}=${session.token}`,
      "Path=/",
      "HttpOnly",
      "SameSite=Lax",
      `Max-Age=${this.config.get("SESSION_TTL_SECONDS", { infer: true })}`,
    ];
    if (secure) parts.push("Secure");
    const cookie = parts.join("; ");
    reply.header("set-cookie", extraCookies.length > 0 ? [cookie, ...extraCookies] : cookie);
    reply.header("cache-control", "no-store");
  }

  private secureCookies(): boolean {
    return this.config.get("NODE_ENV", { infer: true }) === "production";
  }
}

function requestMetadata(request: RequestLike): {
  userAgent: string | null; ipAddress: string | null; deviceToken: string | null;
} {
  return {
    userAgent: typeof request.headers["user-agent"] === "string" ? request.headers["user-agent"] : null,
    ipAddress: request.ip ?? null,
    deviceToken: readCookieHeader(request.headers.cookie, DEVICE_COOKIE),
  };
}

function deviceCookie(token: string, secure: boolean): string {
  return `${DEVICE_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${DEVICE_COOKIE_TTL_SECONDS}${secure ? "; Secure" : ""}`;
}

/** Set-Cookie для новой device-метки, либо null, когда валидная уже стоит. */
function issueDeviceCookie(request: RequestLike, secure: boolean): string | null {
  const existing = readCookieHeader(request.headers.cookie, DEVICE_COOKIE);
  if (existing && /^[A-Za-z0-9_-]{43}$/.test(existing)) return null;
  return deviceCookie(randomBytes(32).toString("base64url"), secure);
}

function clearCookie(): string {
  const secure = process.env.NODE_ENV === "production" ? "; Secure" : "";
  return `${sessionCookieName()}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0${secure}`;
}

function oauthCookie(state: string, secure: boolean): string {
  return `apitoken_oauth_state=${state}; Path=/v1/auth; HttpOnly; SameSite=Lax; Max-Age=600${secure ? "; Secure" : ""}`;
}

function clearOAuthCookie(secure: boolean): string {
  return `apitoken_oauth_state=; Path=/v1/auth; HttpOnly; SameSite=Lax; Max-Age=0${secure ? "; Secure" : ""}`;
}

function readCookieHeader(header: string | string[] | undefined, name: string): string | null {
  if (typeof header !== "string") return null;
  for (const item of header.split(";")) {
    const [key, ...rest] = item.trim().split("=");
    if (key === name) return rest.join("=");
  }
  return null;
}

function sessionCookie(session: AuthSession, config: ConfigService<Environment, true>): string {
  const secure = config.get("NODE_ENV", { infer: true }) === "production";
  return `${sessionCookieName()}=${session.token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${config.get("SESSION_TTL_SECONDS", { infer: true })}${secure ? "; Secure" : ""}`;
}
