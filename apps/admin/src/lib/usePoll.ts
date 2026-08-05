"use client";

import { useEffect, useMemo, useSyncExternalStore } from "react";

// SWR-подобный polling-хук: единая реализация для всех страниц админки
// (заменяет scheduleRefresh/refresh из admin-panel.js).
//
// - дедупликация: poller'ы кэшируются по key, параллельные refresh() делят один fetch;
// - интервал: setInterval, тик пропускается, пока document.hidden;
// - ревалидация при возвращении на вкладку (visibilitychange/focus), если данные
//   старше interval;
// - revalidateAll()/refreshPoller() дергают только poller'ы с живыми подписчиками;
// - remount немедленно ревалидирует stale/error snapshot, но сохраняет свежий cache;
// - состояние хранится снаружи React и читается через useSyncExternalStore —
//   данные переживают навигацию между страницами (stale-while-revalidate).

export type PollSnapshot<T> = {
  data: T | undefined;
  error: Error | undefined;
  /** true, только пока идёт самая первая загрузка (нет ни данных, ни ошибки). */
  isLoading: boolean;
};

export type PollOptions = {
  /** Интервал опроса в мс. 0/отсутствие — без автоматического опроса. */
  interval?: number;
};

type Listener = () => void;

const isHidden = () => typeof document !== "undefined" && document.hidden;

class Poller<T> {
  private snapshot: PollSnapshot<T> = { data: undefined, error: undefined, isLoading: true };
  private listeners = new Set<Listener>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private inflight: Promise<void> | null = null;
  private lastFetchAt = 0;
  private onVisible = () => {
    if (isHidden()) return;
    const staleAfter = this.options.interval ?? 30_000;
    if (Date.now() - this.lastFetchAt >= staleAfter) void this.load();
  };

  constructor(
    private readonly key: string,
    private readonly options: PollOptions,
  ) {
    this.fetcher = () => Promise.reject(new Error("poller: fetcher не установлен"));
  }

  private fetcher: () => Promise<T>;

  // Свежая версия fetcher'а подставляется из usePoll на каждый рендер (через эффект) —
  // poller при этом живёт в реестре и не пересоздаётся.
  setFetcher = (fetcher: () => Promise<T>): void => {
    this.fetcher = fetcher;
  };

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    if (this.listeners.size === 1) {
      activatePollError(this.key);
      this.start();
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) {
        this.stop();
        deactivatePollError(this.key);
      }
    };
  };

  getSnapshot = (): PollSnapshot<T> => this.snapshot;

  refresh = (): void => {
    if (this.listeners.size > 0) void this.load();
  };

  private start(): void {
    if (typeof window !== "undefined") {
      window.addEventListener("focus", this.onVisible);
      document.addEventListener("visibilitychange", this.onVisible);
    }
    if (this.options.interval && this.timer === null) {
      this.timer = setInterval(() => {
        if (!isHidden()) void this.load();
      }, this.options.interval);
    }
    const staleAfter = this.options.interval ?? 30_000;
    if (this.lastFetchAt === 0 || this.snapshot.error || Date.now() - this.lastFetchAt >= staleAfter) {
      void this.load();
    }
  }

  private stop(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
    if (typeof window !== "undefined") {
      window.removeEventListener("focus", this.onVisible);
      document.removeEventListener("visibilitychange", this.onVisible);
    }
  }

  private async load(): Promise<void> {
    if (this.inflight) return this.inflight;
    this.inflight = (async () => {
      try {
        const data = await this.fetcher();
        this.snapshot = { data, error: undefined, isLoading: false };
        clearPollError(this.key, this.listeners.size > 0);
      } catch (cause) {
        const error = cause instanceof Error ? cause : new Error(String(cause));
        // stale-while-revalidate: старые данные сохраняем, показываем ошибку.
        this.snapshot = { ...this.snapshot, error, isLoading: false };
        // AbortError — штатная отмена запроса, не сбой источника (как в admin-panel.js).
        if (error.name !== "AbortError" && this.listeners.size > 0) trackPollError(this.key, error);
      } finally {
        this.lastFetchAt = Date.now();
      }
    })();
    try {
      await this.inflight;
    } finally {
      this.inflight = null;
      this.emit();
    }
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}

// Реестр poller'ов по ключу — дедупликация между компонентами и revalidateAll.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const registry = new Map<string, Poller<any>>();

