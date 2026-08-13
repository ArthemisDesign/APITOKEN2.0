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
                     per-model reviewed Cloud Code origin
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

The immutable pricing authority uses internal provider id `google`. Frozen capability generation 3
pins the original eight tariff-backed models from `crates/metering/src/gemini.rs`; immutable
generation 4 adds `gemini-3-flash-preview` but remains a rejected historical artifact because its
old public-wire gate failed with 404 and no usage. It must never be materialized or activated.
Admitted generation 5 repeats the reviewed model set under a new digest after fresh runner SHA
`cc7e5beb…` completed all 22 paid Pro+Ultra turns: every thinking level, incremental SSE,
profile-local `write → prime → read` cache attribution, fresh/replayed exact PCM WAV accounting and
forced function calls. The sanitized evidence is
[`research/GEMINI_3_FLASH_PUBLICATION_LIVE_ACCEPTANCE.md`](../../research/GEMINI_3_FLASH_PUBLICATION_LIVE_ACCEPTANCE.md).
Stage 5 now materializes the main product on generation 5; the contemporaneous OpenKeys catalog
deliberately remains Anthropic/OpenAI until a separate reviewed 1:1 OpenKeys generation enables
Gemini.

Gemini 3.6 Flash has an official effective-dated exception to the otherwise static text cards.
Through `2026-12-31T23:59:59Z`, input/audio input is `750 nanoUSD/token` ($0.75/M), cached
input/audio input is `75 nanoUSD/token` ($0.075/M), and candidate+thinking output is
`3,750 nanoUSD/token` ($3.75/M). At `2027-01-01T00:00:00Z` (`1798761600`) the compiled schedule
switches atomically to `1,500 / 150 / 7,500 nanoUSD/token` ($1.50 / $0.15 / $7.50 per M). Gemini 3
Search remains `14,000,000 nanoUSD` per provider-reported query on both sides of the cutoff; the
shared free-query allowance is not treated as guaranteed per-request customer pricing. The family
`google/gemini/gemini-3.6-flash` is permanently `seed_safe=false`: a zero-time hot row would
collapse its two epochs. A live correction must use the append-only current+future override pair
and readback procedure in `docs/engine/CONTROL_API.md`.

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
Therefore this gateway treats its exact reviewed IDs as the stable authority and the paid tier as
the entitlement that wins a disagreement, uses exact names only where the ID carries no reviewed
meaning, and never promotes an unknown tier from a `Pro`/`Ultra` substring. Exact reviewed names
(including the Google One and post-rename spellings) remain accepted evidence; they are not
substring inference. This source explains client behavior; live generation remains
the publication gate. On 2026-08-03 a live seller with a real subscription was rejected as
`conflicting_paid_and_current_tiers` (`project=present paid=known_id_name_match
current=known_id_name_drift`), which is the production evidence behind the ID/paid-tier authority
above.

## OAuth and publication flow

Auth Bot uses one installed-application OAuth transaction with PKCE, against the Antigravity
client. No Gemini Developer API key is derived from the subscription, and sellers do not create
OAuth clients or operator Cloud projects.

The former two-transaction shape (official Gemini CLI bootstrap, then Antigravity consent) is
retired. Regression baseline (sanitized production audit, 2026-08-02) had justified it: the first
working subscription was initialized by the Gemini CLI flow (`c805f6f`, wire-calibrated in
`b385278`) and migrated to Antigravity by `241fce3`/`9a475f0`, while an early direct-Antigravity
onboarding stopped at HTTP 503. Owned live evidence since then shows the bootstrap proving nothing
the Antigravity consent does not prove on its own — its Code Assist surface legitimately returns
no project, `paidTier` or `currentTier`, so it was never an admission authority — while costing the
seller a second `select_account consent` screen. Every extra consent in one browser profile is
another chance to confirm the wrong Google account and annul a token that was already paid for.
Admission authority is unchanged and still decisive: verified userinfo, exact reviewed tier,
resolved project, matching roster invariants and one real generation.

1. The seller submits only the account's dedicated proxy. When Auth Bot issues the proxy, OAuth
   starts immediately after issuance.
2. Auth Bot creates a 256-bit `state`, PKCE S256 verifier/challenge and a twenty-minute phase. The
   verifier, canonical proxy, Antigravity public client material and the fixed
   `http://localhost:51121/oauth-callback` redirect are sealed immediately in an
   XChaCha20-Poly1305 envelope bound to `state`; SQLite and its WAL retain no plaintext secret.
3. Telegram delivers the authorization URL as an ordinary hyperlink, never as an inline URL button:
   a Telegram button opens the client's built-in browser, which is a different profile and a
   different egress than the account was created on. The seller opens it in the prepared
   anti-detect profile. The server forces every server-side request through the dedicated proxy;
   browser egress remains a seller-enforced invariant.
