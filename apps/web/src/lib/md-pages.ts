// Machine-readable Markdown for the core sections, generated from the same data as the HTML pages.
// Served by static route handlers under /md so crawlers and AI agents can consume every public
// section as clean text. Add a model to the data and its Markdown updates on the next build.
import {
  ANTHROPIC_BASE_URL as LEGACY_ANTHROPIC_BASE_URL,
  GEMINI_BASE_URL as LEGACY_GEMINI_BASE_URL,
  OPENAI_BASE_URL as LEGACY_OPENAI_BASE_URL,
  ROUTER_BASE_URL,
  ROUTER_OPENAI_BASE_URL,
  catalogModelBySlug,
  claudeModels,
  formatUsd,
  DISCOUNT_FLAT,
  geminiModels,
  modelPath,
  openaiModels,
  type CatalogModel,
} from "./models";
import { B2C_DISCOUNT_PERCENT, B2C_VALUE_MULTIPLIER } from "./pricing-tiers";
import { integrationGuideSeo, SITE_ORIGIN, type IntegrationGuideSlug } from "./seo";
import { API_ERRORS } from "./api-errors";

// Every machine-readable page recommends the unified router endpoint; the
// protocol-specific instructions below stay valid because each lane keeps its
// wire format on the router. Legacy per-provider hosts are referenced
// explicitly where the text acknowledges them.
const API_BASE_URL = ROUTER_BASE_URL;
const OPENAI_BASE_URL = ROUTER_OPENAI_BASE_URL;
const GEMINI_BASE_URL = ROUTER_BASE_URL;

// Per-tool setup facts an agent needs; titles/descriptions come from integrationGuideSeo.
const INTEGRATION_CONFIG: Record<IntegrationGuideSlug, string> = {
  "claude-code": `export ANTHROPIC_BASE_URL=${API_BASE_URL}
export ANTHROPIC_API_KEY=sk-pool-…
# then run: claude`,
  codex: `# ~/.codex/apitoken.config.toml
model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "${OPENAI_BASE_URL}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"

# keep the key in your shell, then pick the profile:
export APITOKEN_API_KEY=sk-pool-…
codex --profile apitoken`,
  cursor: `Cursor Settings → Models → enable "Override Anthropic Base URL"
Base URL: ${API_BASE_URL}
API key:  sk-pool-…   ·   Model: any Claude model (e.g. claude-opus-4-8)`,
  cline: `Cline settings:
API Provider: Anthropic
Base URL:     ${API_BASE_URL}
API Key:      sk-pool-…
Model:        claude-opus-4-8`,
  opencode: `# one-click setup: installs the router plugin (live model catalog,
# limits, pricing) and merges the apitoken provider into
# ~/.config/opencode/opencode.jsonc — asks for your sk-pool-… key,
# backs up an existing config, touches nothing else
curl -fsSL https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh | bash

# verify (models are namespaced: apitoken/<provider>/<model>)
opencode run --model apitoken/openai/gpt-5.6-sol "Reply with exactly: connected"

# manual alternative — opencode.json provider block:
# {
#   "provider": {
#     "apitoken": {
#       "npm": "@ai-sdk/openai-compatible",
#       "name": "apiToken.sale",
#       "options": {
#         "baseURL": "${OPENAI_BASE_URL}",
#         "apiKey": "{env:APITOKEN_API_KEY}"
#       },
#       "models": {
#         "gpt-5.6-sol": { "name": "GPT-5.6 Sol" }
#       }
#     }
#   }
# }`,
  continue: `// ~/.continue/config.json
{
  "models": [{
    "title": "Claude via apiToken.sale",
    "provider": "anthropic",
    "apiBase": "${API_BASE_URL}",
    "apiKey": "sk-pool-…",
    "model": "claude-opus-4-8"
  }]
}`,
  zed: `// Zed settings.json
{
  "language_models": {
    "anthropic": { "api_url": "${API_BASE_URL}" }
  }
}`,
  sdk: `from anthropic import Anthropic
client = Anthropic(base_url="${API_BASE_URL}", api_key="sk-pool-…")
msg = client.messages.create(
    model="claude-opus-4-8",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)`,
};

// Which API surface a guide targets, so the steps name the right base URL and model family.
const INTEGRATION_SURFACE: Record<IntegrationGuideSlug, "anthropic" | "openai"> = {
  "claude-code": "anthropic",
  codex: "openai",
  cursor: "anthropic",
  cline: "anthropic",
  opencode: "openai",
  continue: "anthropic",
  zed: "anthropic",
  sdk: "anthropic",
};

function frontmatter(fields: Record<string, string>): string {
  const lines = Object.entries(fields).map(([key, value]) => `${key}: ${JSON.stringify(value)}`);
  return ["---", ...lines, "---", ""].join("\n");
}

function pct(discount: number): string {
  return `${Math.round(discount * 100)}%`;
}

