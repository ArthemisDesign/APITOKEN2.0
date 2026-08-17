import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-quick-setup",
  cluster: "integrate",
  title: "Claude API Quickstart: From Key to First Call in Minutes",
  h1: "Claude API quickstart: set up and make your first call",
  description: "Claude API quickstart: create one key, point any Anthropic-compatible client at router.apitoken.sale, and send your first /v1/messages request with curl, Python, TypeScript or your IDE.",
  keywords: ["claude api quickstart", "claude api setup", "claude api first request", "anthropic messages api", "claude api base url", "claude api curl example", "claude api hello world", "claude api key", "claude api getting started", "claude api tutorial", "buy claude api access"],
  dek: "This Claude API quickstart takes you from a fresh account to a completed /v1/messages call in minutes. You need exactly three things: one sk-pool key, the router.apitoken.sale base URL, and two HTTP headers. Everything after that is the standard Anthropic Messages API, so the same code runs against the official endpoint unchanged.",
  sections: [
    { h2: "What a Claude API quickstart actually requires", blocks: [
      { type: "p", text: "A working Claude API setup is not a SDK install or a week of onboarding — it is one HTTP POST with two headers. Sign up, generate a key, and send a messages request; the first 2xx usually lands faster than the coffee you made while reading this page. The endpoint speaks the exact Anthropic Messages protocol, which means every tutorial, SDK and coding agent built for Claude already knows how to talk to it." },
      { type: "list", items: [
        "A free account — no approval, no waitlist, no Anthropic account required.",
        "One API key (it looks like sk-pool-…) that works across every supported model, including Claude, GPT, Gemini and Kimi.",
        `The base URL ${BASE} — the single endpoint for new integrations.`,
        "Two headers on every request: x-api-key with your key, and anthropic-version: 2023-06-01.",
      ] },
    ] },
    { h2: "Create the key and choose your endpoint", blocks: [
      { type: "steps", items: [
        "Sign up with Google, GitHub or email, then open the dashboard — there is no review queue.",
        "Generate a key. It is shown once; store it in an environment variable, not in source code.",
        `Set your client's base URL to ${BASE} and confirm it sends requests to POST /v1/messages.`,
      ] },
      { type: "code", code: `Base URL:  ${BASE}\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: ${KEY}\n           anthropic-version: 2023-06-01` },
      { type: "p", text: "The key is live on the next request — there is no activation delay. If your balance is empty, add funds first: top-ups accept any whole-dollar amount, so a single dollar is enough to validate the whole pipeline end to end." },
    ] },
    { h2: "Send the first request with curl", blocks: [
      { type: "p", text: "Prove the path with the smallest possible call before wiring anything into an app. max_tokens is mandatory on the Messages API — omitting it is the most common first-call mistake." },
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      { type: "p", text: "A successful response is a JSON object whose content field is an array of blocks — for a plain reply, one block of type text. Two fields are worth reading on every call during setup: stop_reason tells you whether the model finished (end_turn) or hit your max_tokens ceiling, and usage reports the exact input_tokens and output_tokens you were billed for. If content comes back empty with stop_reason: max_tokens, raise the limit rather than retrying the same request." },
    ] },
    { h2: "The same call from Python or TypeScript", blocks: [
      { type: "p", text: "The official Anthropic SDKs accept a custom base URL, so moving from curl to real code is a one-line override. Model IDs, message shapes, system prompts and tool use all behave exactly as they do against api.anthropic.com." },
      { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(msg.content[0].text)` },
      { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
      { type: "link", text: "Full SDK walkthrough: anthropic-sdk-base-url", href: "/docs/learn/anthropic-sdk-base-url" },
    ] },
    { h2: "Turn on streaming before you build a UI", blocks: [
      { type: "p", text: "Anything a human waits on — chat, code completion, an agent loop with visible progress — should stream. Add \"stream\": true to the same request body and the response becomes Server-Sent Events: a message_start envelope, a sequence of content_block_delta events carrying text fragments, and a message_stop. Your client assembles the fragments; nothing else about the request changes." },
      { type: "code", code: `curl -N ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Count to five."}]\n  }'` },
      { type: "note", text: "Two streaming pitfalls: without -N (or your HTTP client's no-buffer mode) curl buffers the whole SSE body and looks identical to a non-streaming call; and the final usage accounting arrives in the terminal message_delta event, not in a JSON body — read it there if you meter spend per request." },
    ] },
    { h2: "Point your IDE or coding agent at the same key", blocks: [
      { type: "p", text: "Because the endpoint is protocol-identical, any tool with an Anthropic provider setting works by changing two fields. In Cursor, for example: Settings → Models → Anthropic API, set the base URL and paste the key, then pick a current model ID." },
      { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : ${BASE}\nAPI key  : ${KEY}\nModel    : claude-opus-4-8` },
      { type: "p", text: "The same two-field change covers VS Code extensions such as Cline and Continue, and terminal agents that read ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY from the environment. One key, one prepaid balance, every tool." },
      { type: "link", text: "Dedicated guide: claude-api-key-for-cursor", href: "/docs/learn/claude-api-key-for-cursor" },
      { type: "link", text: "Current model lineup and per-model pricing", href: "/models" },
    ] },
    { h2: "First-call errors, decoded", blocks: [
      { type: "p", text: "Almost every failed first call is one of four statuses. Read the body too — errors come back in the Anthropic error envelope with a message that names the offending field." },
      { type: "table", headers: ["Status", "What it means", "Fix"], rows: [
        ["400 Bad Request", "Malformed request body — usually a missing max_tokens or an unknown model ID", "Set max_tokens; use a current model ID such as claude-opus-4-8"],
        ["401 Unauthorized", "Missing or wrong x-api-key, or the request went to the wrong base URL", `Re-check the key was pasted in full and the base URL is ${BASE}`],
        ["402 / insufficient balance", "The prepaid balance cannot cover the request", "Top up any whole-dollar amount and retry"],
        ["429 Too Many Requests", "Concurrency or rate ceiling hit", "Respect the Retry-After header and lower concurrency"],
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "What base URL do I use for the Claude API quickstart?", a: `Use ${BASE} with any Anthropic-compatible tool and send requests to /v1/messages. Existing integrations on the legacy https://api.apitoken.sale host keep working — the unified router is the recommended endpoint for new setups.` },
    { q: "Which auth header does the Claude API require?", a: "Send x-api-key with your key and anthropic-version: 2023-06-01, exactly like the official Anthropic API. Do not use Authorization: Bearer on this surface — that header belongs to the OpenAI-compatible lane." },
    { q: "Do I need an Anthropic account or a credit card on file?", a: "No Anthropic account is required — you sign up with Google, GitHub or email and get your own sk-pool key. Balance is prepaid: you top up any whole-dollar amount and it is spent only when requests run." },
    { q: "What is the cheapest way to verify my setup works?", a: "Top up the smallest whole-dollar amount and send one max_tokens: 1 request — a successful 2xx proves auth, endpoint and billing in a single call. New accounts created with Google or GitHub also start with $5 of platform bonus credit, which can cover the test entirely." },
    { q: "Why does my first call return 400 even with a valid key?", a: "Almost always a missing max_tokens field or a model ID that is not enabled — the Messages API rejects requests without max_tokens. Use a current ID such as claude-opus-4-8 and set an explicit token limit." },
    { q: "Can I use the same key for streaming and tool use?", a: "Yes. Streaming is a \"stream\": true flag on the same request, and tool use follows the standard Anthropic schema — no separate key, plan or endpoint is involved." },
  ],
  related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "how-to-buy-claude-api-key", "claude-api-for-vs-code"],
  updated: "2026-08-17",
};