4. The seller completes the consent. Google redirects to `http://localhost:51121/oauth-callback`;
   no local listener is required, and the page failing to load is expected. The seller pastes the
   complete callback URL into the Auth Bot HTTPS form. Its parser accepts only the exact HTTP
   localhost host, port and path, rejects credentials/fragments/OAuth errors and requires the
   callback `state` to match the hidden form state. Successful claim processing is detached before
   the handler returns `202 Accepted`; terminal failure cleanup is detached before its bounded
   `4xx`/`503` page is returned as well. Closing the browser cannot cancel exchange, failure
   cleanup or the eventual Telegram result.
5. Auth Bot claims the `state` once, exchanges the code with the same client/redirect and performs
   verified userinfo. Paid-plan admission then uses the actual tier/project and reviewed
   tier evidence. A stable reviewed tier ID is the single authority: Google rewrites display names
   (Google One branding, Antigravity wording) without touching the ID, so a name that maps to
   another reviewed plan is treated as drift and journalled, not as a second entitlement. When
   Google returns both `paidTier` and `currentTier` and their reviewed mappings disagree, the paid
   entitlement wins — this mirrors the official client's `paidTier.id ?? currentTier.id` — because
   Antigravity onboarding routinely leaves `currentTier` on a different tier and rejecting the pair
   told sellers with a real subscription that no subscription exists. An unreviewed ID still falls
   back to an exact reviewed name; substring-only matches and evidence that is unreviewed in both
   fields are rejected. Every `unsupported_plan` branch emits
   only structural diagnostics (project and tier-field presence/classes plus allowed-tier count),
   never raw tier, project or identity. `AUTH_BOT_GEMINI_TIER_EVIDENCE=1` is an opt-in operator
   switch that additionally journals bounded raw tier IDs/names of the `loadCodeAssist`
   response, so a genuinely new Google tier can be identified without shipping a build; it stays off
   by default. Roster invariants — an already published Antigravity subject is a reauthorization in
   place, a legacy profile migrates one-way only through its exact subject, canonical proxy and
   IPRoyal identity, and a proxy already bound to another profile is refused — are enforced at
   publication, which is the only moment they can be enforced atomically.
6. **The consented token family is sealed and recorded before anything that can fail.** Consent
   already annulled any previous refresh token for this Google subject, so those tokens are the
   only copy of a subscription the seller was paid for; a tier that Google has not finished
   provisioning, a held account, an unhappy surface or a throttled CONNECT must never destroy them.
   They are parked in an AEAD envelope bound to that seller's chat (never in `profiles.json`, never
   a completed payout), fenced to the exact seller-job generation, kept on record for seven days.
   Tier/project resolved later are re-sealed into the same envelope.
7. Admission sends one tiny non-streaming `gemini-2.5-flash-lite` generation using the runtime
   Antigravity wrapper and headers, **first to the production endpoint the gateway actually serves
   customer traffic from**, so admission is evidence about the surface that will carry the
   subscription. It requires HTTP 2xx, a wrapped candidate and non-zero authoritative
   `usageMetadata`. An access rejection made before the model ran (`403`/`404`) repeats the same
   probe once on the reviewed sandbox endpoint, because a subscription can be admitted on one host
   and refused on the other and nothing was generated or billed yet. `503`, malformed JSON,
   missing usage and ambiguous transport return `generation_unavailable` without trying another
   host; a generation that did run is never replayed automatically, no credential is published and
   seller payout does not complete. A CONNECT-stage transport refusal is a different fact: the
   tunnel never reached Google, so no paid generation exists to protect, and exactly the bounded
   pre-target classes (`proxy_throttle`, `proxy_timeout`, `proxy_upstream`, `proxy_connect`,
   `proxy_eof`, `tls`) are retried on the same 0/2/7/17/37-second schedule as the token exchange.
   An account-level rejection is identical on every host and therefore stops the probe immediately:
   `VALIDATION_REQUIRED` / "Verify your account to continue" becomes `account_validation_required`,
   whose seller instruction is to finish Google's own account verification in
   the same browser profile and proxy — retrying cannot clear it, and a working `gemini.google.com`
   session does not, because this check is separate. Google returns the account's personal
   verification link in `error.details[].metadata.validation_url` while `message` carries only the
   sentence; Auth Bot forwards that link to the seller as copyable text, fail-closed on anything but
   an `https://accounts.google.com/` URL so the message cannot be turned into a phishing vector. The
   journal carries the HTTP status, the surface and Google's enum fields (`error.status`,
   `error.details[].reason`); the free-form `error.message` can name the project or account and is
   printed only under `AUTH_BOT_GEMINI_TIER_EVIDENCE=1`.
8. **A recorded account that has not been admitted is retried automatically: one acceptance
   generation every five minutes for twenty-four hours after consent.** Every attempt runs the
   identical code path — refresh the bearer over the same egress if it aged out, resolve tier and
   project if a previous attempt could not, then exactly one probe — so a late admission rests on
   the same evidence as an immediate one. The schedule lives in SQLite, so a restart neither loses
   it nor fires a burst; claiming an attempt advances the next one, so the seller's button and the
   sweep cannot double-charge one account. A success publishes and settles the deal, payout
   included, through the callback's own code path. Only a verdict no retry can change
   (`authorization`, `account_mismatch`, `duplicate_account`, `duplicate_proxy`,
   `migration_proxy_mismatch`, `stale_handoff`) ends the window early. When the window closes, the
   seller and the admins are told once, probing stops and **the credential stays on record**.
   The seller's `gemini:verified` button remains an immediate manual attempt at any time.
