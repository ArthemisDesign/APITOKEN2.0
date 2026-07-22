import { ConfigService } from "@nestjs/config";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { B2C_SIGNUP_BONUS_BALANCE_NANO } from "@claude-api/contracts";
import {
  createCheckoutSession,
  createDatabase,
  decodeAuthEncryptionKey,
  decryptAuthToken,
  EmailAlreadyRegisteredError,
  getCheckoutSession,
  type Database,
} from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { AuthRateLimitedError, AuthService, InvalidCredentialsError } from "./auth.service.js";
import { OAuthProviderRegistry, type ExternalIdentityProvider } from "./auth-providers.js";

const connectionString = process.env.TEST_DATABASE_URL;
const encryptionKey = Buffer.alloc(32, 7).toString("base64url");

describe.runIf(Boolean(connectionString))("email authentication and authorization", () => {
  let database: Database;
  let auth: AuthService;
  let accountCounter = 0;

  beforeAll(() => {
    database = createDatabase(connectionString!);
    const engine = {
      createAccount: async () => ({ account: `acct_auth_${++accountCounter}`, multBp: 2000, handle: null }),
      creditAccount: async (account: string) => ({ account, balance_nano: "4000000000", balance: "$4.000000000" }),
    } as unknown as EngineClient;
    const config = new ConfigService<Environment, true>({
      SESSION_TTL_SECONDS: 604_800,
      AUTH_TOKEN_ENCRYPTION_KEY: encryptionKey,
      EMAIL_VERIFICATION_REQUIRED: true,
      EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
      PASSWORD_RESET_TTL_SECONDS: 3_600,
    } as Environment);
    auth = new AuthService(database, engine, config);
  });

  beforeEach(async () => {
    accountCounter = 0;
    await clean();
  });

  afterAll(async () => {
    await clean();
    await database.pool.end();
  });

  it("registers with Argon2id, queues an encrypted token, and authenticates only after verification", async () => {
    const registration = await auth.register({
      email: "alice@example.com",
      password: "correct horse battery staple",
      userAgent: "test-agent",
      ipAddress: "127.0.0.1",
    });
    expect(registration.user).toMatchObject({
      email: "alice@example.com", displayName: "alice", emailVerified: false, passwordEnabled: true, engineAccountStatus: "pending", totpEnabled: false,
    });
    expect(registration.session).toBeNull();

    const stored = await database.pool.query(`
      SELECT u.password_hash, at.token_hash, eo.template, eo.payload, ea.engine_account_id
      FROM users u JOIN auth_tokens at ON at.user_id = u.id
      JOIN email_outbox eo ON eo.user_id = u.id JOIN engine_accounts ea ON ea.user_id = u.id
    `);
    expect(stored.rows[0].password_hash).toMatch(/^\$argon2id\$/);
    expect(stored.rows[0].password_hash).not.toContain("correct horse");
    expect(stored.rows[0]).toMatchObject({ template: "verify_email", engine_account_id: null });
    const rawToken = decryptAuthToken(stored.rows[0].payload.encryptedToken, decodeAuthEncryptionKey(encryptionKey));
    expect(stored.rows[0].token_hash).not.toBe(rawToken);
    const session = await auth.verifyEmail({ token: rawToken, userAgent: "test-agent", ipAddress: "127.0.0.1" });
    expect(session.user).toMatchObject({ id: registration.user.id, emailVerified: true });
    await expect(auth.authenticate(session.token)).resolves.toMatchObject({ user: { id: registration.user.id } });
    await expect(auth.verifyEmail({ token: rawToken, userAgent: null, ipAddress: null })).rejects.toThrow("invalid or expired");
  });

  it("allows password registration while verification is disabled without granting the OAuth bonus", async () => {
    const createAccount = vi.fn(async () => ({ account: "acct_password", multBp: 4000, handle: null }));
    const creditAccount = vi.fn();
    const passwordAuth = new AuthService(
      database,
      { createAccount, creditAccount } as unknown as EngineClient,
      new ConfigService<Environment, true>({
        SESSION_TTL_SECONDS: 604_800,
        AUTH_TOKEN_ENCRYPTION_KEY: encryptionKey,
        EMAIL_VERIFICATION_REQUIRED: false,
        EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
        PASSWORD_RESET_TTL_SECONDS: 3_600,
      } as Environment),
    );

    const registration = await passwordAuth.register({
      email: "password-user@gmail.com",
      password: "correct horse battery staple",
      userAgent: "test-agent",
      ipAddress: "192.0.2.10",
    });

    expect(registration.session).not.toBeNull();
    expect(registration.user).toMatchObject({
      email: "password-user@gmail.com",
      emailVerified: false,
      passwordEnabled: true,
      engineAccountStatus: "active",
    });
    expect(createAccount).toHaveBeenCalledOnce();
    expect(creditAccount).not.toHaveBeenCalled();
    await expect(passwordAuth.authenticate(registration.session!.token)).resolves.toMatchObject({
      user: { id: registration.user.id },
    });
    await expect(passwordAuth.login({
      email: "password-user@gmail.com",
      password: "correct horse battery staple",
      userAgent: null,
      ipAddress: "192.0.2.10",
    })).resolves.toMatchObject({ user: { id: registration.user.id, emailVerified: false } });
    await passwordAuth.resendVerification("password-user@gmail.com", "192.0.2.10");

    const stored = await database.pool.query(`
      SELECT u.password_hash, u.email_verified, ea.status, ea.engine_account_id,
             (SELECT count(*)::int FROM auth_tokens WHERE user_id = u.id) AS tokens,
             (SELECT count(*)::int FROM email_outbox WHERE user_id = u.id) AS emails
      FROM users u JOIN engine_accounts ea ON ea.user_id = u.id
      WHERE u.id = $1
    `, [registration.user.id]);
    expect(stored.rows[0].password_hash).toMatch(/^\$argon2id\$/);
    expect(stored.rows[0]).toMatchObject({
      email_verified: false,
      status: "active",
      engine_account_id: "acct_password",
      tokens: 0,
      emails: 0,
    });
  });

  it("uses generic login failure and revokes only the current user's session", async () => {
    const registered = await registerAndVerify({
      email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null,
    });
    await expect(auth.login({
      email: "alice@example.com", password: "definitely incorrect password", userAgent: null, ipAddress: null,
    })).rejects.toBeInstanceOf(InvalidCredentialsError);
    await expect(auth.login({
      email: "missing@example.com", password: "definitely incorrect password", userAgent: null, ipAddress: null,
    })).rejects.toBeInstanceOf(InvalidCredentialsError);

    const loggedIn = await auth.login({
      email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null,
    });
    await auth.logout(loggedIn.sessionId, loggedIn.user.id);
    await expect(auth.authenticate(loggedIn.token)).resolves.toBeNull();
    await expect(auth.authenticate(registered.token)).resolves.not.toBeNull();
  });

  it("rejects duplicate email regardless of case", async () => {
    await auth.register({ email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null });
    await expect(auth.register({
      email: "ALICE@example.com", password: "another correct battery staple", userAgent: null, ipAddress: null,
    })).rejects.toBeInstanceOf(EmailAlreadyRegisteredError);
  });

  it("does not authenticate password registration before verification in every environment", async () => {
    const engine = {
      createAccount: async () => ({ account: "acct_verified_gate", multBp: 2000, handle: null }),
      creditAccount: async (account: string) => ({ account, balance_nano: "4000000000", balance: "$4.000000000" }),
    } as unknown as EngineClient;
    const strictAuth = new AuthService(database, engine, new ConfigService<Environment, true>({
      SESSION_TTL_SECONDS: 604_800,
      AUTH_TOKEN_ENCRYPTION_KEY: encryptionKey,
      EMAIL_VERIFICATION_REQUIRED: true,
      EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
      PASSWORD_RESET_TTL_SECONDS: 3_600,
    } as Environment));
    const result = await strictAuth.register({
      email: "verify@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: "192.0.2.2",
    });
    expect(result.session).toBeNull();
    const sessions = await database.pool.query("SELECT count(*)::int AS count FROM auth_sessions");
    expect(sessions.rows[0]).toEqual({ count: 0 });
  });

  it("rate-limits repeated login guesses in PostgreSQL", async () => {
    await auth.register({ email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: "192.0.2.1" });
    for (let attempt = 0; attempt < 10; attempt += 1) {
      await expect(auth.login({
        email: "alice@example.com", password: "definitely incorrect password", userAgent: null, ipAddress: "192.0.2.1",
      })).rejects.toBeInstanceOf(InvalidCredentialsError);
    }
    await expect(auth.login({
      email: "alice@example.com", password: "definitely incorrect password", userAgent: null, ipAddress: "192.0.2.1",
    })).rejects.toBeInstanceOf(AuthRateLimitedError);
  });

  it("resets a password through a hashed one-time token and revokes existing sessions", async () => {
    const originalSession = await registerAndVerify({
      email: "reset@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null,
    });
    await auth.requestPasswordReset("reset@example.com", "192.0.2.5");
    const outbox = await database.pool.query<{ payload: { encryptedToken: string } }>(`
      SELECT payload FROM email_outbox WHERE template = 'reset_password' ORDER BY created_at DESC LIMIT 1
    `);
    const token = decryptAuthToken(outbox.rows[0]!.payload.encryptedToken, decodeAuthEncryptionKey(encryptionKey));
    await auth.resetPassword(token, "new correct horse battery");
    await expect(auth.authenticate(originalSession.token)).resolves.toBeNull();
    await expect(auth.resetPassword(token, "another correct horse battery")).rejects.toThrow("invalid or expired");
    await expect(auth.login({
      email: "reset@example.com", password: "new correct horse battery", userAgent: null, ipAddress: null,
    })).resolves.toMatchObject({ user: { emailVerified: true } });
  });

  it("creates verified Google/GitHub-style accounts without sending verification email", async () => {
    const externalProvider: ExternalIdentityProvider = {
      code: "github",
      createAuthorizationUrl: ({ state }) => new URL(`https://github.test/authorize?state=${state}`),
      exchangeCallback: async () => ({
        provider: "github",
        subject: "github-user-42",
        email: "developer@gmail.com",
        emailVerified: true,
        displayName: "Developer",
        metadata: { login: "developer" },
      }),
    };
    const creditAccount = vi.fn(async (account: string) => ({
      account, balance_nano: "4000000000", balance: "$4.000000000",
    }));
    const oauthAuth = new AuthService(
      database,
      {
        createAccount: async () => ({ account: `acct_oauth_${++accountCounter}`, multBp: 4000, handle: null }),
        creditAccount,
      } as unknown as EngineClient,
      new ConfigService<Environment, true>({
        SESSION_TTL_SECONDS: 604_800,
        AUTH_TOKEN_ENCRYPTION_KEY: encryptionKey,
        EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
        PASSWORD_RESET_TTL_SECONDS: 3_600,
      } as Environment),
      new OAuthProviderRegistry([externalProvider]),
    );
    const started = await oauthAuth.beginOAuth("github");
    const state = new URL(started.authorizationUrl).searchParams.get("state")!;
    const session = await oauthAuth.completeOAuth({
      provider: "github", code: "temporary-code", state, stateCookie: state, userAgent: null, ipAddress: null,
    });
    expect(session.user).toMatchObject({
      email: "developer@gmail.com", displayName: "Developer", emailVerified: true, passwordEnabled: false, engineAccountStatus: "active", totpEnabled: false,
    });
    expect(creditAccount).toHaveBeenCalledOnce();
    expect(creditAccount).toHaveBeenCalledWith(
      "acct_oauth_1", 4_000_000_000n, `signup-bonus:${session.user.id}`,
    );
    const rows = await database.pool.query(`
      SELECT u.password_hash, u.email_verified, ai.provider,
             (SELECT count(*)::int FROM email_outbox) AS emails
      FROM users u JOIN auth_identities ai ON ai.user_id = u.id
    `);
    expect(rows.rows[0]).toMatchObject({ password_hash: null, email_verified: true, provider: "github", emails: 0 });
  });

  it.each(["google", "github"] as const)(
    "lets a verified %s identity claim a same-email password account and grants the welcome bonus",
    async (providerCode) => {
      const externalProvider: ExternalIdentityProvider = {
        code: providerCode,
        createAuthorizationUrl: ({ state }) => new URL(`https://${providerCode}.test/authorize?state=${state}`),
        exchangeCallback: async () => ({
          provider: providerCode,
          subject: `${providerCode}-claim-42`,
          email: "claimed@gmail.com",
          emailVerified: true,
          displayName: "Claimed User",
          metadata: { login: "claimed" },
        }),
      };
      const creditAccount = vi.fn(async (account: string) => ({
        account, balance_nano: "4000000000", balance: "$4.000000000",
      }));
      const oauthAuth = new AuthService(
        database,
        {
          createAccount: async () => ({ account: `acct_claim_${providerCode}`, multBp: 4000, handle: null }),
          creditAccount,
        } as unknown as EngineClient,
        new ConfigService<Environment, true>({
          SESSION_TTL_SECONDS: 604_800,
          AUTH_TOKEN_ENCRYPTION_KEY: encryptionKey,
          EMAIL_VERIFICATION_REQUIRED: false,
          EMAIL_VERIFICATION_TTL_SECONDS: 86_400,
          PASSWORD_RESET_TTL_SECONDS: 3_600,
        } as Environment),
        new OAuthProviderRegistry([externalProvider]),
      );
      const registration = await oauthAuth.register({
        email: "claimed@gmail.com",
        password: "correct horse battery staple",
        userAgent: "password-browser",
        ipAddress: "192.0.2.20",
      });
      expect(registration.session).not.toBeNull();
      expect(creditAccount).not.toHaveBeenCalled();
      await oauthAuth.requestPasswordReset("claimed@gmail.com", "192.0.2.20");

      const started = await oauthAuth.beginOAuth(providerCode);
      const state = new URL(started.authorizationUrl).searchParams.get("state")!;
      const session = await oauthAuth.completeOAuth({
        provider: providerCode,
        code: "temporary-code",
        state,
        stateCookie: state,
        userAgent: "oauth-browser",
        ipAddress: "192.0.2.21",
      });

      expect(session.user).toMatchObject({
        id: registration.user.id,
        email: "claimed@gmail.com",
        emailVerified: true,
        passwordEnabled: false,
        engineAccountStatus: "active",
      });
      expect(creditAccount).toHaveBeenCalledOnce();
      expect(creditAccount).toHaveBeenCalledWith(
        `acct_claim_${providerCode}`,
        B2C_SIGNUP_BONUS_BALANCE_NANO,
        `signup-bonus:${registration.user.id}`,
      );
      await expect(oauthAuth.authenticate(registration.session!.token)).resolves.toBeNull();
      await expect(oauthAuth.authenticate(session.token)).resolves.toMatchObject({
        user: { id: registration.user.id },
      });
      await expect(oauthAuth.login({
        email: "claimed@gmail.com",
        password: "correct horse battery staple",
        userAgent: null,
        ipAddress: "192.0.2.22",
      })).rejects.toBeInstanceOf(InvalidCredentialsError);

      const stored = await database.pool.query(`
        SELECT (u.password_hash IS NULL) AS password_removed, u.email_verified, ai.provider,
               (SELECT count(*)::int FROM auth_sessions s WHERE s.user_id = u.id AND s.revoked_at IS NULL) AS active_sessions,
               (SELECT count(*)::int FROM auth_sessions s WHERE s.user_id = u.id AND s.revoked_at IS NOT NULL) AS revoked_sessions,
               (SELECT count(*)::int FROM auth_tokens t WHERE t.user_id = u.id AND t.used_at IS NOT NULL) AS invalidated_tokens,
               (SELECT count(*)::int FROM audit_log al WHERE al.target_id = u.id::text AND al.action = 'auth.oauth_claimed') AS claim_events
        FROM users u JOIN auth_identities ai ON ai.user_id = u.id
        WHERE u.id = $1
      `, [registration.user.id]);
      expect(stored.rows[0]).toMatchObject({
        password_removed: true,
        email_verified: true,
        provider: providerCode,
        active_sessions: 1,
        revoked_sessions: 1,
        invalidated_tokens: 1,
        claim_events: 1,
      });
    },
  );

  it("updates only the authenticated user's display name", async () => {
    const alice = await registerAndVerify({
      email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null,
    });
    const bob = await registerAndVerify({
      email: "bob@example.com", password: "another correct battery staple", userAgent: null, ipAddress: null,
    });

    await expect(auth.updateProfile(alice.user.id, "Alice Studio")).resolves.toMatchObject({
      id: alice.user.id, displayName: "Alice Studio",
    });
    await expect(auth.getUser(bob.user.id)).resolves.toMatchObject({
      id: bob.user.id, displayName: "bob",
    });
    const audit = await database.pool.query<{ actor_id: string; target_id: string; action: string }>(`
      SELECT actor_id, target_id, action FROM audit_log WHERE action = 'profile.display_name_updated'
    `);
    expect(audit.rows).toEqual([{
      actor_id: alice.user.id, target_id: alice.user.id, action: "profile.display_name_updated",
    }]);
  });

  it("never returns another user's checkout", async () => {
    const alice = await registerAndVerify({ email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null });
    const bob = await registerAndVerify({ email: "bob@example.com", password: "another correct battery staple", userAgent: null, ipAddress: null });
    const checkout = await createCheckoutSession(database, { userId: alice.user.id, provider: "cryptomus", amountUsd: 10n });
    await expect(getCheckoutSession(database, { id: checkout.id, userId: bob.user.id })).resolves.toBeNull();
    await expect(getCheckoutSession(database, { id: checkout.id, userId: alice.user.id })).resolves.toMatchObject({ id: checkout.id });
  });

  async function clean(): Promise<void> {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, oauth_transactions, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities, checkout_sessions, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  }

  async function registerAndVerify(input: Parameters<AuthService["register"]>[0]): Promise<Awaited<ReturnType<AuthService["verifyEmail"]>>> {
    const registration = await auth.register(input);
    const outbox = await database.pool.query<{ payload: { encryptedToken: string } }>(`
      SELECT payload FROM email_outbox WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1
    `, [registration.user.id]);
    const token = decryptAuthToken(outbox.rows[0]!.payload.encryptedToken, decodeAuthEncryptionKey(encryptionKey));
    return auth.verifyEmail({ token, userAgent: input.userAgent, ipAddress: input.ipAddress });
  }
});
