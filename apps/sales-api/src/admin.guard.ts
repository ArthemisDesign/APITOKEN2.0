import { createHash, timingSafeEqual } from "node:crypto";
import { CanActivate, ExecutionContext, Injectable, UnauthorizedException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

interface AdminRequest {
  headers: Record<string, string | string[] | undefined>;
}

@Injectable()
export class AdminKeyGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<AdminRequest>();
    // x-sales-admin-key — server-side инжект Caddy panel-домена (отличим от commerce
    // x-admin-key при переносе секретов); x-admin-key — прямой вход через /admin sales-web.
    const provided = request.headers["x-sales-admin-key"] ?? request.headers["x-admin-key"];
    if (typeof provided !== "string" || provided.length === 0) {
      throw new UnauthorizedException("admin key required");
    }
    const expected = this.config.get("SALES_ADMIN_KEY", { infer: true });
    if (!safeEqual(provided, expected)) throw new UnauthorizedException("admin key required");
    return true;
  }
}

function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left, "utf8").digest();
  const b = createHash("sha256").update(right, "utf8").digest();
  return timingSafeEqual(a, b);
}
