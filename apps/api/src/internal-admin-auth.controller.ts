import {
  BadRequestException,
  Body,
  Controller,
  ForbiddenException,
  Get,
  Header,
  Headers,
  HttpCode,
  Post,
  Query,
  Req,
  Res,
  UnauthorizedException,
  UseGuards,
} from "@nestjs/common";
import { LegacyAdminImportConflictError, MANAGED_ADMIN_DOMAINS } from "@claude-api/db";
import { z } from "zod";
import {
  AdminAccountsService,
  isLegacyBcryptHash,
  isManagedAdminDomain,
} from "./admin-accounts.service.js";
import { AdminGuard } from "./admin.guard.js";
import {
  ADMIN_SESSION_COOKIE,
  ADMIN_SESSION_TTL_SECONDS,
  AdminSessionService,
} from "./admin-session.service.js";

const legacyRowSchema = z.object({
  username: z.string().regex(/^[A-Za-z0-9._@-]{1,80}$/),
  password_hash: z.string().refine(isLegacyBcryptHash, "invalid legacy bcrypt hash"),
  domains: z.array(z.enum(MANAGED_ADMIN_DOMAINS)).min(1).max(MANAGED_ADMIN_DOMAINS.length),
}).strict();
const legacyImportSchema = z.object({ accounts: z.array(legacyRowSchema).min(2).max(100) }).strict();
const browserLoginSchema = z.object({
  username: z.string().regex(/^[A-Za-z0-9._@-]{1,80}$/),
  password: z.string().min(1).max(1_024),
  return_to: z.string().max(2_048).optional(),
}).strict();

interface ReplyLike {
  header(name: string, value: string | string[]): void;
  status(code: number): ReplyLike;
}
interface RequestLike { headers: Record<string, string | string[] | undefined> }

const SESSION_AUTH_MODE = "session-v1";
const BROWSER_AUTH_PATH = "/__admin-auth";

@Controller("internal/admin-auth")
export class InternalAdminAuthController {
  constructor(
    private readonly accounts: AdminAccountsService,
    private readonly sessions: AdminSessionService,
  ) {}

  @Get("verify")
  @UseGuards(AdminGuard)
  @Header("Cache-Control", "no-store")
  async verify(
    @Req() request: RequestLike,
    @Res({ passthrough: true }) response: ReplyLike,
  ): Promise<Record<string, unknown>> {
    const domain = singleHeader(request.headers["x-admin-domain"]);
    if (!isManagedAdminDomain(domain)) {
      throw new UnauthorizedException("admin authentication required");
    }
    const authorization = singleHeader(request.headers.authorization);
    if (singleHeader(request.headers["x-admin-auth-mode"]) !== SESSION_AUTH_MODE) {
      const legacyAccount = await this.accounts.authenticate({ authorization, domain });
      if (!legacyAccount) {
        response.header("WWW-Authenticate", `Basic realm="${domain}", charset="UTF-8"`);
        throw new UnauthorizedException("admin authentication required");
      }
      setActorHeaders(response, legacyAccount);
      return { authenticated: true };
    }

    const cookie = readCookie(singleHeader(request.headers.cookie) ?? "", ADMIN_SESSION_COOKIE);
    let account = await this.sessions.authenticate(cookie, domain);
    if (!account && authorization) {
      account = await this.accounts.authenticate({ authorization, domain });
      if (account) {
        response.header("Set-Cookie", adminSessionCookie(this.sessions.issue(account, domain)));
      }
    }
    if (!account) {
      const loginLocation = loginUrl(singleHeader(request.headers["x-forwarded-uri"]));
      response.header("X-Admin-Login", loginLocation);
      if (isDocumentNavigation(request.headers)) {
        response.status(303);
        response.header("Location", loginLocation);
        return { authenticated: false };
      }
      response.header("Set-Cookie", clearAdminSessionCookie());
      throw new UnauthorizedException("admin authentication required");
    }
    setActorHeaders(response, account);
    return { authenticated: true };
  }

  @Get("browser/login")
  @UseGuards(AdminGuard)
  @Header("Cache-Control", "no-store")
  @Header("Content-Type", "text/html; charset=utf-8")
  @Header("Referrer-Policy", "same-origin")
  @Header(
    "Content-Security-Policy",
    "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
  )
  loginPage(
    @Headers("x-admin-domain") domain: string | undefined,
    @Query("return_to") returnTo: string | undefined,
  ): string {
    if (!isManagedAdminDomain(domain)) throw new UnauthorizedException("admin authentication required");
    return renderLoginPage(domain, normalizeReturnTo(returnTo), false);
  }

