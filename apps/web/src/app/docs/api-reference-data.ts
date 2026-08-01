// Interactive API reference: one guide per provider × programming language.
// Reuses the builder's endpoint constants and model catalog so the examples
// can never drift away from what the gateway actually serves.
import {
  ANTHROPIC_BASE_URL,
  GEMINI_BASE_URL,
  INTEGRATION_MODELS,
  OPENAI_BASE_URL,
  type IntegrationLanguage,
  type IntegrationProvider,
} from "./integration-builder-data";

export type ApiLanguage = "curl" | "python" | "typescript";

export type ApiStep = {
  title: string;
  text: string;
  code: string;
  codeLabel?: string;
};

export type ApiGuide = {
  title: string;
  summary: string;
  endpoint: string;
  auth: string;
  steps: ApiStep[];
};

const keyPlaceholder = "sk-pool-•••";

function localize(language: IntegrationLanguage, en: string, ru: string): string {
  return language === "ru" ? ru : en;
}

function endpointFor(provider: IntegrationProvider): string {
  if (provider === "anthropic") return ANTHROPIC_BASE_URL;
  if (provider === "gemini") return GEMINI_BASE_URL;
  return OPENAI_BASE_URL;
}

function authFor(provider: IntegrationProvider): string {
  if (provider === "anthropic") return "x-api-key · anthropic-version";
  if (provider === "gemini") return "x-goog-api-key";
  return "Authorization: Bearer";
}

function requestCode(provider: IntegrationProvider, apiLanguage: ApiLanguage, modelId: string): string {
  const prompt = "Reply with exactly: connected";
  if (provider === "anthropic") {
    if (apiLanguage === "curl") {
      return `curl ${ANTHROPIC_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "${modelId}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "${prompt}"}]
  }'`;
    }
    if (apiLanguage === "python") {
      return `import os
from anthropic import Anthropic

client = Anthropic(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="${ANTHROPIC_BASE_URL}",
)

message = client.messages.create(
    model="${modelId}",
    max_tokens=1024,
    messages=[{"role": "user", "content": "${prompt}"}],
)

for block in message.content:
    if block.type == "text":
        print(block.text)`;
    }
    return `import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  apiKey: process.env["APITOKEN_API_KEY"],
  baseURL: "${ANTHROPIC_BASE_URL}",
});

const message = await client.messages.create({
  model: "${modelId}",
  max_tokens: 1024,
  messages: [{ role: "user", content: "${prompt}" }],
});

for (const block of message.content) {
  if (block.type === "text") console.log(block.text);
}`;
  }
  if (provider === "gemini") {
    if (apiLanguage === "curl") {
      return `curl ${GEMINI_BASE_URL}/v1beta/models/${modelId}:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "contents": [{"parts": [{"text": "${prompt}"}]}]
  }'`;
    }
    if (apiLanguage === "python") {
      return `import os
from google import genai
from google.genai import types

client = genai.Client(
    api_key=os.environ["APITOKEN_API_KEY"],
    http_options=types.HttpOptions(base_url="${GEMINI_BASE_URL}"),
)

response = client.models.generate_content(
    model="${modelId}",
    contents="${prompt}",
)
print(response.text)`;
    }
    return `import { GoogleGenAI } from "@google/genai";

const ai = new GoogleGenAI({
  apiKey: process.env["APITOKEN_API_KEY"],
  httpOptions: { baseUrl: "${GEMINI_BASE_URL}" },
});

const response = await ai.models.generateContent({
  model: "${modelId}",
  contents: "${prompt}",
});
console.log(response.text);`;
  }
  if (apiLanguage === "curl") {
    return `curl ${OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "${modelId}",
    "input": "${prompt}"
  }'`;
  }
  if (apiLanguage === "python") {
    return `import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="${OPENAI_BASE_URL}",
)

response = client.responses.create(
    model="${modelId}",
    input="${prompt}",
)
print(response.output_text)`;
  }
  return `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env["APITOKEN_API_KEY"],
  baseURL: "${OPENAI_BASE_URL}",
});

const response = await client.responses.create({
  model: "${modelId}",
  input: "${prompt}",
});

console.log(response.output_text);`;
}

