"use client";

import { useDeferredValue, useMemo, useSyncExternalStore } from "react";
import { api } from "@/lib/api";
import { resourceMatches, subscribeInvalidations } from "@/lib/invalidation";

export type ResourceSnapshot<T> = {
  data: T | undefined;
  error: Error | undefined;
  isLoading: boolean;
  isValidating: boolean;
};

export type ResourceError = {
  key: string;
  message: string;
  dismissed: boolean;
  hasData: boolean;
};

export type ResourceAvailability = "loading" | "ready" | "error";

export type ResourceGroupSnapshot<T extends object> = {
  data: { [K in keyof T]: T[K] | undefined };
  availability: { [K in keyof T]: ResourceAvailability };
  isLoading: boolean;
  isValidating: boolean;
  updatedAt: number;
};

type Listener = () => void;

const MAX_IDLE_RESOURCES = 250;

function abortError(cause: unknown): boolean {
  return cause instanceof Error && cause.name === "AbortError";
}

export function resourceAvailability<T>(snapshot: ResourceSnapshot<T>): ResourceAvailability {
  // A failed revalidation does not make the already rendered section unavailable: keep the
  // last-good payload visible while Error Center reports the refresh failure separately.
  if (snapshot.data !== undefined) return "ready";
  return snapshot.error ? "error" : "loading";
}

export { resourceMatches } from "@/lib/invalidation";

export class Resource<T> {
  private snapshot: ResourceSnapshot<T> = {
    data: undefined,
    error: undefined,
    isLoading: true,
    isValidating: false,
  };
  private readonly listeners = new Set<Listener>();
  private controller: AbortController | null = null;
  private inflight: Promise<void> | null = null;
  private stale = true;
  private queued = false;
  private generation = 0;
  lastUsedAt = Date.now();
  lastSuccessfulAt = 0;

  constructor(readonly key: string) {}

  subscribe = (listener: Listener): (() => void) => {
    const wasEmpty = this.listeners.size === 0;
    this.listeners.add(listener);
    this.lastUsedAt = Date.now();
    if (wasEmpty) activateResourceError(this.key);
    if (this.stale || (this.snapshot.data === undefined && !this.snapshot.error)) void this.load();
    return () => {
      this.listeners.delete(listener);
      this.lastUsedAt = Date.now();
      if (this.listeners.size === 0) {
        deactivateResourceError(this.key);
        // A queued invalidation is relevant only to the subscribers that observed it.
        // The next mount already performs one fresh load because the resource remains stale.
        this.queued = false;
        // A request without a consumer cannot improve the visible UI. The stale flag makes the
        // next mount retry while the last-good snapshot, if any, remains cached.
        if (this.controller) {
          this.generation += 1;
          this.controller.abort();
          this.controller = null;
          this.inflight = null;
        }
      }
    };
  };

  getSnapshot = (): ResourceSnapshot<T> => this.snapshot;

  hasSubscribers(): boolean {
    return this.listeners.size > 0;
  }

  markStale(): void {
    this.stale = true;
    if (!this.hasSubscribers()) return;
    if (this.inflight) this.queued = true;
    else void this.load();
  }

  refresh = (): void => {
    this.stale = true;
    if (!this.hasSubscribers()) return;
    if (this.inflight) this.queued = true;
    else void this.load();
  };