9. Only after generation acceptance is the Antigravity credential sealed and published
   atomically, and the parked copy cleared. After waiting for the publication lock, Auth Bot
   re-checks the exact seller-job generation immediately before the roster write; a cancelled,
   rewound or replaced job fails
   closed. A legacy roster profile migrates one-way in place, preserving opaque id, roster bytes and
   IPRoyal lifecycle; reverse migration or proxy mismatch fails. The runtime discovers the profile
   on its health loop without restart. A two-phase callback created before this rollout remains
   decodable for deployment compatibility: a legacy-bootstrap session sealed by the previous binary
   still transitions to its Antigravity phase instead of stranding a seller mid-flow.

`/cancel` is the explicit full-restart boundary for a Gemini seller handoff. Under the same
publication/terminal lock it first decrypts or resolves the exact pinned egress, atomically deletes
the pending or already-claimed capability and rotates the seller-job generation, then aborts the
old local worker and starts again with a fresh state and PKCE verifier for a new Antigravity
consent.
The generation check immediately before publication makes an old task harmless even if cancellation
arrives while Google I/O is in progress. A malformed/missing egress still fences the old generation
instead of leaving `processing` behind; seller-owned egress is requested again, while fixed egress
fails closed for operator repair. On Auth Bot startup every persisted `processing` session follows
the same restart path. Its one-use Google code is never replayed after a crash or deployment.

Auth Bot's two token exchanges, userinfo, Antigravity `loadCodeAssist`, onboarding and generation
acceptance use the same bounded Node helper source as the runtime through the seller's dedicated
authenticated proxy. The final wire identity is pinned to Antigravity 2.2.1: runtime/control calls use
`antigravity/hub/2.2.1 darwin/arm64`, onboarding appends
`google-api-nodejs-client/10.3.0`, and token exchange uses `Go-http-client/2.0`. There is no ambient
proxy path or arbitrary OAuth client.

Failure attribution is four-way and secret-free: exhausted CONNECT/TLS recovery is
`transport_unavailable`; an established transport followed by temporary HTTP or malformed Google
control-plane data is `temporary_upstream`; a final generation 503, malformed/missing usage response
or ambiguous one-shot generation transport is `generation_unavailable`; and a Google account that is
admitted but held for its own verification is `account_validation_required`. Only the first class is
evidence about the transport path, and even it does not by itself prove that the proxy allocation is
dead. Telegram never asks the seller to replace a proxy for the other three classes.

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
CLAUDE_API_GEMINI_MODELS=gemini-3.1-flash-image,gemini-3.6-flash,gemini-3.5-flash,gemini-3-flash-preview,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite
CLAUDE_API_GEMINI_QUOTA_RESERVE=0.05
CLAUDE_API_GEMINI_QUOTA_RESERVE_JITTER=0.01
```

## Liveness versus duration

A long answer is not a broken one, and time cannot tell the two apart. Nothing on the customer path
is ended by a clock. A request ends early only on a fact: TCP keepalive probes (60s, set by the Node
helper on every request) report the peer gone, the client disconnects and cancel-on-drop tears the
upstream socket down, or the upstream closes the response. None of the three depends on how long the
request has been running. The blast radius of an upstream that hangs while its socket stays healthy
is bounded by the inflight caps, which limit concurrency rather than duration — that is the shape
the risk actually has.

- `CLAUDE_API_GEMINI_READ_TIMEOUT_SECS` (default 120, range 15–600) — token refresh, quota and
  catalogue calls only. Short on purpose: a wedged auxiliary call must rotate the profile out
  quickly, and none of these is a customer request.
- `CLAUDE_API_GEMINI_GENERATION_IDLE_SECS` (default 0 = no deadline, range 0–3600) — customer
  generation. Any non-zero value is a bet on how long a model may think, and some customer task
  always exceeds the bet; it exists only as an operator escape hatch. Behind a CONNECT proxy the
  keepalive probes are invisible to the provider.

Before this, one process-wide 120s value served both, so any non-streaming answer that took longer
than two minutes was destroyed mid-flight and surfaced as `gemini_transport_unavailable` after a
pointless retry — a liveness heuristic acting as a ceiling on customer requests.

`CLAUDE_API_GEMINI_UPSTREAM` defaults to and is production-pinned at
`https://daily-cloudcode-pa.sandbox.googleapis.com`. The validator also recognizes only the daily
and production Cloud Code hosts. Literal HTTP loopback is available only behind the explicit test
opt-in; arbitrary hosts, ports, userinfo, path, query and fragment are rejected. Legacy Gemini CLI
credentials ignore the Antigravity default and remain pinned to
`https://cloudcode-pa.googleapis.com`. Published `gemini-3-flash-preview` uses the
production-configured Antigravity origin and 2.2.1 UA, omits the old IDE metadata, and maps the
public id to the live-proven private wire `gemini-3-flash`. The prior public-wire experiments remain
historical withdrawal evidence; the fresh Pro+Ultra acceptance proved generation, output,
authoritative usage, SSE, thinking, cache, PCM audio and forced-tool controls on the exact
implementation. Production systemd/default lists include the public id. Existing text models and
background health/quota calls keep their live-proven full 2.2.1 tuple. The production systemd
`ExecStart` pins the roster path,
Antigravity default origin and insecure-loopback switch after all shared environment files.

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
  65,536-token model ceiling is clamped to the private endpoint's accepted boundary of 65,535. A
  replayed native `functionCall` keeps a client-supplied opaque `thoughtSignature` unchanged; when
  a client such as Kimi Code 0.33 omits the signature from its next tool turn, the private wrapper
  adds Google's accepted stateless context-engineering marker. The marker never appears in the
  public request/response and requires no gateway-side conversation state;
