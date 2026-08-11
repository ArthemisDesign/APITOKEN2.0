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
    expect(environment.COMMERCE_BASE_URL).toBe("http://127.0.0.1:8791");
    expect(environment.PUBLIC_SALES_BASE_URL).toBe("https://partners.apitoken.sale");
    expect(environment.PUBLIC_MAIN_SITE_URL).toBe("https://apitoken.sale");
    expect(environment.EMAIL_DELIVERY_MODE).toBe("log");
    expect(environment.SYNC_INTERVAL_MS).toBe(60_000);
    expect(environment.EMAIL_POLL_INTERVAL_MS).toBe(5_000);
    expect(environment.DEFAULT_COMMISSION_BPS).toBe(1000);
    expect(environment.DEFAULT_SUB_COMMISSION_BPS).toBe(1000);
    expect(environment.SALES_MIN_PAYOUT_USD).toBe(0);
    expect(environment.PAYOUT_CHAIN_ID).toBe(56);
    expect(environment.PAYOUT_USDT_CONTRACT).toBe("0x55d398326f99059fF775485246999027B3197955");
    expect(environment.PAYOUT_GAS_PRICE_GWEI).toBe("0.05");
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

  it("uses one integer payout minimum and rejects fractional or negative values", () => {
    expect(validateEnvironment({ ...baseEnvironment, SALES_MIN_PAYOUT_USD: "10" }).SALES_MIN_PAYOUT_USD).toBe(10);
    expect(() => validateEnvironment({ ...baseEnvironment, SALES_MIN_PAYOUT_USD: "0.5" })).toThrow();
    expect(() => validateEnvironment({ ...baseEnvironment, SALES_MIN_PAYOUT_USD: "-1" })).toThrow();
  });

  it("requires the payout key and send RPC together", () => {
    expect(() => validateEnvironment({
      ...baseEnvironment,
      PAYOUT_HOT_WALLET_KEY: `0x${"11".repeat(32)}`,
    })).toThrow("PAYOUT_HOT_WALLET_KEY and PAYOUT_SEND_RPC_URL");
    expect(() => validateEnvironment({
      ...baseEnvironment,
      PAYOUT_SEND_RPC_URL: "https://bsc.example",
    })).toThrow("PAYOUT_HOT_WALLET_KEY and PAYOUT_SEND_RPC_URL");
    expect(() => validateEnvironment({
      ...baseEnvironment,
      PAYOUT_HOT_WALLET_KEY: `0x${"11".repeat(32)}`,
      PAYOUT_SEND_RPC_URL: "https://bsc.example",
    })).not.toThrow();
  });

  it("pins BSC mainnet, canonical USDT, valid read RPCs and bounded exact gas", () => {
    expect(() => validateEnvironment({ ...baseEnvironment, PAYOUT_CHAIN_ID: "97" })).toThrow("only BSC mainnet");
    expect(() => validateEnvironment({
      ...baseEnvironment,
      PAYOUT_USDT_CONTRACT: "0x0000000000000000000000000000000000000001",
    })).toThrow("canonical BSC USDT");
    expect(() => validateEnvironment({ ...baseEnvironment, PAYOUT_READ_RPC_URLS: "file:///tmp/rpc" })).toThrow("HTTP(S)");
    expect(() => validateEnvironment({ ...baseEnvironment, PAYOUT_GAS_PRICE_GWEI: "0" })).toThrow("gas price");
    expect(() => validateEnvironment({ ...baseEnvironment, PAYOUT_GAS_PRICE_GWEI: "0.0000000001" })).toThrow("gas price");
    expect(() => validateEnvironment({ ...baseEnvironment, PAYOUT_GAS_PRICE_GWEI: "100.000000001" })).toThrow("gas price");
    expect(validateEnvironment({ ...baseEnvironment, PAYOUT_GAS_PRICE_GWEI: "100" }).PAYOUT_GAS_PRICE_GWEI).toBe("100");
  });
});
