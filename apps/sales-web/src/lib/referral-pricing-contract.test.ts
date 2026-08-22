import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), "utf8");
}

describe("referral pricing copy contract", () => {
  it("does not expose legacy marker writers as a customer discount", () => {
    const overview = source("../app/dashboard/page.tsx");
    const referrals = source("../app/dashboard/referrals/page.tsx");
    const onboarding = source("../app/admin/page.tsx");
    const analytics = source("../app/admin/partner-analytics.tsx");

    expect(overview).not.toContain("referral-discount-card");
    expect(referrals).not.toContain("/discount");
    expect(referrals).not.toMatch(/personal partner rate|партнёрскую ставку|price floor/iu);
    expect(onboarding).not.toMatch(/referralDiscount|promoMax/);
    expect(onboarding).not.toMatch(/allow this partner to give|price floor/iu);
    expect(analytics).not.toContain("/referrals/${u.userRef}/discount");
    expect(analytics).not.toMatch(/Legacy marker|Старый маркер|discountLinks/);
  });
});
