// Classify whether a visit arrived from an AI assistant, so we can measure the
// ROI of GEO / AI-citation optimization. Pure function — no browser access.

// Matched against the referrer HOSTNAME only (never the query string, so a
// Google search for "claude api" is not misread as a Claude referral).
const AI_HOST_TABLE: Array<[RegExp, string]> = [
  [/(^|\.)chatgpt\.com$|(^|\.)chat\.openai\.com$/i, "ChatGPT"],
  [/(^|\.)perplexity\.ai$/i, "Perplexity"],
  [/(^|\.)gemini\.google\.com$|(^|\.)bard\.google\.com$/i, "Gemini"],
  [/(^|\.)claude\.ai$/i, "Claude"],
  [/(^|\.)copilot\.microsoft\.com$|(^|\.)copilot\.cloud\.microsoft$/i, "Copilot"],
  [/(^|\.)grok\.com$|(^|\.)x\.ai$/i, "Grok"],
  [/(^|\.)you\.com$/i, "You.com"],
  [/(^|\.)phind\.com$/i, "Phind"],
  [/(^|\.)poe\.com$/i, "Poe"],
  [/(^|\.)mistral\.ai$/i, "Mistral"],
  [/(^|\.)deepseek\.com$/i, "DeepSeek"],
  [/(^|\.)kimi\.com$|(^|\.)moonshot\.cn$/i, "Kimi"],
  [/(^|\.)doubao\.com$/i, "Doubao"],
];

// Matched against a short utm_source token (an exact word, not a URL).
const AI_UTM_TABLE: Array<[RegExp, string]> = [
  [/^(chatgpt|openai)$/i, "ChatGPT"],
  [/^perplexity$/i, "Perplexity"],
  [/^(gemini|bard)$/i, "Gemini"],
  [/^claude$/i, "Claude"],
  [/^copilot$/i, "Copilot"],
  [/^grok$/i, "Grok"],
  [/^deepseek$/i, "DeepSeek"],
  [/^(kimi|moonshot)$/i, "Kimi"],
  [/^doubao$/i, "Doubao"],
];

function hostname(referrer: string): string | null {
  if (!referrer) return null;
  try {
    return new URL(referrer).hostname;
  } catch {
    return null;
  }
}

export function detectAiSource(referrer: string | null | undefined, utmSource = ""): string | null {
  const host = hostname(referrer ?? "");
  if (host) {
    for (const [pattern, name] of AI_HOST_TABLE) {
      if (pattern.test(host)) return name;
    }
  }
  const utm = utmSource.trim().toLowerCase();
  if (utm) {
    for (const [pattern, name] of AI_UTM_TABLE) {
      if (pattern.test(utm)) return name;
    }
  }
  return null;
}