function installStep(provider: IntegrationProvider, apiLanguage: ApiLanguage, language: IntegrationLanguage): ApiStep | null {
  if (apiLanguage === "curl") return null;
  const pkg = apiLanguage === "python"
    ? (provider === "anthropic" ? "anthropic" : provider === "gemini" ? "google-genai" : "openai")
    : (provider === "anthropic" ? "@anthropic-ai/sdk" : provider === "gemini" ? "@google/genai" : "openai");
  const command = apiLanguage === "python" ? `pip install ${pkg}` : `npm install ${pkg}`;
  return {
    title: localize(language,
      apiLanguage === "python" ? "Install the Python SDK" : "Install the TypeScript SDK",
      apiLanguage === "python" ? "Установите Python SDK" : "Установите TypeScript SDK"),
    text: localize(language,
      "The official SDK works as-is — only the base URL and key change.",
      "Официальный SDK работает без изменений — меняются только base URL и ключ."),
    code: command,
    codeLabel: localize(language, "Terminal", "Терминал"),
  };
}

export function buildApiGuide({
  provider,
  apiLanguage,
  language,
}: {
  provider: IntegrationProvider;
  apiLanguage: ApiLanguage;
  language: IntegrationLanguage;
}): ApiGuide {
  const model = INTEGRATION_MODELS[provider][0];
  const providerName = provider === "anthropic" ? "Claude" : provider === "gemini" ? "Gemini" : "GPT";
  const languageName = apiLanguage === "curl" ? "cURL" : apiLanguage === "python" ? "Python" : "TypeScript";

  const steps: ApiStep[] = [
    {
      title: localize(language, "Store the key in the environment", "Сохраните ключ в окружении"),
      text: localize(language,
        "Keep the key server-side: an environment variable or a secret manager, never a browser bundle. On Windows PowerShell use `$env:APITOKEN_API_KEY` instead.",
        "Держите ключ на сервере: переменная окружения или менеджер секретов, не браузерный bundle. В Windows PowerShell используйте `$env:APITOKEN_API_KEY`."),
      code: `export APITOKEN_API_KEY="${keyPlaceholder}"`,
      codeLabel: localize(language, "Terminal", "Терминал"),
    },
  ];
  const install = installStep(provider, apiLanguage, language);
  if (install) steps.push(install);
  steps.push({
    title: localize(language, "Send the first request", "Отправьте первый запрос"),
    text: localize(language,
      `Every available ${providerName} model answers on this route. Discover the current list with \`GET /models\` instead of hardcoding IDs.`,
      `Все доступные модели ${providerName} отвечают на этом маршруте. Актуальный список получайте через \`GET /models\`, а не зашивайте ID в код.`),
    code: requestCode(provider, apiLanguage, model.id),
    codeLabel: apiLanguage === "curl" ? "HTTP" : languageName,
  });

  return {
    title: `${providerName} API · ${languageName}`,
    summary: provider === "anthropic"
      ? localize(language,
        "Anthropic Messages API with your sk-pool key in `x-api-key`. SDKs add `anthropic-version` automatically.",
        "Anthropic Messages API с ключом sk-pool в `x-api-key`. SDK добавляют `anthropic-version` автоматически.")
      : provider === "gemini"
        ? localize(language,
          "Native Google Gemini API with the same sk-pool key in `x-goog-api-key`.",
          "Нативный Google Gemini API с тем же ключом sk-pool в `x-goog-api-key`.")
        : localize(language,
          "OpenAI-compatible Responses API with the same sk-pool key as `Authorization: Bearer`.",
          "OpenAI-совместимый Responses API с тем же ключом sk-pool в `Authorization: Bearer`."),
    endpoint: endpointFor(provider),
    auth: authFor(provider),
    steps,
  };
}