- reconstructs an allowlisted native response, adds a synthesized `responseId`, and discards Code
  Assist wrapper fields, credits, private trace ids, unknown top-level fields and headers;
- surfaces a mid-stream upstream error as a sanitized native error element rather than a clean
  truncation;
- caps documented inline-media requests at 20 MiB and generated-image response bodies/pending
  stream frames at 64 MiB. Published routes other than Flash Preview reject inline audio before
  both generation and `countTokens`: their generic Antigravity prompt total cannot distinguish the
  higher official audio-input rate. Published Flash Preview is the only exception, and only its
  strict integral-duration PCM WAV generation fallback may reconstruct a missing AUDIO row;
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
  evidence, quarantines only semantic replay conflicts and is drained on shutdown. Once an
  admin-only exact-target event is enqueued, a coalesced wake triggers the free quota/health probe
  immediately; normal customer traffic retains the configured background cadence;
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
  Because that settlement is the most expensive one a customer can receive, the stream translator
  reads `usageMetadata` from **every** envelope Google reports it in — beside `response`, in a
  trailing envelope that carries no `response` at all, and inside it — and a frame whose
  `usageMetadata` carries no token counts (a bare `trafficType`, an empty object) never erases the
  counts an earlier frame reported. A turn that still ends unmetered is no longer credited to the
  model as a success, and writes one content-free journal line naming the request id and the stream
  shape (`frames`, `envelope_only`, `usage_frames`, `countless_usage_frames`, `finish_reason`) so
  the remaining cause is diagnosable without a live capture.

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

The model allowlist is local and price-catalog pinned. The production default contains seven text
models plus the separately routed Nano Banana 2 image model:
`gemini-3.1-flash-image`, `gemini-3.6-flash`, `gemini-3.5-flash`,
`gemini-3-flash-preview`, `gemini-3.1-pro-preview`, `gemini-3.1-flash-lite`,
`gemini-2.5-flash`, and `gemini-2.5-flash-lite`. The original seven were reconfirmed on Google AI
Pro on 2026-07-31; Flash Preview was admitted on 2026-08-03 only after a fresh exact-implementation
matrix completed on both Google AI Pro and Ultra with authoritative terminal usage for all claimed
controls.
`gemini-2.5-pro` is also deliberately not published: it is absent from the
official Antigravity reasoning-model table, and its residual quota bucket does not produce a
working generation route. Private tier ids are never public model names, while
unreviewed agent/foreign-provider ids have no honest public model mapping.
A Developer API price entry proves only that the gateway can meter a model; it does not prove that
an Antigravity subscription can serve it. Publication additionally requires an official
Antigravity model contract, an exact canonical-to-private route and live generation evidence.
A configured id still needs a live smoke test against every tier because Google can change private
model availability independently. The production systemd argv pins this reviewed eight-model
set after shared env files, so a stale
`config.env` cannot silently re-enable Developer-API-only models on the subscription runtime.

### Nano Banana 2 image route and accounting

Antigravity's official model page identifies Nano Banana 2 as the non-customizable generative
image tool, and the authenticated Google AI Pro catalogue exposes an independent
`gemini-3.1-flash-image` quota bucket. The public route keeps the native Gemini
`generateContent`/`streamGenerateContent`/`countTokens` envelope and returns generated media as
`candidates[].content.parts[].inlineData`; images are never written to server disk.

