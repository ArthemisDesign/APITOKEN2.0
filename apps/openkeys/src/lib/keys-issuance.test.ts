import { describe, expect, it, vi } from "vitest";

vi.mock("./db", () => ({
  getDatabase: vi.fn(() => {
    throw new Error("database must not be reached for a rejected pricing override");
  }),
}));
vi.mock("./engine", () => ({
  getEngineClient: vi.fn(() => {
    throw new Error("engine must not be reached for a rejected pricing override");
  }),
}));
vi.mock("./config", () => ({
  loadConfig: vi.fn(() => {
    throw new Error("config must not be reached for a rejected pricing override");
  }),
}));

import { issueBatch, type IssueBatchInput } from "./keys";

const validInput: IssueBatchInput = {
  faceValueNano: 50_000_000_000n,
  quantity: 1,
  label: "direct-service-test",
  note: null,
  apiType: "anthropic",
  createdBy: "test-admin",
};

describe("OpenKeys service issuance boundary", () => {
  it("rejects a direct service call carrying a legacy multiplier before any I/O", async () => {
    const bypass = { ...validInput, multBp: 4_000 };
    await expect(issueBatch(bypass)).rejects.toMatchObject({
      code: "pricing_override_forbidden",
    });
  });

  it("rejects a direct service call trying to select the legacy contract", async () => {
    const bypass = { ...validInput, pricingContract: "legacy" };
    await expect(issueBatch(bypass)).rejects.toMatchObject({
      code: "pricing_override_forbidden",
    });
  });
});
