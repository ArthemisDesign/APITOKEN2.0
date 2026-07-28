import { absoluteUrl } from "./seo";

// Private surfaces that must never be indexed.
export const ROBOTS_DISALLOW = [
  "/dashboard",
  "/auth",
  "/login",
  "/register",
  "/reset-password",
  "/forgot-password",
  "/verify-email",
];

// Search + AI crawlers we explicitly welcome (see the wildcard group too).
export const ROBOTS_WELCOME_AGENTS = [
  "Googlebot", "Bingbot", "DuckDuckBot", "Baiduspider", "Sogou web spider", "Applebot",
  "Mail.RU_Bot", "PetalBot", "Twitterbot", "facebookexternalhit",
  "GPTBot", "ChatGPT-User", "OAI-SearchBot",
  "ClaudeBot", "Claude-User", "Claude-SearchBot", "anthropic-ai",
  "PerplexityBot", "Perplexity-User",
  "Google-Extended", "Gemini-Deep-Research",
  "Applebot-Extended", "CCBot", "cohere-ai",
  "Meta-ExternalAgent", "MistralAI-User", "DuckAssistBot", "Amazonbot",
  "DeepSeekBot", "Qwen-Bot", "YouBot",
];

// `e` is the legacy error-reference short-link parameter. /e/<code> now redirects to a
// fragment instead, but links shared before that change still carry it, and it never
// changes the page — so it must not create a separate indexable URL.
const CLEAN_PARAMS = "utm_source&utm_medium&utm_campaign&utm_term&utm_content&ref&fbclid&gclid&yclid&e";

function group(agent: string, extra: string[] = []): string {
  return [
    `User-agent: ${agent}`,
    "Allow: /",
    ...ROBOTS_DISALLOW.map((path) => `Disallow: ${path}`),
    ...extra,
  ].join("\n");
}

/** Full robots.txt with AI-crawler allowlist, Content-Signal, and Yandex Clean-param. */
export function buildRobotsTxt(): string {
  const blocks: string[] = [];

  // Default group: welcome everyone, declare AI content signals, block private.
  blocks.push([
    "User-agent: *",
    "Content-Signal: search=yes, ai-input=yes, ai-train=yes",
    "Allow: /",
    ...ROBOTS_DISALLOW.map((path) => `Disallow: ${path}`),
  ].join("\n"));

  // Yandex — allow, and strip tracking params so RU URLs do not duplicate.
  blocks.push(group("YandexBot", [`Clean-param: ${CLEAN_PARAMS} /`]));
  blocks.push(group("Yandex", [`Clean-param: ${CLEAN_PARAMS} /`]));
  blocks.push(group("YandexAdditionalBot"));

  // Named search + AI crawlers.
  for (const agent of ROBOTS_WELCOME_AGENTS) blocks.push(group(agent));

  blocks.push(`Sitemap: ${absoluteUrl("/sitemap.xml")}`);

  return blocks.join("\n\n") + "\n";
}
