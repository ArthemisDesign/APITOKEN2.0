import { afterEach, describe, expect, it } from "vitest";

const environment = {
  DATABASE_URL: "postgresql://commerce:password@127.0.0.1:1/commerce",
  ENGINE_BASE_URL: "http://127.0.0.1:1",
  ENGINE_CONTROL_KEY: "e".repeat(32),
  AUTH_TOKEN_ENCRYPTION_KEY: Buffer.alloc(32, 7).toString("base64url"),
  COMMERCIAL_ADMIN_KEY: "a".repeat(32),
} as const;
const previous = new Map<string, string | undefined>();

afterEach(() => {
  for (const [name, value] of previous) {
    if (value === undefined) delete process.env[name];
    else process.env[name] = value;
  }
  previous.clear();
});

describe("commercial API bootstrap", () => {
  it("initializes once with Nest's form parser and managed login routes", async () => {
    for (const [name, value] of Object.entries(environment)) {
      previous.set(name, process.env[name]);
      process.env[name] = value;
    }
    const { createApplication } = await import("./application.js");
    const app = await createApplication();
    try {
      await expect(app.init()).resolves.toBe(app);
      const fastify = app.getHttpAdapter().getInstance();
      expect(fastify.hasContentTypeParser(
        "application/x-www-form-urlencoded",
      )).toBe(true);
      expect(fastify.hasRoute({
        method: "GET",
        url: "/v1/internal/admin-auth/browser/login",
      })).toBe(true);
    } finally {
      await app.close();
    }
  });
});
