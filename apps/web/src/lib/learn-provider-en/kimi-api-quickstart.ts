import type { LearnArticle } from "../learn";
import { ROUTER } from "./shared";

export const article: LearnArticle = {
    slug: "kimi-api-quickstart",
    cluster: "integrate",
    title: "Kimi API Quickstart",
    h1: "Kimi API quickstart with the Anthropic SDK",
    description: "Call Kimi K3 and Kimi for Coding through apiToken.sale using the Anthropic Messages API, x-api-key, namespaced model IDs, terminal usage and one shared balance.",
    keywords: ["kimi api quickstart", "kimi api tutorial", "kimi anthropic api", "kimi k3 api example", "kimi for coding api", "kimi api curl"],
    dek: "Kimi speaks the Anthropic Messages protocol on the unified router. Existing Anthropic clients need only a custom base URL, the apiToken.sale key and an explicit kimi/* model ID.",
    sections: [
      { h2: "First request with curl", blocks: [
        { type: "code", code: "curl " + ROUTER + "/v1/messages \\\n  -H \"x-api-key: $APITOKEN_API_KEY\" \\\n  -H \"anthropic-version: 2023-06-01\" \\\n  -H \"Content-Type: application/json\" \\\n  -d '{\"model\":\"kimi/k3-256k\",\"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with exactly: connected\"}]}'" },
        { type: "p", text: "Terminal usage follows the Anthropic response shape, so existing usage parsers keep working. The route accepts stream: true, but provider-boundary incrementality remains under live validation." },
      ] },
      { h2: "Use the Anthropic Python SDK", blocks: [
        { type: "code", code: [
          "import os",
          "from anthropic import Anthropic",
          "",
          "client = Anthropic(",
          "    api_key=os.environ[\"APITOKEN_API_KEY\"],",
          "    base_url=\"" + ROUTER + "\",",
          ")",
          "",
          "message = client.messages.create(",
          "    model=\"kimi/kimi-for-coding\",",
          "    max_tokens=512,",
          "    messages=[{\"role\": \"user\", \"content\": \"Reply with exactly: connected\"}],",
          ")",
          "print(message.content[0].text)",
        ].join("\n") },
        { type: "note", text: "Do not substitute an official Open Platform ID such as kimi-k2.7-code. The public router accepts the subscription aliases shown by GET /v1/models. OpenAI-compatible clients can reach the same Kimi aliases through the universal /v1 lane." },
      ] },
    ],
    faq: [
      { q: "Can I use the Anthropic SDK for Kimi?", a: "Yes. Point its base_url at https://router.apitoken.sale and choose a kimi/* model ID from the scoped catalog." },
      { q: "Can I set stream: true on the Kimi route?", a: "The route accepts it, but upstream and public chunk incrementality are still being live-verified. Use non-stream mode when chunk timing matters." },
      { q: "What model ID should I start with?", a: "Use kimi/kimi-for-coding for a coding-oriented default or kimi/k3-256k when you need K3 reasoning without the full 1M window." },
    ],
    related: ["how-to-buy-kimi-api-key", "kimi-api-for-claude-code", "kimi-api-for-kimi-code", "kimi-api-for-opencode"],
    published: "2026-08-09",
    updated: "2026-08-09",
  };
