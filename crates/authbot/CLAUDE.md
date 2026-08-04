# crates/authbot — CLAUDE.md

**Role:** pool replenishment — the Telegram bot for purchasing subscriptions. PRODUCER component:
sits OUTSIDE the API layers (`registry←pool←forward←server`), BEFORE the registry. It buys access
from sellers and hands it to the engine.

**Boundaries (hard):**
- Depends on `registry` (only `authority` — subscription registration) + `tokio`, `portable_pty`,
  `rusqlite`, `reqwest` (URL validation/other bot APIs) and `serde`; Gemini Google HTTPS is
  performed by the shared exact Node helper, not reqwest/rustls.
  It does NOT import `pool`/`forward`/`server` and does not reach into their internals.
- Replenishes EXCLUSIVELY this project's pool: its own bot token, its own `AUTH_BOT_FLEET`.
- Its own state (users/offers) lives in the bot's separate SQLite, NOT in the engine registry.
- The subscription registry is ONLY the engine PostgreSQL from the root-owned
  `engine-postgres.env`. SQLite is allowed only for the bot's own workflow state; a registry
  fallback is forbidden — without a DSN the bot does not start.

**Three fundamentally different access-handoff scenarios** (`handoff_kind` selects the branch by
offer product — this is the only place where they diverge):

| | Claude | ChatGPT (Codex) | Gemini via Antigravity OAuth |
|---|---|---|---|
| Result | `sk-ant-oat01-…` | nothing we are allowed to read | refresh/access token + Google subject/project/tier |
| What the purchase becomes | a row in the registry | a `CODEX_HOME` directory | AEAD envelope + opaque entry in `profiles.json` |
| Module | `setup_token.rs` | `codex_login.rs` | `gemini_oauth.rs` |
| Seller steps | proxy → email → `code#state` | proxy → email → one-time code | proxy → Gemini CLI OAuth/code → Antigravity OAuth/localhost URL |
| Step back | `ho_code→ho_email→ho_proxy` | `cx_wait→cx_email→cx_proxy` | `gm_wait→gm_ready→gm_gproxy` |
| How the engine learns | registry reload | homes scan | atomic roster refresh on the health loop |

Every Claude/ChatGPT/Gemini offer immediately explains the entire upcoming path to a newcomer.
After the payout the bot issues a dedicated proxy and walks the seller in detail through a new
anti-detect browser profile, self-registration and activation of the required plan, then through
the corresponding authorization. Gemini waits for a separate "Аккаунт готов" ("Account ready")
confirmation and only then shows the links. Every step emphasizes: do not open the account before
the proxy, do not change the profile/IP, do not send passwords, cookies or payment data.
If automatic proxy issuance is unavailable, the product fallback separately requests and verifies
a proxy, never sending a newcomer onward with an incomplete instruction.
At any step the seller can go back **exactly one step** with the `↩️` button or the word `назад`
("back"). For Claude/Codex `/cancel` uses the same mechanism. For Gemini `/cancel` is deliberately
stronger: it atomically extinguishes the pending/processing OAuth capability, rotates the
seller-job generation, stops the local worker and immediately restarts both OAuth phases with a
fresh state+PKCE on the same egress. An old callback after this cannot publish a credential or
alter the new attempt. Going back with the regular button from a step where a one-time link or
code has already been issued requires explicit confirmation and extinguishes the old capability.
The buyer proxy and a live IPRoyal lease cannot be replaced this way: they have no "enter proxy"
step in the seller history, and `hproxy_order` is never zeroed on any rollback path.
The quick admin keyboard offers Claude, ChatGPT Plus/Pro and selected Google AI plans.

**Batch purchases:** the `/batch` command or the `🧺 Batch-покупка` ("Batch purchase") button
starts a purchase of 2 to 100 identical subscriptions with a single shared payout. Each position
keeps its own proxy, and the seller receives positions strictly in sequence: the next one opens
only after the previous one is successfully handed over. Before creating a batch the admin chooses
the proxy source — own proxies (one per position) or the seller's proxies. This choice splits
Claude, ChatGPT and Google AI/Gemini handoff identically; batch state and an unfinished wizard
survive an authbot restart.

`/jobs` is available to the admin and to an approved seller. The batch card shows completed,
remaining and current positions. A processing batch can be paused immediately: completed positions
are preserved, the current unfinished one returns to `pending`, all its OAuth/device capabilities
are invalidated, and the seller lock is released for a single single-offer. Creating a new batch
while a paused batch exists is forbidden. Resume is allowed only after the single completes and
creates a new exact generation for the same position. The admin can two-step delete a batch from
the work queue; this is a soft-delete into the `cancelled` status, so tx/progress remain in SQLite
for audit. A batch in the indeterminate `paying` phase cannot be deleted before payment review.

