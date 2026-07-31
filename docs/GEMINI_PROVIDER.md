# Gemini OAuth subscription provider

Gemini is a third, isolated provider surface:

```text
Antigravity OAuth handoff            public native API
gemini.api.apitoken.sale/oauth/...   gemini.api.apitoken.sale/v1beta/...
                 │                                  │
                 ▼                                  ▼
authbot 127.0.0.1:8796              Caddy stable origin 127.0.0.1:8794
                 │                                  │
                 └─ encrypted roster                ▼
                                      claude-api-gemini.service :8795
                                                    │
                                                    ▼
                         daily-cloudcode-pa.sandbox.googleapis.com
```

The Gemini runtime, router, OAuth credentials, proxy clients, health/cooling state and deployment
are independent from Claude and OpenAI. The three providers share only the fenced billing authority
and opaque affinity infrastructure.

## Accepted subscriptions

Authbot asks Google for the actual tier and accepts only known Code Assist-compatible paid plans:

| Account type | Accepted tier | Published plan id |
|---|---|---|
| Personal | Google AI Pro | `google_ai_pro` |
| Personal | Google AI Ultra | `google_ai_ultra` |
| Organization | Gemini Code Assist Standard | `code_assist_standard` |
| Organization | Gemini Code Assist Enterprise | `code_assist_enterprise` |
| Workspace | Workspace AI Ultra | `workspace_ai_ultra` |

The product buttons expose exactly these five lines. Google AI Plus, free Individual, Workspace AI
Standard/Plus, AI Expanded and unknown future paid tiers fail closed because they are not in the
reviewed paid Antigravity/Code Assist allowlist. The user's choice never overrides Google's tier
response.

Daily request limits are owned and enforced by Google. Current official documentation lists 1,500
requests/day for Pro and Standard and 2,000/day for Ultra, Enterprise and Workspace AI Ultra, but
limits and available models can change and are not contractual capacity for this gateway.

## OAuth and publication flow

Auth Bot mirrors Antigravity's installed-application OAuth flow with PKCE. It uses Antigravity's
public OAuth client identity and fixed loopback redirect; it does not derive a Gemini Developer API
key from the subscription. Sellers do not create OAuth clients or operator Cloud projects. The
result is an OAuth credential for the internal Cloud Code/Antigravity gateway.

1. The seller submits only the account's dedicated proxy. When Auth Bot issues the proxy, OAuth
   starts immediately after issuance.
2. Auth Bot creates 256-bit `state`, a PKCE S256 verifier/challenge and a twenty-minute SQLite
   session.
3. The PKCE verifier, proxy, Antigravity installed-app client material and fixed exchange redirect are
   immediately moved into one XChaCha20-Poly1305 envelope bound to `state`; no plaintext value is
   retained in the Gemini OAuth row or SQLite WAL.
4. Telegram receives two non-secret links: Google's authorization URL and the Auth Bot completion
   form. The seller opens Google in the account's browser profile through the same dedicated proxy;
   the server cannot enforce the browser's egress.
5. Google redirects the browser to `http://localhost:51121/oauth-callback`. No local listener is
   required: a browser connection error is expected. The seller copies the complete callback URL
   from the address bar and pastes it into the no-store form at
   `https://gemini.api.apitoken.sale/oauth/callback?state=…`. The parser accepts only the exact HTTP
   localhost host, port and path, rejects credentials/fragments/OAuth errors and requires callback
   `state` to match the hidden form state. The form POST keeps the code out of Telegram and access
   log query strings. A raw authorization code remains accepted only for in-flight compatibility.
6. Auth Bot claims `state` exactly once and exchanges the code server-to-server through the account
   proxy with the same PKCE verifier, official client identity and Google redirect used at start.
7. Auth Bot validates verified Google userinfo, calls Antigravity `loadCodeAssist`, completes
   `onboardUser` when required, and re-loads the actual tier/project. Control-plane calls fall back
   only among the three reviewed Cloud Code hosts.
