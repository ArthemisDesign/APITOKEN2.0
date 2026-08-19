// Shared building blocks for the hand-written core learn articles.
// Extracted from learn.ts so each core article can live in its own module
// under learn-core/ without importing values from learn.ts (type-only imports
// from learn.ts are fine and do not create a value cycle).

import type { LearnBlock } from "./learn";

export const BASE = "https://router.apitoken.sale";
export const OPENAI_BASE = "https://router.apitoken.sale/v1";
export const KEY = "sk-pool-•••";

export const cta = (): LearnBlock => ({ type: "note", text: "New accounts created with Google or GitHub start with $5 of platform bonus credit — valid on supported Claude, GPT, Gemini and Kimi models; email/password accounts do not receive the bonus." });
