export const OPENKEYS_PUBLIC_ORIGIN = "https://openkeys.apitoken.sale";

export const UNIVERSAL_CONNECTIONS = {
  claude: {
    label: "Claude / Anthropic API",
    shortLabel: "Claude",
    baseUrl: "https://api.apitoken.sale",
    docsPath: "/docs/claude",
    baseUrlVariable: "ANTHROPIC_BASE_URL",
    apiKeyVariable: "ANTHROPIC_API_KEY",
    authHeader: "x-api-key",
  },
  openai: {
    label: "GPT / OpenAI-совместимый API",
    shortLabel: "GPT / OpenAI",
    baseUrl: "https://openai.api.apitoken.sale/v1",
    docsPath: "/docs/openai",
    baseUrlVariable: "OPENAI_BASE_URL",
    apiKeyVariable: "OPENAI_API_KEY",
    authHeader: "Authorization: Bearer",
  },
} as const;

export interface UniversalKeyHandover {
  faceValue: string;
  secret: string | null;
  viewUrl: string;
}

/** Один самодостаточный текст выдачи: тот же ключ сразу готов для обоих API. */
export function universalKeyHandoverText(key: UniversalKeyHandover): string {
  const secret = key.secret ?? "";
  const claude = UNIVERSAL_CONNECTIONS.claude;
  const openai = UNIVERSAL_CONNECTIONS.openai;

  return [
    `Баланс ключа: ${key.faceValue} по официальным прайсам используемых моделей`,
    "Один ключ и общий баланс работают для Claude и GPT.",
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
    `Остаток и расход: ${key.viewUrl}`,
  ].join("\n");
}
