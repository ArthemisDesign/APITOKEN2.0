import type { AccountView, AuthUser } from "@/lib/api";

/**
 * Stale-while-revalidate снапшот оболочки дашборда (личность + счёт) в sessionStorage.
 * Повторный заход на /dashboard рендерится мгновенно из снапшота, а свежие данные
 * догружаются фоновым тихим запросом и заменяют его. sessionStorage, а не
 * localStorage: снапшот умирает вместе с вкладкой и не переживает сессию устройства.
 */

const STORAGE_KEY = "dashboard-shell-v1";

export interface DashboardShellCache {
  user: AuthUser;
  account: AccountView;
  at: number;
}

/** Инъектируемый минимум Storage — в тестах подменяется Map'ой (vitest работает в node). */
export type ShellStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function defaultStorage(): ShellStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.sessionStorage;
  } catch {
    // sessionStorage может быть недоступен (приватные режимы) — кэш просто отключается.
    return null;
  }
}

export function readDashboardShellCache(storage: ShellStorage | null = defaultStorage()): DashboardShellCache | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<DashboardShellCache> | null;
    if (!parsed || typeof parsed !== "object") return null;
    if (!parsed.user || typeof parsed.user.id !== "string" || !parsed.account) return null;
    return parsed as DashboardShellCache;
  } catch {
    // Битый JSON или чужое значение под нашим ключом — считаем кэш промахом.
    return null;
  }
}

export function writeDashboardShellCache(
  user: AuthUser,
  account: AccountView,
  storage: ShellStorage | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    const snapshot: DashboardShellCache = { user, account, at: Date.now() };
    storage.setItem(STORAGE_KEY, JSON.stringify(snapshot));
  } catch {
    // Квота хранилища — не повод ронять загрузку дашборда.
  }
}

export function clearDashboardShellCache(storage: ShellStorage | null = defaultStorage()): void {
  if (!storage) return;
  try {
    storage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}
