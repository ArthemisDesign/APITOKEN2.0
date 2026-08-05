import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { GeminiCapacityBoard } from "./gemini-capacity-board";
import { SubscriptionExpiry, subscriptionExpiryView } from "./subscription-lifecycle";

const NOW = 1_800_000_000;

describe("subscription lifecycle", () => {
  it("показывает absolute date и producer-computed remaining для Pro 18м", () => {
    const view = subscriptionExpiryView({
      acquired_at: 1_766_534_400,
      subscription_expires_at: 1_813_881_600,
      subscription_days_left: 160.666,
    }, NOW);
    expect(view).toEqual({ date: "25.06.2027", state: "remaining", detail: "осталось 161д" });
  });

  it("показывает Ultra/other 30d fixture без browser plan arithmetic", () => {
    const view = subscriptionExpiryView({
      acquired_at: NOW - 5 * 86_400,
      subscription_expires_at: NOW + 25 * 86_400,
      subscription_days_left: 25,
    }, NOW);
    expect(view.state).toBe("remaining");
    expect(view.detail).toBe("осталось 25д");
  });

  it("различает unknown и expired", () => {
    expect(subscriptionExpiryView({ subscription_expires_at: null, subscription_days_left: null }, NOW)).toEqual({
      date: "—", state: "unknown", detail: "неизвестно",
    });
    expect(subscriptionExpiryView({ subscription_expires_at: NOW - 86_400, subscription_days_left: -1 }, NOW)).toMatchObject({
      state: "expired", detail: "истекла 1д назад",
    });
    expect(subscriptionExpiryView({ subscription_expires_at: NOW + 2 * 86_400, subscription_days_left: null }, NOW)).toMatchObject({
      state: "remaining", detail: "осталось 2д",
    });
  });

  it("рендерит explicit unknown вместо нулевой даты", () => {
    const html = renderToString(<table><tbody><tr><SubscriptionExpiry lifecycle={{}} nowSeconds={NOW} /></tr></tbody></table>);
    expect(html).toContain("неизвестно");
    expect(html).not.toContain("01.01.1970");
  });

  it("актуальная Gemini board показывает Pro 18м, Ultra 30d, expired и unknown без identity", () => {
    const html = renderToString(
      <GeminiCapacityBoard
        nowMs={NOW * 1000}
        response={{
          now: NOW,
          profiles: [
            { id: "opaque-pro", email: "pro…", plan: "google_ai_pro", authenticated: true, subscription_expires_at: 1_813_881_600, subscription_days_left: 160.666 },
            { id: "opaque-ultra", email: "ultr…", plan: "google_ai_ultra", authenticated: true, subscription_expires_at: NOW + 25 * 86_400, subscription_days_left: 25 },
            { id: "opaque-expired", email: "expi…", plan: "code_assist_standard", authenticated: true, subscription_expires_at: NOW - 86_400, subscription_days_left: -1 },
            { id: "opaque-unknown", email: "unkn…", plan: "unreviewed", authenticated: true, subscription_expires_at: null, subscription_days_left: null },
          ],
        }}
      />,
    );
    expect(html).toContain("Окончание");
    expect(html).toContain("25.06.2027");
    expect(html).toContain("осталось 161д");
    expect(html).toContain("осталось 25д");
    expect(html).toContain("истекла 1д назад");
    expect(html).toContain("неизвестно");
    expect(html).not.toContain("opaque-pro");
    expect(html).not.toContain("owner@example.com");
  });
});