**Seller deal isolation:** `seller_jobs` holds exactly one active job per seller — either a
specific single-offer or a specific `batch_id + item_no`. The context is reserved atomically at
deal acceptance, before the blockchain call, and bound to the Claude/ChatGPT/Gemini handoff
(Gemini also stores it in the PKCE session). Each position activation gets a one-time generation
token: a successful authorization can complete only the exact source/id/item/generation with a
matching product type; the mere presence of another active batch nearby never advances its cursor.
While the job is unfinished, no competing single/batch deal can be accepted or paid. The admin
sees the queue via `/jobs` or `📋 Активные сделки` ("Active deals"); to fix a mistaken mark from
an old version, the current batch has a two-step button that returns exactly to the previous
position. The seller-side analogue is the step back inside the access handoff: it goes through
the same generation guard, rotates the token (so any late callback fails closed) and, by the
`phase='processing'` condition, cannot touch a job in the `paying` phase. The originating step's
predicate lives in the same SQL statement as the guard, so a double press moves exactly one step
back, not two. Indeterminate single and batch payouts stay locked until explicit admin review
after chain verification. In `/jobs` the admin can two-step delete the exact single-offer of the
accepted/processing generation: the current handoff is cancelled, the seller lock is released,
the response becomes `cancelled`, and the original response/job phases plus the deletion author
are preserved in `offer_archive_events`. The `paying` phase is not subject to deletion.

**Codex branch invariants (critical):**
1. **Login — only the official client in a PTY; secrets are never logged or forwarded.** The bot
   never sees the password or the second factor. After the device flow the bot reads the staging
   directory's `auth.json` EXACTLY ONCE, seals the OAuth material into an AEAD envelope
   (`codex-credential`) in the engine roster and fully deletes the staging — no plaintext token
   remains anywhere. Before that, the account type is checked with the `codex login status` line,
   the plan via the `chatgpt_plan_type` id_token claim (free is rejected).
2. **An unfinished purchase leaves no traces.** Expired code, refusal, wrong account type →
   staging is deleted; a profile enters the roster only after a successful seal. Stepping back
   from `cx_wait` also wipes staging together with the child process. Waiting for the device flow
   is an explicit `cx_wait` state, not an empty `want`: after a restart it is restored into
   `cx_email`, because the child process does not survive a restart and its one-time code expires
   unattended.
3. **The login goes through the same proxy as the account's future traffic** — otherwise the
   purchase and the usage look like two different users.
4. **The proxy is a secret:** it exists only inside the envelope, never printed to any log or chat.
5. **The roster is published atomically** (tmp+rename, credential 0600, directory 0700): the
   engine never reads half a file and picks up the profile on the next health tick without a
   restart.
6. The bot does NOT edit `config.env`, does NOT restart the engine and does NOT act as root:
   `AUTH_BOT_CODEX_ROSTER_DIR` + keyring is its entire part of the contract.

**Gemini branch invariants (critical):**
1. A new handoff is two separate client-bound OAuth transactions, not a token conversion. First,
   the public installed-app client of the official Gemini CLI with redirect
   `https://codeassist.google.com/authcode` confirms the verified Google identity; its tokens are
   never published and its volatile Code Assist response is not used for admission. Then a new
   `state` + PKCE S256 uses
   the public Antigravity client and the fixed redirect
   `http://localhost:51121/oauth-callback`. Google subject, canonical proxy and seller-job
   generation must all match; legacy proof is carried over only inside the state-bound AEAD of the
   second phase. The seller does not create an OAuth client and does not enable private APIs in
   their project.
2. Token exchange, userinfo, Antigravity `loadCodeAssist` and onboarding go through the same
   `node_transport.cjs` source as runtime: SHA-pinned `/usr/bin/node` v24.18.0 Linux/x64,
   per-account authenticated CONNECT and `env_clear`. The legacy phase preserves the client-bound
   form-order of `google-auth-library` 10.9.0 and token/userinfo identity; the final identity is
   pinned to Antigravity 2.2.1: the control plane
   uses `antigravity/hub/2.2.1 darwin/arm64`, onboarding adds
   `google-api-nodejs-client/10.3.0`, token exchange — `Go-http-client/2.0`; userinfo goes through
   the attested Node-internal Undici
   dispatcher (its headers, pooling and ClientHello must not be replaced by a gaxios profile).
   Proxy/bearer/form exist in zeroizing IPC buffers; Rust TLS and ambient proxy do not
   participate. `loadCodeAssist` sends `ideType=ANTIGRAVITY`, and onboarding sends Antigravity ide
   name/version metadata.
