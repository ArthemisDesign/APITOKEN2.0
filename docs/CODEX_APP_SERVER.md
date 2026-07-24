# Codex app-server: OpenAI-compatible text transport

## Scope and compatibility boundary

The optional Codex provider runs the official OpenAI `codex app-server` as a supervised local
JSON-RPC child. It exposes a deliberately strict, SDK-compatible text subset:

| Public route | Status |
|---|---|
| `POST /v1/responses` | supported, streaming and non-streaming |
| `POST /v1/chat/completions` | supported adapter, streaming and non-streaming |
| `GET /v1/models` | supported; intersected with the live app-server catalog |
| `GET /v1/models/{model}` | supported |

Images, video, audio, embeddings, batches, files, assistants, fine-tuning, WebSocket/realtime,
stored-response retrieval and administrative OpenAI Platform endpoints are not implemented.
Unsupported descendants of the implemented surfaces, including `/v1/responses/compact`,
`/v1/responses/input_tokens`, stored-response retrieval/cancel/input-items routes and nested Chat
Completions paths, return an OpenAI-shaped `404` for normal OpenAI requests. Known unsupported
OpenAI route families are also rejected locally instead of being sent to Anthropic. Only a request
carrying an explicit Anthropic protocol header preserves the pre-existing Claude fallback.
Sampling/output controls that app-server cannot enforce are rejected as OpenAI-shaped `400` errors
instead of being silently ignored. In particular, non-default `temperature`, `top_p`, token caps,
`stop`, penalties, logprobs, `seed`, and multi-choice output are not accepted.

This is not the OpenAI Platform API and must not be represented as an OpenAI-operated endpoint.
ChatGPT subscriptions and OpenAI Platform API billing are separate products. Confirm that the
applicable subscription terms permit the intended commercial workload before customer-facing use.

## Architecture and trust boundary

```text
OpenAI SDK client
  -> Rust HTTP/auth/admission/billing layer
  -> exact history + request translation
  -> pinned, patched official codex app-server over stdio JSON-RPC
  -> OpenAI services through the app-server's supported ChatGPT login
```

The transport never reads or replays ChatGPT bearer tokens. Authentication remains owned by the
official Codex binary in a dedicated `CODEX_HOME`. The Rust child launcher:

- verifies the exact executable SHA-256 and exact `codex --version` before use;
- starts with an empty inherited environment and adds only fixed runtime values plus an allowlist of
  standard proxy variables;
- uses an empty dedicated work directory and disables project docs, MCP servers, plugins, skills,
  apps, collaboration, multi-agent helpers, environment-context injection and permission tools;
- requires `account/read` to report `chatgpt`; API-key auth is rejected;
- redacts child stderr and never logs account identity, credentials, prompts or response bodies.

When the provider is enabled, server startup must complete that binary attestation, app-server
initialization and `account/read` subscription check before the slot can become ready; it also
attempts an initial rate-limit snapshot without making that observability endpoint a hard
availability dependency. A failed Codex activation therefore cannot be promoted by the watchdog
while the previous Claude-capable slot is still healthy. Later child failures are restarted on
demand.

Each API request uses an ephemeral app-server thread. Public continuity is reconstructed from the
client's exact input or a tenant-bound encrypted `previous_response_id` history record. A response
ID from one billed account cannot be replayed by another.

## Removing Codex's injected prompt

The pinned patch is
[`tools/codex-app-server/0001-api-compat-dynamic-tools-only.patch`](../tools/codex-app-server/0001-api-compat-dynamic-tools-only.patch).
It makes the model-visible context contain only:

1. the client's explicit system/developer instructions;
2. the client's conversation items;
3. the client's declared function tools.

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
5. runs the patch-specific core library tests plus the official app-server request-capture test
   proving that empty instruction overrides omit the upstream `instructions` field;
6. runs a locked release build;
7. installs an immutable, content-addressed binary and atomically updates `codex`.

Production layout:

```text
/srv/claude-api/data/codex/bin   root-owned, 0755 directory, 0555 binaries
/srv/claude-api/data/codex/home  deploy-owned, 0700, authentication state
/srv/claude-api/data/codex/work  deploy-owned, 0700, empty working directory
```

Build without touching the running engine:

```bash
sudo /opt/apitoken/repo/tools/codex-app-server/build-pinned.sh \
  --install-dir /srv/claude-api/data/codex/bin
```

