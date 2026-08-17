import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-for-vs-code",
  cluster: "integrate",
  title: "Claude API in VS Code (Cline, Continue)",
  h1: "Use the Claude API in VS Code",
  description: "Run Claude in VS Code with Cline or Continue using an apiToken.sale key. Set the Anthropic base URL to router.apitoken.sale and pay per token at a discount.",
  keywords: ["claude api vs code", "cline claude api", "continue claude api", "claude in vscode", "vscode anthropic api key", "claude api key", "anthropic-compatible api", "claude api base url", "claude api setup", "claude api integration", "claude api vscode"],
  dek: "Free VS Code agents like Cline and Continue accept any Anthropic-compatible endpoint, so you can code with Claude inside VS Code on discounted balance.",
  sections: [
    { h2: "Cline", blocks: [
      { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : ${BASE}\nAPI Key      : ${KEY}\nModel        : claude-opus-4-8` },
    ] },
    { h2: "Continue", blocks: [
      { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "${BASE}",\n    "apiKey": "${KEY}",\n    "model": "claude-opus-4-8"\n  }]\n}` },
      cta(),
    ] },
    { h2: "Which extension and troubleshooting", blocks: [
      { type: "p", text: "Cline is a strong default for autonomous edits; Continue is lighter and good for inline chat and completions. Both are free and use your prepaid balance." },
      { type: "list", items: [
        "401 Unauthorized: the API key or base URL is wrong.",
        "Model not found: use a current ID such as claude-sonnet-5 or claude-opus-4-8.",
        "Slow or 429: reduce concurrency and respect Retry-After.",
      ] },
    ] },
  ],
  faq: [
    { q: "Which VS Code extensions work?", a: "Any extension that supports an Anthropic-compatible endpoint, including Cline and Continue, works with an apiToken.sale key." },
    { q: "Do I need a paid extension?", a: "No. Cline and Continue are free; you only pay for the Claude API usage against your prepaid balance." },
  ],
  related: ["claude-api-key-for-cursor", "anthropic-sdk-base-url", "claude-api-quick-setup", "claude-code-without-subscription"],
};
