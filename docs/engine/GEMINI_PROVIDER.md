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
                                      active/passive slots :8795/:8799
                                                    │
                                                    ▼
                         daily-cloudcode-pa.sandbox.googleapis.com
```

The Gemini runtime, router, OAuth credentials, proxy clients, health/cooling state and deployment
are independent from Claude and OpenAI. The three providers share only the fenced billing authority
and opaque affinity infrastructure.

Product decision (2026-08-02): Gemini is part of the target multi-provider product. B2C inherits
global 50% unless a Gemini provider/model override exists; exact model override wins (for example,
provider 60%, image model 55% → image uses 55%). B2B receives Gemini only through its own explicit
policy, OpenKeys only through an explicit 1:1 catalog entry, and service accounts can use every
runtime-capable Gemini model under `meter_only`. Activation is the global zero-downtime release in
`docs/commerce/MULTI-DISCOUNT.md`, not an independent Gemini client canary.

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

Tier-resolution implementation evidence reviewed on 2026-08-02 from Google's official Apache-2.0
Gemini CLI repository at commit
[`f47d6c6f7a1308d81f9f57acf7d279f0928c5249`](https://github.com/google-gemini/gemini-cli/commit/f47d6c6f7a1308d81f9f57acf7d279f0928c5249):
`packages/core/src/code_assist/setup.ts` selects `paidTier.id ?? currentTier.id` independently from
`paidTier.name ?? currentTier.name`, while `types.ts` defines `UserTierId` as an open string union.
Therefore this gateway treats only its exact reviewed IDs as stable authority, uses exact names as
additional conflict/legacy evidence, and never promotes an unknown tier from a `Pro`/`Ultra`
substring. Exact standalone legacy names remain accepted for already compatible sealed credentials;
they are not substring inference. This source explains client behavior; live generation remains
the publication gate.

## OAuth and publication flow

Auth Bot uses two separate installed-application OAuth transactions with PKCE. OAuth codes and
refresh tokens are client-bound, so this is deliberately not described or implemented as token
conversion: the official Gemini CLI client initializes Code Assist first, then the Antigravity
client receives its own consent for the same Google subject. No Gemini Developer API key is derived
from the subscription, and sellers do not create OAuth clients or operator Cloud projects.

Regression baseline (sanitized production audit, 2026-08-02): the first working subscription was
initialized by the Gemini CLI flow introduced in `c805f6f` and wire-calibrated in `b385278`, then
migrated to Antigravity by `241fce3`/`9a475f0`; the resulting profile has 93 completed runtime turns.
The second profile used direct Antigravity onboarding, passed OAuth/quota discovery, but its first
real generation stopped before execution with HTTP 503 and zero completed turns. Therefore the
automated flow preserves the calibrated Gemini CLI token form and verified userinfo identity as a
regression-tested bootstrap, while the final Antigravity transaction remains a fresh client-bound
consent rather than token conversion. Owned live evidence on 2026-08-02 showed a candidate account
whose legacy OAuth and verified userinfo succeeded while the legacy Code Assist surface returned
no project, `paidTier` or `currentTier`; therefore this surface is not an admission authority and
the Antigravity checks below remain decisive.

1. The seller submits only the account's dedicated proxy. When Auth Bot issues the proxy, OAuth
   starts immediately after issuance.
2. Auth Bot creates a 256-bit `state`, PKCE S256 verifier/challenge and twenty-minute legacy phase.
   The verifier, canonical proxy, Gemini CLI public client material and fixed
   `https://codeassist.google.com/authcode` redirect are sealed immediately in an
   XChaCha20-Poly1305 envelope bound to `state`; SQLite and its WAL retain no plaintext secret.
3. The seller opens the Google link in the prepared browser profile, then copies the one-use Gemini
   CLI code from Google's page into the no-store Auth Bot HTTPS form. Telegram receives only the
   two non-secret links. The server forces every server-side request through the dedicated proxy;
   browser egress remains a seller-enforced invariant.
