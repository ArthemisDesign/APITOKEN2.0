import { describe, expect, it } from "vitest";
import { readSavedTheme, saveTheme, THEME_STORAGE_KEY } from "./theme";

function storage(values: Record<string, string> = {}) {
  const data = new Map(Object.entries(values));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => { data.set(key, value); },
    value: (key: string) => data.get(key),
  };
}

describe("sales theme preference", () => {
  it("uses the shared dashboard key and keeps dark as the first visit", () => {
    const store = storage();
    expect(readSavedTheme(store)).toBe("dark");
    saveTheme(store, "light");
    expect(store.value(THEME_STORAGE_KEY)).toBe("light");
    expect(readSavedTheme(store)).toBe("light");
  });

  it("accepts the legacy key and falls back to the dashboard dark default", () => {
    expect(readSavedTheme(storage({ theme: "light" }))).toBe("light");
    expect(readSavedTheme(storage({ theme: "invalid" }))).toBe("dark");
    expect(readSavedTheme({
      getItem: () => { throw new Error("blocked"); },
    })).toBe("dark");
  });
});
