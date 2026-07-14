import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { hash, verify, argon2id } from "argon2";
import type { AuthUserView } from "@claude-api/contracts";
import {
  completeEngineAccount,
  completeExternalSignIn,
  consumeEmailVerification,
  consumeOAuthTransaction,
  consumePasswordReset,
  consumeAuthRateLimit,
  createOAuthTransaction,
  createAuthSession,
  createEmailUser,
  clearAuthRateLimit,
  failEngineAccount,
  findPasswordUser,
  getAuthUser,
  decodeAuthEncryptionKey,
  encryptAuthToken,
  queueAuthEmailForAddress,
  resolveAuthSession,
  revokeAuthSession,
  type AuthUser,
  type Database,
  type OAuthProvider,
} from "@claude-api/db";
import { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { DATABASE, ENGINE_CLIENT } from "./infrastructure.module.js";
import { OAuthProviderRegistry } from "./auth-providers.js";
import { createFundedEngineAccount } from "./engine-provisioning.js";

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
export class InvalidAuthTokenError extends Error {}
export class InvalidOAuthTransactionError extends Error {}

@Injectable()
export class AuthService {
  constructor(
    @Inject(DATABASE) private readonly database: Database,
    @Inject(ENGINE_CLIENT) private readonly engine: EngineClient,
    private readonly config: ConfigService<Environment, true>,
    private readonly oauthProviders: OAuthProviderRegistry = new OAuthProviderRegistry([]),
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
    const verification = this.createAuthEmailSecret("verify_email");
    const user = await createEmailUser(
      this.database,
      input.email,
      passwordHash,
      input.inviteToken ? tokenHash(input.inviteToken) : undefined,
      verification,
    );
    return { user: userView(user), session: null };
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
    if (!user.emailVerified) {
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

  async verifyEmail(input: {
    token: string; userAgent: string | null; ipAddress: string | null;
  }): Promise<AuthSession> {
    const userId = await consumeEmailVerification(this.database, tokenHash(input.token));
    if (!userId) throw new InvalidAuthTokenError("email verification link is invalid or expired");
    const user = await getAuthUser(this.database, userId);
    if (!user) throw new InvalidAuthTokenError("email verification user is unavailable");
    await this.provisionEngineAccount(user, await this.multiplierForUser(user.id));
    return this.issueSession(user, input.userAgent, input.ipAddress);
  }

  async resendVerification(email: string, ipAddress: string | null): Promise<void> {
    await this.enforceRateLimits("verify-resend", email, ipAddress, 3, 10, 3600);
    await queueAuthEmailForAddress(this.database, {
      email,
      purpose: "verify_email",
      ...this.createAuthEmailSecret("verify_email"),
    });
  }

  async requestPasswordReset(email: string, ipAddress: string | null): Promise<void> {
    await this.enforceRateLimits("password-reset", email, ipAddress, 3, 10, 3600);
    await queueAuthEmailForAddress(this.database, {
      email,
      purpose: "reset_password",
      ...this.createAuthEmailSecret("reset_password"),
    });
  }

  async resetPassword(token: string, password: string): Promise<void> {
    const passwordHash = await hash(password, passwordHashOptions());
    if (!await consumePasswordReset(this.database, tokenHash(token), passwordHash)) {
      throw new InvalidAuthTokenError("password reset link is invalid or expired");
    }
  }

  async beginOAuth(provider: OAuthProvider, inviteToken?: string): Promise<{
    authorizationUrl: string; state: string;
  }> {
    const adapter = this.oauthProviders.get(provider);
    const state = randomBytes(32).toString("base64url");
    const nonce = provider === "google" ? randomBytes(32).toString("base64url") : null;
    const codeVerifier = randomBytes(32).toString("base64url");
    const codeChallenge = createHash("sha256").update(codeVerifier).digest("base64url");
    await createOAuthTransaction(this.database, {
      stateHash: tokenHash(state),
      provider,
      nonce,
      codeVerifier,
      inviteTokenHash: inviteToken ? tokenHash(inviteToken) : null,
      expiresAt: new Date(Date.now() + 10 * 60 * 1000),
    });
    return {
      authorizationUrl: adapter.createAuthorizationUrl({ state, nonce, codeChallenge }).toString(),
      state,
    };
  }

  async completeOAuth(input: {
    provider: OAuthProvider;
    code: string;
    state: string;
    stateCookie: string | null;
    userAgent: string | null;
    ipAddress: string | null;
  }): Promise<AuthSession> {
    if (!input.stateCookie || !safeTokenEqual(input.state, input.stateCookie)) {
      throw new InvalidOAuthTransactionError("OAuth browser state is invalid");
    }
    const transaction = await consumeOAuthTransaction(this.database, tokenHash(input.state), input.provider);
    if (!transaction) throw new InvalidOAuthTransactionError("OAuth transaction is invalid or expired");
    const identity = await this.oauthProviders.get(input.provider).exchangeCallback({
      code: input.code,
      expectedNonce: transaction.nonce,
      codeVerifier: transaction.codeVerifier,
    });
    const user = await completeExternalSignIn(this.database, identity, transaction.inviteTokenHash);
    if (user.status !== "active") throw new InvalidOAuthTransactionError("account is disabled");
    await this.provisionEngineAccount(user, user.engineMultiplierBp);
    return this.issueSession(user, input.userAgent, input.ipAddress);
  }

  providerStatus(): { google: boolean; github: boolean } {
    return { google: this.oauthProviders.has("google"), github: this.oauthProviders.has("github") };
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

  private createAuthEmailSecret(purpose: "verify_email" | "reset_password"): {
    tokenHash: string; encryptedToken: string; expiresAt: Date;
  } {
    const token = randomBytes(32).toString("base64url");
    const ttl = this.config.get(
      purpose === "verify_email" ? "EMAIL_VERIFICATION_TTL_SECONDS" : "PASSWORD_RESET_TTL_SECONDS",
      { infer: true },
    );
    const key = decodeAuthEncryptionKey(this.config.get("AUTH_TOKEN_ENCRYPTION_KEY", { infer: true }));
    return {
      tokenHash: tokenHash(token),
      encryptedToken: encryptAuthToken(token, key),
      expiresAt: new Date(Date.now() + ttl * 1000),
    };
  }

  private async multiplierForUser(userId: string): Promise<number> {
    const result = await this.database.pool.query<{ mult_bp: number }>(
      "SELECT mult_bp FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    return result.rows[0]?.mult_bp ?? 4000;
  }

  private async provisionEngineAccount(user: AuthUser, multiplierBp: number): Promise<void> {
    if (user.engineAccountStatus === "active") return;
    try {
      const account = await createFundedEngineAccount(this.engine, {
        userId: user.id,
        customerType: user.customerType,
        handle: `user:${user.id}`,
        multBp: multiplierBp,
      });
      await completeEngineAccount(this.database, user.id, account.account);
      user.engineAccountStatus = "active";
    } catch (error) {
      await failEngineAccount(this.database, user.id, error instanceof Error ? error.message : "engine account creation failed");
      user.engineAccountStatus = "error";
    }
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

function safeTokenEqual(left: string, right: string): boolean {
  const a = createHash("sha256").update(left).digest();
  const b = createHash("sha256").update(right).digest();
  return timingSafeEqual(a, b);
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
