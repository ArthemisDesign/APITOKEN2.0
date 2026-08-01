/**
 * Реестр API-провайдеров для дашборда: метаданные карточек в usage-разделе.
 * Ключ id совпадает с результатом modelProvider() из lib/model-label.
 * Новый провайдер = одна запись здесь; провайдеры без записи получают
 * автоматическую карточку с буквой вместо логотипа.
 */
export interface DashboardProvider {
  id: string;
  name: string;
  api: string;
  color: string;
  /** SVG-глиф из public/assets. Если не задан, рисуется первая буква имени. */
  logo?: string;
}

export const DASHBOARD_PROVIDERS: DashboardProvider[] = [
  {
    id: "anthropic",
    name: "Claude",
    api: "Anthropic Messages API",
    color: "#d97757",
    logo: "/assets/providers/anthropic.svg",
  },
  {
    id: "openai",
    name: "GPT",
    api: "OpenAI-compatible API",
    color: "#10a37f",
    logo: "/assets/providers/openai.svg",
  },
  {
    id: "gemini",
    name: "Gemini",
    api: "Google Gemini API",
    color: "#4b8bf5",
    logo: "/assets/providers/gemini.svg",
  },
];

/** Авто-карточка для провайдера, которого нет в реестре (например, «other»). */
export function fallbackProvider(id: string, name: string): DashboardProvider {
  return { id, name, api: "API provider", color: "#6f7a8a" };
}
