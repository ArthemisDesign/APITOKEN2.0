// Tool-scoped error pages: /errors, /errors/<tool>, /errors/<tool>/<slug>, mirrored
// in every locale the learn cluster ships (en/ru/zh/ko).
//
// The API-level reference at /docs/errors answers "what does this response mean".
// These pages answer the query people actually type, which names the TOOL first:
// "cursor unable to reach model provider", "claude code api error 429", "codex
// stream error unexpected status 401". One URL per tool×error, so the <title> can
// match the query verbatim — a single reference page can only win one such query.
//
// Same contract as lib/api-errors.ts: strings in `searchStrings` and `symptom` are
// what the tool actually prints, reproduced verbatim, never paraphrased, and never
// translated — people paste what their terminal shows. Only the prose around them
// is localized. Config snippets come from the shipped docs (docs-portal, learn,
// CODEX_APP_SERVER.md) — never invented.

export type ToolErrorLocale = "en" | "ru" | "zh" | "ko";

export const TOOL_ERROR_LOCALES: ToolErrorLocale[] = ["en", "ru", "zh", "ko"];

export type ToolSlug = "claude-code" | "cursor" | "codex" | "opencode" | "cline" | "zed";

export type ToolInfo = {
  slug: ToolSlug;
  /** Display name, identical in every locale. */
  name: string;
  /** English hub copy. Localized hub copy lives in the per-locale translation files. */
  title: string;
  description: string;
  intro: string;
  /** Integration guide on this site, if one exists. */
  guidePath?: string;
};

export type ToolFaq = { q: string; a: string };

export type ToolErrorEntry = {
  tool: ToolSlug;
  /** URL slug under /errors/<tool>/ — shaped after the query, never renamed. */
  slug: string;
  /** HTTP status where one exists; 0 for tool-local failures with no HTTP status. */
  status: number;
  /** English <title> — matches the query phrasing. */
  title: string;
  description: string;
  /** Verbatim output the user sees. First entry renders in the hero code block. */
  searchStrings: string[];
  causes: string[];
  fixes: string[];
  snippet?: { label: string; code: string };
  faq: ToolFaq[];
  /** Anchor code in the /docs/errors catalog covering the underlying API error. */
  refCode?: string;
  /** Whether retrying the identical request can succeed. */
  retryable: boolean;
};

// --- Tools ------------------------------------------------------------------

export const TOOL_ERROR_TOOLS: ToolInfo[] = [
  {
    slug: "claude-code",
    name: "Claude Code",
    title: "Claude Code API Errors — 429, 401, 529, Usage Limit",
    description:
      "Fixes for every error Claude Code prints: API Error 429 rate_limit_error, 401 invalid x-api-key, 529 Overloaded and the usage-limit message. Exact output, cause and fix.",
    intro:
      "Claude Code surfaces API failures as a single line starting with \"API Error:\" followed by the raw response body, and subscription caps as a plain sentence. The pages below cover each form with the exact text, why it happens, and what fixes it.",
    guidePath: "/int-claude-code",
  },
  {
    slug: "cursor",
    name: "Cursor",
    title: "Cursor Model Provider Errors — 401, 429, Unable to Reach",
    description:
      "Fixes for Cursor errors with a custom Anthropic API key: unable to reach model provider, invalid API key on verify, 401 from the provider and rate limits. Cause and fix for each.",
    intro:
      "With a custom Anthropic key, Cursor talks to the provider directly, so most failures are either the key/base-URL configuration or the provider's own response passed through. The pages below separate the two.",
    guidePath: "/int-cursor",
  },
  {
    slug: "codex",
    name: "Codex CLI",
    title: "Codex CLI Errors — Missing OPENAI_API_KEY, config.toml, stream error",
    description:
      "Fixes for Codex CLI failures with a custom model provider: Missing OPENAI_API_KEY, config.toml profile mistakes, auth.json login state and stream error: unexpected status 401.",
    intro:
      "Codex CLI reads ~/.codex/config.toml, resolves a model provider, then streams over the Responses API. Each stage fails differently: a missing env var, a malformed profile, a stale login, or an HTTP error mid-stream. The pages below take them in that order.",
  },
  {
    slug: "opencode",
    name: "opencode",
    title: "opencode Errors — AI_APICallError, Model Not Found, Auth",
    description:
      "Fixes for opencode failures on a custom provider: AI_APICallError, models missing from the provider list, API keys that are not picked up, and silently dropped image attachments.",
    intro:
      "opencode builds on the Vercel AI SDK, so provider failures surface as AI_* error classes, and models outside the models.dev catalog need their capabilities declared by hand in opencode.json. The pages below cover the failures that actually reach users.",
  },
  {
    slug: "cline",
    name: "Cline",
    title: "Cline API Errors — API Request Failed, 401, 429, Context Limit",
    description:
      "Fixes for Cline errors with an Anthropic key: the API Request Failed banner, 401 invalid x-api-key, 429 rate_limit_error retry loops and the context-limit 400. Cause and fix for each.",
    intro:
      "Cline shows most failures as an \"API Request Failed\" banner with the provider's response underneath. The banner itself is not the diagnosis — the status and error.type in the body are. The pages below map each body to its fix.",
    guidePath: "/int-cline",
  },
  {
    slug: "zed",
    name: "Zed",
    title: "Zed Claude Errors — 401, 429, 529, Custom api_url",
    description:
      "Fixes for Zed assistant errors on Anthropic models: 401 invalid x-api-key, 429 rate limits, 529 Overloaded and custom api_url configuration, including the /v1/v1 404 trap.",
    intro:
      "Zed's assistant passes Anthropic responses through nearly verbatim, so the error text you see is the API's own. What is specific to Zed is where the key and the custom api_url live in settings.json — most persistent failures trace back to those two fields.",
    guidePath: "/int-zed",
  },
];

export const TOOL_SLUGS: ToolSlug[] = TOOL_ERROR_TOOLS.map((tool) => tool.slug);

export function findTool(slug: string): ToolInfo | undefined {
  return TOOL_ERROR_TOOLS.find((tool) => tool.slug === slug);
}

// --- Index (hub of hubs) copy ----------------------------------------------

export const TOOL_ERRORS_INDEX = {
  title: "AI Coding Tool Errors — Claude Code, Cursor, Codex, opencode, Cline, Zed",
  description:
    "Error pages for the tools developers actually run against the Claude API: Claude Code, Cursor, Codex CLI, opencode, Cline and Zed. Exact message, cause and fix for each failure.",
  intro:
    "Pick the tool that is failing. Every page quotes the exact text the tool prints, explains what produced it, and gives the shortest path to a working setup. For the API-level reference organized by status code, see the Claude API error codes page.",
};

// --- Entries ----------------------------------------------------------------

const BASE = "https://router.apitoken.sale";
const OPENAI_BASE = "https://router.apitoken.sale/v1";

