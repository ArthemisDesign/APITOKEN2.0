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
  const namespace = provider === "anthropic"
    ? "anthropic"
    : provider === "gemini"
      ? "google"
      : provider === "kimi"
        ? "kimi"
        : "openai";
  return `${namespace}/${modelId}`;
}

/**
 * The wire protocol behind a provider. KIMI has no dialect of its own: it is served over the
 * Anthropic Messages protocol, so every native example, credential scheme and SDK below is the
 * Anthropic one. Branching on the provider with an `else → openai` tail would have documented
 * the wrong client for it.
 */
function protocolOf(provider: IntegrationProvider): "anthropic" | "openai" | "gemini" {
  return provider === "kimi" ? "anthropic" : provider;
}

function endpointFor(provider: IntegrationProvider, style: ApiStyle): string {
  if (style === "openai-compatible") return ROUTER_OPENAI_BASE_URL;
  return protocolOf(provider) === "openai" ? ROUTER_OPENAI_BASE_URL : ROUTER_BASE_URL;
}

function authFor(provider: IntegrationProvider, style: ApiStyle): string {
  if (style === "openai-compatible") return "Authorization: Bearer";
  if (protocolOf(provider) === "anthropic") return "x-api-key · anthropic-version";
  if (protocolOf(provider) === "gemini") return "x-goog-api-key";
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

function imageRequestCode(apiLanguage: ApiLanguage): string {
  const prompt = "A watercolor lighthouse at dawn";
  if (apiLanguage === "curl") {
    return `curl ${ROUTER_OPENAI_BASE_URL}/images/generations \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-image-2",
    "prompt": "${prompt}"
  }'`;
  }
  if (apiLanguage === "python") {
    return `import base64
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="${ROUTER_OPENAI_BASE_URL}",
)

image = client.images.generate(
    model="gpt-image-2",
    prompt="${prompt}",
)

with open("image.png", "wb") as f:
    f.write(base64.b64decode(image.data[0].b64_json))`;
  }
  return `import { writeFile } from "node:fs/promises";
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env["APITOKEN_API_KEY"],
  baseURL: "${ROUTER_OPENAI_BASE_URL}",
});

const image = await client.images.generate({
  model: "gpt-image-2",
  prompt: "${prompt}",
});

await writeFile("image.png", Buffer.from(image.data[0].b64_json!, "base64"));`;
}

function imageMaskRequestCode(apiLanguage: ApiLanguage): string {
  if (apiLanguage === "curl") {
    return `curl ${ROUTER_OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.6-sol",
    "input": [{
      "role": "user",
      "content": [
        {"type": "input_text", "text": "Change only the masked region."},
        {"type": "input_image", "image_url": "data:image/png;base64,SOURCE_PNG"}
      ]
    }],
    "tools": [{
      "type": "image_generation",
      "input_image_mask": {"image_url": "data:image/png;base64,MASK_PNG"}
    }]
  }'`;
  }
  if (apiLanguage === "python") {
    return `import base64, os
from pathlib import Path
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="${ROUTER_OPENAI_BASE_URL}",
)

def png_url(path):
    return "data:image/png;base64," + base64.b64encode(Path(path).read_bytes()).decode()

response = client.responses.create(
    model="gpt-5.6-sol",
    input=[{
        "role": "user",
        "content": [
            {"type": "input_text", "text": "Change only the masked region."},
            {"type": "input_image", "image_url": png_url("photo.png")},
        ],
    }],
    tools=[{
        "type": "image_generation",
        "input_image_mask": {"image_url": png_url("mask.png")},
    }],
)`;
  }
  return `import { readFile } from "node:fs/promises";
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: process.env["APITOKEN_API_KEY"],
  baseURL: "${ROUTER_OPENAI_BASE_URL}",
});

const pngUrl = async (path: string) =>
  "data:image/png;base64," + (await readFile(path)).toString("base64");

const response = await client.responses.create({
  model: "gpt-5.6-sol",
  input: [{
    role: "user",
    content: [
      { type: "input_text", text: "Change only the masked region." },
      { type: "input_image", image_url: await pngUrl("photo.png") },
    ],
  }],
  tools: [{
    type: "image_generation",
    input_image_mask: { image_url: await pngUrl("mask.png") },
  }],
});`;
}

function installStep(provider: IntegrationProvider, style: ApiStyle, apiLanguage: ApiLanguage, language: IntegrationLanguage): ApiStep | null {
  if (apiLanguage === "curl") return null;
  const native = style === "native";
  const pkg = apiLanguage === "python"
    ? (native && protocolOf(provider) === "anthropic"
        ? "anthropic"
        : native && protocolOf(provider) === "gemini" ? "google-genai" : "openai")
    : (native && protocolOf(provider) === "anthropic"
        ? "@anthropic-ai/sdk"
        : native && protocolOf(provider) === "gemini" ? "@google/genai" : "openai");
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
  if (provider === "kimi") {
    return localize(language,
      "KIMI on the unified endpoint, spoken as Anthropic Messages — the same protocol and the same sk-pool key in `x-api-key`, with the model addressed under the `kimi/` namespace.",
      "KIMI на едином endpoint по протоколу Anthropic Messages — тот же протокол и тот же ключ sk-pool в `x-api-key`, модель адресуется в namespace `kimi/`.");
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
  const providerName = provider === "anthropic"
    ? "Claude"
    : provider === "gemini"
      ? "Gemini"
      : provider === "kimi"
        ? "Kimi"
        : "GPT";
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

  // GPT Image 2 images are a native OpenAI lane on the unified router.
  if (provider === "openai" && native) {
    steps.push({
      title: localize(language, "Generate an image with GPT Image 2", "Сгенерируйте изображение через GPT Image 2"),
      text: localize(language,
        `Images run on the same unified endpoint: \`POST ${ROUTER_OPENAI_BASE_URL}/images/generations\` returns one non-streaming base64 PNG billed to the same prepaid balance. The proved contract is deliberately narrow — omit background/quality/size, or send only background=opaque, quality=low, size=auto (an explicit "auto" for background or quality is rejected with 400). To edit, send multipart \`POST /v1/images/edits\` with up to five reference PNGs. The legacy host openai.api.apitoken.sale serves the same routes.`,
        `Изображения работают через тот же единый endpoint: \`POST ${ROUTER_OPENAI_BASE_URL}/images/generations\` возвращает один непотоковый base64 PNG и списывает тот же предоплатный баланс. Доказанный контракт намеренно узкий — не передавайте background/quality/size вовсе либо отправляйте только background=opaque, quality=low, size=auto (явное "auto" для background или quality отклоняется с 400). Для редактирования отправьте multipart \`POST /v1/images/edits\` с одним–пятью reference PNG. Legacy-хост openai.api.apitoken.sale обслуживает те же маршруты.`),
      code: imageRequestCode(apiLanguage),
      codeLabel: apiLanguage === "curl" ? "HTTP" : languageName,
    });
    steps.push({
      title: localize(language, "Inpaint a region with a PNG mask", "Закрасьте область по PNG-маске"),
      text: localize(language,
        `This is not \`POST /v1/images/edits\` with a multipart \`mask\` field — that field is rejected. Region inpaint is a Responses call: send a GPT text model (for example \`gpt-5.6-sol\`) to \`POST ${ROUTER_OPENAI_BASE_URL}/responses\`, put the source PNG in \`input\` as \`input_image\`, and add \`tools: [{type:"image_generation", input_image_mask:{image_url}}]\`. Both URLs must be \`data:image/png;base64,…\`. The mask must match the source size; transparent pixels are the region to change. \`file_id\` masks are not supported (no Files API). Billing follows image tokens; the mask is not a second reference image.`,
        `Это не \`POST /v1/images/edits\` с multipart-полем \`mask\` — это поле отклоняется. Inpaint — вызов Responses: отправьте текстовую GPT-модель (например \`gpt-5.6-sol\`) на \`POST ${ROUTER_OPENAI_BASE_URL}/responses\`, исходный PNG в \`input\` как \`input_image\`, и \`tools: [{type:"image_generation", input_image_mask:{image_url}}]\`. Оба URL — \`data:image/png;base64,…\`. Маска того же размера, что исходник; прозрачные пиксели — зона правки. \`file_id\` не поддерживается (нет Files API). Биллинг по image-токенам; маска не вторая reference-картинка.`),
      code: imageMaskRequestCode(apiLanguage),
      codeLabel: apiLanguage === "curl" ? "HTTP" : languageName,
    });
  }

  return {
    title: `${providerName} · ${native ? "Native API" : "OpenAI-compatible"} · ${languageName}`,
    summary: summaryFor(provider, apiStyle, language),
    endpoint: endpointFor(provider, apiStyle),
    auth: authFor(provider, apiStyle),
    steps,
  };
}
