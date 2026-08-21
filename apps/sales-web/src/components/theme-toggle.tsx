"use client";

import { useEffect, useState } from "react";
import { useI18n } from "@/components/i18n";
import { browserStorage, readSavedTheme, saveTheme, type SalesTheme } from "@/lib/theme";

export function ThemeToggle() {
  const { t } = useI18n();
  const [theme, setTheme] = useState<SalesTheme>("light");
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    const saved = readSavedTheme(browserStorage());
    setTheme(saved);
    setMounted(true);
  }, []);

  useEffect(() => {
    if (!mounted) return;
    if (theme === "dark") document.documentElement.dataset.theme = "dark";
    else delete document.documentElement.dataset.theme;
    saveTheme(browserStorage(), theme);
  }, [mounted, theme]);

  return (
    <button
      type="button"
      className="theme-tgl"
      aria-label={theme === "dark" ? t("Switch to light theme", "Включить светлую тему") : t("Switch to dark theme", "Включить тёмную тему")}
      onClick={() => setTheme((current) => current === "dark" ? "light" : "dark")}
    >
      {theme === "dark" ? "☀" : "☾"}
    </button>
  );
}
