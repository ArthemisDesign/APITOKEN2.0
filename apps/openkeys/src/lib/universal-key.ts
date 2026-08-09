export const OPENKEYS_PUBLIC_ORIGIN = "https://openkeys.apitoken.sale";

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
}

/** Один самодостаточный текст выдачи: тот же ключ сразу готов для всех трёх API. */
export function universalKeyHandoverText(key: UniversalKeyHandover): string {
  const secret = key.secret ?? "";
  const claude = UNIVERSAL_CONNECTIONS.claude;
  const openai = UNIVERSAL_CONNECTIONS.openai;
  const gemini = UNIVERSAL_CONNECTIONS.gemini;
  const kimi = UNIVERSAL_CONNECTIONS.kimi;

  return [
    `Баланс ключа: ${key.faceValue} по официальным прайсам используемых моделей`,
    "Один ключ и общий баланс работают для Claude, GPT, Gemini и Kimi.",
    "",
    "Claude / Anthropic API",
    `${claude.baseUrlVariable}=${claude.baseUrl}`,
    `${claude.apiKeyVariable}=${secret}`,
    `Инструкция: ${OPENKEYS_PUBLIC_ORIGIN}${claude.docsPath}`,
    "",
    "GPT / OpenAI-совместимый API",
    `${openai.baseUrlVariable}=${openai.baseUrl}`,
    `${openai.apiKeyVariable}=${secret}`,
    `Инструкция: ${OPENKEYS_PUBLIC_ORIGIN}${openai.docsPath}`,
    "",
    "Gemini / Google Gemini API",
    `${gemini.baseUrlVariable}=${gemini.baseUrl}`,
    `${gemini.apiKeyVariable}=${secret}`,
    `Инструкция: ${OPENKEYS_PUBLIC_ORIGIN}${gemini.docsPath}`,
    "",
    "Kimi / Anthropic-совместимый API",
    `${kimi.baseUrlVariable}=${kimi.baseUrl}`,
    `${kimi.apiKeyVariable}=${secret}`,
    "Модель указывайте как kimi/k3 — провайдер выбирается идентификатором модели.",
    `Инструкция: ${OPENKEYS_PUBLIC_ORIGIN}${kimi.docsPath}`,
    "",
    `Остаток и расход: ${key.viewUrl}`,
  ].join("\n");
}
