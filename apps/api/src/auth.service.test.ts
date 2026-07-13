import { ConfigService } from "@nestjs/config";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { createCheckoutSession, createDatabase, EmailAlreadyRegisteredError, getCheckoutSession, type Database } from "@claude-api/db";
import type { EngineClient } from "@claude-api/engine-client";
import type { Environment } from "./config.js";
import { AuthRateLimitedError, AuthService, InvalidCredentialsError } from "./auth.service.js";

const connectionString = process.env.TEST_DATABASE_URL;

describe.runIf(Boolean(connectionString))("email authentication and authorization", () => {
  let database: Database;
  let auth: AuthService;
  let accountCounter = 0;

  beforeAll(() => {
    database = createDatabase(connectionString!);
    const engine = {
      createAccount: async () => ({ account: `acct_auth_${++accountCounter}`, multBp: 2000, handle: null }),
    } as unknown as EngineClient;
    const config = new ConfigService<Environment, true>({
      SESSION_TTL_SECONDS: 604_800,
      REQUIRE_VERIFIED_EMAIL: false,
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

  it("registers with Argon2id, queues verification, and stores only a session hash", async () => {
    const session = await auth.register({
      email: "alice@example.com",
      password: "correct horse battery staple",
      userAgent: "test-agent",
      ipAddress: "127.0.0.1",
    });
    expect(session.user).toMatchObject({ email: "alice@example.com", engineAccountStatus: "active" });
    expect(session.session?.token).toMatch(/^[A-Za-z0-9_-]{43}$/);

    const stored = await database.pool.query(`
      SELECT u.password_hash, s.token_hash, eo.template, ea.engine_account_id
      FROM users u JOIN auth_sessions s ON s.user_id = u.id
      JOIN email_outbox eo ON eo.user_id = u.id JOIN engine_accounts ea ON ea.user_id = u.id
    `);
    expect(stored.rows[0].password_hash).toMatch(/^\$argon2id\$/);
    expect(stored.rows[0].password_hash).not.toContain("correct horse");
    expect(stored.rows[0].token_hash).not.toBe(session.session!.token);
    expect(stored.rows[0]).toMatchObject({ template: "verify_email", engine_account_id: "acct_auth_1" });
    await expect(auth.authenticate(session.session!.token)).resolves.toMatchObject({ user: { id: session.user.id } });
  });

  it("uses generic login failure and revokes only the current user's session", async () => {
    const registered = await auth.register({
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
    await expect(auth.authenticate(registered.session!.token)).resolves.not.toBeNull();
  });

  it("rejects duplicate email regardless of case", async () => {
    await auth.register({ email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null });
    await expect(auth.register({
      email: "ALICE@example.com", password: "another correct battery staple", userAgent: null, ipAddress: null,
    })).rejects.toBeInstanceOf(EmailAlreadyRegisteredError);
  });

  it("does not authenticate registration before verification when required", async () => {
    const engine = { createAccount: async () => ({ account: "acct_verified_gate", multBp: 2000, handle: null }) } as unknown as EngineClient;
    const strictAuth = new AuthService(database, engine, new ConfigService<Environment, true>({
      SESSION_TTL_SECONDS: 604_800,
      REQUIRE_VERIFIED_EMAIL: true,
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

  it("never returns another user's checkout", async () => {
    const alice = await auth.register({ email: "alice@example.com", password: "correct horse battery staple", userAgent: null, ipAddress: null });
    const bob = await auth.register({ email: "bob@example.com", password: "another correct battery staple", userAgent: null, ipAddress: null });
    const checkout = await createCheckoutSession(database, { userId: alice.user.id, provider: "cryptomus", amountUsd: 10n });
    await expect(getCheckoutSession(database, { id: checkout.id, userId: bob.user.id })).resolves.toBeNull();
    await expect(getCheckoutSession(database, { id: checkout.id, userId: alice.user.id })).resolves.toMatchObject({ id: checkout.id });
  });

  async function clean(): Promise<void> {
    await database.pool.query(`
      TRUNCATE audit_log, api_keys, engine_credits, webhook_events, payments, email_outbox, auth_rate_limits,
               auth_tokens, auth_sessions, auth_identities, checkout_sessions, engine_accounts, users
      RESTART IDENTITY CASCADE
    `);
  }
});
