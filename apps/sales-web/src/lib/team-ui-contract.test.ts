import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), "utf8");
}

describe("partner team and referral UI contract", () => {
  it("uses the safe team writer and never lets an inviter choose the platform direct rate", () => {
    const team = source("../app/dashboard/team/page.tsx");

    expect(team).toContain('"/v1/partner/team/invites"');
    expect(team).toContain('`/v1/partner/team/${editing.id}`');
    expect(team).not.toContain("body: { telegramUsername: username, commissionBps }");
    expect(team).toContain("platformCommissionBps");
    expect(team).toContain("teamOverrideMaxBps");
  });

  it("renders Commerce email before the outage-safe referral mask", () => {
    const overview = source("../app/dashboard/page.tsx");
    const referrals = source("../app/dashboard/referrals/page.tsx");
    const analytics = source("../app/admin/partner-analytics.tsx");

    expect(overview).toContain("r.email ?? r.userMask");
    expect(referrals).toContain("r.email ?? r.userMask");
    expect(analytics).toContain("u.email ?? u.userMask");
  });

  it("has no promo-code route or visible control in either Sales surface", () => {
    const layout = source("../app/dashboard/layout.tsx");
    const onboarding = source("../app/admin/page.tsx");
    const analytics = source("../app/admin/partner-analytics.tsx");

    expect(existsSync(new URL("../app/dashboard/promo/page.tsx", import.meta.url))).toBe(false);
    expect(layout).not.toMatch(/promo codes|промокоды|dashboard\/promo/iu);
    expect(onboarding).not.toMatch(/promo codes|промокоды|promoMax/iu);
    expect(analytics).not.toMatch(/postPromo|editPromo|Promo:|Промо:/u);
  });
});
