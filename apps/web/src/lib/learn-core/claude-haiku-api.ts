import type { LearnArticle } from "../learn";
import { BASE, KEY, cta } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-haiku-api",
  cluster: "free",
  title: "Claude Haiku API Access",
  h1: "Claude Haiku 4.5 through the API",
  description: "Claude Haiku API access via apiToken.sale: the claude-haiku-4-5 model ID, a working Messages API call, streaming, prompt caching and pricing at a flat 50% off official rates.",
  keywords: ["claude haiku api", "claude haiku 4.5 api", "claude-haiku-4-5", "haiku api key", "claude haiku pricing", "cheapest claude model", "fastest claude model", "claude haiku prompt caching", "claude api free credits", "try claude api free", "claude api free tier"],
  dek: "The Claude Haiku API is where high-volume work belongs: classification, extraction, routing and any request where latency and unit cost matter more than deep reasoning. Haiku 4.5 is metered at $1/$5 per million tokens officially — $0.50/$2.50 here with the flat 50% discount — and it shares one key and one prepaid balance with Sonnet, Opus, GPT, Gemini and Kimi. This guide covers the workloads it fits, a working request, and how to escalate the hard fraction upward.",
  sections: [
    { h2: "The work Haiku 4.5 is built to absorb", blocks: [
      { type: "p", text: "Claude Haiku 4.5 is the fastest and lowest-cost model in the Claude family, and you reach it through the standard Anthropic Messages API with the model ID claude-haiku-4-5 — same request shape, same headers, same streaming as Sonnet or Opus. It is the right default for any workload where latency and per-token price matter more than deep reasoning. Through apiToken.sale it runs on prepaid balance at a flat 50% below official rates, with no subscription and no waitlist." },
      { type: "p", text: "Haiku earns its keep at the edges of a pipeline, where requests are short, frequent and interchangeable:" },
      { type: "list", items: [
        "Classification and tagging: support tickets, content moderation, intent detection — short inputs, short outputs, thousands of calls a day.",
        "Extraction and parsing: pull structured fields from invoices, emails, logs or HTML before a larger model ever sees the data.",
        "Routing and triage: decide which model or tool should handle a request, then escalate only the hard ones.",
        "Latency-sensitive chat: agent inner loops, tool-call glue and autocomplete-style UX where the user is staring at a spinner.",
        "Cheap pre-processing: summarizing, cleaning and chunking long context ahead of an Opus or Sonnet call.",
      ] },
      { type: "note", text: "Haiku is the wrong tool for deep multi-step reasoning, high-stakes analysis and very long generation. If a task keeps failing your quality bar, that is a routing decision, not a prompting problem — send it to Sonnet or Opus instead." },
    ] },
    { h2: "Model ID, context window and output ceiling", blocks: [
      { type: "p", text: "There is one current Haiku ID to memorize: claude-haiku-4-5. It accepts the full Messages API feature set — system prompts, multi-turn messages, tool use, streaming and prompt caching — inside a 200K-token context window with a 64K-token maximum output. Both ceilings are smaller than the Opus and Sonnet line, which matters if you batch large documents into single calls." },
      { type: "table", headers: ["Spec", "Value"], rows: [
        ["Model ID", "claude-haiku-4-5"],
        ["Context window", "200K tokens"],
        ["Max output", "64K tokens"],
        ["Endpoint", "POST /v1/messages (Anthropic Messages shape)"],
        ["Auth header", "x-api-key"],
      ] },
      { type: "p", text: "The Messages API requires max_tokens on every request, streaming or not. Set it to the largest response you actually expect rather than the model ceiling — an unconstrained cap plus a verbose habit is how a cheap workload quietly becomes an expensive one." },
    ] },
    { h2: "Your first Haiku request", blocks: [
      { type: "steps", items: [
        "Create an account and generate a key from the dashboard — it looks like sk-pool-… and works across every supported Claude, GPT, Gemini and Kimi model.",
        `Point any Anthropic-compatible client at the router: set ANTHROPIC_BASE_URL to ${BASE} and ANTHROPIC_API_KEY to your key. The official SDKs need no other change.`,
        "Send POST /v1/messages with the x-api-key and anthropic-version headers, model set to claude-haiku-4-5, and an explicit max_tokens.",
      ] },
      { type: "code", code:
`curl ${BASE}/v1/messages \\
  -H "x-api-key: ${KEY}" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-haiku-4-5",
    "max_tokens": 256,
    "messages": [
      {"role": "user", "content": "Classify this ticket as billing, bug or feature: \\"My invoice shows the wrong total.\\""}
    ]
  }'` },
      { type: "p", text: "The response is the standard Messages shape: an array of content blocks plus a usage object with input and output token counts. That usage object is the record your balance is billed from — metered at official Anthropic rates, with the flat 50% discount subtracted before the draw." },
    ] },
    { h2: "What Haiku costs per call, per million, per month", blocks: [
      { type: "p", text: "Official pricing is $1 per million input tokens and $5 per million output tokens; here that is $0.50/$2.50 after the discount. Output tokens cost five times input tokens, so response-length discipline saves more than prompt trimming on chatty workloads." },
      { type: "table", headers: ["Metered usage", "Official ($ per 1M tokens)", "Here (−50%)"], rows: [
        ["Input", "$1.00", "$0.50"],
        ["Output", "$5.00", "$2.50"],
        ["Cache write (5-minute)", "$1.25", "$0.625"],
        ["Cache read", "$0.10", "$0.05"],
      ] },
      { type: "p", text: "Make it concrete: a classification call with a 600-token prompt and an 80-token answer meters 680 tokens and costs about $0.0005 at the discounted rates. A hundred thousand such calls a month lands near $50 — the kind of volume where per-token pricing stops being an abstraction and starts being a line item." },
      { type: "link", text: "Estimate your own volume in the free cost calculator", href: "/tools/claude-api-cost-calculator" },
    ] },
    { h2: "Keep first-token latency low with streaming", blocks: [
      { type: "p", text: "Set \"stream\": true and the endpoint returns server-sent events instead of one blocking response: message_start, a sequence of content_block_delta events carrying the text as it is generated, then a terminal message_delta with the final usage and message_stop. Render the deltas as they arrive and the perceived latency of a chat UI drops to the first token, which is where Haiku's speed shows. Streaming changes latency perception, not price — the same tokens are metered either way." },
      { type: "note", text: "Take the authoritative token counts from the terminal message_delta event, not from counting deltas yourself. If a stream drops mid-generation, do not retry in a tight loop: issue one fresh request and reconcile spend from the usage you did receive." },
    ] },
    { h2: "Cache the prefix you resend on every call", blocks: [
      { type: "p", text: "High-volume loops re-send the same prefix every time: system prompt, label definitions, few-shot examples. Mark the end of that stable prefix with a cache_control breakpoint and Anthropic holds it in a short-lived cache — a five-minute TTL, refreshed on each hit. The write costs 1.25× the input rate; every subsequent read bills at 0.1×, and the 50% discount stacks on top." },
      { type: "p", text: "Put the breakpoint after the last block that never changes and keep per-request content after it. A breakpoint on text that varies every call never hits and only costs you the write premium — on Haiku rates the premium is small, but at volume it is still wasted spend." },
      { type: "link", text: "Claude Haiku 4.5 pricing in detail (cache rates, context, FAQ)", href: "/models/claude-haiku-4-5" },
    ] },
    { h2: "Escalate the hard fraction to Sonnet or Opus on the same key", blocks: [
      { type: "p", text: "One key and one balance cover every supported model, so routing is a client-side if statement, not an infrastructure project. Send the bulk of traffic to Haiku; when the input is long, the confidence is low or the task genuinely needs multi-step reasoning, resend the same messages array with a different model field — claude-sonnet-5 or an Opus ID. Most production traffic is easy traffic, so most of your spend stays at Haiku rates while the hard requests still get a stronger model." },
      { type: "code", code:
`import anthropic

client = anthropic.Anthropic(
    base_url="${BASE}",
    api_key="${KEY}",
)

def answer(question: str) -> str:
    triage = client.messages.create(
        model="claude-haiku-4-5",
        max_tokens=8,
        system="Reply with one word: EASY for a simple lookup or short task, HARD if it needs multi-step reasoning.",
        messages=[{"role": "user", "content": question}],
    )
    verdict = triage.content[0].text.strip().upper()
    model = "claude-sonnet-5" if verdict.startswith("HARD") else "claude-haiku-4-5"
    reply = client.messages.create(
        model=model,
        max_tokens=1024,
        messages=[{"role": "user", "content": question}],
    )
    return reply.content[0].text` },
      { type: "note", text: "Triage itself costs a Haiku call, so only route when the mix is genuinely uneven. If 95% of requests are trivial, calling Haiku directly with no triage at all is cheaper than paying an extra round trip per request." },
      cta(),
    ] },
  ],
  faq: [
    { q: "What is the model ID for Claude Haiku 4.5 in the API?", a: "claude-haiku-4-5. Pass it as the model field of a standard Messages API request with x-api-key and anthropic-version headers — the same request shape as Sonnet or Opus." },
    { q: "How much does the Claude Haiku API cost per million tokens?", a: "Officially $1 per 1M input and $5 per 1M output tokens. On apiToken.sale every request is metered at official rates minus a flat 50%, so you pay $0.50/$2.50, and cache reads meter at a tenth of the input rate." },
    { q: "Is Haiku 4.5 good enough for coding, or do I need Sonnet?", a: "Haiku fits high-volume, low-complexity work — classification, extraction, routing, agent glue. For daily coding and agent workflows Sonnet is the recommended default; on one shared key you can route each request to the cheapest tier that handles it." },
    { q: "What are the context and output limits of Haiku 4.5?", a: "A 200K-token context window and a 64K-token maximum output — smaller ceilings than the Opus and Sonnet line. Every request must also set an explicit max_tokens value." },
    { q: "Can I call the Haiku API from Cursor, Claude Code or the Anthropic SDK?", a: "Yes. Any Anthropic-compatible client works: set the base URL to the apiToken.sale router, authenticate with x-api-key, and keep the rest of your configuration unchanged." },
    { q: "How can I try the Claude Haiku API for free?", a: "Sign up with Google or GitHub and the account starts with $5 of platform bonus credit, usable on Haiku and every other supported Claude, GPT, Gemini and Kimi model. Email/password accounts do not receive the bonus." },
  ],
  related: ["claude-sonnet-api", "claude-opus-api", "save-tokens-on-claude-api", "cheapest-claude-api"],
  updated: "2026-08-17",
};