The canonical OpenCode integration deliberately does not advertise this as image output. Its
custom provider is `@ai-sdk/openai-compatible` 2.0.41, whose Chat response schema accepts message
content only as string/null and does not consume native `inlineData` or OpenRouter image metadata.
The plugin therefore publishes `modalities.output:["text"]` even for the image model instead of
promising media the client would discard. This is a client-transport limitation, not a provider
outage: native Gemini callers continue to generate, receive and settle images through the routes
above. OpenAI-compatible Chat and Responses callers also receive generated media: the adapters map
image-MIME `inlineData` parts to `image_url` content parts (Chat, including stream deltas) and
`output_image` items with a data URL (Responses), so a universal-lane caller is billed only for
images it actually gets; non-image inline media has no OpenAI representation and is not fabricated.

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
| `gemini-3.7-flash` | no owned private alias; dormant canary retains the exact public id only | Google announced the Developer API model GA on 2026-08-13, but the 2026-08-14 owned catalogue observation found no 3.7 quota/wire row and no live request has passed on this implementation SHA | Stage 1 dormant only; omitted from production defaults, customer discovery, router presets and storefronts until exact-SHA `countTokens`, generation, incremental SSE, advertised controls and each claimed plan pass. An explicitly configured loopback canary retains its protected `/gemini-subs` conversion row solely for tariff/plan-bound admission evidence |
| `gemini-3-flash-preview` | public → `gemini-3-flash`; quota admission joins `gemini-3-flash` + `gemini-3-flash-agent`; configured Antigravity origin, 2.2.1 UA, minimal headers; bounded inline PCM WAV fallback uses exact integral `duration × 32` AUDIO tokens and fails closed on ambiguous cache | fresh runner SHA `cc7e5beb…` / byte-identical runtime implementation completed 22 paid turns on Pro+Ultra: minimal/low/medium/high, incremental SSE, final cache reads with 8,170 cached tokens, fresh/replayed 8-token PCM audio and forced function calls; public identity and terminal response/event usage matched | published; generation 5 main catalog, production defaults, router manifest and public web/docs |
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

Official evidence (review date stated per item where newer than the baseline 2026-07-31 review):

- Gemini 3.7 Flash GA identity, 1M/64K limits, `low`/`medium`/`high`, migration rules and
  introductory-to-standard price boundary:
  <https://ai.google.dev/gemini-api/docs/latest-model> and
  <https://ai.google.dev/gemini-api/docs/pricing> (reviewed 2026-08-14); the hosted Gemini Managed
  Agents “Antigravity agent” described there is not evidence for this gateway's OAuth-backed
  Antigravity/Code Assist route;
- model catalogue and lifecycle: <https://ai.google.dev/gemini-api/docs/models>;
- Gemini 3 Flash Preview shape (1,048,576 input / 65,536 output, text output) and paid rates:
  <https://ai.google.dev/gemini-api/docs/models/gemini-3-flash-preview> and
  <https://ai.google.dev/gemini-api/docs/pricing#gemini-3-flash-preview> (reviewed 2026-08-02);
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

Gemini 3 Flash Preview implementation evidence reviewed on 2026-08-02 and 2026-08-03:

