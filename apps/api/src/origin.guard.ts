import { CanActivate, ExecutionContext, ForbiddenException, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { matchesConfiguredAdminKey } from "./admin.guard.js";
import type { Environment } from "./config.js";

@Injectable()
export class OriginGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<{
      method: string; url: string; headers: Record<string, string | string[] | undefined>;
    }>();
    if (["GET", "HEAD", "OPTIONS"].includes(request.method)) return true;
    const path = request.url.split("?", 1)[0] ?? request.url;
    if (path === "/v1/payments/cryptomus/webhook" || path === "/v1/payments/platega/webhook") return true;
    // Внутренний контур sales↔commerce (/v1/internal/*): service-to-service под собственным
    // ключом (SalesFeedGuard, x-api-key). Браузер кастомный заголовок кросс-сайт не пошлёт
    // (CORS-preflight заблокирует) → CSRF невозможен, как и для x-admin-key ниже.
    if (path.startsWith("/v1/internal/")) return true;
    // Origin-проверка — защита от CSRF из браузера. Запрос с валидным admin-ключом CSRF быть
    // не может (кастомный заголовок нельзя послать кросс-сайт без CORS), а приходит он с другого
    // origin — admin-сайта (Caddy admin.apitoken.sale инжектит ключ server-side). Пропускаем.
    const supplied = request.headers["x-admin-key"];
    if (matchesConfiguredAdminKey(this.config, supplied)) return true;
    const source = typeof request.headers.origin === "string" ? request.headers.origin : null;
    const expected = new URL(this.config.get("PUBLIC_APP_BASE_URL", { infer: true })).origin;
    if (source !== expected) throw new ForbiddenException("request origin is not allowed");
    return true;
  }
}
