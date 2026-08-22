import { describe, expect, it } from "vitest";
import { FEEDS, feedsForPath } from "./feeds";

describe("page-scoped SSE feeds", () => {
  it("opens commerce, partner and engine feeds on the dashboard", () => {
    expect(feedsForPath("/")).toEqual(["/admin/events", "/partner-admin/events", "/events/engine"]);
  });

  it("opens provider engine feeds on subscriptions and skips commerce", () => {
    expect(feedsForPath("/subscriptions")).toEqual([
      "/events/engine",
      "/events/openai",
      "/events/gemini",
      "/events/kimi",
    ]);
  });

  it("opens a single feed on single-source screens", () => {
    expect(feedsForPath("/proxies")).toEqual(["/proxy-admin/events"]);
    expect(feedsForPath("/partners")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/partners/directory")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/partners/onboarding")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/partners/payouts")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/partners/requests")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/partners/abc-id")).toEqual(["/admin/events", "/partner-admin/events"]);
    expect(feedsForPath("/topups")).toEqual(["/admin/events"]);
  });

  it("falls back to commerce and engine for an unknown path", () => {
    expect(feedsForPath("/not-a-real-page")).toEqual(["/admin/events", "/events/engine"]);
  });

  it("never asks for a feed outside the known catalog", () => {
    const catalog = new Set<string>(FEEDS);
    for (const path of ["/", "/subscriptions", "/accounts", "/sales/calculator", "/finance", "/audit"]) {
      for (const feed of feedsForPath(path)) {
        expect(catalog.has(feed), `${feed} on ${path}`).toBe(true);
      }
    }
  });
});