- Google's Apache-2.0 Gemini CLI at commit
  [`f47d6c6f7a1308d81f9f57acf7d279f0928c5249`](https://github.com/google-gemini/gemini-cli/commit/f47d6c6f7a1308d81f9f57acf7d279f0928c5249)
  defines `PREVIEW_GEMINI_FLASH_MODEL = "gemini-3-flash-preview"` and sends that ID unchanged;
- the locally installed, signed Antigravity 2.4.3 language server contains the same public model ID
  and no `gemini-3-flash-agent` generation ID. It contains the non-sandbox daily origin and not the
  sandbox origin. Combined with its already pinned
  `requestType=agent` wrapper and owned `fetchAvailableModels` rows, this separates the public wire
  ID from private quota accounting instead of guessing a public-to-private model alias;
- the initial production smoke on the sandbox origin proved `countTokens` but every paid generation
  capability returned the same model-resource `404`; zero immutable turns and zero spend were
  recorded. This isolates the failure before response translation, streaming and billing;
- the isolated non-sandbox-host A/B retained the same result on 2026-08-02: all ten bounded
  generation legs returned `404`, with zero immutable turns and zero spend. Working
  `gemini-3.6-flash` still returned 200 after cutover for `$0.000003`, so the shared transport and
  existing text route did not regress;
- the signed-UA micro-smoke also returned `404` after a one-token count preflight, with no Preview
  turn or spend in the immutable five-minute window. Host and release identity are therefore not
  sufficient selectors;
- the final minimal-header micro-smoke also returned `404` after a successful one-token
  `countTokens`. It was bounded to one generation request and `$0.0001`, returned exact
  `x-apitoken-execution-state: not_started`, created no immutable Preview turn and spent zero;
- quota presence and a successful token count did not prove generation, so the 2026-08-02 public
  wire attempt correctly withdrew every public/default surface instead of guessing an alias;
- the 2026-08-03 owned Ultra probe in
  [`research/GEMINI_3_FLASH_PRIVATE_ROUTE.md`](../../research/GEMINI_3_FLASH_PRIVATE_ROUTE.md)
  supplied the missing route evidence: private `gemini-3-flash` returned generation 2xx, real text,
  terminal usage with thoughts, canonical `modelVersion`, incremental SSE and working low/high;
  `gemini-3-flash-agent` also served but echoed the weaker `gemini-default` alias identity;
- the then-dormant gateway therefore maps public `gemini-3-flash-preview` to `gemini-3-flash`, rewrites
  native and SSE `modelVersion` back to the public id, and conservatively joins both observed quota
  rows;
- the 2026-08-03 exact-SHA Pro+Ultra gate passed all four thinking levels, incremental SSE and
  cache write/read, then stopped on a successful audio response whose terminal usage exposed 55
  generic prompt tokens but zero audio tokens. Free `countTokens` on the same body returned 4091
  tokens and no modality details on either plan, so it cannot repair settlement. The model is
  withdrawn from publication rather than guessing the official audio-rate split;
- the follow-up dormant candidate uses Google's official 32 audio tokens/second only for strictly
  parsed inline PCM WAV where `frames × 32 / sample_rate` is an integer. It reconstructs an absent
  AUDIO row before public response and Rust settlement only when cache attribution is exact:
  uncached, fully cached, or an explicit cached AUDIO detail. Compressed/file audio, fractional
  token duration and partial cache without modality detail fail before money can be guessed. This
  implementation evidence does not publish the model; it must pass the complete Pro+Ultra matrix
  on its own exact SHA;
- accounting SHA `4b0c6443b55eb1839bdd9ccbe1cc8e5bb1cc8214` passed all thinking levels and
  incremental SSE on both plans, but its fresh run stopped at Ultra cache-write. The terminal turn
  had no visible non-thought output, immutable output was zero, response/event usage parity failed,
  and the nominal write already observed cache-read tokens after the Pro write reused the same
  run/model cache key. That paid turn is not replayable and does not authorize publication. The
  next dormant runner candidate derives an opaque per-profile cache scope and gives cache/audio
  512 output tokens; it requires a new run id and a full exact-SHA matrix from the beginning.
- runner SHA `b9d941c36eb9189f2d11ed4d0a6d3f5b225dd1d8` then proved those isolated cache
  write/read and exact PCM WAV fresh/replay paths on Pro and Ultra, in addition to every thinking
  level and incremental SSE. Its nineteenth paid turn returned the required forced function call,
  public identity, terminal usage and exact response/event parity, but Google reported 65 ordinary
  input tokens and no `toolUsePromptTokenCount`. The old runner incorrectly treated that optional
  non-priced subset as a required billing class and stopped before the second tool leg. The terminal
  report is not resumable. The follow-up keeps the provider `promptTokenCount` as the complete
  priced authority, leaves `tool_prompt_tokens=0` as honest diagnostic evidence and requires a new
  full run; it does not reinterpret or reuse the failed turn.
- runner SHA `d0a9fb4052773517e987d1a79664965a131ef1ac` accepted that optional diagnostic and
  again passed every thinking level plus incremental SSE on Pro and Ultra. Both cache writes were
  successful, but the first read arrived only after the other profile's write and evidence wait and
  contained 12,343 fresh input tokens with no cache class. Its terminal report and
  `34,320,500 nanoUSD` spend are not resumed;
- runner SHA `a4eed55b03835fb0a2b9d360b7c07ca37fe389b6` made replay groups adjacent per
  profile. Pro then observed 8,170 cached tokens, while the immediately repeated Ultra body still
  reported all 12,342 input tokens as fresh. Fourteen successful paid turns cost
  `37,985,500 nanoUSD`; audio, tools and Search were not dispatched. A future fixed three-turn
  cache group may add one deliberate prime, but its two-plan worst-case ceiling is
  `23,099,392,000 nanoUSD` and cannot run under the prior `$21` approval.
- fresh runner SHA `cc7e5bebc16ac720c909f221e6cfc9bd95070561` then used the fixed profile-local
  `write → prime → read` sequence and completed the entire matrix on Pro and Ultra. Both final reads
  exposed 8,170 cached tokens; fresh and replayed PCM WAV turns exposed 8 AUDIO tokens; forced-tool
  turns returned exactly one function call. All 22 turns had public model identity, visible output
  where required, terminal finish/usage and exact response/event parity. The run spent
  `49,232,500 nanoUSD` under its `$24` cap. Search was skipped before dispatch as a documented
  non-blocking control because the per-query surface has no provider hard ceiling for a safe
  reserve. This fresh result supersedes the publication withdrawal without rewriting any failed
  historical report.

The reproducible source/plan/rate/wire dossiers are
[`research/GEMINI_3_FLASH_PREVIEW.md`](../../research/GEMINI_3_FLASH_PREVIEW.md) and
[`research/GEMINI_3_FLASH_PRIVATE_ROUTE.md`](../../research/GEMINI_3_FLASH_PRIVATE_ROUTE.md). The
GREEN publication evidence is
[`research/GEMINI_3_FLASH_PUBLICATION_LIVE_ACCEPTANCE.md`](../../research/GEMINI_3_FLASH_PUBLICATION_LIVE_ACCEPTANCE.md).
The
exact-SHA withdrawal record is
[`research/GEMINI_3_FLASH_PRIVATE_ROUTE_LIVE_WITHDRAWAL.md`](../../research/GEMINI_3_FLASH_PRIVATE_ROUTE_LIVE_WITHDRAWAL.md);
the bounded accounting proof and new candidate contract are
[`research/GEMINI_3_FLASH_AUDIO_ACCOUNTING.md`](../../research/GEMINI_3_FLASH_AUDIO_ACCOUNTING.md).
The failed accounting-candidate gate is preserved in
[`research/GEMINI_3_FLASH_AUDIO_ACCOUNTING_LIVE_WITHDRAWAL.md`](../../research/GEMINI_3_FLASH_AUDIO_ACCOUNTING_LIVE_WITHDRAWAL.md).
The later optional tool-subset runner withdrawal is preserved in
[`research/GEMINI_3_FLASH_TOOL_USAGE_LIVE_WITHDRAWAL.md`](../../research/GEMINI_3_FLASH_TOOL_USAGE_LIVE_WITHDRAWAL.md).
The subsequent cache-liveness withdrawals are preserved in
[`research/GEMINI_3_FLASH_CACHE_LIVENESS_LIVE_WITHDRAWAL.md`](../../research/GEMINI_3_FLASH_CACHE_LIVENESS_LIVE_WITHDRAWAL.md)
and
[`research/GEMINI_3_FLASH_ADJACENT_CACHE_LIVE_WITHDRAWAL.md`](../../research/GEMINI_3_FLASH_ADJACENT_CACHE_LIVE_WITHDRAWAL.md).

## Failure and stream safety

| Condition/result | Profile action | Request action |
|---|---|---|
| first `401` | compare rejected bearer, single-flight refresh | retry once on the same profile |
| repeated `401` or `403 UNAUTHENTICATED` | auth quarantine | rotate to another profile |
| `403 PERMISSION_DENIED` | none — the credential was accepted | rotate for this request only, then return Google's own `403` |
| `429` | cool only that model/profile from `Retry-After`, `google.rpc.RetryInfo` or quota reset | rotate without transport budget |
| network/token refresh, `408`, `409`, `425` | short profile cooldown | bounded rotation |
| generation `5xx` or malformed wrapper/stream | exponential model cooldown | bounded rotation without disabling other models |
| other deterministic `4xx` | keep profile healthy | return a synthetic native-shaped error |
| high local in-flight on every eligible profile | keep bindings and profiles healthy | dispatch immediately; use in-flight only to choose the least-loaded unbound profile |

Private error bodies are never returned verbatim. They may contain account, project, tier or private
endpoint details. Public errors retain only a generic Google-shaped status.

Every upstream `429` additionally emits one privacy-bounded `gemini-rate-limit` journal line without
changing rotation or cooling. Generation lines carry an internal `request_id`, operation and phase
(`http_response`, `stream_start` or `stream_midflight`), routing-attempt ordinal, public/wire model, opaque
profile id, OAuth kind, known `google.rpc` status, allowlisted `ErrorInfo` reason/domain class plus
a keyed fingerprint for unknown reason/domain values, detail type and metadata-key sets, closed
message class, retry hint, actual applied cooldown, process-keyed correlation fingerprints and
the exact already-sanitized model-catalogue state (fresh/stale/missing, age, matching/zero/positive/
unknown bucket counts, minimum remaining basis points and reset distance) observed before cooling.
`diagnostic_body` distinguishes parsed, malformed, oversized and unavailable provider evidence.
An exhausted pre-byte rotation emits a second summary with the same request id and the counts of
429s/routing attempts/distinct profiles. `loadCodeAssist` 429s use `phase=load_code_assist` and the
same bounded machine evidence; its existing profile-wide cooling is applied before the bounded
diagnostic body is drained. Google prose, arbitrary `ErrorInfo.metadata`, quota descriptions,
project/email, tokens, proxy and customer request content are never logged. Correlation fingerprints
are comparable only inside one process lifetime and reset on restart; this prevents offline
enumeration of low-entropy account/quota strings. Identical fingerprints across distinct profiles
identify the same provider error shape but are diagnostic evidence only — they do not change the
existing quota classification.

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
`generationConfig`; `maxOutputTokens` is emitted only for an explicit client cap), then issues an
internal request to
`/v1beta/models/{model}:generateContent` or `:streamGenerateContent?alt=sse` handled by the shared
`gemini_api` path — admission, reserve, affinity, rotation, Code Assist wrapper, metering and
settlement are identical to the native route. When the client omits the cap, shared admission uses
the model's native output limit and lowers it only when the available balance requires it, matching
the native route rather than imposing a universal-lane default. Responses are translated outside that path:
`candidates[0]` parts become chat content/`tool_calls` (synthetic `callu_<name>[_N]` ids),
`usageMetadata` becomes OpenAI usage (completion = candidates + thoughts tokens), and the data-only
SSE stream becomes `chat.completion.chunk` frames with a final usage chunk on EOF when
`stream_options.include_usage` is set.

