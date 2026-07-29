// Machine-readable Markdown for the core sections, generated from the same data as the HTML pages.
// Served by static route handlers under /md so crawlers and AI agents can consume every public
// section as clean text. Add a model or tier to the data and its Markdown updates on the next build.
import {
  ANTHROPIC_BASE_URL,
  OPENAI_BASE_URL,
  catalogModelBySlug,
  claudeModels,
  formatUsd,
  DISCOUNT_BASE,
  DISCOUNT_MAX,
  modelPath,
  openaiModels,
  type CatalogModel,
} from "./models";
import { B2C_PRICING_MILESTONES, formatWholeUsd } from "./pricing-tiers";
import { integrationGuideSeo, SITE_ORIGIN, type IntegrationGuideSlug } from "./seo";
import { API_ERRORS } from "./api-errors";

const API_BASE_URL = ANTHROPIC_BASE_URL;

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
  opencode: `// opencode.json — provider block
{
  "provider": {
    "apitoken": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "apiToken.sale",
      "options": {
        "baseURL": "${OPENAI_BASE_URL}",
        "apiKey": "{env:APITOKEN_API_KEY}"
      },
      "models": {
        "gpt-5.6-sol": { "name": "GPT-5.6 Sol" }
      }
    }
  }
}`,
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

/** Canonical API reference: everything an agent needs to make a first call, from live model data. */
export function buildApiReferenceMarkdown(): string {
  const claudeRows = claudeModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");
  const gptRows = openaiModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");

  return (
    frontmatter({
      title: "apiToken.sale — API reference (Claude & GPT)",
      description:
        "Connect any Anthropic-compatible or OpenAI-compatible client to apiToken.sale: base URLs, exact model IDs, headers, streaming, tool use, prompt caching and error codes for both API surfaces.",
      url: `${SITE_ORIGIN}/docs`,
      language: "en",
    }) +
    `# API reference — apiToken.sale

apiToken.sale is an independent multi-provider gateway. It serves the **standard Anthropic Messages API** with the full Claude line and an **OpenAI-compatible API** (Responses and Chat Completions) with the GPT-5 line — from one prepaid balance and one \`sk-pool-…\` key at a 60–70% discount. Request bodies, responses, streaming and error shapes match the official APIs; only the host and key change.

## Surface 1 — Anthropic Messages API (Claude models)

- **Base URL:** \`${API_BASE_URL}\`
- **Endpoint:** \`POST /v1/messages\`
- **Headers:** \`x-api-key: sk-pool-…\` and \`anthropic-version: 2023-06-01\`
- **Auth:** on this surface the \`sk-pool-…\` key is sent in \`x-api-key\` (not as a bearer token).

Exact Claude model IDs (use the ID unchanged in the \`model\` field). Prices are official Anthropic $ per 1M tokens; you pay 60% less by default and up to 70% less at higher tiers.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${claudeRows}

## Surface 2 — OpenAI-compatible API (GPT models)

- **Base URL:** \`${OPENAI_BASE_URL}\`
- **Endpoints:** \`POST /v1/responses\` and \`POST /v1/chat/completions\` (SSE streaming on both)
- **Models:** \`GET /v1/models\` lists the currently enabled set
- **Auth:** the same \`sk-pool-…\` key sent as \`Authorization: Bearer sk-pool-…\` (x-api-key is not accepted on this surface).
- **Modalities:** text and image input, text output. Audio, files, realtime, assistants, batches and fine-tuning are not available — this is an independent OpenAI-compatible service, not the OpenAI Platform.

Exact GPT model IDs. Prices are official OpenAI $ per 1M tokens with the same 60–70% discount; cached input bills at 10% of input. Requests above 272K input tokens bill at OpenAI long-context rates (2× input, 1.5× output on the whole request). \`gpt-5.6\` is an alias of \`gpt-5.6-sol\`.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${gptRows}

Per-model detail pages: ${[...claudeModels, ...openaiModels].map((m) => `${SITE_ORIGIN}${modelPath(m.slug)}`).join(", ")}.

## First request (curl, Claude)

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

## First request (curl, GPT)

\`\`\`bash
curl ${OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "gpt-5.6-sol",
    "input": "Reply with exactly: connected"
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

Claude Code, Cursor, Cline, Continue, Zed, Aider, Roo Code, LangChain and LiteLLM all work by pointing the Anthropic base URL at \`${API_BASE_URL}\`. For Claude Code: \`export ANTHROPIC_BASE_URL=${API_BASE_URL}\` and \`export ANTHROPIC_API_KEY=sk-pool-…\`. Codex CLI and opencode run on the OpenAI-compatible surface — see the integration guides: ${SITE_ORIGIN}/md/int.

## Capability parity (Anthropic surface)

Everything the Anthropic Messages API supports passes through unchanged:

- **Streaming:** \`"stream": true\` returns standard Anthropic SSE events.
- **Tool use / function calling:** \`tools\` and \`tool_choice\` work identically; multi-turn tool loops supported.
- **Prompt caching:** \`cache_control\` breakpoints are honored and billed at cache read/write rates.
- **Vision:** image content blocks are supported.
- **System prompts, stop sequences, temperature, top_p, max_tokens:** identical semantics.

## Error codes (Anthropic surface)

| Status | Meaning | What to do |
|---|---|---|
| 401 | API key missing, invalid or revoked | Send a valid \`sk-pool-…\` in \`x-api-key\`; if revoked, create a new key. |
| 402 | Prepaid balance too low | Top up any whole-dollar amount; retry after crediting. |
| 429 | Rate limit or temporary upstream capacity | Honor \`Retry-After\`; retry with capped exponential backoff and jitter. |
| 5xx | Temporary gateway or upstream failure | Retry with bounded backoff; keep the request ID and avoid duplicate attempts. |

## Error codes (OpenAI-compatible surface)

Envelope: \`{"error":{"message","type","param","code"}}\`.

| Status | code | Meaning | What to do |
|---|---|---|---|
| 401 | invalid_api_key | Key missing or wrong auth header | Send \`Authorization: Bearer sk-pool-…\` — not x-api-key. |
| 402 | insufficient_quota | Prepaid balance too low | Top up any whole-dollar amount; retry after crediting. |
| 404 | model_not_found | Unknown or disabled model ID | List enabled IDs with \`GET /v1/models\`; check for typos. |
| 429 | rate_limit_error | Rate or concurrency limit | Honor \`Retry-After\`; back off with jitter, cap concurrency. |
| 503 | server_error | Model temporarily unavailable | Retry with bounded backoff; fall back to another tier if urgent. |

## Pricing

Prepaid, per-token at official provider rates minus your tier discount (${pct(DISCOUNT_BASE)} base, up to ${pct(DISCOUNT_MAX)}), shared by both surfaces. No fixed packages or subscriptions; balance never expires. Full tier table: ${SITE_ORIGIN}/md/plans.

## Get started

- Create a key: ${SITE_ORIGIN}/register (Google or GitHub sign-up gets $10 of API usage at official prices)
- All guides (Markdown): ${SITE_ORIGIN}/docs/learn
- Machine-readable index: ${SITE_ORIGIN}/md
- Support: Telegram and apitokensale@gmail.com (English, Russian)
`
  );
}

