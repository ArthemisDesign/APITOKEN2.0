import { learnArticles, renderLearnMarkdown } from "@/lib/learn";
import { SITE_ORIGIN } from "@/lib/seo";

export const dynamic = "force-static";

function build(): string {
  const header = [
    "# apiToken.sale — full Claude API reference for LLMs",
    "",
    "> Complete, self-contained Markdown of every apiToken.sale guide. apiToken.sale is an independent Claude API gateway serving the Anthropic Messages API and the supported Claude line (Opus, Sonnet, Haiku) from prepaid balance at 60–80% off official spend. API base URL: https://api.apitoken.sale. No Anthropic account required; pay by card or crypto.",
    "",
    "---",
    "",
  ].join("\n");
  const body = learnArticles
    .map((article) => renderLearnMarkdown(article, SITE_ORIGIN))
    .join("\n\n---\n\n");
  return header + body + "\n";
}

export function GET(): Response {
  return new Response(build(), {
    status: 200,
    headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "public, max-age=3600" },
  });
}
