// Тема админки: dark/light через data-theme на <html> (CSS-переменные в globals.css).
// Ключ версионирован, чтобы при смене схемы не подхватывать старые значения.
export const THEME_STORAGE_KEY = "theme:v1";

export type Theme = "dark" | "light";

// Должен совпадать с inline-скриптом в src/app/layout.tsx (тот исполняется до
// первой отрисовки и не может импортировать модули).
export function resolveInitialTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  try {
    const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
    return saved === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // приватный режим и т.п. — тема просто не сохранится
  }
}

export function toggleTheme(): Theme {
  const next: Theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  applyTheme(next);
  return next;
}
