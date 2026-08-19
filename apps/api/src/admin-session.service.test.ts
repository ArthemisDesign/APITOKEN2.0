import { afterEach, describe, expect, it, vi } from "vitest";
import { type AdminAccountsService } from "./admin-accounts.service.js";
import { AdminSessionService, ADMIN_SESSION_TTL_SECONDS } from "./admin-session.service.js";

const accountId = "11111111-1111-4111-8111-111111111111";
const sessionVersion = "s".repeat(43);
const identity = { id: accountId, username: "crm-admin", sessionVersion };
const issuedAt = Date.UTC(2026, 7, 19, 12, 0, 0);

afterEach(() => vi.restoreAllMocks());

describe("managed admin sessions", () => {
  it("issues a domain-bound token and revalidates the current account grant", async () => {
    vi.spyOn(Date, "now").mockReturnValue(issuedAt);
    const fake = fakeAccounts();
    fake.resolveSessionIdentity.mockResolvedValue(identity);
    const sessions = makeService(fake.service);

    const token = sessions.issue(identity, "crm.apitoken.sale");

    await expect(sessions.authenticate(token, "crm.apitoken.sale")).resolves.toEqual(identity);
    expect(fake.resolveSessionIdentity).toHaveBeenCalledWith({
      accountId,
      domain: "crm.apitoken.sale",
      sessionVersion,
    });
  });

  it("rejects a token on another managed domain before querying the account", async () => {
    vi.spyOn(Date, "now").mockReturnValue(issuedAt);
    const fake = fakeAccounts();
    const sessions = makeService(fake.service);
    const token = sessions.issue(identity, "crm.apitoken.sale");

    await expect(sessions.authenticate(token, "admin.apitoken.sale")).resolves.toBeNull();
    expect(fake.resolveSessionIdentity).not.toHaveBeenCalled();
  });

  it("rejects tampering, malformed values, and expired tokens without account lookup", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(issuedAt);
    const fake = fakeAccounts();
    const sessions = makeService(fake.service);
    const token = sessions.issue(identity, "crm.apitoken.sale");
    const tampered = `${token.slice(0, -1)}${token.endsWith("a") ? "b" : "a"}`;

    await expect(sessions.authenticate(tampered, "crm.apitoken.sale")).resolves.toBeNull();
    await expect(sessions.authenticate("not.a.valid.token", "crm.apitoken.sale")).resolves.toBeNull();
    await expect(sessions.authenticate("x".repeat(2_049), "crm.apitoken.sale")).resolves.toBeNull();

    now.mockReturnValue(issuedAt + ADMIN_SESSION_TTL_SECONDS * 1_000);
    await expect(sessions.authenticate(token, "crm.apitoken.sale")).resolves.toBeNull();
    expect(fake.resolveSessionIdentity).not.toHaveBeenCalled();
  });

  it("accepts a token signed by the temporary previous key during rotation", async () => {
    vi.spyOn(Date, "now").mockReturnValue(issuedAt);
    const fake = fakeAccounts();
    fake.resolveSessionIdentity.mockResolvedValue(identity);
    const oldService = makeService(fake.service, "o".repeat(32));
    const token = oldService.issue(identity, "crm.apitoken.sale");
    const rotatedService = makeService(fake.service, "n".repeat(32), "o".repeat(32));

    await expect(rotatedService.authenticate(token, "crm.apitoken.sale")).resolves.toEqual(identity);
  });

  it("fails closed when the account, password, status, or domain grant was revoked", async () => {
    vi.spyOn(Date, "now").mockReturnValue(issuedAt);
    const fake = fakeAccounts();
    fake.resolveSessionIdentity.mockResolvedValue(null);
    const sessions = makeService(fake.service);
    const token = sessions.issue(identity, "crm.apitoken.sale");

    await expect(sessions.authenticate(token, "crm.apitoken.sale")).resolves.toBeNull();
  });

  it("refuses to issue or verify sessions when no signing key is configured", async () => {
    const fake = fakeAccounts();
    const sessions = new AdminSessionService(fake.service, { get: () => undefined } as never);

    expect(() => sessions.issue(identity, "crm.apitoken.sale")).toThrow("session key is unavailable");
    await expect(sessions.authenticate(`payload.${"s".repeat(43)}`, "crm.apitoken.sale"))
      .rejects.toThrow("session key is unavailable");
  });
});

function makeService(
  accounts: AdminAccountsService,
  current: string | undefined = "c".repeat(32),
  previous?: string,
): AdminSessionService {
  const config = {
    get: (name: string) => name === "COMMERCIAL_ADMIN_KEY" ? current : previous,
  };
  return new AdminSessionService(accounts, config as never);
}

function fakeAccounts() {
  const accounts = { resolveSessionIdentity: vi.fn() };
  return { ...accounts, service: accounts as unknown as AdminAccountsService };
}
