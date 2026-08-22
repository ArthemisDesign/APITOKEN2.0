import { beforeEach, describe, expect, it, vi } from "vitest";
import { getDatabase } from "@/lib/db";
import { getEngineClient } from "@/lib/engine";
import { loadConfig } from "@/lib/config";
import { assertSecretBoxReady } from "@/lib/secret-box";
import { GET } from "./route";

vi.mock("@/lib/config", () => ({ loadConfig: vi.fn() }));
vi.mock("@/lib/db", () => ({ getDatabase: vi.fn() }));
vi.mock("@/lib/engine", () => ({ getEngineClient: vi.fn() }));
vi.mock("@/lib/secret-box", () => ({ assertSecretBoxReady: vi.fn() }));

const validDatabaseContract = [
  { kind: "column", name: "openkeys_batches.pricing_contract", definition: "text NOT NULL" },
  { kind: "column", name: "openkeys_keys.pricing_contract", definition: "text NOT NULL" },
  { kind: "constraint", name: "openkeys_batches_pricing_contract", definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))" },
  { kind: "constraint", name: "openkeys_batches_official_1_to_1", definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)" },
  { kind: "constraint", name: "openkeys_keys_pricing_contract", definition: "c:CHECK (pricing_contract = ANY (ARRAY['legacy'::text, 'official_1_to_1'::text]))" },
  { kind: "constraint", name: "openkeys_keys_official_1_to_1", definition: "c:CHECK (pricing_contract <> 'official_1_to_1'::text OR mult_bp = 10000)" },
  { kind: "constraint", name: "openkeys_keys_batch_contract_fk", definition: "f:FOREIGN KEY (batch_id, pricing_contract) REFERENCES openkeys_batches(id, pricing_contract) ON DELETE RESTRICT" },
];

function database(contractRows = validDatabaseContract) {
  return {
    pool: {
      query: vi.fn((statement: string) => {
        if (statement === "SELECT 1") return Promise.resolve({ rowCount: 1, rows: [{}] });
        if (statement.includes("SELECT j.id, j.batch_id")) {
          return Promise.resolve({ rowCount: 0, rows: [] });
        }
        return Promise.resolve({ rowCount: contractRows.length, rows: contractRows });
      }),
    },
  };
}

describe("OpenKeys readiness", () => {
  beforeEach(() => {
    vi.mocked(loadConfig).mockReset();
    vi.mocked(loadConfig).mockReturnValue({} as never);
    vi.mocked(assertSecretBoxReady).mockReset();
    vi.mocked(getDatabase).mockReset();
    vi.mocked(getDatabase).mockReturnValue(database() as never);
    vi.mocked(getEngineClient).mockReset();
    vi.mocked(getEngineClient).mockReturnValue({
      getSpendStats: vi.fn().mockResolvedValue({}),
    } as never);
  });

  it("requires config, secret storage, and the exact DB contract without a Control API call", async () => {
    const response = await GET();
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ status: "ready" });
    expect(assertSecretBoxReady).toHaveBeenCalledOnce();
    expect(getEngineClient).not.toHaveBeenCalled();
  });

  it("returns 200 when the engine client throws", async () => {
    vi.mocked(getEngineClient).mockImplementation(() => {
      throw new Error("control key rejected");
    });
    const response = await GET();
    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ status: "ready" });
    expect(getEngineClient).not.toHaveBeenCalled();
  });

  it("returns an opaque 503 when a pricing constraint is absent", async () => {
    vi.mocked(getDatabase).mockReturnValue(database(validDatabaseContract.slice(1)) as never);
    const response = await GET();
    expect(response.status).toBe(503);
    await expect(response.json()).resolves.toEqual({ status: "unavailable" });
  });
});