8. Unknown/free Google subjects and reused proxy URLs fail closed. A duplicate subject is rejected
   except for a one-way migration of that subject's existing legacy Gemini CLI credential to
   Antigravity through the same canonical proxy. The migration preserves the opaque profile id,
   roster bytes and existing IPRoyal lifecycle metadata, and atomically replaces only the sealed
   credential; an existing Antigravity credential, reverse transition or proxy mismatch remains an
   error. Proxy URLs are canonicalized before comparison, so spelling differences such as an
   explicit default port or equivalent percent-encoded credentials cannot place one egress identity
   into rotation twice. Paid-plan admission matches reviewed tier labels exactly rather than
   accepting a future tier merely because its name contains `pro` or `ultra`. A valid paid profile
   is sealed and published atomically; the runtime discovers it on the health loop without restart
   and refreshes tokens with the official per-profile OAuth material.

Auth Bot's token exchange, userinfo, `loadCodeAssist` and onboarding use the same bounded Node helper
source as the runtime through the seller's dedicated authenticated proxy. The wire identity is
pinned to reviewed Antigravity 2.2.1: runtime/control calls use
`antigravity/hub/2.2.1 darwin/arm64`, onboarding appends
`google-api-nodejs-client/10.3.0`, and token exchange uses `Go-http-client/2.0`. There is no ambient
proxy path or arbitrary OAuth client.

Required OAuth scopes:

```text
https://www.googleapis.com/auth/cloud-platform
https://www.googleapis.com/auth/userinfo.email
https://www.googleapis.com/auth/userinfo.profile
https://www.googleapis.com/auth/cclog
https://www.googleapis.com/auth/experimentsandconfigs
```

The installed-app client id/secret is public upstream Antigravity application metadata, not an
operator secret. Account access tokens, refresh tokens, PKCE verifiers, identity, proxy credentials
and encrypted-roster keys remain secret and must never enter the repository, command line, systemd
unit, Telegram or logs.

## Encrypted roster contract

Generate a 32-byte key and assign a non-secret key id:

```bash
openssl rand -hex 32
```

Use the same read keyring in Auth Bot and the Gemini runtime:

```text
AUTH_BOT_GEMINI_CREDENTIAL_KEYS=current:<64-hex>
AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID=current
CLAUDE_API_GEMINI_CREDENTIAL_KEYS=current:<64-hex>
```

The roster contains no Google identity, project, token, plan or proxy:

```json
{
  "profiles": [
    {
      "id": "gemini_oauth_000001",
      "credential_file": "/srv/claude-api/data/gemini/credentials/gemini_oauth_000001.json"
    }
  ]
}
```

Every credential path must be exactly `<roster-dir>/credentials/<profile-id>.json`; alternate paths,
symlinks, duplicate ids, duplicate Google subjects and canonical proxy reuse are rejected.
Directories are `0700`, files are `0600`, profile id is AEAD associated data, and publication is
file-first then atomic roster rename plus directory fsync. Runtime also revalidates the pinned
official OAuth client pair/token endpoint, the exact plan↔tier-label mapping and the reviewed
paid-plan allowlist after opening every envelope; a manually created but cryptographically valid
envelope cannot relax those admission rules.

Start Auth Bot once (or provision the two private directories explicitly) before starting the
Gemini runtime. Its systemd unit requires `/srv/claude-api/data/gemini` to exist and mounts it
read-only; it intentionally fails closed instead of skipping protection for an absent path.

The envelope contains access/refresh tokens, OAuth client material, Google subject/email, managed
project, detected plan, authenticated proxy and the opaque IPRoyal order id when Auth Bot issued the
proxy. Debug output is redacted and secret-bearing structs zeroize on drop.

### IPRoyal lifecycle

Paid Gemini offers use the same common post-payment IPRoyal purchase path as Claude offers. Auth Bot
buys one UK ISP allocation, passes its HTTP CONNECT URL directly into the state-bound OAuth
envelope, and stores the non-secret order id inside the final credential envelope. The seller's
browser, OAuth exchange, userinfo, eligibility/onboarding, access-token refresh, health probes and
generation traffic must stay on that one allocation.

