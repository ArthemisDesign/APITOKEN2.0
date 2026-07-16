import { describe, expect, it } from "vitest";
import { withoutSensitiveUrlData } from "./components/site-analytics";
import { yandexMetrikaPageUrl } from "./lib/yandex-metrika";

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

describe("Yandex Metrika URL attribution privacy", () => {
  it("keeps campaign attribution while removing secrets and fragments", () => {
    expect(yandexMetrikaPageUrl(
      "https://apitoken.sale/?utm_source=codex&utm_medium=referral&utm_campaign=metrika_test&code=secret&state=private#done",
    )).toBe(
      "https://apitoken.sale/?utm_source=codex&utm_medium=referral&utm_campaign=metrika_test",
    );
  });

  it("keeps supported advertising and debugger parameters", () => {
    expect(yandexMetrikaPageUrl(
      "https://apitoken.sale/plans?yclid=123&gclid=456&_ym_debug=2&token=secret",
    )).toBe("https://apitoken.sale/plans?yclid=123&gclid=456&_ym_debug=2");
  });
});
