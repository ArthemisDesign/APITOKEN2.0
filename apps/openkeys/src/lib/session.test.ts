import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { cookies } from "next/headers";
import type { OpenkeysConfig } from "./config";
import { authenticate, currentAdmin, issueSessionValue, SESSION_COOKIE, sessionUser } from "./session";

vi.mock("next/headers", () => ({ cookies: vi.fn() }));

const config: OpenkeysConfig = {
  databaseUrl: "postgresql://localhost/openkeys",
  engineBaseUrl: "http://127.0.0.1:8790",
  engineControlKey: "control-key",
  enginePublicBaseUrl: "https://api.apitoken.sale",
  engineOpenAiPublicBaseUrl: "https://openai.api.apitoken.sale",
  adminAccounts: [
    { user: "first", password: "first-password" },
    { user: "second", password: "second-password" },
  ],
  sessionSecret: "0123456789abcdef0123456789abcdef",
  sessionTtlSeconds: 3600,
  publicBaseUrl: "https://openkeys.apitoken.sale",
};

describe("authenticate", () => {
  it("пускает любую из объявленных учёток", () => {
    expect(authenticate("first", "first-password", config)).toBe("first");
    expect(authenticate("second", "second-password", config)).toBe("second");
  });

  it("не принимает чужой пароль и несуществующий логин", () => {
    expect(authenticate("first", "second-password", config)).toBeNull();
    expect(authenticate("third", "first-password", config)).toBeNull();
    expect(authenticate("", "", config)).toBeNull();
  });
});

describe("сессия", () => {
  it("выданную куку принимает и возвращает имя вошедшего", () => {
    const { value } = issueSessionValue("second", config);
    expect(sessionUser(value, config)).toBe("second");
  });

  it("подделанную подпись отвергает", () => {
    const { value } = issueSessionValue("first", config);
    const parts = value.split(".");
    parts[3] = "tampered";
    expect(sessionUser(parts.join("."), config)).toBeNull();
  });

  it("не даёт подменить пользователя, сохранив подпись", () => {
    const { value } = issueSessionValue("first", config);
    const parts = value.split(".");
    parts[2] = Buffer.from("second", "utf8").toString("base64url");
    expect(sessionUser(parts.join("."), config)).toBeNull();
  });

  it("истёкшую куку не принимает", () => {
    const expired = issueSessionValue("first", { ...config, sessionTtlSeconds: -1 });
    expect(sessionUser(expired.value, config)).toBeNull();
  });

  it("удалённая из конфига учётка теряет доступ немедленно", () => {
    const { value } = issueSessionValue("second", config);
    const withoutSecond: OpenkeysConfig = {
      ...config,
      adminAccounts: [{ user: "first", password: "first-password" }],
    };
    expect(sessionUser(value, withoutSecond)).toBeNull();
  });

  it("мусор вместо куки не роняет проверку", () => {
    for (const value of ["", "a.b.c", "1.2.3.4.5", "notanumber.n.u.s"]) {
      expect(sessionUser(value, config)).toBeNull();
    }
  });
});

describe("currentAdmin", () => {
  const mockCookies = vi.mocked(cookies);

  function cookieStore(value: string | undefined) {
    return { get: () => (value === undefined ? undefined : { name: SESSION_COOKIE, value }) };
  }

  beforeAll(() => {
    vi.stubEnv("OPENKEYS_SESSION_SECRET", config.sessionSecret);
    vi.stubEnv("OPENKEYS_DATABASE_URL", config.databaseUrl);
    vi.stubEnv("ENGINE_CONTROL_KEY", config.engineControlKey);
    vi.stubEnv("OPENKEYS_ADMIN_USER", "first");
    vi.stubEnv("OPENKEYS_ADMIN_PASSWORD", "first-password");
  });

  afterAll(() => {
    vi.unstubAllEnvs();
  });

  it("валидную сессию принимает, отсутствующую отклоняет — оба пути без error-лога", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { value } = issueSessionValue("first", config);

    mockCookies.mockResolvedValueOnce(cookieStore(value) as never);
    await expect(currentAdmin()).resolves.toBe("first");

    mockCookies.mockResolvedValueOnce(cookieStore(undefined) as never);
    await expect(currentAdmin()).resolves.toBeNull();

    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it("падение конфигурации env логирует, но по-прежнему отвечает null (401)", async () => {
    vi.stubEnv("OPENKEYS_SESSION_SECRET", "");
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { value } = issueSessionValue("first", config);

    mockCookies.mockResolvedValueOnce(cookieStore(value) as never);
    await expect(currentAdmin()).resolves.toBeNull();

    expect(spy).toHaveBeenCalledWith("openkeys admin session check failed", expect.objectContaining({
      message: expect.stringContaining("OPENKEYS_SESSION_SECRET"),
    }));
    spy.mockRestore();
  });
});
