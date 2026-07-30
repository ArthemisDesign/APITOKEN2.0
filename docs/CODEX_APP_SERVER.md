# Codex app-server: OpenAI-compatible text transport

## Scope and compatibility boundary

The optional Codex provider runs the official OpenAI `codex app-server` as a supervised local
JSON-RPC child. Its public base URL is `https://openai.api.apitoken.sale/v1`; the existing
`https://api.apitoken.sale` hostname remains exclusively Anthropic-compatible. It exposes a
lenient, SDK-compatible text+image subset:

| Public route | Status |
|---|---|
| `POST /v1/responses` | supported, streaming and non-streaming |
| `GET /v1/responses/{id}` | supported for `store=true` responses within the history TTL |
| `DELETE /v1/responses/{id}` | supported; deletes the stored response |
| `GET /v1/responses/{id}/input_items` | supported; returns the stored request items |
| `POST /v1/responses/input_tokens` | supported; estimates input tokens without running a turn |
| `POST /v1/chat/completions` | supported adapter, streaming and non-streaming |
| `GET /v1/models` | supported; intersected with the live app-server catalog |
| `GET /v1/models/{model}` | supported |

User messages may carry images: Chat Completions `image_url` parts and Responses `input_image`
parts accept inline `data:image/…` URLs and remote `http(s)://` URLs, including in replayed
history. Video, audio, embeddings, batches, files, assistants, fine-tuning, WebSocket/realtime,
`/v1/responses/compact` and administrative OpenAI Platform endpoints are not implemented and
return an OpenAI-shaped `404`, as does every other route on the OpenAI-compatible hostname —
nothing is ever forwarded to Anthropic from it. Only a request sent to that hostname enters this
provider; authentication headers never select a provider.
Parameters that app-server cannot enforce are accepted and ignored instead of rejected, so stock
SDKs and agent terminals never fail on defaults they send. This covers sampling/output controls
(`temperature`, `top_p`, penalties, logprobs, `seed`, multi-choice output), `store`,
`stream_options`, forced `tool_choice` values (degrade to `"auto"`),
`parallel_tool_calls=false`, `strict=true` tools (degrade to non-strict), unknown `include`
values, reasoning efforts the model does not advertise (degrade to the model default), message
`name` hints, assistant `refusal`/`audio` history fields and any unknown present or future
fields. Two output controls are enforced on the delivered text rather than upstream: `stop`
sequences (the stream is cut at the sequence, which is never emitted) and
`max_tokens`/`max_completion_tokens` (approximate cap at ~4 characters per token; a truncated
answer finishes with `finish_reason="length"`). Settlement always uses exact upstream usage
regardless of client-side cuts. Requests that are structurally unusable — missing model/input,
empty messages, malformed tool history, invalid image URLs — still return OpenAI-shaped `400`
errors.

Fast is a service tier on the existing GPT model IDs, not a separate `-fast` model family.
Requests may send `service_tier: "priority"` (the OpenAI-compatible wire value) or
`service_tier: "fast"` (the Codex configuration spelling). For a catalog model that supports Fast,
the gateway normalizes either value to `priority`, sends it to app-server's `thread/start`, verifies
that the thread accepted it, and reports `service_tier: "priority"` in the response. Standard,
`auto`, `flex`, unknown and malformed values retain lenient compatibility and run at the default
tier. The gateway sends the app-server's explicit `"default"` sentinel for default-tier requests
and verifies the acknowledgement, so a profile-local Codex Fast setting cannot silently upgrade
customer traffic.

Streaming is spec-complete for agent terminals: Chat Completions streams reasoning summaries as
`reasoning_content` deltas (and joins them into `message.reasoning_content` for non-streaming
calls); Responses streams emit the full `response.*` lifecycle ending in `response.completed` or
`response.failed`; both transports send data-bearing SSE progress every 15 s during long reasoning
stretches, because EventSource clients discard comments before applying idle timers. Legacy Chat
Completions `functions`/`function_call` parameters are translated to the
modern `tools`/`tool_choice` surface. Non-streaming responses carry `x-ratelimit-limit/remaining/
reset-tokens` headers derived from the provider window (percent basis). Stored responses are
kept for the history TTL (default 24 h) bound to the owning tenant; `store=false` responses are
never persisted and therefore not retrievable.