4. Auth Bot claims the legacy `state` once, exchanges the code with the same client/redirect and
   performs verified userinfo. Before issuing a second consent it rejects an already published
   subject or incompatible legacy-profile proxy. The resulting legacy tokens never enter a roster;
   missing legacy Code Assist project/tier evidence neither admits nor rejects the subscription.
5. After the Antigravity consent, paid-plan admission uses the actual tier/project and reviewed
   tier evidence. A stable reviewed
   tier ID is authoritative even when Google changes its display name; an exact known name that
   points to another plan conflicts and fails closed. When Google returns both `paidTier` and
   `currentTier`, a reviewed mapping from either field is accepted, while contradictory reviewed
   mappings, unknown IDs without exact legacy-name evidence and substring-only matches are rejected.
   Before issuing the Antigravity consent, Auth Bot scans the encrypted roster: an existing
   Antigravity subject is reported as an already connected duplicate while its live refresh token
   is still safe; a legacy profile may continue only through its exact subject, canonical proxy and
   IPRoyal identity. Every final `unsupported_plan` branch emits
   only structural diagnostics (project and tier-field presence/classes plus allowed-tier count),
   never raw tier, project or identity.
6. A successful bootstrap is atomically replaced in SQLite by a fresh Antigravity `state`/PKCE
   phase and a rotated exact seller-job generation. Only the legacy Google subject and same proxy
   are carried forward, inside the new state-bound AEAD. Restart, replay, pause or job replacement
   cannot move an old callback into the new phase.
7. The seller completes Antigravity consent in the same Google account/browser/proxy. Google
   redirects to `http://localhost:51121/oauth-callback`; no local listener is required. The seller
   pastes the complete callback URL into the second HTTPS form. Its parser accepts only the exact
   HTTP localhost host, port and path, rejects credentials/fragments/OAuth errors and requires the
   callback `state` to match the hidden form state.
8. Auth Bot exchanges the final code through the same proxy, verifies that Google subject exactly
   matches the legacy proof, and resolves the Antigravity tier/project. Control-plane calls fall
   back only among the three reviewed Cloud Code hosts.
9. Admission sends exactly one tiny non-streaming `gemini-2.5-flash-lite` generation to the
   production-pinned sandbox endpoint using the runtime Antigravity wrapper and headers. It requires
   HTTP 2xx, a wrapped candidate and non-zero authoritative `usageMetadata`. `503`, malformed JSON,
   missing usage and ambiguous transport return `generation_unavailable`; the paid request is never
   replayed automatically, no credential is published and seller payout does not complete.
10. Only after generation acceptance is the final Antigravity credential sealed and published
   atomically. After waiting for the publication lock, Auth Bot re-checks the exact seller-job
   generation immediately before the roster write; a cancelled, rewound or replaced job fails
   closed. A legacy roster profile migrates one-way in place, preserving opaque id, roster bytes and
   IPRoyal lifecycle; reverse migration or proxy mismatch fails. The runtime discovers the profile
   on its health loop without restart. A direct Antigravity callback created before this rollout
   remains decodable for deployment compatibility and retains the former in-place reauthorization
   rule because its already-issued consent may have invalidated the old token.

Auth Bot's two token exchanges, userinfo, Antigravity `loadCodeAssist`, onboarding and generation
acceptance use the same bounded Node helper source as the runtime through the seller's dedicated
authenticated proxy. The final wire identity is pinned to Antigravity 2.2.1: runtime/control calls use
`antigravity/hub/2.2.1 darwin/arm64`, onboarding appends
`google-api-nodejs-client/10.3.0`, and token exchange uses `Go-http-client/2.0`. There is no ambient
proxy path or arbitrary OAuth client.

