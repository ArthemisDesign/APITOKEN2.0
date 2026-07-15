import { describe, expect, it } from "vitest";
import { dashboardHref, dashboardSections, parseDashboardSection } from "./dashboard-route";

describe("dashboard URL state", () => {
  it("round-trips every dashboard view through a reloadable URL", () => {
    for (const section of dashboardSections) {
      const href = dashboardHref(section);
      const url = new URL(href, "https://apitoken.sale");
      expect(parseDashboardSection(url.searchParams.get("view"))).toBe(section);
    }
  });

  it("uses the clean dashboard URL for overview and rejects unknown views", () => {
    expect(dashboardHref("overview")).toBe("/dashboard");
    expect(parseDashboardSection(null)).toBe("overview");
    expect(parseDashboardSection("billing-admin")).toBe("overview");
  });
});
