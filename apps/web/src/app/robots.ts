import type { MetadataRoute } from "next";
import { absoluteUrl, SITE_ORIGIN } from "@/lib/seo";

// Private surfaces that must never be indexed.
const DISALLOW_PRIVATE = [
  "/dashboard",
  "/auth",
  "/login",
  "/register",
  "/reset-password",
  "/forgot-password",
  "/verify-email",
];

// Search + AI crawlers we explicitly welcome. Being named (rather than relying
// on the wildcard) maximises discoverability in classic search and in AI
// assistants that cite live sources, and overrides any managed AI-blocking
// default on the CDN.
const WELCOME_AGENTS = [
  // Classic search
  "Googlebot", "Bingbot", "YandexBot", "Yandex", "DuckDuckBot",
  "Baiduspider", "Sogou web spider", "Applebot",
  // Social unfurlers
  "Twitterbot", "facebookexternalhit",
  // AI training / grounding / live citation
  "GPTBot", "ChatGPT-User", "OAI-SearchBot",
  "ClaudeBot", "Claude-User", "Claude-SearchBot", "anthropic-ai",
  "PerplexityBot", "Perplexity-User",
  "Google-Extended", "Gemini-Deep-Research",
  "Applebot-Extended", "CCBot", "cohere-ai",
  "Meta-ExternalAgent", "MistralAI-User", "DuckAssistBot", "Amazonbot",
  "DeepSeekBot", "Qwen-Bot", "YouBot",
];

export default function robots(): MetadataRoute.Robots {
  return {
    rules: [
      { userAgent: "*", allow: "/", disallow: DISALLOW_PRIVATE },
      ...WELCOME_AGENTS.map((userAgent) => ({ userAgent, allow: "/", disallow: DISALLOW_PRIVATE })),
    ],
    sitemap: absoluteUrl("/sitemap.xml"),
    host: SITE_ORIGIN,
  };
}
