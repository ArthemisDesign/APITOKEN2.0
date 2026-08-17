import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-quick-setup",
  cluster: "integrate",
  title: "Claude API Setup in Two Minutes",
  h1: "Set up the Claude API in two minutes",
  description: "A two-minute Claude API quickstart: create a key, set your base URL to router.apitoken.sale, and send your first /v1/messages request with curl, Python or your IDE.",
  keywords: ["claude api quickstart", "claude api setup", "claude api first request", "anthropic messages api", "claude api base url", "buy claude api", "claude api access", "claude api tokens", "claude api top up", "claude api reseller", "claude api provider"],
  dek: "This is the fastest path from zero to a working Claude API call. Everything below uses the standard Anthropic Messages API, so it drops straight into your existing code.",
  sections: [
    { h2: "1. Create a key", blocks: [ { type: "p", text: "Sign up, open the dashboard, and generate a key. It looks like sk-pool-… and works across every supported model." } ] },
    { h2: "2. Set your endpoint", blocks: [
      { type: "p", text: "Point any Anthropic-compatible client at the gateway:" },
      { type: "code", code: `Base URL:  ${BASE}\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: ${KEY}\n           anthropic-version: 2023-06-01` },
    ] },
    { h2: "3. Send your first request", blocks: [
      { type: "code", code: `curl ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      cta(),
    ] },
    { h2: "Common first-call errors", blocks: [
      { type: "list", items: [
        "401 Unauthorized — missing or wrong x-api-key, or wrong base URL.",
        "400 Bad Request — check the model ID and that max_tokens is set.",
        "429 Too Many Requests — respect Retry-After and lower concurrency.",
        "402 / insufficient balance — top up any whole-dollar amount.",
      ] },
    ] },
  ],
  faq: [
    { q: "What base URL do I use?", a: `Use ${BASE} with any Anthropic-compatible tool and send requests to /v1/messages. Existing integrations on the legacy https://api.apitoken.sale host keep working — the unified router is the recommended endpoint for new setups.` },
    { q: "Which auth header is required?", a: "Send x-api-key with your key and anthropic-version, exactly like the official Anthropic API." },
  ],
  related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "how-to-buy-claude-api-key", "claude-api-for-vs-code"],
};
