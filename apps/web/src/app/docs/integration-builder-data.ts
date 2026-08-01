// Keep the interactive builder's catalog deliberately small. Importing the SEO model
// registry here would ship every long description/FAQ to the browser just to render
// two endpoint strings.
export const ANTHROPIC_BASE_URL = "https://api.apitoken.sale";
export const OPENAI_BASE_URL = "https://openai.api.apitoken.sale/v1";
export const GEMINI_BASE_URL = "https://gemini.api.apitoken.sale";

export type IntegrationProvider = "anthropic" | "openai" | "gemini";
export type IntegrationTool = "claude-code" | "codex" | "gemini-cli" | "opencode" | "pi" | "hermes";
export type IntegrationOs = "unix" | "powershell" | "cmd";
export type IntegrationLanguage = "en" | "ru";

export type IntegrationModel = {
  id: string;
  name: string;
};

export type IntegrationStep = {
  title: string;
  text: string;
  code: string;
  codeLabel?: string;
};

export type IntegrationGuide = {
  title: string;
  summary: string;
  endpoint: string;
  requirement?: string;
  securityNote?: string;
  steps: IntegrationStep[];
};

export const INTEGRATION_MODELS: Record<IntegrationProvider, readonly IntegrationModel[]> = {
  anthropic: [
    { id: "claude-opus-5", name: "Claude Opus 5" },
    { id: "claude-fable-5", name: "Claude Fable 5" },
    { id: "claude-opus-4-8", name: "Claude Opus 4.8" },
    { id: "claude-opus-4-7", name: "Claude Opus 4.7" },
    { id: "claude-sonnet-5", name: "Claude Sonnet 5" },
    { id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" },
    { id: "claude-haiku-4-5", name: "Claude Haiku 4.5" },
  ],
  openai: [
    { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
    { id: "gpt-5.6-terra", name: "GPT-5.6 Terra" },
    { id: "gpt-5.6-luna", name: "GPT-5.6 Luna" },
    { id: "gpt-5.5", name: "GPT-5.5" },
    { id: "gpt-5.4", name: "GPT-5.4" },
  ],
  gemini: [
    { id: "gemini-3.6-flash", name: "Gemini 3.6 Flash" },
    { id: "gemini-3.5-flash", name: "Gemini 3.5 Flash" },
    { id: "gemini-3.1-pro-preview", name: "Gemini 3.1 Pro Preview" },
    { id: "gemini-3.1-flash-lite", name: "Gemini 3.1 Flash-Lite" },
    { id: "gemini-2.5-flash", name: "Gemini 2.5 Flash" },
    { id: "gemini-2.5-flash-lite", name: "Gemini 2.5 Flash-Lite" },
    { id: "gemini-3.1-flash-image", name: "Gemini 3.1 Flash Image (Nano Banana 2)" },
  ],
};

export const TOOL_COMPATIBILITY: Record<IntegrationTool, readonly IntegrationProvider[]> = {
  "claude-code": ["anthropic"],
  codex: ["openai"],
  "gemini-cli": ["gemini"],
  opencode: ["anthropic", "openai", "gemini"],
  pi: ["anthropic", "openai", "gemini"],
  hermes: ["openai"],
};

const TOOL_NAMES: Record<IntegrationTool, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "gemini-cli": "Gemini CLI",
  opencode: "OpenCode",
  pi: "Pi",
  hermes: "Hermes",
};

const keyPlaceholder = "sk-pool-•••";

const PROVIDER_ENDPOINTS: Record<IntegrationProvider, string> = {
  anthropic: ANTHROPIC_BASE_URL,
  openai: OPENAI_BASE_URL,
  gemini: GEMINI_BASE_URL,
};

function localize(language: IntegrationLanguage, en: string, ru: string): string {
  return language === "ru" ? ru : en;
}

function isWindows(os: IntegrationOs): boolean {
  return os === "powershell" || os === "cmd";
}

function configPath(os: IntegrationOs, posixPath: string, windowsPath: string): string {
  return isWindows(os) ? windowsPath : posixPath;
}

function environmentCommand(os: IntegrationOs, values: Record<string, string>, unset: string[] = []): string {
  if (os === "powershell") {
    const removals = unset.map((name) => `Remove-Item Env:${name} -ErrorAction SilentlyContinue`);
    const assignments = Object.entries(values).map(([name, value]) => `$env:${name} = "${value}"`);
    return [...removals, ...assignments].join("\n");
  }
  if (os === "cmd") {
    const removals = unset.map((name) => `set "${name}="`);
    const assignments = Object.entries(values).map(([name, value]) => `set "${name}=${value}"`);
    return [...removals, ...assignments].join("\n");
  }
  const removals = unset.map((name) => `unset ${name}`);
  const assignments = Object.entries(values).map(([name, value]) => `export ${name}="${value}"`);
  return [...removals, ...assignments].join("\n");
}

function claudeCodeGuide(model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  const connection = environmentCommand(os, {
    ANTHROPIC_BASE_URL,
    ANTHROPIC_API_KEY: keyPlaceholder,
  }, ["ANTHROPIC_AUTH_TOKEN"]);
  return {
    title: `Claude Code · ${model.name}`,
    summary: localize(language, "Native Claude coding agent through the Anthropic Messages API.", "Нативный coding agent Claude через Anthropic Messages API."),
    endpoint: ANTHROPIC_BASE_URL,
    steps: [
      {
        title: localize(language, "Set the connection", "Задайте подключение"),
        text: localize(language, "Run in the terminal that will start Claude Code. The key lives only in this session.", "Выполните в терминале, из которого запустите Claude Code. Ключ останется только в этой сессии."),
        code: connection,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
      {
        title: localize(language, "Start Claude Code", "Запустите Claude Code"),
        text: localize(language, "The explicit model flag avoids inheriting a model from an old login or project setting.", "Явный model flag не даст подхватить модель из старого логина или настроек проекта."),
        code: `claude --model ${model.id}`,
        codeLabel: localize(language, "Run", "Запуск"),
      },
      {
        title: localize(language, "Verify inside Claude Code", "Проверьте внутри Claude Code"),
        text: localize(language, "The status screen must show apiToken.sale as the Anthropic base URL and ANTHROPIC_API_KEY as the credential source.", "В статусе должны быть apiToken.sale как Anthropic base URL и ANTHROPIC_API_KEY как источник ключа."),
        code: `/status\n\nReply with exactly: connected`,
        codeLabel: localize(language, "Inside Claude Code", "В Claude Code"),
      },
    ],
  };
}

function codexGuide(model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  const path = configPath(os, "~/.codex/apitoken.config.toml", "%USERPROFILE%\\.codex\\apitoken.config.toml");
  const profile = `model = "${model.id}"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "${OPENAI_BASE_URL}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"`;
  const run = `${environmentCommand(os, { APITOKEN_API_KEY: keyPlaceholder })}\n\ncodex --profile apitoken`;
  return {
    title: `Codex · ${model.name}`,
    summary: localize(language, "Codex CLI through a separate Responses API profile.", "Codex CLI через отдельный профиль Responses API."),
    endpoint: OPENAI_BASE_URL,
    steps: [
      {
        title: localize(language, "Create a separate profile", "Создайте отдельный профиль"),
        text: localize(language, `Save this as \`${path}\`. Your normal Codex login and default config stay untouched.`, `Сохраните как \`${path}\`. Обычный логин и основной config Codex не изменятся.`),
        code: profile,
        codeLabel: path,
      },
      {
        title: localize(language, "Set the key and start", "Задайте ключ и запустите"),
        text: localize(language, "Codex reads the secret from the current shell and the provider settings from the named profile.", "Codex возьмёт секрет из текущей оболочки, а настройки провайдера — из отдельного профиля."),
        code: run,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
      {
        title: localize(language, "Verify the route", "Проверьте маршрут"),
        text: localize(language, "Send one deterministic prompt. A normal answer confirms the profile, key, endpoint, model, and streaming path.", "Отправьте один однозначный запрос. Ответ подтвердит профиль, ключ, endpoint, модель и streaming."),
        code: `Reply with exactly: connected`,
        codeLabel: localize(language, "Inside Codex", "В Codex"),
      },
    ],
  };
}

function geminiCliGuide(model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  const connection = environmentCommand(os, {
    GOOGLE_GEMINI_BASE_URL: GEMINI_BASE_URL,
    GEMINI_API_KEY: keyPlaceholder,
  });
  return {
    title: `Gemini CLI · ${model.name}`,
    summary: localize(language, "Native Gemini coding agent through the Google Gemini API.", "Нативный coding agent Gemini через Google Gemini API."),
    endpoint: GEMINI_BASE_URL,
    requirement: localize(language, "If you previously signed in with a Google account, run /auth inside Gemini CLI and switch to the API key — a saved OAuth login can take precedence.", "Если раньше входили через Google-аккаунт, выполните /auth внутри Gemini CLI и переключитесь на API key — сохранённый OAuth-логин может иметь приоритет."),
    steps: [
      {
        title: localize(language, "Set the connection", "Задайте подключение"),
        text: localize(language, "Run in the terminal that will start Gemini CLI. The key lives only in this session and is sent as x-goog-api-key.", "Выполните в терминале, из которого запустите Gemini CLI. Ключ останется только в этой сессии и отправляется как x-goog-api-key."),
        code: connection,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
      {
        title: localize(language, "Start Gemini CLI", "Запустите Gemini CLI"),
        text: localize(language, "The explicit model flag avoids inheriting a model from an old login or project setting.", "Явный model flag не даст подхватить модель из старого логина или настроек проекта."),
        code: `gemini --model ${model.id}`,
        codeLabel: localize(language, "Run", "Запуск"),
      },
      {
        title: localize(language, "Verify inside Gemini CLI", "Проверьте внутри Gemini CLI"),
        text: localize(language, "A normal answer confirms the gateway route, the key, and the model.", "Ответ подтвердит маршрут через gateway, ключ и модель."),
        code: `Reply with exactly: connected`,
        codeLabel: localize(language, "Inside Gemini CLI", "В Gemini CLI"),
      },
    ],
  };
}

function openCodeConfig(provider: IntegrationProvider, model: IntegrationModel): string {
  // The Google ai-sdk package expects the baseURL including the /v1beta prefix,
  // while the Anthropic one wants the /v1 suffix; OpenAI already carries it.
  const npmProvider = provider === "anthropic"
    ? "@ai-sdk/anthropic"
    : provider === "gemini"
      ? "@ai-sdk/google"
      : "@ai-sdk/openai-compatible";
  const baseURL = provider === "anthropic"
    ? `${ANTHROPIC_BASE_URL}/v1`
    : provider === "gemini"
      ? `${GEMINI_BASE_URL}/v1beta`
      : OPENAI_BASE_URL;
  const label = provider === "anthropic" ? "Claude" : provider === "gemini" ? "Gemini" : "GPT";
  return JSON.stringify({
    $schema: "https://opencode.ai/config.json",
    provider: {
      apitoken: {
        npm: npmProvider,
        name: `apiToken.sale · ${label}`,
        options: {
          baseURL,
          apiKey: "{env:APITOKEN_API_KEY}",
        },
        models: {
          [model.id]: { name: model.name },
        },
      },
    },
  }, null, 2);
}

function openCodeGuide(provider: IntegrationProvider, model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  const path = configPath(os, "~/.config/opencode/opencode.json", "%USERPROFILE%\\.config\\opencode\\opencode.json");
  const modelRef = `apitoken/${model.id}`;
  const run = `${environmentCommand(os, { APITOKEN_API_KEY: keyPlaceholder })}\n\nopencode --model ${modelRef}`;
  return {
    title: `OpenCode · ${model.name}`,
    summary: localize(language, "Open-source terminal coding agent with an isolated custom provider.", "Open-source coding agent в терминале с отдельным custom provider."),
    endpoint: PROVIDER_ENDPOINTS[provider],
    steps: [
      {
        title: localize(language, "Add the provider", "Добавьте провайдера"),
        text: localize(language, `Save the configuration as \`${path}\`. The key remains an environment reference, not a value in JSON.`, `Сохраните конфигурацию как \`${path}\`. В JSON останется ссылка на переменную, а не сам ключ.`),
        code: openCodeConfig(provider, model),
        codeLabel: path,
      },
      {
        title: localize(language, "Set the key and start", "Задайте ключ и запустите"),
        text: localize(language, "The provider/model form selects this route explicitly and does not affect other OpenCode providers.", "Формат provider/model явно выбирает это подключение и не затрагивает другие провайдеры OpenCode."),
        code: run,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
      {
        title: localize(language, "Run a real check", "Выполните реальную проверку"),
        text: localize(language, "This one-shot command uses the same configuration, tools, and streaming client as an interactive session.", "Одноразовая команда использует ту же конфигурацию, tools и streaming, что и интерактивная сессия."),
        code: `opencode run --model ${modelRef} "Reply with exactly: connected"`,
        codeLabel: localize(language, "Verification", "Проверка"),
      },
    ],
  };
}

function piConfig(provider: IntegrationProvider, model: IntegrationModel): string {
  return JSON.stringify({
    providers: {
      apitoken: {
        baseUrl: PROVIDER_ENDPOINTS[provider],
        // Pi's OpenAI completions adapter is the broadest-compatible path for
        // gateways. Responses remains the dedicated wire format for Codex.
        api: provider === "anthropic" ? "anthropic-messages" : provider === "gemini" ? "google-generative-ai" : "openai-completions",
        apiKey: "$APITOKEN_API_KEY",
        models: [{
          id: model.id,
          name: model.name,
          reasoning: true,
          input: ["text", "image"],
        }],
      },
    },
  }, null, 2);
}

function piGuide(provider: IntegrationProvider, model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  const path = configPath(os, "~/.pi/agent/models.json", "%USERPROFILE%\\.pi\\agent\\models.json");
  const modelRef = `apitoken/${model.id}`;
  const requirement = isWindows(os)
    ? localize(language, "Pi requires Git Bash on Windows. Install Git for Windows first; Pi detects `bash.exe` automatically.", "На Windows Pi нужен Git Bash. Сначала установите Git for Windows — Pi сам найдёт `bash.exe`.")
    : undefined;
  return {
    title: `Pi · ${model.name}`,
    summary: localize(language, "Minimal terminal coding harness with a custom model catalog.", "Минималистичный coding harness в терминале со своим каталогом моделей."),
    endpoint: PROVIDER_ENDPOINTS[provider],
    requirement,
    steps: [
      {
        title: localize(language, "Add the model catalog", "Добавьте каталог модели"),
        text: localize(language, `Save as \`${path}\`. Pi reloads this file when the model picker opens.`, `Сохраните как \`${path}\`. Pi перечитывает файл при открытии выбора модели.`),
        code: piConfig(provider, model),
        codeLabel: path,
      },
      {
        title: localize(language, "Set the key and start", "Задайте ключ и запустите"),
        text: localize(language, "The selected model is addressed as provider/model, so other Pi logins remain available.", "Модель выбирается как provider/model, поэтому остальные логины Pi останутся доступны."),
        code: `${environmentCommand(os, { APITOKEN_API_KEY: keyPlaceholder })}\n\npi --model ${modelRef}`,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
      {
        title: localize(language, "Verify in Pi", "Проверьте в Pi"),
        text: localize(language, "Open the model picker to confirm the exact provider and model, then send the test prompt.", "Откройте выбор модели, проверьте provider и model, затем отправьте тестовый запрос."),
        code: `/model\n\nReply with exactly: connected`,
        codeLabel: localize(language, "Inside Pi", "В Pi"),
      },
    ],
  };
}

function hermesInstall(os: IntegrationOs): string {
  if (os === "powershell") return "iex (irm https://hermes-agent.nousresearch.com/install.ps1)";
  if (os === "cmd") return `powershell -NoProfile -ExecutionPolicy Bypass -Command "iex (irm 'https://hermes-agent.nousresearch.com/install.ps1')"`;
  return "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash";
}

function hermesGuide(model: IntegrationModel, os: IntegrationOs, language: IntegrationLanguage): IntegrationGuide {
  return {
    title: `Hermes · ${model.name}`,
    summary: localize(language, "Advanced general agent with terminal, coding tools, memory, and automation.", "Advanced‑агент с терминалом, coding tools, памятью и автоматизациями."),
    endpoint: OPENAI_BASE_URL,
    requirement: localize(language, "Hermes is broader than a coding-only harness. Choose it when you also need persistent memory, messaging, or automations.", "Hermes шире обычного coding harness. Выбирайте его, если нужны ещё память, мессенджеры или автоматизации."),
    securityNote: localize(language, "Hermes stores the credential in `~/.hermes`. Keep that file private and never commit it.", "Hermes сохраняет ключ в `~/.hermes`. Держите эту папку приватной и не коммитьте её."),
    steps: [
      {
        title: localize(language, "Install Hermes", "Установите Hermes"),
        text: localize(language, "Use the official installer for the selected operating system. Skip this step if Hermes is already installed.", "Используйте официальный installer для выбранной ОС. Пропустите шаг, если Hermes уже установлен."),
        code: hermesInstall(os),
        codeLabel: localize(language, "Official installer", "Официальный installer"),
      },
      {
        title: localize(language, "Choose Custom endpoint", "Выберите Custom endpoint"),
        text: localize(language, "Run the model wizard and enter these values. Hermes verifies /models before saving them.", "Запустите мастер моделей и введите эти значения. Перед сохранением Hermes проверит /models."),
        code: `hermes model\n\nProvider: Custom endpoint (self-hosted / VLLM / etc.)\nBase URL: ${OPENAI_BASE_URL}\nAPI mode: Chat Completions\nModel: ${model.id}\nAPI key: ${keyPlaceholder}`,
        codeLabel: localize(language, "Model wizard", "Мастер моделей"),
      },
      {
        title: localize(language, "Diagnose and start", "Проверьте и запустите"),
        text: localize(language, "Doctor checks the installation and active provider before the interactive agent starts.", "Doctor проверит установку и активного провайдера до запуска интерактивного агента."),
        code: `hermes doctor\nhermes`,
        codeLabel: localize(language, "Terminal", "Терминал"),
      },
    ],
  };
}

export function isToolCompatible(tool: IntegrationTool, provider: IntegrationProvider): boolean {
  return TOOL_COMPATIBILITY[tool].includes(provider);
}

export function buildIntegrationGuide({
  provider,
  tool,
  os,
  modelId,
  language,
}: {
  provider: IntegrationProvider;
  tool: IntegrationTool;
  os: IntegrationOs;
  modelId: string;
  language: IntegrationLanguage;
}): IntegrationGuide {
  if (!isToolCompatible(tool, provider)) throw new Error(`${TOOL_NAMES[tool]} does not support ${provider}`);
  const model = INTEGRATION_MODELS[provider].find((candidate) => candidate.id === modelId);
  if (!model) throw new Error(`Unknown ${provider} model: ${modelId}`);

  if (tool === "claude-code") return claudeCodeGuide(model, os, language);
  if (tool === "codex") return codexGuide(model, os, language);
  if (tool === "gemini-cli") return geminiCliGuide(model, os, language);
  if (tool === "opencode") return openCodeGuide(provider, model, os, language);
  if (tool === "pi") return piGuide(provider, model, os, language);
  return hermesGuide(model, os, language);
}
