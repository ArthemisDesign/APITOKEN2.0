import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { hash, verify, argon2id } from "argon2";
import {
  clearPartnerRateLimit,
  consumePartnerEmailVerification,
  consumePartnerPasswordReset,
  consumePartnerRateLimit,
  createPartner,
  createPartnerSession,
  decodeSalesEncryptionKey,
  encryptSalesToken,
  findPasswordPartner,
  getPartner,
  queuePartnerEmailForAddress,
  resolvePartnerSession,
  revokePartnerSession,
  EmailAlreadyRegisteredError,
  InvalidInviteError,
  ReferralCodeCollisionError,
  type Partner,
  type PartnerAuthPurpose,
  type SalesDatabase,
} from "@claude-api/sales-db";
import type { Environment } from "./config.js";
import { SALES_DATABASE } from "./infrastructure.module.js";

const dummyHash = hash("not-a-real-partner-password", passwordHashOptions());

const REFERRAL_CODE_ALPHABET = "abcdefghijklmnopqrstuvwxyz234567";

export interface PartnerView {
  id: string;
  email: string;
  displayName: string | null;
  status: Partner["status"];
  emailVerified: boolean;
  referralCode: string;
  commissionBps: number;
  subCommissionBps: number;
  payoutMethod: string | null;
  payoutDetails: unknown;
}

export interface PartnerSession {
  sessionId: string;
  token: string;
  expiresAt: Date;
  partner: PartnerView;
}

export type LoginResult =
  | { kind: "session"; session: PartnerSession }
  | { kind: "verification_required" };

export class InvalidCredentialsError extends Error {}
export class PartnerSuspendedError extends Error {}
export class AuthRateLimitedError extends Error {}
export class InvalidAuthTokenError extends Error {}
export { EmailAlreadyRegisteredError, InvalidInviteError };