/** Purpose-built runbook for an AI agent that must configure an unknown user environment safely. */
export function buildAgentSetupMarkdown(): string {
  const anthropicModelIds = claudeModels.map((model) => `- \`${model.id}\` — ${model.name}`).join("\n");
  const openAiModelIds = openaiModels.map((model) => `- \`${model.id}\` — ${model.name}`).join("\n");
  const geminiModelIds = geminiModels.map((model) => `- \`${model.id}\` — ${model.name}`).join("\n");

  return (
    frontmatter({
      title: "Connect apiToken.sale — execution guide for AI agents",
      description:
        "An operational runbook for connecting any supported client, IDE, CLI or SDK to apiToken.sale across Windows, macOS and Linux, including API selection, secure key handling, verification and diagnostics.",
      url: `${SITE_ORIGIN}/docs#agent-setup`,
      language: "en",
    }) +
    `# Connect apiToken.sale — execution guide for AI agents

You are reading the canonical setup contract for an AI agent. Your task is to connect the user's existing project or tool to apiToken.sale with the smallest safe change, verify a real response, and report exactly what you changed. Do not make the user choose a provider surface or manually translate configuration fields when you can infer them.

This guide owns the changing setup facts. The short instruction copied from the website should remain stable as models and compatible tools are added.

## Definition of done

The connection is complete only when all of the following are true:

1. You identified the operating system, shell, client or SDK, runtime, and existing provider configuration.
2. You selected a protocol the client natively supports and an exact model ID that is currently available.
3. The API key is stored in an environment variable or secret store, never in tracked source code.
4. A minimal real API request succeeded through apiToken.sale.
5. You told the user which files or settings changed, which endpoint and model are active, and how to revert the setup. Never repeat the full key in the report.

## Safety rules

- Inspect before editing. Read the relevant configuration and \`.gitignore\`; do not rewrite unrelated files or replace an existing provider setup without explaining the impact.
- Never print, log, transmit, commit, or paste a full \`sk-pool-…\` key into chat, screenshots, issue trackers, browser code, mobile apps, or client-side bundles.
- Prefer a server-side environment variable or the tool's protected secret storage. A project \`.env.local\` is acceptable only when it is ignored by version control. Put variable names, never values, in \`.env.example\`.
- If the key is missing, ask the user to create it at ${SITE_ORIGIN}/dashboard?view=keys and enter it through the local tool or shell. Do not ask them to send the full key to support.
- Do not make destructive repository operations, install unrelated packages, or change the project's model behavior beyond what connection requires.
- Verification makes a billable real request. Keep it minimal and tell the user before running anything unusually expensive.

## Step 1 — inspect the environment

Determine, without guessing:

- OS and version: Windows, macOS, Linux, WSL, container, or remote development host.
- Active shell: PowerShell, cmd.exe, bash, zsh, fish, or another shell.
- Target: IDE, coding agent, CLI, application, framework, raw HTTP client, or SDK and its exact version.
- Runtime and package manager when relevant: Node.js/npm/pnpm/yarn, Python/pip/uv/poetry, Go, Rust, Java, .NET, Docker, or CI.
- Existing model provider, base URL, environment-variable names, proxy settings, and configuration scope (user, workspace, project, CI, or production).
- Whether the client supports Anthropic Messages, OpenAI Responses, OpenAI Chat Completions, the native Gemini API, or more than one of them.

Use the remote/container OS that actually runs the client, not merely the desktop OS hosting the editor.

## Step 2 — choose the compatible API surface

Choose by the client's wire protocol, not by brand preference. All protocols live on one unified router endpoint; the per-protocol base URLs below differ only by path suffix.

### Anthropic Messages surface

- Base URL: \`${API_BASE_URL}\` — do not append \`/v1\` when an SDK asks for a base URL.
- Request endpoint: \`POST ${API_BASE_URL}/v1/messages\`.
- Direct HTTP auth: \`x-api-key: sk-pool-…\`.
- Direct HTTP version header: \`anthropic-version: 2023-06-01\`.
- Official Anthropic SDKs normally add the version header themselves.
- Use this surface for Claude Code, Anthropic SDKs, and tools whose custom-provider mode is Anthropic-compatible.
- Standard Messages request bodies, response objects, SSE streaming, tools, prompt caching, vision, and Anthropic error envelopes pass through unchanged.

### OpenAI-compatible surface

- Base URL: \`${OPENAI_BASE_URL}\` — this base already includes \`/v1\`; do not produce \`/v1/v1\`.
- Responses: \`POST ${OPENAI_BASE_URL}/responses\`.
- Chat Completions: \`POST ${OPENAI_BASE_URL}/chat/completions\`.
- Model discovery: \`GET ${OPENAI_BASE_URL}/models\`.
- Auth: \`Authorization: Bearer sk-pool-…\`; do not use \`x-api-key\` on this surface.
- Use this surface for Codex, OpenAI SDKs, and tools with an OpenAI-compatible custom provider.
- Responses and Chat Completions support SSE streaming and text or image input with text output. This endpoint does not provide the unrelated OpenAI Platform services such as audio, realtime, assistants, batches, files, or fine-tuning.

### Gemini surface (native Google API)

- Base URL: \`${GEMINI_BASE_URL}\`.
- Generate: \`POST ${GEMINI_BASE_URL}/v1beta/models/{model}:generateContent\`.
- Streaming: \`POST ${GEMINI_BASE_URL}/v1beta/models/{model}:streamGenerateContent\`.
- Token counting: \`POST ${GEMINI_BASE_URL}/v1beta/models/{model}:countTokens\`.
- Model discovery: \`GET ${GEMINI_BASE_URL}/v1beta/models\`.
- Auth: \`x-goog-api-key: sk-pool-…\`; do not use \`x-api-key\` or \`Authorization: Bearer\` on this surface.
- Use this surface for Google GenAI SDKs and tools with a Gemini-compatible custom provider.

The same \`sk-pool-…\` key and account balance work on every lane of the unified endpoint. If a client supports both protocols, preserve the protocol it already uses unless the user requests a change. If it supports neither custom base URLs nor custom providers, explain the incompatibility and recommend a supported integration rather than pretending the setup succeeded.

## Current model catalog

Never invent or normalize a model ID. Use the exact ID. The full data-driven catalog, including context limits and capabilities, is ${SITE_ORIGIN}/md/models.

Anthropic Messages models:

${anthropicModelIds}

OpenAI-compatible models:

${openAiModelIds}

Gemini models (native Google API):

${geminiModelIds}

For the OpenAI-compatible surface, call \`GET /v1/models\` when the key is available so the runtime result wins over cached documentation; on the Gemini surface the equivalent is \`GET /v1beta/models\`. For the Anthropic surface, use the canonical model catalog above. If the user did not request a model, keep the project's existing model family when possible; otherwise choose a sensible current model for the stated workload and explain the choice.

## Step 3 — store the key for the actual OS and tool

Use \`APITOKEN_API_KEY\` as the neutral local name. Add a tool-specific alias only when the client requires it.

### macOS and Linux — bash or zsh, current session

\`\`\`bash
export APITOKEN_API_KEY="sk-pool-…"
\`\`\`

For an Anthropic-native client:

\`\`\`bash
export ANTHROPIC_BASE_URL="${API_BASE_URL}"
export ANTHROPIC_API_KEY="$APITOKEN_API_KEY"
\`\`\`

For an OpenAI-compatible client that reads standard environment names:

\`\`\`bash
export OPENAI_BASE_URL="${OPENAI_BASE_URL}"
export OPENAI_API_KEY="$APITOKEN_API_KEY"
\`\`\`

For persistence, use the active shell's correct profile only with the user's consent. Prefer a secret manager or CI secret over a plaintext profile. Fish syntax differs; use \`set -gx NAME value\` for the current session and do not paste secrets into a tracked fish config.

### Windows PowerShell — current process

\`\`\`powershell
$env:APITOKEN_API_KEY = "sk-pool-…"
$env:ANTHROPIC_BASE_URL = "${API_BASE_URL}"
$env:ANTHROPIC_API_KEY = $env:APITOKEN_API_KEY
\`\`\`

For an OpenAI-compatible client, set \`OPENAI_BASE_URL\` to \`${OPENAI_BASE_URL}\` and \`OPENAI_API_KEY\` to \`$env:APITOKEN_API_KEY\`. Persistent user-level variables apply only to new processes; do not claim the running terminal inherited them. Prefer Windows Credential Manager or the client's protected key field when available.

### Windows cmd.exe — current process

\`\`\`bat
set APITOKEN_API_KEY=sk-pool-…
set ANTHROPIC_BASE_URL=${API_BASE_URL}
set ANTHROPIC_API_KEY=%APITOKEN_API_KEY%
\`\`\`

Avoid putting the key on a command line that will be recorded in shared shell history. When interacting with a human, let them enter the secret locally.

### Project, container, CI, and production

- Use the platform's encrypted secret store for the key.
- Inject the base URL as non-secret configuration and the key as a secret at runtime.
- Restart or redeploy the process after changing environment variables, then verify from the same runtime environment.
- Never expose the key through \`NEXT_PUBLIC_*\`, \`VITE_*\`, browser JavaScript, a public Docker build argument, or a committed manifest.

## Step 4 — map common tools

| Tool or codebase | Preferred surface | Required setup |
|---|---|---|
| Claude Code | Anthropic Messages | \`ANTHROPIC_BASE_URL=${API_BASE_URL}\`, \`ANTHROPIC_API_KEY=<key>\` |
| Anthropic Python/TypeScript SDK | Anthropic Messages | custom \`base_url\` / \`baseURL\` plus the key |
| Cursor, Cline, Continue, Zed, Roo Code | Preserve the configured compatible provider | select Anthropic-compatible mode when available, set \`${API_BASE_URL}\`, exact model ID, protected key field |
| Codex CLI | OpenAI-compatible Responses | named provider profile with \`base_url = "${OPENAI_BASE_URL}"\`, \`wire_api = "responses"\`, and \`env_key = "APITOKEN_API_KEY"\` |
| OpenAI Python/JavaScript SDK | OpenAI-compatible | custom \`base_url\` / \`baseURL\`, bearer key, Responses or Chat Completions |
| Generic OpenAI-compatible tool | OpenAI-compatible | \`${OPENAI_BASE_URL}\`, bearer key, exact model returned by \`GET /v1/models\` |
| Raw HTTP | Match the request schema | use the exact endpoint and auth header from Step 2 |

Tool-specific machine-readable guides are indexed at ${SITE_ORIGIN}/md/int. Read the matching guide and the installed tool's current configuration before editing paths from memory.

## Step 5 — verify with a real request

Verify from the same machine, container, user account, and proxy path as the target application. A website reachable in a desktop browser does not prove a container or remote IDE can reach it.

Anthropic Messages verification:

\`\`\`bash
curl --fail-with-body ${API_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{"model":"claude-haiku-4-5","max_tokens":16,"messages":[{"role":"user","content":"Reply with exactly: connected"}]}'
\`\`\`

OpenAI-compatible discovery and verification:

\`\`\`bash
curl --fail-with-body ${OPENAI_BASE_URL}/models \\
  -H "Authorization: Bearer $APITOKEN_API_KEY"

curl --fail-with-body ${OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "content-type: application/json" \\
  -d '{"model":"gpt-5.6-luna","input":"Reply with exactly: connected","max_output_tokens":16}'
\`\`\`

On PowerShell, use \`curl.exe\` for these curl examples or create the equivalent \`Invoke-RestMethod\` request with a header dictionary. Do not assume the \`curl\` alias has curl semantics on older Windows PowerShell.

After the direct request succeeds, run the target client itself. If the integration needs streaming, tools, or images, perform one small feature-specific check after the basic non-streaming check.

## Diagnostic decision tree

1. **DNS, TLS, connection refused, or timeout before HTTP:** verify the exact hostname, system clock, proxy, VPN, firewall, container DNS, and remote-host network. Test from the target runtime.
2. **HTML, a website page, or 404 route output:** the base URL is wrong. Check for a missing endpoint or duplicated \`/v1/v1\`.
3. **401:** confirm the key is active and not surrounded by quotes or whitespace in the stored value. Anthropic uses \`x-api-key\`; OpenAI-compatible uses \`Authorization: Bearer\`; Gemini uses \`x-goog-api-key\`. A revoked key must be replaced, not retried.
4. **402:** the account's available balance is insufficient. Top up in the dashboard; backoff cannot fix it.
5. **404 model_not_found:** list OpenAI-compatible models with \`GET /v1/models\` or check ${SITE_ORIGIN}/md/models, then use the exact ID on the matching surface.
6. **413 or context-length failure:** reduce prompt, image, tool-schema, or output size; confirm the selected model's limits.
7. **429:** honor \`Retry-After\`, reduce concurrency, and use bounded exponential backoff with jitter. Do not create an unbounded retry loop.
8. **5xx:** keep the request ID and approximate timestamp, retry a small number of times with backoff, and check ${SITE_ORIGIN}/status.
9. **Streaming does not arrive incrementally:** confirm \`stream: true\`, the correct SDK streaming method, SSE parsing, and that a reverse proxy is not buffering the response.
10. **Direct curl works but the client fails:** inspect the client's actual configuration scope, inherited environment, restarted process, provider mode, generated URL, model ID, and proxy. Do not blame the gateway before comparing the outgoing request.

The complete exact-error catalog is ${SITE_ORIGIN}/md/docs/errors.

## Escalate to support

Telegram: https://t.me/apitokensupportbot — AI first-line support is available 24/7 and a person can join the same case.

Send this diagnostic brief with secrets redacted:

\`\`\`text
Operating system and version:
Client / IDE / CLI / SDK and version:
Runtime and shell:
What is being connected:
API surface and exact endpoint:
Model ID:
HTTP status and exact error text:
Approximate time and timezone:
Request ID, if present:
What has already been tried:
API key: REDACTED (last 4 characters only):
\`\`\`

Support must never request a password, payment-card data, or a full API key. If a key was exposed, revoke it in the dashboard and issue a replacement before continuing.

## Final report to the user

State concisely:

- detected environment and target tool;
- selected API surface, base URL, and exact model ID;
- settings or files changed, without secret values;
- verification performed and its result;
- how to restart, switch model, or revert;
- any unresolved error with HTTP status, request ID, and the next concrete action.
`
  );
}

