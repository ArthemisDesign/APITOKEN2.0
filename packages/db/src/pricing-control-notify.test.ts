import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";
import {
  PRICING_CONTROL_JOBS_CHANNEL,
  PricingControlNotifyListener,
  type PricingControlNotifyClient,
} from "./pricing-control-notify.js";

class FakeClient extends EventEmitter implements PricingControlNotifyClient {
  connected = false;
  listened = false;
  ended = false;
  failConnect = false;

  async connect(): Promise<void> {
    if (this.failConnect) throw new Error("connection refused");
    this.connected = true;
  }

  async query(text: string): Promise<unknown> {
    if (text === `LISTEN ${PRICING_CONTROL_JOBS_CHANNEL}`) this.listened = true;
    return {};
  }

  removeAllListeners(): this {
    return super.removeAllListeners();
  }

  async end(): Promise<unknown> {
    this.ended = true;
    // A real pg Client emits "end" when its connection closes, including a local end().
    queueMicrotask(() => this.emit("end"));
    return undefined;
  }
}

const flushMicrotasks = async (rounds = 20): Promise<void> => {
  for (let index = 0; index < rounds; index += 1) await Promise.resolve();
};

describe("PricingControlNotifyListener", () => {
  it("forwards channel notifications to onWake with the table payload", async () => {
    const client = new FakeClient();
    const onWake = vi.fn();
    const listener = new PricingControlNotifyListener("postgresql://unused", {
      onWake,
      clientFactory: () => client,
    });

    listener.start();
    await vi.waitFor(() => expect(client.listened).toBe(true));

    client.emit("notification", { channel: PRICING_CONTROL_JOBS_CHANNEL, payload: "engine_policy_jobs" });
    client.emit("notification", { channel: "some_other_channel", payload: "ignored" });
    await flushMicrotasks();

    expect(onWake).toHaveBeenCalledTimes(1);
    expect(onWake).toHaveBeenCalledWith("engine_policy_jobs");

    await listener.stop();
    expect(client.ended).toBe(true);
  });

  it("reconnects with backoff after a connection failure and reports the error", async () => {
    const first = new FakeClient();
    first.failConnect = true;
    const second = new FakeClient();
    const clients = [first, second];
    const onError = vi.fn();
    const listener = new PricingControlNotifyListener("postgresql://unused", {
      onWake: () => undefined,
      onError,
      reconnectDelaysMs: [1, 1],
      clientFactory: () => clients.shift() ?? new FakeClient(),
    });

    listener.start();
    await vi.waitFor(() => expect(second.listened).toBe(true), { timeout: 2_000 });

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]![0].message).toBe("connection refused");
    await listener.stop();
  });

  it("reports a mid-listen drop, reconnects, and keeps waking", async () => {
    const first = new FakeClient();
    const second = new FakeClient();
    const clients = [first, second];
    const onWake = vi.fn();
    const onError = vi.fn();
    const listener = new PricingControlNotifyListener("postgresql://unused", {
      onWake,
      onError,
      reconnectDelaysMs: [1, 1],
      clientFactory: () => clients.shift() ?? new FakeClient(),
    });

    listener.start();
    await vi.waitFor(() => expect(first.listened).toBe(true));
    first.emit("error", new Error("server closed the connection"));
    await vi.waitFor(() => expect(second.listened).toBe(true), { timeout: 2_000 });

    second.emit("notification", { channel: PRICING_CONTROL_JOBS_CHANNEL, payload: "engine_switch_jobs" });
    await flushMicrotasks();

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onWake).toHaveBeenCalledWith("engine_switch_jobs");
    await listener.stop();
  });

  it("stops during backoff without reconnecting again", async () => {
    const first = new FakeClient();
    first.failConnect = true;
    let factoryCalls = 0;
    const listener = new PricingControlNotifyListener("postgresql://unused", {
      onWake: () => undefined,
      reconnectDelaysMs: [60_000],
      clientFactory: () => {
        factoryCalls += 1;
        return first;
      },
    });

    listener.start();
    await vi.waitFor(() => expect(factoryCalls).toBe(1));
    await listener.stop();
    await flushMicrotasks();

    expect(factoryCalls).toBe(1);
  });
});