Code Assist compatibility is normalized at the adapter boundary. Its private `Schema` parser accepts
the official Google `Schema` vocabulary, not arbitrary legal JSON Schema. One bounded translator is
used by Chat and Responses tool declarations, Messages `input_schema`, and Chat/Responses
structured-output schemas. It preserves the Google fields, expands local JSON Pointer
`$ref`/`$defs`, maps string `const` to `enum`, numeric `const` to equal bounds,
`type: [T, null]` to `nullable`, numeric exclusive bounds to the nearest representable inclusive
IEEE-754 bound, and `contains: {}` cardinality to item bounds. Harmless annotations and unused
definitions are removed. Property names that happen to equal schema keywords remain data.

The translator never drops a validation constraint merely to satisfy Code Assist. Constraints with
no exact Google representation (`patternProperties`, non-trivial dependencies/contains,
`unevaluatedProperties:false`, `propertyNames`, conditionals, closed
`additionalProperties`, `multipleOf` other than the integer no-op, uniqueness, composition and
external/recursive references) and unknown keywords fail locally before reserve/upstream. OpenAI
surfaces return the exact JSON Pointer suffix in `error.param`; Messages includes the same path in
its Anthropic error message. Expansion is fail-closed at 4096 schema nodes or depth 64. This keeps
common AI SDK schemas working without turning a stronger client contract into a weaker one.

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
`name`/`strict` wrapper and applying the same Code Assist supported-subset translator used for tools.

