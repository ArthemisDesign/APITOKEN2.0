"use client";

import { useEffect, useMemo, useSyncExternalStore } from "react";
import { usePathname } from "next/navigation";
import { refreshMountedResources, RESOURCE_FALLBACK_INTERVAL_MS } from "@/lib/resources";
import { feedsForPath, type FeedPath } from "@/lib/feeds";
import { publishInvalidation, type InvalidationKind } from "@/lib/invalidation";

export { FEEDS, feedsForPath } from "@/lib/feeds";
export type { FeedPath } from "@/lib/feeds";

type FeedState = "connecting" | "live" | "recovering";

export type RealtimeSnapshot = {
  live: number;
  total: number;
  state: FeedState;
};

type ChangePayload = { resources?: unknown };
type Listener = () => void;

const listeners = new Set<Listener>();
const states = new Map<string, FeedState>();
let sources = new Map<string, EventSource>();
let consumers = 0;
let snapshot: RealtimeSnapshot = { live: 0, total: 0, state: "connecting" };

function rebuildSnapshot(): void {
  const total = states.size;
  const live = [...states.values()].filter((state) => state === "live").length;
  const recovering = [...states.values()].some((state) => state === "recovering");
  snapshot = {
    live,
    total,
    state: total > 0 && live === total ? "live" : recovering ? "recovering" : "connecting",
  };
  for (const listener of listeners) listener();
}

function setState(feed: string, state: FeedState): void {
  if (states.get(feed) === state) return;
  states.set(feed, state);
  rebuildSnapshot();
}

/** Heartbeat events never enter this parser and therefore can never trigger a request. */
export function applyRealtimePayload(raw: string, kind: InvalidationKind = "change"): boolean {
  let payload: ChangePayload;
  try {
    payload = JSON.parse(raw) as ChangePayload;
  } catch {
    return false;
  }
  if (!Array.isArray(payload.resources)) return false;
  const resources = payload.resources.filter(
    (value): value is string => typeof value === "string" && value.startsWith("/") && value.length <= 256,
  );
  if (!resources.length) return false;
  publishInvalidation(resources, kind);
  return true;
}

function openFeed(feed: FeedPath): void {
  if (sources.has(feed) || typeof EventSource === "undefined") return;
  states.set(feed, "connecting");
  let source: EventSource;
  try {
    source = new EventSource(feed);
  } catch {
    states.set(feed, "recovering");
    return;
  }
  source.onopen = () => setState(feed, "live");
  source.onerror = () => setState(feed, "recovering");
  source.addEventListener("change", (event: Event) => {
    if (event instanceof MessageEvent) applyRealtimePayload(String(event.data ?? ""), "change");
  });
  source.addEventListener("resync", (event: Event) => {
    if (event instanceof MessageEvent) applyRealtimePayload(String(event.data ?? ""), "resync");
  });
  sources.set(feed, source);
}

function closeFeed(feed: string): void {
  const source = sources.get(feed);
  if (source) {
    source.close();
    sources.delete(feed);
  }
  states.delete(feed);
}

function reconcile(wanted: readonly FeedPath[]): void {
  if (typeof EventSource === "undefined") return;
  const keep = new Set<string>(wanted);
  for (const feed of [...sources.keys()]) {
    if (!keep.has(feed)) closeFeed(feed);
  }
  for (const feed of wanted) openFeed(feed);
  rebuildSnapshot();
}

function stop(): void {
  for (const source of sources.values()) source.close();
  sources = new Map();
  states.clear();
  snapshot = { live: 0, total: 0, state: "connecting" };
  rebuildSnapshot();
}

export function RealtimeBridge(): null {
  const pathname = usePathname() ?? "/";
  const wanted = useMemo(() => feedsForPath(pathname), [pathname]);
  useEffect(() => {
    consumers += 1;
    return () => {
      consumers -= 1;
      if (consumers === 0) stop();
    };
  }, []);
  useEffect(() => {
    if (consumers === 0) return;
    reconcile(wanted);
  }, [wanted]);
  return null;
}

export function subscribeRealtime(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getRealtimeSnapshot(): RealtimeSnapshot {
  return snapshot;
}

const SERVER_REALTIME: RealtimeSnapshot = { live: 0, total: 0, state: "connecting" };

export function useRealtimeStatus(): RealtimeSnapshot {
  return useSyncExternalStore(subscribeRealtime, getRealtimeSnapshot, () => SERVER_REALTIME);
}

/**
 * SSE remains the fast path, while this bridge bounds staleness if an invalidation was lost or a
 * browser suspended the stream. The 30s interval runs only while opened feeds are not fully live.
 * Returning online or to a visible tab always refreshes immediately.
 */
export function ResourceFreshnessBridge(): null {
  useEffect(() => {
    const refreshWhenActive = () => {
      if (document.visibilityState !== "visible" || !navigator.onLine) return;
      refreshMountedResources();
    };
    const pollWhenRealtimeIsDown = () => {
      if (getRealtimeSnapshot().state === "live") return;
      refreshWhenActive();
    };
    const timer = window.setInterval(pollWhenRealtimeIsDown, RESOURCE_FALLBACK_INTERVAL_MS);
    document.addEventListener("visibilitychange", refreshWhenActive);
    window.addEventListener("online", refreshWhenActive);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", refreshWhenActive);
      window.removeEventListener("online", refreshWhenActive);
    };
  }, []);
  return null;
}
