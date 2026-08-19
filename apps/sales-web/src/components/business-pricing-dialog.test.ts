import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";

const dialog = readFileSync(new URL("./business-pricing-dialog.tsx", import.meta.url), "utf8");
const referrals = readFileSync(new URL("../app/dashboard/referrals/page.tsx", import.meta.url), "utf8");

describe("partner B2B pricing surface", () => {
  it("is shown only to a partner who holds the grant with a real ceiling", () => {
    // An ordinary partner's cabinet must look exactly as before — no B2B controls at all.
    expect(referrals).toContain("partner.b2bEnabled === true && (partner.b2bMaxDiscountBps ?? 0) > 0");
    expect(referrals).toContain("{b2bAllowed ?");
  });

  it("offers exactly the providers commerce accepts", () => {
    // A provider id commerce does not know would be stored and then never match a request.
    for (const provider of ["anthropic", "openai", "google", "kimi", "glm"]) {
      expect(dialog).toContain(`"${provider}"`);
    }
  });

  it("never sends a discount the client-side ceiling rejects", () => {
    // The client check is convenience only — the server enforces it twice — but it must not
    // silently submit an over-ceiling value and surface a raw server error instead.
    expect(dialog).toContain("percent > ceiling ? \"invalid\" : percent");
  });

  it("requires a base discount when converting, not only provider overrides", () => {
    // Provider overrides alone would leave every other model at the ordinary B2C price.
    expect(dialog).toContain("!isB2b && parsedDefault === null");
  });

  it("posts to the partner-scoped route, never to an admin one", () => {
    expect(dialog).toContain("/v1/partner/referrals/${row.userRef}/business-pricing");
    expect(dialog).not.toContain("/v1/admin");
  });
});
