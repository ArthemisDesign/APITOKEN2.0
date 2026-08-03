import { describe, expect, it } from "vitest";
import { validateEnvironment } from "./config.js";

const requiredEnvironment = {
  DATABASE_URL: "postgresql://commerce:password@127.0.0.1:5432/commerce",
  ENGINE_BASE_URL: "http://127.0.0.1:8790",
  ENGINE_CONTROL_KEY: "c".repeat(32),
  AUTH_TOKEN_ENCRYPTION_KEY: Buffer.alloc(32, 7).toString("base64url"),
};

describe("commercial API configuration", () => {
  it("keeps email verification disabled by default", () => {
    const environment = validateEnvironment(requiredEnvironment);
    expect(environment.EMAIL_VERIFICATION_REQUIRED).toBe(false);
    expect(environment.OPENKEYS_INTERNAL_BASE_URL).toBe("http://127.0.0.1:3410");
    expect(environment.OPENKEYS_CONTROL_KEY).toBeUndefined();
  });

  it("accepts a dedicated OpenKeys Stage 5 credential only on a safe internal origin", () => {
    const environment = validateEnvironment({
      ...requiredEnvironment,
      OPENKEYS_INTERNAL_BASE_URL: "http://127.0.0.1:4410",
      OPENKEYS_CONTROL_KEY: "o".repeat(32),
    });
    expect(environment.OPENKEYS_INTERNAL_BASE_URL).toBe("http://127.0.0.1:4410");
    expect(environment.OPENKEYS_CONTROL_KEY).toBe("o".repeat(32));
    expect(() => validateEnvironment({
      ...requiredEnvironment,
      OPENKEYS_INTERNAL_BASE_URL: "http://openkeys.example.test",
    })).toThrow("HTTP OPENKEYS_INTERNAL_BASE_URL is allowed only for loopback hosts");
    expect(() => validateEnvironment({
      ...requiredEnvironment,
      OPENKEYS_CONTROL_KEY: "too-short",
    })).toThrow();
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

  it("accepts a temporary previous admin key only when it meets the key contract", () => {
    expect(validateEnvironment({
      ...requiredEnvironment,
      COMMERCIAL_ADMIN_KEY: "a".repeat(32),
      COMMERCIAL_ADMIN_PREVIOUS_KEY: "b".repeat(32),
    }).COMMERCIAL_ADMIN_PREVIOUS_KEY).toBe("b".repeat(32));
    expect(() => validateEnvironment({
      ...requiredEnvironment,
      COMMERCIAL_ADMIN_PREVIOUS_KEY: "too-short",
    })).toThrow();
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
      CONTENT_STUDIO_ENGINE_KEY: `sk-pool-${"x".repeat(32)}`,
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

  it("requires a dedicated Content Studio AI key in production", () => {
    expect(() => validateEnvironment({ ...requiredEnvironment, NODE_ENV: "production" }))
      .toThrow("CONTENT_STUDIO_ENGINE_KEY is required in production");
    expect(validateEnvironment({
      ...requiredEnvironment,
      NODE_ENV: "production",
      CONTENT_STUDIO_ENGINE_KEY: `sk-pool-${"x".repeat(32)}`,
    }).CONTENT_STUDIO_ENGINE_KEY).toMatch(/^sk-pool-/);
  });
});
