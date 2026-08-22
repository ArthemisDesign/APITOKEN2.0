import { createHash, createHmac, timingSafeEqual } from "node:crypto";
import { Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { MANAGED_ADMIN_DOMAINS, type ManagedAdminDomain } from "@claude-api/db";
import { z } from "zod";
import type { Environment } from "./config.js";
import {
  AdminAccountsService,
  type ManagedAdminAuthIdentity,
} from "./admin-accounts.service.js";

export const ADMIN_SESSION_COOKIE = "__Host-apitoken_admin_session";
export const ADMIN_SESSION_TTL_SECONDS = 180 * 24 * 60 * 60;
export const ADMIN_AUTH_VERIFY_CACHE_TTL_MS = 2_000;
const ADMIN_SESSION_TTL_MS = ADMIN_SESSION_TTL_SECONDS * 1_000;
const CLOCK_SKEW_MS = 60_000;
const VERIFY_CACHE_MAX = 256;

const payloadSchema = z.object({
  v: z.literal(1),
  sub: z.string().uuid(),
  domain: z.enum(MANAGED_ADMIN_DOMAINS),
  session_version: z.string().regex(/^[A-Za-z0-9_-]{43}$/),
  issued_at: z.number().int().nonnegative(),
  expires_at: z.number().int().positive(),
}).strict();

@Injectable()
export class AdminSessionService {
  private readonly verifyCache = new Map<string, { identity: ManagedAdminAuthIdentity; expiresAt: number }>();

  constructor(
    private readonly accounts: AdminAccountsService,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  issue(identity: ManagedAdminAuthIdentity, domain: ManagedAdminDomain): string {
    const issuedAt = Date.now();
    const payload = Buffer.from(JSON.stringify({
      v: 1,
      sub: identity.id,
      domain,
      session_version: identity.sessionVersion,
      issued_at: issuedAt,
      expires_at: issuedAt + ADMIN_SESSION_TTL_MS,
    }), "utf8").toString("base64url");
    return `${payload}.${this.sign(payload, this.signingKeys()[0]!)}`;
  }

  async authenticate(token: string | null, domain: ManagedAdminDomain): Promise<ManagedAdminAuthIdentity | null> {
    const payload = this.verify(token);
    if (!payload || payload.domain !== domain || !token) return null;
    const cacheKey = createHash("sha256").update(`${domain}\0${token}`, "utf8").digest("base64url");
    const now = Date.now();
    const cached = this.verifyCache.get(cacheKey);
    if (cached && cached.expiresAt > now) return cached.identity;
    const identity = await this.accounts.resolveSessionIdentity({
      accountId: payload.sub,
      domain,
      sessionVersion: payload.session_version,
    });
    if (!identity) return null;
    if (this.verifyCache.size >= VERIFY_CACHE_MAX) {
      const oldest = this.verifyCache.keys().next().value;
      if (oldest) this.verifyCache.delete(oldest);
    }
    this.verifyCache.set(cacheKey, { identity, expiresAt: now + ADMIN_AUTH_VERIFY_CACHE_TTL_MS });
    return identity;
  }

  private verify(token: string | null): z.infer<typeof payloadSchema> | null {
    if (!token || token.length > 2_048) return null;
    const parts = token.split(".");
    if (parts.length !== 2) return null;
    const [encoded, suppliedSignature] = parts;
    if (!encoded || !suppliedSignature || suppliedSignature.length !== 43 ||
        !/^[A-Za-z0-9_-]+$/.test(encoded) ||
        !/^[A-Za-z0-9_-]+$/.test(suppliedSignature)) return null;
    const signatureValid = this.signingKeys().some((key) => safeEqualBase64Url(
      suppliedSignature,
      this.sign(encoded, key),
    ));
    if (!signatureValid) return null;
    let parsed: unknown;
    try {
      parsed = JSON.parse(Buffer.from(encoded, "base64url").toString("utf8"));
    } catch {
      return null;
    }
    const result = payloadSchema.safeParse(parsed);
    if (!result.success) return null;
    const now = Date.now();
    if (result.data.issued_at > now + CLOCK_SKEW_MS || result.data.expires_at <= now ||
        result.data.expires_at - result.data.issued_at !== ADMIN_SESSION_TTL_MS) return null;
    return result.data;
  }

  private sign(payload: string, key: string): string {
    return createHmac("sha256", key)
      .update("managed-admin-session-v1\0", "utf8")
      .update(payload, "ascii")
      .digest("base64url");
  }

  private signingKeys(): string[] {
    const keys = [
      this.config.get("COMMERCIAL_ADMIN_KEY", { infer: true }),
      this.config.get("COMMERCIAL_ADMIN_PREVIOUS_KEY", { infer: true }),
    ].filter((value): value is string => typeof value === "string" && value.length >= 32);
    if (keys.length === 0) throw new Error("managed admin session key is unavailable");
    return [...new Set(keys)];
  }
}

function safeEqualBase64Url(left: string, right: string): boolean {
  let a: Buffer;
  let b: Buffer;
  try {
    a = Buffer.from(left, "base64url");
    b = Buffer.from(right, "base64url");
  } catch {
    return false;
  }
  return a.length === b.length && timingSafeEqual(a, b);
}