3. OAuth codes/tokens never go through Telegram. In the legacy phase the seller copies the
   one-time Gemini CLI code shown by Google into a no-store HTTPS form. In the Antigravity phase
   localhost may fail to open; the seller copies the full URL from the address bar into a separate
   form. The parser verifies the exact HTTP localhost:51121 path, the callback state and the
   absence of credentials/fragment/OAuth error.
   The short-lived proxy lives in SQLite only as an XChaCha20-Poly1305 envelope, bound by AAD to
   the one-time state; the form/callback claim is one-time.
4. The legacy phase checks verified userinfo and performs a duplicate/proxy preflight before the
   second consent. Absence of a project/tier on the legacy Code Assist surface proves neither
   compatibility nor incompatibility of the account, so authoritative tier/project admission runs
   only after a fresh Antigravity consent. Only known
   Google AI Pro/Ultra, Code Assist Standard/Enterprise and Workspace AI Ultra are accepted. Free,
   Plus, incompatible Workspace and unknown future paid tiers fail closed. The offer-creation menu
   shows only Google AI Pro/Ultra; organizational tiers keep being recognized for compatibility
   with old callbacks and the actual plan check after OAuth.
   After the final tier check a non-streaming
   `gemini-2.5-flash-lite:generateContent` runs with runtime headers; it requires a 2xx, a wrapped
   candidate and non-zero authoritative `usageMetadata`. The surface is the reviewed sandbox host
   first, and ONLY on a pre-generation access refusal (403/404) is the same probe repeated on the
   production host from which the engine actually serves traffic: an account may be admitted on
   one host and rejected on the other, and a 403 means the model never ran and no paid generation
   was spent. A 503, malformed response, missing usage or ambiguous transport does not publish the
   credential, does not complete the payout and does NOT move to the second surface; a paid
   generation that already happened is never automatically repeated. A Google-account-level refusal
   is identical on every surface, so it is recognized immediately and not retried anywhere else:
   `VALIDATION_REQUIRED` / "Verify your account to continue" is
   `account_validation_required`, a separate outcome instructing the seller to complete Google
   verification in the same profile and proxy — not "wait and retry": a retry does not change the
   account state. Google puts the personal verification link in
   `error.details[].metadata.validation_url`, while `message` carries only the phrase — the link
   is extracted and forwarded to the seller as copyable text (not a clickable link: it must be
   opened in their profile and egress, not in Telegram's built-in browser).
   It comes from upstream, so fail-closed: only an `https://accounts.google.com/` prefix,
   no control/whitespace/quotes and ≤2048 bytes — otherwise our own message would become phishing.
   Such an account's tokens are NOT discarded: Google has already confirmed identity, tier and
   project, and rerunning both consents is an extra chance to confirm the wrong account. They are
   parked as an AEAD envelope in `gemini_pending_verifications` (AAD
   `gemini-verification-<chat>`, so one seller's envelope cannot be opened for another), exactly
   one per chat, fenced by the exact seller-job generation with a 72-hour TTL. This is NOT
   publication: nothing enters `profiles.json` and the payout does not complete. A
   `gemini:verified` button appears next to the message, and every press is one real acceptance
   generation on the parked tokens (the access token is refreshed via the same egress when
   needed). Success publishes the profile and closes the deal with the same code path as the
   callback; a repeated hold shows the button again; any other verdict erases the parking, as do a
   new consent, `/cancel` and an expired TTL. `countTokens`, quota and `loadCodeAssist` are not
   acceptance. Only the HTTP status, the surface and Google's enum fields (`error.status`,
   `error.details[].reason`) go to the log;
   free-form `error.message` — only under `AUTH_BOT_GEMINI_TIER_EVIDENCE=1`, because it may
   contain the project and account.
5. Google subject is the quota identity: two DIFFERENT subjects cannot share a profile, and one
   subject always occupies exactly one profile. The legacy preflight recognizes an already
   published Antigravity subject BEFORE checking the volatile tier display and the second consent,
   so repeating an already connected account returns the exact duplicate outcome instead of a
   false "subscription not found", and does not annul the live refresh token. An existing
   legacy profile may migrate to Antigravity only with the same subject/proxy; the profile id,
   roster and IPRoyal lifecycle are preserved. An in-flight Antigravity callback from an old
   version stays compatible and, with the exact same subject/proxy, can atomically replace the
   material in place, because its consent may already have annulled the old token. Changing the
   proxy and reverting to legacy fail closed. The authorization
   link always carries `prompt=select_account consent`: `consent` alone is not enough — it
   reconfirms the already logged-in account without a selection screen, and a seller doing batch
   positions in a row in one browser profile silently reconfirms the previous account and kills
   its token. Email, subject, project, tier, OAuth secret/token and the authenticated proxy live
   only inside the AEAD.
   If Google returns both `paidTier` and `currentTier`, the exact reviewed tier ID is the only
   authority: Google rewrites the display name (Google One → without "One", Antigravity wording)
   without touching the ID, so a name of another known plan counts as drift and is logged rather
   than blocking access. On a mismatch between the reviewed `paidTier` and `currentTier` plans,
   `paidTier` wins (the official client chooses the same way: `paidTier.id ?? currentTier.id`),
   because Antigravity onboarding legitimately leaves `currentTier` on a different plan, and the
   old fail-closed behavior told sellers with a live subscription "subscription not found". An
   unknown ID falls back to the exact reviewed name; familiar substrings do not grant access, and
   evidence not reviewed in any field fails closed. Before every `unsupported_plan` the log
   receives only bounded shape classes: presence of project/paid/current, the number of allowed
   tiers and `known_id`/`known_name`/`name_drift` without raw tier, project or identity. A
   separate opt-in `AUTH_BOT_GEMINI_TIER_EVIDENCE=1` (off by default) additionally prints the
   bounded raw tier id/name of the final `loadCodeAssist` — otherwise a new Google plan is
   recognized only by a new deploy.
6. Credential envelopes and `profiles.json` are `0600`, directories are `0700`,
   symlinks/alternate paths are forbidden. A new publication writes the envelope first, then the
   atomic roster rename+fsync. Migration preserves the opaque profile id, roster and the existing
   IPRoyal lifecycle, atomically replacing only the envelope. After generation acceptance and
   publication-lock wait, the exact seller-job generation is re-checked immediately before the
   write; SQLite and the roster do not form a shared transaction, so this minimizes the inevitable
   cross-store window. Startup rewrap moves old envelopes to the active kid, preserving online key
   rotation.
   A manual egress change is performed only by the local operator commands `gemini-proxy-stage`,
   `gemini-proxy-commit` and `gemini-proxy-rollback` with the Auth Bot stopped: the proxy is read
   from stdin, the old envelope remains as an encrypted rollback, and runtime picks up the atomic
   replace without a restart. Telegram, argv and command output never contain the proxy. Stage
   does not accept another profile's proxy and resets the IPRoyal order to `0`, because the bot
   cannot renew an external proxy.
7. After an unsuccessful OAuth, retry preserves the exact egress for buyer/IPRoyal and
   seller-proxy. Any second-phase error starts a new two-phase generation; legacy token/project
   survive nowhere. `transport_unavailable`, control-plane `temporary_upstream` and final
   `generation_unavailable` are different outcomes, so a healthy proxy is no longer blamed by a
   message for a Google HTTP/malformed response or a generation 503. In a
   seller-proxy job the command `повторить` ("retry") creates a new PKCE generation with the saved
   proxy, while a new proxy message explicitly replaces it. Before the account-creation
   instruction only local URL canonization runs: speculative CONNECT is forbidden, because a
   residential gateway may answer a transient 403 to the probe itself with a fully working
   allocation. The real OAuth transport is serialized inside authbot and distinguishes the bounded
   CONNECT classes `proxy_auth`, `proxy_throttle`,
   `proxy_rejected`, `proxy_upstream`, `proxy_connect`, `proxy_eof`, `proxy_protocol` and
   `proxy_timeout`. Safe pre-target token-exchange refusals are automatically retried by a fresh
   helper at 0/2/7/17/37 seconds; after the token is obtained, idempotent userinfo/Code Assist
   operations use the same bounded recovery. An ambiguous post-send timeout/network error never
   replays a one-time authorization code. The transport log contains only the attempt number and
   the bounded class, never the proxy URL/credentials. `назад` ("back") from `gm_wait` restores
   the egress from the sealed PKCE transaction instead of asking for the proxy again
   (`start_gemini_oauth` erases `users.hproxy`, so no other copy exists); while the callback is
   already processing the code, rollback refuses rather than racing the exchange. `/cancel` is a
   separate generation-fenced restart: it is entitled to stop an already claimed callback,
   restores the sealed or pinned egress before deleting the old session and immediately issues a
   fresh two-phase attempt. If the egress is corrupted or externally deleted, the old generation
   is extinguished anyway; the seller proxy is requested anew, a fixed proxy requires operator
   repair. A regular rollback never erases a pinned proxy — the previous `/cancel` did so
   unconditionally and thereby permanently locked a job with the buyer's proxy.
8. **Reconstructing the proxy from the seller's message is reversible.** `ip:port:user:pass` is
   split into exactly four fields (the password may contain `:`), and userinfo is percent-encoded
   into the unreserved set, because canonization further down the stack DECODES percent sequences:
   without encoding, a literal `%41` in the password becomes `A`, and `/`, `?`, `#` break the
   authority parsing. Any loss here goes into CONNECT as someone else's password and comes back as
   the `proxy_auth` class, which is indistinguishable from a dead proxy without a manual
   investigation. The `ip:port` form remains valid (IP-based authorization), but the seller is
   explicitly told that the login and password were not recognized. Rejected input is logged only
   as a keyless fingerprint (form, host/port validity, field lengths).
**Secrets:** `AUTH_BOT_TOKEN`, the BSC payout key, Claude/Gemini credentials and proxies — only in
`authbot.env` or closed runtime files (outside the repo). Do not commit, do not print.

**Env:**
- `AUTH_BOT_TOKEN`, `AUTH_BOT_ADMIN`, `AUTH_BOT_FLEET`, `CLAUDE_API_DATABASE_URL` — the basics.
- `AUTH_BOT_CLAUDE_BIN` (priority) / `CLAUDE_BIN` — the claude CLI for the Claude branch. The
  production unit sets the former and bind-mounts the official install read-only at
  `/run/claude-authbot/claude`, without exposing the rest of home; the legacy `CLAUDE_BIN` from
  `authbot.env` cannot override the unit path.
- `AUTH_BOT_CLAUDE_CONFIG_DIR` — writable root of isolated Claude sessions (default
  `/srv/claude-api/data/authbot`); tokens and state must not live in home.
- `AUTH_BOT_CODEX_BIN` — the pinned codex CLI (default `/srv/claude-api/data/codex/bin/codex`).
- `AUTH_BOT_CODEX_HOMES_DIR` — staging directory of the device flow (hidden login directories;
  NOT the pool).
- `AUTH_BOT_CODEX_ROSTER_DIR` — root of the engine's `credentials/` + `profiles.json` (default
  `/srv/claude-api/data/codex`); the engine's `CLAUDE_API_CODEX_PROFILES_FILE` must point to
  `<this directory>/profiles.json`.