The 30-minute lifecycle scan reads only opaque profile/order/timestamp metadata from the encrypted
roster. Near expiry it extends the exact IPRoyal order; it never replaces the proxy or silently uses
direct host egress. A missing order produces an operator alert. Manually supplied proxies have order
id zero and remain externally managed.

### Key rotation

1. Add `new:<key>` before the old key in both keyrings; keep `old:<key>` present.
2. Set `AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID=new` and restart Auth Bot.
3. Auth Bot validates the complete roster and atomically re-seals every old envelope under `new`.
4. Verify Auth Bot and Gemini readiness, then remove `old` from both environments.

A missing/corrupt key, unexpected path or duplicate account stops rotation without publishing a
partially trusted roster. Individual file replacements are atomic, and both keys remain readable
during the transition.

## Environment

Auth Bot (`/srv/claude-api/data/authbot.env`):

```text
AUTH_BOT_IPROYAL_KEY=<existing reseller key shared with Claude provisioning>
AUTH_BOT_GEMINI_CREDENTIAL_KEYS=current:<64-hex>[,old:<64-hex>]
AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID=current
AUTH_BOT_GEMINI_REDIRECT_URI=https://gemini.api.apitoken.sale/oauth/callback
AUTH_BOT_GEMINI_OAUTH_BIND=127.0.0.1:8796
AUTH_BOT_GEMINI_DIR=/srv/claude-api/data/gemini
```

`AUTH_BOT_GEMINI_REDIRECT_URI` retains its legacy name but now identifies the public Auth Bot code
form; it is not the redirect sent to Google. Keep `authbot.env`, `server.env` and generated key
material root-owned and mode `0600`; never place them in a release directory, shell history,
Telegram message or systemd command line.

Gemini runtime (`config.env` or `server.env`):

```text
CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json
CLAUDE_API_GEMINI_CREDENTIAL_KEYS=current:<64-hex>[,old:<64-hex>]
CLAUDE_API_GEMINI_MODELS=gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite
CLAUDE_API_GEMINI_MAX_INFLIGHT_PER_PROFILE=6
CLAUDE_API_GEMINI_QUOTA_RESERVE=0.05
CLAUDE_API_GEMINI_QUOTA_RESERVE_JITTER=0.01
```

`CLAUDE_API_GEMINI_UPSTREAM` defaults to and is production-pinned at
`https://daily-cloudcode-pa.sandbox.googleapis.com`. The validator also recognizes only the daily
and production Cloud Code hosts. Literal HTTP loopback is available only behind the explicit test
opt-in; arbitrary hosts, ports, userinfo, path, query and fragment are rejected. Legacy Gemini CLI
credentials ignore the Antigravity default and remain pinned to
`https://cloudcode-pa.googleapis.com`. The production systemd `ExecStart` pins the roster path,
Antigravity origin and insecure-loopback switch after all shared environment files.

The same argv-level boundary pins the attested official runtime profile:

```text
CLAUDE_API_GEMINI_ANTIGRAVITY_VERSION=2.2.1
CLAUDE_API_GEMINI_NODE_BINARY=/usr/bin/node
CLAUDE_API_GEMINI_NODE_VERSION=v24.18.0
CLAUDE_API_GEMINI_NODE_SHA256=41a74efb34cbde5c7632cdac0cf8bd1a14d0b8d73dc1e82755014d9a9ce70f5c
```

Production startup hashes the binary before accepting profiles, and each helper handshake verifies
the Node version plus Linux/x64 platform. A Node/OpenSSL upgrade is therefore an explicit reviewed
fingerprint change, never an ambient package update.

## Runtime behavior

The public surface remains native Gemini-shaped:

