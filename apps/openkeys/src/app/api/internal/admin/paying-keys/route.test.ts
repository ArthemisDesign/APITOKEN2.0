import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { loadPayingKeys, type PayingKeysPage } from "@/lib/keys";
import { GET } from "./route";

vi.mock("@/lib/keys", () => ({ loadPayingKeys: vi.fn() }));

const controlKey = "openkeys-paying-keys-control";
const payload: PayingKeysPage = {
  days: 30,
  total: 0,
  limit: 50,
  offset: 0,
  sort: "spent",
  dir: "desc",
  rows: [],
};

function request(query = "", key = controlKey, actor = "operator"): Request {
  return new Request(`http://127.0.0.1:3410/api/internal/admin/paying-keys${query}`, {
    headers: {
      "x-openkeys-control-key": key,
      "x-admin-actor": actor,
    },
  });
}

describe("OpenKeys paying keys route", () => {
  beforeAll(() => {
    process.env.OPENKEYS_DATABASE_URL = "postgresql://openkeys:test@127.0.0.1:5432/openkeys";
    process.env.ENGINE_CONTROL_KEY = controlKey;
    process.env.OPENKEYS_SESSION_SECRET = "s".repeat(32);
    process.env.OPENKEYS_ADMIN_USER = "admin";
    process.env.OPENKEYS_ADMIN_PASSWORD = "password";
  });

  beforeEach(() => {
    vi.mocked(loadPayingKeys).mockReset().mockResolvedValue(payload);
  });

  it("hides the endpoint behind the exact credential and verified actor", async () => {
    expect((await GET(request("", "wrong"))).status).toBe(404);
    expect((await GET(request("", controlKey, ""))).status).toBe(404);
    expect(loadPayingKeys).not.toHaveBeenCalled();
  });

  it("returns a no-store page and delegates normalized filters", async () => {
    const response = await GET(request("?days=7&limit=25&offset=50&q=%20masked%20&status=disabled&sort=nominal&dir=asc"));

    expect(response.status).toBe(200);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(await response.json()).toEqual(payload);
    expect(loadPayingKeys).toHaveBeenCalledWith({
      days: 7,
      limit: 25,
      offset: 50,
      q: "masked",
      status: "disabled",
      sort: "nominal",
      dir: "asc",
    });
  });

  it("uses documented defaults", async () => {
    await GET(request());
    expect(loadPayingKeys).toHaveBeenCalledWith({
      days: 30,
      limit: 50,
      offset: 0,
      q: "",
      status: "all",
      sort: "spent",
      dir: "desc",
    });
  });

  it.each([
    "?days=2",
    "?limit=0",
    "?limit=101",
    "?offset=-1",
    "?offset=100001",
    `?q=${"x".repeat(81)}`,
    "?status=removed",
    "?sort=requests",
    "?dir=sideways",
  ])("rejects invalid query %s before delegation", async (query) => {
    const response = await GET(request(query));
    expect(response.status).toBe(400);
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(loadPayingKeys).not.toHaveBeenCalled();
  });
});