Record the emitted `CODEX_BINARY_SHA256`; enabling the provider without the matching digest is a
startup error.

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
CLAUDE_API_CODEX_BIN=/srv/claude-api/data/codex/bin/codex
CLAUDE_API_CODEX_BIN_SHA256=<sha256 emitted by build-pinned.sh>
CLAUDE_API_CODEX_VERSION=codex-cli 0.145.0
CLAUDE_API_CODEX_HOME=/srv/claude-api/data/codex/home
CLAUDE_API_CODEX_WORK_DIR=/srv/claude-api/data/codex/work
CLAUDE_API_CODEX_MODELS=gpt-5.6,gpt-5.6-sol,gpt-5.6-terra,gpt-5.6-luna,gpt-5.5,gpt-5.4
CLAUDE_API_CODEX_MAX_CONCURRENT=4
```

Optional controls:

```dotenv
CLAUDE_API_CODEX_STARTUP_TIMEOUT_MS=20000
CLAUDE_API_CODEX_RPC_TIMEOUT_MS=15000
CLAUDE_API_CODEX_TURN_TIMEOUT_MS=600000
CLAUDE_API_CODEX_RESERVE_OVERHEAD_TOKENS=16384
CLAUDE_API_CODEX_HISTORY_TTL_SECS=86400
CLAUDE_API_CODEX_HISTORY_LOCAL_CAP=10000
CLAUDE_API_CODEX_HISTORY_REDIS_TIMEOUT_MS=100
```

When `CLAUDE_API_REDIS_URL` enables shared response history, Codex reuses
`CLAUDE_API_AFFINITY_SECRET` to encrypt and authenticate tenant-bound history records. Startup
fails closed if Redis history is enabled without that secret; changing it invalidates previously
issued `previous_response_id` values.

Only standard `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` variables, including their
lower-case spellings, may reach the child. No custom TLS fingerprinting, user-agent spoofing or
private endpoint replay is used. A proxy therefore changes network egress but does not impersonate
another OpenAI client.

The shared `/v1/models` path chooses the OpenAI-compatible catalog for Bearer-authenticated OpenAI
clients. Existing Anthropic clients remain on the Claude path when they send Anthropic protocol
headers or use `x-api-key` without a Bearer header. Model objects use `owned_by: "apitoken"` and do
not falsely claim OpenAI ownership.

## Usage, rate limits and billing

The app-server's authoritative completed-turn usage is used for both response objects and durable
settlement. Cached input and long-context price multipliers are applied from the pinned per-model
catalog. The release catalog is copied from the official
[OpenAI standard token-pricing table](https://developers.openai.com/api/docs/pricing) and must be
reviewed again whenever the Codex/model pin changes; it is never updated remotely at runtime.
Admission makes a conservative input estimate and reserves provider-hidden overhead; settlement
refunds the difference using exact upstream usage.

App-server rate-limit notifications are cached and exported as process/rate-limit metrics. A reached
subscription window produces an OpenAI-shaped `429` with `Retry-After`. There is one authenticated
Codex home in the initial implementation, not the existing multi-subscription Claude rotation pool.
Streaming delivery is bounded: a client that stops consuming frames cannot block the shared
app-server transport indefinitely. Its already-started turn is still drained to authoritative usage
and settled, matching the existing Claude disconnect invariant.

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
10. Point an unmodified official OpenAI SDK at the gateway and verify model listing, Responses,
    typed Responses streaming, Chat Completions and Chat streaming.
11. Verify unsupported nested routes return OpenAI-shaped `404` responses and never reach Claude.
12. Enable through the normal watchdog/blue-green promotion and wait for `/ready` plus smoke checks.

## Deliberate first-release gaps

The highest-value follow-ups are a pool of isolated `CODEX_HOME` accounts with health-aware
scheduling, `/v1/responses/compact`, `/v1/responses/input_tokens`, stored-response lifecycle routes
and the Responses WebSocket transport. They require explicit semantics and tests; the gateway does
not pretend to support them today. A remotely mutable model/price catalog is intentionally avoided:
the live app-server catalog is intersected with an operator-reviewed, pinned billing catalog so an
upstream metadata change cannot silently alter customer charging.

The provider-only kill switch is:

```dotenv
CLAUDE_API_CODEX_ENABLED=0
```

Disabling it leaves the Claude path, database schema and balances unchanged. There is no database
migration for this provider. Full engine rollback remains the normal pinned-SHA watchdog rollback;
the pre-Codex production baseline is recorded in the deployment log before activation.

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
