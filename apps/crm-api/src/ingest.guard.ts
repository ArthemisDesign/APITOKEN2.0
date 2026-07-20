import { createHash, timingSafeEqual } from "node:crypto";
import { CanActivate, ExecutionContext, Injectable, UnauthorizedException } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import type { Environment } from "./config.js";

interface IngestRequest {
  headers: Record<string, string | string[] | undefined>;
}

/** Парсеры ходят мимо basic_auth людей — только по x-crm-ingest-key. */
@Injectable()
export class IngestKeyGuard implements CanActivate {
  constructor(private readonly config: ConfigService<Environment, true>) {}

  canActivate(context: ExecutionContext): boolean {
    const request = context.switchToHttp().getRequest<IngestRequest>();
    const provided = request.headers["x-crm-ingest-key"];
    if (typeof provided !== "string" || provided.length === 0) {
      throw new UnauthorizedException("ingest key required");
    }
    const expected = this.config.get("CRM_INGEST_KEY", { infer: true });
    if (!safeEqual(provided, expected)) throw new UnauthorizedException("ingest key required");
    return true;
  }
}

function safeEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left, "utf8").digest();
  const b = createHash("sha256").update(right, "utf8").digest();
  return timingSafeEqual(a, b);
}