/** Canonical API reference: everything an agent needs to make a first call, from live model data. */
export function buildApiReferenceMarkdown(): string {
  const claudeRows = claudeModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");
  const gptRows = openaiModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");
  const geminiRows = geminiModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");

  return (
    frontmatter({
      title: "apiToken.sale — API reference (Claude, GPT & Gemini)",
      description:
        "One unified router endpoint for every provider: native Anthropic Messages, OpenAI Responses and Google Gemini APIs plus an OpenAI-compatible route for any catalog model — base URLs, exact model IDs, headers, streaming, tool use, prompt caching and error codes.",
      url: `${SITE_ORIGIN}/docs`,
      language: "en",
    }) +
    `# API reference — apiToken.sale

apiToken.sale is an independent multi-provider gateway built as a **unified router**. One endpoint — \`${API_BASE_URL}\` — serves the **native Anthropic Messages API**, the **native OpenAI Responses API** and the **native Google Gemini API**, plus an **OpenAI-compatible universal route** that reaches every catalog model from any OpenAI-compatible client. One prepaid balance and one \`sk-pool-…\` key at a flat 50% discount work on every lane. Native lanes pass requests through byte-faithfully — request bodies, responses, streaming and error shapes match the official APIs; the universal route translates fail-closed, so an unsupported parameter returns a clear \`400 unsupported_parameter\` instead of being silently dropped.

## Unified endpoint

- **Base URL:** \`${API_BASE_URL}\`
- **Key:** the same \`sk-pool-…\` on every lane, sent in the header style of the protocol: \`x-api-key\` (Anthropic), \`Authorization: Bearer\` (OpenAI lanes), \`x-goog-api-key\` (Gemini).

| Lane | Endpoints | Auth header |
|---|---|---|
| Anthropic Messages (native) | \`POST /v1/messages\` · \`POST /v1/messages/count_tokens\` | \`x-api-key\` + \`anthropic-version\` |
| OpenAI Responses (native) | \`POST /v1/responses\` · \`POST /v1/responses/input_tokens\` · \`GET /v1/responses/{id}\` | \`Authorization: Bearer\` |
| OpenAI-compatible (universal) | \`POST /v1/chat/completions\` | \`Authorization: Bearer\` |
| Unified catalog | \`GET /v1/models\` · \`GET /v1/models/{id}\` | any lane header |
| Gemini (native) | \`GET /v1beta/models\` · \`POST /v1beta/models/{model}:generateContent\` · \`POST /v1beta/models/{model}:streamGenerateContent\` · \`POST /v1beta/models/{model}:countTokens\` | \`x-goog-api-key\` |

- **Model dispatch:** native lanes also serve models of the other providers — \`POST /v1/messages\` accepts GPT and Gemini models, \`POST /v1/responses\` and \`POST /v1/chat/completions\` accept Claude and Gemini models. One client protocol, the whole catalog.
- **Namespaced model IDs:** the unified catalog publishes \`anthropic/claude-*\`, \`openai/gpt-*\` and \`google/gemini-*\`. Prefer namespaced IDs on shared lanes; bare native IDs keep working while they are globally unambiguous.
- **Modalities:** text and image input, text output (Gemini Nano Banana models also output images). Audio, files, realtime, assistants, batches and fine-tuning are not available — this is an independent service, not the OpenAI Platform.

## Claude models (Anthropic lane)

Exact Claude model IDs (use the ID unchanged in the \`model\` field; on shared lanes the namespaced form is \`anthropic/<id>\`). Prices are official Anthropic $ per 1M tokens; you pay a flat 50% less on every request.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${claudeRows}

## GPT models (OpenAI lanes)

Exact GPT model IDs (namespaced form \`openai/<id>\`). Prices are official OpenAI $ per 1M tokens with the same flat 50% discount; cached input bills at 10% of input. Requests above 272K input tokens bill at OpenAI long-context rates (2× input, 1.5× output on the whole request). \`gpt-5.6\` is an alias of \`gpt-5.6-sol\`.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${gptRows}

## Gemini models (Gemini lane)

Exact Gemini model IDs (namespaced form \`google/<id>\`). Prices are official Google $ per 1M tokens with the same flat 50% discount; cached input bills at 10% of input. gemini-3.1-pro-preview requests above 200K input tokens bill at long-context rates (2× input, 1.5× output on the whole request). gemini-3.1-flash-image bills image output at $60 per 1M image-output tokens.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${geminiRows}

Per-model detail pages: ${[...claudeModels, ...openaiModels, ...geminiModels].map((m) => `${SITE_ORIGIN}${modelPath(m.slug)}`).join(", ")}.

## First request (native Anthropic lane, curl)

\`\`\`bash
curl ${API_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-opus-4-8",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'
\`\`\`

## First request (OpenAI-compatible universal route, curl)

\`\`\`bash
curl ${OPENAI_BASE_URL}/chat/completions \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.6-sol",
    "messages": [{"role": "user", "content": "Reply with exactly: connected"}]
  }'
\`\`\`

This one route serves the whole catalog: swap the model for \`anthropic/claude-opus-4-8\` or \`google/gemini-3.6-flash\` without changing code, endpoint or key. The native OpenAI Responses API is also on this host at \`POST ${OPENAI_BASE_URL}/responses\`.

## First request (native Gemini lane, curl)

\`\`\`bash
curl ${GEMINI_BASE_URL}/v1beta/models/gemini-3.6-flash:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "contents": [{"parts": [{"text": "Reply with exactly: connected"}]}]
  }'
\`\`\`

## Official SDKs

Set the base URL and reuse the official SDKs unchanged.

\`\`\`python
from anthropic import Anthropic
client = Anthropic(base_url="${API_BASE_URL}", api_key="sk-pool-…")
msg = client.messages.create(
    model="claude-opus-4-8",
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)
print(msg.content)
\`\`\`

\`\`\`typescript
import Anthropic from "@anthropic-ai/sdk";
const client = new Anthropic({ baseURL: "${API_BASE_URL}", apiKey: "sk-pool-…" });
const msg = await client.messages.create({
  model: "claude-opus-4-8",
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello" }],
});
\`\`\`

\`\`\`python
from openai import OpenAI
client = OpenAI(api_key="sk-pool-…", base_url="${OPENAI_BASE_URL}")
response = client.responses.create(model="gpt-5.6-sol", input="Reply with exactly: connected")
print(response.output_text)
\`\`\`

## Coding tools

Claude Code, Cursor, Cline, Continue, Zed, Aider, Roo Code, LangChain and LiteLLM all work by pointing the Anthropic base URL at \`${API_BASE_URL}\`. For Claude Code: \`export ANTHROPIC_BASE_URL=${API_BASE_URL}\` and \`export ANTHROPIC_API_KEY=sk-pool-…\`. Codex CLI and OpenAI-compatible tools run on the OpenAI lanes at \`${OPENAI_BASE_URL}\` — see the integration guides: ${SITE_ORIGIN}/md/int. Google GenAI SDKs and Gemini-compatible tools run on the native Gemini lane at \`${GEMINI_BASE_URL}\` with the key sent as \`x-goog-api-key\`.

## Capability parity (native lanes)

Everything the provider APIs support passes through unchanged:

- **Streaming:** \`"stream": true\` returns standard Anthropic SSE events; OpenAI lanes stream SSE on both Responses and Chat Completions; the Gemini lane streams via \`:streamGenerateContent\`.
- **Tool use / function calling:** \`tools\` and \`tool_choice\` work identically; multi-turn tool loops supported.
- **Prompt caching:** Anthropic \`cache_control\` breakpoints are honored and billed at cache read/write rates; GPT prefixes cache automatically at 10% of input.
- **Vision:** image content blocks are supported.
- **System prompts, stop sequences, temperature, top_p, max_tokens:** identical semantics.

## Error codes

Every lane keeps its provider's error envelope: Anthropic lanes return Anthropic's JSON, OpenAI lanes return \`{"error":{"message","type","param","code"}}\`, and the Gemini lane returns \`{"error":{"code","message","status"}}\`.

| Status | Meaning | What to do |
|---|---|---|
| 400 | Unsupported parameter on the target lane (\`unsupported_parameter\`) or malformed request | Remove the flagged parameter or switch to the model's native lane; the router never silently drops parameters. |
| 401 | API key missing, invalid or revoked | Send a valid \`sk-pool-…\` in the lane's auth header; if revoked, create a new key. |
| 402 | Prepaid balance too low | Top up any whole-dollar amount; retry after crediting. |
| 404 | Unknown or disabled model ID | List enabled IDs with \`GET /v1/models\`; check for typos or use the namespaced form. |
| 429 | Rate limit or temporary upstream capacity | Honor \`Retry-After\`; retry with capped exponential backoff and jitter. |
| 5xx | Temporary gateway or upstream failure | Retry with bounded backoff; keep the request ID and avoid duplicate attempts. |

## Legacy per-provider endpoints

Already integrated? The original per-provider hosts remain fully supported with the same key and balance — no migration required:

- Anthropic Messages: \`${LEGACY_ANTHROPIC_BASE_URL}\`
- OpenAI-compatible: \`${LEGACY_OPENAI_BASE_URL}\`
- Gemini: \`${LEGACY_GEMINI_BASE_URL}\`

New integrations should use the unified router endpoint above — new capabilities land there first.

## Pricing

Prepaid, per-token at official provider rates minus the flat ${pct(DISCOUNT_FLAT)} discount on every request, shared by all lanes. No fixed packages or subscriptions; balance never expires. Pricing details: ${SITE_ORIGIN}/md/plans.

## Get started

- Create a key: ${SITE_ORIGIN}/register (Google or GitHub sign-up gets $5 of platform bonus credit)
- Agent setup runbook (OS, client detection, secure configuration and verification): ${SITE_ORIGIN}/md/connect
- All guides (Markdown): ${SITE_ORIGIN}/docs/learn
- Machine-readable index: ${SITE_ORIGIN}/md
- Support: Telegram and apitokensale@gmail.com (English, Russian)
`
  );
}

