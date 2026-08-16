import { OPENKEYS_SUPPORTED_MODELS } from "@claude-api/contracts";

export const OPENKEYS_PUBLIC_ORIGIN = "https://openkeys.apitoken.sale";
export const APITOKEN_DOCS_URL = "https://apitoken.sale/docs";

export const UNIVERSAL_CONNECTIONS = {
  claude: {
    label: "Claude / Anthropic API",
    shortLabel: "Claude",
    baseUrl: "https://router.apitoken.sale",
    docsPath: "/docs/claude",
    baseUrlVariable: "ANTHROPIC_BASE_URL",
    apiKeyVariable: "ANTHROPIC_API_KEY",
    authHeader: "x-api-key",
  },
  openai: {
    label: "GPT / OpenAI-совместимый API",
    shortLabel: "GPT / OpenAI",
    baseUrl: "https://router.apitoken.sale/v1",
    docsPath: "/docs/openai",
    baseUrlVariable: "OPENAI_BASE_URL",
    apiKeyVariable: "OPENAI_API_KEY",
    authHeader: "Authorization: Bearer",
  },
  gemini: {
    label: "Gemini / Google Gemini API",
    shortLabel: "Gemini",
    baseUrl: "https://router.apitoken.sale",
    docsPath: "/docs",
    baseUrlVariable: "GOOGLE_GEMINI_BASE_URL",
    apiKeyVariable: "GEMINI_API_KEY",
    authHeader: "x-goog-api-key",
  },
  // KIMI speaks Anthropic Messages, so the same variables as Claude carry it — the model id is
  // what selects the provider (`kimi/k3`), which is why this entry looks like the Claude one.
  kimi: {
    label: "Kimi / Anthropic-совместимый API",
    shortLabel: "Kimi",
    baseUrl: "https://router.apitoken.sale",
    docsPath: "/docs/claude",
    baseUrlVariable: "ANTHROPIC_BASE_URL",
    apiKeyVariable: "ANTHROPIC_API_KEY",
    authHeader: "x-api-key",
  },
} as const;

export interface UniversalKeyHandover {
  faceValue: string;
  secret: string | null;
  viewUrl: string;
  supportedModels?: readonly string[];
}

function modelLine(models: readonly string[], prefix: "claude-" | "gpt-"): string {
  const matching = models.filter((model) => model.startsWith(prefix));
  return matching.length > 0 ? matching.join(", ") : "актуальные модели — через /v1/models";
}

/** Один компактный текст выдачи: секрет встречается ровно один раз и готов к отправке клиенту. */
export function universalKeyHandoverText(key: UniversalKeyHandover): string {
  const models = key.supportedModels ?? OPENKEYS_SUPPORTED_MODELS;

  return [
    `Ваш API-ключ на ${key.faceValue} готов`,
    "",
    "🔑 Ключ",
    key.secret ?? "Секрет недоступен",
    "",
    "🤖 Доступные модели",
    `Claude: ${modelLine(models, "claude-")}`,
    `GPT: ${modelLine(models, "gpt-")}`,
    "Также доступны Gemini и Kimi. Полный актуальный каталог — через /v1/models после подключения.",
    "",
    "📖 Документация",
    APITOKEN_DOCS_URL,
    "",
    "📊 Профиль ключа — остаток и расход",
    key.viewUrl,
    "",
    `Один ключ и общий баланс. Номинал ${key.faceValue}; списание 1:1 по официальной цене модели.`,
  ].join("\n");
}
