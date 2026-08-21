export type SalesTheme = "light" | "dark";

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

export function readSavedTheme(storage: ReadableStorage | null): SalesTheme {
  try {
    const saved = storage?.getItem(THEME_STORAGE_KEY) ?? storage?.getItem(LEGACY_THEME_STORAGE_KEY);
    return saved === "dark" ? "dark" : "light";
  } catch {
    return "light";
  }
}

export function saveTheme(storage: WritableStorage | null, theme: SalesTheme): void {
  try {
    storage?.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Browser storage can be unavailable. The selected theme still applies to this page.
  }
}

/** Apply a saved theme before hydration. No saved value keeps the partner cabinet light. */
export const themeBootstrapScript = `(()=>{try{const s=localStorage.getItem('${THEME_STORAGE_KEY}')??localStorage.getItem('${LEGACY_THEME_STORAGE_KEY}');if(s==='dark')document.documentElement.dataset.theme='dark'}catch{}})()`;