- `AUTH_BOT_CODEX_CREDENTIAL_KEYS`, `AUTH_BOT_CODEX_CREDENTIAL_ACTIVE_KID` — the AEAD keyring
  shared with runtime and the active publication/rotation key (`CLAUDE_API_CODEX_CREDENTIAL_KEYS`
  on the engine side).
- `AUTH_BOT_GEMINI_DIR` — root of `credentials/` + `profiles.json` (default
  `/srv/claude-api/data/gemini`); the engine's `CLAUDE_API_GEMINI_PROFILES_FILE` must point to
  `<this directory>/profiles.json`.
- `AUTH_BOT_GEMINI_REDIRECT_URI`, `AUTH_BOT_GEMINI_OAUTH_BIND` — the public HTTPS form accepting
  the one-time code (`…/oauth/callback`) + its loopback bind. The legacy redirect name is kept
  for env compatibility; Google gets the fixed Antigravity localhost redirect.
- `AUTH_BOT_GEMINI_CREDENTIAL_KEYS`, `AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID` — the AEAD keyring
  shared with runtime and the active publication/rotation key.
- `AUTH_BOT_GEMINI_TIER_EVIDENCE` — `1` enables bounded raw tier id/name in the Gemini admission
  log (diagnostics for a new Google plan). Off by default.
