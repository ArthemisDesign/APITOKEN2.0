import { ConfigService } from "@nestjs/config";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
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
      email: "alice@example.com", emailVerified: false, passwordEnabled: true, engineAccountStatus: "pending",
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
        email: "developer@example.com",
        emailVerified: true,
        displayName: "Developer",
        metadata: { login: "developer" },
      }),
    };
    const oauthAuth = new AuthService(
      database,
      {
        createAccount: async () => ({ account: `acct_oauth_${++accountCounter}`, multBp: 4000, handle: null }),
        creditAccount: async (account: string) => ({ account, balance_nano: "4000000000", balance: "$4.000000000" }),
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
      email: "developer@example.com", emailVerified: true, passwordEnabled: false, engineAccountStatus: "active",
    });
    const rows = await database.pool.query(`
      SELECT u.password_hash, u.email_verified, ai.provider,
             (SELECT count(*)::int FROM email_outbox) AS emails
      FROM users u JOIN auth_identities ai ON ai.user_id = u.id
    `);
    expect(rows.rows[0]).toMatchObject({ password_hash: null, email_verified: true, provider: "github", emails: 0 });
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
