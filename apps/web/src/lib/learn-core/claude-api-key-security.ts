import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-key-security",
  cluster: "integrate",
  title: "Securing Your Claude API Key",
  h1: "Keep your Claude API key secure",
  description: "How to protect a Claude API key on apiToken.sale with a lifetime spending limit, optional expiration, separate named keys, prompt revocation, and safe secret storage.",
  keywords: ["claude api key security", "protect api key", "rotate claude api key", "claude api key management", "secure anthropic key"],
  dek: "Your key spends real balance, so treat it like a credential. apiToken.sale gives you controls to limit blast radius if a key ever leaks.",
  sections: [
    { h2: "Controls that limit risk", blocks: [
      { type: "list", items: [
        "Set a lifetime spending limit for the key.",
        "Choose an expiration date when temporary access should end automatically.",
        "Issue a separate, clearly named key per tool or environment.",
        "To replace a key, create the replacement, update the client, then revoke the old key.",
      ] },
    ] },
    { h2: "Basic hygiene", blocks: [
      { type: "list", items: [
        "Never commit keys to git or paste them into chats.",
        "Store keys in environment variables or a secret manager.",
        "Revoke and rotate immediately if a key is exposed.",
      ] },
      cta(),
    ] },
  ],
  faq: [
    { q: "How do I limit damage if a key leaks?", a: "Use a lifetime spending limit and expiration date, keep separate named keys per client, and revoke the exposed key immediately." },
    { q: "Where should I store my key?", a: "In environment variables or a secret manager — never committed to git or shared in chats." },
  ],
  related: ["claude-api-best-practices", "claude-api-rate-limits", "claude-code-api-key", "how-billing-works"],
};
