import { BadRequestException, ConflictException, UnauthorizedException } from "@nestjs/common";
import { describe, expect, it, vi } from "vitest";
import { ManagedAdminConflictError } from "@claude-api/db";
import { AdminAccountsController } from "./admin-accounts.controller.js";
import { type AdminAccountsService } from "./admin-accounts.service.js";
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
    const controller = new InternalAdminAuthController(fake.service);
    const reply = { header: vi.fn() };
    await expect(controller.verify(
      "Basic abc",
      "backend.apitoken.sale",
      reply,
    )).rejects.toBeInstanceOf(UnauthorizedException);
    expect(fake.authenticate).not.toHaveBeenCalled();
  });

  it("returns actor headers only after domain-scoped authentication", async () => {
    const fake = fakeAccounts();
    const controller = new InternalAdminAuthController(fake.service);
    const reply = { header: vi.fn() };
    fake.authenticate.mockResolvedValue({ id: accountId, username: "main-admin" });
    await expect(controller.verify(
      "Basic abc",
      "admin.apitoken.sale",
      reply,
    )).resolves.toEqual({ authenticated: true });
    expect(reply.header).toHaveBeenCalledWith("X-Admin-Actor", "main-admin");
    expect(reply.header).toHaveBeenCalledWith("X-Admin-Account-Id", accountId);
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
    importLegacy: vi.fn(),
  };
  return { ...accounts, service: accounts as unknown as AdminAccountsService };
}