This is not the OpenAI Platform API and must not be represented as an OpenAI-operated endpoint.
ChatGPT subscriptions and OpenAI Platform API billing are separate products. Confirm that the
applicable subscription terms permit the intended commercial workload before customer-facing use.

## Customer connection

The same `sk-pool-…` key and prepaid balance work on both public hosts, but the wire protocols and
authentication headers are different. OpenAI-compatible requests use a bearer token:

```bash
export APITOKEN_API_KEY='sk-pool-…'

curl https://openai.api.apitoken.sale/v1/responses \
  -H "Authorization: Bearer $APITOKEN_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-5.6-sol",
    "service_tier": "priority",
    "input": "Reply with exactly: connected"
  }'
```

Clients must discover the current model intersection from `GET /v1/models`; they must not assume
that every OpenAI model name is available. An official OpenAI SDK needs only its key and base URL
changed:

```python
import os
from openai import OpenAI

client = OpenAI(
    api_key=os.environ["APITOKEN_API_KEY"],
    base_url="https://openai.api.apitoken.sale/v1",
)
print(client.responses.create(
    model="gpt-5.6-sol",
    service_tier="priority",
    input="Reply with exactly: connected",
).output_text)
```

Current Codex CLI releases can use a named overlay without replacing the user's normal login or
default configuration. Create `~/.codex/apitoken.config.toml`:

```toml
model = "gpt-5.6-sol"
model_provider = "apitoken"

[model_providers.apitoken]
name = "apiToken.sale"
base_url = "https://openai.api.apitoken.sale/v1"
wire_api = "responses"
env_key = "APITOKEN_API_KEY"
```

Then keep the key in the environment and select the profile explicitly:

```bash
export APITOKEN_API_KEY='sk-pool-…'
codex --profile apitoken
```

The gateway accepts the current CLI's Responses compatibility fields: bounded
`client_metadata`, `prompt_cache_key`, `reasoning.context="all_turns"`, `text.verbosity`, and the
developer `additional_tools` input item. Function tools, function namespaces and Lark-grammar
custom tools are translated into request-local app-server dynamic tools. Custom tool calls are
returned to Codex, which executes them on the customer's machine and submits their output in the
next request; the gateway never executes a customer's `exec` source. Codex CLI 0.146's
client-executed `tool_search` wire type is bridged through an equivalent private dynamic function
for the pinned app-server, then translated back to `tool_search_call`; the following
`tool_search_output` is replayed through the same boundary without changing the stored public
history. Deferred function tools preserve `defer_loading`. Codex catalog-refresh requests receive
the CLI-native empty `models` overlay and keep using that CLI version's bundled metadata; ordinary
OpenAI clients continue to receive the standard `object: "list"` catalog.

Chat Completions accepts the equivalent top-level `reasoning_effort`, `verbosity`, and
`prompt_cache_key` controls and translates them to the same app-server turn settings.

opencode works through the Chat Completions surface. Its provider models are not in the models.dev
catalog, so opencode assigns a config-defined model text-only default capabilities and silently
replaces attached images with an in-band "this model does not support image input" note — the
gateway never receives the image. Declare the image modality explicitly per model in
`opencode.json` (and restart opencode afterwards):

```json
{
  "provider": {
    "apitoken": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "apiToken.sale",
      "options": {
        "baseURL": "https://openai.api.apitoken.sale/v1",
        "apiKey": "{env:APITOKEN_API_KEY}"
      },
      "models": {
        "gpt-5.6-sol": {
          "name": "GPT-5.6 Sol",
          "attachment": true,
          "modalities": {
            "input": ["text", "image"],
            "output": ["text"]
          },
          "limit": { "context": 272000, "output": 32000 }
        }
      }
    }
  }
}
```

Once `"image"` is in `modalities.input`, pasted and attached images are sent as standard
Chat Completions `image_url` parts with inline `data:` URLs, which the gateway accepts (see the
image rules above). The same declaration is what any other capability-gated terminal needs; the
wire contract itself is plain Chat Completions/Responses image parts, so ungated clients (OpenAI
SDKs, Codex CLI) need nothing extra.

