import { UNIVERSAL_CONNECTIONS } from "@/lib/universal-key";

/**
 * Полные публичные доки живут только на apitoken.sale — они не требуют логина и
 * не содержат ничего дашбордного, поэтому OpenKeys ссылается на них вместо
 * поддержки собственной копии инструкций.
 */
export const OFFICIAL_DOCS_URL = "https://apitoken.sale/docs";
export const OFFICIAL_DOCS_OPENAI_URL = "https://apitoken.sale/docs#openai";

export const CLAUDE_DEFAULT_MODEL = "claude-sonnet-5";
export const CLAUDE_API_VERSION = "2023-06-01";
export const OPENAI_DEFAULT_MODEL = "gpt-5.6-sol";

const API_KEY_PLACEHOLDER = "YOUR_SK_POOL_API_KEY";
const CLAUDE_BASE_URL = UNIVERSAL_CONNECTIONS.claude.baseUrl;
const OPENAI_BASE_URL = UNIVERSAL_CONNECTIONS.openai.baseUrl;

export function buildClaudeCodeCommands(apiKey?: string | null): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;
  return `echo 'export ANTHROPIC_BASE_URL="${CLAUDE_BASE_URL}"' >> ~/.zshrc
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
base_url = "${OPENAI_BASE_URL}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"
EOF
echo 'export APITOKEN_API_KEY="${key}"' >> ~/.zshrc
source ~/.zshrc
codex --profile apitoken`;
}

export function buildAgentHandoff({
  apiKey,
  language,
}: {
  apiKey?: string | null;
  language: "en" | "ru";
}): string {
  const key = apiKey?.trim() || API_KEY_PLACEHOLDER;

  if (language === "ru") {
    return `Подключи этот проект к apiToken.sale — один ключ sk-pool работает сразу с двумя API.

Anthropic Messages API (модели Claude):
- Base URL: ${CLAUDE_BASE_URL}
- Messages endpoint: POST ${CLAUDE_BASE_URL}/v1/messages
- Переменная ключа: ANTHROPIC_API_KEY
- Переменная адреса: ANTHROPIC_BASE_URL=${CLAUDE_BASE_URL}
- Для прямого HTTP: x-api-key: ${key}
- Версия API: anthropic-version: ${CLAUDE_API_VERSION}
- Модель по умолчанию: ${CLAUDE_DEFAULT_MODEL}

OpenAI-совместимый API (модели GPT):
- Base URL: ${OPENAI_BASE_URL}
- Responses endpoint: POST ${OPENAI_BASE_URL}/responses (также Chat Completions: POST ${OPENAI_BASE_URL}/chat/completions)
- Авторизация: Authorization: Bearer ${key}
- Список моделей: GET ${OPENAI_BASE_URL}/models
- Модель по умолчанию: ${OPENAI_DEFAULT_MODEL}

Общее:
- API key: ${key}
- Баланс общий для обоих API.
- Документация: ${OFFICIAL_DOCS_URL}

Изучи стек проекта и интегрируй подходящий официальный SDK (Anthropic или OpenAI) или прямой HTTP-вызов нужного API. Храни ключ только в серверной переменной окружения или менеджере секретов, не добавляй его в исходный код, клиентский бандл или логи. Сохрани текущую архитектуру проекта и проверь интеграцию минимальным запросом.`;
  }

  return `Connect this project to apiToken.sale — one sk-pool key works with both APIs.

Anthropic Messages API (Claude models):
- Base URL: ${CLAUDE_BASE_URL}
- Messages endpoint: POST ${CLAUDE_BASE_URL}/v1/messages
- Key environment variable: ANTHROPIC_API_KEY
- Base URL environment variable: ANTHROPIC_BASE_URL=${CLAUDE_BASE_URL}
- For direct HTTP: x-api-key: ${key}
- API version: anthropic-version: ${CLAUDE_API_VERSION}
- Default model: ${CLAUDE_DEFAULT_MODEL}

OpenAI-compatible API (GPT models):
- Base URL: ${OPENAI_BASE_URL}
- Responses endpoint: POST ${OPENAI_BASE_URL}/responses (Chat Completions also available: POST ${OPENAI_BASE_URL}/chat/completions)
- Authorization: Bearer ${key}
- Model discovery: GET ${OPENAI_BASE_URL}/models
- Default model: ${OPENAI_DEFAULT_MODEL}

Shared:
- API key: ${key}
- One prepaid balance covers both APIs.
- Documentation: ${OFFICIAL_DOCS_URL}

Inspect this project's stack and integrate the appropriate official SDK (Anthropic or OpenAI) or the direct HTTP call for the right API. Keep the key only in a server-side environment variable or secret manager; never commit it, expose it in a client bundle, or write it to logs. Preserve the project's existing architecture and verify the integration with one minimal request.`;
}