  private async load(): Promise<void> {
    if (this.inflight) return this.inflight;
    this.controller = new AbortController();
    const controller = this.controller;
    const generation = ++this.generation;
    this.snapshot = {
      ...this.snapshot,
      isLoading: this.snapshot.data === undefined,
      isValidating: this.snapshot.data !== undefined,
    };
    this.emit();
    const request = (async () => {
      try {
        const data = await api<T>(this.key, { signal: controller.signal });
        if (controller.signal.aborted || generation !== this.generation) return;
        this.stale = false;
        this.lastSuccessfulAt = Date.now();
        this.snapshot = { data, error: undefined, isLoading: false, isValidating: false };
        clearResourceError(this.key, this.hasSubscribers());
      } catch (cause) {
        // An aborted request may settle after this resource has already started a
        // newer generation. It must not overwrite that generation's loading state.
        if (generation !== this.generation) return;
        if (abortError(cause)) {
          this.stale = true;
          this.snapshot = {
            ...this.snapshot,
            isLoading: this.snapshot.data === undefined,
            isValidating: false,
          };
          return;
        }
        const error = cause instanceof Error ? cause : new Error(String(cause));
        this.stale = true;
        this.snapshot = { ...this.snapshot, error, isLoading: false, isValidating: false };
        if (this.hasSubscribers()) trackResourceError(this.key, error, this.snapshot.data !== undefined);
      } finally {
        if (this.controller === controller && generation === this.generation) this.controller = null;
      }
    })();
    this.inflight = request;
    try {
      await request;
    } finally {
      if (generation !== this.generation) return;
      if (this.inflight === request) this.inflight = null;
      this.lastUsedAt = Date.now();
      this.emit();
      pruneIdleResources();
      if (this.queued && this.hasSubscribers()) {
        this.queued = false;
        void this.load();
      }
    }
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}

// Exact URL is the sole identity: navigation and independent page sections share both last-good
// data and an in-flight request without coupling unrelated endpoints.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const registry = new Map<string, Resource<any>>();

export function getResource<T>(key: string): Resource<T> {
  let resource = registry.get(key) as Resource<T> | undefined;
  if (!resource) {
    resource = new Resource<T>(key);
    registry.set(key, resource);
    pruneIdleResources();
  }
  return resource;
}

function pruneIdleResources(): void {
  if (registry.size <= MAX_IDLE_RESOURCES) return;
  const idle = [...registry.values()]
    .filter((resource) => !resource.hasSubscribers())
    .sort((left, right) => left.lastUsedAt - right.lastUsedAt);
  for (const resource of idle.slice(0, registry.size - MAX_IDLE_RESOURCES)) {
    registry.delete(resource.key);
  }
}

const SERVER_SNAPSHOT: ResourceSnapshot<never> = {
  data: undefined,
  error: undefined,
  isLoading: true,
  isValidating: false,
};

export function useResource<T>(key: string): ResourceSnapshot<T> & { refresh: () => void } {
  const resource = useMemo(() => getResource<T>(key), [key]);
  const snapshot = useSyncExternalStore(
    resource.subscribe,
    resource.getSnapshot,
    () => SERVER_SNAPSHOT as ResourceSnapshot<T>,
  );
  const deferredData = useDeferredValue(snapshot.data);
  const data = snapshot.data ?? deferredData;
  return {
    ...snapshot,
    data,
    isLoading: snapshot.isLoading && data === undefined,
    isValidating: snapshot.isValidating || (snapshot.isLoading && data !== undefined),
    refresh: resource.refresh,
  };
}

type ResourcePaths<T extends object> = { [K in keyof T]: string };

class ResourceGroup<T extends object> {
  private readonly entries: Array<[keyof T, Resource<T[keyof T]>]>;
  private snapshot: ResourceGroupSnapshot<T>;

  constructor(paths: ResourcePaths<T>) {
    this.entries = (Object.entries(paths) as Array<[keyof T, string]>).map(([name, path]) => [
      name,
      getResource<T[keyof T]>(path),
    ]);
    this.snapshot = this.buildSnapshot();
  }

  subscribe = (listener: Listener): (() => void) => {
    const unsubscribers = this.entries.map(([, resource]) =>
      resource.subscribe(() => {
        this.snapshot = this.buildSnapshot();
        listener();
      }),
    );
    this.snapshot = this.buildSnapshot();
    return () => {
      for (const unsubscribe of unsubscribers) unsubscribe();
    };
  };

  getSnapshot = (): ResourceGroupSnapshot<T> => this.snapshot;

  refresh = (): void => {
    for (const [, resource] of this.entries) resource.refresh();
  };

