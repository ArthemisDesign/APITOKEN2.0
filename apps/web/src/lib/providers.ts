/**
 * Реестр API-провайдеров для дашборда: метаданные карточек в usage-разделе.
 * Ключ id совпадает с providerId из usage/pricing policy API.
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
  /** Публичный endpoint провайдера для строки подключения в карточке. */
  endpoint?: string;
  /** Схема авторизации рядом с endpoint (короткий ярлык). */
  auth?: string;
  /** Куда ведёт ссылка «Setup guide». */
  docsPath?: string;
}

export const DASHBOARD_PROVIDERS: DashboardProvider[] = [
  {
    id: "anthropic",
    name: "Claude",
    api: "Anthropic Messages API",
    color: "#d97757",
    logo: "/assets/providers/anthropic.svg",
    endpoint: "router.apitoken.sale",
    auth: "x-api-key",
    docsPath: "/docs",
  },
  {
    id: "openai",
    name: "GPT",
    api: "OpenAI-compatible API",
    color: "#10a37f",
    logo: "/assets/providers/openai.svg",
    endpoint: "router.apitoken.sale/v1",
    auth: "Authorization: Bearer",
    docsPath: "/docs",
  },
  {
    id: "google",
    name: "Gemini",
    api: "Google Gemini API",
    color: "#4b8bf5",
    logo: "/assets/providers/gemini.svg",
    endpoint: "router.apitoken.sale",
    auth: "x-goog-api-key",
    docsPath: "/docs",
  },
];

/** Авто-карточка для провайдера, которого нет в реестре (например, «other»). */
export function fallbackProvider(id: string, name: string): DashboardProvider {
  return { id, name, api: "API provider", color: "#6f7a8a" };
}
