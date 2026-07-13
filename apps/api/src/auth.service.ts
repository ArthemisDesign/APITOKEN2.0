import { createHash, randomBytes } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { hash, verify, argon2id } from "argon2";
import type { AuthUserView } from "@claude-api/contracts";
import {
  completeEngineAccount,
  consumeAuthRateLimit,
  createAuthSession,
  createEmailUser,
  clearAuthRateLimit,
  failEngineAccount,
  findPasswordUser,
  getAuthUser,
  resolveAuthSession,
  revokeAuthSession,
  type AuthUser,
  type Database,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";

const dummyHash = hash("not-a-real-user-password", passwordHashOptions());

export interface AuthSession {
  sessionId: string;
  token: string;
  expiresAt: Date;
  user: AuthUserView;
}

export interface RegistrationResult {
  user: AuthUserView;
  session: AuthSession | null;
}

export class InvalidCredentialsError extends Error {}
export class EmailVerificationRequiredError extends Error {}
export class AuthRateLimitedError extends Error {}

@Injectable()
export class AuthService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly config: ConfigService<Environment, true>,
  ) {}

  async register(input: {
    email: string;
    password: string;
    inviteToken?: string | undefined;
    userAgent: string | null;
    ipAddress: string | null;
  }): Promise<RegistrationResult> {
    await this.enforceRateLimits("register", input.email, input.ipAddress, 5, 20, 3600);
    const passwordHash = await hash(input.password, passwordHashOptions());
    const user = await createEmailUser(
      this.database,
      input.email,
      passwordHash,
      input.inviteToken ? tokenHash(input.inviteToken) : undefined,
    );
    try {
      const account = await this.engine.createAccount({
        handle: `user:${user.id}`,
        multBp: user.engineMultiplierBp,
      });
      await completeEngineAccount(this.database, user.id, account.account);
      user.engineAccountStatus = "active";
    } catch (error) {
      await failEngineAccount(this.database, user.id, error instanceof Error ? error.message : "engine account creation failed");
      user.engineAccountStatus = "error";
    }
    if (this.config.get("REQUIRE_VERIFIED_EMAIL", { infer: true })) {
      return { user: userView(user), session: null };
    }
    const session = await this.issueSession(user, input.userAgent, input.ipAddress);
    return { user: session.user, session };
  }

  async login(input: { email: string; password: string; userAgent: string | null; ipAddress: string | null }): Promise<AuthSession> {
    const keys = await this.enforceRateLimits("login", input.email, input.ipAddress, 10, 50, 900);
    const user = await findPasswordUser(this.database, input.email);
    const candidateHash = user?.passwordHash ?? await dummyHash;
    let valid = false;
    try {
      valid = await verify(candidateHash, input.password);
    } catch {
      valid = false;
    }
    if (!user || !valid || user.status !== "active" || !user.passwordHash) {
      throw new InvalidCredentialsError("invalid email or password");
    }
    if (this.config.get("REQUIRE_VERIFIED_EMAIL", { infer: true }) && !user.emailVerified) {
      throw new EmailVerificationRequiredError("email verification is required");
    }
    await clearAuthRateLimit(this.database, keys);
    return this.issueSession(user, input.userAgent, input.ipAddress);
  }

  async authenticate(token: string): Promise<{ sessionId: string; user: AuthUserView } | null> {
    if (!/^[A-Za-z0-9_-]{43}$/.test(token)) return null;
    const resolved = await resolveAuthSession(this.database, tokenHash(token));
    return resolved ? { sessionId: resolved.sessionId, user: userView(resolved.user) } : null;
  }

  async logout(sessionId: string, userId: string): Promise<void> {
    await revokeAuthSession(this.database, sessionId, userId);
  }

  async getUser(userId: string): Promise<AuthUserView | null> {
    const user = await getAuthUser(this.database, userId);
    return user ? userView(user) : null;
  }

  private async issueSession(user: AuthUser, userAgent: string | null, ipAddress: string | null): Promise<AuthSession> {
    const token = randomBytes(32).toString("base64url");
    const ttlSeconds = this.config.get("SESSION_TTL_SECONDS", { infer: true });
    const expiresAt = new Date(Date.now() + ttlSeconds * 1000);
    const sessionId = await createAuthSession(this.database, {
      userId: user.id,
      tokenHash: tokenHash(token),
      expiresAt,
      userAgent: userAgent?.slice(0, 500) ?? null,
      ipAddress: ipAddress?.slice(0, 100) ?? null,
    });
    return { sessionId, token, expiresAt, user: userView(user) };
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
      consumeAuthRateLimit(this.database, { keyHash: keys[0]!, maximum: emailMaximum, windowSeconds }),
      consumeAuthRateLimit(this.database, { keyHash: keys[1]!, maximum: ipMaximum, windowSeconds }),
    ]);
    if (!emailAllowed || !ipAllowed) throw new AuthRateLimitedError("too many authentication attempts");
    return keys;
  }
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

function userView(user: AuthUser): AuthUserView {
  return {
    id: user.id,
    email: user.email,
    emailVerified: user.emailVerified,
    engineAccountStatus: user.engineAccountStatus,
    customerType: user.customerType,
  };
}
