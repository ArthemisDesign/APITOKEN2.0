# Gemini OAuth subscription provider

Gemini is a third, isolated provider surface:

```text
official CLI code-entry form         public native API
gemini.api.apitoken.sale/oauth/...   gemini.api.apitoken.sale/v1beta/...
                 │                                  │
                 ▼                                  ▼
authbot 127.0.0.1:8796              Caddy stable origin 127.0.0.1:8794
                 │                                  │
                 └─ encrypted roster                ▼
                                      claude-api-gemini.service :8795
                                                    │
                                                    ▼
                                      cloudcode-pa.googleapis.com
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
Standard/Plus, AI Expanded and unknown future paid tiers fail closed because the official Gemini CLI
quota document does not list them as compatible. The user's choice never overrides Google's tier
response.

Daily request limits are owned and enforced by Google. Current official documentation lists 1,500
requests/day for Pro and Standard and 2,000/day for Ultra, Enterprise and Workspace AI Ultra, but
limits and available models can change and are not contractual capacity for this gateway.

## OAuth and publication flow

Auth Bot mirrors the official Gemini CLI manual OAuth flow. It uses the public installed-application
OAuth identity embedded in Gemini CLI and Google's fixed Code Assist redirect, so the Code Assist
request is attributed to the registered Gemini CLI consumer project rather than an unrelated seller
or operator Cloud project. Sellers do not create OAuth clients and do not enable private APIs.
The source of truth is Gemini CLI's
[`packages/core/src/code_assist/oauth2.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/code_assist/oauth2.ts).

1. The seller submits only the account's dedicated proxy. When Auth Bot issues the proxy, OAuth
   starts immediately after issuance.
2. Auth Bot creates 256-bit `state`, a PKCE S256 verifier/challenge and a twenty-minute SQLite
   session.
3. The PKCE verifier, proxy, official installed-app client material and fixed exchange redirect are
   immediately moved into one XChaCha20-Poly1305 envelope bound to `state`; no plaintext value is
   retained in the Gemini OAuth row or SQLite WAL.
4. Telegram receives two non-secret links: Google's authorization URL and the Auth Bot code-entry
   form. The seller opens Google in the account's browser profile through the same dedicated proxy;
   the server cannot enforce the browser's egress.
5. Google redirects to `https://codeassist.google.com/authcode` and displays the one-use code. The
   seller pastes it into the no-store Auth Bot form at
   `https://gemini.api.apitoken.sale/oauth/callback?state=…`. The form POST keeps the code out of
   Telegram, URL query strings, browser history, referrers and ordinary access logs.
6. Auth Bot claims `state` exactly once and exchanges the code server-to-server through the account
   proxy with the same PKCE verifier, official client identity and Google redirect used at start.
7. Auth Bot validates verified Google userinfo, calls `loadCodeAssist`, completes Google's default
   onboarding when required, and re-loads the actual tier/project.
8. Unknown/free/duplicate Google subjects and reused proxy URLs fail closed. Proxy URLs are
   canonicalized before duplicate comparison, so spelling differences such as an explicit default
   port or equivalent percent-encoded credentials cannot place one egress identity into rotation
   twice. Paid-plan admission matches reviewed tier labels exactly rather than accepting a future
   tier merely because its name contains `pro` or `ultra`. A valid paid profile is sealed and
   published atomically; the runtime discovers it on the health loop without restart and refreshes
   tokens with the official per-profile OAuth material.

Auth Bot's token exchange, userinfo, `loadCodeAssist`, onboarding and operation polling use the same
attested Node helper source as the runtime, through the seller's dedicated authenticated proxy.
Google Auth calls carry the 10.9.0 library headers; Code Assist calls carry the Gemini CLI 0.53.0
default-model identity and official setup/onboarding bodies. There is no custom eligibility mode,
`client-metadata` header, Rust TLS fallback or ambient proxy path.

Required OAuth scopes:

```text
https://www.googleapis.com/auth/cloud-platform
https://www.googleapis.com/auth/userinfo.email
https://www.googleapis.com/auth/userinfo.profile
```

The installed-app client id/secret is public upstream Gemini CLI application metadata, not an
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
CLAUDE_API_GEMINI_MODELS=gemini-3.1-flash-lite,gemini-2.5-pro,gemini-2.5-flash,gemini-2.5-flash-lite
```

`CLAUDE_API_GEMINI_UPSTREAM` is pinned to `https://cloudcode-pa.googleapis.com`. Literal HTTP
loopback is available only behind the explicit test opt-in; arbitrary hosts, userinfo, path, query
and fragment are rejected. The production systemd `ExecStart` additionally pins the roster path,
Google origin and insecure-loopback switch after all shared environment files, so those deployment
boundaries cannot drift through `config.env`.

The same argv-level boundary pins the attested official runtime profile:

