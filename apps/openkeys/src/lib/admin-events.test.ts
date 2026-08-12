import { describe, expect, it, vi } from "vitest";
import { OpenkeysAdminChangeFeed, openkeysChangeForTable } from "./admin-events";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function listenerClient() {
  return {
    query: vi.fn(async () => undefined),
    on: vi.fn(),
    removeAllListeners: vi.fn(),
    release: vi.fn(),
  };
}

describe("openkeysChangeForTable", () => {
  it("invalidates each key projection after an individual key write", () => {
    expect(openkeysChangeForTable("openkeys_keys")).toEqual({
      source: "openkeys",
      table: "openkeys_keys",
      resources: [
        "/openkeys-admin/keys",
        "/openkeys-admin/sellers",
        "/openkeys-admin/paying-keys",
        "/openkeys-admin/lookup",
      ],
    });
  });

  it("falls back to an owner-wide resync for a future allowlisted table", () => {
    const event = openkeysChangeForTable("future_table");
    expect(event.resources).toContain("/openkeys-admin/keys");
    expect(event.resources).not.toContain("/admin/users");
  });

  it("single-flights LISTEN and emits the initial resync only after it is ready", async () => {
    const acquired = deferred<ReturnType<typeof listenerClient>>();
    const acquire = vi.fn(() => acquired.promise);
    const feed = new OpenkeysAdminChangeFeed(acquire as never);
    const first = vi.fn();
    const second = vi.fn();

    const unsubscribeFirst = feed.subscribe(first);
    const unsubscribeSecond = feed.subscribe(second);
    expect(acquire).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
    expect(second).not.toHaveBeenCalled();

    const listener = listenerClient();
    acquired.resolve(listener);
    await vi.waitFor(() => expect(first).toHaveBeenCalledTimes(1));

    expect(second).toHaveBeenCalledTimes(1);
    expect(listener.query).toHaveBeenCalledWith("LISTEN openkeys_admin_changes");
    unsubscribeFirst();
    unsubscribeSecond();
    expect(listener.release).not.toHaveBeenCalled();
  });
});