Diagnostic `client_metadata` and `safety_identifier` values are validated and discarded at the
public boundary. They are never logged or forwarded to the pooled account. `prompt_cache_key` is
validated and echoed, then projected into the affinity lineage. The value sent through the patched
official app-server is a stable keyed digest scoped to the metered tenant, never the customer's raw
identifier. When the client omits it, the gateway derives the same opaque key from the shared
system/tools cache root or the resolved conversation lineage. This is necessary because every
public HTTP request uses an ephemeral Codex thread; stock app-server would otherwise route every
request under a new thread UUID and defeat cross-request OpenAI prompt-cache hits.

## Architecture and trust boundary

```text
OpenAI SDK client
  -> Rust HTTP/auth/admission/billing layer
  -> exact history + request translation
  -> pinned, patched official codex app-server over stdio JSON-RPC
  -> OpenAI services through the app-server's supported ChatGPT login
```

The transport never reads or replays ChatGPT bearer tokens. Authentication remains owned by the
official Codex binary in its own dedicated `CODEX_HOME`. Every configured home runs one supervised
child of the same attested binary; homes share nothing else, and are identified everywhere by their
configured index so no path or account identity reaches a log or a metric label. The Rust child
launcher:

- verifies the exact executable SHA-256 and exact `codex --version` before use;
- starts with an empty inherited environment and adds only fixed runtime values plus an allowlist of
  standard proxy variables;
- uses an empty dedicated work directory and disables project docs, MCP servers, plugins, skills,
  apps, collaboration, multi-agent helpers, environment-context injection and permission tools;
- requires `account/read` to report `chatgpt`; API-key auth is rejected;
- redacts child stderr and never logs account identity, credentials, prompts or response bodies.

When the provider is enabled, server startup must complete that binary attestation, app-server
initialization and `account/read` subscription check before the slot can become ready. Every
transport has a one-home service floor: a single working subscription remains routable instead of
becoming a synthetic 503 because no spare account exists. Slots also attempt an initial rate-limit
snapshot without making that observability endpoint a hard availability dependency. A failed Codex
activation therefore cannot be promoted while the previous slot is still healthy. Requiring every
discovered home at startup would let one expired device login block the service, so a failing extra
home starts quarantined and is reported by `CodexHomeUnauthenticated` instead.

OpenAI blue-green admission is observational and capacity-preserving like the Claude cutover: every
live HTTP generation must expose the exact same opaque authenticated-home set before the old slot is
drained. Parity is a readiness condition with no minimum soak interval: the first complete snapshot
admits the candidate. One equal home is valid; a candidate subset of the old pool is not. The gate
also fences the exact gateway process generations, sends no transport signal, takes no daemon
lifecycle lock and performs no repair. It observes authenticated clients attached to the actually
serving persistent daemons, so a safe 0.N-1 -> 0.N app-server roll is not a prerequisite for the
HTTP cutover. A separate desired-topology or pin change may ask Rust gateways to rediscover sockets,
but the signal is restricted to each unit's `MainPID`; signalling the whole cgroup would also kill
the authenticated proxy children. Individual daemon rolls remain sequential and preserve their
running turns, while their total duration cannot fail or roll back an otherwise healthy deployment.

A background health loop re-reads `account/read` and the rate-limit snapshot for every home on
`CLAUDE_API_CODEX_HEALTH_INTERVAL_SECS`. A device login expires with no traffic on it, so without
that sweep a dead home would stay silently unusable until a customer request selected it. The same
loop lets a re-authenticated home rejoin the rotation without an engine restart.

Each API request uses an ephemeral app-server thread. Public continuity is reconstructed from the
client's exact input or a tenant-bound encrypted `previous_response_id` history record. A response
ID from one billed account cannot be replayed by another.

## Removing Codex's injected prompt

The pinned patch is
[`tools/codex-app-server/0001-api-compat-dynamic-tools-only.patch`](../tools/codex-app-server/0001-api-compat-dynamic-tools-only.patch).
It makes the model-visible context contain only:

