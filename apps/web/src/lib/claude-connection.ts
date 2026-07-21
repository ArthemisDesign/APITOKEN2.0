export const CLAUDE_API_BASE_URL = "https://api.apitoken.sale";
export const CLAUDE_MESSAGES_URL = `${CLAUDE_API_BASE_URL}/v1/messages`;
export const CLAUDE_DEFAULT_MODEL = "claude-sonnet-5";
export const CLAUDE_API_VERSION = "2023-06-01";

const PUBLIC_SITE_ORIGIN = "https://apitoken.sale";
const API_KEY_PLACEHOLDER = "YOUR_SK_POOL_API_KEY";

export function buildClaudeCodeCommands(apiKey?: string | null): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;
  return `export ANTHROPIC_BASE_URL="${CLAUDE_API_BASE_URL}"
export ANTHROPIC_API_KEY="${key}"

claude`;
}

export function publicDocsUrl(docsUrl: string): string {
  try {
    const resolved = new URL(docsUrl, PUBLIC_SITE_ORIGIN);
    if (resolved.protocol === "http:" || resolved.protocol === "https:") return resolved.toString();
  } catch {
    // Fall through to the canonical public documentation address.
  }
  return `${PUBLIC_SITE_ORIGIN}/docs`;
}

export function buildClaudeAgentHandoff({
  apiKey,
  docsUrl,
  language,
}: {
  apiKey?: string | null;
  docsUrl: string;
  language: "en" | "ru";
}): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;
  const docs = publicDocsUrl(docsUrl);

  if (language === "ru") {
    return `Подключи этот проект к Claude через apiToken.sale.

Данные подключения:
- API: Anthropic Messages API (совместим с официальными Anthropic SDK)
- Base URL: ${CLAUDE_API_BASE_URL}
- Messages endpoint: POST ${CLAUDE_MESSAGES_URL}
- API key: ${key}
- Переменная ключа: ANTHROPIC_API_KEY
- Переменная адреса: ANTHROPIC_BASE_URL=${CLAUDE_API_BASE_URL}
- Для прямого HTTP: x-api-key: ${key}
- Версия API: anthropic-version: ${CLAUDE_API_VERSION}
- Модель по умолчанию: ${CLAUDE_DEFAULT_MODEL}
- Документация: ${docs}

Изучи стек проекта и интегрируй Claude подходящим официальным Anthropic SDK или прямым Messages API. Храни ключ только в серверной переменной окружения или менеджере секретов, не добавляй его в исходный код, клиентский бандл или логи. Сохрани текущую архитектуру проекта и проверь интеграцию минимальным запросом.`;
  }

  return `Connect this project to Claude through apiToken.sale.

Connection details:
- API: Anthropic Messages API (compatible with the official Anthropic SDKs)
- Base URL: ${CLAUDE_API_BASE_URL}
- Messages endpoint: POST ${CLAUDE_MESSAGES_URL}
- API key: ${key}
- Key environment variable: ANTHROPIC_API_KEY
- Base URL environment variable: ANTHROPIC_BASE_URL=${CLAUDE_API_BASE_URL}
- For direct HTTP: x-api-key: ${key}
- API version: anthropic-version: ${CLAUDE_API_VERSION}
- Default model: ${CLAUDE_DEFAULT_MODEL}
- Documentation: ${docs}

Inspect this project's stack and integrate Claude with the appropriate official Anthropic SDK or the Messages API directly. Keep the key only in a server-side environment variable or secret manager; never commit it, expose it in a client bundle, or write it to logs. Preserve the project's existing architecture and verify the integration with one minimal request.`;
}
