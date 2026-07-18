import { describe, expect, it } from "vitest";
import { validateEnvironment } from "./config.js";

const baseEnvironment = {
  SALES_DATABASE_URL: "postgres://sales:sales@127.0.0.1:5432/sales",
  SALES_TOKEN_ENCRYPTION_KEY: "A".repeat(43),
  SALES_ADMIN_KEY: "k".repeat(32),
  SALES_CONTROL_KEY: "c".repeat(32),
};

describe("validateEnvironment", () => {
  it("applies documented defaults", () => {
    const environment = validateEnvironment({ ...baseEnvironment });
    expect(environment.PORT).toBe(3100);
    expect(environment.HOST).toBe("127.0.0.1");
    expect(environment.SALES_SESSION_TTL_SECONDS).toBe(2_592_000);
    expect(environment.COMMERCE_BASE_URL).toBe("http://127.0.0.1:3000");
    expect(environment.PUBLIC_SALES_BASE_URL).toBe("https://sales.apitoken.sale");
    expect(environment.PUBLIC_MAIN_SITE_URL).toBe("https://apitoken.sale");
    expect(environment.EMAIL_DELIVERY_MODE).toBe("log");
    expect(environment.SYNC_INTERVAL_MS).toBe(60_000);
    expect(environment.EMAIL_POLL_INTERVAL_MS).toBe(5_000);
    expect(environment.DEFAULT_COMMISSION_BPS).toBe(1000);
    expect(environment.DEFAULT_SUB_COMMISSION_BPS).toBe(1000);
  });

  it("rejects a missing database url", () => {
    const { SALES_DATABASE_URL: _url, ...rest } = baseEnvironment;
    expect(() => validateEnvironment(rest)).toThrow();
  });

  it("rejects a malformed encryption key", () => {
    expect(() => validateEnvironment({ ...baseEnvironment, SALES_TOKEN_ENCRYPTION_KEY: "short" })).toThrow();
  });

  it("requires SMTP settings when delivery mode is smtp", () => {
    expect(() => validateEnvironment({ ...baseEnvironment, EMAIL_DELIVERY_MODE: "smtp" })).toThrow();
    expect(() => validateEnvironment({
      ...baseEnvironment,
      EMAIL_DELIVERY_MODE: "smtp",
      EMAIL_FROM: "partners@apitoken.sale",
      SMTP_HOST: "smtp.example.com",
      SMTP_PORT: "465",
    })).not.toThrow();
  });

  it("requires SMTP delivery in production", () => {
    expect(() => validateEnvironment({ ...baseEnvironment, NODE_ENV: "production" })).toThrow();
  });

  it("rejects one-sided SMTP credentials", () => {
    expect(() => validateEnvironment({ ...baseEnvironment, SMTP_USERNAME: "user" })).toThrow();
  });
});