/** Model catalog with exact IDs, context, limits and discounted per-token prices. */
export function buildModelsMarkdown(): string {
  const sectionFor = (m: CatalogModel): string => {
    const inHere = formatUsd(m.inputPerM * (1 - DISCOUNT_FLAT));
    const outHere = formatUsd(m.outputPerM * (1 - DISCOUNT_FLAT));
    const surface = m.provider === "anthropic"
      ? `- **Lane:** Anthropic Messages API (native) at \`${API_BASE_URL}\` · also callable via \`POST /v1/chat/completions\` as \`anthropic/${m.id}\``
      : m.provider === "openai"
        ? `- **Lane:** OpenAI Responses / Chat Completions at \`${OPENAI_BASE_URL}\` (Authorization: Bearer) · namespaced ID \`openai/${m.id}\``
        : `- **Lane:** Gemini API (native) at \`${GEMINI_BASE_URL}\` (x-goog-api-key) · also callable via \`POST /v1/chat/completions\` as \`google/${m.id}\``;
    const cached = m.provider === "openai"
      ? `\n- **Cached input (per 1M):** $${m.cachedInputPerM} · **Cache write:** $${m.cacheWritePerM}`
      : m.provider === "gemini"
        ? `\n- **Cached input (per 1M):** $${m.cachedInputPerM}`
        : "";
    return [
      `## ${m.name}`,
      "",
      `- **Model ID:** \`${m.id}\``,
      `- **Tier:** ${m.tier}`,
      `- **Context window:** ${m.context}`,
      `- **Max output:** ${m.maxOutput}`,
      `- **Official price (per 1M):** $${m.inputPerM} input / $${m.outputPerM} output${cached}`,
      `- **Your price (per 1M):** input ${inHere}, output ${outHere} (flat ${pct(DISCOUNT_FLAT)} off)`,
      surface,
      `- **Best for:** ${m.bestFor.join(" ")}`,
      `- **Detail page:** ${SITE_ORIGIN}${modelPath(m.slug)}`,
    ].join("\n");
  };

  return (
    frontmatter({
      title: "apiToken.sale — model catalog (Claude, GPT & Gemini)",
      description:
        "Every Claude, GPT and Gemini model available through apiToken.sale with exact API IDs, context windows, max output and discounted per-token pricing.",
      url: `${SITE_ORIGIN}/models`,
      language: "en",
    }) +
    `# Model catalog

All models run on one \`sk-pool-…\` key and one prepaid balance through the unified router endpoint \`${API_BASE_URL}\` — native Anthropic, OpenAI and Gemini lanes plus one OpenAI-compatible route for any model. Use the model ID unchanged in the \`model\` field; on shared lanes prefer the namespaced form (\`anthropic/<id>\`, \`openai/<id>\`, \`google/<id>\`).

# Claude models (Anthropic Messages API)

${claudeModels.map(sectionFor).join("\n\n")}

# GPT models (OpenAI-compatible API)

${openaiModels.map(sectionFor).join("\n\n")}

# Gemini models (Google Gemini API)

${geminiModels.map(sectionFor).join("\n\n")}

---
API reference: ${SITE_ORIGIN}/md/docs · Pricing: ${SITE_ORIGIN}/md/plans
`
  );
}