  @Post("browser/login")
  @UseGuards(AdminGuard)
  @HttpCode(200)
  @Header("Cache-Control", "no-store")
  @Header("Content-Type", "text/html; charset=utf-8")
  @Header("Referrer-Policy", "same-origin")
  async browserLogin(
    @Headers("x-admin-domain") domain: string | undefined,
    @Headers("origin") origin: string | undefined,
    @Headers("referer") referer: string | undefined,
    @Body() body: unknown,
    @Res({ passthrough: true }) response: ReplyLike,
  ): Promise<string> {
    if (!isManagedAdminDomain(domain)) throw new UnauthorizedException("admin authentication required");
    requireSameOrigin({ origin, referer, domain });
    const parsed = browserLoginSchema.safeParse(body);
    const returnTo = normalizeReturnTo(parsed.success ? parsed.data.return_to : undefined);
    if (!parsed.success) {
      response.status(401);
      return renderLoginPage(domain, returnTo, true);
    }
    const account = await this.accounts.authenticatePassword({
      username: parsed.data.username,
      password: parsed.data.password,
      domain,
    });
    if (!account) {
      response.status(401);
      return renderLoginPage(domain, returnTo, true, parsed.data.username);
    }
    response.status(303);
    response.header("Location", returnTo);
    response.header("Set-Cookie", adminSessionCookie(this.sessions.issue(account, domain)));
    return "";
  }

  @Post("browser/logout")
  @UseGuards(AdminGuard)
  @HttpCode(303)
  @Header("Cache-Control", "no-store")
  @Header("Referrer-Policy", "same-origin")
  browserLogout(
    @Headers("x-admin-domain") domain: string | undefined,
    @Headers("origin") origin: string | undefined,
    @Headers("referer") referer: string | undefined,
    @Res({ passthrough: true }) response: ReplyLike,
  ): string {
    if (!isManagedAdminDomain(domain)) throw new UnauthorizedException("admin authentication required");
    requireSameOrigin({ origin, referer, domain });
    response.header("Set-Cookie", clearAdminSessionCookie());
    response.header("Location", `${BROWSER_AUTH_PATH}/login`);
    return "";
  }

  @Post("legacy-import")
  @UseGuards(AdminGuard)
  @Header("Cache-Control", "no-store")
  async importLegacy(@Body() body: unknown): Promise<Record<string, unknown>> {
    const parsed = legacyImportSchema.safeParse(body);
    if (!parsed.success) throw new BadRequestException(parsed.error.flatten());
    try {
      return await this.accounts.importLegacy(parsed.data.accounts.map((account) => ({
        username: account.username,
        passwordHash: account.password_hash,
        domains: account.domains,
      })));
    } catch (error) {
      if (error instanceof LegacyAdminImportConflictError) throw new BadRequestException(error.message);
      throw error;
    }
  }
}

export function adminSessionCookie(token: string, now = Date.now()): string {
  const expires = new Date(now + ADMIN_SESSION_TTL_SECONDS * 1_000).toUTCString();
  return `${ADMIN_SESSION_COOKIE}=${token}; Path=/; HttpOnly; Secure; SameSite=Lax; ` +
    `Max-Age=${ADMIN_SESSION_TTL_SECONDS}; Expires=${expires}; Priority=High`;
}

export function clearAdminSessionCookie(): string {
  return `${ADMIN_SESSION_COOKIE}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0`;
}

function setActorHeaders(response: ReplyLike, account: { id: string; username: string }): void {
  response.header("X-Admin-Actor", account.username);
  response.header("X-Admin-Account-Id", account.id);
}

function isDocumentNavigation(headers: RequestLike["headers"]): boolean {
  const method = singleHeader(headers["x-forwarded-method"])?.toUpperCase();
  if (method !== "GET" && method !== "HEAD") return false;
  if (singleHeader(headers["sec-fetch-dest"])?.toLowerCase() === "document") return true;
  return (singleHeader(headers.accept) ?? "").toLowerCase().split(",").some((part) =>
    part.trim().startsWith("text/html"),
  );
}

