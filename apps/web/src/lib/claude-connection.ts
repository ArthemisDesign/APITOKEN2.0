export const CLAUDE_API_BASE_URL = "https://api.apitoken.sale";
export const CLAUDE_MESSAGES_URL = `${CLAUDE_API_BASE_URL}/v1/messages`;
export const CLAUDE_DEFAULT_MODEL = "claude-sonnet-5";
export const CLAUDE_API_VERSION = "2023-06-01";

export const OPENAI_API_BASE_URL = "https://openai.api.apitoken.sale/v1";
export const OPENAI_RESPONSES_URL = `${OPENAI_API_BASE_URL}/responses`;
export const OPENAI_DEFAULT_MODEL = "gpt-5.6-sol";

const PUBLIC_SITE_ORIGIN = "https://apitoken.sale";
const API_KEY_PLACEHOLDER = "YOUR_SK_POOL_API_KEY";

export function buildClaudeCodeCommands(apiKey?: string | null): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;
  return `echo 'export ANTHROPIC_BASE_URL="${CLAUDE_API_BASE_URL}"' >> ~/.zshrc
echo 'export ANTHROPIC_API_KEY="${key}"' >> ~/.zshrc
source ~/.zshrc
claude`;
}

export function buildCodexCommands(apiKey?: string | null): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;
  return `mkdir -p ~/.codex && cat > ~/.codex/apitoken.config.toml << 'EOF'
model = "${OPENAI_DEFAULT_MODEL}"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "${OPENAI_API_BASE_URL}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"
EOF
echo 'export APITOKEN_API_KEY="${key}"' >> ~/.zshrc
source ~/.zshrc
codex --profile apitoken`;
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

export function buildAgentHandoff({
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
    return `Подключи этот проект к apiToken.sale — один ключ sk-pool работает сразу с двумя API.

Anthropic Messages API (модели Claude):
- Base URL: ${CLAUDE_API_BASE_URL}
- Messages endpoint: POST ${CLAUDE_MESSAGES_URL}
- Переменная ключа: ANTHROPIC_API_KEY
- Переменная адреса: ANTHROPIC_BASE_URL=${CLAUDE_API_BASE_URL}
- Для прямого HTTP: x-api-key: ${key}
- Версия API: anthropic-version: ${CLAUDE_API_VERSION}
- Модель по умолчанию: ${CLAUDE_DEFAULT_MODEL}

OpenAI-совместимый API (модели GPT):
- Base URL: ${OPENAI_API_BASE_URL}
- Responses endpoint: POST ${OPENAI_RESPONSES_URL} (также Chat Completions: POST ${OPENAI_API_BASE_URL}/chat/completions)
- Авторизация: Authorization: Bearer ${key}
- Список моделей: GET ${OPENAI_API_BASE_URL}/models
- Модель по умолчанию: ${OPENAI_DEFAULT_MODEL}

Общее:
- API key: ${key}
- Баланс и скидка общие для обоих API.
- Документация: ${docs}

Изучи стек проекта и интегрируй подходящий официальный SDK (Anthropic или OpenAI) или прямой HTTP-вызов нужного API. Храни ключ только в серверной переменной окружения или менеджере секретов, не добавляй его в исходный код, клиентский бандл или логи. Сохрани текущую архитектуру проекта и проверь интеграцию минимальным запросом.`;
  }

  return `Connect this project to apiToken.sale — one sk-pool key works with both APIs.

Anthropic Messages API (Claude models):
- Base URL: ${CLAUDE_API_BASE_URL}
- Messages endpoint: POST ${CLAUDE_MESSAGES_URL}
- Key environment variable: ANTHROPIC_API_KEY
- Base URL environment variable: ANTHROPIC_BASE_URL=${CLAUDE_API_BASE_URL}
- For direct HTTP: x-api-key: ${key}
- API version: anthropic-version: ${CLAUDE_API_VERSION}
- Default model: ${CLAUDE_DEFAULT_MODEL}

OpenAI-compatible API (GPT models):
- Base URL: ${OPENAI_API_BASE_URL}
- Responses endpoint: POST ${OPENAI_RESPONSES_URL} (Chat Completions also available: POST ${OPENAI_API_BASE_URL}/chat/completions)
- Authorization: Bearer ${key}
- Model discovery: GET ${OPENAI_API_BASE_URL}/models
- Default model: ${OPENAI_DEFAULT_MODEL}

Shared:
- API key: ${key}
- One prepaid balance and one discount cover both APIs.
- Documentation: ${docs}

Inspect this project's stack and integrate the appropriate official SDK (Anthropic or OpenAI) or the direct HTTP call for the right API. Keep the key only in a server-side environment variable or secret manager; never commit it, expose it in a client bundle, or write it to logs. Preserve the project's existing architecture and verify the integration with one minimal request.`;
}
