import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "anthropic-sdk-base-url",
  cluster: "integrate",
  title: "Use Anthropic SDKs with a Custom Base URL",
  h1: "Point the Anthropic SDK at apiToken.sale",
  description: "Use the official Anthropic Python and TypeScript SDKs with apiToken.sale by setting base_url to router.apitoken.sale. Same SDK, same code, lower cost per token.",
  keywords: ["anthropic sdk base url", "anthropic python sdk custom endpoint", "claude sdk base url", "anthropic typescript sdk", "claude api sdk"],
  dek: "The official Anthropic SDKs let you override the base URL, so switching to apiToken.sale is a one-line change — your model IDs and message code stay exactly the same.",
  sections: [
    { h2: "Python", blocks: [
      { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
    ] },
    { h2: "TypeScript", blocks: [
      { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
      cta(),
    ] },
    { h2: "Verify the switch worked", blocks: [
      { type: "p", text: "After changing the base URL, make one request and confirm you get a normal Anthropic response. Streaming, tool use and system prompts all behave exactly as with api.anthropic.com — only the billing endpoint changed." },
      { type: "list", items: [
        "A 401 means the key or base URL is wrong — re-check both.",
        "Keep the same model IDs; no code around messages needs to change.",
        "Read usage per request in the dashboard to confirm spend and your discount.",
      ] },
    ] },
  ],
  faq: [
    { q: "Can I keep using the official Anthropic SDK?", a: "Yes. Set base_url (Python) or baseURL (TypeScript) to apiToken.sale and everything else stays the same." },
    { q: "Do model IDs change?", a: "No. Use the same model IDs such as claude-opus-4-8 and claude-sonnet-5." },
  ],
  related: ["claude-api-quick-setup", "claude-api-for-vs-code", "claude-code-without-subscription", "claude-api-key-for-cursor"],
};
