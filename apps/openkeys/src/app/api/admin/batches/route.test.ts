import { beforeEach, describe, expect, it, vi } from "vitest";
import { EngineClientError } from "@claude-api/engine-client";
import { getDatabase } from "@/lib/db";
import { getEngineClient } from "@/lib/engine";
import { listBatches } from "@/lib/keys";
import { currentAdmin } from "@/lib/session";
import { GET } from "./route";

vi.mock("@/lib/session", () => ({ currentAdmin: vi.fn() }));
vi.mock("@/lib/db", () => ({ getDatabase: vi.fn() }));
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

const validDatabaseContract = [
  { kind: "column", name: "openkeys_batches.pricing_contract", definition: "text NOT NULL" },
  { kind: "column", name: "openkeys_keys.pricing_contract", definition: "text NOT NULL" },
  { kind: "constraint", name: "openkeys_batches_pricing_contract", definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))" },
  { kind: "constraint", name: "openkeys_batches_official_1_to_1", definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)" },
  { kind: "constraint", name: "openkeys_keys_pricing_contract", definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))" },
  { kind: "constraint", name: "openkeys_keys_official_1_to_1", definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)" },
  { kind: "constraint", name: "openkeys_keys_batch_contract_fk", definition: "f:FOREIGN KEY (batch_id, pricing_contract) REFERENCES openkeys_batches(id, pricing_contract) ON DELETE RESTRICT" },
];

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
    vi.mocked(getDatabase).mockReset();
    vi.mocked(getDatabase).mockReturnValue({
      pool: { query: vi.fn().mockResolvedValue({ rows: validDatabaseContract }) },
    } as never);
    vi.mocked(getEngineClient).mockReset();
    vi.mocked(getEngineClient).mockReturnValue(engine({
      getSpendStats: vi.fn().mockResolvedValue({}),
    }) as never);
  });

  it("возвращает 401 без сессии", async () => {
    vi.mocked(currentAdmin).mockResolvedValue(null);
    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(401);
    expect(listBatches).not.toHaveBeenCalled();
  });

  it("marks issuance ready only after the DB contract and authenticated engine read pass", async () => {
    const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      issuanceAuthority: { ready: true },
    });
    expect(vi.mocked(getEngineClient)().getSpendStats).toHaveBeenCalledOnce();
  });

  it("keeps inventory readable but blocks issuance when the control key or engine read fails", async () => {
    vi.mocked(getEngineClient).mockReturnValue(engine({
      getSpendStats: vi.fn().mockRejectedValue(
        new EngineClientError("engine request failed", 401, false),
      ),
    }) as never);
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
      expect(response.status).toBe(200);
      await expect(response.json()).resolves.toMatchObject({
        batches: [],
        issuanceAuthority: {
          ready: false,
          reason: { code: "engine_unavailable" },
        },
      });
    } finally {
      errorLog.mockRestore();
    }
  });

  it("blocks issuance when the live PostgreSQL constraint shape differs", async () => {
    vi.mocked(getDatabase).mockReturnValue({
      pool: { query: vi.fn().mockResolvedValue({ rows: validDatabaseContract.slice(1) }) },
    } as never);
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      const response = await GET(new Request("http://127.0.0.1:3410/api/admin/batches"));
      expect(response.status).toBe(200);
      await expect(response.json()).resolves.toMatchObject({
        issuanceAuthority: {
          ready: false,
          reason: { code: "pricing_database_contract_mismatch" },
        },
      });
    } finally {
      errorLog.mockRestore();
    }
  });


});
