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
    getPricingReleaseProvisioningContextV2: vi.fn(async () => null),
    getActivePricingCatalog: vi.fn(async () => null),
    getActiveProviderSwitches: vi.fn(async () => null),
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

  it("при подтверждённом release context отвечает ready без чтения legacy authority", async () => {
    const releaseContext = { head: { active_generation: 10 } };
    const client = engine({
      getPricingReleaseProvisioningContextV2: vi.fn(async () => releaseContext),
    });
    vi.mocked(getEngineClient).mockReturnValue(client as never);

    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(200);
    const payload = await response.json();
    expect(payload.issuanceAuthority.ready).toBe(true);
    expect(payload.issuanceAuthority.supportedModels.length).toBeGreaterThan(0);
    expect(client.getActivePricingCatalog).not.toHaveBeenCalled();
  });

  it("неподтверждённый authority не прячет склад и возвращает reason с кодом ошибки", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(getEngineClient).mockReturnValue(engine({}) as never);

    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(200);
    const payload = await response.json();
    expect(payload.totals).toEqual(batches.totals);
    expect(payload.issuanceAuthority.ready).toBe(false);
    expect(payload.issuanceAuthority.reason.code).toBe("pricing_authority_missing");
    expect(spy).toHaveBeenCalledWith("openkeys issuance authority check failed", expect.objectContaining({
      code: "pricing_authority_missing",
    }));
    spy.mockRestore();
  });

  it("недоступный движок отличается от неподтверждённого authority", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(getEngineClient).mockReturnValue(engine({
      getPricingReleaseProvisioningContextV2: vi.fn(async () => {
        throw new EngineClientError("engine request failed", undefined, true);
      }),
    }) as never);

    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(200);
    const payload = await response.json();
    expect(payload.issuanceAuthority.ready).toBe(false);
    expect(payload.issuanceAuthority.reason.code).toBe("engine_unavailable");
    expect(spy).toHaveBeenCalledWith("openkeys issuance authority check failed", expect.objectContaining({
      code: "engine_unavailable",
      error: "EngineClientError",
    }));
    spy.mockRestore();
  });
});