// ── Реестр ошибок источников (аддитивно; аналог failures Map из admin-panel.js) ──
// Ошибка хранится вместе со snapshot poller'а, но ErrorCenter показывает её только
// пока у источника есть живые подписчики. Версия recovery меняется лишь при успехе,
// который действительно снял видимую ошибку активного источника.
export type PollError = {
  /** Ключ poller'а (первый аргумент usePoll/getPoller). */
  key: string;
  message: string;
  /** Скрыт оператором кнопкой ×; при повторном падении флаг сохраняется (как в легаси). */
  dismissed: boolean;
};

const errorRegistry = new Map<string, PollError>();
const activeErrorKeys = new Set<string>();
const errorListeners = new Set<Listener>();
// Кэшированный snapshot: ссылка меняется только при изменении реестра — иначе
// useSyncExternalStore уйдёт в бесконечный ререндер.
let errorsSnapshot: PollError[] = [];
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

function activatePollError(key: string): void {
  activeErrorKeys.add(key);
  if (errorRegistry.has(key)) emitErrors();
}

function deactivatePollError(key: string): void {
  activeErrorKeys.delete(key);
  if (errorRegistry.has(key)) emitErrors();
}

function trackPollError(key: string, error: Error): void {
  const previous = errorRegistry.get(key);
  errorRegistry.set(key, {
    key,
    message: error.message || String(error),
    dismissed: previous?.dismissed ?? false,
  });
  emitErrors();
}

function clearPollError(key: string, live: boolean): void {
  const previous = errorRegistry.get(key);
  if (!previous) return;
  if (live && activeErrorKeys.has(key) && !previous.dismissed) errorRecoveryVersion += 1;
  errorRegistry.delete(key);
  emitErrors();
}

export function subscribeErrors(listener: () => void): () => void {
  errorListeners.add(listener);
  return () => {
    errorListeners.delete(listener);
  };
}

/** Текущие нескрытые ошибки активных источников (стабильная ссылка между изменениями). */
export function getErrors(): PollError[] {
  return errorsSnapshot;
}

/**
 * Версия подтверждённого восстановления: растёт только после успешного fetch,
 * снявшего видимую ошибку живого источника. Использует подписку subscribeErrors.
 */
export function getErrorRecoveryVersion(): number {
  return errorRecoveryVersion;
}

/** Скрыть ошибку кнопкой ×: из списка пропадёт, но при повторном падении вернётся скрытой. */
export function dismissError(key: string): void {
  const entry = errorRegistry.get(key);
  if (!entry || entry.dismissed) return;
  entry.dismissed = true;
  emitErrors();
}

/** Повторить запрос живого источника; размонтированный poller осознанно остаётся без сети. */
export function refreshPoller(key: string): void {
  registry.get(key)?.refresh();
}

export interface PollerHandle<T> {
  subscribe(listener: () => void): () => void;
  getSnapshot(): PollSnapshot<T>;
  refresh(): void;
  setFetcher(fetcher: () => Promise<T>): void;
}

// Достать (или создать) poller по ключу. Экспортирован также для юнит-тестов
// ядра опроса без React-окружения.
export function getPoller<T>(key: string, fetcher?: () => Promise<T>, options: PollOptions = {}): PollerHandle<T> {
  let existing = registry.get(key) as Poller<T> | undefined;
  if (!existing) {
    existing = new Poller<T>(key, options);
    registry.set(key, existing);
  }
  if (fetcher) existing.setFetcher(fetcher);
  return existing;
}

export function revalidateAll(): void {
  for (const poller of registry.values()) poller.refresh();
}

/** Только для тестов: сбросить реестр poller'ов и ошибок между сценариями. */
export function __clearPollersForTests(): void {
  registry.clear();
  errorRegistry.clear();
  activeErrorKeys.clear();
  errorRecoveryVersion = 0;
  rebuildErrors();
}

const SERVER_SNAPSHOT: PollSnapshot<never> = { data: undefined, error: undefined, isLoading: true };

export function usePoll<T>(
  key: string,
  fetcher: () => Promise<T>,
  options: PollOptions = {},
): PollSnapshot<T> & { refresh: () => void } {
  const interval = options.interval;
  const poller = useMemo(() => getPoller<T>(key, undefined, { interval }), [key, interval]);

  // Свежий fetcher подставляем в poller эффектом до подписки useSyncExternalStore
  // (эффекты исполняются в порядке объявления) — пересоздание poller'а не нужно.
  useEffect(() => {
    poller.setFetcher(fetcher);
  }, [poller, fetcher]);

  const snapshot = useSyncExternalStore(
    poller.subscribe,
    poller.getSnapshot,
    () => SERVER_SNAPSHOT as PollSnapshot<T>,
  );

  return { ...snapshot, refresh: poller.refresh };
}
