import { describe, expect, it } from "vitest";
import { withoutSensitiveUrlData } from "./components/site-analytics";

describe("Vercel Analytics URL privacy", () => {
  it("keeps the route while removing query strings and fragments", () => {
    expect(withoutSensitiveUrlData("https://apitoken.sale/auth/callback?code=secret&state=private#done"))
      .toBe("https://apitoken.sale/auth/callback");
    expect(withoutSensitiveUrlData("/reset-password?token=secret"))
      .toBe("/reset-password");
    expect(withoutSensitiveUrlData("/privacy"))
      .toBe("/privacy");
  });
});
