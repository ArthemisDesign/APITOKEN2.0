import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { OpenkeysConfig } from "@/lib/config";
import { resolveViewTokenByApiKey } from "@/lib/keys";
import { usageSessionToken, USAGE_SESSION_COOKIE } from "@/lib/usage-session";
import { POST as login } from "./lookup/route";
import { POST as logout } from "./logout/route";

vi.mock("@/lib/keys", () => ({ resolveViewTokenByApiKey: vi.fn() }));

const sessionSecret = "0123456789abcdef0123456789abcdef";
const oldViewToken = "abcdefghijklmnopqrstuv";
const newViewToken = "1234567890123456789012";
const config = { sessionSecret } as OpenkeysConfig;

function request(path: string, body?: unknown): Request {
  return new Request(`https://openkeys.apitoken.sale${path}`, {
    method: "POST",
    headers: {
      origin: "https://openkeys.apitoken.sale",
      ...(body === undefined ? {} : { "content-type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

describe("OpenKeys usage-session routes", () => {
  beforeEach(() => {
    vi.stubEnv("OPENKEYS_SESSION_SECRET", sessionSecret);
    vi.stubEnv("OPENKEYS_DATABASE_URL", "postgresql://openkeys:test@127.0.0.1:5432/openkeys");
    vi.stubEnv("ENGINE_CONTROL_KEY", "test-control-key");
    vi.stubEnv("OPENKEYS_ADMIN_USER", "admin");
    vi.stubEnv("OPENKEYS_ADMIN_PASSWORD", "test-password");
    vi.mocked(resolveViewTokenByApiKey).mockReset();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("replaces an existing profile session when another key logs in", async () => {
    vi.mocked(resolveViewTokenByApiKey)
      .mockResolvedValueOnce(oldViewToken)
      .mockResolvedValueOnce(newViewToken);

    const oldResponse = await login(request("/api/usage/lookup", { key: "sk-pool-old-key" }));
    const oldCookie = oldResponse.cookies.get(USAGE_SESSION_COOKIE)?.value;
    expect(oldResponse.status).toBe(200);
    expect(usageSessionToken(oldCookie, config)).toBe(oldViewToken);

    const newResponse = await login(request("/api/usage/lookup", { key: "sk-pool-new-key" }));
    const newCookie = newResponse.cookies.get(USAGE_SESSION_COOKIE)?.value;
    expect(newResponse.status).toBe(200);
    expect(newCookie).not.toBe(oldCookie);
    expect(usageSessionToken(newCookie, config)).toBe(newViewToken);

    const setCookie = newResponse.headers.get("set-cookie") ?? "";
    expect(setCookie).toContain(`${USAGE_SESSION_COOKIE}=`);
    expect(setCookie).toMatch(/Path=\//i);
    expect(setCookie).toMatch(/HttpOnly/i);
    expect(setCookie).toMatch(/Secure/i);
    expect(setCookie).toMatch(/SameSite=lax/i);
  });

  it("deletes the exact host-only profile cookie on logout", async () => {
    const response = await logout(request("/api/usage/logout"));
    expect(response.status).toBe(200);
    expect(response.cookies.get(USAGE_SESSION_COOKIE)?.value).toBe("");

    const setCookie = response.headers.get("set-cookie") ?? "";
    expect(setCookie).toContain(`${USAGE_SESSION_COOKIE}=`);
    expect(setCookie).toMatch(/Path=\//i);
    expect(setCookie).toMatch(/Max-Age=0/i);
    expect(setCookie).toMatch(/HttpOnly/i);
    expect(setCookie).toMatch(/Secure/i);
    expect(setCookie).toMatch(/SameSite=lax/i);
  });
});
