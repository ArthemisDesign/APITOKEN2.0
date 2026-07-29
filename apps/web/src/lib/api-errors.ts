// Catalog of the error responses a Claude API client actually sees, and what to do
// about each one. Single source of truth: it renders /docs/errors, the /md/docs/errors
// Markdown twin, the FAQ/TechArticle structured data, and the /e/<code> short links.
//
// The verbatim `message` strings matter more than the prose around them: developers
// paste the exact string into a search box or an AI assistant, so the page has to
// contain it character for character. Every string here was taken from a real report
// (SDK issue trackers, tool forums) or from the published API reference — never
// paraphrased, because a paraphrase matches nothing.

export type ErrorSurface = "anthropic" | "apitoken" | "openai";

export type ApiErrorEntry = {
  /** Stable slug. Used as the anchor and in /e/<code> — never rename one. */
  code: string;
  status: number;
  /** The `error.type` field in the response body. */
  type: string;
  /** The `error.code` field — present on the OpenAI-compatible surface only. */
  envelopeCode?: string;
  /** Verbatim `error.message`. Where the text embeds numbers, a real example is kept. */
  message: string;
  title: string;
  /** Whether retrying the identical request can succeed. */
  retryable: boolean;
  /**
   * "anthropic" — the string is identical on api.anthropic.com and here, so the entry
   * serves every Claude API user. "apitoken" — the string exists only on this gateway.
   * "openai" — the string comes from the OpenAI-compatible surface at
   * openai.api.apitoken.sale, whose envelope is {"error":{message,type,param,code}}.
   */
  surface: ErrorSurface;
  causes: string[];
  fixes: string[];
  snippet?: { label: string; code: string };
  /** Other verbatim forms of the same failure that people paste into search. */
  alsoSearchedAs?: string[];
};