1. the client's explicit system/developer instructions;
2. the client's conversation items;
3. the client's declared function, namespaced function and custom grammar tools.

Codex personality text, environment context, project instructions, plugin/skill descriptions,
collaboration instructions, permission prompts and built-in Codex tools are excluded. Empty
`baseInstructions` is passed explicitly when the client supplies no base/system instruction, so
the model-family default prompt is not restored. Responses `instructions` and Chat `system`
messages otherwise replace that base with the client's exact text; Chat `developer` messages remain
explicit developer context. The official source test proves that an empty base instruction omits
the upstream `instructions` field; patch-specific tests prove that initial context and tools are
limited to the three sources above.

This removes client-side Codex/app-server prompt injection. It cannot and does not claim to remove
provider-side model behavior that OpenAI may apply after the request leaves the official client.
Raw chain-of-thought is never returned. Public reasoning contains summary events only; encrypted
reasoning state is returned only when a Responses request explicitly asks for
`include: ["reasoning.encrypted_content"]`.

## Pinned reproducible build

The source pin and all expected digests live in
[`tools/codex-app-server/UPSTREAM.pin`](../tools/codex-app-server/UPSTREAM.pin). The builder:

1. fetches only tag `rust-v0.145.0`;
2. verifies commit `25af12f7e61572b0bc18ddb1008be543b91519b0`;
3. verifies the upstream lockfile and local patch digests;
4. applies the patch with `git apply --check`;
5. runs the patch-specific core and app-server library tests, including custom-tool preservation
   and validation, plus the official app-server request-capture test proving that empty instruction
   overrides omit the upstream `instructions` field;
6. runs a locked release build;
7. installs an immutable, content-addressed binary and atomically updates `codex`.

Production layout:

```text
/srv/claude-api/data/codex/bin   root-owned, 0755 directory, 0555 versioned binaries
/srv/claude-api/data/codex/home  deploy-owned, 0700, authentication state
/srv/claude-api/data/codex/work  deploy-owned, 0700, empty working directory
```

For a standalone/bootstrap audit, build without touching the running engine:

```bash
sudo /opt/apitoken/repo/tools/codex-app-server/build-pinned.sh \
  --install-dir /srv/claude-api/data/codex/bin
```

Record the emitted `CODEX_BINARY_SHA256`; enabling the provider without the matching digest is a
startup error.

Normal production delivery is automatic. A change under `tools/codex-app-server/` selects a
dedicated candidate lane that runs this complete pinned build and its patch tests as
`apitoken-ci`. The marker binds the resulting regular executable, source commit, version and
SHA-256 to the exact candidate. While holding the normal deploy lock, the fixed root helper copies
only that tested executable to a content-addressed path, atomically updates the three Codex lines in
`config.env` without reading it into logs, and advances the convenience `codex` symlink. A new
engine slot therefore starts with one immutable path and its matching digest; no second build and no
manual SSH deployment are involved.

## Authentication

Authentication is an explicit operator step performed as the same unprivileged user that runs the
engine. Never copy, print, inspect, archive or put the Codex auth store in the repository:

```bash
sudo -u deploy env -i \
  HOME=/srv/claude-api/data/codex/home \
  CODEX_HOME=/srv/claude-api/data/codex/home \
  PATH=/srv/claude-api/data/codex/bin:/usr/bin:/bin \
  /srv/claude-api/data/codex/bin/codex login --device-auth
```

The operator completes the device flow in a browser. The gateway performs a read-only
`account/read` check at process startup and refuses any account type other than `chatgpt`.

## Configuration

Configuration is read only by `crates/server`; `crates/forward` receives a typed `CodexConfig`.
The provider is fail-closed and off by default:

```dotenv
CLAUDE_API_CODEX_ENABLED=1
CLAUDE_API_CODEX_BIN=/srv/claude-api/data/codex/bin/codex-<source-commit>-<sha256>
CLAUDE_API_CODEX_BIN_SHA256=<sha256 emitted by build-pinned.sh>
CLAUDE_API_CODEX_VERSION=codex-cli 0.145.0
CLAUDE_API_CODEX_HOMES=/srv/claude-api/data/codex/home,/srv/claude-api/data/codex/home2
CLAUDE_API_CODEX_WORK_DIR=/srv/claude-api/data/codex/work
CLAUDE_API_CODEX_MODELS=gpt-5.6,gpt-5.6-sol,gpt-5.6-terra,gpt-5.6-luna,gpt-5.5,gpt-5.4
CLAUDE_API_CODEX_MAX_CONCURRENT=4
```

