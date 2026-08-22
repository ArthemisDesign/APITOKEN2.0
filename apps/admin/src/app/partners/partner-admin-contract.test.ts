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
    const nav = source("../../lib/nav.ts");
    expect(nav).toContain('href: "/partners"');
  });

  it("approves an application atomically with every authority field", () => {
    const onboarding = source("./onboarding/page.tsx");
    const decision = onboarding.slice(
      onboarding.indexOf("async function decideApplication"),
      onboarding.indexOf("const applications ="),
    );

    expect(decision).toContain("/decision");
    for (const field of [
      "commissionBps",
      "subCommissionBps",
      "teamOverrideMaxBps",
      "teamInvitesEnabled",
      "b2bEnabled",
      "b2bMaxDiscountBps",
      "b2bCanDelegate",
    ]) {
      expect(decision).toContain(field);
    }
    expect(decision).not.toContain("/partner-admin/partners/");
    expect(decision).not.toMatch(/partially|частич|finish the settings|завершите настройку/i);
  });

  it("keeps all active Partner Admin surfaces free of promo writers", () => {
    const files = [
      "./page.tsx",
      "./directory/page.tsx",
      "./onboarding/page.tsx",
      "./requests/page.tsx",
      "./payouts/page.tsx",
      "./[id]/page.tsx",
    ];
    for (const file of files) {
      const contents = source(file);
      expect(contents).not.toMatch(/promoMax|referralDiscount|promo codes|промокоды/i);
    }
  });

  it("renders account identities email-first and keeps pre-account Telegram explicit", () => {
    const directory = source("./directory/page.tsx");
    const detail = source("./[id]/page.tsx");
    const onboarding = source("./onboarding/page.tsx");
    const helpers = source("./helpers.ts");
    expect(directory).toContain("partner.email ?? partner.displayName ??");
    expect(detail).toContain("partner.email ?? partner.displayName ??");
    expect(helpers).toContain("email?: string");
    expect(onboarding).toContain("Pre-account identity");
    expect(onboarding).toContain("after registration email is the displayed identity");
  });
});