Failure attribution is three-way and secret-free: exhausted CONNECT/TLS recovery is
`transport_unavailable`; an established transport followed by temporary HTTP or malformed Google
control-plane data is `temporary_upstream`; a final generation 503, malformed/missing usage response
or ambiguous one-shot generation transport is `generation_unavailable`. Only the first class is
evidence about the transport path, and even it does not by itself prove that the proxy allocation is
dead. Telegram never asks the seller to replace a proxy for the latter two classes.

Legacy bootstrap scopes:

```text
https://www.googleapis.com/auth/cloud-platform
https://www.googleapis.com/auth/userinfo.email
https://www.googleapis.com/auth/userinfo.profile
```

Final Antigravity scopes:

```text
https://www.googleapis.com/auth/cloud-platform
https://www.googleapis.com/auth/userinfo.email
https://www.googleapis.com/auth/userinfo.profile
https://www.googleapis.com/auth/cclog
https://www.googleapis.com/auth/experimentsandconfigs
```

Both installed-app client id/secret pairs are public upstream Google application metadata, not
operator secrets. Account access tokens, refresh tokens, PKCE verifiers, identity, proxy credentials
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

### Transactional manual proxy replacement

Changing a published profile's proxy by repeating OAuth is intentionally rejected: Google can
invalidate the old refresh token before the proxy mismatch is detected. Use the Auth Bot operator
commands instead. They read the replacement proxy only from stdin, retain the old credential as a
private encrypted rollback envelope, atomically replace the active envelope, and never print either
proxy. Stop Auth Bot around every operator command so it cannot race a seller publication; the
Gemini runtime stays online and reloads the replacement on its health loop.

```bash
profile_id=gemini_oauth_000002
set -a
. /srv/claude-api/data/authbot.env
set +a
systemctl stop claude-authbot.service
read -r -s -p 'Replacement proxy: ' GEMINI_REPLACEMENT_PROXY
printf '%s\n' "$GEMINI_REPLACEMENT_PROXY" \
  | runuser -u deploy -p -- /srv/claude-api/releases/current/authbot \
      gemini-proxy-stage "$profile_id"
unset GEMINI_REPLACEMENT_PROXY
systemctl start claude-authbot.service
```

After the runtime reloads, require a successful admin-only exact-profile `countTokens` and one
non-stream generation with non-zero immutable usage/cost evidence. A normal request without
`x-apitoken-calibration-profile` is not proof because it may spill to another profile. If the exact
test succeeds, stop Auth Bot and run `gemini-proxy-commit <profile_id>` under the same sourced env
and `runuser` boundary; otherwise run `gemini-proxy-rollback <profile_id>`. Start Auth Bot again in
either case. `stage` refuses a second pending replacement and reuse of another profile's canonical
proxy. A manual replacement clears the IPRoyal order id because Auth Bot cannot extend a proxy it
did not issue.

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
CLAUDE_API_GEMINI_MODELS=gemini-3.1-flash-image,gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite
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

- resolves opaque tenant-bound prompt affinity and always keeps a routable resolved conversation on
  the same subscription, regardless of local in-flight depth. Independent unbound requests dispatch
  immediately and use in-flight only to spread load; there is no per-profile concurrency cap or
  local wait/reject path. Independent sessions with the same large
  system/tools cache root deliberately seed two competitive subscriptions before a warm copy is
  preferred, preventing one common prefix from collapsing the fleet onto its first home;
- for text generation, derives a UUID-shaped upstream `request.sessionId` from the keyed affinity
  lineage: it stays stable across a growing conversation, changes for another explicit session or
  tenant, and never exposes a raw tenant/session value; one `agent-<uuid>` request id is created
  before rotation and reused for every retry of that customer request. The image route keeps public
  affinity but uses its stateless first-party identity described below;
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
- caps documented inline-media requests at 20 MiB and generated-image response bodies/pending
  stream frames at 64 MiB;
