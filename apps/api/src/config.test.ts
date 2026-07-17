import { describe, expect, it } from "vitest";
import { validateEnvironment } from "./config.js";

const requiredEnvironment = {
  DATABASE_URL: "postgresql://commerce:password@127.0.0.1:5432/commerce",
  ENGINE_BASE_URL: "http://127.0.0.1:8787",
  ENGINE_CONTROL_KEY: "c".repeat(32),
  AUTH_TOKEN_ENCRYPTION_KEY: Buffer.alloc(32, 7).toString("base64url"),
};

describe("commercial API configuration", () => {
  it("keeps email verification disabled by default", () => {
    expect(validateEnvironment(requiredEnvironment).EMAIL_VERIFICATION_REQUIRED).toBe(false);
  });

  it("accepts an explicit email verification switch", () => {
    expect(validateEnvironment({
      ...requiredEnvironment,
      EMAIL_VERIFICATION_REQUIRED: "true",
    }).EMAIL_VERIFICATION_REQUIRED).toBe(true);
    expect(validateEnvironment({
      ...requiredEnvironment,
      EMAIL_VERIFICATION_REQUIRED: "false",
    }).EMAIL_VERIFICATION_REQUIRED).toBe(false);
  });
});
