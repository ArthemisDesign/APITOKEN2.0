// Interactive API reference: one guide per provider × API style × programming language.
// Reuses the builder's endpoint constants and model catalog so the examples
// can never drift away from what the gateway actually serves.
import {
  INTEGRATION_MODELS,
  ROUTER_BASE_URL,
  ROUTER_OPENAI_BASE_URL,
  type IntegrationLanguage,
  type IntegrationProvider,
} from "./integration-builder-data";

export type ApiLanguage = "curl" | "python" | "typescript";
export type ApiStyle = "native" | "openai-compatible";

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

// The unified catalog publishes namespaced IDs (anthropic/*, openai/*, google/*).
// On the OpenAI-compatible lane they are the safe way to address a model of any
// provider; bare native IDs work only while they are globally unambiguous.
export function namespacedModelId(provider: IntegrationProvider, modelId: string): string {
  const namespace = provider === "anthropic" ? "anthropic" : provider === "gemini" ? "google" : "openai";
  return `${namespace}/${modelId}`;
}

function endpointFor(provider: IntegrationProvider, style: ApiStyle): string {
  if (style === "openai-compatible") return ROUTER_OPENAI_BASE_URL;
  return provider === "openai" ? ROUTER_OPENAI_BASE_URL : ROUTER_BASE_URL;
}

function authFor(provider: IntegrationProvider, style: ApiStyle): string {
  if (style === "openai-compatible") return "Authorization: Bearer";
  if (provider === "anthropic") return "x-api-key · anthropic-version";
  if (provider === "gemini") return "x-goog-api-key";
  return "Authorization: Bearer";
}

function nativeRequestCode(provider: IntegrationProvider, apiLanguage: ApiLanguage, modelId: string): string {
  const prompt = "Reply with exactly: connected";
  if (provider === "anthropic") {
    if (apiLanguage === "curl") {
      return `curl ${ROUTER_BASE_URL}/v1/messages \\
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
    base_url="${ROUTER_BASE_URL}",
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
  baseURL: "${ROUTER_BASE_URL}",
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
      return `curl ${ROUTER_BASE_URL}/v1beta/models/${modelId}:generateContent \\
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
    http_options=types.HttpOptions(base_url="${ROUTER_BASE_URL}"),
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
  httpOptions: { baseUrl: "${ROUTER_BASE_URL}" },
});

const response = await ai.models.generateContent({
  model: "${modelId}",
  contents: "${prompt}",
});
console.log(response.text);`;
  }
  if (apiLanguage === "curl") {
    return `curl ${ROUTER_OPENAI_BASE_URL}/responses \\
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
    base_url="${ROUTER_OPENAI_BASE_URL}",
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
  baseURL: "${ROUTER_OPENAI_BASE_URL}",
});

const response = await client.responses.create({
  model: "${modelId}",
  input: "${prompt}",
});

console.log(response.output_text);`;
}

function compatibleRequestCode(provider: IntegrationProvider, apiLanguage: ApiLanguage, modelId: string): string {
  const prompt = "Reply with exactly: connected";
  const catalogId = namespacedModelId(provider, modelId);
  if (apiLanguage === "curl") {
    return `curl ${ROUTER_OPENAI_BASE_URL}/chat/completions \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "${catalogId}",
    "messages": [{"role": "user", "content": "${prompt}"}]
  }'`;
  }
  if (apiLanguage === "python") {
    return `import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="${ROUTER_OPENAI_BASE_URL}",
)

response = client.chat.completions.create(
    model="${catalogId}",
    messages=[{"role": "user", "content": "${prompt}"}],
)
print(response.choices[0].message.content)`;
  }
  return `import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env["APITOKEN_API_KEY"],
  baseURL: "${ROUTER_OPENAI_BASE_URL}",
});

const response = await client.chat.completions.create({
  model: "${catalogId}",
  messages: [{ role: "user", content: "${prompt}" }],
});

console.log(response.choices[0]?.message.content);`;
}