```text
CLAUDE_API_GEMINI_CLI_VERSION=0.53.0
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
`client-metadata`, Google project/client headers and forwarding/IP headers are stripped. Runtime
requests use the actual Gemini CLI 0.53.0 wire identity, including OAuth2Client's appended
`google-api-nodejs-client/10.9.0` token and `x-goog-api-client: gl-node/24.18.0`. Production HTTPS
generation, quota retrieval, health probes and access-token refresh run through a persistent,
per-profile Node helper and native Node/OpenSSL `https`; there is no approximate Rust/BoringSSL TLS
emulation. The authenticated proxy is supplied in the first private IPC frame, never argv or env.
Serialized outbound frames, inbound NDJSON/base64 staging, OAuth response collections and
short-lived Rust token/header/form strings are zeroized on drop. Every production request uses HTTP
CONNECT through that profile's proxy. Direct host egress and ambient proxy/TLS environment hooks are
absent. Literal loopback mocks remain on the Rust test transport and cannot be enabled for a
non-loopback origin.

The exact production observation for `/usr/bin/node` v24.18.0 on Linux x64 has two official
HTTP/1.1 profiles. Gaxios/`https` (generation, quota, probe and token requests) is JA3
`944d1e1858cd278718f8a46b65d3212f`, JA4 `t13d5211_b262b3658495_8e6e362c5eac`. Gemini CLI's global
fetch userinfo path is independently reproduced with the Node-internal Undici dispatcher and is JA3
`d67b094811e5145139d7cea5f014309f`, JA4 `t13d5212h1_b262b3658495_8e6e362c5eac`; its target headers
are the official `Authorization`, `accept: */*`, `accept-language: *`, `sec-fetch-mode: cors`,
`user-agent: node`, `accept-encoding: br, gzip, deflate` sequence. Both observations were made
through HTTP CONNECT using the SHA-pinned production binary. They attest transport equivalence to
the pinned official runtime paths; they are not a promise that Google will never apply account,
quota or abuse policy.

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
- derives a UUID-shaped upstream `request.session_id` from the keyed affinity lineage: it stays
  stable across a growing conversation, changes for another explicit session or tenant, and never
  exposes a raw tenant/session value; `user_prompt_id` follows Gemini CLI's
  `<session UUID>########<human-turn ordinal>` shape, excluding tool-result-only contents;
- decrypts the selected project/proxy only in memory;
- obtains an access token with a per-profile single-flight mutex and 120-second expiry skew;
- wraps the native body for `v1internal:{generateContent,streamGenerateContent,countTokens}`;
- reconstructs an allowlisted native response, adds a synthesized `responseId`, and discards Code
  Assist wrapper fields, credits, private trace ids, unknown top-level fields and headers;
- surfaces a mid-stream upstream error as a sanitized native error element rather than a clean
  truncation;
- caps response bodies and pending stream frames at 32 MiB;
- periodically calls official `v1internal:retrieveUserQuota`, keeps a sanitized per-model catalogue,
  and cools only the exhausted model/profile pair until Google's reset time;
- reserves customer balance before upstream delivery and settles from native `usageMetadata`.
  A metered non-stream success without authoritative non-zero usage is withheld and refunded; once
  streaming bytes have been delivered, missing final usage settles the conservative hold and emits
  an operational counter instead of inventing a usage event or granting a free request.

The model allowlist is local and price-catalog pinned. The default list contains the four models
confirmed against the production Google AI Pro profile on 2026-07-30: `gemini-3.1-flash-lite`,
`gemini-2.5-pro`, `gemini-2.5-flash`, and `gemini-2.5-flash-lite`. A configured id still needs a live
smoke test against every tier; Google can change private model availability independently. The
production systemd argv pins this calibrated four-model set after shared env files, so a stale
`config.env` cannot silently re-enable Developer-API-only models on the subscription runtime.

## Failure and stream safety

| Upstream result | Profile action | Request action |
|---|---|---|
| first `401` | compare rejected bearer, single-flight refresh | retry once on the same profile |
| repeated `401` or `403` | auth quarantine | rotate to another profile |
| `429` | cool only that model/profile from `Retry-After`, `google.rpc.RetryInfo` or quota reset | rotate without transport budget |
| network, `408`, `409`, `425`, `5xx`, malformed wrapper | short cooldown | bounded rotation |
| other deterministic `4xx` | keep profile healthy | return a synthetic native-shaped error |

Private error bodies are never returned verbatim. They may contain account, project, tier or private
endpoint details. Public errors retain only a generic Google-shaped status.

Streaming retry is allowed only before the first translated native SSE event. Startup is bounded by
time, bytes and chunk count; after delivery begins, consecutive private/accounting-only events are
also bounded, so an upstream that never produces another public event cannot hold a request forever.
After delivery starts, the request never jumps accounts. A downstream disconnect stops customer
delivery but continues bounded upstream drain to the final usage snapshot; shutdown tracks these
tasks, aborts at the deadline and settles the last known usage before exit.

Health probes call `loadCodeAssist` in `HEALTH_CHECK` mode and do not spend a model request. Empty
rosters may boot so Auth Bot can publish the first profile. Bad reloads leave the current pool intact.
Probe success does not clear a generation model's independent 429 cooling. A fresh official quota
snapshot is authoritative for catalogued models; a stale/missing bucket fails open, while an explicit
zero in any request/token quota dimension blocks that model until the latest parsed RFC3339 reset
among all exhausted dimensions. A positive second dimension never overrides an explicit exhausted
one.

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
sanitized quota/cooling timestamps, affinity counters, missing-usage count and both attested gaxios
and Undici transport versions/hashes. Subject, email, project, proxy and OAuth material are never serialized. Caddy maps
the same endpoint into the unified `admin.apitoken.sale` subscription page through stable origin
`127.0.0.1:8794`.

Expected safety properties are covered by tests for envelope AAD/key rotation, duplicate subject
rejection, hot roster reload, query/header credential stripping, Code Assist wrapper/credit removal,
bounded response parsing, quota/auth/transport rotation, concurrent 401 single-flight refresh,
affinity, split SSE translation, no post-event retry, disconnect drain and shutdown settlement.