@Injectable()
export class AuthService {
  constructor(
    @Inject(SALES_DATABASE) private readonly database: SalesDatabase,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async register(input: {
    email: string;
    password: string;
    displayName?: string | undefined;
    inviteCode?: string | undefined;
    ipAddress: string | null;
  }): Promise<PartnerView> {
    await this.enforceRateLimits("register", input.email, input.ipAddress, 5, 20, 3600);
    const passwordHash = await hash(input.password, passwordHashOptions());
    for (let attempt = 0; ; attempt += 1) {
      try {
        const partner = await createPartner(this.database, {
          email: input.email,
          passwordHash,
          displayName: input.displayName ?? null,
          referralCode: generateCode(8),
          inviteCode: input.inviteCode ?? null,
          commissionBps: this.config.get("DEFAULT_COMMISSION_BPS", { infer: true }),
          subCommissionBps: this.config.get("DEFAULT_SUB_COMMISSION_BPS", { infer: true }),
          verification: this.createAuthEmailSecret("verify_email"),
        });
        return partnerView(partner);
      } catch (error) {
        if (error instanceof ReferralCodeCollisionError && attempt < 5) continue;
        throw error;
      }
    }
  }

  async login(input: {
    email: string;
    password: string;
    userAgent: string | null;
    ipAddress: string | null;
  }): Promise<LoginResult> {
    const keys = await this.enforceRateLimits("login", input.email, input.ipAddress, 10, 50, 900);
    const partner = await findPasswordPartner(this.database, input.email);
    const candidateHash = partner?.passwordHash ?? await dummyHash;
    let valid = false;
    try {
      valid = await verify(candidateHash, input.password);
    } catch {
      valid = false;
    }
    if (!partner || !valid) throw new InvalidCredentialsError("invalid email or password");
    if (partner.status === "suspended") throw new PartnerSuspendedError("partner account is suspended");
    if (partner.status === "pending" || !partner.emailVerified) return { kind: "verification_required" };
    await clearPartnerRateLimit(this.database, keys);
    const session = await this.issueSession(partner, input.userAgent, input.ipAddress);
    return { kind: "session", session };
  }

  async authenticate(token: string): Promise<{ sessionId: string; partner: PartnerView } | null> {
    if (!/^[A-Za-z0-9_-]{43}$/.test(token)) return null;
    const resolved = await resolvePartnerSession(this.database, tokenHash(token));
    return resolved ? { sessionId: resolved.sessionId, partner: partnerView(resolved.partner) } : null;
  }

  async logout(sessionId: string, partnerId: string): Promise<void> {
    await revokePartnerSession(this.database, sessionId, partnerId);
  }

  async verifyEmail(input: {
    token: string; userAgent: string | null; ipAddress: string | null;
  }): Promise<PartnerSession> {
    const partnerId = await consumePartnerEmailVerification(this.database, tokenHash(input.token));
    if (!partnerId) throw new InvalidAuthTokenError("email verification link is invalid or expired");
    const partner = await getPartner(this.database, partnerId);
    if (!partner || partner.status !== "active") {
      throw new InvalidAuthTokenError("partner account is unavailable");
    }
    return this.issueSession(partner, input.userAgent, input.ipAddress);
  }

  async resendVerification(email: string, ipAddress: string | null): Promise<void> {
    await this.enforceRateLimits("verify-resend", email, ipAddress, 3, 10, 3600);
    await queuePartnerEmailForAddress(this.database, {
      email,
      purpose: "verify_email",
      ...this.createAuthEmailSecret("verify_email"),
    });
  }

  async requestPasswordReset(email: string, ipAddress: string | null): Promise<void> {
    await this.enforceRateLimits("password-reset", email, ipAddress, 3, 10, 3600);
    await queuePartnerEmailForAddress(this.database, {
      email,
      purpose: "reset_password",
      ...this.createAuthEmailSecret("reset_password"),
    });
  }

  async resetPassword(token: string, password: string): Promise<void> {
    const passwordHash = await hash(password, passwordHashOptions());
    if (!await consumePartnerPasswordReset(this.database, tokenHash(token), passwordHash)) {
      throw new InvalidAuthTokenError("password reset link is invalid or expired");
    }
  }

  private async issueSession(partner: Partner, userAgent: string | null, ipAddress: string | null): Promise<PartnerSession> {
    const token = randomBytes(32).toString("base64url");
    const ttlSeconds = this.config.get("SALES_SESSION_TTL_SECONDS", { infer: true });
    const expiresAt = new Date(Date.now() + ttlSeconds * 1000);
    const sessionId = await createPartnerSession(this.database, {
      partnerId: partner.id,
      tokenHash: tokenHash(token),
      expiresAt,
      userAgent: userAgent?.slice(0, 500) ?? null,
      ipAddress: ipAddress?.slice(0, 100) ?? null,
    });
    return { sessionId, token, expiresAt, partner: partnerView(partner) };
  }

  private async enforceRateLimits(
    scope: string,
    email: string,
    ipAddress: string | null,
    emailMaximum: number,
    ipMaximum: number,
    windowSeconds: number,
  ): Promise<string[]> {
    const keys = [rateKey(`${scope}:email:${email.toLowerCase()}`), rateKey(`${scope}:ip:${ipAddress ?? "unknown"}`)];
    const [emailAllowed, ipAllowed] = await Promise.all([
      consumePartnerRateLimit(this.database, { keyHash: keys[0]!, maximum: emailMaximum, windowSeconds }),
      consumePartnerRateLimit(this.database, { keyHash: keys[1]!, maximum: ipMaximum, windowSeconds }),
    ]);
    if (!emailAllowed || !ipAllowed) throw new AuthRateLimitedError("too many authentication attempts");
    return keys;
  }

  private createAuthEmailSecret(purpose: PartnerAuthPurpose): {
    tokenHash: string; encryptedToken: string; expiresAt: Date;
  } {
    const token = randomBytes(32).toString("base64url");
    const ttl = this.config.get(
      purpose === "verify_email" ? "EMAIL_VERIFICATION_TTL_SECONDS" : "PASSWORD_RESET_TTL_SECONDS",
      { infer: true },
    );
    const key = decodeSalesEncryptionKey(this.config.get("SALES_TOKEN_ENCRYPTION_KEY", { infer: true }));
    return {
      tokenHash: tokenHash(token),
      encryptedToken: encryptSalesToken(token, key),
      expiresAt: new Date(Date.now() + ttl * 1000),
    };
  }
}

export function partnerView(partner: Partner): PartnerView {
  return {
    id: partner.id,
    email: partner.email,
    displayName: partner.displayName,
    status: partner.status,
    emailVerified: partner.emailVerified,
    referralCode: partner.referralCode,
    commissionBps: partner.commissionBps,
    subCommissionBps: partner.subCommissionBps,
    payoutMethod: partner.payoutMethod,
    payoutDetails: partner.payoutDetails,
  };
}

export function generateCode(length: number): string {
  const bytes = randomBytes(length);
  let code = "";
  for (let index = 0; index < length; index += 1) {
    code += REFERRAL_CODE_ALPHABET[bytes[index]! % REFERRAL_CODE_ALPHABET.length];
  }
  return code;
}

function passwordHashOptions(): Parameters<typeof hash>[1] {
  return { type: argon2id, memoryCost: 19_456, timeCost: 2, parallelism: 1 };
}

function tokenHash(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function rateKey(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}
