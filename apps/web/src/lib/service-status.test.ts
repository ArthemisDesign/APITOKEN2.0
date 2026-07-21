import { describe, expect, it } from "vitest";
import { deriveServiceStatus, isCoreHealth, type CoreHealth } from "./service-status";

const healthy: CoreHealth = {
  ok: true,
  service: "commercial-api",
  database: "up",
  engine: "up",
};

describe("public service status", () => {
  it("maps the existing commercial health check without claiming payment-provider coverage", () => {
    const status = deriveServiceStatus(healthy, "2026-07-21T12:00:00.000Z");
    expect(status.overall).toBe("operational");
    expect(status.components.find((entry) => entry.name.startsWith("API gateway"))?.level).toBe("operational");
    expect(status.components.find((entry) => entry.name.startsWith("Payments"))).toMatchObject({
      level: "unknown",
      label: "Not independently monitored",
    });
  });

  it("reports dependency failures as degraded instead of hard-coding all systems operational", () => {
    const status = deriveServiceStatus({ ...healthy, ok: false, engine: "down" });
    expect(status.overall).toBe("degraded");
    expect(status.components.find((entry) => entry.name.startsWith("API gateway"))?.level).toBe("unavailable");
    expect(status.components.find((entry) => entry.name.startsWith("Dashboard"))?.level).toBe("degraded");
  });

  it("treats failed or malformed checks as unknown, not as an outage or a success", () => {
    expect(deriveServiceStatus(null).overall).toBe("unknown");
    expect(isCoreHealth({ ok: true, service: "commercial-api", database: "up" })).toBe(false);
  });
});