```text
GET     /v1beta/models                                    (pageSize / pageToken honoured)
GET     /v1beta/models/{model}
POST    /v1beta/models/{model}:generateContent
POST    /v1beta/models/{model}:streamGenerateContent      (alt=sse → SSE; default / alt=json → JSON array)
POST    /v1beta/models/{model}:countTokens
OPTIONS *                                                 (CORS preflight, unauthenticated)
```

Query credentials are forbidden. The customer's `x-goog-api-key`, `x-api-key` or Bearer token
authorizes this gateway and is never sent to Google. Client Authorization, User-Agent,
Google project headers and forwarding/IP headers are stripped. Runtime new profiles use the pinned
Antigravity 2.2.1 wire identity, including the reviewed bounded `Client-Metadata` and
`x-goog-api-client` values; caller-supplied variants never pass through. Antigravity refresh uses
`Go-http-client/2.0`; generation and control calls use `antigravity/hub/2.2.1 darwin/arm64`.
Production HTTPS generation, quota retrieval, health
probes and access-token refresh run through a persistent,
per-profile Node helper and native Node/OpenSSL `https`; there is no approximate Rust/BoringSSL TLS
emulation. The authenticated proxy is supplied in the first private IPC frame, never argv or env.
Serialized outbound frames, inbound NDJSON/base64 staging, OAuth response collections and
short-lived Rust token/header/form strings are zeroized on drop. Every production request uses HTTP
CONNECT through that profile's proxy. Direct host egress and ambient proxy/TLS environment hooks are
absent. Literal loopback mocks remain on the Rust test transport and cannot be enabled for a
non-loopback origin.

The exact gateway transport observation for `/usr/bin/node` v24.18.0 on Linux x64 has two pinned
HTTP/1.1 profiles. The `https` path (generation, quota, probe and token requests) is JA3
`944d1e1858cd278718f8a46b65d3212f`, JA4 `t13d5211_b262b3658495_8e6e362c5eac`. The global-fetch
userinfo path is independently reproduced with the Node-internal Undici dispatcher and is JA3
`d67b094811e5145139d7cea5f014309f`, JA4 `t13d5212h1_b262b3658495_8e6e362c5eac`; its target headers
are the official `Authorization`, `accept: */*`, `accept-language: *`, `sec-fetch-mode: cors`,
`user-agent: node`, `accept-encoding: br, gzip, deflate` sequence. Both observations were made
through HTTP CONNECT using the SHA-pinned production binary. They attest the gateway's transport
stability; they are not a promise that Google will never apply account, quota or abuse policy.

The public surface is shaped to be indistinguishable from `generativelanguage.googleapis.com` on the
client side: proto-JSON snake_case aliases are accepted (and canonicalized) alongside camelCase; a
native-shaped `responseId` is synthesized on every response and stream chunk (never the correlatable
Code Assist trace id); `streamGenerateContent` frames as SSE only when the client asks for `alt=sse`
and otherwise streams the native JSON array; errors carry a native `error.details[]`
(`google.rpc.ErrorInfo`/`RetryInfo`) with Google-consistent HTTP↔status pairs; the model resource
carries the native version and sampling defaults; and every response carries Google's security and
CORS headers so a browser SDK can call the gateway. Balance exhaustion remains the documented 402.

For every request the runtime:

- resolves opaque tenant-bound prompt affinity and prefers the same subscription;
- derives a UUID-shaped upstream `request.sessionId` from the keyed affinity lineage: it stays
  stable across a growing conversation, changes for another explicit session or tenant, and never
  exposes a raw tenant/session value; one `agent-<uuid>` request id is created before rotation and
  reused for every retry of that customer request;
- decrypts the selected project/proxy only in memory;
- obtains an access token with a per-profile single-flight mutex and 120-second expiry skew;
- wraps the native body for `v1internal:{generateContent,streamGenerateContent,countTokens}`;
- resolves a canonical Gemini 3 model plus `thinkingConfig.thinkingLevel` to the reviewed private
  Antigravity effort/quota bucket before admission. Quota and model cooling follow that private
  bucket, while affinity, customer billing, the public catalogue and returned `modelVersion` stay
  on the canonical Developer API model id;