- periodically calls Antigravity `v1internal:fetchAvailableModels`, keeps a sanitized per-model
  `remainingFraction`/`resetTime` catalogue, and cools only the exhausted model/profile pair until
  Google's reset time;
- independently calls `v1internal:retrieveUserQuotaSummary` and accepts only the exact
  `gemini-5h` and `gemini-weekly` buckets. Every successful generation with terminal usage,
  including admin traffic, creates one immutable event with the internal request id, opaque
  profile, paid plan, public model, tariff schedule, all input/audio/cache/output/thinking/image/
  tool/search facts and every official API-cost leg in integer nanoUSD. The same authority
  transaction advances cumulative profile spend; missing terminal usage never fabricates an
  event. A bounded FIFO retains transient failures, prevents a quota poll from overtaking paid
  evidence, quarantines only semantic replay conflicts and is drained on shutdown;
- stores quota observations and estimator state in the plan-scoped exact authority keyed by
  `profile + paid plan + bucket + duration`. Provider decimals are fixed-point `10^-8`, while the
  actual lexical endpoint resolution is stored separately. The first snapshot is an anchor; the
  first later interval with positive fraction movement and positive settled spend immediately
  publishes `capacity = 100000000 * ΣΔspend / ΣΔused`. Low/high use the combined resolution of both
  interval endpoints; high is `null` when true movement cannot be bounded above. Quota movement may
  wait one snapshot for settlement, but repeated quota-only movement becomes unattributed and is
  excluded. Reset/rolling rollover, jitter, stale points, checked overflow and estimator-version
  replay are explicit. There is no subscription nominal, prior, EMA, WLS, float money arithmetic
  or use of foreign-provider `3p-*` buckets. The 5h and weekly histories remain independent and
  only like-for-like paid plans may be pooled;
- immediately dispatches every paid-profile request with no local concurrency ceiling. For unbound
  work, fresh
  quota evidence wins over stale evidence, then current in-flight load spreads the burst, and only
  coarse 10-point quota buckets above 50% used steer near the wall; an atomic cursor rotates equal
  candidates. Never-arrived quota evidence is neutral, while stale evidence remains fail-open but
  cannot look like an artificial 100%-remaining winner. A small deterministic per-profile reserve
  prevents synchronized draining, but remains soft: if all eligible profiles are below reserve,
  the final working subscription continues serving until Google reports an explicit zero;
- records generation-specific failure streaks, last success/failure timestamps and an exponential
  per-model cooldown. HTTP 5xx/malformed generation failures therefore degrade only that model;
  proxy/network/token-refresh failures still cool the complete profile. `countTokens` remains a
  quota-free diagnostic and cannot falsely rehabilitate generation health;
- reserves customer balance before upstream delivery and settles from native `usageMetadata`.
  A metered non-stream success without authoritative non-zero usage is withheld and refunded; once
  streaming bytes have been delivered, missing final usage settles the conservative hold and emits
  an operational counter instead of inventing a usage event or granting a free request.

### What the Gemini API-dollar estimate means

Antigravity's official plan documentation says that “the rate limits are correlated with the
amount of work done by the agent, which can differ from prompt to prompt.” Consequently, no honest
conversion can label Google AI Pro or Ultra as a fixed number of Developer API dollars. The gateway
reports three different quantities instead:

- `cap_usd`: the cumulative official-API-dollar equivalent of the workload mix actually observed;
- `low_usd` / `high_usd`: the accumulated per-interval workload envelope expanded by the actual
  decimal resolution of both endpoint snapshots; `high_usd=null` means the observation is too
  coarse for a finite upper bound;
- `confidence`: sample maturity × workload-envelope stability × fraction-resolution quality.

The old pre-plan Gemini estimator and its 2026-07-31 observations are deliberately not copied into
this authority: their durable identity did not prove paid plan, lexical resolution or immutable
request attribution. The exact authority therefore starts from new live evidence instead of
laundering a plausible historical number into a trusted capacity. A future traffic mix can still
move the realized blend, and Google can change quota policy; immutable observations let estimator
upgrades replay the same facts. The controlled procedure is
`docs/ops/GEMINI_CALIBRATION.md`. Official source: <https://antigravity.google/docs/plans>.

