import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-code-without-subscription",
  cluster: "free",
  title: "Use Claude Code Without a Subscription",
  h1: "Claude Code without the $200/month plan",
  description: "Run Claude Code on pay-as-you-go API balance instead of a monthly subscription. Set ANTHROPIC_BASE_URL to router.apitoken.sale and pay only for what you use.",
  keywords: ["claude code without subscription", "claude code api key", "claude code pay as you go", "claude code cheap", "claude code no subscription", "free claude api", "claude api free", "claude api no credit card", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "Claude Code does not have to mean a fixed monthly plan. Point it at an API key with prepaid balance and you pay per token — ideal if your usage is spiky or you just want to try it.",
  sections: [
    { h2: "Two environment variables", blocks: [
      { type: "code", code: `export ANTHROPIC_BASE_URL=${BASE}\nexport ANTHROPIC_API_KEY=${KEY}\n\n# then just run\nclaude` },
      { type: "p", text: "That is the entire change. Claude Code keeps every feature — it simply bills against your prepaid balance at a discount instead of a subscription." },
    ] },
    { h2: "When pay-as-you-go wins", blocks: [
      { type: "list", items: [
        "Occasional or bursty usage where a flat monthly fee is wasteful.",
        "Trying Claude Code before committing to a plan.",
        "Keeping several tools on one balance and one key.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "Does Claude Code work with a custom API key?", a: "Yes. Set ANTHROPIC_BASE_URL and ANTHROPIC_API_KEY and Claude Code uses your key and balance directly." },
    { q: "Do I lose any features?", a: "No. Claude Code behaves identically; only billing changes from a subscription to prepaid per-token usage." },
  ],
  related: ["claude-api-key-for-cursor", "cheapest-claude-api", "claude-opus-api", "anthropic-sdk-base-url"],
};
