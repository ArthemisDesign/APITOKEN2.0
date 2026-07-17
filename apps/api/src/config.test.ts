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

  it("requires a complete GitHub OAuth configuration", () => {
    expect(() => validateEnvironment({
      ...requiredEnvironment,
      GITHUB_CLIENT_ID: "github-client-id",
    })).toThrow("all GitHub OAuth settings must be provided together");
  });

  it("accepts only the canonical GitHub callback in production", () => {
    const githubEnvironment = {
      ...requiredEnvironment,
      NODE_ENV: "production",
      GITHUB_CLIENT_ID: "github-client-id",
      GITHUB_CLIENT_SECRET: "github-client-secret",
      GITHUB_REDIRECT_URI: "https://backend.apitoken.sale/v1/auth/github/callback",
    } as const;

    expect(validateEnvironment(githubEnvironment).GITHUB_CLIENT_ID).toBe("github-client-id");
    expect(() => validateEnvironment({
      ...githubEnvironment,
      GITHUB_REDIRECT_URI: "https://backend.apitoken.sale/v1/auth/github/other",
    })).toThrow("GITHUB_REDIRECT_URI must use the canonical production callback");
  });
});