Stage 3.4b adds reasoning. `reasoning_effort` (`minimal`/`low`/`medium`/`high`; `null` or absent
turns it off) is translated into `generationConfig.thinkingConfig` — `thinkingLevel` is proxied
verbatim because the plane itself maps the level into the private wire model id, and
`includeThoughts: true` opts into thought parts; any other non-null value is rejected with `400
invalid_request` (`param: reasoning_effort`). In responses, thought parts (`"thought": true`) go
to the OpenAI `reasoning_content` extension instead of leaking into content: non-stream they are
concatenated into `message.reasoning_content` (present only when non-empty), in the SSE stream
each thought part becomes a `{"delta": {"reasoning_content": ...}}` chunk ahead of content
deltas in upstream order. `thoughtSignature` is always dropped — universal lanes never expose
signatures (decision 4). On a later Chat request, non-empty `reasoning_content` is therefore
display-only and is never promoted into an unsigned native thought part. If it is the assistant
turn's only payload, that turn is omitted and adjacent user contents are merged; a genuinely empty
assistant without reasoning remains a `400`. This keeps the adapter's own thought-only response
replayable by `@ai-sdk/openai-compatible` without forging provider signatures.

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

## What a caller can diagnose from a refusal

Support spent a working day on one customer's `503` because the refusal carried nothing
actionable: no machine reason, no request id, and the native `/v1beta` surface was outside the
`customer_http_error` audit. The contract below exists so that never repeats.

**Deliberate refusals name themselves.** Every input this gateway will never accept is rejected
before dispatch as `400 INVALID_ARGUMENT` carrying a `google.rpc.ErrorInfo` detail with a stable
`reason`. An SDK can branch on it, and it distinguishes "never going to work" from "retry later".

| `reason` | Meaning | What the caller should do |
|---|---|---|
| `FILE_URI_UNSUPPORTED` | `fileData`/`file_uri` references a Files API resource | inline the bytes as `inlineData` (`mimeType` + base64) |
| `CACHED_CONTENT_UNSUPPORTED` | `cachedContent` resource | send the content inline |
| `AUDIO_INPUT_UNSUPPORTED` | audio on a model without exact audio accounting | use `gemini-3-flash-preview` with inline `audio/wav` |
| `SERVICE_TIER_UNSUPPORTED` | explicit `serviceTier` | drop the field |
| `STORE_CONTROL_UNSUPPORTED` | explicit `store` | drop the field |
| `API_KEY_INVALID` | key not accepted | check the `x-goog-api-key` header |
| `RATE_LIMIT_EXCEEDED` | pool quota; carries `RetryInfo` | honour the retry delay |

A Files API reference deserves the explicit note: the uploaded resource belongs to the caller's own
Google project, while this gateway calls the provider under a pooled subscription. The file is
invisible to us, so every profile answers `PERMISSION_DENIED` identically. It is an unsupported
input, not an outage, and no retry or rotation can change that.

**Every error response carries `x-request-id`.** The same id appears in the journal, so a customer
quoting it lets an operator find their exact request:

```bash
journalctl -u 'claude-api-gemini@*.service' --since today --grep='<request-id>'
```

**Retryable and terminal are kept apart.** A `503`/`429` from this gateway means capacity — it
carries `RetryInfo` and retrying is correct. A `400` or `403` is terminal: the caller must change
the request. The gateway no longer reports a provider's `PERMISSION_DENIED` as a retryable `503`,
which previously drove SDKs into retry loops over an input that could never succeed.
