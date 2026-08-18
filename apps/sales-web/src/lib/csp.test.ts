import { describe, expect, it } from "vitest";
import { contentSecurityPolicy, isTelegramWidgetPath } from "./csp";

function directive(policy: string, name: string): string {
  const found = policy.split("; ").find((part) => part.startsWith(`${name} `) || part === name);
  if (!found) throw new Error(`no ${name} directive in: ${policy}`);
  return found;
}

describe("content security policy", () => {
  it("lets the Telegram widget run on the sign-in pages", () => {
    for (const path of ["/login", "/register", "/login/", "/register/"]) {
      const policy = contentSecurityPolicy(path);
      // Виджет вычисляет код строкой — без 'unsafe-eval' кнопка входа не рендерится вообще.
      expect(directive(policy, "script-src")).toContain("'unsafe-eval'");
      expect(directive(policy, "script-src")).toContain("https://telegram.org");
      expect(directive(policy, "frame-src")).toBe("frame-src https://oauth.telegram.org");
    }
  });

  it("keeps the cabinet and admin under the strict policy", () => {
    for (const path of ["/", "/dashboard", "/dashboard/payouts", "/admin", "/loginx", "/dashboard/login"]) {
      const policy = contentSecurityPolicy(path);
      expect(directive(policy, "script-src")).toBe("script-src 'self' 'unsafe-inline'");
      expect(directive(policy, "frame-src")).toBe("frame-src 'none'");
    }
  });

  it("keeps the invariants that hold everywhere", () => {
    for (const path of ["/login", "/dashboard"]) {
      const policy = contentSecurityPolicy(path);
      expect(directive(policy, "default-src")).toBe("default-src 'self'");
      expect(directive(policy, "connect-src")).toBe("connect-src 'self'");
      expect(directive(policy, "object-src")).toBe("object-src 'none'");
      expect(directive(policy, "base-uri")).toBe("base-uri 'none'");
      expect(directive(policy, "frame-ancestors")).toBe("frame-ancestors 'none'");
    }
  });

  it("treats only the exact sign-in routes as widget pages", () => {
    expect(isTelegramWidgetPath("/login")).toBe(true);
    expect(isTelegramWidgetPath("/register")).toBe(true);
    expect(isTelegramWidgetPath("/login/extra")).toBe(false);
    expect(isTelegramWidgetPath("/")).toBe(false);
  });
});