  private buildSnapshot(): ResourceGroupSnapshot<T> {
    const data = {} as { [K in keyof T]: T[K] | undefined };
    const availability = {} as { [K in keyof T]: ResourceAvailability };
    let isLoading = false;
    let isValidating = false;
    let updatedAt = 0;
    for (const [name, resource] of this.entries) {
      const current = resource.getSnapshot();
      data[name] = current.data;
      availability[name] = resourceAvailability(current);
      isLoading ||= current.isLoading;
      isValidating ||= current.isValidating;
      updatedAt = Math.max(updatedAt, resource.lastSuccessfulAt);
    }
    return { data, availability, isLoading, isValidating, updatedAt };
  }
}

const SERVER_GROUP_SNAPSHOT: ResourceGroupSnapshot<Record<string, unknown>> = {
  data: {},
  availability: {},
  isLoading: true,
  isValidating: false,
  updatedAt: 0,
};

export function useResources<T extends object>(
  paths: ResourcePaths<T>,
): ResourceGroupSnapshot<T> & { refresh: () => void } {
  const signature = Object.entries(paths)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([name, path]) => `${name}\0${path}`)
    .join("\n");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- signature is the structural dependency; inline maps are expected.
  const group = useMemo(() => new ResourceGroup<T>(paths), [signature]);
  const snapshot = useSyncExternalStore(
    group.subscribe,
    group.getSnapshot,
    () => SERVER_GROUP_SNAPSHOT as ResourceGroupSnapshot<T>,
  );
  const deferredSnapshot = useDeferredValue(snapshot);
  const data = {} as { [K in keyof T]: T[K] | undefined };
  let hasFallback = false;
  for (const name of Object.keys(paths) as Array<keyof T>) {
    const current = snapshot.data[name];
    data[name] = current ?? deferredSnapshot.data[name];
    hasFallback ||= current === undefined && data[name] !== undefined;
  }
  return {
    ...snapshot,
    data,
    updatedAt: snapshot.updatedAt || (hasFallback ? deferredSnapshot.updatedAt : 0),
    isLoading: snapshot.isLoading && Object.values(data).every((value) => value === undefined),
    isValidating: snapshot.isValidating || (snapshot.isLoading && hasFallback),
    refresh: group.refresh,
  };
}

export function invalidateResources(prefixes: readonly string[]): void {
  const valid = prefixes.filter((prefix) => prefix.startsWith("/") && prefix.length <= 256);
  if (!valid.length) return;
  for (const resource of registry.values()) {
    if (valid.some((prefix) => resourceMatches(resource.key, prefix))) resource.markStale();
  }
}

subscribeInvalidations(invalidateResources);

/** Explicit operator refresh: only resources mounted on the current screen make a request. */
export function refreshMountedResources(): void {
  for (const resource of registry.values()) {
    if (resource.hasSubscribers()) resource.refresh();
  }
}

export function getResourceSnapshot<T>(key: string): ResourceSnapshot<T> {
  return getResource<T>(key).getSnapshot();
}

// Error registry is kept separate so one request can render stale data and still surface the
// failure. Only mounted sources appear in the global error center.
const errorRegistry = new Map<string, ResourceError>();
const activeErrorKeys = new Set<string>();
const errorListeners = new Set<Listener>();
let errorsSnapshot: ResourceError[] = [];
let errorRecoveryVersion = 0;

function rebuildErrors(): void {
  errorsSnapshot = [...errorRegistry.values()].filter(
    (entry) => activeErrorKeys.has(entry.key) && !entry.dismissed,
  );
}

function emitErrors(): void {
  rebuildErrors();
  for (const listener of errorListeners) listener();
}

export function activateResourceError(key: string): void {
  activeErrorKeys.add(key);
  if (errorRegistry.has(key)) emitErrors();
}

export function deactivateResourceError(key: string): void {
  activeErrorKeys.delete(key);
  if (errorRegistry.has(key)) emitErrors();
}

export function trackResourceError(key: string, error: Error, hasData = false): void {
  const previous = errorRegistry.get(key);
  errorRegistry.set(key, {
    key,
    message: error.message || String(error),
    dismissed: previous?.dismissed ?? false,
    hasData,
  });
  emitErrors();
}

export function clearResourceError(key: string, live: boolean): void {
  const previous = errorRegistry.get(key);
  if (!previous) return;
  if (live && activeErrorKeys.has(key) && !previous.dismissed) errorRecoveryVersion += 1;
  errorRegistry.delete(key);
  emitErrors();
}

export function subscribeErrors(listener: Listener): () => void {
  errorListeners.add(listener);
  return () => errorListeners.delete(listener);
}

export function getErrors(): ResourceError[] {
  return errorsSnapshot;
}

export function getErrorRecoveryVersion(): number {
  return errorRecoveryVersion;
}

export function dismissError(key: string): void {
  const entry = errorRegistry.get(key);
  if (!entry || entry.dismissed) return;
  entry.dismissed = true;
  emitErrors();
}

export function refreshResource(key: string): void {
  registry.get(key)?.refresh();
}

export function __resetResourcesForTests(): void {
  registry.clear();
  errorRegistry.clear();
  activeErrorKeys.clear();
  errorListeners.clear();
  errorsSnapshot = [];
  errorRecoveryVersion = 0;
}