/** Flat 50% B2C pricing, generated from the live pricing model. */
export function buildPlansMarkdown(): string {
  return (
    frontmatter({
      title: "apiToken.sale — API pricing (Claude, GPT & Gemini)",
      description:
        "apiToken.sale flat pricing: 50% off official provider rates on every request, for every account and any top-up amount. Prepaid per-token billing at official Anthropic, OpenAI and Google rates.",
      url: `${SITE_ORIGIN}/plans`,
      language: "en",
    }) +
    `# Pricing — flat ${B2C_DISCOUNT_PERCENT}% off

Top up any whole-dollar amount. Each request is billed at the official provider token price, then the flat ${B2C_DISCOUNT_PERCENT}% discount is applied and deducted from balance. One balance and one rate cover Claude, GPT and Gemini models alike. No fixed packages, no subscriptions, no tiers, balance never expires.

| Top up | Discount (value multiplier) | Official API value |
|---|---|---|
| Any whole USD amount | ${B2C_DISCOUNT_PERCENT}% (×${B2C_VALUE_MULTIPLIER}) | top-up × ${B2C_VALUE_MULTIPLIER} — e.g. $50 → $100 |

- The same ${B2C_DISCOUNT_PERCENT}% discount applies to **every request** — no thresholds, no monthly targets, nothing to unlock or keep.
- Official API value = top-up ÷ the share paid after the discount; billing itself is exact.
- B2B pricing is negotiated separately.

---
API reference: ${SITE_ORIGIN}/md/docs · Models: ${SITE_ORIGIN}/md/models
`
  );
}

