import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), "utf8");
}

describe("unified Partner Admin UI contract", () => {
  it("owns every operator workflow in the main admin", () => {
    for (const route of ["directory", "onboarding", "requests", "payouts"]) {
      expect(existsSync(new URL(`./${route}/page.tsx`, import.meta.url))).toBe(true);
    }
    expect(existsSync(new URL("./[id]/page.tsx", import.meta.url))).toBe(true);
    expect(source("../../lib/nav.ts")).toContain('href: "/partners"');
  });

  it("onboards an existing Commerce account with every authority boundary", () => {
    const onboarding = source("./onboarding/page.tsx");
    const fields = source("./partner-onboarding-form.tsx");
    expect(onboarding).toContain('send("/admin/referral/partners", "POST"');
    expect(onboarding).toContain('type="email"');
    for (const field of [
      "commissionBps",
      "teamOverrideMaxBps",
      "teamInvitesEnabled",
      "b2bEnabled",
      "b2bMaxDiscountBps",
      "b2bCanDelegate",
    ]) {
      expect(fields).toContain(field);
    }
  });

  it("uses Commerce referral routes for identity, settings, requests, and manual payouts", () => {
    const files = [
      "./page.tsx",
      "./directory/page.tsx",
      "./onboarding/page.tsx",
      "./requests/page.tsx",
      "./[id]/page.tsx",
    ];
    for (const file of files) {
      const contents = source(file);
      expect(contents).not.toContain("/partner-admin/");
      expect(contents).not.toMatch(/telegramUsername|promoMax|referralDiscount/i);
    }
    expect(source("./payouts/page.tsx")).toContain('useResource<{ items: AdminPartnerPayout[] }>("/admin/referral/payouts")');
  });

  it("keeps the retained Team share separate from platform-funded direct commission", () => {
    const all = [
      source("./onboarding/page.tsx"),
      source("./partner-onboarding-form.tsx"),
      source("./directory/page.tsx"),
      source("./[id]/page.tsx"),
    ].join("\n");
    expect(all).toContain("удерживаемая Team-доля");
    expect(all).not.toMatch(/Надбавка Team|Team override/i);
    expect(all).toContain("Team share ≤ 20%");
  });

  it("adds full partner onboarding to the Commerce Users table", () => {
    const users = source("../users/page.tsx");
    const dialog = source("./partner-onboarding-form.tsx");
    expect(users).toContain("<PartnerOnboardingDialog");
    expect(users).toContain('t("Make Partner", "Сделать партнёром")');
    expect(dialog).toContain('`/admin/users/${encodeURIComponent(props.target.id)}/referral-partner`');
  });
});