`CLAUDE_API_CODEX_HOMES` is the pool form and lists each authenticated profile in rotation order;
`CLAUDE_API_CODEX_HOME` remains the single-home spelling, so an existing environment keeps working
unchanged. Listing one directory twice is a startup error: two children sharing one auth store would
corrupt its token refresh and would double-count one subscription's capacity. `MAX_CONCURRENT` is
retained only for environment compatibility. A pinned app-server accepts one model turn at a time;
when every purchased home is sampling, additional turns wait for the first idle home instead of
receiving a local concurrency `429` or timing out an incompatible parallel `thread/start`. Codex
does not use the Claude provider's global admission semaphore or the commercial per-key in-flight
limiter. Upstream subscription exhaustion, authentication, billing authorization and bounded
transport deadlines remain real provider/safety boundaries. Provision each additional home exactly
like the first — `0700`, owned by the engine user — and authenticate it separately with the device
flow above.

Home selection is cache-first, mirroring the Claude fleet's affinity layer: a conversation is pinned
to the home that first served it (via the shared `AffinityStore`, keyed by the same tenant scope and
projected onto the same canonical shape through `infer_codex`), so a follow-up request reuses that
home's warm OpenAI prompt cache instead of being spread by load. A shared system/tools cache root
immediately seeds two competitive homes when the pool has them; after that, a warm home is preferred
only while its remaining calibrated-or-prior USD capacity is at least 70% of the fleet leader.
Otherwise placement uses the global capacity leader immediately. This is neither a timer nor a
readiness quorum: one working home serves normally, and no background repair process is involved.
OpenAI root warmth uses
the provider's 30-minute default retention while Anthropic retains its own 5m/1h cache-control TTLs.
Affinity is a fail-open optimization — local L1 plus the optional shared Redis L2 — and is a no-op
while the pool holds a single home. Fixed provider processes derive separate affinity keys from the
shared secret, so Anthropic and OpenAI session aliases cannot overwrite each other's Redis placement.
Candidates tied on remaining capacity and in-flight turns use an atomic rotating discovery-order
cursor. This spreads both sequential traffic and simultaneous selectors instead of herding every
equal snapshot onto the first configured subscription.
The selected lineage also supplies the upstream `prompt_cache_key`, so cache placement and
subscription placement cannot diverge. On spill after an account limit/auth failure, the same key
is retained while the affinity binding is atomically rebound to the home that completed the turn.

The gateway takes one advisory lock for the complete pool at
`/run/apitoken/codex-home.lock`. `systemd/apitoken-tmpfiles.conf` creates that file under a
root-owned, non-writable parent before any provider starts. A per-home lock is insufficient because
two concurrently starting processes could each win a different profile, and replacing a home could
replace the lock inode. Home directory identity is checked separately; replacement or proxy changes
retire the old child after its active turns finish, then publish the new generation.

Optional controls:

```dotenv
CLAUDE_API_CODEX_ADMIT_BELOW_USED_PERCENT=100
CLAUDE_API_CODEX_WINDOW_CAP_USD=1500
CLAUDE_API_CODEX_HEALTH_INTERVAL_SECS=300
CLAUDE_API_CODEX_STARTUP_TIMEOUT_MS=20000
CLAUDE_API_CODEX_RPC_TIMEOUT_MS=15000
CLAUDE_API_CODEX_TURN_TIMEOUT_MS=600000
CLAUDE_API_CODEX_RESERVE_OVERHEAD_TOKENS=16384
CLAUDE_API_CODEX_HISTORY_TTL_SECS=86400
CLAUDE_API_CODEX_HISTORY_LOCAL_CAP=10000
CLAUDE_API_CODEX_HISTORY_REDIS_TIMEOUT_MS=1000
```

