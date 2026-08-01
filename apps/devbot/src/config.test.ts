import { describe, expect, it } from "vitest";
import { parseConfig } from "./config.js";

function minimalEnv(): Record<string, string> {
  return {
    DEVBOT_TELEGRAM_TOKEN: "123456:token",
    DEVBOT_CHAT_ID: "-100999",
    DEVBOT_ADMIN_IDS: "42, 43",
    DEVBOT_TOPIC_CRITICAL: "1",
    DEVBOT_TOPIC_DEPLOYS: "2",
    DEVBOT_TOPIC_WARNINGS: "3",
    DEVBOT_TOPIC_COMMERCE: "4",
    DEVBOT_TOPIC_CI: "5",
    DEVBOT_TOPIC_DIGEST: "6",
    DEVBOT_AM_SECRET: "0123456789abcdef",
  };
}

describe("parseConfig", () => {
  it("parses a minimal env with defaults applied", () => {
    const config = parseConfig(minimalEnv());
    expect(config.chatId).toBe(-100999);
    expect(config.adminIds.has(42)).toBe(true);
    expect(config.adminIds.has(43)).toBe(true);
    expect(config.topics).toEqual({ critical: 1, deploys: 2, warnings: 3, commerce: 4, ci: 5, digest: 6 });
    expect(config.port).toBe(3800);
    expect(config.githubRepo).toBe("3xcalibur-tech/Claude_API");
    expect(config.pollGithubMs).toBe(45_000);
    expect(config.timeZone).toBe("Asia/Tbilisi");
    expect(config.alertmanagerUrl).toBe("http://127.0.0.1:9093");
    expect(config.journaldEnabled).toBe(false);
    expect(config.logLevel).toBe("info");
    expect(config.githubToken).toBeUndefined();
    expect(config.engineReadonlyKey).toBeUndefined();
  });

  it("treats empty optional secrets as absent", () => {
    const config = parseConfig({ ...minimalEnv(), DEVBOT_GITHUB_TOKEN: "", DEVBOT_ENGINE_CONTROL_KEY: "x".repeat(20) });
    expect(config.githubToken).toBeUndefined();
    expect(config.engineControlKey).toBe("x".repeat(20));
  });

  it("fails fast without the required keys", () => {
    expect(() => parseConfig({})).toThrow();
    const { DEVBOT_AM_SECRET: _dropped, ...withoutSecret } = minimalEnv();
    expect(() => parseConfig(withoutSecret)).toThrow();
  });

  it("validates the admin id list shape", () => {
    expect(() => parseConfig({ ...minimalEnv(), DEVBOT_ADMIN_IDS: "not-a-number" })).toThrow();
  });

  it("accepts only valid IANA time zones", () => {
    expect(parseConfig({ ...minimalEnv(), DEVBOT_TIME_ZONE: "Europe/Berlin" }).timeZone).toBe("Europe/Berlin");
    expect(() => parseConfig({ ...minimalEnv(), DEVBOT_TIME_ZONE: "Tbilisi-ish" })).toThrow();
  });
});
