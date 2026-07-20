import { createHash, timingSafeEqual } from "node:crypto";
import { CanActivate, ExecutionContext, Injectable, UnauthorizedException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

@Injectable()
export class AdminGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<{ headers: Record<string, string | string[] | undefined> }>();
    const supplied = request.headers["x-admin-key"];
    if (!matchesConfiguredAdminKey(this.config, supplied)) {
      throw new UnauthorizedException("admin authentication required");
    }
    return true;
  }
}

export function matchesConfiguredAdminKey(
  config: ConfigService<Environment, true>,
  supplied: string | string[] | undefined,
): boolean {
  if (typeof supplied !== "string") return false;
  const configured = [
    config.get("COMMERCIAL_ADMIN_KEY", { infer: true }),
    config.get("COMMERCIAL_ADMIN_PREVIOUS_KEY", { infer: true }),
  ].filter((value): value is string => typeof value === "string" && value.length > 0);
  return configured.some((value) => safeEqual(value, supplied));
}

export function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left).digest();
  const b = createHash("sha256").update(right).digest();
  return timingSafeEqual(a, b);
}