- `AUTH_BOT_IPROYAL_KEY` — automatic proxy issuance (empty = manual input).

The background lifecycle check refreshes the proxy expiry and, when needed, renews the same
IPRoyal allocation, but does not send periodic "Контроль прокси" ("Proxy check") reports to
Telegram.

**Deploy:** the watchdog builds the bot together with the engine and places the tested binary in
the immutable engine release; `claude-authbot.service` runs
`/srv/claude-api/releases/current/authbot`. A changed binary is restarted after promotion. On
startup, a lost in-memory Claude child is restored from persisted `ho_code` into `ho_email`, and
an interrupted ChatGPT wait from `cx_wait` into `cx_email`; the seller sends the email and gets a
fresh flow. Gemini `processing` sessions never replay an already submitted Google code: startup
uses the same generation-fenced `/cancel` path, preserves the egress and automatically issues the
seller completely new state+PKCE links.

**Verification:** `cargo test -p authbot`. A live Telegram/OAuth/Google API run — only on the
server.

## KIMI (Kimi Code) — device-code acquisition, dormant for now

The offer menu and the quick admin keyboard cover the entire paid Kimi Code ladder under the
provider's names: Andante, Moderato, Allegretto, Allegro and the top Vivace. The price is entered
per offer individually and is never assumed anywhere: the provider's USD and CNY pages diverge.