export const TOOL_ERRORS: ToolErrorEntry[] = [
  // ——— Claude Code ———
  {
    tool: "claude-code",
    slug: "api-error-429",
    status: 429,
    title: "Claude Code API Error 429 (rate_limit_error) — Cause and Fix",
    description:
      "Claude Code prints API Error: 429 rate_limit_error when per-minute throughput is exhausted. What the limit actually is, why parallel agents trigger it, and how to fix it.",
    searchStrings: [
      `API Error: 429 {"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your organization's rate limit of 80,000 input tokens per minute. Please reduce the prompt length or the maximum tokens requested, or try again later."}}`,
      "claude code api error 429",
      "claude code rate limit error",
    ],
    causes: [
      "The per-minute token or request ceiling on the API organization behind your key was exceeded. The number in the message is that organization's own limit, so it differs between accounts.",
      "Several Claude Code sessions or subagents running in parallel against the same key — each one resends its whole context every turn, so bursts add up faster than they look.",
      "A single very large context can exceed a per-minute token budget on its own, which is why the message suggests shortening the prompt as well as waiting.",
      "Retries piling on top of the burst that caused the first 429, enlarging it instead of draining it.",
    ],
    fixes: [
      "Wait out the minute window — Claude Code retries automatically and honours Retry-After. If it keeps recurring, reduce how many sessions share one key.",
      "Shrink what each turn carries: /compact an oversized conversation, or start a fresh session instead of dragging a long history along.",
      "Do not confuse this with \"Claude usage limit reached\" — that is a subscription cap with a reset time, not per-minute throughput. The fixes are different.",
      "If you consistently need more throughput than the key's organization allows, that is a capacity conversation, not a retry problem — raise it with your provider.",
    ],
    faq: [
      {
        q: "Does API Error 429 in Claude Code mean my subscription ran out?",
        a: "No. A 429 is per-minute throughput on an API key. A subscription running out shows as \"Claude usage limit reached\" with a reset time instead.",
      },
      {
        q: "Does Claude Code retry a 429 by itself?",
        a: "Yes — it backs off and retries automatically. If the error persists, the burst is being refilled faster than the window drains, usually by parallel sessions on the same key.",
      },
      {
        q: "Why does one huge prompt cause a 429 on its own?",
        a: "Rate limits are counted in tokens per minute, and a single oversized context can spend the whole minute's budget in one request.",
      },
    ],
    refCode: "rate-limit",
    retryable: true,
  },
  {
    tool: "claude-code",
    slug: "api-error-401",
    status: 401,
    title: "Claude Code API Error 401 invalid x-api-key — Fix",
    description:
      "Claude Code prints API Error: 401 invalid x-api-key when the key does not reach the endpoint it is talking to. The environment-variable rules that cause it and the fix.",
    searchStrings: [
      `API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`,
      "claude code 401 custom ANTHROPIC_BASE_URL",
      "claude code invalid api key",
    ],
    causes: [
      "ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN are both set — both headers go out and the request is rejected. An empty string still counts as set. This is the most common cause when a custom base URL is involved.",
      "The variable is set in a different shell than the one that launched claude — an export in one terminal does not exist in another, and GUI launchers do not read your shell profile.",
      "ANTHROPIC_BASE_URL points at one provider while the key was issued by another. A valid key sent to the wrong endpoint is still a 401.",
      "The key was revoked, or expired if it was issued with an expiry date.",
    ],
    fixes: [
      "Pick one variable and unset the other: for this gateway, use ANTHROPIC_AUTH_TOKEN with ANTHROPIC_BASE_URL and make sure ANTHROPIC_API_KEY is not also exported.",
      "Verify inside the same shell that runs claude: print the first characters of the variable right before launching.",
      "Confirm the key is active in your dashboard and that the base URL matches the key's issuer.",
    ],
    snippet: {
      label: "Working environment for this gateway",
      code: `export ANTHROPIC_BASE_URL="${BASE}"
export ANTHROPIC_AUTH_TOKEN="sk-pool-•••"
unset ANTHROPIC_API_KEY   # must not be set at the same time
claude`,
    },
    faq: [
      {
        q: "Why does Claude Code return 401 right after I set a custom ANTHROPIC_BASE_URL?",
        a: "Usually both ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN are set at once, or the key belongs to a different endpoint than the base URL points at. Unset one variable and match key to endpoint.",
      },
      {
        q: "Is ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN the right variable?",
        a: "ANTHROPIC_API_KEY becomes the x-api-key header; ANTHROPIC_AUTH_TOKEN becomes Authorization: Bearer. Use exactly one. With this gateway, ANTHROPIC_AUTH_TOKEN is the documented choice.",
      },
      {
        q: "The same key works in curl but not in Claude Code — why?",
        a: "The shell that runs claude has different environment state: a competing variable, a stale value, or no value at all. Check the environment in that exact shell.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  {
    tool: "claude-code",
    slug: "api-error-529",
    status: 529,
    title: "Claude Code API Error 529 Overloaded — What It Means",
    description:
      "Claude Code prints API Error: 529 Overloaded when upstream capacity is saturated. Why it is not your request's fault, why it clusters, and what actually helps.",
    searchStrings: [
      `API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`,
      "claude code overloaded error",
      "claude code 529 keeps happening",
    ],
    causes: [
      "Upstream capacity is temporarily saturated. A 529 describes the service at that moment, not your request — nothing in your payload caused it.",
      "It clusters during incidents and peak hours: the same request typically succeeds minutes later with no change on your side.",
    ],
    fixes: [
      "Let Claude Code retry — it backs off automatically. If a run keeps dying, wait a few minutes rather than hammering the same minute.",
      "For long unattended runs, prefer smaller models for mechanical steps: they are generally less contended, so the run survives capacity dips.",
      "If 529s persist for a long stretch, check the provider's status page before changing anything in your setup — your configuration is almost never the cause.",
    ],
    faq: [
      {
        q: "Is a 529 different from a 429?",
        a: "Yes. A 429 is your own throughput ceiling; a 529 is upstream capacity. Backoff helps both, but only a 429 responds to reducing your own usage.",
      },
      {
        q: "Did my prompt cause the 529?",
        a: "No. Overloaded is a service-side condition. The identical request usually succeeds once capacity recovers.",
      },
      {
        q: "Should I switch models when I see 529s?",
        a: "For latency-sensitive work, temporarily using a smaller model helps because it is less contended. For everything else, waiting is enough.",
      },
    ],
    refCode: "overloaded",
    retryable: true,
  },
  {
    tool: "claude-code",
    slug: "usage-limit-reached",
    status: 0,
    title: "Claude usage limit reached in Claude Code — Reset Time and Options",
    description:
      "Claude Code shows \"Claude usage limit reached. Your limit will reset at…\" on subscription plans. What the cap actually counts, when it resets, and how pay-as-you-go API access differs.",
    searchStrings: [
      "Claude usage limit reached. Your limit will reset at 3pm (America/New_York)",
      "claude code usage limit reached",
      "claude weekly limit reached",
    ],
    causes: [
      "This is a Claude Pro or Max subscription cap, not an HTTP error. It appears when Claude Code is signed in with a subscription rather than an API key.",
      "Caps are enforced on a rolling window — commonly a five-hour session window plus a weekly ceiling — so heavy days can exhaust a weekly allowance well before the week ends.",
      "Long sessions resend their whole history every turn, so a single busy conversation consumes allowance far faster than its visible output suggests.",
    ],
    fixes: [
      "Wait for the stated reset — the message names the time, and it is a rolling window rather than a calendar boundary.",
      "Trim what each turn carries: /compact long conversations, avoid dragging one giant session through unrelated tasks.",
      "If the work cannot wait for a reset, pay-as-you-go API access is billed per token instead of per plan allowance, so it has no session or weekly cap to exhaust. That is the honest difference: metering, not a workaround.",
    ],
    snippet: {
      label: "Switch Claude Code from subscription to API balance",
      code: `export ANTHROPIC_BASE_URL="${BASE}"
export ANTHROPIC_AUTH_TOKEN="sk-pool-•••"
claude`,
    },
    faq: [
      {
        q: "When does the Claude usage limit reset?",
        a: "At the time named in the message itself. Session windows reset on a rolling basis (commonly five hours); weekly ceilings reset a week after the usage that filled them.",
      },
      {
        q: "Is this the same as API Error 429?",
        a: "No. The usage limit is a subscription allowance; a 429 is per-minute API throughput. They come from different systems and have different fixes.",
      },
      {
        q: "Does API access have the same weekly limit?",
        a: "No. API usage is metered per token against a balance, with per-minute rate limits but no session or weekly allowance.",
      },
    ],
    refCode: "usage-limit-reached",
    retryable: true,
  },

  // ——— Cursor ———
  {
    tool: "cursor",
    slug: "unable-to-reach-model-provider",
    status: 0,
    title: "Cursor: Unable to Reach the Model Provider — Causes and Fixes",
    description:
      "Cursor reports it cannot reach the model provider when the request dies before an HTTP response: wrong base URL, network path, or a provider outage. How to tell which one you have.",
    searchStrings: [
      "We're having trouble connecting to the model provider. This might be temporary - please try again in a moment.",
      "cursor unable to reach model provider",
      "cursor connection failed anthropic",
    ],
    causes: [
      "The custom base URL is wrong at the transport level — a typo in the host, a scheme mistake, or a URL that resolves nowhere — so no HTTP response ever arrives.",
      "The network path is blocked: a proxy, VPN or firewall between Cursor and the endpoint drops the connection.",
      "The provider itself is briefly down. In that case nothing on your side changed and the error clears on its own.",
      "An override URL was pasted with a trailing path segment the provider does not serve, so the TLS or HTTP layer fails before any API error can be returned.",
    ],
    fixes: [
      "Test the exact base URL outside Cursor with curl. If curl cannot connect either, the problem is the URL or the network, not Cursor.",
      "Set the override to the origin only and let the client append /v1 paths itself.",
      "If curl works but Cursor does not, check whether Cursor is routed through a proxy or VPN the terminal is not using.",
      "If nothing changed on your side and the error is new, wait a few minutes — transient provider incidents produce exactly this message.",
    ],
    snippet: {
      label: "Prove the endpoint is reachable",
      code: `curl ${BASE}/v1/models \\
  -H "x-api-key: sk-pool-•••" \\
  -H "anthropic-version: 2023-06-01"`,
    },
    faq: [
      {
        q: "Is \"unable to reach model provider\" the same as a 401 or 429?",
        a: "No. This message means no HTTP response arrived at all. A 401 or 429 means the provider answered and rejected the request — different layer, different fix.",
      },
      {
        q: "Cursor worked yesterday and fails today with no changes — what now?",
        a: "That pattern is a transient outage or a network-path change (VPN, proxy, captive portal). Verify with curl first; do not rewrite a configuration that already worked.",
      },
      {
        q: "What base URL should the Anthropic override use?",
        a: "The origin only — for this gateway, https://router.apitoken.sale. Do not append /v1: clients add API paths themselves, and a doubled path fails.",
      },
    ],
    retryable: true,
  },
  {
    tool: "cursor",
    slug: "invalid-api-key",
    status: 401,
    title: "Cursor Invalid API Key (Anthropic) — Why Verify Fails",
    description:
      "Cursor rejects an Anthropic key on verify when the key and the base URL do not belong together, or the override field still points at the default endpoint. The checklist that fixes it.",
    searchStrings: [
      "Invalid API key",
      "cursor anthropic api key not working",
      "cursor verify api key failed",
    ],
    causes: [
      "The key belongs to a custom endpoint but \"Override Anthropic Base URL\" is off or empty, so Cursor verifies the key against the default api.anthropic.com — which has never seen it.",
      "The base URL is set but the key was pasted with whitespace, a missing prefix, or is a different provider's key format entirely.",
      "The key was revoked or has expired.",
      "The override URL includes a path suffix, so the verification request goes to a URL the endpoint does not serve.",
    ],
    fixes: [
      "Enable the base URL override and set it to the origin that issued the key before pasting the key itself — verification runs against whatever URL is active at that moment.",
      "Paste the key with no surrounding whitespace and check the prefix matches what your dashboard shows.",
      "Confirm the key is active in the dashboard of its issuer, then re-verify in Cursor.",
    ],
    snippet: {
      label: "Cursor settings that verify against this gateway",
      code: `Cursor Settings → Models → Anthropic API Key
  API Key:  sk-pool-•••
  Override Anthropic Base URL:  ${BASE}`,
    },
    faq: [
      {
        q: "Why does Cursor say my key is invalid when it works in curl?",
        a: "Cursor verifies against the base URL configured at that moment. If the override is off, verification hits api.anthropic.com, which rejects a gateway key. Set the override first, then verify.",
      },
      {
        q: "Do I need an Anthropic account for a custom Anthropic-compatible key in Cursor?",
        a: "No. The Anthropic provider fields accept any Anthropic-compatible endpoint: set the override URL to the issuer and paste that issuer's key.",
      },
      {
        q: "Which models work after the key verifies?",
        a: "The ones the endpoint behind the override serves. List them with GET /v1/models against the same base URL and key.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  {
    tool: "cursor",
    slug: "provider-returned-401",
    status: 401,
    title: "Cursor: Request Failed With Status Code 401 — Anthropic Key Fix",
    description:
      "A 401 inside Cursor chat means the provider answered and rejected the credentials: key and base URL mismatched, key revoked, or the wrong header reaching the endpoint. The fix path.",
    searchStrings: [
      "Request failed with status code 401",
      `{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`,
      "cursor 401 anthropic",
    ],
    causes: [
      "The key verified once but was later revoked or expired — Cursor keeps using it until the provider starts answering 401.",
      "The base URL override was changed after the key was saved, so the saved key now goes to an endpoint that has never seen it.",
      "The account behind the key was suspended or its access withdrawn.",
    ],
    fixes: [
      "Re-check the pair as it exists right now: the override URL and the key must come from the same issuer. Fix whichever side changed.",
      "Test the exact key against the exact base URL with curl — it removes Cursor from the equation in one step.",
      "If curl also returns 401, the key itself is dead: check its status in the issuer's dashboard or reissue it.",
    ],
    snippet: {
      label: "Reproduce outside Cursor",
      code: `curl ${BASE}/v1/messages \\
  -H "x-api-key: sk-pool-•••" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{"model":"claude-sonnet-5","max_tokens":32,"messages":[{"role":"user","content":"ping"}]}'`,
    },
    faq: [
      {
        q: "Cursor verified my key earlier — why 401 now?",
        a: "Verification is a snapshot. A key revoked afterwards, or a base URL changed afterwards, produces 401s on every later request even though the initial verify passed.",
      },
      {
        q: "Is this Cursor's bug?",
        a: "Almost never. A 401 is the provider's answer passed through. Reproduce it with curl: if curl gets 401 too, the credentials are the problem.",
      },
      {
        q: "What if curl succeeds but Cursor still gets 401?",
        a: "Then Cursor is sending different credentials than you think — re-open settings and re-paste the key with the override URL already enabled.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  // ——— Codex CLI ———
  {
    tool: "codex",
    slug: "missing-openai-api-key",
    status: 0,
    title: "Codex CLI: Missing OPENAI_API_KEY — Custom Provider Setup",
    description:
      "Codex refuses to start when the environment variable its provider expects is unset. How env_key works in config.toml, why exports vanish between shells, and a working profile.",
    searchStrings: [
      "Missing OPENAI_API_KEY",
      "OPENAI_API_KEY environment variable not set",
      "codex api key not found",
    ],
    causes: [
      "The environment variable named by the provider's env_key is not set in the shell that runs codex. With a custom provider the variable can be any name — the error names whichever one is missing.",
      "The key was exported in a different terminal or added to a profile file that this shell never sourced.",
      "The variable is set but empty, which counts as missing.",
      "The profile in use points at a different provider than you think, so Codex looks for a different variable than the one you exported.",
    ],
    fixes: [
      "Declare the provider in a profile and export the exact variable its env_key names, in the same shell, before running codex.",
      "Print the variable right before launching to confirm it survived: an export is per-shell, not global.",
      "Run codex with the profile named explicitly so there is no ambiguity about which provider — and which env_key — is active.",
    ],
    snippet: {
      label: "Working profile for this gateway",
      code: `# ~/.codex/apitoken.config.toml
model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "${OPENAI_BASE}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"

# then, in the same shell:
export APITOKEN_API_KEY="sk-pool-•••"
codex --profile apitoken`,
    },
    faq: [
      {
        q: "Do I need an OpenAI account key to run Codex CLI?",
        a: "No. With a custom model provider, env_key names any variable you choose, and the key comes from that provider — OPENAI_API_KEY itself is only the default provider's variable.",
      },
      {
        q: "I exported the key but Codex still says it is missing — why?",
        a: "Exports are per-shell. If codex runs in another terminal, a multiplexer pane, or an IDE task, that environment never saw your export. Set it where codex actually runs.",
      },
      {
        q: "Where should the key live, the TOML or the shell?",
        a: "The shell. config.toml stores only the variable's name via env_key, so the secret never sits in a config file.",
      },
    ],
    retryable: false,
  },
  {
    tool: "codex",
    slug: "config-toml-error",
    status: 0,
    title: "Codex config.toml Errors — model_providers Setup That Works",
    description:
      "Codex fails to load a malformed ~/.codex/config.toml: unknown provider names, wrong wire_api, missing sections. The exact structure a custom provider needs.",
    searchStrings: [
      "codex config.toml error",
      "unknown model provider",
      "codex model_providers not working",
    ],
    causes: [
      "model_provider names a provider that has no matching [model_providers.<name>] section — the reference and the section must use the same identifier.",
      "TOML syntax errors: an unclosed string, a section header typo, or values pasted from JSON with commas and braces.",
      "wire_api does not match what the endpoint serves, so requests are shaped for the wrong protocol.",
      "The file edited is not the file Codex reads — a profile file passed with --profile has a specific expected location and name.",
    ],
    fixes: [
      "Keep the provider identifier identical in both places: model_provider = \"apitoken\" must be matched by [model_providers.apitoken].",
      "Use wire_api = \"responses\" for a Responses-API endpoint — this gateway's OpenAI-compatible surface serves Responses and Chat Completions.",
      "Validate the file is real TOML: strings quoted, one key = value per line, section headers in brackets.",
      "Run codex --profile <name> and watch which file it reports loading; fix that file, not a lookalike.",
    ],
    snippet: {
      label: "Minimal correct profile",
      code: `model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "${OPENAI_BASE}"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"`,
    },
    faq: [
      {
        q: "Why does Codex say my model provider is unknown?",
        a: "The model_provider value must exactly match a [model_providers.<name>] section in the same file. A typo on either side breaks the lookup.",
      },
      {
        q: "Should wire_api be responses or chat?",
        a: "Whatever the endpoint serves. This gateway's OpenAI-compatible surface accepts the Responses API — wire_api = \"responses\" — and Chat Completions as well.",
      },
      {
        q: "Does the base_url need /v1?",
        a: "For Codex profiles against this gateway, yes: the documented value is https://router.apitoken.sale/v1, exactly as shipped in the docs.",
      },
    ],
    retryable: false,
  },
  {
    tool: "codex",
    slug: "auth-json-error",
    status: 0,
    title: "Codex auth.json / Login Errors — API Key Auth Instead",
    description:
      "Codex login state lives in ~/.codex/auth.json, and a stale or missing file produces login prompts and auth failures. When to re-login and when a custom provider skips it entirely.",
    searchStrings: [
      "codex auth.json error",
      "codex not logged in",
      "codex login failed",
    ],
    causes: [
      "auth.json is missing, unreadable or stale, so the default provider has no working login.",
      "The login belongs to a different account or plan state than the session expects — refreshing tokens no longer succeed.",
      "The failure is misattributed: with a custom provider and env_key, ChatGPT-login state is irrelevant, and the real problem is the environment variable or profile.",
    ],
    fixes: [
      "For the default provider, re-run the login flow so auth.json is rewritten fresh.",
      "For a custom provider, auth.json does not participate: authentication is the env_key variable. Check the profile is actually selected and the variable is set in the running shell.",
      "Keep the two paths mentally separate — subscription login for the default provider, API key for custom providers. Errors from one are not fixed in the other.",
    ],
    faq: [
      {
        q: "Does a custom provider in Codex use auth.json?",
        a: "No. A [model_providers.*] entry authenticates with the environment variable named by env_key. auth.json only carries the default ChatGPT login.",
      },
      {
        q: "Why does Codex keep asking me to log in?",
        a: "Its stored login state cannot be refreshed. Re-login to rewrite it — or, if you meant to use a custom provider, select the profile explicitly so no login is needed.",
      },
      {
        q: "Can I run Codex without any ChatGPT subscription?",
        a: "Yes — with a custom provider profile and that provider's API key in the environment, Codex runs entirely on API-key auth.",
      },
    ],
    retryable: false,
  },
  {
    tool: "codex",
    slug: "stream-error",
    status: 401,
    title: "Codex: stream error: unexpected status 401/404 — Fix",
    description:
      "Mid-stream HTTP failures in Codex print as stream error: unexpected status. What 401, 404 and 429 mean there, and how to reproduce them with curl to find the broken half.",
    searchStrings: [
      "stream error: unexpected status 401 Unauthorized",
      "codex stream error unexpected status",
      "stream disconnected before completion",
    ],
    causes: [
      "401 — the env_key variable is unset, empty, or its key does not belong to the base_url in the profile.",
      "404 — the base_url is wrong for the wire protocol: a missing /v1, a doubled /v1/v1, or a host that does not serve the Responses API.",
      "429 — the key's per-minute throughput is exhausted; the stream is refused before it starts.",
      "A proxy or network device cutting the SSE stream mid-response produces the disconnect variant rather than a status.",
    ],
    fixes: [
      "Reproduce with curl against the exact base_url from the profile plus /responses, sending the same key — the status tells you which half is broken.",
      "For 401, fix the key/endpoint pair; for 404, fix the base_url to the documented value; for 429, wait out the window and reduce parallel runs.",
      "For repeated mid-stream disconnects with no status, test without VPN or proxy — SSE is the first casualty of interfering middleboxes.",
    ],
    snippet: {
      label: "Reproduce outside Codex",
      code: `curl ${OPENAI_BASE}/responses \\
  -H "Authorization: Bearer $APITOKEN_API_KEY" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"gpt-5.6-sol","input":"Reply with exactly: connected"}'`,
    },
    faq: [
      {
        q: "What does stream error: unexpected status 401 mean in Codex?",
        a: "The endpoint rejected the credentials at stream start. Check that the profile's env_key variable is set in the running shell and that its key belongs to the profile's base_url.",
      },
      {
        q: "Why 404 when the host is clearly right?",
        a: "Path mistakes: base_url must include /v1 for this gateway, and must not double it. The Responses path is appended by Codex itself.",
      },
      {
        q: "The stream dies halfway with no status — is that the API?",
        a: "Usually the network path: proxies and VPNs that buffer or terminate SSE. Reproduce with curl on a clean connection before blaming the endpoint.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },

  // ——— opencode ———
  {
    tool: "opencode",
    slug: "ai-apicallerror",
    status: 0,
    title: "opencode AI_APICallError — Causes and Fixes",
    description:
      "opencode surfaces provider failures as AI_APICallError from the Vercel AI SDK. How to read the wrapped status code and fix the baseURL, key or model underneath.",
    searchStrings: [
      "AI_APICallError",
      "opencode AI_APICallError",
      "opencode api call error",
    ],
    causes: [
      "AI_APICallError is a wrapper, not a diagnosis: the AI SDK throws it for any non-success HTTP response, and the real cause is the wrapped status and body.",
      "401 inside it — the provider's apiKey option is wrong, or the {env:...} placeholder names a variable that is not set.",
      "404 inside it — the baseURL is wrong for the provider's protocol: missing /v1, doubled /v1/v1, or a model id the endpoint does not serve.",
      "429 or 529 inside it — throughput or upstream capacity; nothing in your config is wrong.",
    ],
    fixes: [
      "Read the statusCode and responseBody fields of the error first — they carry the provider's actual answer.",
      "Verify the provider block: baseURL exact, apiKey resolving from an environment variable that exists in the shell that launched opencode.",
      "Restart opencode after any opencode.json change — the config is read at startup.",
    ],
    snippet: {
      label: "Provider block for this gateway",
      code: `{
  "provider": {
    "apitoken": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "apiToken.sale",
      "options": {
        "baseURL": "${OPENAI_BASE}",
        "apiKey": "{env:APITOKEN_API_KEY}"
      }
    }
  }
}`,
    },
    faq: [
      {
        q: "Is AI_APICallError an opencode bug?",
        a: "No — it is the AI SDK reporting that the provider returned a non-success response. The wrapped status code identifies the real problem.",
      },
      {
        q: "Where does opencode read my API key from?",
        a: "From the provider's options.apiKey. With an {env:NAME} placeholder, the variable must exist in the environment opencode was launched from.",
      },
      {
        q: "I fixed opencode.json and nothing changed — why?",
        a: "opencode reads its config at startup. Restart it after every change.",
      },
    ],
    retryable: false,
  },
  {
    tool: "opencode",
    slug: "model-not-found",
    status: 404,
    title: "opencode Model Not Found — Declaring Custom Provider Models",
    description:
      "opencode only offers models it knows about, and custom provider models are not in the models.dev catalog. How to declare a model in opencode.json so it appears and works.",
    searchStrings: [
      "opencode model not found",
      "ModelNotFoundError",
      "opencode custom provider model missing",
    ],
    causes: [
      "The model is not declared in the provider's models map, and custom provider models are not in the models.dev catalog opencode consults — so the model simply is not offered.",
      "The declared model id does not match what the endpoint serves — a typo or a retired id returns 404 from the provider.",
      "The model was added to opencode.json but opencode was not restarted, so the old config is still active.",
    ],
    fixes: [
      "Declare each model explicitly in the provider's models map in opencode.json, with the id exactly as the endpoint serves it.",
      "List the endpoint's live models with GET /v1/models rather than guessing ids.",
      "Restart opencode after the change.",
    ],
    snippet: {
      label: "Declare the model explicitly",
      code: `{
  "provider": {
    "apitoken": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "apiToken.sale",
      "options": {
        "baseURL": "${OPENAI_BASE}",
        "apiKey": "{env:APITOKEN_API_KEY}"
      },
      "models": {
        "gpt-5.6-sol": { "name": "GPT-5.6 Sol" }
      }
    }
  }
}`,
    },
    faq: [
      {
        q: "Why doesn't my custom provider's model show up in opencode?",
        a: "Models outside the models.dev catalog must be declared by hand in the provider's models map. Undeclared models are not offered at all.",
      },
      {
        q: "How do I find the exact model id to declare?",
        a: "Query the provider's GET /v1/models with your key and copy the id verbatim.",
      },
      {
        q: "The model is declared but requests 404 — what else?",
        a: "The id in opencode.json must match the endpoint's id character for character, and the baseURL must be the documented one. Check both, then restart opencode.",
      },
    ],
    retryable: false,
  },
  {
    tool: "opencode",
    slug: "auth-config-error",
    status: 401,
    title: "opencode API Key Not Picked Up — auth and {env} Placeholders",
    description:
      "opencode authenticates a custom provider through options.apiKey, usually an {env:...} placeholder. Why the variable silently resolves empty and requests fail with 401.",
    searchStrings: [
      "opencode 401 unauthorized",
      "opencode api key not working",
      "opencode auth error custom provider",
    ],
    causes: [
      "The {env:NAME} placeholder names a variable that is not set in the environment opencode launched from — it resolves empty and the provider sees no key.",
      "The variable is exported in one shell but opencode starts from another: a desktop launcher, another terminal, a multiplexer pane.",
      "The key is pasted literally into opencode.json with surrounding whitespace, or it belongs to a different endpoint than the baseURL.",
    ],
    fixes: [
      "Export the exact variable named in the placeholder in the same shell, then start opencode from that shell.",
      "Confirm the pair with curl: the baseURL from the config plus the key from the variable must succeed outside opencode first.",
      "Prefer the {env:...} form over pasting the key into the file — it keeps the secret out of dotfiles and version control.",
    ],
    snippet: {
      label: "Key via environment, not in the file",
      code: `export APITOKEN_API_KEY="sk-pool-•••"
opencode

# opencode.json references it as:
#   "apiKey": "{env:APITOKEN_API_KEY}"`,
    },
    faq: [
      {
        q: "Why does opencode send no API key even though opencode.json names one?",
        a: "An {env:NAME} placeholder resolves at startup from opencode's own environment. If that shell never exported the variable, the key is empty.",
      },
      {
        q: "Is it safe to paste the key directly into opencode.json?",
        a: "It works, but the env placeholder is the better habit: config files travel into backups and repositories, and a pasted key travels with them.",
      },
      {
        q: "How do I check which half is broken, the key or the URL?",
        a: "curl the baseURL with the key by hand. 401 means the key/endpoint pair; connection errors mean the URL or network.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  {
    tool: "opencode",
    slug: "image-input-not-supported",
    status: 0,
    title: "opencode: This Model Does Not Support Image Input — Fix",
    description:
      "opencode silently replaces attached images with a note when the model's modalities are not declared. The one-line opencode.json declaration that turns image input on.",
    searchStrings: [
      "this model does not support image input",
      "opencode image not sent",
      "opencode attachment ignored",
    ],
    causes: [
      "Custom provider models are not in the models.dev catalog, so opencode assigns them text-only default capabilities — regardless of what the model can actually do.",
      "With text-only capabilities, opencode replaces attached images with an in-band \"this model does not support image input\" note; the provider never receives the image at all.",
      "The modalities were declared but opencode was not restarted afterwards.",
    ],
    fixes: [
      "Declare the image modality explicitly for the model in opencode.json and restart opencode.",
      "Once \"image\" is in modalities.input, pasted and attached images go out as standard Chat Completions image parts, which this gateway accepts.",
    ],
    snippet: {
      label: "Declare image input for the model",
      code: `{
  "provider": {
    "apitoken": {
      "models": {
        "gpt-5.6-sol": {
          "name": "GPT-5.6 Sol",
          "attachment": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          }
        }
      }
    }
  }
}`,
    },
    faq: [
      {
        q: "Why does the model answer as if it never saw my image?",
        a: "It never did. Without a declared image modality, opencode strips the attachment and sends a text note in its place — the request that reaches the provider is text-only.",
      },
      {
        q: "Is this a provider limitation?",
        a: "No — it is a client-side capability gate. The same model accepts images as soon as the modality is declared in opencode.json.",
      },
      {
        q: "Do other tools need this declaration?",
        a: "Only capability-gated clients. The wire contract is plain Chat Completions image parts, so ungated clients like the OpenAI SDKs need nothing extra.",
      },
    ],
    retryable: false,
  },

  // ——— Cline ———
  {
    tool: "cline",
    slug: "api-request-failed",
    status: 0,
    title: "Cline API Request Failed — Diagnosing the Real Error",
    description:
      "Cline's API Request Failed banner is a wrapper around the provider's response. How to read the status and error.type underneath and route yourself to the actual fix.",
    searchStrings: [
      "API Request Failed",
      "cline api request failed",
      "cline api error",
    ],
    causes: [
      "The banner is generic: Cline shows it for any failed provider call. The diagnosis is the status code and error body underneath, not the banner text.",
      "401 underneath — key/endpoint mismatch or a revoked key.",
      "429 underneath — the key's per-minute budget, amplified by Cline's automatic retries.",
      "400 with a context message underneath — the conversation plus max_tokens no longer fits the model's window.",
    ],
    fixes: [
      "Expand the error and read the JSON body. Every body maps to a specific page: 401 to the invalid-key fix, 429 to the rate-limit fix, context 400 to the context-limit fix.",
      "If the body is a connection failure rather than JSON, treat it as a network/base-URL problem and verify the endpoint with curl.",
      "Do not toggle unrelated settings on repeat failures — reproduce outside Cline first, then change exactly one thing.",
    ],
    faq: [
      {
        q: "What does API Request Failed actually mean in Cline?",
        a: "Only that the provider call did not succeed. The real error is the status and body shown with it — read those first.",
      },
      {
        q: "Cline retries and fails repeatedly — should I keep clicking retry?",
        a: "No. For 401 and 400 the same request will fail forever; fix the cause. Retrying only helps transient 429/5xx conditions, and Cline already does that itself.",
      },
      {
        q: "How do I take Cline out of the equation?",
        a: "Send the same request with curl using the base URL and key from Cline's settings. The curl result tells you whether the problem is the setup or the tool.",
      },
    ],
    retryable: false,
  },
  {
    tool: "cline",
    slug: "invalid-api-key-401",
    status: 401,
    title: "Cline 401 invalid x-api-key (Anthropic) — Fix",
    description:
      "Cline returns 401 invalid x-api-key when the Anthropic-compatible endpoint rejects the key: wrong base URL field, whitespace in the key, or a revoked key. The fix checklist.",
    searchStrings: [
      `{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`,
      "cline 401 anthropic",
      "cline invalid api key",
    ],
    causes: [
      "The custom base URL option is off, so the key is verified against the default endpoint that has never seen it.",
      "The key was pasted with leading or trailing whitespace, or truncated.",
      "The base URL includes a path suffix, so requests go to a URL the endpoint does not serve.",
      "The key was revoked or expired.",
    ],
    fixes: [
      "In Cline's Anthropic provider settings, enable the custom base URL and set it to the origin that issued the key — for this gateway, https://router.apitoken.sale, with no /v1 suffix.",
      "Re-paste the key cleanly and confirm it is active in the issuer's dashboard.",
      "Verify the pair with curl before returning to Cline.",
    ],
    snippet: {
      label: "Cline provider settings",
      code: `API Provider:  Anthropic
Anthropic API Key:  sk-pool-•••
[x] Use custom base URL:  ${BASE}`,
    },
    faq: [
      {
        q: "Why does Cline reject a key that works elsewhere?",
        a: "If the custom base URL box is unchecked, Cline sends the key to api.anthropic.com. Enable the base URL first, then the same key verifies.",
      },
      {
        q: "Does the Cline base URL need /v1?",
        a: "No — the origin only. Cline's SDK appends API paths itself; a /v1 suffix produces doubled paths and hard-to-read failures.",
      },
      {
        q: "Which models can I select after the key works?",
        a: "The ones the endpoint serves. List them with GET /v1/models against the same base URL and key.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  {
    tool: "cline",
    slug: "rate-limit-429",
    status: 429,
    title: "Cline 429 rate_limit_error — Stop the Retry Loop",
    description:
      "Cline agent runs burn per-minute token budgets quickly and then loop on 429 retries. Why agent workloads trigger rate limits and how to get a run through.",
    searchStrings: [
      `{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your organization's rate limit of 80,000 input tokens per minute. Please reduce the prompt length or the maximum tokens requested, or try again later."}}`,
      "cline 429 rate limit",
      "cline retrying request",
    ],
    causes: [
      "Agent loops are token-hungry: every step resends the conversation, file context and tool results, so a single active task can spend a minute's budget alone.",
      "The key is shared with other tools or teammates and their traffic fills the same window.",
      "Automatic retries during a saturated window keep the window saturated.",
    ],
    fixes: [
      "Let one 429 pass — Cline backs off and retries. If a run loops on 429s, pause it for a minute instead of restarting it repeatedly.",
      "Reduce the context the task carries: smaller file selections, narrower task scope per run.",
      "Give Cline a dedicated key so other tools' bursts do not eat its window — and if you consistently need more throughput, raise the ceiling with the key's issuer rather than fighting it.",
    ],
    faq: [
      {
        q: "Why does Cline hit 429 so much faster than chat tools?",
        a: "An agent step is not one message — it resends history, file context and tool output every iteration. Token throughput per minute is several times a chat's.",
      },
      {
        q: "Will retrying immediately help?",
        a: "No — the window is per minute, and immediate retries keep it full. Backing off for the window's remainder is what clears it.",
      },
      {
        q: "Is this Cline's own limit?",
        a: "No. Cline has no metering of its own with your key; the 429 is the API organization's per-minute ceiling behind that key.",
      },
    ],
    refCode: "rate-limit",
    retryable: true,
  },
  {
    tool: "cline",
    slug: "context-limit",
    status: 400,
    title: "Cline: input length and max_tokens exceed context limit — Fix",
    description:
      "The 400 input length and max_tokens exceed context limit appears when Cline's accumulated task context plus the output reservation no longer fit the model's window. How to recover.",
    searchStrings: [
      "input length and `max_tokens` exceed context limit: 199999 + 8192 > 200000",
      "cline context limit exceeded",
      "prompt is too long: 212164 tokens > 199999 maximum",
    ],
    causes: [
      "The conversation history plus attached files plus the max_tokens reservation exceed the model's context window — the two numbers in the message are your input and the ceiling.",
      "Long-running tasks accumulate every file read and tool result; nothing is dropped automatically until the window is already full.",
      "A large max_tokens setting reserves output space that then no longer fits next to the input.",
    ],
    fixes: [
      "Start a new task for the next unit of work — carrying one giant task across a whole feature is what fills the window.",
      "Reduce what the task holds: narrower file mentions, avoid pasting whole files that could be referenced.",
      "If the model supports a larger context window variant, selecting it raises the ceiling; otherwise trim input rather than raising max_tokens.",
    ],
    faq: [
      {
        q: "What do the numbers in the error mean?",
        a: "Your input tokens plus the max_tokens reservation, against the model's window: 199999 + 8192 > 200000 says the input alone nearly fills the window before any output is reserved.",
      },
      {
        q: "Why did this appear mid-task when everything worked before?",
        a: "Context accumulates every step. The task crossed the ceiling at that step, not because that step was special but because it was one step too many.",
      },
      {
        q: "Does lowering max_tokens fix it?",
        a: "Sometimes briefly — it shrinks the reservation. The durable fix is less input: smaller tasks, trimmed context.",
      },
    ],
    refCode: "prompt-too-long",
    retryable: false,
  },

  // ——— Zed ———
  {
    tool: "zed",
    slug: "invalid-api-key",
    status: 401,
    title: "Zed Claude 401 invalid x-api-key — Anthropic Settings Fix",
    description:
      "Zed's assistant passes the Anthropic 401 invalid x-api-key through verbatim. Where the key and api_url live in Zed settings and why they must come from the same issuer.",
    searchStrings: [
      `{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}`,
      "zed anthropic api key not working",
      "zed claude 401",
    ],
    causes: [
      "The key in Zed's Anthropic provider settings and the api_url point at different issuers — a gateway key sent to the default endpoint, or vice versa.",
      "The key was pasted with whitespace or truncated.",
      "A custom api_url was set with a /v1 suffix, so requests hit a doubled path and fail before or instead of auth.",
      "The key was revoked or expired.",
    ],
    fixes: [
      "Set language_models.anthropic.api_url to the origin that issued the key, and paste that issuer's key in the provider settings.",
      "Keep the api_url to the origin only — Zed appends /v1 paths itself.",
      "Verify the exact pair with curl before changing anything else.",
    ],
    snippet: {
      label: "Zed settings.json for this gateway",
      code: `{
  "language_models": {
    "anthropic": {
      "api_url": "${BASE}"
    }
  }
}`,
    },
    faq: [
      {
        q: "Where does Zed keep the Anthropic base URL?",
        a: "In settings.json under language_models.anthropic.api_url. The API key itself is entered in the assistant's provider configuration.",
      },
      {
        q: "Why does my key fail in Zed but work in curl?",
        a: "Zed sends it to whatever api_url is configured. If that differs from the URL you curl — including a stray /v1 — the endpoint seeing the key is not the one that issued it.",
      },
      {
        q: "Do I need an Anthropic account to use Claude in Zed?",
        a: "No — any Anthropic-compatible endpoint works: set api_url to the issuer and use its key.",
      },
    ],
    refCode: "invalid-api-key",
    retryable: false,
  },
  {
    tool: "zed",
    slug: "rate-limit-429",
    status: 429,
    title: "Zed Assistant 429 rate_limit_error — Fix",
    description:
      "A 429 in Zed's assistant is the per-minute ceiling of the API organization behind your key. Why long threads and shared keys trigger it and what clears it.",
    searchStrings: [
      `{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your organization's rate limit of 80,000 input tokens per minute. Please reduce the prompt length or the maximum tokens requested, or try again later."}}`,
      "zed claude rate limit",
      "zed assistant 429",
    ],
    causes: [
      "The per-minute token budget of the key's organization was exceeded — long assistant threads resend their whole history each message.",
      "The same key is used by other tools at the same time, and their traffic shares the window.",
      "Large context attachments multiply the tokens each message carries.",
    ],
    fixes: [
      "Wait out the minute window, then continue — a single 429 needs no configuration change.",
      "Start a fresh thread for new work instead of extending one long-running conversation.",
      "Give Zed its own key if the current one is shared across tools.",
    ],
    faq: [
      {
        q: "Is this Zed's limit or my key's limit?",
        a: "The key's. Zed adds no metering of its own; the 429 comes from the API organization behind the configured key.",
      },
      {
        q: "Why do long threads make it worse?",
        a: "Each message resends the entire thread. The counted input grows with the conversation even when your visible question is short.",
      },
      {
        q: "Does retrying immediately help?",
        a: "No — the window is per minute. Immediate retries keep it saturated; a short pause clears it.",
      },
    ],
    refCode: "rate-limit",
    retryable: true,
  },
  {
    tool: "zed",
    slug: "overloaded-529",
    status: 529,
    title: "Zed: 529 Overloaded From Claude — What To Do",
    description:
      "Zed shows the Anthropic 529 Overloaded verbatim when upstream capacity is saturated. Why your settings are not the cause and what to do while it lasts.",
    searchStrings: [
      `{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`,
      "zed claude overloaded",
      "zed 529 error",
    ],
    causes: [
      "Upstream capacity is temporarily saturated. The 529 describes the service, not your request or your Zed configuration.",
      "It clusters during incidents and peak hours, then clears with no change on your side.",
    ],
    fixes: [
      "Retry after a short pause — the identical request typically succeeds once capacity recovers.",
      "For work that cannot wait, switch the thread to a smaller model temporarily; smaller models are generally less contended.",
      "Resist reconfiguring: api_url and key changes cannot fix a capacity condition and usually introduce a real error on top.",
    ],
    faq: [
      {
        q: "Did my Zed configuration cause the 529?",
        a: "No. Overloaded is a service-side condition passed through verbatim. If your setup produced errors, they would be 401s or 404s, not 529s.",
      },
      {
        q: "How long does a 529 episode last?",
        a: "Usually minutes. If it persists, check the provider's status page rather than editing settings.",
      },
      {
        q: "Is 529 the same as 429?",
        a: "No — 429 is your own throughput ceiling, 529 is upstream capacity. Only the 429 responds to reducing your usage.",
      },
    ],
    refCode: "overloaded",
    retryable: true,
  },
  {
    tool: "zed",
    slug: "api-url-config",
    status: 404,
    title: "Zed api_url for Anthropic — Custom Base URL Setup and the /v1/v1 404",
    description:
      "How to point Zed's Anthropic provider at a custom endpoint with language_models.anthropic.api_url, and why appending /v1 produces 404s from a doubled path.",
    searchStrings: [
      "zed custom anthropic api url",
      "zed api_url not working",
      `{"type":"error","error":{"type":"not_found_error","message":"Not found"}}`,
    ],
    causes: [
      "api_url was set including /v1, and Zed appends API paths itself — the request goes to /v1/v1/messages, which does not exist.",
      "The api_url was added under the wrong settings key or with a typo, so Zed silently keeps using the default endpoint.",
      "The endpoint is right but the selected model id is one it does not serve, which is the other source of 404s.",
    ],
    fixes: [
      "Set api_url to the origin only and let Zed build the paths.",
      "Put the setting exactly at language_models.anthropic.api_url in settings.json — a misplaced key does not error, it just does nothing.",
      "If the 404 body names a model, the path is fine and the model id is the problem: list the endpoint's models and pick a served id.",
    ],
    snippet: {
      label: "Correct settings.json",
      code: `{
  "language_models": {
    "anthropic": {
      "api_url": "${BASE}"
    }
  }
}
// wrong: "api_url": "${BASE}/v1"  → /v1/v1/messages → 404`,
    },
    faq: [
      {
        q: "Should the Zed api_url include /v1?",
        a: "No. Zed appends /v1/messages itself. An api_url ending in /v1 produces a doubled /v1/v1 path and a 404.",
      },
      {
        q: "How do I know my custom api_url is actually in effect?",
        a: "Break it intentionally for one request (a nonsense host) — if nothing changes, Zed is not reading the key you edited; fix the settings path.",
      },
      {
        q: "The path is right but I still get 404 — why?",
        a: "The 404 body usually names the missing thing. If it names a model, select an id the endpoint serves; list them via GET /v1/models.",
      },
    ],
    refCode: "not-found",
    retryable: false,
  },
  {
    tool: "cursor",
    slug: "rate-limit-exceeded",
    status: 429,
    title: "Cursor Rate Limit / 429 With Your Own Anthropic Key",
    description:
      "With a custom Anthropic key, a 429 in Cursor is the key's own per-minute ceiling, not Cursor's plan limits. Why long chats trigger it and how to stop the loop.",
    searchStrings: [
      `{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your organization's rate limit of 80,000 input tokens per minute. Please reduce the prompt length or the maximum tokens requested, or try again later."}}`,
      "cursor rate limit exceeded custom api key",
      "cursor 429 anthropic key",
    ],
    causes: [
      "The per-minute token ceiling of the organization behind your key was exceeded. With a custom key, Cursor's own plan limits are out of the picture — this is your key's budget.",
      "Long chats resend the whole conversation plus attached context every message, so one busy tab can spend a minute's token budget alone.",
      "The same key is shared with other tools or teammates, and their traffic counts against the same window.",
      "Rapid-fire retries after the first 429 keep the window full.",
    ],
    fixes: [
      "Start a new chat for a new task instead of extending one giant conversation — context resent per message is what eats the budget.",
      "Give Cursor its own key if the current one is shared, so one tool's burst cannot starve another.",
      "Wait out the minute window before retrying; tight manual retries prolong the condition.",
    ],
    faq: [
      {
        q: "Is this Cursor's fast-requests limit?",
        a: "No. With your own key, requests bypass Cursor's plan metering entirely. The 429 comes from the API organization behind your key.",
      },
      {
        q: "Why do long conversations hit 429 more often?",
        a: "Every message resends the whole history and attachments. The visible reply is small, but the counted input tokens grow with the conversation.",
      },
      {
        q: "Does upgrading my Cursor plan help?",
        a: "Not for this error. The ceiling belongs to the API key. Reduce per-minute usage, stop sharing the key, or arrange higher throughput with its issuer.",
      },
    ],
    refCode: "rate-limit",
    retryable: true,
  },
];

// --- Lookups and paths ------------------------------------------------------

export function toolErrors(tool: ToolSlug): ToolErrorEntry[] {
  return TOOL_ERRORS.filter((entry) => entry.tool === tool);
}

export function findToolError(tool: string, slug: string): ToolErrorEntry | undefined {
  return TOOL_ERRORS.find((entry) => entry.tool === tool && entry.slug === slug);
}

function localePrefix(locale: ToolErrorLocale): string {
  return locale === "en" ? "" : `/${locale}`;
}

export function toolErrorsIndexPath(locale: ToolErrorLocale): string {
  return `${localePrefix(locale)}/errors`;
}

export function toolHubPath(tool: ToolSlug, locale: ToolErrorLocale): string {
  return `${localePrefix(locale)}/errors/${tool}`;
}

export function toolErrorPath(tool: ToolSlug, slug: string, locale: ToolErrorLocale): string {
  return `${localePrefix(locale)}/errors/${tool}/${slug}`;
}

// --- Localization -----------------------------------------------------------
//
// Same contract as lib/api-errors.ts: only explanatory prose is translated.
// `searchStrings` and snippet code are tool output and configuration — they stay
// as-is in every locale, because people paste what their terminal printed.

export type LocalizedToolError = {
  title: string;
  description: string;
  causes: string[];
  fixes: string[];
  snippetLabel?: string;
  faq: ToolFaq[];
};

export type LocalizedToolInfo = {
  title: string;
  description: string;
  intro: string;
};

export type ToolErrorsUi = {
  eyebrow: string;
  indexTitle: string;
  indexDescription: string;
  indexIntro: string;
  errorsIn: string; // "Errors in {tool}" heading pattern, {tool} substituted
  whatYouSee: string;
  why: string;
  how: string;
  faqHeading: string;
  alsoSearched: string;
  colError: string;
  colMeaning: string;
  backToTool: string;
  allTools: string;
  fullReference: string;
  fullReferenceBlurb: string;
  setupGuide: string;
  ctaHeading: string;
  ctaBody: string;
  ctaButton: string;
  ctaDocs: string;
};

export type ToolErrorTranslations = {
  ui: ToolErrorsUi;
  index: LocalizedToolInfo;
  tools: Record<ToolSlug, LocalizedToolInfo>;
  entries: Record<string, LocalizedToolError>; // key: "<tool>/<slug>"
};

export const toolErrorsUiEn: ToolErrorsUi = {
  eyebrow: "Troubleshooting",
  indexTitle: TOOL_ERRORS_INDEX.title,
  indexDescription: TOOL_ERRORS_INDEX.description,
  indexIntro: TOOL_ERRORS_INDEX.intro,
  errorsIn: "{tool} errors",
  whatYouSee: "What you see",
  why: "Why it happens",
  how: "How to fix it",
  faqHeading: "FAQ",
  alsoSearched: "Related searches",
  colError: "Error",
  colMeaning: "Meaning",
  backToTool: "All {tool} errors",
  allTools: "All tools",
  fullReference: "Claude API error codes",
  fullReferenceBlurb: "The API-level reference: every status code and verbatim response body, organized by code rather than by tool.",
  setupGuide: "Setup guide",
  ctaHeading: "Skip the broken setup",
  ctaBody: "apiToken.sale serves the standard Anthropic API — same models, same SDKs, one prepaid balance. Point your tool's base URL at it, and the config on this page works as written.",
  ctaButton: "Get a key",
  ctaDocs: "API docs",
};

export type ResolvedToolError = ToolErrorEntry & {
  localeTitle: string;
  localeDescription: string;
};

/** Merge an entry with its translation; EN passes through. */
export function resolveToolError(
  entry: ToolErrorEntry,
  locale: ToolErrorLocale,
  translations?: ToolErrorTranslations,
): ResolvedToolError {
  if (locale === "en" || !translations) {
    return { ...entry, localeTitle: entry.title, localeDescription: entry.description };
  }
  const translated = translations.entries[`${entry.tool}/${entry.slug}`];
  // Fall back to English rather than dropping content: a missing translation
  // must degrade to a usable page, not a hole in the cluster.
  if (!translated) {
    return { ...entry, localeTitle: entry.title, localeDescription: entry.description };
  }
  return {
    ...entry,
    localeTitle: translated.title,
    localeDescription: translated.description,
    causes: translated.causes,
    fixes: translated.fixes,
    faq: translated.faq,
    snippet: entry.snippet
      ? { ...entry.snippet, label: translated.snippetLabel ?? entry.snippet.label }
      : undefined,
  };
}

export function resolveToolInfo(
  tool: ToolInfo,
  locale: ToolErrorLocale,
  translations?: ToolErrorTranslations,
): LocalizedToolInfo {
  if (locale === "en" || !translations) {
    return { title: tool.title, description: tool.description, intro: tool.intro };
  }
  return translations.tools[tool.slug] ?? { title: tool.title, description: tool.description, intro: tool.intro };
}
