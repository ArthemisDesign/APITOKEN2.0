import { CanActivate, ExecutionContext, ForbiddenException, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

@Injectable()
export class OriginGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<{
      method: string; url: string; headers: Record<string, string | string[] | undefined>;
    }>();
    if (["GET", "HEAD", "OPTIONS"].includes(request.method)) return true;
    if (request.url.split("?", 1)[0] === "/v1/payments/cryptomus/webhook") return true;
    const source = typeof request.headers.origin === "string" ? request.headers.origin : null;
    const expected = new URL(this.config.get("PUBLIC_APP_BASE_URL", { infer: true })).origin;
    if (source !== expected) throw new ForbiddenException("request origin is not allowed");
    return true;
  }
}
