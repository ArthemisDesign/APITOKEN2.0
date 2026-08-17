import type { LearnArticle } from "../learn";
import { cta, BASE, KEY } from "../learn-shared";

export const article: LearnArticle = {
  slug: "claude-api-streaming",
  cluster: "explain",
  title: "Claude API Streaming (SSE): Tokens as They Are Generated",
  h1: "Claude API streaming: SSE responses, token by token",
  description: "How Claude API streaming works on apiToken.sale: stream:true, the Anthropic SSE event sequence, SDK helpers, final token usage, and why billing is identical to non-streaming.",
  keywords: ["claude api streaming", "claude sse", "stream claude responses", "anthropic streaming api", "claude api server-sent events", "claude messages api stream true", "anthropic sdk streaming", "claude api real-time responses", "claude streaming python", "claude api stream example"],
  dek: "Claude API streaming sends each token over server-sent events as it is generated, instead of making you wait for the full message. On apiToken.sale it is the standard Anthropic SSE format at the same endpoint, billed per token exactly like a non-streaming call. This guide walks through the request, the event sequence, and the failure modes that matter in production.",
  sections: [
    { h2: "Turn on streaming with stream:true", blocks: [
      { type: "p", text: `Claude API streaming is one flag, not a new endpoint. POST to ${BASE}/v1/messages with your key in the x-api-key header, the anthropic-version: 2023-06-01 header, and "stream": true in the body — the gateway answers with the standard Anthropic server-sent events stream instead of a single JSON document. The request shape, model IDs and headers are the same ones api.anthropic.com expects, so any client that already speaks the Messages API streams without modification.` },
      { type: "code", code: `curl -N ${BASE}/v1/messages \\\n  -H "x-api-key: ${KEY}" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
      { type: "p", text: "The -N flag disables curl's output buffering, which is the single most common reason a first streaming test looks broken: the response is arriving fine, but curl holds it until the connection closes. With buffering off you watch text_delta events land in real time. The response has Content-Type: text/event-stream and stays open until the model finishes." },
      { type: "steps", items: [
        `Generate a key in the dashboard — it looks like ${KEY} and works on every supported Claude model.`,
        "Run the curl above with -N and watch the events arrive incrementally.",
        "Confirm the stream opens with message_start, carries content_block_delta chunks, and closes with message_delta then message_stop.",
        "Open the dashboard usage view and match the request's input and output tokens against what the stream reported.",
      ] },
    ] },
    { h2: "Read the SSE event sequence, not raw text", blocks: [
      { type: "p", text: "An Anthropic stream is a typed event sequence, and treating it as a raw text feed is where hand-rolled clients go wrong. Each event arrives as an event: name line plus a data: JSON line. A minimal stream for a short answer looks like this:" },
      { type: "code", code: `event: message_start\ndata: {"type":"message_start","message":{"id":"msg_01...","role":"assistant","model":"claude-sonnet-5","usage":{"input_tokens":12,"output_tokens":1}}}\n\nevent: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\nevent: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}\n\nevent: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\nevent: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}\n\nevent: message_stop\ndata: {"type":"message_stop"}` },
      { type: "table", headers: ["Event", "What it carries"], rows: [
        ["message_start", "The message shell: id, role, model and usage.input_tokens for the prompt"],
        ["content_block_start / content_block_stop", "Boundaries of each output block — text or tool_use — at a given index"],
        ["content_block_delta", "Incremental text_delta for text blocks, input_json_delta fragments for tool calls"],
        ["ping", "Keepalive between blocks; safe to ignore but do not treat it as an error"],
        ["message_delta", "stop_reason and cumulative usage.output_tokens — the authoritative output count"],
        ["message_stop", "End of the stream; the connection closes after this"],
      ] },
      { type: "p", text: "Two accounting rules follow from the sequence. First, input_tokens is fixed at message_start, while output_tokens accumulates and is only final in the message_delta that carries stop_reason — so log usage from the terminal events, never by counting deltas yourself. Second, a generation can contain several content blocks (text interleaved with tool_use), each with its own index, so accumulate deltas per index rather than appending everything into one string. Tool arguments arrive as partial JSON fragments in input_json_delta and must be concatenated, then parsed once at content_block_stop." },
      { type: "p", text: "Consuming a stream without an SDK takes one discipline: split on protocol boundaries, not on chunks. Read the body as a stream (in the browser and Node 18+, res.body is a ReadableStream), buffer bytes until you see a blank line, and treat everything between blank lines as one event. Network chunks do not align with events — a data: line can arrive split across two reads, and several events can share one read. Parse JSON only from the data: payload, and only for event types you handle. EventSource is not an option here: it only speaks GET, and the Messages API requires POST." },
      { type: "note", text: "Long streams can include ping events or go quiet while the model thinks. Set your read timeout against silence between events, not against total stream duration — a hard 30-second total timeout will kill legitimate long generations." },
    ] },
    { h2: "What streaming changes — and what it does not", blocks: [
      { type: "p", text: "The official SDKs hide the event plumbing. Point the client at the gateway and use its streaming helper; the events above are surfaced as an iterator, and the final message with authoritative usage is one call away:" },
      { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="${BASE}",\n    api_key="${KEY}",\n)\n\nwith client.messages.stream(\n    model="claude-sonnet-5",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Explain SSE in one paragraph"}],\n) as stream:\n    for text in stream.text_stream:\n        print(text, end="", flush=True)\n    final = stream.get_final_message()\n    print(final.usage)  # input_tokens + final output_tokens` },
      { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "${BASE}",\n  apiKey: "${KEY}",\n});\n\nconst stream = client.messages.stream({\n  model: "claude-sonnet-5",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Explain SSE in one paragraph" }],\n});\nstream.on("text", (text) => process.stdout.write(text));\nconst final = await stream.finalMessage();\nconsole.log(final.usage);` },
      { type: "p", text: "What does not change is money: streaming and non-streaming requests are billed the same way — by input and output tokens — so you lose nothing by streaming. A streamed answer of 500 output tokens costs exactly what the same 500 tokens cost buffered, and the request shows up in the dashboard usage breakdown with the same token lines either way. What changes is perceived latency (the first token arrives in a fraction of the total time), resilience for long generations (an idle, silent non-streaming connection is exactly what proxies and load balancers like to time out), and how early your code can react — an agent can dispatch a tool call the moment its closing brace lands instead of after the whole reply." },
      { type: "list", items: [
        "Chat and coding UIs where users watch the answer appear — the difference between an app that feels instant and one that feels stalled.",
        "Long generations, so you can render or act on partial output early and keep every hop on the path busy.",
        "Agents that stop or branch as soon as a complete tool call is emitted.",
      ] },
      { type: "p", text: "For short batch jobs — classification, extraction, anything under a few hundred tokens that nobody watches — non-streaming is simpler to retry and log, and the cost is identical either way. Whichever mode you choose, remember that a stream can fail after the 200 OK: an event: error or a dropped connection before message_stop means the generation did not finish. Treat accumulated partial output as untrusted — never persist it or feed it into the next step of an agent loop — and re-issue the request." },
      { type: "link", text: "Estimate a streamed workload's cost with the Claude API cost calculator", href: "/tools/claude-api-cost-calculator" },
      { type: "link", text: "If streams fail with 429 under load, see the rate-limit guide", href: "/docs/learn/claude-api-rate-limits" },
      cta(),
    ] },
  ],
  faq: [
    { q: "Does apiToken.sale support Claude API streaming?", a: `Yes. Set "stream": true on POST ${BASE}/v1/messages with your x-api-key and anthropic-version headers and you get the standard Anthropic SSE event stream — message_start, content_block_delta chunks, message_delta with final usage, message_stop. It works for coding agents, IDEs, the official Anthropic Python and TypeScript SDK streaming helpers, and production calls.` },
    { q: "Does streaming Claude responses cost more than non-streaming?", a: "No. Streaming and non-streaming requests are billed identically by input and output tokens, and the final usage totals match a buffered response — read them from the terminal message_delta event or from the dashboard usage view. Streaming only changes when the tokens reach you, not what they cost." },
  ],
  related: ["claude-api-quick-setup", "claude-api-rate-limits", "anthropic-sdk-base-url", "claude-api-for-ai-agents"],
  updated: "2026-08-17",
};