- adapts valid public generation requests to Antigravity's stricter private wire contract: blank or
  omitted `contents[].role` values are inferred as alternating `user`/`model` turns, and the public
  65,536-token model ceiling is clamped to the private endpoint's accepted boundary of 65,535;
- reconstructs an allowlisted native response, adds a synthesized `responseId`, and discards Code
  Assist wrapper fields, credits, private trace ids, unknown top-level fields and headers;
- surfaces a mid-stream upstream error as a sanitized native error element rather than a clean
  truncation;
- caps response bodies and pending stream frames at 32 MiB;
- periodically calls Antigravity `v1internal:fetchAvailableModels`, keeps a sanitized per-model
  `remainingFraction`/`resetTime` catalogue, and cools only the exhausted model/profile pair until
  Google's reset time;
- independently calls `v1internal:retrieveUserQuotaSummary` and accepts only the exact
  `gemini-5h` and `gemini-weekly` buckets. Every successful generation, including admin traffic,
  credits the serving opaque profile with its exact audited Developer API price in integer
  nanoUSD. Each bucket is calibrated independently from real positive fraction movement with
  cumulative integer weighted least squares
  `capacity = 100000000 * Σ(Δused * Δspend) / Σ(Δused²)`. There is no subscription-price prior,
  EMA, float money arithmetic or use of the foreign-provider `3p-*` buckets. The first snapshot is
  an anchor and the first movement after cold start/reset is censored. A cold profile stays `null`
  with no dollar Prometheus sample until the next complete interval; a later reset preserves the
  already measured cumulative estimate while rearming the safe anchor. Cumulative spend, CAS state
  and raw replay evidence live in the engine authority and survive blue-green deploys;
- limits each paid profile to a bounded number of concurrent requests and routes new work toward
  profiles with more per-model quota headroom. A small deterministic per-profile reserve prevents
  synchronized draining, but remains soft: if all eligible profiles are below reserve, the final
  working subscription continues serving until Google reports an explicit zero;
- records generation-specific failure streaks, last success/failure timestamps and an exponential
  per-model cooldown. HTTP 5xx/malformed generation failures therefore degrade only that model;
  proxy/network/token-refresh failures still cool the complete profile. `countTokens` remains a
  quota-free diagnostic and cannot falsely rehabilitate generation health;
- reserves customer balance before upstream delivery and settles from native `usageMetadata`.
  A metered non-stream success without authoritative non-zero usage is withheld and refunded; once
  streaming bytes have been delivered, missing final usage settles the conservative hold and emits
  an operational counter instead of inventing a usage event or granting a free request.

The model allowlist is local and price-catalog pinned. The default list contains the six text
models whose non-stream, native stream and token-count paths were reconfirmed against the
production Google AI Pro profile on 2026-07-31: `gemini-3.6-flash`, `gemini-3.5-flash`,
`gemini-3.1-pro-preview`, `gemini-3.1-flash-lite`, `gemini-2.5-flash`, and
`gemini-2.5-flash-lite`. `gemini-2.5-pro` is deliberately not published: it is absent from the
official Antigravity reasoning-model table, and its residual quota bucket does not produce a
working generation route. Private tier ids are never public model names, while
image/agent/foreign-provider ids have no honest public text-model mapping.
A Developer API price entry proves only that the gateway can meter a model; it does not prove that
an Antigravity subscription can serve it. Publication additionally requires an official
Antigravity model contract, an exact canonical-to-private route and live generation evidence.
A configured id still needs a live smoke test against every tier because Google can change private
model availability independently. The production systemd argv pins this calibrated six-model
set after shared env files, so a stale
`config.env` cannot silently re-enable Developer-API-only models on the subscription runtime.

### Text-model evidence matrix