/** One model's full spec (exact ID, context, limits, pricing, best-for, notes, FAQ). */
export function buildModelMarkdown(model: CatalogModel): string {
  const inHere = formatUsd(model.inputPerM * (1 - DISCOUNT_FLAT));
  const outHere = formatUsd(model.outputPerM * (1 - DISCOUNT_FLAT));
  const surfaceLine = model.provider === "anthropic"
    ? `- **Base URL:** \`${API_BASE_URL}\` · **Endpoint:** \`POST /v1/messages\``
    : model.provider === "openai"
      ? `- **Base URL:** \`${OPENAI_BASE_URL}\` · **Endpoints:** \`POST /v1/responses\`, \`POST /v1/chat/completions\` (Authorization: Bearer)`
      : `- **Base URL:** \`${GEMINI_BASE_URL}\` · **Endpoint:** \`POST /v1beta/models/${model.id}:generateContent\` (x-goog-api-key)`;
  const callSnippet = model.provider === "anthropic"
    ? `\`\`\`bash
curl ${API_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{"model": "${model.id}", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
\`\`\``
    : model.provider === "openai"
      ? `\`\`\`bash
curl ${OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model": "${model.id}", "input": "Reply with exactly: connected"}'
\`\`\``
      : `\`\`\`bash
curl ${GEMINI_BASE_URL}/v1beta/models/${model.id}:generateContent \\
  -H "x-goog-api-key: $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"contents": [{"parts": [{"text": "Reply with exactly: connected"}]}]}'
\`\`\``;
  return (
    frontmatter({
      title: `${model.name} — API access`,
      description: model.description,
      url: `${SITE_ORIGIN}${modelPath(model.slug)}`,
      language: "en",
    }) +
    `# ${model.name}

${model.dek}

- **Model ID:** \`${model.id}\`
- **Tier:** ${model.tier}
- **Context window:** ${model.context}
- **Max output:** ${model.maxOutput}
- **Official price (per 1M):** $${model.inputPerM} input / $${model.outputPerM} output
- **Your price (per 1M):** input ${inHere}, output ${outHere} (flat ${pct(DISCOUNT_FLAT)} off)
${surfaceLine}

## Best for

${model.bestFor.map((b) => `- ${b}`).join("\n")}

${model.notes.length ? `## Notes\n\n${model.notes.map((n) => `- ${n}`).join("\n")}\n` : ""}
## Call it

${callSnippet}

${model.faq.length ? `## FAQ\n\n${model.faq.map((f) => `**${f.q}**\n\n${f.a}`).join("\n\n")}\n` : ""}
---
API reference: ${SITE_ORIGIN}/md/docs · All models: ${SITE_ORIGIN}/md/models
`
  );
}

export function buildModelMarkdownBySlug(slug: string): string | null {
  const model = catalogModelBySlug[slug];
  return model ? buildModelMarkdown(model) : null;
}

export const integrationSlugs = Object.keys(integrationGuideSeo) as IntegrationGuideSlug[];

/** One tool's connection guide: title/description from SEO data, exact config from the map. */
export function buildIntegrationMarkdown(slug: string): string | null {
  if (!(slug in integrationGuideSeo)) return null;
  const key = slug as IntegrationGuideSlug;
  const seo = integrationGuideSeo[key];
  const name = seo.title.replace(/^Connect /, "").replace(/ to (apiToken\.sale|the Claude API)$/, "");
  const surface = INTEGRATION_SURFACE[key];
  const steps = surface === "anthropic"
    ? [
        `1. Create a key at ${SITE_ORIGIN}/register — it looks like \`sk-pool-…\` and works across every Claude and GPT model.`,
        `2. Point ${name} at the gateway: set the Anthropic base URL to \`${API_BASE_URL}\` and paste your key.`,
        `3. Pick a Claude model (e.g. \`claude-opus-4-8\`) and start — billing is per token at your discount.`,
      ]
    : [
        `1. Create a key at ${SITE_ORIGIN}/register — it looks like \`sk-pool-…\` and works across every Claude and GPT model.`,
        `2. Point ${name} at the OpenAI-compatible surface: base URL \`${OPENAI_BASE_URL}\`, key sent as \`Authorization: Bearer\`.`,
        `3. Pick a GPT model (e.g. \`gpt-5.6-sol\`) and start — billing is per token at your discount, \`GET /v1/models\` lists the enabled set.`,
      ];
  return (
    frontmatter({
      title: seo.title,
      description: seo.description,
      url: `${SITE_ORIGIN}${seo.path}`,
      language: "en",
    }) +
    `# ${seo.title}

${seo.description}

## Steps

${steps.join("\n")}

## Configuration

\`\`\`
${INTEGRATION_CONFIG[key]}
\`\`\`

---
API reference: ${SITE_ORIGIN}/md/docs · All integrations: ${SITE_ORIGIN}/md/int
`
  );
}

