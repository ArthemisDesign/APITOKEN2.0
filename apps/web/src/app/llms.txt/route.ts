import { clusterLabels, learnArticles, learnPath, LEARN_HUB_PATH, type LearnCluster } from "@/lib/learn";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

const clusterOrder: LearnCluster[] = ["buy", "free", "integrate", "compare", "explain"];

function build(): string {
  const lines: string[] = [
    "# apiToken.sale — Claude API access",
    "",
    "> apiToken.sale is an independent Claude API gateway. It serves the standard Anthropic Messages API and the full supported Claude line (Opus, Sonnet, Haiku) from prepaid balance at a progressive discount of 60% up to 80% off official spend. No Anthropic account, waitlist, or billing-country requirement; pay by bank card or cryptocurrency.",
    "",
    "## For AI agents",
    "",
    "Every guide under /docs/learn is available as clean Markdown at /md/docs/learn/<slug>. Example: " + SITE_ORIGIN + "/md/docs/learn/cheapest-claude-api",
    "",
    "## Key facts",
    "",
    "- Website: " + SITE_ORIGIN,
    "- API base URL: https://api.apitoken.sale",
    "- API format: Anthropic Messages API (POST /v1/messages), x-api-key + anthropic-version headers",
    "- Models: Claude Opus 4.8, Opus 4.7, Sonnet 5, Sonnet 4.6, Haiku 4.5 (one key and balance)",
    "- Billing: prepaid, per-token at official rates minus a 60–80% B2C discount; balance never expires",
    "- Payment: bank card or cryptocurrency",
    "- Free start: $10 of Claude usage at official API prices, no card required",
    "- Onboarding: instant, self-serve, no Anthropic account",
    "- Support: Telegram and apitokensale@gmail.com (English, Russian)",
    "",
    "## Guides",
    "",
    "- [All Claude API guides](" + SITE_ORIGIN + LEARN_HUB_PATH + ")",
  ];
  for (const cluster of clusterOrder) {
    const items = learnArticles.filter((article) => article.cluster === cluster);
    if (items.length === 0) continue;
    lines.push("", "### " + clusterLabels[cluster].label);
    for (const article of items) {
      lines.push(`- [${article.title}](${SITE_ORIGIN}${learnPath(article.slug)}) — ${article.description}`);
    }
  }
  lines.push("");
  return lines.join("\n");
}

export function GET(): Response {
  return new Response(build(), {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "public, max-age=3600" },
  });
}
