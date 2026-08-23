import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const source = readFileSync(join(import.meta.dirname, "referral.tsx"), "utf8");
const apiSource = readFileSync(join(import.meta.dirname, "..", "..", "..", "lib", "api.ts"), "utf8");
const fixtureSource = readFileSync(join(import.meta.dirname, "..", "..", "..", "lib", "preview-fixtures.ts"), "utf8");

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
    expect(source).toContain('"payouts", "docs"');
    expect(source).not.toContain('"settings"');
  });

  it("keeps referral actions on an email-owned row and exposes search plus the B2B ceiling", () => {
    expect(source).toContain('type="search"');
    expect(source).toContain("setPricing(item)");
    expect(source).toContain("BusinessPricingDialog");
    expect(source).toContain("snapshot.membership.b2bMaxDiscountBps");
    expect(source).toContain("value * 100");
    expect(source).not.toMatch(/balanceNano/);
  });

  it("uses production provider metadata while hiding non-production GLM from referral analytics", () => {
    expect(source).toContain("DASHBOARD_PROVIDERS");
    expect(source).toContain('new Set(["glm", "zai", "zhipu"])');
    expect(source).toContain('className="uprovider-card"');
    expect(source).toContain('className="usage-graph referral-earnings-graph"');
  });

  it("offers reviewable active and no-access states only through preview fixtures", () => {
    expect(fixtureSource).toContain('get("partner-preview")');
    expect(fixtureSource).toContain('previewState === "no-access"');
    expect(fixtureSource).toContain('{ state: "unavailable", membership: null }');
    expect(apiSource).toContain('process.env.NEXT_PUBLIC_PREVIEW_FIXTURES === "1"');
  });

  it("confirms destructive invitation revocation and manages modal focus", () => {
    expect(source).toContain('role="alertdialog"');
    expect(source).toContain("revokeInviteTitle");
    expect(source).toContain('event.key === "Escape"');
    expect(source).toContain('event.key !== "Tab"');
    expect(source).toContain("if (await revoke(revoking.id))");
  });

  it("lets an account without access apply for review and reach Telegram", () => {
    expect(source).toContain("api.submitReferralApplication");
    expect(source).toContain('api.referralApplication()');
    expect(source).toContain('href="https://t.me/bozinodev"');
    expect(source).toContain("applyPendingTitle");
    expect(fixtureSource).toContain('case "GET /referral/applications/me"');
    expect(apiSource).toContain('request<{ application: ReferralApplication }>("/referral/applications"');
  });

  it("contains no promo-code workflow", () => {
    expect(source).not.toMatch(/promo.?code|промокод/i);
    expect(apiSource).not.toContain("redeemPromo");
  });
});
