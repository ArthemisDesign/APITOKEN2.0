import { UNIVERSAL_CONNECTIONS } from "@/lib/universal-key";

/**
 * Реестр API-провайдеров, доступных на универсальном ключе.
 * Новый провайдер = одна запись здесь: карточка на странице расхода
 * появится автоматически (со статусом «готов», пока по нему нет запросов).
 */
export interface ProviderDescriptor {
  /** Совпадает с provider в статистике использования (claude = Anthropic). */
  id: string;
  name: string;
  api: string;
  baseUrl: string;
  authHeader: string;
  docsPath: string;
  color: string;
  /** SVG-глиф из public/assets. Если не задан, рисуется первая буква имени. */
  logo?: string;
}

export const PROVIDER_COLORS = {
  anthropic: "#d97757",
  openai: "#10a37f",
  gemini: "#4b8bf5",
  unattributed: "#6f7a8a",
} as const;

export const PROVIDER_REGISTRY: ProviderDescriptor[] = [
  {
    id: "claude",
    name: "Claude",
    api: "Anthropic Messages API",
    baseUrl: UNIVERSAL_CONNECTIONS.claude.baseUrl,
    authHeader: UNIVERSAL_CONNECTIONS.claude.authHeader,
    docsPath: UNIVERSAL_CONNECTIONS.claude.docsPath,
    color: PROVIDER_COLORS.anthropic,
    logo: "/assets/providers/anthropic.svg",
  },
  {
    id: "openai",
    name: "GPT",
    api: "OpenAI-compatible API",
    baseUrl: UNIVERSAL_CONNECTIONS.openai.baseUrl,
    authHeader: UNIVERSAL_CONNECTIONS.openai.authHeader,
    docsPath: UNIVERSAL_CONNECTIONS.openai.docsPath,
    color: PROVIDER_COLORS.openai,
    logo: "/assets/providers/openai.svg",
  },
  {
    id: "gemini",
    name: "Gemini",
    api: "Google Gemini API",
    baseUrl: UNIVERSAL_CONNECTIONS.gemini.baseUrl,
    authHeader: UNIVERSAL_CONNECTIONS.gemini.authHeader,
    docsPath: UNIVERSAL_CONNECTIONS.gemini.docsPath,
    color: PROVIDER_COLORS.gemini,
    logo: "/assets/providers/gemini.svg",
  },
];
