import { describe, expect, it } from "vitest";
import { usesAuthEntryGuard, usesAuthShell, usesPublicSiteShell } from "./persistent-route-shell";

describe("persistent route shell routing", () => {
  it("wraps every marketing and public-information page", () => {
    for (const path of [
      "/",
      "/ru",
      "/models/claude-opus-4-8",
      "/about",
      "/contacts",
      "/changelog",
      "/status",
      "/blog/example",
    ]) expect(usesPublicSiteShell(path), path).toBe(true);
  });

  it("keeps documentation and private app routes out of the marketing shell", () => {
    for (const path of ["/docs", "/ru/docs", "/dashboard", "/usage"]) {
      expect(usesPublicSiteShell(path), path).toBe(false);
    }
  });

  it("wraps English and Russian auth forms but not a fabricated localized callback", () => {
    for (const path of ["/login", "/ru/login", "/ru/reset-password", "/auth/callback"]) {
      expect(usesAuthShell(path), path).toBe(true);
    }
    expect(usesAuthShell("/ru/auth/callback")).toBe(false);
  });

  it("checks authentication before rendering every login and registration route", () => {
    for (const path of ["/login", "/register", "/ru/login", "/ru/register"]) {
      expect(usesAuthEntryGuard(path), path).toBe(true);
    }
    for (const path of ["/forgot-password", "/verify-email", "/auth/callback", "/dashboard"]) {
      expect(usesAuthEntryGuard(path), path).toBe(false);
    }
  });
});