When `CLAUDE_API_REDIS_URL` enables shared response history, Codex reuses
`CLAUDE_API_AFFINITY_SECRET` to encrypt and authenticate tenant-bound history records. Startup
fails closed if Redis history is enabled without that secret; changing it invalidates previously
issued `previous_response_id` values.

Only standard `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` variables, including their
lower-case spellings, may reach the child. No custom TLS fingerprinting, user-agent spoofing or
private endpoint replay is used. A proxy therefore changes network egress but does not impersonate
another OpenAI client.

The `/v1/models` path on `openai.api.apitoken.sale` returns the OpenAI-compatible catalog.
The same path on `api.apitoken.sale`, like every other path on that hostname, preserves the original
Anthropic contract regardless of whether the client authenticates with `x-api-key`,
`Authorization: Bearer`, or both. Model objects use `owned_by: "apitoken"` and do not falsely claim
OpenAI ownership.

## Usage, rate limits and billing

The app-server's authoritative completed-turn usage is used for both response objects and durable
settlement. Responses exposes both `cached_tokens` and `cache_write_tokens` in
`input_tokens_details`; Chat exposes the same pair in `prompt_tokens_details`, matching the current
OpenAI SDK schemas. GPT-5.6 cache writes use the published 1.25x fresh-input rate; older advertised
models retain their catalog rates. Cached input, cache writes, long-context and requested Fast
subscription-credit multipliers are
applied from the pinned per-model catalog. Fast uses the published ChatGPT multipliers: 2.5x for
GPT-5.6 (including Sol/Terra/Luna) and GPT-5.5, and 2x for GPT-5.4. The same multiplier is applied
to admission reserve, final customer settlement, provider usage ledger and per-home capacity spend,
so a Fast turn cannot be under-reserved or make sellable subscription capacity look larger than it
is. The release catalog is copied from the official
[OpenAI standard token-pricing table](https://developers.openai.com/api/docs/pricing) and must be
reviewed together with the official
[Codex Fast documentation](https://learn.chatgpt.com/docs/agent-configuration/speed#fast-mode)
whenever the Codex/model pin changes; it is never updated remotely at runtime.
Admission makes a conservative input estimate and reserves provider-hidden overhead; settlement
refunds the difference using exact upstream usage.

App-server rate-limit notifications are cached and exported as process/rate-limit metrics, per home
and in aggregate. A home stops being admitted once a reported window reaches
`CLAUDE_API_CODEX_ADMIT_BELOW_USED_PERCENT` (100% by default), so the wall is met by a clean `429`
carrying the real reset instead of mid-turn; a missing snapshot is treated as available because that
endpoint is observational and must never become a hard availability dependency. When every home is
cooling or out of headroom the client receives one OpenAI-shaped `429` with the soonest recovery,
never an individual account's error. Settled traffic is attributed with an explicit `provider`
column (`anthropic` / `openai`) so the admin spend breakdown reports which upstream earned a request
rather than inferring it from the model string.

### Window-capacity calibration

Every successful turn (billed or admin) credits its exact official-price cost to the serving home,
and each rate-limit snapshot feeds a per-window calibration: an interval of at least two integer
`usedPercent` points calibrates `cap = Δspend / Δused` only when gateway spend explains at least
half of the movement (the account owner's own Codex usage must not become pool capacity) and the
sample is within `[0.25x, 4x]` of the configured prior (anti-poison). Accepted samples blend into
an EMA clamped to a 2x jump per step; window rollover only re-anchors. The result — the
subscription's sellable capacity in official-price USD per full window — is exported as
`claude_api_codex_home_window_capacity_usd{home,slot}` with a `..._calibrated` flag distinguishing
measured figures from the prior, `claude_api_codex_home_window_remaining_usd` for the unused share,
and pool sums in `claude_api_codex_window_capacity_usd{slot}` /
`claude_api_codex_window_remaining_usd{slot}`. The prior is `CLAUDE_API_CODEX_WINDOW_CAP_USD`
(default $1500/week; a ChatGPT Pro account measured ≈$1700–1800/week in July 2026), scaled by
window duration for non-weekly windows. Calibration state is in-memory: after a restart it
re-anchors from the first new snapshot, so treat the first hours after a deploy as prior-backed. Streaming delivery is bounded: a client that stops consuming frames cannot block the shared
app-server transport indefinitely. Its already-started turn is still drained to authoritative usage
and settled, matching the existing Claude disconnect invariant. Detached Responses/Chat stream tasks
hold a gateway shutdown permit through history persistence and settlement; shutdown waits for all of
them for at most 30 seconds on an OpenAI-serving process, regardless of a larger generic configured
drain deadline. It then cancels any remainder and reaps the whole Codex process group before allowing
the process-wide home lock to be released. Backpressured detached settlements
are tracked until they enter the billing FIFO, so its final flush cannot overtake them. The normal
turn-completion timeout remains capped at 600 seconds; the shutdown deadline is the stricter bound
during a deploy.

## Buying accounts: the authbot device flow

A Claude subscription is a token, so the authbot writes it to the registry and the engine reloads
it. A ChatGPT subscription has no token we may keep, so the unit that grows this pool is a
directory. `crates/authbot` therefore drives `codex login --device-auth` itself:

1. the seller is paid through the existing offer flow and supplies a proxy and the account address;
2. the bot creates hidden sibling `.<slug>.pending-<chat>` mode 0700 and starts the device flow in a
   PTY with that staging `CODEX_HOME`, sending the login through the seller's proxy so the purchase
   and the later traffic do not look like two different users;
3. the seller opens the printed link, enters the one-time code (valid 15 minutes) and approves;
4. the pinned CLI polls OpenAI itself and exits — unlike the Claude flow there is no `code#state`
   to paste back;
5. the bot confirms with `codex login status` that the profile is a ChatGPT login, writes the
   optional `proxy.url` (0600), then atomically renames staging to the public `<slug>` directory.

The bot never reads, prints or forwards anything from the auth store. A flow that expires, is
refused, or completes as an API-key login leaves no directory behind, so an unfinished purchase
cannot enter the pool. The engine admits the account on its next health tick — no restart, no
config edit, no root.

The authbot is built beside the engine and promoted from the same immutable candidate. The release
controller never kills a running different authbot because old versions have no race-free intake
drain handshake. The watchdog protects that live binary's immutable release; the new binary is
adopted only when the service is already inactive or later restarts naturally. This avoids the
cross-version `auth.json`/`proxy.url` publication race entirely.

## Verification and activation

Before enabling production traffic:

1. Verify the pinned build and patch tests.
2. Complete `codex login --device-auth`.
3. Verify `account/read` reports only account type `chatgpt`.
4. Verify the live model intersection through `/v1/models`.
5. Run non-streaming and streaming Responses calls.
6. Run non-streaming and streaming Chat Completions calls.
7. Run function-call round trips and structured-output validation.
8. Verify system/developer prompt precedence and absence of Codex helper behavior.
9. Verify disconnect drain with exact usage settlement, tenant history isolation and rate-limit
   mapping.
10. With more than one home configured, verify rotation: exhausting or de-authenticating one home
    must move traffic to another without a client-visible error, and
    `claude_api_codex_homes_available` must fall by exactly one.
11. Point an unmodified official OpenAI SDK at the gateway and verify model listing, Responses,
    typed Responses streaming, Chat Completions and Chat streaming.
12. Point an unmodified current Codex CLI profile at the gateway and verify both a text-only turn
    and a custom `exec` call/output round trip.
13. Verify unsupported nested routes return OpenAI-shaped `404` responses and never reach Claude.
14. Verify the stored-response lifecycle: a `store=true` turn is retrievable via
    `GET /v1/responses/{id}` and `/input_items`, deletable via `DELETE`, and a `store=false`
    turn 404s on all three. Verify `POST /v1/responses/input_tokens` returns an estimate.
15. Enable through the normal watchdog/blue-green promotion and wait for `/ready` plus smoke checks.

## Deliberate first-release gaps

The highest-value follow-ups are `/v1/responses/compact` and the Responses WebSocket transport.
They require explicit semantics and tests; the gateway does not pretend to support them today. A
remotely mutable model/price catalog is intentionally avoided:
the live app-server catalog is intersected with an operator-reviewed, pinned billing catalog so an
upstream metadata change cannot silently alter customer charging.

The provider-only kill switch is:

```dotenv
CLAUDE_API_CODEX_ENABLED=0
```

Disabling it leaves the fixed OpenAI process healthy but returns an OpenAI-shaped
`invalid_request_error` from its public surface; the Claude path, database schema and balances remain
unchanged. There is no database migration for this provider. Full engine rollback remains the normal
pinned-SHA watchdog rollback; the provider controller requires both current and previous immutable
releases to carry the tested `.provider-runtime-v1` capability marker before destructive handoff.

The production kill-switch drill on 2026-07-24 promoted off-phase engine SHA
`ba6fa42f7b430d3798a89a4cc0847f8ed725d472` through the complete watchdog gate. Its process
reported `claude_api_codex_enabled 0`, `claude_api_codex_process_live 0`, and no Codex rate-limit
snapshot. A public Anthropic Messages request still completed on `claude-haiku-4-5-20251001` and
settled exact usage with zero reservation left behind. The temporary account and key were disabled
and their balance returned to zero. Before the drill, the exact enabled config was preserved as the
root-only mode-0400 file `config.env.pre-killswitch-20260724-cec33a1`; its SHA-256 is
`e751548aa8f0f2089d9f995525e674dda1036cbb468e367841ebd774cc5820a7`.

The full engine rollback controller was also preflighted against immutable pre-Codex release
`10e21891643af8c01a9fe4b171095fc10a51683d` in `--engine-bluegreen --dry-run` mode. It validated
the release and calculated the exact `current`/`previous` link transaction without changing links,
services, locks, or database state.

The initial production build was independently verified on 2026-07-24:

- pre-Codex engine baseline: `10e21891643af8c01a9fe4b171095fc10a51683d`;
- Codex-capable engine release: `2299ca91c88324d3ac4bbc3039d0bcf913ed21b8`;
- pinned-builder correction: `a325c50ed8bd22ea665c913b44291aef5a1c9201`;
- Linux Codex SHA-256:
  `8f8226ed19ea65f4315aca39a8db9f9e5165ccbfcc9a7d4e6c7c8f7f51e6de2d`.

The host-local environment is not committed. Activation first preserves a pre-Codex copy of
`config.env`, then changes the shared environment without restarting the serving slot. A subsequent
engine-scoped commit makes the watchdog build a fresh slot, whose eager Codex preflight must pass
before blue-green promotion. Both `config.env.example` and `tools/codex-app-server/*` are classified
as engine paths so future activation-contract or pinned-tooling changes cannot bypass that gate.

## CLIProxyAPI audit

The implementation was compared against
[`router-for-me/CLIProxyAPI`](https://github.com/router-for-me/CLIProxyAPI) at audited commit
`285322cd97add6b21f60c267debec44fbec74060`. The useful transport patterns carried into this
gateway are:

- terminal stream-error and subscription-rate-limit classification;
- reconstruction from completed response items when delta events are incomplete;
- function-callback fallback, duplicate suppression and parallel function-call preservation;
- reasoning-summary lifecycle plus opt-in encrypted reasoning continuity;
- paginated live-model discovery intersected with a locally reviewed catalog.

The repository's direct private-backend/OAuth replay, hard-coded client headers, user-agent
impersonation and TLS-fingerprint shaping are deliberately not copied. This gateway keeps
credentials inside the official app-server and forwards only the standard proxy environment.
Likewise, prices and billable models are not remotely mutable: upstream discovery can remove an
unavailable model, but it cannot silently add a model or change customer pricing.

Two CLIProxyAPI workarounds are intentionally unnecessary on this path. Its rewriting of overlong
raw Responses item IDs and dropping of some encrypted reasoning items compensate for constraints
encountered while sending translated JSON directly to a private Codex backend. Here, typed
`thread/inject_items` validation is owned by the pinned official app-server, while gateway-created
IDs are already short and bounded. If a future app-server pin exposes a concrete limit, add it as a
public validation rule with a capture test rather than silently mutating customer input.
