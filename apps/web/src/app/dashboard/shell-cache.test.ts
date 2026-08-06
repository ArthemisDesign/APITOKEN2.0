import { describe, expect, it } from "vitest";
import type { AccountView, AuthUser } from "@/lib/api";
import {
  clearDashboardShellCache,
  readDashboardShellCache,
  writeDashboardShellCache,
  type ShellStorage,
} from "./shell-cache";

function fakeStorage(initial: Record<string, string> = {}): ShellStorage {
  const dump = new Map<string, string>(Object.entries(initial));
  return {
    getItem: (key) => dump.get(key) ?? null,
    setItem: (key, value) => void dump.set(key, value),
    removeItem: (key) => void dump.delete(key),
  };
}

const user = { id: "user_1" } as AuthUser;
const account = { balanceNano: "1000" } as AccountView;

describe("dashboard shell cache", () => {
  it("round-trips a written snapshot", () => {
    const storage = fakeStorage();
    writeDashboardShellCache(user, account, storage);
    const snapshot = readDashboardShellCache(storage);
    expect(snapshot?.user.id).toBe("user_1");
    expect(snapshot?.account.balanceNano).toBe("1000");
    expect(typeof snapshot?.at).toBe("number");
  });

  it("treats a missing key, corrupt JSON and foreign shapes as a cache miss", () => {
    expect(readDashboardShellCache(fakeStorage())).toBeNull();
    expect(readDashboardShellCache(fakeStorage({ "dashboard-shell-v1": "{oops" }))).toBeNull();
    expect(readDashboardShellCache(fakeStorage({ "dashboard-shell-v1": '"just a string"' }))).toBeNull();
    expect(
      readDashboardShellCache(fakeStorage({ "dashboard-shell-v1": JSON.stringify({ user: {}, account: null }) })),
    ).toBeNull();
  });

  it("disables itself on absent or throwing storage instead of failing the load", () => {
    expect(readDashboardShellCache(null)).toBeNull();
    const throwing: ShellStorage = {
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
      removeItem: () => { throw new Error("denied"); },
    };
    expect(readDashboardShellCache(throwing)).toBeNull();
    writeDashboardShellCache(user, account, throwing);
    clearDashboardShellCache(throwing);
  });

  it("clear removes the snapshot", () => {
    const storage = fakeStorage();
    writeDashboardShellCache(user, account, storage);
    clearDashboardShellCache(storage);
    expect(readDashboardShellCache(storage)).toBeNull();
  });
});
