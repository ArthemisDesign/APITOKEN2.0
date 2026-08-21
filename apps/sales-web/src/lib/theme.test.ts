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
  it("uses the shared dashboard key and keeps light as the first visit", () => {
    const store = storage();
    expect(readSavedTheme(store)).toBe("light");
    saveTheme(store, "dark");
    expect(store.value(THEME_STORAGE_KEY)).toBe("dark");
    expect(readSavedTheme(store)).toBe("dark");
  });

  it("accepts the legacy key for users who already selected dark mode", () => {
    expect(readSavedTheme(storage({ theme: "dark" }))).toBe("dark");
    expect(readSavedTheme(storage({ theme: "invalid" }))).toBe("light");
  });
});
