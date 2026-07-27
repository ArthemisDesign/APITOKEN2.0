import { describe, expect, it } from "vitest";
import { guardRequest, readJsonLimited } from "./request-guard";

function request(body = "{}", origin = "https://openkeys.apitoken.sale"): Request {
  return new Request("https://openkeys.apitoken.sale/api/test", {
    method: "POST",
    headers: { origin, "content-type": "application/json", "x-forwarded-for": "203.0.113.8" },
    body,
  });
}

describe("request guard", () => {
  it("rejects cross-origin mutations", () => {
    expect(guardRequest(request("{}", "https://attacker.example"), "cross-origin", 10, 60_000)?.status).toBe(403);
  });

  it("limits repeated requests from the effective proxy address", () => {
    expect(guardRequest(request(), "limited", 1, 60_000)).toBeNull();
    const rejected = guardRequest(request(), "limited", 1, 60_000);
    expect(rejected?.status).toBe(429);
    expect(rejected?.headers.get("retry-after")).toBeTruthy();
  });

  it("parses a small JSON body", async () => {
    await expect(readJsonLimited<{ ok: boolean }>(request('{"ok":true}'))).resolves.toEqual({ ok: true });
  });

  it("rejects an oversized JSON body", async () => {
    await expect(readJsonLimited(request(JSON.stringify({ value: "x".repeat(20_000) })))).rejects.toThrow(
      "payload_too_large",
    );
  });
});
