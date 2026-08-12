"use client";

import { useEffect, useSyncExternalStore } from "react";
import { publishInvalidation } from "@/lib/invalidation";

const FEEDS = [
  "/admin/events",
  "/partner-admin/events",
  "/openkeys-admin/events",
  "/proxy-admin/events",
  "/events/engine",
  "/events/openai",
  "/events/gemini",
  "/events/kimi",
] as const;

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
let snapshot: RealtimeSnapshot = { live: 0, total: FEEDS.length, state: "connecting" };

function rebuildSnapshot(): void {
  const live = [...states.values()].filter((state) => state === "live").length;
  const recovering = [...states.values()].some((state) => state === "recovering");
  snapshot = {
    live,
    total: FEEDS.length,
    state: live === FEEDS.length ? "live" : recovering ? "recovering" : "connecting",
  };
  for (const listener of listeners) listener();
}

function setState(feed: string, state: FeedState): void {
  if (states.get(feed) === state) return;
  states.set(feed, state);
  rebuildSnapshot();
}

/** Heartbeat events never enter this parser and therefore can never trigger a request. */
export function applyRealtimePayload(raw: string): boolean {
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
  publishInvalidation(resources);
  return true;
}

function start(): void {
  if (sources.size || typeof EventSource === "undefined") return;
  for (const feed of FEEDS) {
    states.set(feed, "connecting");
    let source: EventSource;
    try {
      source = new EventSource(feed);
    } catch {
      states.set(feed, "recovering");
      continue;
    }
    source.onopen = () => setState(feed, "live");
    source.onerror = () => setState(feed, "recovering");
    const apply = (event: Event) => {
      if (event instanceof MessageEvent) applyRealtimePayload(String(event.data ?? ""));
    };
    source.addEventListener("change", apply);
    source.addEventListener("resync", apply);
    sources.set(feed, source);
  }
  rebuildSnapshot();
}

function stop(): void {
  for (const source of sources.values()) source.close();
  sources = new Map();
  states.clear();
  snapshot = { live: 0, total: FEEDS.length, state: "connecting" };
  rebuildSnapshot();
}

export function RealtimeBridge(): null {
  useEffect(() => {
    consumers += 1;
    start();
    return () => {
      consumers -= 1;
      if (consumers === 0) stop();
    };
  }, []);
  return null;
}

export function subscribeRealtime(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getRealtimeSnapshot(): RealtimeSnapshot {
  return snapshot;
}

const SERVER_REALTIME: RealtimeSnapshot = { live: 0, total: FEEDS.length, state: "connecting" };

export function useRealtimeStatus(): RealtimeSnapshot {
  return useSyncExternalStore(subscribeRealtime, getRealtimeSnapshot, () => SERVER_REALTIME);
}
