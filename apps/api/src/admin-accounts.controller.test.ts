import {
  BadRequestException,
  ConflictException,
  ForbiddenException,
  UnauthorizedException,
} from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { ManagedAdminConflictError } from "@claude-api/db";
import { AdminAccountsController } from "./admin-accounts.controller.js";
import { type AdminAccountsService } from "./admin-accounts.service.js";
import { type AdminSessionService } from "./admin-session.service.js";
import { InternalAdminAuthController } from "./internal-admin-auth.controller.js";

const accountId = "11111111-1111-4111-8111-111111111111";

describe("managed admin HTTP contract", () => {
  it("accepts an eight-character password and rejects seven characters", async () => {
    const fake = fakeAccounts();
    const controller = new AdminAccountsController(fake.service);
    fake.changePassword.mockResolvedValue({ changed_self: false });

    await expect(controller.changePassword(accountId, {
      password: "12345678",
      reason: "scheduled credential rotation",
    })).resolves.toEqual({ changed_self: false });
    await expect(controller.changePassword(accountId, {
      password: "1234567",
      reason: "scheduled credential rotation",
    })).rejects.toBeInstanceOf(BadRequestException);
    expect(fake.changePassword).toHaveBeenCalledTimes(1);
  });

  it("validates usernames, strong passwords, domains, reasons, and IDs", async () => {
    const fake = fakeAccounts();
    const controller = new AdminAccountsController(fake.service);
    await expect(controller.create({
      username: "bad username",
      password: "short",
      domains: ["unknown.example"],
      reason: "x",
    })).rejects.toBeInstanceOf(BadRequestException);
    await expect(controller.changePassword("not-a-uuid", {})).rejects.toBeInstanceOf(BadRequestException);
    expect(fake.create).not.toHaveBeenCalled();
  });

  it("forwards current-account identity for self password rotation", async () => {
    const fake = fakeAccounts();
    const controller = new AdminAccountsController(fake.service);
    fake.changePassword.mockResolvedValue({ changed_self: true });
    await expect(controller.changePassword(accountId, {
      password: "correct horse battery staple",
      reason: "scheduled credential rotation",
    }, accountId, "main-admin")).resolves.toEqual({ changed_self: true });
    expect(fake.changePassword).toHaveBeenCalledWith({
      accountId,
      password: "correct horse battery staple",
      reason: "scheduled credential rotation",
      actorId: accountId,
    });
  });

  it("maps duplicate usernames to conflict", async () => {
    const fake = fakeAccounts();
    const controller = new AdminAccountsController(fake.service);
    fake.create.mockRejectedValue(new ManagedAdminConflictError("admin username already exists"));
    await expect(controller.create({
      username: "main-admin",
      password: "correct horse battery staple",
      domains: ["admin.apitoken.sale"],
      reason: "add another operator",
    })).rejects.toBeInstanceOf(ConflictException);
  });
});

