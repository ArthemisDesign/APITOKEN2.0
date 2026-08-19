import type { LearnArticle } from "../learn";
import { cta, KEY, OPENAI_BASE } from "../learn-shared";

export const article: LearnArticle = {
  slug: "gpt-5-6-sol-vs-terra-vs-luna",
  cluster: "compare",
  title: "GPT-5.6 Sol vs Terra vs Luna: Which Tier?",
  h1: "GPT-5.6 Sol vs Terra vs Luna: pick by task, not by loyalty",
  description: "GPT-5.6 Sol vs Terra vs Luna compared: official and discounted prices per 1M tokens, shared 400K context and reasoning effort — all three tiers on one key.",
  keywords: ["gpt-5.6 sol vs terra", "gpt-5.6 terra vs luna", "gpt-5.6 sol vs terra vs luna", "best gpt-5.6 model for coding", "gpt-5.6 model comparison", "gpt-5.6-sol vs gpt-5.6-terra", "gpt-5.6 pricing tiers", "which gpt model for coding", "gpt-5.6 flagship vs balanced", "gpt-5.6 reasoning effort", "gpt model routing"],
  dek: "GPT-5.6 Sol, Terra and Luna are the same model family at three price points: identical 400K context, 128K output ceiling and reasoning controls, with token rates from $0.20/$1.20 up to $5/$30 per 1M. The right answer for most workloads is Terra as the default, Sol as the escalation tier, and Luna for high-volume mechanical steps — and on apiToken.sale all three run on one key against one prepaid balance at 50% off official rates.",
  sections: [
    { h2: "The short answer: Terra by default, Sol and Luna at the edges", blocks: [
      { type: "p", text: "Use gpt-5.6-terra for almost everything, escalate to gpt-5.6-sol when a task genuinely needs deeper reasoning, and push predictable bulk work down to gpt-5.6-luna. Terra keeps the full 400K context window, the 128K output ceiling and the complete reasoning-effort range at 40% of Sol's token price, which makes it the correct default for coding, production chat and agent loops." },
      { type: "p", text: "The expensive mistakes sit at both extremes. Running every request on Sol pays flagship rates for work Luna could finish; refusing to leave Luna burns retries on tasks it was never going to complete. Treat the three tiers as one system: Luna drafts the easy volume, Terra does the real work, Sol handles the exceptions." },
    ] },
    { h2: "One family, three meters on the same capabilities", blocks: [
      { type: "p", text: "Sol, Terra and Luna are not different products. They share the Responses and Chat Completions surfaces, SSE streaming, text-and-image input with text output, and the same reasoning-effort set — none through xhigh, plus max on the GPT-5.6 line. What changes per tier is capability depth, latency and the meter. All rates below are per 1M tokens; the discounted column is what actually leaves your prepaid balance." },
      { type: "table", headers: ["", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"], rows: [
        ["Official input / output", "$5 / $30", "$2 / $12", "$0.20 / $1.20"],
        ["Here after flat 50% off", "$2.50 / $15", "$1 / $6", "$0.10 / $0.60"],
        ["Cached input (official)", "$0.50", "$0.20", "$0.02"],
        ["Context window", "400K tokens", "400K tokens", "400K tokens"],
        ["Max output", "128K tokens", "128K tokens", "128K tokens"],
        ["Reasoning effort", "none → max", "none → max", "none → max"],
        ["Role", "Escalation tier", "Daily driver", "Volume tier"],
      ] },
      { type: "note", text: "The bare model ID gpt-5.6 is an alias of gpt-5.6-sol — it bills at Sol rates, not at some average of the family. Pin an explicit tier in production config so a silent default never routes routine traffic onto the flagship meter." },
    ] },
    { h2: "When Sol earns a meter 2.5× Terra's", blocks: [
      { type: "p", text: "Sol is the tier you rent, not the tier you live in. Its premium buys reasoning depth and consistency over long horizons — holding a large diff or a multi-step plan together without drifting. The trigger for escalation should be evidence, not vibes: a failed Terra attempt, a refactor spanning more files than you can hold in your head, or an architecture decision you cannot afford to reverse." },
      { type: "list", items: [
        "Multi-file refactors where a missed edge case costs more than the tokens.",
        "Subtle debugging — race conditions, memory corruption, flaky tests with no obvious cause.",
        "Architecture and design trade-off analysis, where a bad call dwarfs any token bill.",
        "A final review pass over Terra-generated diffs before they merge.",
        "Long autonomous agent runs that must stay coherent across hours of accumulated context.",
      ] },
      { type: "p", text: "Because output tokens cost six times input on Sol, the cheapest Sol call is a short one. Feed it a tight, well-scoped prompt — the failing test, the relevant diff, the exact question — rather than an unfiltered codebase dump." },
    ] },
    { h2: "When Luna beats Terra on unit economics", blocks: [
      { type: "p", text: "Luna costs 4% of Sol and a tenth of Terra per token, so any task it completes first try is almost free. Its limits are real, though: depth-sensitive work will fail on Luna and cost you a Terra or Sol retry anyway, which erases the saving. Route to Luna only work that is deterministic, narrow and easy to verify." },
      { type: "list", items: [
        "Classification, tagging, routing and intent detection in production traffic.",
        "Extraction and reformatting — JSON shaping, boilerplate, renames, throwaway scripts.",
        "Cheap sub-steps inside an agent loop: summarizing a tool result before the main model sees it.",
        "Latency-sensitive replies where first-token speed matters more than the last points of quality.",
      ] },
      { type: "note", text: "Measure the split before committing to it. If more than a small fraction of Luna's output needs a Terra redo, the effective cost of the 'cheap' tier is Luna plus the retry — usually worse than sending the task to Terra directly." },
    ] },
    { h2: "Switching tiers is a one-field change", blocks: [
      { type: "p", text: `There is no per-tier signup, plan or endpoint. One apiToken.sale key (it looks like ${KEY}) covers Sol, Terra and Luna — plus supported Claude, Gemini and Kimi models — against a single prepaid balance. Routing between tiers is swapping the model ID in the same Responses call:` },
      { type: "code", code: `curl ${OPENAI_BASE}/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-terra",\n    "input": "Review this diff for regressions."\n  }'` },
      { type: "p", text: `Change "gpt-5.6-terra" to "gpt-5.6-sol" or "gpt-5.6-luna" and the same request runs on that tier — same base URL, same Bearer header, same balance. With the official SDK the routing policy is one constructor and a model string:` },
      { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="${OPENAI_BASE}",\n)\n\ndef route(task: str, hard: bool) -> str:\n    model = "gpt-5.6-sol" if hard else "gpt-5.6-terra"\n    return client.responses.create(model=model, input=task).output_text` },
      { type: "p", text: "The dashboard records settled token usage and the exact discounted charge per request, so you can see what your routing policy actually costs instead of guessing. Confirm the enabled model set any time with GET " + OPENAI_BASE + "/models — the unified catalog namespaces IDs by provider (anthropic/*, openai/*, google/*)." },
      { type: "link", text: "Full per-model specs and discounted prices", href: "/models" },
    ] },
    { h2: "Caching and the 272K boundary can outweigh the tier choice", blocks: [
      { type: "p", text: "Two pricing mechanics move the bill as much as picking the right tier. First, cached input: a repeated prompt prefix bills at the cached rate — $0.50 per 1M on Sol versus $5 fresh, with the same 10% ratio on Terra and Luna — and cache writes bill at 125% of normal input. In long agent loops that resend the same system prompt and history, a stable prefix compounds into the largest single saving available." },
      { type: "p", text: "Second, the long-context step: above 272K input tokens the whole request reprices at 2× input and 1.5× output — not just the overflow. A 273K-token request costs more than double a 270K one. Split oversized contexts or trim history before crossing the boundary, whichever tier you are on." },
      { type: "note", text: "Reasoning tokens bill as output tokens. Cranking effort to max on Sol means paying $30 per 1M (official) for deliberate thinking — worth it when the reasoning is the product, waste on mechanical tasks. Match effort to the tier: cheap tiers at high effort are often worse value than a stronger tier at low effort." },
    ] },
    { h2: "A routing policy you can run tomorrow", blocks: [
      { type: "steps", items: [
        "Default every workload to gpt-5.6-terra — interactive coding, CI agents and production traffic alike.",
        "Write down escalation triggers in advance: a failed Terra attempt, a multi-file refactor, or an irreversible design decision goes to gpt-5.6-sol with a tightly scoped prompt.",
        "Move deterministic high-volume steps — classification, extraction, formatting — to gpt-5.6-luna, and track its redo rate so silent failures do not eat the saving.",
        "Keep a stable prompt prefix so cached-input rates apply, and keep requests under the 272K long-context boundary.",
        "Review settled per-request usage in the dashboard weekly and adjust the split by measured cost, not by model-name loyalty.",
      ] },
      { type: "p", text: "The flat 50% B2C discount applies identically to all three tiers, so the relative ranking never shifts — Terra is always the cheaper meter than Sol, Luna always cheaper than Terra. There is no subscription and no seat fee: an idle week costs nothing, and a heavy one costs exactly the tokens it consumed at half the official spend." },
      { type: "link", text: "GPT API pricing: every leg of the bill explained", href: "/docs/learn/gpt-api-pricing" },
      { type: "link", text: "OpenAI-compatible quickstart: from curl to the official SDK", href: "/docs/learn/openai-api-quickstart" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Which GPT-5.6 model is best for coding?", a: "Start with gpt-5.6-terra: it keeps Sol's 400K context and full reasoning controls at 40% of the token price. Escalate to gpt-5.6-sol for the hardest architecture, debugging or agentic work, and use gpt-5.6-luna for cheap deterministic sub-steps." },
    { q: "How much cheaper is Terra than Sol?", a: "Officially $2/$12 per 1M input/output tokens versus Sol's $5/$30 — 40% of the flagship rate. On apiToken.sale the flat 50% discount applies to both: $1/$6 for Terra and $2.50/$15 for Sol." },
    { q: "Do Sol, Terra and Luna use different endpoints or keys?", a: "No. All three run on the same OpenAI-compatible base URL with the same Bearer key and prepaid balance; only the model ID in the request changes." },
    { q: "Does Terra support the max reasoning effort?", a: "Yes. Sol, Terra and Luna expose the same GPT-5.6 reasoning-effort set — none through xhigh plus max. Reasoning tokens bill as output, so max effort on Sol costs the $30-per-1M output rate." },
    { q: "Is gpt-5.6 the same model as gpt-5.6-sol?", a: "gpt-5.6 is an alias that tracks the flagship, so it bills at Sol rates. Pin an explicit tier — gpt-5.6-sol, gpt-5.6-terra or gpt-5.6-luna — in production config to keep the meter predictable." },
    { q: "What happens above 272K input tokens?", a: "OpenAI long-context rates apply to the entire request — 2× input and 1.5× output, before the 50% discount. Split or trim oversized contexts before crossing the boundary on any tier." },
  ],
  related: ["gpt-api-pricing", "openai-api-quickstart", "codex-cli-setup", "gpt-image-2-api-guide"],
  published: "2026-08-09",
  updated: "2026-08-17",
};
