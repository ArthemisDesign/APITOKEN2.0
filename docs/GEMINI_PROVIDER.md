# Gemini OAuth subscription provider

Gemini is a third, isolated provider surface:

```text
Google OAuth callback                public native API
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

1. The seller submits the account's dedicated proxy to Auth Bot.
2. Auth Bot creates 256-bit `state`, a PKCE S256 verifier/challenge and a ten-minute SQLite session.
3. The PKCE verifier and proxy are immediately moved into one XChaCha20-Poly1305 envelope bound to
   `state`; neither plaintext value is retained in the Gemini OAuth row or SQLite WAL.
4. Telegram receives only a Google authorization URL. Tokens are never pasted into chat. The
   seller must open it in the account's browser profile through the same dedicated proxy; the
   server cannot enforce the browser's egress.
5. `https://gemini.api.apitoken.sale/oauth/callback` claims `state` exactly once and exchanges the
   code server-to-server through the account proxy.
6. Auth Bot validates verified Google userinfo, calls `loadCodeAssist`, completes Google's default
   onboarding when required, and re-loads the actual tier/project.
7. Unknown/free/duplicate Google subjects and reused proxy URLs fail closed. A valid paid profile is
   sealed and published atomically; the runtime discovers it on the health loop without restart.

Required OAuth scopes:

```text
https://www.googleapis.com/auth/cloud-platform
https://www.googleapis.com/auth/userinfo.email
https://www.googleapis.com/auth/userinfo.profile
```

Create a Google OAuth **Web application** with this exact redirect URI:

```text
https://gemini.api.apitoken.sale/oauth/callback
```

Configure and verify the OAuth consent screen as required by Google. Do not put client credentials
in the repository, command line or systemd unit.

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
symlinks, duplicate ids, duplicate Google subjects and exact proxy reuse are rejected. Directories
are `0700`, files are `0600`, profile id is AEAD associated data, and publication is file-first then
atomic roster rename plus directory fsync.

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
AUTH_BOT_GEMINI_CLIENT_ID=<operator web OAuth client id>
AUTH_BOT_GEMINI_CLIENT_SECRET=<operator web OAuth client secret>
AUTH_BOT_GEMINI_CREDENTIAL_KEYS=current:<64-hex>[,old:<64-hex>]
AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID=current
AUTH_BOT_GEMINI_REDIRECT_URI=https://gemini.api.apitoken.sale/oauth/callback
AUTH_BOT_GEMINI_OAUTH_BIND=127.0.0.1:8796
AUTH_BOT_GEMINI_DIR=/srv/claude-api/data/gemini
```

Keep `authbot.env`, `server.env` and any generated key material root-owned and mode `0600`; never
place them in a release directory, shell history, Telegram message or systemd command line.

Gemini runtime (`config.env` or `server.env`):

```text
CLAUDE_API_GEMINI_PROFILES_FILE=/srv/claude-api/data/gemini/profiles.json
CLAUDE_API_GEMINI_CREDENTIAL_KEYS=current:<64-hex>[,old:<64-hex>]
CLAUDE_API_GEMINI_MODELS=gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-flash-lite,gemini-2.5-pro,gemini-2.5-flash,gemini-2.5-flash-lite
```

`CLAUDE_API_GEMINI_UPSTREAM` is pinned to `https://cloudcode-pa.googleapis.com`. Literal HTTP
loopback is available only behind the explicit test opt-in; arbitrary hosts, userinfo, path, query
and fragment are rejected. The production systemd `ExecStart` additionally pins the roster path,
Google origin and insecure-loopback switch after all shared environment files, so those deployment
boundaries cannot drift through `config.env`.

## Runtime behavior

The public surface remains native Gemini-shaped:

```text
GET  /v1beta/models
GET  /v1beta/models/{model}
POST /v1beta/models/{model}:generateContent
POST /v1beta/models/{model}:streamGenerateContent?alt=sse
POST /v1beta/models/{model}:countTokens
```

Query credentials are forbidden. The customer's `x-goog-api-key`, `x-api-key` or Bearer token
authorizes this gateway and is never sent to Google. Client Authorization, User-Agent,
`client-metadata`, Google project/client headers and forwarding/IP headers are stripped. Runtime
requests use the truthful identity `apitoken-gemini-provider/1`. Auth Bot eligibility checks,
token refresh, health probes and generation share the same rustls transport family; no component
impersonates Gemini CLI or silently falls back from the profile proxy to host egress.

For every request the runtime:

- resolves opaque tenant-bound prompt affinity and prefers the same subscription;
- decrypts the selected project/proxy only in memory;
- obtains an access token with a per-profile single-flight mutex and 120-second expiry skew;
- wraps the native body for `v1internal:{generateContent,streamGenerateContent,countTokens}`;
- reconstructs an allowlisted native response and discards Code Assist wrapper fields, credits,
  private trace ids, unknown top-level fields and headers;
- caps response bodies and pending SSE frames at 32 MiB;
- reserves customer balance before upstream delivery and settles from native `usageMetadata`.

The model allowlist is local and price-catalog pinned. A configured id still needs a live smoke test
against every tier; Google can change private model availability independently.

## Failure and stream safety

| Upstream result | Profile action | Request action |
|---|---|---|
| first `401` | compare rejected bearer, single-flight refresh | retry once on the same profile |
| repeated `401` or `403` | auth quarantine | rotate to another profile |
| `429` | cooldown from `Retry-After` or `google.rpc.RetryInfo` | rotate without transport budget |
| network, `408`, `409`, `425`, `5xx`, malformed wrapper | short cooldown | bounded rotation |
| other deterministic `4xx` | keep profile healthy | return a synthetic native-shaped error |

Private error bodies are never returned verbatim. They may contain account, project, tier or private
endpoint details. Public errors retain only a generic Google-shaped status.

Streaming retry is allowed only before the first translated native SSE event. After delivery starts,
the request never jumps accounts. A downstream disconnect stops customer delivery but continues
bounded upstream drain to the final usage snapshot; shutdown tracks these tasks, aborts at the
deadline and settles the last known usage before exit.

Health probes call `loadCodeAssist` in `HEALTH_CHECK` mode and do not spend a model request. Empty
rosters may boot so Auth Bot can publish the first profile. Bad reloads leave the current pool intact.

## Operations

```bash
systemctl status claude-authbot.service claude-api-gemini.service
curl --fail http://127.0.0.1:8795/ready
curl --fail http://127.0.0.1:8794/ready
curl --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  https://gemini.api.apitoken.sale/v1beta/models
```

Expected safety properties are covered by tests for envelope AAD/key rotation, duplicate subject
rejection, hot roster reload, query/header credential stripping, Code Assist wrapper/credit removal,
bounded response parsing, quota/auth/transport rotation, concurrent 401 single-flight refresh,
affinity, split SSE translation, no post-event retry, disconnect drain and shutdown settlement.

Never deploy or enable the provider merely because local tests pass: OAuth verification, written
service authorization, live tier detection, current model availability and quota behavior are
external preconditions.