The model allowlist is local and price-catalog pinned. The default list contains six text models
whose non-stream, native stream and token-count paths were reconfirmed against the production
Google AI Pro profile on 2026-07-31, plus the separately routed Nano Banana 2 image model:
`gemini-3.1-flash-image`, `gemini-3.6-flash`, `gemini-3.5-flash`,
`gemini-3.1-pro-preview`, `gemini-3.1-flash-lite`, `gemini-2.5-flash`, and
`gemini-2.5-flash-lite`. `gemini-2.5-pro` is deliberately not published: it is absent from the
official Antigravity reasoning-model table, and its residual quota bucket does not produce a
working generation route. Private tier ids are never public model names, while
unreviewed agent/foreign-provider ids have no honest public model mapping.
A Developer API price entry proves only that the gateway can meter a model; it does not prove that
an Antigravity subscription can serve it. Publication additionally requires an official
Antigravity model contract, an exact canonical-to-private route and live generation evidence.
A configured id still needs a live smoke test against every tier because Google can change private
model availability independently. The production systemd argv pins this calibrated seven-model
set after shared env files, so a stale
`config.env` cannot silently re-enable Developer-API-only models on the subscription runtime.

### Nano Banana 2 image route and accounting

Antigravity's official model page identifies Nano Banana 2 as the non-customizable generative
image tool, and the authenticated Google AI Pro catalogue exposes an independent
`gemini-3.1-flash-image` quota bucket. The public route keeps the native Gemini
`generateContent`/`streamGenerateContent`/`countTokens` envelope and returns generated media as
`candidates[].content.parts[].inlineData`; images are never written to server disk.

Image requests use `https://cloudcode-pa.googleapis.com`, the production endpoint selected by the
Antigravity language server and independent working implementations. The configured sandbox host
continues to serve the live-verified text surface, but its advertised image quota row is not proof
of an image generation backend: valid image requests there return a generic 503. Explicit literal
loopback mocks retain their configured origin.

For Antigravity the wrapper uses the complete image identity rather than mixing it with an agent
turn: `requestType=image_gen`, `requestId=image_gen/<unix-ms>/<uuid>/12`, no private `sessionId`,
`candidateCount=1`, and `responseModalities=[TEXT,IMAGE]`. The public affinity binding is still
used to select a warm subscription, but it is never sent as an unsupported image session. Missing
image controls become explicit `aspectRatio=1:1` and `imageSize=1K`; the live-verified subscription
sizes are `1K`, `2K`, and `4K`, and the accepted ratios are `1:1`, `1:4`, `1:8`, `2:3`, `3:2`, `3:4`,
`4:1`, `4:3`, `4:5`, `5:4`, `8:1`, `9:16`, `16:9`, and `21:9`. Up to 14 inline reference images
are accepted, using only the documented PNG, JPEG, WEBP, HEIC and HEIF MIME types with valid base64.
Project-scoped `fileData`, system instructions, tools, structured output, multiple candidates,
non-equivalent response-modalities overrides and private-route thinking controls fail closed rather
than being silently dropped. Google's Developer API also documents `0.5K`, but the Antigravity
subscription endpoint returns `INVALID_ARGUMENT` for the same native spelling; the gateway rejects
it locally before balance reservation until that private capability is live-verified.