`kimi_oauth.rs` — a pure protocol module for acquiring a KIMI subscription. It does NOT own the
Telegram state, the seller job, the payout or the roster publication: the wizard that calls it
arrives as a separate dependent change, so that the new provider contract is not mixed with seller
state-machine edits in one diff. Evidence facts and labels — `docs/engine/KIMI_PROVIDER.md` §2.

- **Grant — RFC 8628 device authorization** at `https://auth.kimi.com`
  (`/api/oauth/device_authorization` → `/api/oauth/token`). This is the correct form for handoff:
  the seller sees only the short `user_code` and `verification_uri_complete` and **never** hands
  the operator a password, 2FA, cookie, token or proxy. The `device_code` never leaves the bot —
  `seller_prompt()` does not contain it, and this is pinned by a test.
- **The refresh family is rotating.** A grant without a `refresh_token` is rejected: it would die
  on the first refresh. A response with an unknown OAuth error code does not count as "keep
  polling" — otherwise a provider contract change would silently spin until the deadline.
- **Identity is taken only from `/me`**, not from what the seller entered. An empty `user_id`
  breaks quota attribution, an empty `user_level_name` collapses different calibration cohorts into
  one, a status other than `USER_STATUS_NORMAL` means an unroutable account — all three fail
  closed before anything is sealed. `email`/`phone`/`nickname` never enter the parsed structure at
  all.
- **Egress is mandatory for the entire acquisition.** A malformed proxy URL fails the acquisition
  rather than silently falling back to direct egress: opening an account from one IP and
  authorizing from another is exactly what triggers the provider's risk checks.
- **Polling boundaries.** The provider's interval is honored, but never below 5 s; `slow_down`
  increases it; the deadline of one acquisition is capped by our 15-minute ceiling regardless of
  `expires_in`, so that a stuck acquisition does not hold the seller job forever.
- The caller seals/publishes the envelope, and only BEFORE completing the payout: a failed,
  expired or wrong-plan flow must leave neither a credential file nor a roster row.

`kimi_roster.rs` — the file contract of publication. Separated from the exchange deliberately:
there is the protocol, here is the filesystem.

- **Order — envelope, then roster**, both atomic + parent fsync. The reverse order would give the
  engine a roster row whose credential file does not exist yet, and it would fail a healthy
  profile on every reload.
- **Subject is the quota identity.** KIMI quota is shared across all devices and API keys of the
  account, so the unit is `user_id`, not a key and not a profile id. Two profiles with one subject
  would double the capacity of one subscription and tear its calibration evidence into two rows.
  Re-authorization of a known subject **replaces** its profile in place, preserving the id (and
  with it affinity, health and calibration history); a new subject with an already occupied
  profile id is rejected. Replacement is not a convenience: the provider rotates the refresh
  family on every consent, so refusing would leave a knowingly dead token in the roster.
- **Fail closed:** a roster row must point exactly at the canonical path of its own id (otherwise
  editing the roster would redirect the engine to a file outside the sealed directory); a symlink,
  a world-readable file and an unreadable envelope stop publication rather than dropping the
  profile; an invalid credential does not touch the roster at all. The roster holds only the
  opaque id and path — no subject, no plan, no token.