The two namespaces are not interchangeable. Public names and prices come from the official Gemini
Developer API; private ids come from authenticated Antigravity `fetchAvailableModels` plus a live
text-generation check. A quota row by itself is never admission evidence.

| Public Developer API model | Private Antigravity wire id | Production evidence | Decision |
|---|---|---|---|
| `gemini-3.6-flash` | low → `gemini-3.6-flash-low`; medium/default → `gemini-3.6-flash-medium`; high → `gemini-3.6-flash-high` | default/minimal/low/medium/high: generate 200, incremental SSE 200, countTokens 200; canonical modelVersion and non-zero usage verified on 2026-07-31 | published |
| `gemini-3.5-flash` | minimal → `gemini-3.5-flash-extra-low`; low/medium/high/default → `gemini-3.5-flash-low`, with the requested native thinking level preserved | default/minimal/low/medium/high: generate 200, incremental SSE 200, countTokens 200; default and `alt=json` JSON streams 200; canonical modelVersion and non-zero usage verified on Google AI Pro on 2026-07-31 | published |
| `gemini-3.1-pro-preview` | low → `gemini-3.1-pro-low`; medium/high/default → `gemini-pro-agent` with the requested native thinking level preserved | default/low/medium/high: generate 200, incremental SSE 200, countTokens 200; canonical modelVersion and non-zero usage verified on 2026-07-31 | published |
| `gemini-3.1-flash-lite` | same id | generate 200, SSE 200 | published |
| `gemini-2.5-flash` | same id | generate 200, SSE 200 | published |
| `gemini-2.5-flash-lite` | same id | generate 200, SSE 200 | published |
| `gemini-2.5-pro` | same id | not listed for any Antigravity plan; residual Pro quota row exists, but fresh probe gives generate → 503 `UNAVAILABLE`, SSE/countTokens → 429 `RESOURCE_EXHAUSTED` | rejected even if requested through env |
| `gemini-3.5-flash-lite` | absent | Developer API model, but absent from the Antigravity model table and the live Pro subscription catalogue | rejected on every currently documented Antigravity plan |
| `gemini-3.6-flash-tiered` | unknown private semantics | quota-only row without a display contract | never published |

Antigravity's official model table marks Gemini 3.6 Flash, Gemini 3.5 Flash and Gemini 3.1 Pro as
available on Free/Google AI Plus, Google AI Pro, Google AI Ultra and Enterprise. Pro and Ultra
change quota size and refresh cadence, not that model set; Ultra therefore does not unlock
`gemini-2.5-pro` or `gemini-3.5-flash-lite`. The gateway still requires a live check for each
profile type because documented product access and a working private API route are separate facts.

| Current Antigravity model | Free / Google AI Plus | Google AI Pro | Google AI Ultra | Enterprise | Gateway decision |
|---|---:|---:|---:|---:|---|
| Gemini 3.6 Flash | yes | yes | yes | yes | published |
| Gemini 3.5 Flash | yes | yes | yes | yes | published |
| Gemini 3.1 Pro | yes | yes | yes | yes | published as `gemini-3.1-pro-preview` |
| Gemini 2.5 Pro | not listed | not listed | not listed | not listed | rejected |
| Gemini 3.5 Flash-Lite | not listed | not listed | not listed | not listed | rejected |

The three older published text routes (`gemini-3.1-flash-lite`, `gemini-2.5-flash` and
`gemini-2.5-flash-lite`) are not claims about the current Antigravity marketing table. They remain
enabled because their exact private routes generated and streamed successfully on the live Google
AI Pro profile. They do not require Ultra. Availability on another profile type must be established
by that profile's own live calibration rather than inferred from the Developer API catalogue.

Official evidence reviewed on 2026-07-31:

- model catalogue and lifecycle: <https://ai.google.dev/gemini-api/docs/models>;
- Antigravity model availability by plan: <https://antigravity.google/docs/models>;
- Antigravity plan and quota differences: <https://antigravity.google/docs/plans>;
- Gemini 3.6 Flash shape (1,048,576 input / 65,536 output, text output):
  <https://ai.google.dev/gemini-api/docs/models/gemini-3.6-flash>;