function installStep(provider: IntegrationProvider, style: ApiStyle, apiLanguage: ApiLanguage, language: IntegrationLanguage): ApiStep | null {
  if (apiLanguage === "curl") return null;
  const native = style === "native";
  const pkg = apiLanguage === "python"
    ? (native && provider === "anthropic" ? "anthropic" : native && provider === "gemini" ? "google-genai" : "openai")
    : (native && provider === "anthropic" ? "@anthropic-ai/sdk" : native && provider === "gemini" ? "@google/genai" : "openai");
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

function summaryFor(provider: IntegrationProvider, style: ApiStyle, language: IntegrationLanguage): string {
  if (style === "openai-compatible") {
    return localize(language,
      "One OpenAI-compatible route for every catalog model — any provider, any OpenAI client. Address the model by its namespaced catalog ID; unsupported parameters fail closed with a clear 400 instead of being silently dropped.",
      "Один OpenAI-совместимый маршрут для всех моделей каталога — любой провайдер, любой OpenAI-клиент. Модель указывайте namespaced ID из каталога; неподдерживаемые параметры fail-closed с понятным 400, а не молча отбрасываются.");
  }
  if (provider === "anthropic") {
    return localize(language,
      "Anthropic Messages API on the unified endpoint — byte-faithful protocol with your sk-pool key in `x-api-key`. SDKs add `anthropic-version` automatically.",
      "Anthropic Messages API на едином endpoint — протокол байт-в-байт, ключ sk-pool в `x-api-key`. SDK добавляют `anthropic-version` автоматически.");
  }
  if (provider === "gemini") {
    return localize(language,
      "Native Google Gemini API on the unified endpoint — protocol unchanged, same sk-pool key in `x-goog-api-key`.",
      "Нативный Google Gemini API на едином endpoint — протокол без изменений, тот же ключ sk-pool в `x-goog-api-key`.");
  }
  return localize(language,
    "OpenAI Responses API on the unified endpoint — native wire format with the same sk-pool key as `Authorization: Bearer`.",
    "OpenAI Responses API на едином endpoint — нативный wire format с тем же ключом sk-pool в `Authorization: Bearer`.");
}

export function buildApiGuide({
  provider,
  apiStyle,
  apiLanguage,
  language,
}: {
  provider: IntegrationProvider;
  apiStyle: ApiStyle;
  apiLanguage: ApiLanguage;
  language: IntegrationLanguage;
}): ApiGuide {
  const model = INTEGRATION_MODELS[provider][0];
  const providerName = provider === "anthropic" ? "Claude" : provider === "gemini" ? "Gemini" : "GPT";
  const languageName = apiLanguage === "curl" ? "cURL" : apiLanguage === "python" ? "Python" : "TypeScript";
  const native = apiStyle === "native";

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
  const install = installStep(provider, apiStyle, apiLanguage, language);
  if (install) steps.push(install);
  steps.push({
    title: localize(language, "Send the first request", "Отправьте первый запрос"),
    text: native
      ? localize(language,
        `Every available ${providerName} model answers on this route. Discover the current list with \`GET /v1/models\` on the same endpoint instead of hardcoding IDs.`,
        `Все доступные модели ${providerName} отвечают на этом маршруте. Актуальный список получайте через \`GET /v1/models\` на том же endpoint, а не зашивайте ID в код.`)
      : localize(language,
        `This one route serves every provider in the catalog — ${providerName} here. Swap the namespaced model ID for any entry from \`GET /v1/models\` without changing code or keys.`,
        `Этот единый маршрут обслуживает всех провайдеров каталога — здесь ${providerName}. Подставьте namespaced ID любой модели из \`GET /v1/models\` — код и ключ не меняются.`),
    code: native
      ? nativeRequestCode(provider, apiLanguage, model.id)
      : compatibleRequestCode(provider, apiLanguage, model.id),
    codeLabel: apiLanguage === "curl" ? "HTTP" : languageName,
  });

  return {
    title: `${providerName} · ${native ? "Native API" : "OpenAI-compatible"} · ${languageName}`,
    summary: summaryFor(provider, apiStyle, language),
    endpoint: endpointFor(provider, apiStyle),
    auth: authFor(provider, apiStyle),
    steps,
  };
}
