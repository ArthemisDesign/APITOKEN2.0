import { CanActivate, ExecutionContext, ForbiddenException, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { safeEqual } from "./admin.guard.js";
import type { Environment } from "./config.js";

@Injectable()
export class OriginGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<{
      method: string; url: string; headers: Record<string, string | string[] | undefined>;
    }>();
    if (["GET", "HEAD", "OPTIONS"].includes(request.method)) return true;
    const path = request.url.split("?", 1)[0];
    if (path === "/v1/payments/cryptomus/webhook" || path === "/v1/payments/platega/webhook") return true;
    // Origin-проверка — защита от CSRF из браузера. Запрос с валидным admin-ключом CSRF быть
    // не может (кастомный заголовок нельзя послать кросс-сайт без CORS), а приходит он с другого
    // origin — панели (Caddy panel.apitoken.sale инжектит ключ server-side). Пропускаем.
    const adminKey = this.config.get("COMMERCIAL_ADMIN_KEY", { infer: true });
    const supplied = request.headers["x-admin-key"];
    if (adminKey && typeof supplied === "string" && safeEqual(adminKey, supplied)) return true;
    const source = typeof request.headers.origin === "string" ? request.headers.origin : null;
    const expected = new URL(this.config.get("PUBLIC_APP_BASE_URL", { infer: true })).origin;
    if (source !== expected) throw new ForbiddenException("request origin is not allowed");
    return true;
  }
}