- Gemini 3.5 Flash shape (1,048,576 input / 65,536 output, text output):
  <https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash>;
- Gemini 3.1 Pro Preview shape: <https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview>;
- paid standard prices: <https://ai.google.dev/gemini-api/docs/pricing>;
- thinking-level defaults and supported values: <https://ai.google.dev/gemini-api/docs/thinking#thinking-levels>;
- REST schema/discovery revision `20260729`:
  <https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta>.

## Failure and stream safety

| Upstream result | Profile action | Request action |
|---|---|---|
| first `401` | compare rejected bearer, single-flight refresh | retry once on the same profile |
| repeated `401` or `403` | auth quarantine | rotate to another profile |
| `429` | cool only that model/profile from `Retry-After`, `google.rpc.RetryInfo` or quota reset | rotate without transport budget |
| network/token refresh, `408`, `409`, `425` | short profile cooldown | bounded rotation |
| generation `5xx` or malformed wrapper/stream | exponential model cooldown | bounded rotation without disabling other models |
| other deterministic `4xx` | keep profile healthy | return a synthetic native-shaped error |

Private error bodies are never returned verbatim. They may contain account, project, tier or private
endpoint details. Public errors retain only a generic Google-shaped status.

Streaming retry is allowed only before the first translated native SSE event. Startup is bounded by
time, bytes and chunk count; after delivery begins, consecutive private/accounting-only events are
also bounded, so an upstream that never produces another public event cannot hold a request forever.
After delivery starts, the request never jumps accounts. A downstream disconnect stops customer
delivery but continues bounded upstream drain to the final usage snapshot; shutdown tracks these
tasks, aborts at the deadline and settles the last known usage before exit.

Antigravity health probes call `loadCodeAssist` with `metadata.ideType=ANTIGRAVITY` and do not spend
a model request. Empty
rosters may boot so Auth Bot can publish the first profile. Bad reloads leave the current pool intact.
Probe success does not clear a generation model's independent 429 cooling. A fresh official quota
snapshot is authoritative for catalogued models; a stale/missing bucket fails open, while an explicit
zero blocks that model until its parsed RFC3339 reset. Legacy Gemini CLI profiles keep their former
`HEALTH_CHECK`, `retrieveUserQuota`, `request.session_id`, `user_prompt_id` and Google library
headers so an existing sealed roster remains usable during migration.

## Operations

```bash
systemctl status claude-authbot.service claude-api-gemini.service
curl --fail http://127.0.0.1:8795/ready
curl --fail http://127.0.0.1:8794/ready
curl -H 'x-api-key: <control-or-readonly-key>' http://127.0.0.1:8794/gemini-subs
curl --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  https://gemini.api.apitoken.sale/v1beta/models
```

`/gemini-subs` is read-only-key protected and exposes only opaque profile ids, model availability,
sanitized quota/cooling timestamps, independent 5h/weekly fractions and measured official-price
capacity/remaining/confidence, calibration persistence health, generation failure
streak/timestamps/classes, low-cardinality transport/backend/malformed/stream-start counters,
affinity counters, missing-usage count and pinned HTTPS/Undici transport versions/hashes. Unknown
capacity stays JSON `null`; measured fleet totals include only profiles with evidence. Subject,
email, project, tier, proxy and OAuth material are never serialized. Caddy maps the same endpoint
into the unified `admin.apitoken.sale` subscription page through stable origin `127.0.0.1:8794`.

Expected safety properties are covered by tests for envelope AAD/key rotation, duplicate subject
rejection, in-place legacy-to-Antigravity migration with proxy/lifecycle preservation, hot roster
reload, query/header credential stripping, Code Assist wrapper/credit removal, bounded response
parsing, quota/auth/transport rotation, concurrent 401 single-flight refresh, affinity, split SSE
translation, no post-event retry, disconnect drain and shutdown settlement.