Official paid-standard equivalence is pinned as integer nanoUSD: `$0.50/M` input tokens,
`$3/M` text plus thinking output tokens, and `$60/M` generated-image tokens. Authoritative
settlement splits `usageMetadata.candidatesTokensDetails[modality=IMAGE]` from ordinary candidate
tokens, then charges the two output SKUs separately. The private production response can omit that
detail while retaining only aggregate `candidatesTokenCount`; when it actually delivered
`inlineData`, settlement splits out the official fixed token count for the requested size and
prices the aggregate remainder as text/thinking. An explicit provider modality split always wins,
and a refusal/text-only response never receives an image charge. Preflight reserves the complete
requested image without silently lowering its quality: 1,120 image tokens for 1K (`$0.0672`),
1,680 for 2K (`$0.1008`), and 2,520 for 4K (`$0.1512`), plus bounded text/input and grounding. A
stream that delivered bytes but never supplied final usage settles the conservative hold without
inventing a token event.

For money, the paid-tier pricing table is authoritative. On 2026-07-31 Google's separate image
generation resolution table showed different 2K/4K processing-token figures; those describe the
generation surface but do not override the pricing page's explicit billable-token counts and USD
equivalents. The gateway therefore pins the pricing SKUs above and prefers an explicit provider
modality breakdown whenever one is returned. The same official pricing page lists the currently
unexposed 0.5K Developer API SKU as 747 image tokens (`$0.04482`); that price is research evidence,
not proof that the Antigravity subscription transport can serve the resolution.

### Video is a separate provider surface

Neither the official Antigravity model table nor the authenticated subscription catalogue exposes
Gemini Omni Flash or any `veo-*` identity. Google AI Pro/Ultra access inside the Gemini app or Flow
does not grant a Code Assist OAuth API route, so this gateway does not publish a fake video model.
Video requires a separately configured Gemini Developer API key/Cloud Billing credential and a
long-running file/operation lifecycle before it can be admitted to production.

The reviewed official paid-tier accounting basis for that future provider is:

| Developer API model | Official output accounting |
|---|---:|
| `gemini-omni-flash-preview` | `$17.50/M` video tokens at 5,792 tokens/second of 720p = `$0.10136/second` (documented as approximately `$0.10/second`); input `$1.50/M`, text output `$9/M` |
| `veo-3.1-generate-preview` | `$0.40/second` at 720p/1080p; `$0.60/second` at 4K |
| `veo-3.1-fast-generate-preview` | `$0.10/second` at 720p; `$0.12/second` at 1080p; `$0.30/second` at 4K |
| `veo-3.1-lite-generate-preview` | `$0.05/second` at 720p; `$0.08/second` at 1080p; 4K unsupported |

Veo is charged only for successfully generated video. These prices are not folded into the
subscription 5h/weekly calibration because no subscription video quota or request path exists.

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
- Nano Banana 2 generation, sizes, ratios and input formats:
  <https://ai.google.dev/gemini-api/docs/image-generation> and
  <https://ai.google.dev/gemini-api/docs/models/gemini-3.1-flash-image>;
- Gemini video generation contracts and lifecycle: <https://ai.google.dev/gemini-api/docs/video>;
- paid standard prices: <https://ai.google.dev/gemini-api/docs/pricing>;
- thinking-level defaults and supported values: <https://ai.google.dev/gemini-api/docs/thinking#thinking-levels>;
- REST schema/discovery revision `20260729`:
  <https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta>.

## Failure and stream safety

| Condition/result | Profile action | Request action |
|---|---|---|
| first `401` | compare rejected bearer, single-flight refresh | retry once on the same profile |
| repeated `401` or `403` | auth quarantine | rotate to another profile |
| `429` | cool only that model/profile from `Retry-After`, `google.rpc.RetryInfo` or quota reset | rotate without transport budget |
| network/token refresh, `408`, `409`, `425` | short profile cooldown | bounded rotation |
| generation `5xx` or malformed wrapper/stream | exponential model cooldown | bounded rotation without disabling other models |
| other deterministic `4xx` | keep profile healthy | return a synthetic native-shaped error |
| high local in-flight on every eligible profile | keep bindings and profiles healthy | dispatch immediately; use in-flight only to choose the least-loaded unbound profile |

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
A successful probe/turn clears stale global auth/transport quarantine, including a concurrent stale-
token race, but never clears a generation model's independent 429 cooling. A fresh official quota
snapshot is authoritative for catalogued models; a stale/missing bucket fails open, while an explicit
zero blocks that model until its parsed RFC3339 reset. Legacy Gemini CLI profiles keep their former
`HEALTH_CHECK`, `retrieveUserQuota`, `request.session_id`, `user_prompt_id` and Google library
headers so an existing sealed roster remains usable during migration.

