import { describe, expect, it, vi } from "vitest";
import { AdminEventsService, commerceChangeForTable } from "./admin-events.service.js";

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

describe("commerceChangeForTable", () => {
  it("invalidates only user-facing commerce projections for a user write", () => {
    expect(commerceChangeForTable("users")).toEqual({
      source: "commerce",
      table: "users",
      resources: ["/admin/users", "/admin/dashboard", "/admin/finance"],
    });
  });

  it("fails safe with a bounded owner-wide resync for an unknown allowlisted payload", () => {
    const event = commerceChangeForTable("future_table");
    expect(event.resources).toContain("/admin/users");
    expect(event.resources).toContain("/admin/admin-accounts");
    expect(event.resources).not.toContain("/partner-admin/overview");
  });

  it("single-flights startup and waits until LISTEN is active", async () => {
    const acquired = deferred<ReturnType<typeof listenerClient>>();
    const connect = vi.fn(() => acquired.promise);
    const service = new AdminEventsService({ pool: { connect } } as never);

    const first = service.onModuleInit();
    const second = service.onModuleInit();
    expect(connect).toHaveBeenCalledTimes(1);

    const listener = listenerClient();
    acquired.resolve(listener);
    await Promise.all([first, second]);

    expect(listener.query).toHaveBeenCalledWith("LISTEN commerce_admin_changes");
    await service.onApplicationShutdown();
    expect(listener.query).toHaveBeenCalledWith("UNLISTEN commerce_admin_changes");
    expect(listener.release).toHaveBeenCalledTimes(1);
  });
});
