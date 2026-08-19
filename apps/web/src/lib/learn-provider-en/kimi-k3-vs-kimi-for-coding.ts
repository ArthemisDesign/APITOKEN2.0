import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "kimi-k3-vs-kimi-for-coding",
  cluster: "compare",
  title: "Kimi K3 vs Kimi for Coding: Context and Price",
  h1: "Kimi K3 vs Kimi for Coding: which model for which workload",
  description: "Kimi K3 vs Kimi for Coding compared: 256K vs 1M context, tunable reasoning vs always-on thinking, High Speed's double rate and a two-tier routing policy.",
  keywords: ["kimi k3 vs kimi for coding", "kimi k3 api", "kimi for coding price", "best kimi model for coding", "kimi k3 256k vs 1m", "kimi highspeed worth it", "kimi models comparison", "kimi k3 reasoning effort", "kimi k2.7 code", "which kimi model for coding agents"],
  dek: "Kimi K3 is the reasoning and long-context family; Kimi for Coding is the low-cost coding family with thinking always on. This Kimi K3 vs Kimi for Coding comparison maps every public alias — context window, reasoning controls and per-token rates — and ends with a routing policy that sends everyday edits to the cheap model and escalates hard or oversized tasks to K3.",
  sections: [
    { h2: "The short answer: cost per edit vs reasoning headroom", blocks: [
      { type: "p", text: "Use Kimi for Coding as your default coding model and escalate to K3 when the task outgrows it. Kimi for Coding bills at $0.19 / $0.95 / $4 per million cache-hit, cache-miss and output tokens — the lowest general coding rate in the published Kimi set — while K3 charges $0.30 / $3 / $15 and buys you a 1M context mode plus explicit low, high and max reasoning-effort control. Both families are reachable through the same apiToken.sale key, so the choice is a per-request routing decision, not an account decision." },
      { type: "p", text: "The practical split most teams land on: autocomplete-adjacent edits, test generation, small refactors and high-volume agent loops go to Kimi for Coding; whole-repository analysis, long-document work and problems that need visible deliberation go to K3. High Speed is a latency purchase, not a capability upgrade — it serves the same coding model at exactly double the token rates." },
    ] },
    { h2: "Alias map: context windows and thinking modes", blocks: [
      { type: "table", headers: ["Public alias", "Context", "Reasoning control", "Best fit"], rows: [
        ["kimi/kimi-for-coding", "256K", "Thinking enabled", "Everyday coding and economical agent loops"],
        ["kimi/kimi-for-coding-highspeed", "256K", "Thinking enabled", "Latency-sensitive coding where speed pays for itself"],
        ["kimi/k3-256k", "256K", "low / high / max effort, high default", "K3 reasoning without the full-context mode"],
        ["kimi/k3 · kimi/k3[1m]", "1M", "low / high / max effort, high default", "Long codebases, documents and hard reasoning"],
      ] },
      { type: "p", text: "k3[1m] is a compatibility spelling of K3's 1M mode, not a separately priced model. The router normalizes it to the provider's real k3 wire model, so kimi/k3 and kimi/k3[1m] produce the same traffic and the same bill." },
      { type: "p", text: "The 256K forms matter more than they look. If a task fits in 256K tokens, k3-256k gives you the K3 reasoning controls without committing the request to the 1M context mode — the right default for hard-but-small problems like a single gnarly algorithm or a tricky concurrency bug." },
    ] },
    { h2: "What each request actually costs", blocks: [
      { type: "p", text: "Kimi publishes three legs — cache hit, cache miss and output — instead of one input price, and caching is automatic. A repeated prefix bills at the hit rate; a newly cached token bills as a miss, not as a free or hidden fourth leg. apiToken.sale prices the model actually served and applies a flat 50% discount to every leg:" },
      { type: "table", headers: ["Alias", "Official hit / miss / output per 1M", "After the flat 50% discount"], rows: [
        ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
        ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
        ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
      ] },
      { type: "note", text: "Reasoning tokens are a subset of output and bill at the output rate; they are not added again as a separate token class. A K3 request at max effort can therefore cost noticeably more than the same prompt at low effort — the difference shows up in output volume, not in a surcharge." },
      { type: "p", text: "One useful way to read the table: after the discount, High Speed costs exactly what base Kimi for Coding costs officially. If you were going to pay Moonshot's sticker price for the base model anyway, the low-latency variant here is the same money." },
    ] },
    { h2: "Reasoning controls: an effort knob vs always-on thinking", blocks: [
      { type: "list", items: [
        "K3 exposes low, high and max reasoning effort; high is the default. Lower the effort on cheap exploratory turns and raise it only for the steps that actually need deliberation.",
        "Kimi for Coding and High Speed run with thinking enabled and expose no effort selector — you get the family's fixed thinking behavior on every call.",
        "On the Anthropic lane, treat a none/off thinking setting as disabling K3 reasoning, not as a model selector: live coverage kept those turns on the K3 tariff.",
        "kimi-k2.6 is not an addressable public model. Do not try to reach an older generation by tweaking reasoning parameters.",
      ] },
      { type: "p", text: "This asymmetry drives the cost math. Kimi for Coding's always-on thinking is priced into a $4 output rate; K3's controllable effort is priced into a $15 one. Paying K3 output prices for a task that never needed max effort is the most common way teams overspend on this pair." },
    ] },
    { h2: "When High Speed earns its double rate", blocks: [
      { type: "p", text: "High Speed's cache-hit, cache-miss and output rates are exactly double the base Kimi for Coding rates, and the model underneath is the same. You are buying latency, full stop. That trade is rational in exactly one situation: a human is waiting on the response and their time costs more than the tokens." },
      { type: "list", items: [
        "Worth it: interactive pair-programming sessions, editor-integrated completion loops, live demos.",
        "Not worth it: CI test generation, overnight refactor batches, evaluation sweeps, any queued or retried workload.",
        "Never worth it: tasks you intended for K3 anyway — High Speed is the coding family, not a faster K3.",
      ] },
    ] },
    { h2: "A two-tier routing policy for real agent loops", blocks: [
      { type: "p", text: "Because both families share one key and one balance, a router can split work per call. The policy that holds up in practice: estimate the request's context size and difficulty, send cheap-and-small to Kimi for Coding, and escalate anything large or hard to K3 with an explicit effort level:" },
      { type: "code", code: [
        "from anthropic import Anthropic",
        "",
        "client = Anthropic(base_url=\"" + BASE + "\", api_key=\"" + KEY + "\")",
        "",
        "def pick_model(approx_input_tokens: int, hard: bool) -> tuple[str, dict]:",
        "    if approx_input_tokens > 200_000:",
        "        return \"kimi/k3[1m]\", {\"effort\": \"high\"}   # needs the 1M window",
        "    if hard:",
        "        return \"kimi/k3-256k\", {\"effort\": \"max\"}    # small but difficult",
        "    return \"kimi/kimi-for-coding\", {}                   # everyday default",
        "",
        "model, extra = pick_model(approx_input_tokens=12_000, hard=False)",
        "msg = client.messages.create(",
        "    model=model, max_tokens=1024,",
        "    messages=[{\"role\": \"user\", \"content\": \"Reply with exactly: connected\"}],",
        ")",
        "print(msg.usage)  # terminal usage: check which cache leg you actually hit",
      ].join("\n") },
      { type: "p", text: "The terminal usage object is your feedback loop. If a workload you routed to K3 keeps coming back with small output and cache-hit-heavy input, it belongs on the cheaper alias; if Kimi for Coding keeps failing a class of task, that class is your escalation rule made concrete." },
    ] },
    { h2: "Pin aliases from the live catalog, not from memory", blocks: [
      { type: "steps", items: [
        "Fetch the scoped catalog with your key: curl " + BASE + "/v1/models -H \"Authorization: Bearer " + KEY + "\". Model access is catalog-driven, so this response — not a blog post, including this one — is the source of truth for what your key can call.",
        "Pin the exact alias string (kimi/k3-256k, kimi/kimi-for-coding, …) in your client configuration. Bare spellings without the kimi/ namespace belong to the Anthropic lane; check the catalog before hardcoding either form.",
        "Send one tiny probe request per alias you pinned and inspect the terminal usage. Confirm the billed model and the cache legs match what you intended before letting an agent loop run unattended.",
        "Re-check /v1/models before pinning an alias into a long-lived environment or CI variable; the catalog, not the alias string, defines availability.",
      ] },
      { type: "note", text: "Do not request Kimi's internal official model IDs. Public router traffic uses the subscription aliases from the catalog; internal tariff IDs such as kimi-k2.7-code are not accepted spellings." },
    ] },
    { h2: "One prepaid balance behind both families", blocks: [
      { type: "p", text: "There is no per-model plan to pick. One apiToken.sale key covers the supported Claude, GPT, Gemini and Kimi catalogs, every request is metered at official rates minus the flat 50% discount, and the draw comes out of a prepaid balance that never expires. A team running Kimi for Coding for volume and K3 for hard cases sees one balance, one invoice trail and per-request usage in the dashboard." },
      { type: "p", text: "Because the balance never expires, splitting work across the two families carries no commitment risk: a top-up made during a K3-heavy week still pays for next month's Kimi for Coding traffic at the same discounted rates." },
      cta(),
      { type: "link", text: "Full per-model Kimi rates, including cache legs", href: "/models" },
    ] },
  ],
  faq: [
    { q: "Which Kimi model is best for coding?", a: "Kimi for Coding is the economical default at $0.19 / $0.95 / $4 per million cache-hit, cache-miss and output tokens officially — half that after the flat 50% discount. Escalate to K3 for harder reasoning or long-context codebase work, and use High Speed only when lower latency is worth exactly double the base rates." },
    { q: "Are k3 and k3[1m] different models?", a: "No. Both select the same K3 1M mode; the bracket form is a compatibility alias that the router normalizes to the provider's real k3 wire model, and there is no separate price for it." },
    { q: "What is the difference between k3-256k and k3?", a: "Context mode. k3-256k runs K3 with its reasoning-effort controls (low, high and max, defaulting to high) inside a 256K window, while k3 / k3[1m] enable the 1M context mode for long codebases and documents." },
    { q: "Is Kimi for Coding High Speed a smarter model?", a: "No. It is the same coding model served with lower latency at exactly double the cache-hit, cache-miss and output rates. Buy it when a human is waiting on the response; skip it for batch and CI work." },
    { q: "Can I request Kimi's internal official model IDs through the router?", a: "No. Use the public subscription aliases returned by the scoped GET /v1/models catalog. Internal tariff IDs such as kimi-k2.7-code are not accepted, and kimi-k2.6 is not an addressable public model." },
    { q: "Do Kimi reasoning tokens cost extra?", a: "They bill at the output rate as a subset of output tokens — $4 per million officially on Kimi for Coding, $15 on K3, halved by the discount — and are never charged as a separate token class on top." },
  ],
  related: ["kimi-api-pricing", "kimi-api-quickstart", "kimi-api-for-claude-code", "how-to-buy-kimi-api-key"],
  published: "2026-08-09",
  updated: "2026-08-17",
};