## Universal chat surface

`POST /v1/chat/completions` (stage 3.3 of `docs/engine/UNIFIED_ROUTER.md`) is served by the adapter
in `crates/forward/src/gemini/chat.rs`. It translates the OpenAI chat request into a
`GenerateContentRequest` (system/developer → `systemInstruction`, same-role merge of `contents`,
tool history ↔ `functionCall`/`functionResponse` parts with the tool name recovered from
`tool_call_id`, `tool_choice` → `toolConfig.functionCallingConfig`, generation knobs →
`generationConfig` with a 4096 `maxOutputTokens` default), then issues an internal request to
`/v1beta/models/{model}:generateContent` or `:streamGenerateContent?alt=sse` handled by the shared
`gemini_api` path — admission, reserve, affinity, rotation, Code Assist wrapper, metering and
settlement are identical to the native route. Responses are translated outside that path:
`candidates[0]` parts become chat content/`tool_calls` (synthetic `callu_<name>[_N]` ids),
`usageMetadata` becomes OpenAI usage (completion = candidates + thoughts tokens), and the data-only
SSE stream becomes `chat.completion.chunk` frames with a final usage chunk on EOF when
`stream_options.include_usage` is set.

Code Assist compatibility is normalized at the adapter boundary. Its private `Schema` parser rejects
three legal JSON Schema 2020-12 keywords emitted by common OpenAI-compatible clients: `$schema` and
numeric `exclusiveMinimum`/`exclusiveMaximum`. Chat and Responses tool declarations, Messages
`input_schema`, and Chat/Responses structured-output schemas recursively remove only those schema
keywords. Property names with the same spelling under `properties` remain intact.

Replayed tool history is also stateless. Gemini thinking models require a `thoughtSignature` on a
replayed model `functionCall` part, but OpenAI/Responses/Messages clients do not preserve Gemini's
opaque provider signature. Every reconstructed call therefore carries Google's accepted context-
engineering marker `context_engineering_is_the_way_to_go`. The exact marker is private-wire input
only: actual signatures returned by Gemini are still dropped under UNIFIED_ROUTER decision 4, public
tool-call ids and response shapes are unchanged, and no signature state is stored by the gateway.

Two deliberate differences from the Anthropic-plane adapter: the capability matrix is closed at the
top level — an unknown request field is rejected with `400 unsupported_parameter` instead of being
proxied, because the Code Assist wrapper would silently drop it; and the native `400
API_KEY_INVALID` answer is re-mapped to `401 authentication_error`, which is what OpenAI clients
expect for a bad key. Error bodies keep the OpenAI envelope with the original HTTP status (402
included) and `Retry-After`.

Stage 3.4a adds multimodal input and structured output to this surface. `image_url` parts of user
messages become `inlineData` parts; only `data:` URLs are accepted because this plane has no
outbound fetch for remote images, so an `http(s)` image URL is rejected with `400 invalid_request`
and a non-default `detail` with `400 unsupported_parameter`. `response_format` is translated into
`generationConfig`: `json_object` and `json_schema` both set `responseMimeType:
application/json`, and `json_schema` additionally sets `responseSchema`, dropping the OpenAI
`name`/`strict` wrapper and applying the same Code Assist schema sanitizer used for tools.

