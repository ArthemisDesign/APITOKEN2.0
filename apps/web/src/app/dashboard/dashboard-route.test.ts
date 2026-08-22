import { describe, expect, it } from "vitest";
import { dashboardHref, dashboardSections, parseDashboardSection } from "./dashboard-route";

describe("dashboard URL state", () => {
  it("round-trips every dashboard view through a reloadable URL", () => {
    for (const section of dashboardSections) {
      for (const language of ["en", "ru"] as const) {
        const href = dashboardHref(section, language);
        const url = new URL(href, "https://apitoken.sale");
        expect(parseDashboardSection(url.searchParams.get("view"))).toBe(section);
        expect(url.pathname).toBe(language === "ru" ? "/ru/dashboard" : "/dashboard");
      }
    }
  });

  it("uses the clean dashboard URL for overview and rejects unknown views", () => {
    expect(dashboardHref("overview")).toBe("/dashboard");
    expect(dashboardHref("overview", "ru")).toBe("/ru/dashboard");
    expect(dashboardHref("keys", "ru")).toBe("/ru/dashboard?view=keys");
    expect(parseDashboardSection(null)).toBe("overview");
    expect(parseDashboardSection("billing-admin")).toBe("overview");
    expect(parseDashboardSection("promos")).toBe("overview");
    expect(parseDashboardSection("refer")).toBe("overview");
    expect(parseDashboardSection("orders")).toBe("overview");
  });

  it("keeps the retired security URL useful by opening Profile", () => {
    expect(parseDashboardSection("security")).toBe("profile");
  });
});