/** Model catalog with exact IDs, context, limits and discounted price ranges. */
export function buildModelsMarkdown(): string {
  const sectionFor = (m: CatalogModel): string => {
    const inFrom = formatUsd(m.inputPerM * (1 - DISCOUNT_BASE));
    const inBest = formatUsd(m.inputPerM * (1 - DISCOUNT_MAX));
    const outFrom = formatUsd(m.outputPerM * (1 - DISCOUNT_BASE));
    const outBest = formatUsd(m.outputPerM * (1 - DISCOUNT_MAX));
    const surface = m.provider === "anthropic"
      ? `- **Surface:** Anthropic Messages API at \`${API_BASE_URL}\``
      : `- **Surface:** OpenAI-compatible API at \`${OPENAI_BASE_URL}\` (Authorization: Bearer)`;
    const cached = m.provider === "openai"
      ? `\n- **Cached input (per 1M):** $${m.cachedInputPerM} · **Cache write:** $${m.cacheWritePerM}`
      : "";
    return [
      `## ${m.name}`,
      "",
      `- **Model ID:** \`${m.id}\``,
      `- **Tier:** ${m.tier}`,
      `- **Context window:** ${m.context}`,
      `- **Max output:** ${m.maxOutput}`,
      `- **Official price (per 1M):** $${m.inputPerM} input / $${m.outputPerM} output${cached}`,
      `- **Your price (per 1M):** input ${inFrom} → ${inBest}, output ${outFrom} → ${outBest} (${pct(DISCOUNT_BASE)}–${pct(DISCOUNT_MAX)} off)`,
      surface,
      `- **Best for:** ${m.bestFor.join(" ")}`,
      `- **Detail page:** ${SITE_ORIGIN}${modelPath(m.slug)}`,
    ].join("\n");
  };

  return (
    frontmatter({
      title: "apiToken.sale — model catalog (Claude & GPT)",
      description:
        "Every Claude and GPT model available through apiToken.sale with exact API IDs, context windows, max output and discounted per-token pricing.",
      url: `${SITE_ORIGIN}/models`,
      language: "en",
    }) +
    `# Model catalog

All models run on one \`sk-pool-…\` key and one prepaid balance — Claude models via \`${API_BASE_URL}\`, GPT models via \`${OPENAI_BASE_URL}\`. Use the model ID unchanged in the \`model\` field.

# Claude models (Anthropic Messages API)

${claudeModels.map(sectionFor).join("\n\n")}

# GPT models (OpenAI-compatible API)

${openaiModels.map(sectionFor).join("\n\n")}

---
API reference: ${SITE_ORIGIN}/md/docs · Pricing tiers: ${SITE_ORIGIN}/md/plans
`
  );
}

