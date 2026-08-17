import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "anthropic-sdk-base-url",
  cluster: "integrate",
  title: "Use Anthropic SDKs with a Custom Base URL",
  h1: "Point the Anthropic SDK at apiToken.sale",
  description: "Use the official Anthropic Python and TypeScript SDKs with apiToken.sale by setting base_url to router.apitoken.sale. Same SDK, same code, lower cost per token.",
  keywords: ["anthropic sdk base url", "anthropic python sdk custom endpoint", "claude sdk base url", "anthropic typescript sdk", "claude api sdk", "anthropic_base_url environment variable", "claude api custom endpoint", "anthropic sdk proxy", "@anthropic-ai/sdk baseurl", "claude api gateway url"],
  dek: "Every official Anthropic SDK accepts a custom Anthropic SDK base URL, so moving to apiToken.sale is a one-argument change. Your model IDs, message code and streaming logic stay exactly the same — only the endpoint and the per-token price change.",
  updated: "2026-08-17",
  sections: [
    { h2: "One argument switches the endpoint", blocks: [
      { type: "p", text: `Both official Anthropic SDKs — Python and TypeScript — let you override the API root when you construct the client. Set it to ${BASE} and every request your code already makes is served by apiToken.sale's gateway instead of api.anthropic.com. Nothing else in your codebase moves: same anthropic package, same Messages API, same model IDs like claude-opus-4-8, same response objects.` },
      { type: "p", text: "What changes is billing. Each call is metered at official Anthropic token rates, your flat 50% discount is subtracted, and the net amount is drawn from a prepaid balance you top up in whole-dollar amounts. No subscription, no per-seat fee — idle days cost nothing." },
    ] },
    { h2: "Python: base_url on the client", blocks: [
      { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      { type: "p", text: "The async client takes the identical keyword: AsyncAnthropic(base_url=..., api_key=...). Streaming via client.messages.stream, tool use, system prompts and prompt caching all ride on the same connection — there is no separate endpoint to configure for them." },
      { type: "note", text: "Pass the bare root, not a path. The SDK appends /v1/messages itself, so base_url=\".../v1\" produces requests to /v1/v1/messages and a 404. The same rule applies to the TypeScript SDK." },
    ] },
    { h2: "TypeScript: baseURL on the client", blocks: [
      { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
      { type: "p", text: "The @anthropic-ai/sdk package sends the x-api-key and anthropic-version headers for you, exactly as it does against the official endpoint. Retries, timeouts and error classes (APIError, RateLimitError and friends) behave identically, so existing error handling keeps working." },
    ] },
    { h2: "Prefer environment variables in shared code", blocks: [
      { type: "p", text: "Both SDKs read ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY from the environment when the constructor arguments are absent. That makes the switch a deployment detail instead of a code change — useful when the same repository runs against different endpoints in development and production." },
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# your code now constructs Anthropic() with no arguments` },
      { type: "p", text: "Tools built on top of the SDK inherit the same variables. Claude Code, for example, honours ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY directly, and frameworks like LangChain or LiteLLM forward the same environment to their Anthropic client underneath. Explicit constructor arguments win over environment variables when both are set, so a one-off override in a script never leaks into your deployed configuration." },
    ] },
    { h2: "What crosses the gateway unchanged", blocks: [
      { type: "list", items: [
        "The full Messages API surface: POST /v1/messages with the same request and response JSON.",
        "SSE streaming — incremental chunks arrive exactly as from api.anthropic.com.",
        "Tool use and function calling, including multi-turn tool_result loops.",
        "System prompts, vision inputs and prompt caching with cache_control breakpoints.",
        "The usage object on every response, so your token- and cost-tracking code keeps working.",
        "Model IDs: claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 and the rest of the supported catalog.",
      ] },
      { type: "p", text: "One key covers every supported model — Claude alongside GPT, Gemini and Kimi — so a multi-provider project keeps a single credential and a single balance. Per-request spend and the applied discount are visible in the dashboard after each call." },
      { type: "link", text: "Supported model IDs and per-model pricing", href: "/models" },
      { type: "link", text: "Estimate monthly spend in the cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "First-request checklist and common errors", blocks: [
      { type: "steps", items: [
        "Create a free account, open the dashboard and generate a key — it looks like sk-pool-… and works across the supported Claude, GPT, Gemini and Kimi models.",
        `Set base_url / baseURL to ${BASE} in code, or export ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY.`,
        "Run the Python or TypeScript snippet above once and confirm you get a normal Anthropic message response.",
        "Open the dashboard and verify the request appears with its token usage, cost and discount.",
      ] },
      { type: "table", headers: ["Status", "Meaning", "Fix"], rows: [
        ["401 Unauthorized", "Missing or wrong x-api-key, or wrong base URL", "Re-check the key and that the URL is the bare root"],
        ["400 Bad Request", "Malformed request body", "Check the model ID and that max_tokens is set"],
        ["402 Payment Required", "Insufficient prepaid balance", "Top up any whole-dollar amount in the dashboard"],
        ["429 Too Many Requests", "Concurrency above the current limit", "Respect Retry-After and lower parallelism"],
      ] },
      { type: "p", text: "Because the SDK, the wire format and the error taxonomy are identical on both endpoints, the switch is reversible at any time: point base_url back to api.anthropic.com (or delete the override) and the same code talks to Anthropic directly again. Many teams keep both clients constructed side by side during a migration week and route a small percentage of traffic to the new endpoint before flipping fully." },
      { type: "note", text: "Existing integrations on the legacy https://api.apitoken.sale host keep working. The unified router at router.apitoken.sale is the recommended endpoint for new setups because one base URL serves all four providers." },
      cta(),
    ] },
  ],
  faq: [
    { q: "Can I keep using the official Anthropic SDK?", a: "Yes. Set base_url (Python) or baseURL (TypeScript) to https://router.apitoken.sale and everything else — imports, model IDs, streaming, error handling — stays the same." },
    { q: "Do model IDs change when I switch base URL?", a: "No. Use the same IDs as on the official API, such as claude-opus-4-8, claude-sonnet-5 and claude-haiku-4-5." },
    { q: "Should the base URL end with /v1?", a: "No. The SDK appends /v1/messages to whatever root you pass, so a trailing /v1 breaks the path. Pass https://router.apitoken.sale exactly." },
    { q: "Do streaming and tool use work through a custom base URL?", a: "Yes. The gateway serves the standard Anthropic Messages API, so SSE streaming, tool calling, system prompts and prompt caching behave exactly as with api.anthropic.com." },
    { q: "How do I switch back to Anthropic later?", a: "Remove the base_url / baseURL argument or unset ANTHROPIC_BASE_URL. The SDK then defaults back to https://api.anthropic.com — no other code change is needed." },
  ],
  related: ["claude-api-quick-setup", "claude-api-for-vs-code", "claude-code-without-subscription", "claude-api-key-for-cursor"],
};