**Seller wizard.** Steps `km_proxy → km_ready → km_wait`, button `kimi:ready`. A separate callback
per provider is deliberate: a shared id would let one deal's button advance another deal. At
`km_proxy` the bot accepts the proxy as a text message — this branch did not exist before, and the
input fell into the generic fallback "Доступна только команда /start" ("Only the /start command
is available"). Parsing is the same reversible one as Gemini's (`parse_proxy_input`, a password
with `:` survives reconstruction), and before pinning, the proxy passes
`kimi_credential::normalize_proxy_url` canonization, so garbage never reaches `km_ready` and does
not lock the seller into a device flow with a broken egress. A pinned buyer/IPRoyal proxy cannot
be replaced by the seller's message — the shared `job_accepts_seller_proxy` decides; invalid input
leaves the deal on `km_proxy` with a safe re-prompt, and only the keyless form fingerprint goes to
the log. A single-offer with the buyer's proxy and IPRoyal issuance arrive at `km_ready` through
the same `prepare_kimi_account` as a batch, so the card with the button always arrives.
`km_ready` requires both its own step AND a saved proxy — otherwise the button is inert, because
an acquisition without an assigned egress would authorize from an IP other than the one the
account was opened from. Polling runs under the generation guard: on every tick the deal is
re-checked, so cancel, step back and restart stop the acquisition instead of letting it publish
into a deal that has already moved on. Stepping back from `km_wait` extinguishes the issued code
and requires confirmation; without a recoverable egress it degrades to `km_proxy`, otherwise the
seller would land on `km_ready` with an empty `hproxy` — a dead end. An expired, rejected or
invalid flow returns to `km_ready`, leaves neither an envelope nor a roster row and **does not
complete the payout**. Token polling is read-only, so a transport failure is safely retried until
the deadline and never replays a one-time operation.

**Env:** `AUTH_BOT_KIMI_DIR` (root of `credentials/` + `profiles.json`, default
`/srv/claude-api/data/kimi`), `AUTH_BOT_KIMI_CREDENTIAL_KEYS`,
`AUTH_BOT_KIMI_CREDENTIAL_ACTIVE_KID`. Without a keyring the branch publishes nothing and honestly
tells the seller that onboarding is temporarily unavailable, instead of walking them through a
flow whose result would have to be discarded.

Verification: `cargo test -p authbot kimi`.

## GLM (Zhipu AI / Z.ai Coding Plan) — static API key

`glm_key.rs` — a pure protocol module for validating a GLM Coding Plan key. It does NOT own the
Telegram state, the seller job, the payout or the roster publication: the seller wizard in
`bot.rs` calls it step by step. Evidence facts and labels — `docs/engine/GLM_PROVIDER.md` §2
(credential/identity), §4 (wire), §7 (acquisition flow).

- **Credential — a static key from the console**; there is no OAuth device flow. The seller buys
  the exact individual credits plan (Lite/Pro/Max per the offer product) on their own account and
  sends the key to the bot as text; the bot never requests a password, 2FA or cookies — the key is
  the only credential artifact, like `sk-ant-oat01-…` for Claude.
- **The quota probe is free and read-only**: `GET {base}/api/monitor/usage/quota/limit` with the
  `Authorization: <key>` header WITHOUT a Bearer prefix. Protocol trap: an invalid key answers
  **HTTP 200 with `code: 401` in the body**, so the parser looks at the business code, not the
  HTTP status. Limit fields (`unit/number/usage/currentValue/remaining/…`) are kept raw — the
  unit semantics are unproven (`oss-hypothesis`, manifest §6), there is no interpretation.
- **The plan is corroborated by quota**: the observed 5-hour window limit (the smallest
  TIME_LIMIT window, `number` or `currentValue+usage`) must match the official credits of the
  declared plan (2,000/12,000/28,000 per 5h). A contradiction is `PlanMismatch`; the legacy
  prompts form, Team tokens (`TOKENS_LIMIT` without `TIME_LIMIT`) and ambiguous windows are
  `UnsupportedPlanShape`, fail closed without guessing.
- **Admission — one minimal paid generation** (`POST {base}/api/anthropic/v1/messages`,
  `Authorization: Bearer`, `glm-4.7`, `max_tokens=1`). Success is a 2xx with non-empty `usage`
  (`input_tokens`/`output_tokens`). Error classes are a typed `KeyVerdict`, no bools:
  1000–1005/1309/1311/1313/1315/1113 — invalid key (with a typed reason for a safe seller hint),
  1308/1310 — valid key with zero quota (`QuotaExhausted`, this is NOT an invalid key),
  1316–1321 — Team/legacy form. Other codes and responses without a business code are transport.
  A paid generation is NEVER automatically repeated after ambiguous transport; the read-only
  probe is retried with bounded backoff (1s → 30s cap), the deadline of one validation is 60
  seconds.