/** Progressive discount tiers, generated from the live B2C pricing model. */
export function buildPlansMarkdown(): string {
  const rows = B2C_PRICING_MILESTONES.map((t) => {
    const topUp = Number(t.platformSpendUsd) === 0 ? "— (default)" : formatWholeUsd(t.platformSpendUsd);
    const keep = Number(t.holdUsd) === 0 ? "—" : `${formatWholeUsd(t.holdUsd)} / 30 days`;
    const usage = Number(t.visibleOfficialUsageUsd) === 0 ? "—" : formatWholeUsd(t.visibleOfficialUsageUsd);
    const mult = (100 / (100 - t.discountPercent)).toFixed(2);
    return `| ${t.label} | ${t.discountPercent}% (×${mult}) | ${topUp} | ${keep} | ${usage} |`;
  }).join("\n");

  return (
    frontmatter({
      title: "apiToken.sale — API pricing tiers (Claude & GPT)",
      description:
        "apiToken.sale progressive discount tiers: 60% off by default, up to 70% off with cumulative top-ups. Prepaid per-token billing at official Anthropic and OpenAI rates.",
      url: `${SITE_ORIGIN}/plans`,
      language: "en",
    }) +
    `# Pricing & discount tiers

Top up any whole-dollar amount. Each request is billed at the official provider token price, then your active tier discount is applied and deducted from balance. One balance and one discount track cover Claude and GPT models alike. No fixed packages, no subscriptions, balance never expires.

| Tier | Discount (value multiplier) | Top up to reach | Keep the tier | Approx. official usage |
|---|---|---|---|---|
${rows}

- **Starter (${B2C_PRICING_MILESTONES[0]!.discountPercent}%)** is the permanent base tier — free, no minimum, never expires.
- Higher tiers are unlocked by **cumulative top-ups** and kept by spending ≥ 50% of the tier's threshold every rolling 30 days; otherwise the account drops one tier.
- B2B pricing is negotiated separately.

---
API reference: ${SITE_ORIGIN}/md/docs · Models: ${SITE_ORIGIN}/md/models
`
  );
}

/** One model's full spec (exact ID, context, limits, pricing, best-for, notes, FAQ). */
export function buildModelMarkdown(model: CatalogModel): string {
  const inFrom = formatUsd(model.inputPerM * (1 - DISCOUNT_BASE));
  const inBest = formatUsd(model.inputPerM * (1 - DISCOUNT_MAX));
  const outFrom = formatUsd(model.outputPerM * (1 - DISCOUNT_BASE));
  const outBest = formatUsd(model.outputPerM * (1 - DISCOUNT_MAX));
  const surfaceLine = model.provider === "anthropic"
    ? `- **Base URL:** \`${API_BASE_URL}\` · **Endpoint:** \`POST /v1/messages\``
    : `- **Base URL:** \`${OPENAI_BASE_URL}\` · **Endpoints:** \`POST /v1/responses\`, \`POST /v1/chat/completions\` (Authorization: Bearer)`;
  const callSnippet = model.provider === "anthropic"
    ? `\`\`\`bash
curl ${API_BASE_URL}/v1/messages \\
  -H "x-api-key: $APITOKEN_API_KEY" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{"model": "${model.id}", "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]}'
\`\`\``
    : `\`\`\`bash
curl ${OPENAI_BASE_URL}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model": "${model.id}", "input": "Reply with exactly: connected"}'
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
- **Your price (per 1M):** input ${inFrom} → ${inBest}, output ${outFrom} → ${outBest} (${pct(DISCOUNT_BASE)}–${pct(DISCOUNT_MAX)} off)
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

apiToken.sale serves the standard Anthropic Messages API and an OpenAI-compatible API, so
non-gateway errors here behave exactly as they do against the official endpoints.
Anthropic base URL: ${API_BASE_URL} · OpenAI-compatible base URL: ${OPENAI_BASE_URL}
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

- API reference (connection, models, streaming, tools, errors): ${SITE_ORIGIN}/md/docs
- Error reference (exact response text, cause and fix for every error): ${SITE_ORIGIN}/md/docs/errors
- Model catalog (exact IDs, context, pricing): ${SITE_ORIGIN}/md/models
- Per-model spec: append the model slug to ${SITE_ORIGIN}/md/models/<slug> (${[...claudeModels, ...openaiModels].map((m) => m.slug).join(", ")})
- Pricing & discount tiers: ${SITE_ORIGIN}/md/plans
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
