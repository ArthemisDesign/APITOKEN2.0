// Machine-readable Markdown for the core sections, generated from the same data as the HTML pages.
// Served by static route handlers under /md so crawlers and AI agents can consume every public
// section as clean text. Add a model or tier to the data and its Markdown updates on the next build.
import { claudeModels, formatUsd, DISCOUNT_BASE, DISCOUNT_MAX, modelPath } from "./models";
import { B2C_PRICING_MILESTONES, formatWholeUsd } from "./pricing-tiers";
import { SITE_ORIGIN } from "./seo";

const API_BASE_URL = "https://api.apitoken.sale";

function frontmatter(fields: Record<string, string>): string {
  const lines = Object.entries(fields).map(([key, value]) => `${key}: ${JSON.stringify(value)}`);
  return ["---", ...lines, "---", ""].join("\n");
}

function pct(discount: number): string {
  return `${Math.round(discount * 100)}%`;
}

/** Canonical API reference: everything an agent needs to make a first call, from live model data. */
export function buildApiReferenceMarkdown(): string {
  const modelRows = claudeModels
    .map((m) => `| \`${m.id}\` | ${m.tier} | ${m.context} | ${m.maxOutput} | $${m.inputPerM} / $${m.outputPerM} |`)
    .join("\n");

  return (
    frontmatter({
      title: "apiToken.sale — Claude API reference",
      description:
        "Connect any Anthropic-compatible client to the Claude API through apiToken.sale: base URL, exact model IDs, headers, streaming, tool use, prompt caching and error codes.",
      url: `${SITE_ORIGIN}/docs`,
      language: "en",
    }) +
    `# Claude API reference — apiToken.sale

apiToken.sale is an independent gateway that serves the **standard Anthropic Messages API** and the full Claude line from prepaid balance at a 60–80% discount. Point any Anthropic-compatible client at the base URL below — request bodies, responses, streaming and error shapes are identical to Anthropic. Only the host and key change.

## Connection

- **Base URL:** \`${API_BASE_URL}\`
- **Endpoint:** \`POST /v1/messages\`
- **Headers:** \`x-api-key: sk-pool-…\` and \`anthropic-version: 2023-06-01\`
- **Auth:** the \`sk-pool-…\` key is an API key sent in \`x-api-key\` (not a bearer token in Authorization).

## Models

Exact API model IDs (use the ID unchanged in the \`model\` field). Prices are official Anthropic $ per 1M tokens; you pay 60% less by default and up to 80% less at higher tiers.

| model ID | Tier | Context | Max output | Official in / out (per 1M) |
|---|---|---|---|---|
${modelRows}

Per-model detail pages: ${claudeModels.map((m) => `${SITE_ORIGIN}${modelPath(m.slug)}`).join(", ")}.

## First request (curl)

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

## Official SDKs

Set the base URL and reuse the official Anthropic SDKs unchanged.

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

## Coding tools

Claude Code, Cursor, Cline, Continue, Zed, Aider, Roo Code, LangChain and LiteLLM all work by pointing the Anthropic base URL at \`${API_BASE_URL}\`. For Claude Code: \`export ANTHROPIC_BASE_URL=${API_BASE_URL}\` and \`export ANTHROPIC_API_KEY=sk-pool-…\`.

## Capability parity

Everything the Anthropic Messages API supports passes through unchanged:

- **Streaming:** \`"stream": true\` returns standard Anthropic SSE events.
- **Tool use / function calling:** \`tools\` and \`tool_choice\` work identically; multi-turn tool loops supported.
- **Prompt caching:** \`cache_control\` breakpoints are honored and billed at cache read/write rates.
- **Vision:** image content blocks are supported.
- **System prompts, stop sequences, temperature, top_p, max_tokens:** identical semantics.

## Error codes

| Status | Meaning | What to do |
|---|---|---|
| 401 | API key missing, invalid or revoked | Send a valid \`sk-pool-…\` in \`x-api-key\`; if revoked, create a new key. |
| 402 | Prepaid balance too low | Top up any whole-dollar amount; retry after crediting. |
| 429 | Rate limit or temporary upstream capacity | Honor \`Retry-After\`; retry with capped exponential backoff and jitter. |
| 5xx | Temporary gateway or upstream failure | Retry with bounded backoff; keep the request ID and avoid duplicate attempts. |

## Pricing

Prepaid, per-token at official Anthropic rates minus your tier discount (${pct(DISCOUNT_BASE)} base, up to ${pct(DISCOUNT_MAX)}). No fixed packages or subscriptions; balance never expires. Full tier table: ${SITE_ORIGIN}/md/plans.

## Get started

- Create a key: ${SITE_ORIGIN}/register (Google or GitHub sign-up gets $10 of Claude usage at official prices)
- All guides (Markdown): ${SITE_ORIGIN}/docs/learn
- Machine-readable index: ${SITE_ORIGIN}/md
- Support: Telegram and apitokensale@gmail.com (English, Russian)
`
  );
}

/** Model catalog with exact IDs, context, limits and discounted price ranges. */
export function buildModelsMarkdown(): string {
  const sections = claudeModels
    .map((m) => {
      const inFrom = formatUsd(m.inputPerM * (1 - DISCOUNT_BASE));
      const inBest = formatUsd(m.inputPerM * (1 - DISCOUNT_MAX));
      const outFrom = formatUsd(m.outputPerM * (1 - DISCOUNT_BASE));
      const outBest = formatUsd(m.outputPerM * (1 - DISCOUNT_MAX));
      return [
        `## ${m.name}`,
        "",
        `- **Model ID:** \`${m.id}\``,
        `- **Tier:** ${m.tier}`,
        `- **Context window:** ${m.context}`,
        `- **Max output:** ${m.maxOutput}`,
        `- **Official price (per 1M):** $${m.inputPerM} input / $${m.outputPerM} output`,
        `- **Your price (per 1M):** input ${inFrom} → ${inBest}, output ${outFrom} → ${outBest} (${pct(DISCOUNT_BASE)}–${pct(DISCOUNT_MAX)} off)`,
        `- **Best for:** ${m.bestFor.join(" ")}`,
        `- **Detail page:** ${SITE_ORIGIN}${modelPath(m.slug)}`,
      ].join("\n");
    })
    .join("\n\n");

  return (
    frontmatter({
      title: "apiToken.sale — Claude model catalog",
      description:
        "Every Claude model available through apiToken.sale with exact API IDs, context windows, max output and discounted per-token pricing.",
      url: `${SITE_ORIGIN}/models`,
      language: "en",
    }) +
    `# Claude model catalog

All models run on one \`sk-pool-…\` key and one prepaid balance via \`${API_BASE_URL}\`. Use the model ID unchanged in the \`model\` field.

${sections}

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
      title: "apiToken.sale — Claude API pricing tiers",
      description:
        "apiToken.sale progressive discount tiers: 60% off by default, up to 80% off with cumulative top-ups. Prepaid per-token billing at official Anthropic rates.",
      url: `${SITE_ORIGIN}/plans`,
      language: "en",
    }) +
    `# Pricing & discount tiers

Top up any whole-dollar amount. Each request is billed at the official Anthropic token price, then your active tier discount is applied and deducted from balance. No fixed packages, no subscriptions, balance never expires.

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

/** Index of every machine-readable Markdown document on the site. */
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
- Model catalog (exact IDs, context, pricing): ${SITE_ORIGIN}/md/models
- Pricing & discount tiers: ${SITE_ORIGIN}/md/plans

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
