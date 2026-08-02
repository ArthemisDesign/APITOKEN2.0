import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { loadOpenKeysPricingInventoryPageV2 } from "@/lib/pricing-inventory";
import { GET } from "./route";

vi.mock("@/lib/pricing-inventory", () => ({
  loadOpenKeysPricingInventoryPageV2: vi.fn(),
}));

const controlKey = "control-key-for-openkeys-pricing-inventory";
const page = {
  inventory_digest: `sha256:v2:${"1".repeat(64)}`,
  accounts: [{
    account_id: "acct_openkeys",
    source_id: "00000000-0000-4000-8000-000000000001",
    lifecycle: "active" as const,
    pricing_contract: "official_1_to_1" as const,
    source_multiplier_bp: 10_000,
    content_digest: `sha256:v2:${"2".repeat(64)}`,
  }],
  next_after_account_id: null,
};

function request(query = "", key = controlKey): Request {
  return new Request(`http://127.0.0.1:3410/api/internal/pricing/v2/inventory${query}`, {
    headers: { "x-openkeys-control-key": key },
  });
}

describe("OpenKeys pricing inventory route", () => {
  beforeAll(() => {
    process.env.OPENKEYS_DATABASE_URL = "postgresql://openkeys:test@127.0.0.1:5432/openkeys";
    process.env.ENGINE_CONTROL_KEY = controlKey;
    process.env.OPENKEYS_SESSION_SECRET = "s".repeat(32);
    process.env.OPENKEYS_ADMIN_USER = "admin";
    process.env.OPENKEYS_ADMIN_PASSWORD = "password";
  });

  beforeEach(() => {
    vi.mocked(loadOpenKeysPricingInventoryPageV2).mockReset();
    vi.mocked(loadOpenKeysPricingInventoryPageV2).mockResolvedValue(page);
  });

  it("returns 404 for a wrong machine credential", async () => {
    const response = await GET(request("", "wrong"));
    expect(response.status).toBe(404);
    expect(loadOpenKeysPricingInventoryPageV2).not.toHaveBeenCalled();
  });

  it("returns a no-store bounded page for the exact credential", async () => {
    const response = await GET(request("?after_account_id=acct_previous&limit=25"));
    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(await response.json()).toEqual({ inventory: page });
    expect(loadOpenKeysPricingInventoryPageV2).toHaveBeenCalledWith({
      afterAccountId: "acct_previous",
      limit: 25,
    });
  });

  it("rejects malformed cursors and limits before database access", async () => {
    expect((await GET(request("?limit=501"))).status).toBe(400);
    expect((await GET(request("?after_account_id=other"))).status).toBe(400);
    expect(loadOpenKeysPricingInventoryPageV2).not.toHaveBeenCalled();
  });
});
