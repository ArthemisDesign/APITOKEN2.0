import { beforeEach, describe, expect, it, vi } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import { getEngineClient } from "@/lib/engine";
import { listBatches } from "@/lib/keys";
import { currentAdmin } from "@/lib/session";
import { GET } from "./route";

vi.mock("@/lib/session", () => ({ currentAdmin: vi.fn() }));
vi.mock("@/lib/engine", () => ({ getEngineClient: vi.fn() }));
vi.mock("@/lib/keys", () => ({
  MAX_BATCH_QUANTITY: 100,
  listBatches: vi.fn(),
  issueBatch: vi.fn(),
  BatchIssuanceError: class BatchIssuanceError extends Error {},
}));

const batches = {
  batches: [],
  total: 0,
  limit: 20,
  offset: 0,
  totals: { stock: 0, delivered: 0, disabled: 0 },
};

function engine(overrides: Record<string, unknown>) {
  return {
    ...overrides,
  };
}

describe("OpenKeys admin batches route", () => {
  beforeEach(() => {
    vi.mocked(currentAdmin).mockReset();
    vi.mocked(currentAdmin).mockResolvedValue("admin");
    vi.mocked(listBatches).mockReset();
    vi.mocked(listBatches).mockResolvedValue(batches);
    vi.mocked(getEngineClient).mockReset();
  });

  it("возвращает 401 без сессии", async () => {
    vi.mocked(currentAdmin).mockResolvedValue(null);
    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(401);
    expect(listBatches).not.toHaveBeenCalled();
  });



});