export const API_ERRORS: ApiErrorEntry[] = [
  {
    code: "invalid-api-key",
    status: 401,
    type: "authentication_error",
    message: "invalid x-api-key",
    title: "401 — invalid x-api-key",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The x-api-key header is missing or empty — often because the environment variable is unset in the shell that actually runs the process.",
      "The key is sent in the wrong header. ANTHROPIC_API_KEY becomes x-api-key; ANTHROPIC_AUTH_TOKEN becomes Authorization: Bearer. A valid key in the wrong header still returns this error.",
      "Both ANTHROPIC_API_KEY and ANTHROPIC_AUTH_TOKEN are set, so both headers go out and the request is rejected. An empty string still counts as set.",
      "The key was revoked, or expired if it was issued with an expiry date.",
      "The key is fine but the base URL points somewhere that has never seen it.",
    ],
    fixes: [
      "Print the first few characters of the variable inside the same process that fails. Most 401s are an environment or quoting problem rather than a bad key.",
      "Pick one variable and unset the other. This is the single most common cause when a custom base URL is involved.",
      "Confirm the key is active in your dashboard, and that the base URL matches the key's issuer.",
    ],
    snippet: {
      label: "Check what is actually being sent",
      code: `# Is the variable set in THIS shell?
echo "\${ANTHROPIC_API_KEY:0:12}…"
# Is a competing variable also set?
env | grep -E 'ANTHROPIC_(API_KEY|AUTH_TOKEN|BASE_URL)'

curl https://api.apitoken.sale/v1/models \\
  -H "x-api-key: $ANTHROPIC_API_KEY" \\
  -H "anthropic-version: 2023-06-01"`,
    },
    alsoSearchedAs: [
      `401 - {'type': 'error', 'error': {'type': 'authentication_error', 'message': 'invalid x-api-key'}}`,
      "litellm.AuthenticationError: AnthropicException - invalid x-api-key",
      "anthropic.AuthenticationError",
      "claude code 401 custom ANTHROPIC_BASE_URL",
      "cursor bad user api key unauthorized anthropic",
    ],
  },
  {
    code: "rate-limit",
    status: 429,
    type: "rate_limit_error",
    message:
      "This request would exceed your organization's rate limit of 80,000 input tokens per minute. Please reduce the prompt length or the maximum tokens requested, or try again later.",
    title: "429 — rate limit exceeded",
    retryable: true,
    surface: "anthropic",
    causes: [
      "The per-minute token or request ceiling was exceeded. The number in the message is your own limit, so it differs between accounts.",
      "A burst with no concurrency ceiling — a parallel map over a large list is the usual culprit.",
      "Retries piling on top of the requests that caused the first 429, which enlarges the burst instead of draining it.",
      "A single very large prompt can exceed a per-minute token budget on its own, which is why the message suggests shortening the prompt as well as waiting.",
    ],
    fixes: [
      "Honour the Retry-After header rather than guessing an interval.",
      "The official SDKs already retry 429 and 5xx with exponential backoff (twice by default) — raise max_retries instead of hand-rolling a loop.",
      "Cap concurrency at the call site. A semaphore fixes more 429s than any retry policy.",
      "Do not confuse this with a subscription usage cap — see the usage-limit entry below. They are different systems with different fixes.",
    ],
    snippet: {
      label: "Let the SDK back off for you",
      code: `import anthropic

client = anthropic.Anthropic(max_retries=5)  # retries 429 and 5xx with backoff`,
    },
    alsoSearchedAs: [
      "Number of request tokens has exceeded your per-minute rate limit",
      "anthropic.RateLimitError",
      "claude api 429 too many requests",
    ],
  },
  {
    code: "overloaded",
    status: 529,
    type: "overloaded_error",
    message: "Overloaded",
    title: "529 — Overloaded",
    retryable: true,
    surface: "anthropic",
    causes: [
      "Upstream capacity is temporarily saturated. 529 describes the service, not your request.",
      "It clusters during incidents: the same request typically succeeds minutes later with no change.",
    ],
    fixes: [
      "Retry with exponential backoff and jitter. Never in a tight loop — that is what produced the pile-up.",
      "Note the status is 529, not 503. Some HTTP clients and proxies only treat a hardcoded set of codes as retryable and omit 529, so the retry you think you have may not fire.",
      "For latency-sensitive paths, fall back to a smaller model, which is generally less contended.",
    ],
    alsoSearchedAs: [
      `API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}`,
      "anthropic api overloaded error repeated 529",
      "claude 529 vs 429",
    ],
  },
  {
    code: "credit-balance-too-low",
    status: 400,
    type: "invalid_request_error",
    message:
      "Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.",
    title: "400 — credit balance is too low",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The Anthropic organization behind the key has no API credits left. API credits are a separate wallet from a Pro or Max subscription.",
      "A Pro or Max subscriber sees this because the tool is authenticating with an API key rather than the subscription — the subscription does not fund API calls.",
      "Auto-reload is off, or the card on file was declined.",
    ],
    fixes: [
      "Check whether the failing tool is using an API key or a subscription login. This error always concerns API credits.",
      "Add credits to the organization, or enable auto-reload so long jobs do not stop mid-run.",
      "On this gateway the equivalent condition is a 402 with a different message — see the insufficient-balance entry.",
    ],
    alsoSearchedAs: [
      "claude credit balance is too low but I have credits",
      "claude pro credit balance too low",
      "your credit balance is too low to access the anthropic api",
    ],
  },
  {
    code: "usage-limit-reached",
    status: 0,
    type: "subscription usage cap (not an API error)",
    message: "Claude usage limit reached. Your limit will reset at 3pm (America/New_York)",
    title: "Claude usage limit reached — subscription cap, not an API error",
    retryable: true,
    surface: "anthropic",
    causes: [
      "This is a Claude Pro or Max subscription cap, not an HTTP error from the API. It comes from the apps and from Claude Code when signed in with a subscription.",
      "Caps are enforced on a rolling window (commonly a five-hour session window plus a weekly ceiling), so heavy days can exhaust a weekly allowance well before the week ends.",
      "It is unrelated to a 429: the API's rate limits are per-minute throughput, while this is a plan allowance.",
    ],
    fixes: [
      "Wait for the stated reset time — the message names it, and it is a rolling window rather than a calendar boundary.",
      "Reduce what each request carries. Long conversations resend their whole history every turn, so trimming or compacting context stretches an allowance considerably.",
      "If the work cannot wait for a reset, pay-as-you-go API access is billed per token instead of per plan allowance, so it has no weekly cap to exhaust. That is the honest difference: metering, not a way around an enforcement action.",
    ],
    alsoSearchedAs: [
      "Claude AI usage limit reached",
      "claude weekly limit reached",
      "claude max 20x weekly limit",
      "when does claude usage limit reset",
      "лимит claude исчерпан когда сбросится",
    ],
  },
  {
    code: "prompt-too-long",
    status: 400,
    type: "invalid_request_error",
    message: "prompt is too long: 212164 tokens > 199999 maximum",
    title: "400 — prompt is too long",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The request exceeds the model's context window. The two numbers are your prompt size and the ceiling for that model.",
      "An agent loop that appends every tool result to the history without ever pruning it.",
      "Large files or documents pasted inline rather than referenced.",
    ],
    fixes: [
      "Count before sending: the count_tokens endpoint gives an exact, model-specific number. Do not estimate with a tokenizer built for another vendor — it undercounts Claude noticeably.",
      "Prune old tool results from the history, or enable server-side compaction so earlier turns are summarized instead of resent verbatim.",
      "Upload large documents once via the Files API and reference them by file_id.",
    ],
    alsoSearchedAs: ["claude prompt is too long tokens > maximum", "claude 200k context limit error"],
  },
  {
    code: "tool-result-missing",
    status: 400,
    type: "invalid_request_error",
    message:
      "`tool_use` ids were found without `tool_result` blocks immediately after: toolu_… Each `tool_use` block must have a corresponding `tool_result` block in the next message.",
    title: "400 — tool_use without a matching tool_result",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The assistant turn requested one or more tools, and the next message did not return a result for every one of them.",
      "Only the text was appended to the history instead of the full response content, so the tool_use blocks were silently dropped.",
      "Several tools were requested in parallel and the results were split across multiple user messages instead of batched into one.",
      "A tool threw, and the code skipped sending a result rather than sending an error result.",
    ],
    fixes: [
      "Append the entire response content to the history, not just the text.",
      "Return every tool_result for a turn inside a single user message. Splitting them also trains the model to stop making parallel tool calls.",
      "On failure, still return a tool_result with is_error set — never omit it.",
    ],
    alsoSearchedAs: ["tool_use ids were found without tool_result blocks", "claude code tool_result error"],
  },
  {
    code: "temperature-and-top-p",
    status: 400,
    type: "invalid_request_error",
    message: "`temperature` and `top_p` cannot both be specified for this model. Please use only one.",
    title: "400 — temperature and top_p cannot both be specified",
    retryable: false,
    surface: "anthropic",
    causes: [
      "Both sampling parameters were sent to a Claude 4 model. Frameworks commonly set defaults for both, so the code may never have set them explicitly.",
      "On Claude Opus 4.7 and later — including Opus 4.8, Opus 5 and Fable 5 — the parameters were removed entirely, so sending either one returns a 400.",
      "On Claude Sonnet 5 a non-default value is rejected while the default is accepted, so the same code can pass on one route and fail on another.",
    ],
    fixes: [
      "On Claude 4.x, send at most one of the two.",
      "On Opus 4.7 and later, delete both, plus top_k. There is no replacement parameter — behaviour is steered by prompting and by the effort setting.",
      "If temperature=0 was there for determinism, note it never guaranteed identical output on any model.",
    ],
    snippet: {
      label: "Before and after",
      code: `# Before — 400
client.messages.create(model="claude-opus-5", temperature=0.7, top_p=0.9, …)

# After
client.messages.create(model="claude-opus-5", …)`,
    },
    alsoSearchedAs: [
      "temperature and top_p cannot both be specified",
      "claude opus temperature removed",
      "bedrock claude temperature and topP error",
    ],
  },
  {
    code: "thinking-budget-tokens",
    status: 400,
    type: "invalid_request_error",
    message: "`max_tokens` must be greater than `thinking.budget_tokens`",
    title: "400 — max_tokens must exceed thinking.budget_tokens",
    retryable: false,
    surface: "anthropic",
    causes: [
      "On models that still accept a fixed thinking budget, that budget must be strictly smaller than max_tokens, since thinking and the reply share the same output allowance.",
      "On Claude Opus 4.7 and later and on Sonnet 5 the fixed budget was removed altogether, which surfaces as a different 400 telling you to use adaptive thinking and the effort setting instead.",
    ],
    fixes: [
      "Raise max_tokens above the budget, or lower the budget.",
      "On current models, switch to adaptive thinking and control depth with output_config.effort (low, medium, high, xhigh, max). Effort goes inside output_config, not at the top level.",
      "With thinking enabled, max_tokens caps thinking plus reply together — a budget sized for the answer alone can truncate mid-response.",
    ],
    snippet: {
      label: "Current form",
      code: `thinking={"type": "adaptive"},
output_config={"effort": "high"}`,
    },
    alsoSearchedAs: [
      "max_tokens must be greater than thinking.budget_tokens",
      `"thinking.type.enabled" is not supported for this model`,
      `"thinking.type.disabled" is not supported for this model`,
      "budget_tokens removed claude",
    ],
  },
  {
    code: "prefill-not-supported",
    status: 400,
    type: "invalid_request_error",
    message: "This model does not support assistant message prefill. The conversation must end with a user message.",
    title: "400 — assistant message prefill not supported",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The conversation ends on an assistant message used to force the opening of the reply. That is rejected on Claude Opus 4.6 and later, Sonnet 4.6 and later, and Fable 5.",
      "Assistant messages elsewhere in the history — few-shot examples, for instance — are still fine. Only a trailing one is rejected.",
      "Many frameworks prefill internally, so the code may never do it explicitly.",
    ],
    fixes: [
      "To force a JSON shape, use structured outputs via output_config.format instead of prefilling an opening brace.",
      "To force a label, define a tool with an enum field listing the valid labels.",
      "To suppress a preamble, instruct it in the system prompt: respond directly, with no opening phrases.",
      "To continue an interrupted response, move the continuation into the user turn and quote where it stopped.",
    ],
    alsoSearchedAs: [
      "This model does not support assistant message prefill",
      "claude prefill trailing whitespace error",
    ],
  },
  {
    code: "max-tokens-too-large",
    status: 400,
    type: "invalid_request_error",
    message:
      "max_tokens: 128001 > 128000, which is the maximum allowed number of output tokens for claude-opus-4-6",
    title: "400 — max_tokens above the model's output ceiling",
    retryable: false,
    surface: "anthropic",
    causes: [
      "max_tokens exceeds the output ceiling for that specific model. The ceiling is per model and is not the same as the context window.",
      "A configuration written for one model reused with another that has a lower ceiling.",
    ],
    fixes: [
      "Look up the ceiling for the model you are calling rather than assuming a shared value.",
      "Above roughly 16K output tokens, stream the response — a large non-streaming request can exceed the SDK's HTTP timeout even when max_tokens is legal.",
    ],
    alsoSearchedAs: ["which is the maximum allowed number of output tokens", "claude max_tokens too large"],
  },
  {
    code: "request-too-large",
    status: 413,
    type: "request_too_large",
    message: "Request exceeds the maximum size",
    title: "413 — request too large",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The serialized body exceeds the size ceiling. Base64-encoded images and PDFs are the usual reason.",
      "Base64 inflates binary data by roughly a third, so a file that looks safe on disk can be over the limit on the wire.",
      "Requests can hit an intermediate ceiling below the documented maximum when many files are attached at once.",
    ],
    fixes: [
      "Resize or recompress images before encoding — most vision tasks do not need the original resolution.",
      "Upload large documents once via the Files API and reference them by file_id rather than resending bytes each turn.",
      "Trim the message history instead of replaying every turn verbatim.",
    ],
    alsoSearchedAs: ["claude api 413 request_too_large", "claude request exceeds the maximum size"],
  },
  {
    code: "not-found",
    status: 404,
    type: "not_found_error",
    message: "model: claude-opus-4-5-20251101",
    title: "404 — model or endpoint not found",
    retryable: false,
    surface: "anthropic",
    causes: [
      "A model id that does not exist: a typo, a date suffix appended to an alias, or an id retired in a deprecation wave.",
      "Model ids use hyphens throughout — claude-sonnet-4-6, never claude-sonnet-4.6.",
      "A base URL that already ends in /v1, so the SDK produced /v1/v1/messages.",
    ],
    fixes: [
      "Set the base URL to the origin only and let the SDK append /v1 itself.",
      "List the models the key can actually reach instead of guessing an id.",
      "Replace retired ids: Claude 3.7 Sonnet and Claude 3.5 Sonnet map to claude-sonnet-5, Claude 3.5 Haiku to claude-haiku-4-5, Claude 3 Opus to claude-opus-5.",
    ],
    snippet: {
      label: "List the models this key can use",
      code: `curl https://api.apitoken.sale/v1/models \\
  -H "x-api-key: $ANTHROPIC_API_KEY" \\
  -H "anthropic-version: 2023-06-01"`,
    },
    alsoSearchedAs: [
      "claude api 404 not_found_error model",
      "cursor model not found anthropic api key",
      "claude-3-5-sonnet 404",
    ],
  },
  {
    code: "permission-denied",
    status: 403,
    type: "permission_error",
    message: "Your API key does not have permission to use the specified resource.",
    title: "403 — permission denied",
    retryable: false,
    surface: "anthropic",
    causes: [
      "The key is valid but not entitled to the model or feature requested.",
      "A regional restriction. This variant often arrives with a terser body such as 'Request not allowed' and is about where the request originates, not about the key.",
      "On the Anthropic API a billing problem can also surface as 403, distinguished by the error type rather than the status.",
    ],
    fixes: [
      "Branch on error.type, not on the status alone — it separates a permission problem from a billing one.",
      "Try a model you know the key can reach, to establish whether the key itself is healthy.",
      "For a regional block, the fix is where the request egresses from, not the key. This gateway accepts requests from regions where the upstream API is not directly reachable.",
    ],
    alsoSearchedAs: ["anthropic 403 Request not allowed", "claude api 403 forbidden country"],
  },
  {
    code: "streaming-required",
    status: 400,
    type: "invalid_request_error",
    message: "Streaming is strongly recommended for operations that may take longer than 10 minutes",
    title: "Streaming required for long operations",
    retryable: false,
    surface: "anthropic",
    causes: [
      "A non-streaming request was made with a max_tokens large enough that the response could exceed the request timeout.",
      "It shows up most in no-code and workflow tools, where the node sets a large max_tokens but does not expose a streaming toggle.",
    ],
    fixes: [
      "Stream the request and collect the final message from the stream helper.",
      "If streaming is not available in your tool, lower max_tokens to something the timeout can accommodate — roughly 16K output tokens is a safe non-streaming ceiling.",
    ],
    snippet: {
      label: "Stream and take the final message",
      code: `with client.messages.stream(model="claude-opus-5", max_tokens=64000, …) as stream:
    message = stream.get_final_message()`,
    },
    alsoSearchedAs: ["Streaming is required for operations that may take longer than 10 minutes"],
  },
  {
    code: "insufficient-balance",
    status: 402,
    type: "invalid_request_error",
    message: "insufficient balance or key spending limit reached for this request",
    title: "402 — insufficient balance or key spending limit reached",
    retryable: false,
    surface: "apitoken",
    causes: [
      "The prepaid balance does not cover the request just submitted.",
      "The key carries its own spending limit and has reached it, even though the account still has balance.",
      "A large max_tokens reserves a correspondingly large hold up front, so a request can be refused while the balance still looks non-zero. The unused part of the hold is released when the request settles.",
    ],
    fixes: [
      "Top up the balance, or raise the spending limit on that key.",
      "Lower max_tokens to what the response actually needs — the hold is sized from max_tokens, not from the tokens finally used.",
      "Read the live balance with the same key you use for inference.",
    ],
    snippet: {
      label: "Check the balance for a key",
      code: `curl https://api.apitoken.sale/balance \\
  -H "x-api-key: $ANTHROPIC_API_KEY"`,
    },
    alsoSearchedAs: ["claude api 402", "api key spending limit reached"],
  },
  {
    code: "invalid-beta-header",
    status: 400,
    type: "invalid_request_error",
    message: "invalid anthropic-beta header",
    title: "400 — invalid anthropic-beta header",
    retryable: false,
    surface: "apitoken",
    causes: [
      "The anthropic-beta header carries a flag this gateway does not accept, or the value is malformed.",
      "Several flags joined by something other than a comma.",
      "A flag copied from documentation for a feature that has since gone GA and no longer takes a header.",
    ],
    fixes: [
      "Send multiple flags as one comma-separated value.",
      "Drop flags for features that are now GA — effort, fine-grained tool streaming and the 128K output header among them.",
      "If the SDK sets the header for you, do not also set it by hand.",
    ],
    alsoSearchedAs: ["anthropic-beta header error"],
  },
  {
    code: "invalid-request-body",
    status: 400,
    type: "invalid_request_error",
    message: "Could not parse request body.",
    title: "400 — could not parse request body",
    retryable: false,
    surface: "apitoken",
    causes: [
      "The body is not valid JSON — a trailing comma, a single-quoted string, or a shell variable that expanded into an unescaped quote.",
      "A missing or non-JSON Content-Type header.",
    ],
    fixes: [
      "Validate the body before sending. Most of these never reach the API in a working state.",
      "In shell scripts, build the body with a heredoc or jq rather than string interpolation.",
    ],
    alsoSearchedAs: ["claude api could not parse request body"],
  },
  {
    code: "api-error",
    status: 500,
    type: "api_error",
    message: "Internal server error",
    title: "500 — internal server error",
    retryable: true,
    surface: "anthropic",
    causes: ["An unexpected failure while processing the request. Nothing in your payload caused it."],
    fixes: [
      "Retry with exponential backoff — the SDKs do this for 5xx automatically.",
      "If it persists for one request while others succeed, capture the request id and send it to support.",
    ],
    alsoSearchedAs: ["anthropic api_error internal server error"],
  },
  // ——— OpenAI-compatible surface (openai.api.apitoken.sale) ———
  // Envelope: {"error":{"message","type","param","code"}}. Verbatim strings verified
  // against the gateway (crates/forward/src/codex/api.rs) — do not paraphrase.
  {
    code: "openai-invalid-api-key",
    status: 401,
    type: "invalid_request_error",
    envelopeCode: "invalid_api_key",
    message: "Incorrect API key provided.",
    title: "401 — Incorrect API key provided",
    retryable: false,
    surface: "openai",
    causes: [
      "The key was sent in the x-api-key header. The OpenAI-compatible endpoint authenticates with Authorization: Bearer — x-api-key is only for the Anthropic surface.",
      "The Authorization header is missing the Bearer prefix, or the environment variable it was built from is empty in the shell that runs the process.",
      "The key was revoked, or expired if it was issued with an expiry date.",
      "The key is valid but the base URL points at the Anthropic surface (api.apitoken.sale) instead of openai.api.apitoken.sale/v1.",
    ],
    fixes: [
      "Send the same sk-pool key as Authorization: Bearer sk-pool-… to https://openai.api.apitoken.sale/v1.",
      "With the official OpenAI SDK, set api_key (or OPENAI_API_KEY) and base_url — the SDK adds the Bearer header for you.",
      "Confirm the key is active in your dashboard and that the host is the OpenAI-compatible one.",
    ],
    snippet: {
      label: "Reproduce outside your tool",
      code: `curl https://openai.api.apitoken.sale/v1/models \\
  -H "Authorization: Bearer $APITOKEN_API_KEY"`,
    },
    alsoSearchedAs: [
      `{"error":{"message":"Incorrect API key provided.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}`,
      "openai.AuthenticationError",
      "codex stream error: unexpected status 401",
    ],
  },
  {
    code: "openai-insufficient-quota",
    status: 402,
    type: "insufficient_quota",
    envelopeCode: "insufficient_quota",
    message: "Your account balance is insufficient for this request.",
    title: "402 — account balance is insufficient",
    retryable: false,
    surface: "openai",
    causes: [
      "The prepaid balance shared by both API surfaces is too low to cover the request's reservation.",
      "A large max output or a long conversation raises the reservation above the remaining balance even when previous calls succeeded.",
    ],
    fixes: [
      "Top up any whole-dollar amount and retry after the payment is credited. Backoff alone never resolves a 402.",
      "Lower max output tokens or trim the conversation so the reservation fits the current balance.",
    ],
    alsoSearchedAs: [
      "openai insufficient_quota",
      "codex 402 insufficient balance",
    ],
  },
  {
    code: "openai-model-not-found",
    status: 404,
    type: "invalid_request_error",
    envelopeCode: "model_not_found",
    message: `The model "gpt-9.9" does not exist or you do not have access to it.`,
    title: "404 — model does not exist",
    retryable: false,
    surface: "openai",
    causes: [
      "The model ID is misspelled or belongs to the other surface: Claude IDs (claude-*) only exist on the Anthropic endpoint, GPT IDs (gpt-*) only on the OpenAI-compatible endpoint.",
      "The model is not in the currently enabled catalog — the served set changes as models are admitted.",
    ],
    fixes: [
      "List the models your key can actually use: GET https://openai.api.apitoken.sale/v1/models with Authorization: Bearer.",
      "Check the ID character for character — gpt-5.6-sol, not gpt5.6 or gpt-5.6.sol. gpt-5.6 is a valid alias of gpt-5.6-sol.",
    ],
    snippet: {
      label: "Discover the enabled models",
      code: `curl https://openai.api.apitoken.sale/v1/models \\
  -H "Authorization: Bearer $APITOKEN_API_KEY"`,
    },
    alsoSearchedAs: [
      "openai model_not_found",
      "codex stream error: unexpected status 404",
      "The model does not exist or you do not have access to it",
    ],
  },
  {
    code: "openai-rate-limit",
    status: 429,
    type: "rate_limit_error",
    envelopeCode: "rate_limit_exceeded",
    message: "Rate limit reached. Please retry shortly.",
    title: "429 — rate limit reached",
    retryable: true,
    surface: "openai",
    causes: [
      "The account's concurrency or rate ceiling was exceeded — a parallel burst with no cap is the usual cause.",
      "Retries piling on top of the requests that caused the first 429 enlarge the burst instead of draining it.",
    ],
    fixes: [
      "Honor the Retry-After header — the response carries one.",
      "Retry with capped exponential backoff and jitter, and cap concurrency at the call site.",
    ],
    alsoSearchedAs: [
      "openai rate_limit_error",
      "codex stream error: unexpected status 429",
    ],
  },
  {
    code: "openai-service-unavailable",
    status: 503,
    type: "server_error",
    envelopeCode: "service_unavailable",
    message: "The requested model is temporarily unavailable. Please retry.",
    title: "503 — model temporarily unavailable",
    retryable: true,
    surface: "openai",
    causes: [
      "Upstream capacity for the requested model is temporarily saturated. 503 describes the service, not your request.",
      "It clusters during incidents: the same request typically succeeds minutes later with no change.",
    ],
    fixes: [
      "Retry with exponential backoff and jitter — the response carries a Retry-After hint.",
      "For latency-sensitive paths, fall back to another enabled model tier, which is generally less contended.",
    ],
    alsoSearchedAs: ["openai service_unavailable server_error"],
  },
];

