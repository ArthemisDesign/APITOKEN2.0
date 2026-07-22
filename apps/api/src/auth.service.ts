import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { ForbiddenException, Inject, Injectable } from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { hash, verify, argon2id } from "argon2";
import { B2C_SIGNUP_BONUS_BALANCE_NANO, type AuthUserView } from "@claude-api/contracts";
import {
  completeExternalSignIn,
  consumeEmailVerification,
  consumeOAuthTransaction,
  consumePasswordReset,
  consumeAuthRateLimit,
  createOAuthTransaction,
  createAuthSession,
  createEmailUser,
  clearAuthRateLimit,
  findPasswordUser,
  getAuthUser,
  decodeAuthEncryptionKey,
  encryptAuthToken,
  queueAuthEmailForAddress,
  recordReferralAttribution,
  getReferralAttributionCode,
  setReferralFloor,
  resolveAuthSession,
  revokeAuthSession,
  updateUserDisplayName,
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
export class EngineAccountDisabledError extends ForbiddenException {
  constructor() {
    super("engine account is disabled");
  }
}

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
    referralCode?: string | undefined;
    userAgent: string | null;
    ipAddress: string | null;
  }): Promise<RegistrationResult> {
    await this.enforceRateLimits("register", input.email, input.ipAddress, 5, 20, 3600);
    const passwordHash = await hash(input.password, passwordHashOptions());
    const verificationRequired = this.emailVerificationRequired();
    const verification = verificationRequired ? this.createAuthEmailSecret("verify_email") : undefined;
    const user = await createEmailUser(
      this.database,
      input.email,
      passwordHash,
      input.inviteToken ? tokenHash(input.inviteToken) : undefined,
      verification,
    );
    await this.attributeReferral(user.id, input.referralCode);
    if (verificationRequired) return { user: userView(user), session: null };
    await this.provisionEngineAccount(user, user.engineMultiplierBp, false);
    const session = await this.issueSession(user, input.userAgent, input.ipAddress);
    return { user: session.user, session };
  }

  /** Атрибуция — best-effort: сбой записи реф-кода не должен ломать регистрацию. */
  private async attributeReferral(userId: string, referralCode: string | undefined): Promise<void> {
    if (!referralCode) return;
    try {
      await recordReferralAttribution(this.database, userId, referralCode.toLowerCase());
    } catch {
      // не логируем код — этого достаточно для диагностики по user_id через БД
    }
  }

  /**
   * Сразу при первой активации движок-аккаунта применяем скидку-«пол» персональной реф-ссылки,
   * чтобы реферал увидел свою ставку с ПЕРВОГО захода (иначе floor доезжает лишь async sales-фидом,
   * и до тех пор дашборд показывает обычный b2c). Read-only резолв в sales; гашение ссылки остаётся
   * за фидом. Полностью best-effort и идемпотентно: если sales недоступен — фид применит floor позже.
   */
  private async applyReferralFloorFromAttribution(userId: string): Promise<void> {
    const base = this.config.get("SALES_API_URL", { infer: true });
    const key = this.config.get("SALES_CONTROL_KEY", { infer: true });
    if (!base || !key) return;
    try {
      const code = await getReferralAttributionCode(this.database, userId);
      if (!code) return;
      const url = new URL(`/v1/internal/partners/referral-discount?code=${encodeURIComponent(code)}`, base);
      const response = await fetch(url, { headers: { "x-api-key": key }, signal: AbortSignal.timeout(4_000) });
      if (!response.ok) return;
      const body = (await response.json()) as { discountBps?: unknown };
      const discountBps = typeof body.discountBps === "number" ? body.discountBps : 0;
      if (discountBps > 0) {
        await setReferralFloor(this.database, { userId, floorBps: discountBps, actorId: "referral-signup" });
      }
    } catch {
      // best-effort: async sales-фид применит floor при следующем тике
    }
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
    if (this.emailVerificationRequired() && !user.emailVerified) {
      throw new EmailVerificationRequiredError("email verification is required");
    }
    await this.provisionEngineAccount(user, await this.multiplierForUser(user.id), false);
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
    await this.provisionEngineAccount(user, await this.multiplierForUser(user.id), false);
    return this.issueSession(user, input.userAgent, input.ipAddress);
  }

  async resendVerification(email: string, ipAddress: string | null): Promise<void> {
    if (!this.emailVerificationRequired()) return;
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

  async beginOAuth(provider: OAuthProvider, inviteToken?: string, referralCode?: string): Promise<{
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
      // Реф-код (?ref=) переживает редирект в провайдера, чтобы на complete закрепить атрибуцию
      // и (если это код партнёра) сделать аккаунт B2B ДО выдачи welcome-бонуса.
      referralCode: normalizeReferralCode(referralCode),
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
    await this.provisionEngineAccount(user, user.engineMultiplierBp, false);
    // Реф партнёра остаётся обычным b2c и получает welcome-бонус, как все. Реф-код лишь закрепляет
    // атрибуцию (sales-фид подхватит для комиссии + применит скидку-«пол» партнёра асинхронно).
    if (transaction.referralCode) await this.attributeReferral(user.id, transaction.referralCode);
    await this.grantOAuthWelcomeBonus(user);
    return this.issueSession(user, input.userAgent, input.ipAddress);
  }

  providerStatus(): { google: boolean; github: boolean } {
    return { google: this.oauthProviders.has("google"), github: this.oauthProviders.has("github") };
  }

  async getUser(userId: string): Promise<AuthUserView | null> {
    const user = await getAuthUser(this.database, userId);
    return user ? userView(user) : null;
  }

  async updateProfile(userId: string, displayName: string): Promise<AuthUserView | null> {
    const user = await updateUserDisplayName(this.database, userId, displayName);
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

  private async grantOAuthWelcomeBonus(user: AuthUser): Promise<void> {
    if (user.customerType !== "b2c" || user.engineAccountStatus !== "active") return;
    const result = await this.database.pool.query<{ engine_account_id: string | null }>(`
      SELECT engine_account_id
      FROM engine_accounts
      WHERE user_id = $1 AND status = 'active'
    `, [user.id]);
    const engineAccountId = result.rows[0]?.engine_account_id;
    if (!engineAccountId) return;
    await this.engine.creditAccount(
      engineAccountId,
      B2C_SIGNUP_BONUS_BALANCE_NANO,
      `signup-bonus:${user.id}`,
    );
  }

  private emailVerificationRequired(): boolean {
    return this.config.get("EMAIL_VERIFICATION_REQUIRED", { infer: true }) === true;
  }

  private async provisionEngineAccount(
    user: AuthUser,
    multiplierBp: number,
    welcomeBonusEligible: boolean,
  ): Promise<void> {
    if (user.engineAccountStatus === "active") return;
    if (user.engineAccountStatus === "disabled") throw new EngineAccountDisabledError();

    const currentStatus = await this.engineAccountStatusForUser(user.id);
    if (currentStatus === "active") {
      user.engineAccountStatus = "active";
      return;
    }
    if (currentStatus === "disabled") {
      user.engineAccountStatus = "disabled";
      throw new EngineAccountDisabledError();
    }
    if (currentStatus === null) throw new Error("engine account mapping is missing");

    try {
      const account = await createFundedEngineAccount(this.engine, {
        userId: user.id,
        customerType: user.customerType,
        handle: `user:${user.id}`,
        multBp: multiplierBp,
        welcomeBonusEligible,
      });
      const completed = await this.database.pool.query(`
        UPDATE engine_accounts
        SET engine_account_id = $2, status = 'active', last_error = NULL, updated_at = now()
        WHERE user_id = $1 AND status IN ('pending', 'error')
        RETURNING status
      `, [user.id, account.account]);
      if (completed.rowCount === 1) {
        user.engineAccountStatus = "active";
        // Реферал по скидочной ссылке должен видеть свою ставку СРАЗУ — применяем «пол» синхронно,
        // не дожидаясь асинхронного sales-фида. Best-effort и идемпотентно с фидом.
        await this.applyReferralFloorFromAttribution(user.id);
        return;
      }

      const latestStatus = await this.engineAccountStatusForUser(user.id);
      if (latestStatus === "active") {
        user.engineAccountStatus = "active";
        return;
      }
      if (latestStatus === "disabled") {
        user.engineAccountStatus = "disabled";
        // AUDIT-TODO(C61): add a durable provisioning lease/version plus remote disable compensation for a claim lost after creation.
        throw new EngineAccountDisabledError();
      }
      throw new Error("engine account mapping changed during provisioning");
    } catch (error) {
      if (error instanceof EngineAccountDisabledError) throw error;
      const failed = await this.database.pool.query(`
        UPDATE engine_accounts
        SET status = 'error', last_error = $2, updated_at = now()
        WHERE user_id = $1 AND status IN ('pending', 'error')
        RETURNING status
      `, [user.id, error instanceof Error ? error.message.slice(0, 1000) : "engine account creation failed"]);
      if (failed.rowCount === 1) {
        user.engineAccountStatus = "error";
        return;
      }

      const latestStatus = await this.engineAccountStatusForUser(user.id);
      if (latestStatus === "disabled") {
        user.engineAccountStatus = "disabled";
        throw new EngineAccountDisabledError();
      }
      if (latestStatus === "active") {
        user.engineAccountStatus = "active";
        return;
      }
      user.engineAccountStatus = "error";
    }
  }

  private async engineAccountStatusForUser(userId: string): Promise<AuthUser["engineAccountStatus"] | null> {
    const result = await this.database.pool.query<{ status: AuthUser["engineAccountStatus"] }>(
      "SELECT status FROM engine_accounts WHERE user_id = $1",
      [userId],
    );
    return result.rows[0]?.status ?? null;
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

// Реф-код: те же символы, что и на клиенте (lib/referral). Невалидный → null (атрибуции нет).
function normalizeReferralCode(code: string | undefined): string | null {
  if (!code) return null;
  const trimmed = code.trim().toLowerCase();
  return /^[a-z0-9_-]{3,32}$/.test(trimmed) ? trimmed : null;
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
    displayName: user.displayName,
    emailVerified: user.emailVerified,
    passwordEnabled: user.passwordEnabled,
    engineAccountStatus: user.engineAccountStatus,
    customerType: user.customerType,
    totpEnabled: user.totpEnabled,
  };
}
