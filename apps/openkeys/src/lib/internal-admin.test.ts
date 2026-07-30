import { describe, expect, it } from "vitest";
import { internalAdminActor } from "./internal-admin";

function request(headers: Record<string, string>): Request {
  const normalized = new Map(Object.entries(headers).map(([name, value]) => [name.toLowerCase(), value]));
  return {
    headers: { get: (name: string) => normalized.get(name.toLowerCase()) ?? null },
  } as unknown as Request;
}

describe("internal OpenKeys admin authentication", () => {
  it("requires both the server credential and verified actor", () => {
    expect(internalAdminActor(request({
      "x-openkeys-control-key": "control-secret",
      "x-admin-actor": "operator",
    }), "control-secret")).toBe("operator");
    expect(internalAdminActor(request({ "x-admin-actor": "operator" }), "control-secret")).toBeNull();
    expect(internalAdminActor(request({ "x-openkeys-control-key": "control-secret" }), "control-secret")).toBeNull();
  });

  it("rejects a wrong credential and control characters in identity", () => {
    expect(internalAdminActor(request({
      "x-openkeys-control-key": "wrong",
      "x-admin-actor": "operator",
    }), "control-secret")).toBeNull();
    expect(internalAdminActor(request({
      "x-openkeys-control-key": "control-secret",
      "x-admin-actor": "operator\nspoof",
    }), "control-secret")).toBeNull();
  });
});