export const ERROR_CODES: string[] = API_ERRORS.map((entry) => entry.code);

export function findApiError(code: string): ApiErrorEntry | undefined {
  return API_ERRORS.find((entry) => entry.code === code);
}

// --- Localization -----------------------------------------------------------
//
// Only the explanatory prose is translated. `message`, `type` and `alsoSearchedAs`
// are API responses and tool output, so they stay in English in every locale —
// a translated error string would match nothing when someone pastes what their
// terminal actually printed, which is the entire point of this page.

export type ErrorLocale = "en" | "ru";

export type LocalizedError = {
  title: string;
  causes: string[];
  fixes: string[];
  snippetLabel?: string;
};

export type ResolvedApiError = ApiErrorEntry & { localeTitle: string };

export const errorsUi: Record<ErrorLocale, {
  eyebrow: string;
  title: string;
  description: string;
  envelopeIntro: string;
  envelopeNote: string;
  allCodes: string;
  colStatus: string;
  colType: string;
  colMeaning: string;
  colRetry: string;
  retryYes: string;
  retryNo: string;
  why: string;
  how: string;
  variants: string;
  shortLink: string;
  originGateway: string;
  originShared: string;
  originSubscription: string;
  originOpenAi: string;
  openAiHeading: string;
  openAiIntro: string;
  stuckHeading: string;
  stuckBody: string;
  ctaDocs: string;
  ctaSupport: string;
  seeAlso: string;
}> = {
  en: {
    eyebrow: "Reference",
    title: "API Error Codes — Claude & OpenAI-compatible",
    description:
      "Every API error explained: 401 invalid x-api-key, 429 rate_limit_error, 529 Overloaded and 413 request_too_large on the Anthropic surface, plus 401 invalid_api_key, 402 insufficient_quota and 404 model_not_found on the OpenAI-compatible surface. Exact response text, cause and fix for each.",
    envelopeIntro:
      "Every error on the Anthropic surface is returned as JSON with the same envelope, so you can branch on error.type without parsing the message text:",
    envelopeNote:
      "Match on the HTTP status and error.type, never on the message string — messages are prose and can be reworded, while the type is a contract. In the official SDKs this means catching the typed exception classes rather than inspecting text. This page is written the other way round only because the message is what you have in front of you when something breaks.",
    allCodes: "Anthropic surface — all codes",
    colStatus: "Status",
    colType: "error.type",
    colMeaning: "Meaning",
    colRetry: "Retry?",
    retryYes: "Yes, back off",
    retryNo: "No",
    why: "Why it happens",
    how: "How to fix it",
    variants: "Other forms of the same failure",
    shortLink: "Short link",
    originGateway: "This response is specific to this gateway — the Anthropic API has no equivalent.",
    originShared: "Identical on api.anthropic.com and on this gateway.",
    originSubscription: "Comes from Anthropic's own apps and subscription plans, not from this gateway.",
    originOpenAi: "Returned by the OpenAI-compatible endpoint at openai.api.apitoken.sale.",
    openAiHeading: "OpenAI-compatible surface — all codes",
    openAiIntro:
      "The OpenAI-compatible endpoint returns the OpenAI error envelope instead — branch on error.code and the HTTP status. These are the exact responses of openai.api.apitoken.sale:",
    stuckHeading: "Still stuck?",
    stuckBody:
      "If a request fails in a way this page does not cover, send us the endpoint, the masked key id, the HTTP status and the response body. Never send the full key.",
    ctaDocs: "API docs",
    ctaSupport: "Contact support",
    seeAlso: "See also the rate limits guide and how to point an SDK at a custom base URL.",
  },
  ru: {
    eyebrow: "Справочник",
    title: "Коды ошибок API — Claude и OpenAI-совместимый",
    description:
      "Разбор всех ошибок API: 401 invalid x-api-key, 429 rate_limit_error, 529 Overloaded и 413 request_too_large на Anthropic-поверхности, плюс 401 invalid_api_key, 402 insufficient_quota и 404 model_not_found на OpenAI-совместимой. Точный текст ответа, причина и решение для каждой.",
    envelopeIntro:
      "Любая ошибка на Anthropic-поверхности возвращается в JSON с одинаковым конвертом, поэтому ветвиться можно по error.type, не разбирая текст сообщения:",
    envelopeNote:
      "Сопоставляйте HTTP-статус и error.type, но никогда не текст сообщения: сообщение — это проза, его могут переформулировать, а тип — это контракт. В официальных SDK это означает ловить типизированные классы исключений, а не искать подстроки. Эта страница построена наоборот только потому, что в момент поломки перед глазами у вас именно сообщение.",
    allCodes: "Anthropic-поверхность — все коды",
    colStatus: "Статус",
    colType: "error.type",
    colMeaning: "Что означает",
    colRetry: "Ретрай?",
    retryYes: "Да, с задержкой",
    retryNo: "Нет",
    why: "Почему возникает",
    how: "Что делать",
    variants: "Другие формы той же ошибки",
    shortLink: "Короткая ссылка",
    originGateway: "Такой ответ есть только у этого шлюза — в Anthropic API аналога нет.",
    originShared: "Идентично на api.anthropic.com и на этом шлюзе.",
    originSubscription: "Приходит из приложений и подписок Anthropic, а не от этого шлюза.",
    originOpenAi: "Возвращается OpenAI-совместимым эндпоинтом openai.api.apitoken.sale.",
    openAiHeading: "OpenAI-совместимая поверхность — все коды",
    openAiIntro:
      "OpenAI-совместимый эндпоинт возвращает конверт ошибок OpenAI — ветвитесь по error.code и HTTP-статусу. Это точные ответы openai.api.apitoken.sale:",
    stuckHeading: "Не помогло?",
    stuckBody:
      "Если запрос падает так, как здесь не описано, пришлите нам эндпоинт, маскированный идентификатор ключа, HTTP-статус и тело ответа. Полный ключ присылать не нужно никогда.",
    ctaDocs: "Документация API",
    ctaSupport: "Написать в поддержку",
    seeAlso: "Смотрите также гайд про лимиты и как направить SDK на свой base URL.",
  },
};

/** Merge the shared catalog entry with the translation for `locale`. */
export function resolveApiErrors(
  locale: ErrorLocale,
  translations: Record<string, LocalizedError>,
): ResolvedApiError[] {
  if (locale === "en") return API_ERRORS.map((entry) => ({ ...entry, localeTitle: entry.title }));

  return API_ERRORS.map((entry) => {
    const translated = translations[entry.code];
    // Fall back to English rather than dropping the entry: a missing translation
    // should degrade to a usable page, not to a hole in the reference.
    if (!translated) return { ...entry, localeTitle: entry.title };
    return {
      ...entry,
      localeTitle: translated.title,
      causes: translated.causes,
      fixes: translated.fixes,
      snippet: entry.snippet
        ? { ...entry.snippet, label: translated.snippetLabel ?? entry.snippet.label }
        : undefined,
    };
  });
}

/** Entries whose verbatim string exists only on this gateway. */
export function gatewayOnlyErrors(): ApiErrorEntry[] {
  return API_ERRORS.filter((entry) => entry.surface === "apitoken");
}