Stage 3.4b adds reasoning. `reasoning_effort` (`minimal`/`low`/`medium`/`high`; `null` or absent
turns it off) is translated into `generationConfig.thinkingConfig` — `thinkingLevel` is proxied
verbatim because the plane itself maps the level into the private wire model id, and
`includeThoughts: true` opts into thought parts; any other non-null value is rejected with `400
invalid_request` (`param: reasoning_effort`). In responses, thought parts (`"thought": true`) go
to the OpenAI `reasoning_content` extension instead of leaking into content: non-stream they are
concatenated into `message.reasoning_content` (present only when non-empty), in the SSE stream
each thought part becomes a `{"delta": {"reasoning_content": ...}}` chunk ahead of content
deltas in upstream order. `thoughtSignature` is always dropped — universal lanes never expose
signatures (decision 4).

## Operations

```bash
systemctl status claude-authbot.service 'claude-api-gemini@*.service'
for port in 8795 8799; do curl -sS -o /dev/null -w "$port %{http_code}\\n" \
  "http://127.0.0.1:$port/ready" || true; done
curl --fail http://127.0.0.1:8794/ready
curl -H 'x-api-key: <control-or-readonly-key>' http://127.0.0.1:8794/gemini-subs
curl --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  https://gemini.api.apitoken.sale/v1beta/models
```

Steady state is exactly one active, ready, enabled slot and one stopped, disabled slot. Operators and
clients use stable origin 8794 (or the public hostname), never a runtime port. During deploy the
candidate must pass exact-release/provider/readiness gates before the old slot returns 503 and stops
accepting new requests; its established SSE requests may finish during bounded asynchronous drain.

`/gemini-subs` is read-only-key protected and exposes opaque profile ids plus a bounded operator
email hint (at most four local-part characters, never the domain), model availability,
sanitized quota/cooling timestamps, independent 5h/weekly fractions and workload-dependent
official-API-dollar blend/remaining/envelope/confidence plus the exact spend/fraction evidence,
exact-authority availability, bounded FIFO pending/dropped/persistence diagnostics and the newest
512 immutable turn vectors for controlled attribution, generation failure
streak/timestamps/classes, low-cardinality transport/backend/malformed/stream-start counters,
affinity counters, missing-usage count and pinned HTTPS/Undici transport versions/hashes. Unknown
capacity stays JSON `null`; measured fleet totals include only currently routable profiles with
evidence and publish canonical decimal `*_nano` strings beside display-only USD compatibility
fields. Non-authenticated/account-cooling/all-model-cooling profiles retain quota/reset evidence
for diagnosis but their per-profile and fleet saleable API-dollar fields stay `null`. The response
marks this explicitly as `realized_workload_api_equivalent` with
`fixed_subscription_nominal=false`. It also carries the reviewed non-secret paid-plan identity
(`google_ai_pro|google_ai_ultra|code_assist_standard|code_assist_enterprise|workspace_ai_ultra`)
for like-for-like admin sales aggregation, the paid-tier `metering::gemini` conversion catalogue,
and the exact public-model → private quota-bucket mapping. `remaining_amount` is a decimal string;
when Google publishes only a fraction, consumers must leave token amount unknown. Subject, full
email, domain, project, private tier, proxy and OAuth material are never serialized. Caddy maps the same endpoint into the unified
`admin.apitoken.sale` subscription and calculator pages through stable origin `127.0.0.1:8794`.

Expected safety properties are covered by tests for envelope AAD/key rotation, duplicate subject
rejection, in-place legacy-to-Antigravity migration with proxy/lifecycle preservation, hot roster
reload, query/header credential stripping, Code Assist wrapper/credit removal, bounded response
parsing, quota/auth/transport rotation, concurrent 401 single-flight refresh and sticky affinity,
two-copy shared-root warming, stale-quota ordering, 10,000 immediate leases and concurrent fan-out
that reaches upstream without a release event, split SSE translation,
no post-event retry, disconnect drain and shutdown settlement.
