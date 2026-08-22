import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const source = readFileSync(join(import.meta.dirname, "referral.tsx"), "utf8");
const apiSource = readFileSync(join(import.meta.dirname, "..", "..", "..", "lib", "api.ts"), "utf8");

describe("Commerce Dashboard partner surface", () => {
  it("keeps account identity email-first and session-owned", () => {
    expect(source).toContain('type="email" autoComplete="email"');
    expect(source).toContain('translate="no"');
    expect(source).not.toMatch(/commerceUserId|partnerId|telegramUsername/);
    expect(apiSource).not.toMatch(/referral(?:InviteTeam|UpdateTeam|RequestB2B|SetBusinessPricing)[\s\S]{0,300}commerceUserId/);
  });

  it("models Team income as a retained share capped at twenty percent", () => {
    expect(source).toContain("retained from a member’s fixed platform commission");
    expect(source).toContain("общая выплата останется $10");
    expect(source).toContain("не получаете надбавку сверху");
    expect(source).toContain("Math.min(2_000, snapshot.membership.teamOverrideMaxBps)");
    expect(source).not.toMatch(/markup/i);
  });

  it("keeps partner subviews reloadable and provides the manual-access state", () => {
    expect(source).toContain('url.searchParams.set("view", "referral")');
    expect(source).toContain('url.searchParams.set("tab", next)');
    expect(source).toContain('window.addEventListener("popstate", sync)');
    expect(source).toContain('href="https://t.me/bozinodev"');
    expect(source).toContain('snapshot.state === "unavailable"');
    expect(source).toContain('snapshot.state === "disabled"');
  });

  it("confirms destructive invitation revocation and manages modal focus", () => {
    expect(source).toContain('role="alertdialog"');
    expect(source).toContain("revokeInviteTitle");
    expect(source).toContain('event.key === "Escape"');
    expect(source).toContain('event.key !== "Tab"');
    expect(source).toContain("if (await revoke(revoking.id))");
  });

  it("contains no promo-code workflow", () => {
    expect(source).not.toMatch(/promo.?code|промокод/i);
    expect(apiSource).not.toContain("redeemPromo");
  });
});
