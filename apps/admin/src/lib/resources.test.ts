import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  __resetResourcesForTests,
  getResource,
  getResourceSnapshot,
  getErrors,
  invalidateResources,
  resourceAvailability,
  resourceMatches,
  refreshMountedResources,
} from "./resources";
import { applyRealtimePayload } from "./realtime";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  __resetResourcesForTests();
});

describe("resource invalidation", () => {
  it("keeps last-good data available while a failed revalidation is reported separately", () => {
    expect(resourceAvailability({
      data: { version: 1 },
      error: new Error("refresh failed"),
      isLoading: false,
      isValidating: false,
    })).toBe("ready");
    expect(resourceAvailability({
      data: undefined,
      error: new Error("initial load failed"),
      isLoading: false,
      isValidating: false,
    })).toBe("error");
  });

  it("matches path boundaries while ignoring query strings", () => {
    expect(resourceMatches("/admin/users?limit=50", "/admin/users")).toBe(true);
    expect(resourceMatches("/admin/users/42", "/admin/users")).toBe(true);
    expect(resourceMatches("/admin/users-export", "/admin/users")).toBe(false);
  });

  it("rejects malformed event data and never treats heartbeat data as an invalidation", () => {
    expect(applyRealtimePayload("not-json")).toBe(false);
    expect(applyRealtimePayload(JSON.stringify({ source: "engine" }))).toBe(false);
    expect(applyRealtimePayload(JSON.stringify({ resources: ["relative", 7] }))).toBe(false);
  });

  it("accepts valid change and resync payload shapes", () => {
    expect(applyRealtimePayload(JSON.stringify({ resources: ["/overview", "/capacity"] }))).toBe(true);
  });

  it("does not fetch merely because an unmounted URL was invalidated", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    getResourceSnapshot("/overview");
    invalidateResources(["/overview"]);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("refreshes only resources with a live subscriber", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn(async () => new Response(JSON.stringify({ ok: true }))) as typeof fetch;
    try {
      getResourceSnapshot("/hidden");
      const mounted = getResource<unknown>("/mounted");
      const unsubscribe = mounted.subscribe(() => undefined);
      await flush();
      refreshMountedResources();
      await flush();

      expect(globalThis.fetch).toHaveBeenCalledTimes(2);
      expect(globalThis.fetch).toHaveBeenNthCalledWith(1, "/mounted", expect.any(Object));
      expect(globalThis.fetch).toHaveBeenNthCalledWith(2, "/mounted", expect.any(Object));
      unsubscribe();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("deduplicates an exact URL across concurrent subscribers", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn(async () => new Response(JSON.stringify({ value: 7 }))) as typeof fetch;
    try {
      const resource = getResource<{ value: number }>("/overview");
      const unsubscribeFirst = resource.subscribe(() => undefined);
      const unsubscribeSecond = resource.subscribe(() => undefined);
      await flush();

      expect(globalThis.fetch).toHaveBeenCalledTimes(1);
      expect(resource.getSnapshot()).toMatchObject({ data: { value: 7 }, isLoading: false });
      unsubscribeFirst();
      unsubscribeSecond();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("keeps a shared URL error active until its final subscriber leaves", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn(async () =>
      new Response(JSON.stringify({ message: "shared upstream unavailable" }), { status: 503 })) as typeof fetch;
    try {
      const resource = getResource<unknown>("/overview");
      const unsubscribeFirst = resource.subscribe(() => undefined);
      const unsubscribeSecond = resource.subscribe(() => undefined);
      await flush();

      expect(getErrors()).toHaveLength(1);
      unsubscribeFirst();
      expect(getErrors()).toHaveLength(1);
      unsubscribeSecond();
      expect(getErrors()).toHaveLength(0);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("delivers independently completed endpoints without a Promise.all barrier", async () => {
    const pending = new Map<string, (response: Response) => void>();
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn((url: string | URL | Request) => new Promise<Response>((resolve) => {
      pending.set(String(url), resolve);
    })) as typeof fetch;
    try {
      const fast = getResource<{ value: string }>("/fast");
      const slow = getResource<{ value: string }>("/slow");
      const unsubscribeFast = fast.subscribe(() => undefined);
      const unsubscribeSlow = slow.subscribe(() => undefined);

      pending.get("/fast")?.(new Response(JSON.stringify({ value: "ready" })));
      await flush();
      expect(fast.getSnapshot().data).toEqual({ value: "ready" });
      expect(slow.getSnapshot().data).toBeUndefined();
      expect(slow.getSnapshot().isLoading).toBe(true);

      pending.get("/slow")?.(new Response(JSON.stringify({ value: "later" })));
      await flush();
      unsubscribeFast();
      unsubscribeSlow();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("queues one follow-up when an invalidation arrives during a request", async () => {
    const resolvers: Array<(response: Response) => void> = [];
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn(() => new Promise<Response>((resolve) => resolvers.push(resolve))) as typeof fetch;
    try {
      const resource = getResource<{ version: number }>("/overview");
      const unsubscribe = resource.subscribe(() => undefined);
      invalidateResources(["/overview"]);
      expect(globalThis.fetch).toHaveBeenCalledTimes(1);

      resolvers[0]?.(new Response(JSON.stringify({ version: 1 })));
      await flush();
      expect(globalThis.fetch).toHaveBeenCalledTimes(2);
      resolvers[1]?.(new Response(JSON.stringify({ version: 2 })));
      await flush();
      expect(resource.getSnapshot().data).toEqual({ version: 2 });
      unsubscribe();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("keeps last-good data when a revalidation fails", async () => {
    const originalFetch = globalThis.fetch;
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-12T12:00:00Z"));
    globalThis.fetch = vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ version: 1 })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ message: "upstream unavailable" }), { status: 503 })) as typeof fetch;
    try {
      const resource = getResource<{ version: number }>("/overview");
      const unsubscribe = resource.subscribe(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      const lastSuccessfulAt = resource.lastSuccessfulAt;
      vi.setSystemTime(new Date("2026-08-12T12:05:00Z"));
      resource.refresh();
      await vi.advanceTimersByTimeAsync(0);

      expect(resource.getSnapshot()).toMatchObject({
        data: { version: 1 },
        isLoading: false,
        isValidating: false,
      });
      expect(resource.getSnapshot().error?.message).toBe("upstream unavailable");
      expect(resource.lastSuccessfulAt).toBe(lastSuccessfulAt);
      expect(getErrors()).toEqual([{ key: "/overview", message: "upstream unavailable", dismissed: false, hasData: true }]);
      unsubscribe();
    } finally {
      globalThis.fetch = originalFetch;
      vi.useRealTimers();
    }
  });

  it("aborts an orphaned URL request after its last subscriber unmounts", async () => {
    let aborted = false;
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn((_url: string | URL | Request, init?: RequestInit) => new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => {
        aborted = true;
        reject(new DOMException("Aborted", "AbortError"));
      });
    })) as typeof fetch;
    try {
      const resource = getResource<unknown>("/overview");
      const unsubscribe = resource.subscribe(() => undefined);
      unsubscribe();
      expect(aborted).toBe(true);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("does not carry a queued revalidation across an orphan abort", async () => {
    const requests: Array<{ resolve: (response: Response) => void; signal: AbortSignal | null }> = [];
    const originalFetch = globalThis.fetch;
    globalThis.fetch = vi.fn((_url: string | URL | Request, init?: RequestInit) => new Promise<Response>((resolve, reject) => {
      const signal = init?.signal ?? null;
      signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      requests.push({ resolve, signal });
    })) as typeof fetch;
    try {
      const resource = getResource<{ version: number }>("/overview");
      const unsubscribeFirst = resource.subscribe(() => undefined);
      invalidateResources(["/overview"]);
      unsubscribeFirst();

      expect(requests).toHaveLength(1);
      expect(requests[0]?.signal?.aborted).toBe(true);

      const unsubscribeSecond = resource.subscribe(() => undefined);
      expect(requests).toHaveLength(2);
      requests[1]?.resolve(new Response(JSON.stringify({ version: 2 })));
      await flush();

      expect(globalThis.fetch).toHaveBeenCalledTimes(2);
      expect(resource.getSnapshot().data).toEqual({ version: 2 });
      unsubscribeSecond();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