/** Index of all tool connection guides. */
export function buildIntegrationsIndexMarkdown(): string {
  const rows = integrationSlugs
    .map((slug) => `- [${integrationGuideSeo[slug].title}](${SITE_ORIGIN}/md/int/${slug}) — ${integrationGuideSeo[slug].description}`)
    .join("\n");
  return (
    frontmatter({
      title: "apiToken.sale — API integrations",
      description: "Connect coding tools and SDKs to apiToken.sale via the Anthropic-compatible or OpenAI-compatible endpoint — one key and balance for Claude and GPT models.",
      url: `${SITE_ORIGIN}/integrations`,
      language: "en",
    }) +
    `# API integrations

Anthropic-compatible tools connect by pointing their Anthropic base URL at \`${API_BASE_URL}\`; OpenAI-compatible tools use \`${OPENAI_BASE_URL}\` with \`Authorization: Bearer\`. Both draw on the same \`sk-pool-…\` key and prepaid balance.

${rows}

---
API reference: ${SITE_ORIGIN}/md/docs
`
  );
}

/** Index of every machine-readable Markdown document on the site. */
/**
 * Error reference as Markdown. Same catalog as /docs/errors, so the verbatim message
 * strings stay identical across the HTML page, this file and the GitHub mirror —
 * which is the whole point: an agent answering "what does this error mean" should
 * find the exact string it was given.
 */
