export type SavedLanguage = "en" | "ru";
export type SavedTheme = "light" | "dark";

export const LANGUAGE_STORAGE_KEY = "lang:v1";
export const THEME_STORAGE_KEY = "theme:v1";
export const LEGACY_THEME_STORAGE_KEY = "theme";

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

export function browserStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function readSavedLanguage(storage: ReadableStorage | null): SavedLanguage | null {
  try {
    const saved = storage?.getItem(LANGUAGE_STORAGE_KEY);
    return saved === "en" || saved === "ru" ? saved : null;
  } catch {
    return null;
  }
}

export function saveLanguage(storage: WritableStorage | null, language: SavedLanguage): void {
  try {
    storage?.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // Browser storage may be unavailable; navigation must still work.
  }
}

export function readSavedTheme(storage: ReadableStorage | null): SavedTheme {
  try {
    const saved = storage?.getItem(THEME_STORAGE_KEY) ?? storage?.getItem(LEGACY_THEME_STORAGE_KEY);
    return saved === "dark" ? "dark" : "light";
  } catch {
    return "light";
  }
}

export function saveTheme(storage: WritableStorage | null, theme: SavedTheme): void {
  try {
    storage?.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Browser storage may be unavailable; the selected theme still applies for this page.
  }
}

export const themeBootstrapScript = `(()=>{try{const s=localStorage.getItem('${THEME_STORAGE_KEY}')??localStorage.getItem('${LEGACY_THEME_STORAGE_KEY}');const t=s==='dark'?'dark':'light';if(t==='dark')document.documentElement.dataset.theme='dark';else delete document.documentElement.dataset.theme}catch{delete document.documentElement.dataset.theme}})()`;