- **Envelope sealing — only BEFORE completing the payout**: a failed, expired or wrong-plan flow
  leaves neither a credential file nor a roster row.

`glm_roster.rs` — the file contract of publication, a mirror of `kimi_roster.rs`.

- **Order — envelope, then roster**, both atomic + parent fsync (files 0600, directories 0700):
  the engine never reads a roster row whose credential file does not exist yet.
- **The key is the quota identity.** GLM has no machine-readable `/me`, so subject dedup runs on
  the API key itself: the comparison is performed on opened envelopes inside the safe zone, the
  raw key never leaves it. Re-publication of the same key **replaces** the profile in place,
  preserving the profile id (affinity, health and calibration history survive the replacement); a
  new key with an already occupied profile id is rejected.
- **Fail closed:** a roster row must point exactly at the canonical path of its own id; a
  symlink, a world-readable file and an unreadable envelope stop publication rather than dropping
  the profile; an invalid credential does not touch the roster at all. The roster holds only the
  opaque id and path — no key, no plan, no proxy.

**Seller wizard.** Steps `glm_proxy → glm_ready → glm_wait`, button `glm:ready`. A separate
callback per provider is deliberate: a shared id would let one deal's button advance another
deal. Products — the three offers "GLM Coding Plan Lite/Pro/Max"; the `handoff_kind` rule keys on
provider words (`glm`, `zhipu`, `z.ai`, `bigmodel`, `coding plan`) and ranks above the others,
because bare tier names mean nothing (Lite/Pro/Max — Claude also has a Max). At `glm_proxy` the
bot accepts the proxy as a text message: parsing is the same reversible one as KIMI's
(`parse_proxy_input`, a password with `:` survives reconstruction), and before pinning, the proxy
passes `glm_credential::normalize_proxy_url` canonization, so garbage never reaches `glm_ready`.
A pinned buyer/IPRoyal proxy cannot be replaced by the seller's message — the shared
`job_accepts_seller_proxy` decides; invalid input leaves the deal on `glm_proxy` with a safe
re-prompt, and only the keyless form fingerprint goes to the log. A single-offer with the buyer's
proxy and IPRoyal issuance arrive at `glm_ready` through the same `prepare_glm_account` as a
batch. The `glm_ready` card offers the platform selection
(`glm:region:int`/`glm:region:cn`, default int): an `api.z.ai` key does not work on
`open.bigmodel.cn` and vice versa, so the choice is stored in `users.hregion` all the way to
`credential_from`, survives a restart and resets to the international default when each new deal
enters `glm_ready`. `glm_ready` requires both its own step AND a saved proxy — otherwise the
button is inert, because validation without an assigned egress would come from an IP other than
the one the account was opened from. At `glm_wait` the seller sends the API key as a single text
message, and the bot validates it on the deal's egress: free quota probe (bounded retry; failure
— return to `glm_ready` with a typed hint) → corroborate_plan against the declared product
(mismatch/legacy — return with a hint) → one minimal paid generation (`QuotaExhausted` — a
separate hint "квота исчерпана, пришлите позже/другой" ("quota exhausted, send later/another
one"), this is NOT an invalid key; ambiguous transport — the paid request is not automatically
repeated) → `credential_from` → `glm_roster::publish` (same key — replace-in-place; occupied id —
typed error) → payout completion ONLY after publication. All hints are static: the key is not
logged (the log sees only `key_len`), is not echoed back to the chat and is not stored in SQLite
— validation lives only in memory. Stepping back from `glm_wait` extinguishes the key wait and
requires confirmation; without a recoverable egress it degrades to `glm_proxy`, otherwise the
seller would land on `glm_ready` with an empty `hproxy` — a dead end. An invalid, wrong-plan or
transport-broken flow returns to `glm_ready`, leaves neither an envelope nor a roster row and
**does not complete the payout**. After a restart an unfinished `glm_wait` is restored into
`glm_ready` — the seller will send the key again, while the proxy and the platform selection
survive the restart.

**Env:** `AUTH_BOT_GLM_DIR` (root of `credentials/` + `profiles.json`, default
`/srv/claude-api/data/glm`), `AUTH_BOT_GLM_CREDENTIAL_KEYS`,
`AUTH_BOT_GLM_CREDENTIAL_ACTIVE_KID`. Without a keyring the branch publishes nothing — as with
KIMI, intake is gated only on the AEAD keyring, and the bot honestly tells the seller that
onboarding is temporarily unavailable, instead of walking them through a flow whose result would
have to be discarded.

Verification: `cargo test -p authbot glm`.