function loginUrl(forwardedUri: string | undefined): string {
  return `${BROWSER_AUTH_PATH}/login?return_to=${encodeURIComponent(normalizeReturnTo(forwardedUri))}`;
}

function normalizeReturnTo(value: string | undefined): string {
  if (!value || value.length > 2_048 || !value.startsWith("/") || value.startsWith("//") ||
      value.includes("\\") || /[\u0000-\u001f\u007f]/.test(value) || value.startsWith(BROWSER_AUTH_PATH)) {
    return "/";
  }
  return value;
}

function requireSameOrigin(input: {
  origin: string | undefined;
  referer: string | undefined;
  domain: string;
}): void {
  const expected = `https://${input.domain}`;
  if (input.origin !== undefined) {
    if (input.origin === expected) return;
    throw new ForbiddenException("same-origin form required");
  }
  if (input.referer !== undefined && exactOrigin(input.referer) === expected) return;
  throw new ForbiddenException("same-origin form required");
}

function exactOrigin(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.username || url.password) return null;
    return url.origin;
  } catch {
    return null;
  }
}

function readCookie(header: string, name: string): string | null {
  for (const item of header.split(";")) {
    const separator = item.indexOf("=");
    if (separator < 0) continue;
    if (item.slice(0, separator).trim() === name) return item.slice(separator + 1).trim();
  }
  return null;
}

function singleHeader(value: string | string[] | undefined): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function renderLoginPage(
  domain: string,
  returnTo: string,
  invalid: boolean,
  username = "",
): string {
  const title = domain === "crm.apitoken.sale" ? "CRM" :
    domain === "admin.apitoken.sale" ? "Панель управления" :
    domain === "monitoring.apitoken.sale" ? "Мониторинг" : "Закрытый раздел";
  const error = invalid
    ? '<div class="error" role="alert">Неверный логин или пароль</div>'
    : "";
  return `<!doctype html>
<html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="robots" content="noindex,nofollow"><title>Вход · ${escapeHtml(title)}</title>
<style>:root{color-scheme:dark}*{box-sizing:border-box}body{margin:0;min-height:100svh;display:grid;place-items:center;padding:24px;background:#0c0e12;color:#f4f5f7;font:15px/1.45 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}.card{width:min(100%,390px);padding:28px;border:1px solid #292d36;border-radius:20px;background:#15181f;box-shadow:0 24px 80px #0008}.eyebrow{margin:0 0 8px;color:#78e6aa;font-size:12px;font-weight:800;letter-spacing:.13em;text-transform:uppercase}h1{margin:0 0 8px;font-size:28px;letter-spacing:-.03em}p{margin:0 0 24px;color:#a9afbb}.field{display:grid;gap:7px;margin:0 0 15px}.field span{color:#cbd0d8;font-size:13px;font-weight:700}input{width:100%;height:48px;padding:0 14px;border:1px solid #343946;border-radius:12px;outline:0;background:#0f1116;color:#fff;font:16px inherit}input:focus{border-color:#55d993;box-shadow:0 0 0 3px #55d99322}button{width:100%;height:48px;margin-top:5px;border:0;border-radius:12px;background:#55d993;color:#08130d;font:800 15px inherit;cursor:pointer}.error{margin:0 0 16px;padding:11px 12px;border:1px solid #ff6b6b66;border-radius:10px;background:#ff6b6b12;color:#ffb5b5}.note{margin:16px 0 0;color:#7e8592;font-size:12px;text-align:center}</style></head>
<body><main class="card"><div class="eyebrow">apitoken.sale</div><h1>${escapeHtml(title)}</h1><p>Войдите один раз — сессия сохранится на этом устройстве.</p>${error}
<form method="post" action="${BROWSER_AUTH_PATH}/login"><input type="hidden" name="return_to" value="${escapeHtml(returnTo)}">
<label class="field"><span>Логин</span><input name="username" value="${escapeHtml(username)}" autocomplete="username" autocapitalize="none" spellcheck="false" maxlength="80" required autofocus></label>
<label class="field"><span>Пароль</span><input name="password" type="password" autocomplete="current-password" maxlength="1024" required></label>
<button type="submit">Войти</button></form><div class="note">Защищённая сессия · пароль в браузере не хранится</div></main></body></html>`;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case "&": return "&amp;";
      case "<": return "&lt;";
      case ">": return "&gt;";
      case '"': return "&quot;";
      default: return "&#39;";
    }
  });
}
