import { beforeAll, describe, expect, it, vi } from "vitest";
import type { OpenkeysAdminChangeEvent } from "@/lib/admin-events";
import { GET } from "./route";

const feed = vi.hoisted(() => ({
  unsubscribe: vi.fn(),
  subscribe: vi.fn(),
}));

vi.mock("@/lib/admin-events", () => ({
  getOpenkeysAdminChangeFeed: () => ({ subscribe: feed.subscribe }),
}));

const controlKey = "openkeys-events-control";

function request(key = controlKey, actor = "operator"): Request {
  return new Request("http://127.0.0.1:3410/api/internal/admin/events", {
    headers: {
      "x-openkeys-control-key": key,
      "x-admin-actor": actor,
    },
  });
}

describe("OpenKeys admin event route", () => {
  beforeAll(() => {
    process.env.OPENKEYS_DATABASE_URL = "postgresql://openkeys:test@127.0.0.1:5432/openkeys";
    process.env.ENGINE_CONTROL_KEY = controlKey;
    process.env.OPENKEYS_SESSION_SECRET = "s".repeat(32);
    process.env.OPENKEYS_ADMIN_USER = "admin";
    process.env.OPENKEYS_ADMIN_PASSWORD = "password";

    feed.subscribe.mockImplementation(
      (subscriber: (event: OpenkeysAdminChangeEvent) => void) => {
        subscriber({
          source: "openkeys",
          resources: ["/openkeys-admin/keys"],
          resync: true,
        });
        return feed.unsubscribe;
      },
    );
  });

  it("hides the stream behind the exact credential and verified actor", async () => {
    expect((await GET(request("wrong"))).status).toBe(404);
    expect((await GET(request(controlKey, ""))).status).toBe(404);
    expect(feed.subscribe).not.toHaveBeenCalled();
  });

  it("sends eager resync without waiting for heartbeat or EOF", async () => {
    const response = await GET(request());
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("text/event-stream; charset=utf-8");
    expect(response.headers.get("cache-control")).toBe("no-cache, no-transform");

    const reader = response.body?.getReader();
    expect(reader).toBeDefined();
    const first = await reader!.read();
    expect(first.done).toBe(false);
    const text = new TextDecoder().decode(first.value);
    expect(text).toContain("event: resync");
    expect(text).toContain('"source":"openkeys"');
    expect(text).toContain('"resync":true');

    await reader!.cancel();
    expect(feed.unsubscribe).toHaveBeenCalledTimes(1);
  });
});
