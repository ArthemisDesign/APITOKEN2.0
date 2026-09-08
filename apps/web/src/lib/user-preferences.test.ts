import { describe, expect, it, vi } from "vitest";
import {
  LANGUAGE_STORAGE_KEY,
  LANDING_THEME_STORAGE_KEY,
  LEGACY_THEME_STORAGE_KEY,
  THEME_STORAGE_KEY,
  readSavedLanguage,
  readSavedTheme,
  saveLanguage,
  saveTheme,
} from "./user-preferences";

function storageWith(values: Record<string, string | null>) {
  return {
    getItem: vi.fn((key: string) => values[key] ?? null),
    setItem: vi.fn(),
  };
}

describe("user preferences", () => {
  it("reads and writes the versioned language preference", () => {
    const storage = storageWith({ [LANGUAGE_STORAGE_KEY]: "ru" });
    expect(readSavedLanguage(storage)).toBe("ru");
    saveLanguage(storage, "en");
    expect(storage.setItem).toHaveBeenCalledWith(LANGUAGE_STORAGE_KEY, "en");
  });

  it("ignores invalid saved languages", () => {
    expect(readSavedLanguage(storageWith({ [LANGUAGE_STORAGE_KEY]: "de" }))).toBeNull();
  });

  it("prefers the versioned theme and supports the legacy theme key", () => {
    expect(readSavedTheme(storageWith({ [THEME_STORAGE_KEY]: "light", [LEGACY_THEME_STORAGE_KEY]: "dark" }))).toBe("light");
    expect(readSavedTheme(storageWith({ [LEGACY_THEME_STORAGE_KEY]: "light" }))).toBe("light");
  });

  it("uses light for invalid values and tolerates unavailable storage", () => {
    expect(readSavedTheme(storageWith({ [THEME_STORAGE_KEY]: "system" }))).toBe("light");
    const unavailable = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
    expect(readSavedLanguage(unavailable)).toBeNull();
    expect(readSavedTheme(unavailable)).toBe("light");
    expect(() => saveLanguage(unavailable, "ru")).not.toThrow();
    expect(() => saveTheme(unavailable, "light")).not.toThrow();
  });

  it("keeps the landing and app theme in sync with the versioned key as authority", () => {
    expect(readSavedTheme(storageWith({ [LANDING_THEME_STORAGE_KEY]: "dark" }))).toBe("dark");
    const storage = storageWith({ [THEME_STORAGE_KEY]: "light", [LANDING_THEME_STORAGE_KEY]: "dark" });
    expect(readSavedTheme(storage)).toBe("light");
    saveTheme(storage, "dark");
    expect(storage.setItem).toHaveBeenCalledWith(THEME_STORAGE_KEY, "dark");
    expect(storage.setItem).toHaveBeenCalledWith(LANDING_THEME_STORAGE_KEY, "dark");
  });
});