export function buildErrorsMarkdown(): string {
  const sectionFor = (entry: (typeof API_ERRORS)[number]) => {
    const head = entry.status === 0 ? entry.title : `${entry.status} ${entry.type} — ${entry.title}`;
    const body =
      entry.status === 0
        ? entry.message
        : entry.surface === "openai"
          ? `HTTP ${entry.status}\n{"error":{"message":${JSON.stringify(entry.message)},"type":"${entry.type}","param":null,"code":"${entry.envelopeCode ?? entry.type}"}}`
          : `HTTP ${entry.status}\n{"type":"error","error":{"type":"${entry.type}","message":${JSON.stringify(entry.message)}}}`;
    const variants = entry.alsoSearchedAs?.length
      ? `\n**Other forms of the same failure**\n\n${entry.alsoSearchedAs.map((v) => `- \`${v}\``).join("\n")}\n`
      : "";
    const origin =
      entry.surface === "openai"
        ? "Returned by the OpenAI-compatible endpoint at openai.api.apitoken.sale."
        : entry.surface === "apitoken"
          ? "Specific to apiToken.sale — the Anthropic API has no equivalent response."
          : entry.status === 0
            ? "Comes from Anthropic's own apps and subscription plans, not from an API call."
            : "Identical on api.anthropic.com and on apiToken.sale.";

    return `## ${head}

\`\`\`
${body}
\`\`\`

**Why it happens**

${entry.causes.map((c) => `- ${c}`).join("\n")}

**How to fix it**

${entry.fixes.map((f) => `- ${f}`).join("\n")}
${entry.snippet ? `\n**${entry.snippet.label}**\n\n\`\`\`\n${entry.snippet.code}\n\`\`\`\n` : ""}${variants}
Short link: ${SITE_ORIGIN}/e/${entry.code} · ${origin}`;
  };

  const anthropicEntries = API_ERRORS.filter((entry) => entry.surface !== "openai");
  const openAiEntries = API_ERRORS.filter((entry) => entry.surface === "openai");

  return (
    frontmatter({
      title: "API error codes (Claude & OpenAI-compatible) — cause and fix for each",
      description:
        "Every API error with its exact response text: 401 invalid x-api-key, 429 rate_limit_error, 529 Overloaded, 413 request_too_large on the Anthropic surface, and 401 invalid_api_key, 402 insufficient_quota, 404 model_not_found on the OpenAI-compatible surface.",
      url: `${SITE_ORIGIN}/docs/errors`,
      language: "en",
    }) +
    `# API error codes

## Anthropic surface (api.apitoken.sale)

Every error is returned with the same envelope, so branch on \`error.type\` and the HTTP
status rather than on the message text:

\`\`\`
{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}
\`\`\`

In the official SDKs that means catching the typed exception classes rather than string
matching. The verbatim messages below are reproduced because they are what you have in
front of you when something breaks.

| Status | error.type | Meaning | Retry? |
|---|---|---|---|
${anthropicEntries.map((e) => `| ${e.status === 0 ? "—" : e.status} | \`${e.type}\` | ${e.title.replace(/^\d+\s+—\s+/, "")} | ${e.retryable ? "Yes, with backoff" : "No — fix the request"} |`).join("\n")}

${anthropicEntries.map(sectionFor).join("\n\n")}

## OpenAI-compatible surface (openai.api.apitoken.sale)

This surface returns the OpenAI error envelope — branch on \`error.code\` and the HTTP status:

\`\`\`
{"error":{"message":"Incorrect API key provided.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}
\`\`\`

| Status | error.type | error.code | Meaning | Retry? |
|---|---|---|---|---|
${openAiEntries.map((e) => `| ${e.status} | \`${e.type}\` | \`${e.envelopeCode ?? e.type}\` | ${e.title.replace(/^\d+\s+—\s+/, "")} | ${e.retryable ? "Yes, with backoff" : "No — fix the request"} |`).join("\n")}

${openAiEntries.map(sectionFor).join("\n\n")}

---

apiToken.sale serves the native Anthropic Messages, OpenAI Responses and Gemini APIs plus an
OpenAI-compatible route through one unified router endpoint, so non-gateway errors here behave
exactly as they do against the official endpoints.
Unified base URL: ${API_BASE_URL} · OpenAI lanes: ${OPENAI_BASE_URL}
`
  );
}

export function buildMdIndexMarkdown(): string {
  return (
    frontmatter({
      title: "apiToken.sale — Markdown index for AI agents",
      description: "Index of every apiToken.sale section available as clean Markdown for crawlers and AI agents.",
      url: `${SITE_ORIGIN}/md`,
      language: "en",
    }) +
    `# Markdown index for AI agents

Every public section of apiToken.sale is available as clean Markdown. Private dashboard, auth and account pages are excluded.

## Core references

- Agent setup runbook (use this to configure an unknown OS, client or SDK): ${SITE_ORIGIN}/md/connect
- API reference (connection, models, streaming, tools, errors): ${SITE_ORIGIN}/md/docs
- Error reference (exact response text, cause and fix for every error): ${SITE_ORIGIN}/md/docs/errors
- Model catalog (exact IDs, context, pricing): ${SITE_ORIGIN}/md/models
- Per-model spec: append the model slug to ${SITE_ORIGIN}/md/models/<slug> (${[...claudeModels, ...openaiModels, ...geminiModels].map((m) => m.slug).join(", ")})
- Pricing & flat 50% discount: ${SITE_ORIGIN}/md/plans
- Integrations (all tools): ${SITE_ORIGIN}/md/int
- Per-tool setup: append the slug to ${SITE_ORIGIN}/md/int/<slug> (${integrationSlugs.join(", ")})

## Guides

- All guides, categorized, with descriptions: ${SITE_ORIGIN}/llms.txt
- Every guide as Markdown: append its slug to ${SITE_ORIGIN}/md/docs/learn/<slug>
- Full text of all guides in one file: ${SITE_ORIGIN}/llms-full.txt

## Site map

- HTML sitemap: ${SITE_ORIGIN}/sitemap.xml
- Overview and key facts: ${SITE_ORIGIN}/llms.txt

Localized guide indexes: ${SITE_ORIGIN}/llms-ru.txt, ${SITE_ORIGIN}/llms-zh.txt, ${SITE_ORIGIN}/llms-ko.txt
`
  );
}
