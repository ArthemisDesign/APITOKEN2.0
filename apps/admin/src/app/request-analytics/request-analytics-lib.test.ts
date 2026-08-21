import { describe, expect, it } from "vitest";
import { displayValue, durationLabel, requestAnalyticsUrls, requestAnalyticsWindow, routeLabel } from "./request-analytics-lib";

describe("request analytics presentation", () => {
  it("builds exact half-open summary and keyset URLs", () => {
    expect(requestAnalyticsWindow(24, 100_000_000)).toEqual({ from: 13_600, to: 100_000 });
    expect(requestAnalyticsUrls(24, "a+b", 100_000_000)).toEqual({
      summary: "/admin/request-analytics/summary?from=13600&to=100000",
      page: "/admin/request-analytics?from=13600&to=100000&limit=100&cursor=a%2Bb",
    });
  });

  it("keeps unknown and unmeasured evidence explicit", () => {
    expect(displayValue("unknown")).toBe("—");
    expect(displayValue(null)).toBe("—");
    expect(durationLabel(null)).toBe("—");
    expect(durationLabel(0)).toBe("< 1 с");
    expect(routeLabel({ provider_plane: "openai", route_class: "native", request_class: "responses" }))
      .toBe("openai · native · responses");
  });
});
