import type { LearnArticle } from "../learn";
import { cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-3-5-vs-claude-4",
  cluster: "compare",
  title: "Claude 3.5 vs Claude 4 — What Changed",
  h1: "Claude 3.5 vs Claude 4: what changed",
  description: "Moving from Claude 3.5 to the current Claude 4 line? What improved, updated model IDs, and how to switch on apiToken.sale with one base-URL change.",
  keywords: ["claude 3.5 vs 4", "claude 4 vs 3.5", "claude model migration", "upgrade claude model", "new claude models", "claude api pricing", "claude api tokens", "how claude api works", "claude api explained", "anthropic api"],
  dek: "The current Claude line is a clear step up from 3.5 in reasoning and coding. Migrating is mostly a model-ID change — everything else stays the same.",
  sections: [
    { h2: "What improved", blocks: [
      { type: "p", text: "The Opus, Sonnet and Haiku 4-series models improve on 3.5 for agentic coding, long-context consistency and complex reasoning, while keeping the same Messages API." },
    ] },
    { h2: "How to migrate", blocks: [
      { type: "p", text: "Swap the model ID to a current one — for example claude-opus-4-8, claude-sonnet-5 or claude-haiku-4-5 — and keep your existing request code. On apiToken.sale it is the same key and endpoint." },
      cta(),
    ] },
  ],
  faq: [
    { q: "Is Claude 4 much better than 3.5?", a: "Yes, especially for coding, agents and long-context tasks, while using the same API format." },
    { q: "Is migrating hard?", a: "No — update the model ID (e.g. to claude-sonnet-5) and your existing Messages API code keeps working." },
  ],
  related: ["best-claude-model-for-coding", "claude-opus-vs-sonnet", "claude-sonnet-api", "claude-api-quick-setup"],
};
