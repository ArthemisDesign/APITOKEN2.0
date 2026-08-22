import { describe, expect, it, vi } from "vitest";
import { AdminEventsService, salesChangeForTable } from "./admin-events.service.js";

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

describe("salesChangeForTable", () => {
  it("maps payout changes without refreshing unrelated partner applications", () => {
    expect(salesChangeForTable("payout_batches")).toEqual({
      source: "sales",
      table: "payout_batches",
      resources: ["/partner-admin/payouts", "/partner-admin/payouts/batches", "/partner-admin/payouts/engine"],
    });
  });

  it("invalidates only the unified request queue for request/effect transitions", () => {
    expect(salesChangeForTable("partner_requests").resources).toEqual(["/partner-admin/requests"]);
    expect(salesChangeForTable("partner_request_effects").resources).toEqual(["/partner-admin/requests"]);
  });

  it("keeps an unknown future payload inside the sales owner boundary", () => {
    const event = salesChangeForTable("future_table");
    expect(event.resources).toContain("/partner-admin/overview");
    expect(event.resources).not.toContain("/admin/users");
  });

  it("single-flights startup and owns one listener through shutdown", async () => {
    const acquired = deferred<ReturnType<typeof listenerClient>>();
    const connect = vi.fn(() => acquired.promise);
    const service = new AdminEventsService({ pool: { connect } } as never);

    const first = service.onModuleInit();
    const second = service.onModuleInit();
    expect(connect).toHaveBeenCalledTimes(1);

    const listener = listenerClient();
    acquired.resolve(listener);
    await Promise.all([first, second]);

    expect(listener.query).toHaveBeenCalledWith("LISTEN sales_admin_changes");
    await service.onApplicationShutdown();
    expect(listener.query).toHaveBeenCalledWith("UNLISTEN sales_admin_changes");
    expect(listener.release).toHaveBeenCalledTimes(1);
  });
});