describe("internal managed-admin verifier", () => {
  it("rejects an unknown managed domain before checking credentials", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    await expect(controller.verify(
      { headers: { authorization: "Basic abc", "x-admin-domain": "backend.apitoken.sale" } },
      reply,
    )).rejects.toBeInstanceOf(UnauthorizedException);
    expect(fake.authenticate).not.toHaveBeenCalled();
  });

  it("returns actor headers only after domain-scoped authentication", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    fake.authenticate.mockResolvedValue({ id: accountId, username: "main-admin", sessionVersion: "v".repeat(43) });
    await expect(controller.verify(
      { headers: { authorization: "Basic abc", "x-admin-domain": "admin.apitoken.sale" } },
      reply,
    )).resolves.toEqual({ authenticated: true });
    expect(reply.header).toHaveBeenCalledWith("X-Admin-Actor", "main-admin");
    expect(reply.header).toHaveBeenCalledWith("X-Admin-Account-Id", accountId);
  });

  it("keeps the Basic challenge only for the legacy Caddy contract", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    fake.authenticate.mockResolvedValue(null);
    await expect(controller.verify(
      { headers: { "x-admin-domain": "crm.apitoken.sale" } },
      reply,
    )).rejects.toBeInstanceOf(UnauthorizedException);
    expect(reply.header).toHaveBeenCalledWith(
      "WWW-Authenticate",
      'Basic realm="crm.apitoken.sale", charset="UTF-8"',
    );
    expect(sessions.authenticate).not.toHaveBeenCalled();
  });

  it("returns a challenge-free 401 contract for API requests without a session", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    sessions.authenticate.mockResolvedValue(null);
    await expect(controller.verify({ headers: {
      "x-admin-domain": "crm.apitoken.sale",
      "x-admin-auth-mode": "session-v1",
      "x-forwarded-method": "GET",
      "x-forwarded-uri": "/v1/chats?hot=true",
      accept: "application/json",
    } }, reply)).rejects.toBeInstanceOf(UnauthorizedException);
    expect(reply.header).not.toHaveBeenCalledWith("WWW-Authenticate", expect.anything());
    expect(reply.header).toHaveBeenCalledWith(
      "X-Admin-Login",
      "/__admin-auth/login?return_to=%2Fv1%2Fchats%3Fhot%3Dtrue",
    );
  });

  it("redirects document navigation to login and rejects an external return target", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    sessions.authenticate.mockResolvedValue(null);
    await expect(controller.verify({ headers: {
      "x-admin-domain": "crm.apitoken.sale",
      "x-admin-auth-mode": "session-v1",
      "x-forwarded-method": "GET",
      "x-forwarded-uri": "//attacker.example/path",
      "sec-fetch-dest": "document",
    } }, reply)).resolves.toEqual({ authenticated: false });
    expect(reply.status).toHaveBeenCalledWith(303);
    expect(reply.header).toHaveBeenCalledWith("Location", "/__admin-auth/login?return_to=%2F");
  });

  it("accepts a persistent cookie without rechecking the password", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    sessions.authenticate.mockResolvedValue({ id: accountId, username: "crm-admin", sessionVersion: "v".repeat(43) });
    await expect(controller.verify({ headers: {
      "x-admin-domain": "crm.apitoken.sale",
      "x-admin-auth-mode": "session-v1",
      cookie: "other=1; __Host-apitoken_admin_session=opaque-session",
    } }, reply)).resolves.toEqual({ authenticated: true });
    expect(sessions.authenticate).toHaveBeenCalledWith("opaque-session", "crm.apitoken.sale");
    expect(fake.authenticate).not.toHaveBeenCalled();
  });

  it("upgrades an explicitly supplied Basic credential into a persistent cookie", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    sessions.authenticate.mockResolvedValue(null);
    fake.authenticate.mockResolvedValue({ id: accountId, username: "crm-admin", sessionVersion: "v".repeat(43) });
    sessions.issue.mockReturnValue("new-session");
    await controller.verify({ headers: {
      "x-admin-domain": "crm.apitoken.sale",
      "x-admin-auth-mode": "session-v1",
      authorization: "Basic abc",
    } }, reply);
    expect(reply.header).toHaveBeenCalledWith("Set-Cookie", expect.stringContaining(
      "__Host-apitoken_admin_session=new-session; Path=/; HttpOnly; Secure; SameSite=Lax",
    ));
  });

  it("renders a mobile login and sets a 180-day host-only cookie after same-origin login", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    const reply = fakeReply();
    fake.authenticatePassword.mockResolvedValue({
      id: accountId,
      username: "crm-admin",
      sessionVersion: "v".repeat(43),
    });
    sessions.issue.mockReturnValue("issued-session");
    expect(controller.loginPage("crm.apitoken.sale", "/?tab=hot")).toContain("Войдите один раз");
    await expect(controller.browserLogin(
      "crm.apitoken.sale",
      "https://crm.apitoken.sale",
      { username: "crm-admin", password: "secret", return_to: "/?tab=hot" },
      reply,
    )).resolves.toBe("");
    expect(reply.status).toHaveBeenCalledWith(303);
    expect(reply.header).toHaveBeenCalledWith("Location", "/?tab=hot");
    expect(reply.header).toHaveBeenCalledWith("Set-Cookie", expect.stringMatching(
      /Max-Age=15552000; Expires=.*; Priority=High$/,
    ));
  });

  it("rejects cross-origin login form submission before checking a password", async () => {
    const fake = fakeAccounts();
    const sessions = fakeSessions();
    const controller = new InternalAdminAuthController(fake.service, sessions.service);
    await expect(controller.browserLogin(
      "crm.apitoken.sale",
      "https://attacker.example",
      { username: "crm-admin", password: "secret" },
      fakeReply(),
    )).rejects.toBeInstanceOf(ForbiddenException);
    expect(fake.authenticatePassword).not.toHaveBeenCalled();
  });
});

function fakeAccounts() {
  const accounts = {
    domains: vi.fn(),
    list: vi.fn(),
    create: vi.fn(),
    changePassword: vi.fn(),
    setDomains: vi.fn(),
    setStatus: vi.fn(),
    authenticate: vi.fn(),
    authenticatePassword: vi.fn(),
    resolveSessionIdentity: vi.fn(),
    importLegacy: vi.fn(),
  };
  return { ...accounts, service: accounts as unknown as AdminAccountsService };
}

function fakeSessions() {
  const sessions = { authenticate: vi.fn(), issue: vi.fn() };
  return { ...sessions, service: sessions as unknown as AdminSessionService };
}

function fakeReply() {
  const reply = { header: vi.fn(), status: vi.fn() };
  reply.status.mockReturnValue(reply);
  return reply;
}
