# Engine integration guide (for the website backend + payments)

This document is everything you need to build a **paid website** on top of our engine **without touching Rust**.
You write: the website (registration/account dashboard), payment acceptance, your own user database. The engine
takes care of: serving the Claude API, subscription rotation, and **exact money accounting** (reserve/charge down to the nanodollar).
Your backend commands the engine over HTTP via the **Control API** (`/admin/*`).

---

## 1. Roles: who does what

| | **Engine** (ready-made, you don't touch it) | **Your service** (you write it) |
|---|---|---|
| Serving `POST /v1/messages` to clients | ✅ | — |
| Subscription rotation, limits, resilience | ✅ | — |
| **Authoritative account balance**, reserve/charge | ✅ | — |
| Accounts/keys/journal in engine-owned PostgreSQL | ✅ | — |
| Website, registration, account dashboard | — | ✅ |
| Payment acceptance (Stripe/crypto/…) + webhooks | — | ✅ |
| Your own DB: users, passwords, user→account_id mapping, payment history | — | ✅ |
| Engine Control API calls | — | ✅ |

**Model:** the engine is the source of truth for money. Your service stores PEOPLE and PAYMENTS, and credits
and reads money at the engine. One `account` in the engine = one client (or team) on your side.

---

## 2. Access

- **Production base for the API/worker on the same host:** `http://127.0.0.1:8790`. This is an explicitly
  loopback-bound Caddy origin that health-routes the active engine slot 8787/8788. Never pin a
  commerce consumer to a specific slot port. From another host the Control API must go only over an
  authenticated private network/TLS; the public engine domain does not expose admin routes.
- **Your key:** `CONTROL_KEY` (issued separately). Send it in the header **`x-api-key: <CONTROL_KEY>`** on every
  `/admin/*` request. The same key **cannot** serve `/v1` — it is management-only (a compromised
  backend ≠ free inference).
- All bodies are **JSON**. All money amounts are integer **nanodollars**: `1 USD = 1 000 000 000 nano`. No floats
  in money — work in nano and divide by 1e9 only for display.

### Operator telemetry of subscriptions

The same-origin admin panel additionally reads `GET /capacity`, `GET /codex-subs`, `GET /gemini-subs`,
`GET /kimi-subs` and `GET /glm-subs`.
These routes are protected by server-side control/panel auth; the browser reaches them only through the closed
`admin.apitoken.sale`, and no keys are issued to it.

Authbot owns a separate loopback proxy-lifecycle contract, also exposed only through that closed
admin vhost. Its dedicated authorization secret is the stable raw
`/etc/apitoken/proxy-admin.key`: the `/etc/apitoken` parent is root-owned and
non-deploy-writable, and the key is a `root:root` `0600` regular non-symlink file containing exactly
64 lowercase hex bytes plus an optional final LF. This avoids placing the authority below the
deploy-writable `/srv/claude-api/data` parent. The installer provisions it atomically before
installing either the unit or Caddy configuration. It migrates one exact legacy
`AUTH_BOT_PROXY_ADMIN_KEY` assignment out of `authbot.env`; malformed, duplicate, or divergent
legacy/file values fail installation rather than choosing one. `server.env` is never a migration
source: an `AUTH_BOT_PROXY_ADMIN_KEY` or `AUTH_BOT_PROXY_ADMIN_KEY_FILE` assignment there rejects
installation.

Systemd uses
`LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key` to give authbot a private per-service
copy. It first loads `authbot.env`, `engine-postgres.env` and the optional `server.env`, then its
`ExecStart=/usr/bin/env ... AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key ...` command assignment
pins the path passed to the bounded Rust parser. The path is deliberately not set with
`Environment=`, so no environment file can redirect it. The parser accepts only that dedicated file
and the exact format above. The root-run Caddy installer and renderer use only the `/etc/apitoken`
raw path. The renderer matches an existing live `X-Proxy-Admin-Key` header name case-insensitively
and rejects duplicate or mismatched live values. Caddy sends the canonical header; it also overwrites
`x-api-key` with the shared key solely so the previous authbot binary remains available during
mixed-version rollout or rollback. The new listener ignores that compatibility header and
authenticates only the dedicated header, so possession of the shared key cannot authorize access to
`account_email`. The dedicated value is not placed in sibling service environments or credentials.
`ProtectProc=invisible` and `ProcSubset=pid` remain in force. After operator-subcommand
early-return handling and before loading daemon secrets, Linux authbot calls
`prctl(PR_SET_DUMPABLE, 0)`, blocking same-UID `ptrace`, `process_vm_readv`, and sensitive proc-memory
access. Code already executing inside authbot itself is within the same trust boundary, and no
defense can protect against such in-process code. Authbot otherwise uses the shared key only for outgoing
`/codex-subs` and `/gemini-subs` status reads:

- `GET /proxy-admin/inventory` returns schema version, observation time, IPRoyal balance as an exact
  decimal nanoUSD string, an aggregate auto-extend warning, and sanitized rows. The item key set is
  `inventory_id`, `account_email`, `proxy_hint`, `order_hint`, `provider`, `subscription_plan`,
  `liveness`, `subscription_expires_at`, `proxy_expires_at`, `binding_status`, `renewable`,
  `renew_block_code`. `account_email` is restricted to an ASCII local part using alphanumerics plus
  ``.!#$%&'*+/=?^_`{|}~-`` (no edge/consecutive dots, at most 64 bytes), followed by DNS-style
  domain labels (alphanumeric/hyphen, no edge hyphen, at most 63 bytes each and 254 bytes total). Full
  `account_email` is the sole managed-admin identity exception: it is
  allowed only in this closed `managed_admin_auth` `/proxies` response, marked `no-store` and kept
  in memory, never SQLite or logs. Tokens, subjects, projects, raw IP/proxy URLs/credentials and all
  other identities or secrets remain forbidden. `/capacity`, `/codex-subs` and `/gemini-subs`
  continue to expose only masked email.
- Items are only subscription-backed rows with a durable exact binding to an existing IPRoyal
  allocation (`binding_status=bound`) and liveness other than `dead`. Unmatched allocations,
  external/unbound/mismatched subscriptions and dead subscriptions are not serialized. Unique-IP
  reconciliation for legacy/external profiles may continue in the background, but no row is visible
  until the exact binding is durable. GPT rows retain public `provider=gpt`, while their durable
  binding namespace is `codex`. A legacy `gpt` binding migrates in place only on one exact local
  id + order + allocation-IP match, preserving `inventory_id`; unresolved, mismatched or ambiguous
  rows are not adopted. Claude and Gemini use the same public and binding provider name.
- GPT/Gemini liveness is joined authoritatively by opaque id to sanitized loopback `/codex-subs` and
  `/gemini-subs` using the shared engine control key. Authbot trusts only id plus status; unavailable,
  malformed, missing or duplicate status evidence closes that provider. GPT accepts exactly
  `account_state=healthy|suspect|dead`; an empty or unknown value is schema drift and closes the
  whole GPT source. Gemini `authenticated!=true` is dead; `disabled=true` is degraded and
  nonrenewable, not dead. The configurable origins are
  `AUTH_BOT_PROXY_ADMIN_CODEX_RUNTIME_URL` and
  `AUTH_BOT_PROXY_ADMIN_GEMINI_RUNTIME_URL`, defaulting to
  `http://127.0.0.1:8792/codex-subs` and `http://127.0.0.1:8794/gemini-subs`. They accept only HTTP
  with a literal loopback host, the exact provider path, and no credentials, query, fragment or
  external origin.
- `POST /proxy-admin/renew` accepts only `{idempotency_key: UUID, inventory_ids: string[1..100]}`
  and additionally requires the verified `X-Admin-Actor`. Before a paid extension it repeats the
  durable exact-binding, authoritative-liveness and local subscription-expiry checks;
  `subscription_expires_at <= now` fails as `local_profile_inactive` without calling the provider.
  It groups allocations by order and reports per-inventory `renewed|failed|uncertain`. After exact
  same-key replay handling, a different UUID overlapping a `pending` or `in_progress` inventory ID
  or exact order/allocation receives `409 renewal_selection_busy` before insertion, so it cannot
  remain queued for later spend; disjoint selections can proceed. Claiming also repairs legacy or
  corrupt overlap atomically: an explicit direct claim is the winner, while the background claimant
  chooses the oldest `(created_at,id)`; existing `in_progress` work wins over pending rows. The winner
  becomes `in_progress` and every overlapping pending sibling becomes terminal `indeterminate` in
  the same `IMMEDIATE` transaction. Such a sibling replays as uncertain with no provider call and can
  never later be claimed. Idempotency is unchanged; an uncertain paid POST is never automatically
  replayed.

Every IPRoyal purchase has `auto_extend=false`; a free background guard disables unexpected
order-level auto-extend across the complete inventory and confirms the state by exact refetch. No
background task performs a paid extension. Subscription lifecycle is 30 days for Claude/GPT, 18
Gregorian UTC calendar months for Gemini `google_ai_pro`, and 30 days for other Gemini plans.

`GET /spend-stats` (windows `d1`/`d7`/`d30` plus an optional `from`/`to` range) is additionally consumed
server-side by the commercial backend for `GET /admin/finance/engine-spend`: it is the only source that
also covers engine accounts with no commerce user (OpenKeys, service/manual accounts). It stays a
read-only operator projection in engine USD numbers — never a money authority for commerce.

`GET /capacity` additionally publishes Claude `window_totals`, horizon `available_nano` and
`conversion_models`. Money fields for calculations are decimal nanoUSD strings. The catalog comes from
`metering` and separates Standard/Fast input, cache-read, cache-write 5m/1h and output; Web Search has
a separate per-request rate. Per-sub `rem5h_nano`/`rem7d_nano` and email mask let the panel
draw compact windows without float money and without exposing the account. Each `per_sub` object also
has nullable `acquired_at`, `subscription_expires_at` (Unix seconds) and `subscription_days_left`
(fractional days at response `now`, negative after expiry). These values come from registry `added_ts`;
the server joins by the full internal email before serializing the mask, never by the non-unique mask.

Claude capacity is the exact realized API-dollar equivalent of the actually served mixture, not the
Max/Pro price and not a promise of a fixed number of tokens. Every successful turn (customer or admin)
persists an immutable model/tier/geo/tariff event with separate token and API nanoUSD legs; a free
poll persists only a quota observation. After authoritative usage the backend itself enqueues the spend event
into a durable FIFO and wakes a free post-turn count-tokens probe of the serving subscription; opening
the admin panel and a follow-up user request are not needed to accumulate evidence. A forced probe
is debounced to once per 15 seconds per subscription, and a poll observation always drains the pending
turn FIFO before reading cumulative spend. An empty plan is restored by the backend probe via the official
OAuth profile endpoint; an inference-only token with a 403 can only inherit the unanimously known
paid plan (`pro|max5|max20`) of the current fleet. A mixed/unknown fleet stays fail-closed. The discovered plan
is durably recorded in the registry and applied to the live roster before the quota observation;
opening the admin panel or manually editing the cohort is not required. If the response and the post-turn poll land in
the same second, the changed quota is still accepted in FIFO order; only an exact endpoint duplicate
is ignored. For each exact plan and window independently:

```text
capacity_per_subscription_nano =
  100_000_000 × Σobserved_spend_nano / Σobserved_fraction_units
```

The 5h (`300` minutes) and 7d (`10080` minutes) windows do not share anchor/history. `plan_cohorts` pools evidence
only of the same `plan + window_minutes`, so all routable subscriptions of one plan get
one pooled estimate; `window_totals` sums it across the routable fleet. Another plan without its own
positive evidence makes the fleet total `null`, not a partial sum. The subscription face value,
configured prior, EMA/WLS and float money do not participate in the authority.

The current remaining requires a quota snapshot no older than 900 seconds. Historical full-window capacity
may remain known with a stale/missing snapshot, but remaining/horizon are then `null` with an exact
`missing_reason`. The same fail-closed rule applies while FIFO delivery of exact turn evidence has a
pending event or degraded integrity. `calibration_delivery` publishes `pending_events`,
`dropped_events`, `persistence_ok` and `queue_limit`; the normal state is `0/0/true`. A failed head
survives a transient authority outage in memory and is retried ahead of later events/snapshots;
immutable replay is idempotent, and a semantic conflict is isolated and increments the dropped diagnostic.
The last exact provider snapshot remains available only as a diagnostic display-state until
its future reset: `windows[].used_fraction_units`, `resets_at`, `last_known_quota_source` and
`last_known_remaining_nano` keep their previous value, while the top-level `reset5h_in`/`reset7d_in`
keep counting down. This covers both an idle routable subscription and quota-cooling,
when a new probe is deliberately deferred until reset. A new snapshot replaces the display-state immediately;
after the provider deadline the old fraction/reset/last-known remaining become `null` and are not
carried into the new window. `windows[].snapshot_fresh` stays `false` all this time, and the canonical
`remaining_nano`, fleet remaining and horizon stay `null`: `last_known_remaining_nano` must not be
treated as currently sellable capacity. Pending/degraded calibration delivery also does not
publish this display-state.
`calibration_evidence` contains aggregates of real requests by masked email/model/tier/geo/tariff with all
token/cost legs; for the UI they can be sorted by `api_total_nanousd`.
`calibration_recent_turns` is a bounded newest-first window of up to 512 individual immutable Anthropic events.
Each row contains an opaque internal `request_id`, the same masked email, the full model/tier/geo/
tariff identity and all token/cost legs; the prompt, full email and credential are not published.
`calibration_recent_turn_limit=512` fixes the server-side bound. This window is meant for exact
operator attribution of a live test via request-id set difference; aggregates must not be used
for that, because parallel customer traffic legitimately changes the same row.

The bounded production run and the rules for interpreting model-level quota deltas are described in
`docs/ops/CLAUDE_CALIBRATION.md`; the runner uses only this backend contract and does not depend on the UI.

`GET /overview` keeps the previous rounded `supply.*_usd` display fields for the panel, but their source
is now the same exact report. The canonical values live next to them in `supply.avail_nano`,
`cap_nano`, `consumed_nano`; `supply.legacy_pool_prior_authoritative=false`. Without exact
evidence, capacity-facing fields fail closed to `null` instead of falling back to the old pool prior/EMA.

`GET /codex-subs` separates two different notions:

- `*_nanocredits` — native spend and capacity of the ChatGPT subscription; identical plans are compared
  in credits;
- `*_nano` / `*_nanousd` — the official public API replacement cost of the actual or selected
  workload. It varies with the model, Standard/Fast, cache mix, output and long context, and is not
  a fixed subscription face value.

On every home, `calibration_evidence` contains immutable aggregates by model/effective tier/
provider-reported tier/tariff schedules: turns, fresh-derived total input, cached input,
cache-write, output/reasoning, all API legs and all ChatGPT-credit legs. This evidence appears
after the first successful turn and does not wait for quota movement. `capacity_nanocredits` stays `null` until
a confirmed positive `Δquota` appears; `null` does not mean zero. The integrity fields
`calibration_pending_events`/`calibration_dropped_events` must be `0/0`.

`measurement_resolution_fraction_units` on a window reports the real numeric resolution of the
quota snapshot: for a typical whole `40%` it is `1_000_000`, not `1`. The low/high estimator v10
accounts for half the resolution of both endpoints; if the quota movement is no larger than this
tolerance, the upper bound honestly stays `null`.

For the commercial answer on identical subscriptions, use the root `plan_cohorts`, grouped
by exact `plan + window_minutes`. `capacity_per_home_nanocredits` is one shared pooled estimate for
each home of that cohort, and `fleet_capacity_*`/`fleet_remaining_*` are its size and current remainder
across the whole cohort. The point estimate formula:

```text
capacity_per_home_nanocredits =
  100_000_000 × Σobserved_spend_nanocredits / Σobserved_fraction_units
```

`measured_homes` shows the number of contributors, `homes_total` — the cohort size. Low/high is a
conservative shared envelope; if at least one contributor does not yield a finite upper bound,
the cohort high is also `null`. Per-home `capacity_nanocredits` is not overwritten and remains raw audit
evidence, so its spread with whole-percent quota is expected. `window_totals` also remains the sum of
individual estimates. API USD must not be taken from `plan_cohorts`: it depends on the workload and is computed
via the conversion formula below.

The response root also publishes `conversion_models`: versioned API/credit rates, independent Fast
multipliers and long-context modifiers. All money and credits are serialized as decimal strings; tokens,
percentages, timestamps and counters are numbers. Email is only a bounded mask without a domain. Every home
also publishes nullable `acquired_at`, `subscription_expires_at` and `subscription_days_left` from the
sealed credential's immutable `issued_at`; Codex expiry is exactly 30×86400 seconds later. The UI must compute
workload conversion via BigInt:

```text
API equivalent nanoUSD = capacity_nanocredits × workload_api_nanousd / workload_nanocredits
```

Reasoning is a diagnostic subset of output and is not added a second time. Cache-write has its own API
rate but is included in fresh input on the credit card.

`GET /gemini-subs` publishes canonical `capacity_nano`/`remaining_nano` fleet totals,
`conversion_models` from `metering::gemini` and `quota_model_ids` for joining a public model with its
Antigravity effort buckets. `remaining_amount` is serialized as a decimal string; if Google returns
only `remaining_fraction`, the token/unit quantity remains unknown and is not derived from a
workload-dollar blend. Profiles add the same nullable lifecycle fields from immutable credential `issued_at`:
canonical `google_ai_pro` expires after 18 Gregorian UTC calendar months (time-of-day preserved and an invalid
month-end clamped), while every other canonical Gemini plan expires after exactly 30×86400 seconds.
`subscription_days_left` is fractional at response `now` and may be negative. The profile contains only a
bounded email hint (four characters of the local part without the domain); full email, subject, project,
private tier, proxy and OAuth are not serialized.

`GET /kimi-subs` is a read-only operational projection of the backend-only KIMI plane. Production
is served by a dedicated default-off KIMI plane via the stable loopback origin 8803
(`claude-api-kimi@8804/8805`); the gateway built into the Anthropic runtime remains dev/test-only.
The gate is the control key
(`control_authed`, like `/codex-subs`; the panel key does not qualify). On a process without the plane, the response is a
disabled envelope `{"now": <unix>, "enabled": false, "profiles": []}`. An enabled envelope publishes
`delivery` (pending/dropped/persistence bounded FIFO), fleet counts (total/live/available
profiles, inflight, three cooling axes) and per-profile objects with cooling-until timestamps, the last
quota snapshot per window (`used`/`limit` — provider authority, with the fraction and the real measurement
resolution alongside) and per-window calibration from durable PostgreSQL evidence (samples, confidence,
capacity/remaining as decimal nano strings, estimator version). For a safe live runner
the envelope additionally publishes `calibration_authority_available`,
`calibration_recent_turn_limit` and `calibration_recent_turns` — immutable turn events
(engine request id, opaque profile id, bounded plan label, served/requested model, full usage
and exact nano-legs as decimal strings) — plus `conversion_models` with the official rate card for
worst-case bounds. Redaction contract: only
opaque profile ids and reviewed bounded plan labels are serialized; subject, email, phone, token, proxy,
credential path, customer/request id and raw provider errors are never serialized; the unknown is
`null`, not 0. The plane is default-off: while KIMI is not enabled, the envelope is always disabled.

Exact runner attribution is supported at dispatch: the admin-only headers
`x-apitoken-calibration-profile` (full opaque id, 1..128) and
`x-apitoken-calibration-request-id` (UUIDv4) come as a pair, only under the admin key, and never
go upstream; a half-pair, a non-admin key or a garbage value is rejected with 400. A pinned turn
uses the passed immutable id and executes on exactly the specified profile: cooling/wall is
a wall, not a reason to rebind to a neighboring profile.

`GET /glm-subs` is a read-only operational projection of the backend-only GLM (Z.ai Coding Plan)
plane, which lives inside the Anthropic runtime (the same origin 8790, no separate
process). The gate is the control key (`control_authed`, like `/codex-subs` and `/kimi-subs`; the panel key
does not qualify). On a process without the plane, the response is a disabled envelope `{"now": <unix>,
"enabled": false, "profiles": []}`. An enabled envelope publishes `delivery` (pending/dropped/
persistence bounded FIFO), fleet counts (total/live/available profiles, inflight, the durable
account-dead/account-suspect axes and two timed cooling axes), `window_totals` (fleet aggregation of the two
canonical windows 5h/7d: `window_minutes` 300/10080 — a projection of the exact `duration_secs`; capacity and
remaining as decimal nanoUSD strings, the aggregate `null` until at least one fleet profile names
a value for the window — a partial sum is never published) and per-profile objects with durable
account flags, cooling-until timestamps, the last quota snapshot per window (raw counters
`null` while their unit semantics are unproven) and per-window calibration from durable PostgreSQL
evidence (samples, confidence, capacity/remaining as decimal nano strings + exact native
microcredits, estimator version). Redaction contract: only opaque profile ids and
bounded plan labels are serialized (the roster is limited to three reviewed individual plans anyway); the key's
subject-digest, the key itself, proxy, base_url, credential path, customer/request id and raw provider errors
are never serialized; the unknown is `null`, not 0. The plane is default-off: while GLM is not
enabled, the envelope is always disabled.

---

## 3. Money model (mandatory to understand)

An account has three "buckets"; the engine maintains the invariant:
```
free_balance + reserved + spent = credited   (always, down to the nanodollar)
```
- **balance_nano** — free money available to spend right now.
- **reserved_nano** — temporarily held for "in-flight" requests (the engine reserves a ceiling before
  the request and returns the difference after). You don't touch this — just know that "in the moment" the balance
  may be slightly lower by the amount of in-flight requests.
- **spent_nano** — total spent (monotonically increasing).
- **mult_bp** — the current legacy scalar in basis points: `2000 = ×0.20`. After Stage 9 the client
  price comes from the immutable release/policy rule; the scalar remains a migration/audit source and
  is not a fallback. Service uses the separate `meter_only`, not `mult_bp=0`.

B2C/B2B/OpenKeys physically cannot go negative: if money runs short, the engine will trim the response to
the balance or return `402`. Service is an explicit exception: official usage is accounted durably, but the
balance is neither reserved nor debited.

---

## 4. Canonical scenarios

### A. Client registration
1. A user registers on your website → you create a record in YOUR OWN DB.
2. `POST /admin/account` → the engine returns `account_id` (`acct_…`). Store it next to the user.
   A retry with the same non-empty `handle` returns the same account, so registration recovery
   is idempotent and does not create orphaned accounts.
3. `POST /admin/key` with this `account_id` → the engine returns **`sk-pool-…`**. For a strict account
   the request must include the exact `activation_policy_ack` from the applied active policy. Show the key
   to the user **once** (it is their API key, a secret). An account can have many keys.

### B. Payment → credit (IDEMPOTENT!)
1. The user pays → your payment provider sends you a **webhook**.
2. You validate the webhook and call `POST /admin/account/{id}/credit` with `amount_nano` (string) and
   `ref` = **provider-qualified transaction id** of the form `<provider>:<transaction-id>`
   (for example, `stripe:pi_123`).
3. The engine credits **idempotently by `ref`**: if the provider delivers the webhook twice — the second time
   will **NOT double** the credit (it returns the same balance). A positive credit WITHOUT a provider-qualified `ref`
   is rejected with `400` — this guarantees that identical transaction ids from different providers
   cannot collide in the global UNIQUE index.

### C. Account dashboard (balance/keys/history)
- Balance/spend: `GET /admin/account/{id}` → `balance_nano`, `spent_nano`, `reserved_nano`.
- User's key list: `GET /admin/account/{id}/keys` (non-secret `key_id`, mask,
  label/status/spend).
- Payment/spend history: `GET /admin/account/{id}/ledger?limit=50` (top-ups/charges, newest first).
- Spend breakdown by models/tokens: `GET /admin/account/{id}/usage?window=30d` (for the dashboard).

### D. How the client USES the API (what to show them in your docs)
The client points any Anthropic-compatible tool at our base and their `sk-pool-` key:
```bash
curl https://<base>/v1/messages \
  -H "x-api-key: sk-pool-…" -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model":"claude-opus-4-8","max_tokens":1024,"messages":[{"role":"user","content":"hi"}]}'
```
The client checks their own balance: `GET /v1` → no; **`GET /balance`** with their own `sk-pool-` key
(`x-api-key`) → JSON with balance/spend. Everything else is the pure Anthropic API (streaming, tools, count_tokens).

---

## 5. Control API reference (`x-api-key: <CONTROL_KEY>`)

### Accounts
```
POST /admin/account                     {"handle"?, "mult_bp"?}      → 200 {account, mult_bp, handle}
POST /admin/accounts/query              {"account_ids":["acct_…"]}   → 200 {accounts:[{account,
                                                                            balance_nano,spent_nano,
                                                                            reserved_nano,balance,mult_bp,
                                                                            status,handle}]} (1..500 id;
                                                                            400 on an empty/invalid list)
GET  /admin/account/{id}                                             → 200 {account, balance_nano, spent_nano,
                                                                            reserved_nano, balance, mult_bp, status, handle,
                                                                            funding:{...}} | 404
POST /admin/account/{id}/credit         {"amount_nano": "25000000000",
                                         "ref": "<provider>:<tx>"}   → 200 {account, balance_nano, balance} | 400 | 404 | 409
                                        (amount_nano only — a decimal i64 string in nano;
                                         unknown fields are rejected with 422. Idempotent by ref;
                                         for a positive amount ref is REQUIRED in the format
                                         <provider>:<transaction-id>; amount < 0 = debit/correction,
                                         ref optional for it; 409 — ref already used
                                         by another payment)
POST /admin/account/{id}/status         {"status":"active"|"disabled"}  → 200 {account,status,updated} | 404
POST /admin/account/{id}/pricing        {"mult_bp":0..10000}             → 200 {account,mult_bp,updated} | 404
GET  /admin/account/{id}/keys                                        → 200 {keys:[{key_id,key_masked,label,status,
                                                                            spent_nano,spent,reserved_nano,
                                                                            spend_limit_nano,expires_ts,
                                                                            created_ts,last_used_ts}]}
GET  /admin/account/{id}/ledger?limit=N[&after_id=ID]                 → 200 {entries:[{id,kind,request_id,
                                                                            amount_nano,ref,ts,provider,
                                                                            official_nano,attribution,
                                                                            funding_allocations,...}]}
POST /admin/account/{id}/ledger/ack     {"last_id": "12345"}          → 200 {account, consumer:"pricing",
                                                                            last_id} | 400
                                        (durable watermark for consumer="pricing": a decimal string
                                         of a non-negative integer; retention deletes old
                                         charge detail only below the watermark)
GET  /admin/account/{id}/usage?window=30d                            → 200 {account, window, since_ts,
                                                                            until_ts, requests,
                                                                            total_official_nano,
                                                                            total_charged_nano,
                                                                            buckets:{...}, models:[...],
                                                                            daily:[...],
                                                                            daily_providers:[...],
                                                                            keys:[...]} | 404
                                        (window = <n>d | <n>h | all; by default and on an
                                         unrecognized value — 30d)
```

Without `after_id`, ledger entries are the newest bounded history. With `after_id`, entries are
returned oldest-first with `id > after_id`; this is the durable worker cursor for usage attribution,
funding validation and referral commission. It is no longer a tier/progressive-pricing authority in
the target contract.

`funding` is read together with the scalar account aggregates from one snapshot. It contains
`account_class`, `funding_enforcement`, `reconciliation_state`, `bucket_count` and, for
`balance/reserved/spent`, separate `paid_*_nano`, `bonus_*_nano`, `other_*_nano` and
`unattributed_*_nano`. In the current schema `bonus` may reference the historical
`welcome_track_bonus`; target writers create the provider-independent `welcome_bonus`, available to any
B2C model. `paid` means durable paid funding. Online Stage 6 classifies the exact welcome
remainder, and all other legacy residual as paid per the approved contract; the manual reviewer artifact
is not used. An active legacy reservation does not require an idle gap if the exact source state proves a
fully paid reserve: the normalization transaction simultaneously creates the generation/head and its
immutable paid-only funding snapshot without changing the persisted pricing identity. An ambiguous live
welcome reserve remains a typed blocker.

A new ledger row persists the expand-compatible top-level `request_id`, `provider` and `official_nano`.
For a pre-column charge with `ledger.provider IS NULL`, the reader first looks up immutable `usage_events` by the
exact `account_id + request_id`. For an older pair where both `request_id IS NULL`, only
the full settlement fingerprint is allowed: the same account, null-safe key/ref/model, exact
`charge_nano = amount_nano` and a timestamp difference of no more than one second. The provider is returned
only if all candidates contain the same non-empty value; ambiguity stays `null`,
and a disagreement between the recovered value and the persisted ledger provider closes the read with an error.
The model participates only in the full fingerprint and is never converted into a provider.
`attribution` is `null` for a historical row without `attribution_schema_version`; otherwise it
carries the persisted snapshot/policy/rule/catalog/switch/tariff/eligibility/runtime-manifest fields,
`official_cost_json`, categorical funding totals and the original `funding_allocation_json` without
re-resolving. `funding_allocations` is always an array of normalized durable
allocations (`bucket_id`, `source_type`, `source_ref`, `bucket_version`, `direction`, `amount_nano`,
optional `allocation_order`); old rows honestly return an empty array. All `*_nano`, ledger
IDs and generations remain integer JSON values; `packages/contracts` normalizes them into decimal
strings before they reach JavaScript business logic.

After Stage 9, release-v2 settlement charge rows carry `attribution` with
`snapshot_kind="release_v2"` and `attribution_schema_version=2`: release lineage
(`release_schema_version`, `release_generation`, `release_digest`, `release_billing_mode`,
`release_funding_generation`), `account_class`, the exact `paid_funded_nano`/`bonus_funded_nano`/
`other_funded_nano` split of the actual charge (their sum always equals `amount_nano`),
the `snapshot_digest` of the reserve-time snapshot and `funding_allocation_json` with the v2 lot identity
(`lot_id`, `lot_source_type`, `lot_version`, `direction`, `amount_nano`, `allocation_order`), mirroring the
durable `funding_ledger_allocations_v2`. The legacy fields `pricing_mode`, `rule_origin` and
`*_eligible` on such rows remain `null`: commission eligibility for the release-v2 consumer
is computed by the consumer itself from `account_class` + `paid_funded_nano`, not from the pricing mode. A `meter_only`
(service) settlement does not create a charge row at all.

`GET /admin/account/{id}/usage` aggregates the persisted immutable settlement components over
a fixed half-open interval `[since_ts, until_ts)` — it is NOT a recompute against the current price list. All
`*_nano` in the response are decimal strings; tokens, requests and timestamps are numbers. `buckets` splits the
official cost into `input`, `output`, `cache_read`, `cache_write`, `web_search`; rows that
cannot be honestly attributed to components (legacy) land in `unattributed_legacy`, and the sum of all
buckets always equals `total_official_nano`. `total_charged_nano` is how much was actually charged to the
account after the multiplier. `models`, `daily`, `daily_providers` and `keys` give the same breakdown
by models, days, providers and masked keys.

### Access keys
```
POST /admin/key                         {"account_id", "label"?,
                                         "spend_limit_nano"?, "expires_ts"?,
                                         "activation_policy_ack"?: {
                                           "effective_policy_version": integer,
                                           "policy_digest": string
                                         }}
                                                                     → 200 {key:"sk-pool-…", key_id:"key_…", account,
                                                                            label,spend_limit_nano,expires_ts}
                                                                       | 400 | 409  (key is visible 1 time!)
POST /admin/key-id/{key_id}/status      {"status":"active"|"disabled",
                                         "activation_policy_ack"?: {
                                           "effective_policy_version": integer,
                                           "policy_digest": string
                                         }} → 200 {key_id,status,updated} | 400 | 404 | 409 (recommended)
POST /admin/key-id/{key_id}/label       {"label":"…"}                → 200 {key_id,label,updated} | 400 | 404
                                        (1..64 characters after trim)
POST /admin/account/{id}/key-id/{key_id}/policy
                                        {"spend_limit_nano":string|null,
                                         "expires_ts":integer|null}
                                                                     → 200 {key_id,spend_limit_nano,
                                                                            expires_ts,updated} | 404 | 409
```

`key_id` gives no access to `/v1` and is safe to store in the commerce PostgreSQL. The new backend
must revoke keys by `key_id`, so that a usable `sk-pool-…` is never persisted.
The full key never appears in a URL; the legacy endpoint has been removed.

For a strict account, issuing a new key and moving a disabled key back to `active` require an ACK
that matches the current active policy's `effective_version` and `content_digest` verbatim.
A missing, stale or incorrect ACK returns `409`; a syntactically valid but invalid identity
(non-positive version, empty/untrimmed digest) returns `400`. Disabling a key
does not require an ACK. For a legacy/shadow account the field is optional, but if it is passed, the engine
still checks the exact match and does not accept an ambiguous confirmation.

`spend_limit_nano` is an optional positive decimal string and caps lifetime charged platform spend
for that key. `expires_ts` is an optional future Unix timestamp in seconds. The engine enforces both
again inside the atomic reservation transaction, including in-flight holds, so concurrent requests
cannot cross a key's cap. `NULL` means unlimited/no expiration and preserves legacy behavior.
The policy endpoint is an account-scoped full replacement: both nullable fields are required.
It can increase or clear a limit and extend or clear expiry without changing key status. A new
limit below `spent_nano + reserved_nano` is rejected atomically with `409` and code
`limit_below_committed`, so an edit cannot invalidate an in-flight reservation.

### Versioned multi-provider pricing (Stage 3C)

Pricing control is an explicit `prepare → read → activate` protocol. Preparing an immutable
version never changes traffic. Activation is a monotonic compare-and-set (CAS), and callers must
send the exact expected active target. Catalog, switches, and account policy are separate heads;
the supported order for a new release is catalog first, then switches, then policy.

```
POST /admin/pricing/catalog/prepare
GET  /admin/pricing/catalog/{product_id}/version/{generation}
GET  /admin/pricing/catalog/{product_id}/active
POST /admin/pricing/catalog/{product_id}/activate

POST /admin/pricing/switches/prepare
GET  /admin/pricing/switches/version/{generation}
GET  /admin/pricing/switches/active
POST /admin/pricing/switches/activate

POST /admin/pricing/policy/prepare
GET  /admin/pricing/policy/{account_id}/version/{effective_version}
GET  /admin/pricing/policy/{account_id}/active
GET  /admin/pricing/policy/{account_id}/state
POST /admin/pricing/policy/{account_id}/activate
POST /admin/pricing/policy/{account_id}/locked-openkeys-transition
```

Prepare bodies are the complete immutable `PricingCatalogSpec`, `ProviderSwitchSpec`, or
`AccountPolicySpec`. They include schema/capability generations and digests, content digest, full
entries/rules, and all policy lineage pins. Unknown JSON fields are rejected. A prepare ACK returns
`result=stored|unchanged` and echoes the complete immutable identity; the same version with a
different body is `409 version_conflict`.

Catalog activation repeats the complete prepared immutable spec rather than only its compact
version/digest target:

```json
{
  "catalog": {
    "product_id": "main",
    "generation": 2,
    "schema_version": 1,
    "capability_generation": 4,
    "capability_digest": "sha256:capability...",
    "content_digest": "sha256:catalog...",
    "entries": []
  },
  "expectation": {"exact": {"version": 1, "content_digest": "sha256:..."}}
}
```

Switch activation likewise sends `switches` with the complete prepared `ProviderSwitchSpec` plus
the CAS `expectation`. Use `"expectation":"absent"` only for the first catalog or switch head.
Account-policy activation sends the complete prepared `policy`, the complete target `binding`, and
`expectation="unbound"|{"inactive":...}|{"exact":...}`. Catalog `product_id` and policy
`account_id` must match the URL. Before CAS, the engine reads the named immutable generation and
requires exact spec equality; a missing prepared version is `missing_dependency`, while different
content under the same version is `version_conflict`.

Successful activation returns `result=applied|unchanged`. Exact retry after a lost ACK returns
`unchanged` for the same committed target. Its identity echoes the complete catalog/switch spec, or
the complete policy plus derived binding target, together with the expectation. Rejections are
typed and retain evidence:

- `400 invalid` — malformed schema, rules, identity, binding, or unsupported strict state;
- `409 missing_dependency` — required prepared/active catalog or switches are absent;
- `409 stale` — target is older than durable state;
- `409 version_conflict` — same version has another digest/content;
- `409 cas_mismatch` / `policy_cas_mismatch` — expected head/binding differs; response includes
  the actual durable state;
- `423 locked` — immutable legacy policy cannot be replaced.

#### Shadow lineage rebind (B2C↔B2B class change)

An account's policy lineage identity (`policy_id`, owner, `account_class`, `product_id`) is
immutable once established — with one additive exception. While the account's stored binding is
`policy_enforcement="shadow"` (pre-strict rehearsal, where billing still runs off the legacy
scalar), a prepare whose spec carries a different class/product identity is accepted as a rebind:
the new lineage starts its own `policy_version` sequence while `effective_version` must stay
monotonic across the whole account history. Activation of a rebind version still CAS-pins the
exact OLD lineage target and requires the target binding to remain `shadow`; it atomically moves
the binding row to the new class/product. This is the delivery path for B2C→B2B conversions,
which move the account from the shared `global-b2c` policy to its own `b2b_client` policy. Once
enforcement is `strict`, identity is fully immutable and a class/product change is
`409 version_conflict`. A same-class lineage change (different `policy_id`/owner without a
class/product change) is never a rebind and stays `409 version_conflict` even under `shadow`.

`GET .../state` reads the live scalar, policy binding, pinned policy catalog/switches, and current
admission catalog/switches in one database snapshot. Stage 3C does not backfill data, issue keys,
enable strict enforcement, or bypass the catalog → switches → policy order.

#### Replacement-locked legacy OpenKeys transition

`POST /admin/pricing/policy/{account_id}/locked-openkeys-transition` is an additive
producer-first exception for one migration shape; it is not a general policy-unlock operation.
The request is:

```json
{
  "policy": {
    "account_id": "openkeys-account",
    "effective_version": 2,
    "policy_id": "openkeys:openkeys-account",
    "policy_version": 2,
    "source_policy_digest": "sha256:managed-source",
    "owner_type": "open_keys",
    "owner_id": "openkeys-account",
    "account_class": "open_keys",
    "product_id": "openkeys",
    "schema_version": 1,
    "catalog_generation": 5,
    "switch_generation": 5,
    "content_digest": "sha256:managed-policy",
    "replacement_locked": false,
    "rules": [
      {
        "rule_id": "openkeys-anthropic-1to1",
        "rule_digest": "sha256:anthropic-rule",
        "scope": {"provider": {"provider_id": "anthropic"}},
        "pricing_mode": "discount",
        "rule_origin": "managed",
        "discount_bps": 0,
        "payable_multiplier_bp": 10000,
        "track_eligible": false,
        "retention_eligible": false,
        "commission_eligible": false
      },
      {
        "rule_id": "openkeys-openai-1to1",
        "rule_digest": "sha256:openai-rule",
        "scope": {"provider": {"provider_id": "openai"}},
        "pricing_mode": "discount",
        "rule_origin": "managed",
        "discount_bps": 0,
        "payable_multiplier_bp": 10000,
        "track_eligible": false,
        "retention_eligible": false,
        "commission_eligible": false
      }
    ]
  },
  "expected_active": {
    "target": {
      "version": 1,
      "content_digest": "sha256:legacy-policy"
    },
    "binding": {
      "policy_enforcement": "legacy_scalar",
      "funding_enforcement": "legacy_single",
      "reconciliation_state": "pending"
    }
  }
}
```

`policy.rules` must contain only managed provider rules for the providers enabled by the named
catalog/switch lineage. Every rule has `pricing_mode=discount`, `discount_bps=0`,
`payable_multiplier_bp=10000`, and all track/retention/commission flags false. Model rules,
discounted OpenKeys, an empty ruleset, identity changes, retained replacement lock, or a version
jump other than exactly one are `400 invalid`.

Under the account policy lock the engine verifies that `expected_active` is the exact current and
latest replacement-locked legacy OpenKeys policy, validates both old and new policies against the
live account multiplier and immutable dependencies, and requires the successor's exact catalog and
switch targets to be active. One SQLite/PostgreSQL transaction then inserts the immutable successor
and CAS-moves the binding to:

```json
{
  "policy_enforcement": "shadow",
  "funding_enforcement": "legacy_single",
  "reconciliation_state": "verified"
}
```

Success is `result=applied`; an exact retry after a lost ACK is `result=unchanged`. A competing
binding change is `409 policy_cas_mismatch`; missing/inactive exact dependencies and immutable
version conflicts retain the normal typed pricing errors. Any child insert or CAS failure rolls the
whole transaction back. The transaction also consumes the source replacement lock: the historical
locked row is cleared atomically with the binding switch, so later generations advance the
engine-validated canonical managed 1:1 successor through the generic policy prepare/activate CAS
lane. Until that transition, and for any lineage whose active policy is still replacement-locked,
generic policy prepare/activate return `423 locked`. A second transition on the same lineage is
rejected by the successor identity validation; an exact replay of the applied transition remains
`result=unchanged`. The transition changes neither live price nor funding authority: it only makes the
canonical OpenKeys 1:1 successor available in shadow before the all-account Stage 9 release-head
CAS. Consumers are connected after the GREEN producer SHA: strict request/identity schemas live in
`packages/contracts`, the typed transport is
`EngineClient.lockedOpenkeysPolicyTransition`, and the durable Stage 7 shadow-rollout lane
(`packages/db` store, bounded `apps/worker` delivery, AdminGuard staging/read endpoints in
`apps/api`) is its only production caller — see `docs/commerce/MULTI_DISCOUNT_STAGE7.md`.

### Pricing release v2: producer and activation surface

Engine PostgreSQL exposes an additive producer-first surface for the immutable release/funding-v2
authority. Immutable preparation remains traffic-neutral; activation is a separate evidence-gated
operation:

```text
POST /admin/pricing/v2/policy/prepare
GET  /admin/pricing/v2/policy/{policy_id}/version/{policy_version}
GET  /admin/pricing/v2/policy/{policy_id}/latest
POST /admin/pricing/v2/release/prepare
GET  /admin/pricing/v2/release/{generation}
POST /admin/pricing/v2/recovery-link/prepare
GET  /admin/pricing/v2/recovery-link/{target_generation}/{recovery_generation}
POST /admin/pricing/v2/assignment-extension/prepare
GET  /admin/pricing/v2/assignment-extension/{head_version}/{account_id}
POST /admin/pricing/v2/stage8-evidence/capture
POST /admin/pricing/v2/activate
GET  /admin/pricing/v2/head
GET  /admin/pricing/v2/provisioning-context
GET  /admin/pricing/v2/inventory?after_account_id=<id>&limit=500
GET  /admin/pricing/v2/funding/{account_id}/normalization
POST /admin/pricing/v2/funding/{account_id}/normalization
```

Policy/release/link/assignment-extension rows are append-only; policy and release identities are
monotonic by policy version or release generation. `GET .../policy/{policy_id}/latest` returns
`{"policy": <newest complete immutable policy>}` or `404` when that lineage is absent. It is a
read-only reconciliation surface for a consumer whose local evidence can lag a successfully prepared
engine policy; it does not allocate a version or weaken prepare-time monotonicity. Prepare returns the
same typed `stored|unchanged|stale|version_conflict|missing_dependency|invalid` result envelope as
Stage 3C. `GET .../head` returns `{ "head": null }` until a protected consumer submits a fresh passed Stage 8
identity to the activation route. Prepare routes cannot move the global head, mutate an immutable
release manifest or change balances.
An assignment extension can make one post-cutover account resolvable under an already-active head;
the provisioning consumer must therefore complete its exact readback before issuing or enabling a
usable customer key.

`GET /admin/pricing/v2/provisioning-context` is the post-cutover discovery authority for account
provisioning outside the commerce database. It returns `{ "context": null }` before cutover. After
cutover, `context` is materialized in one PostgreSQL `REPEATABLE READ READ ONLY` snapshot:

```text
head = { active_generation, active_digest, head_version, updated_ts }
activation = { activation_id, activation_kind=cutover|recovery,
               evidence_digest, activated_ts }
active_release = {
  generation, release_kind, schema_version,
  capability_generation, capability_digest,
  main_catalog_generation, main_catalog_digest,
  openkeys_catalog_generation, openkeys_catalog_digest,
  switch_generation, switch_digest,
  inventory_digest, funding_manifest_digest,
  minimum_runtime_schema_version, content_digest
}
paired_recovery? = { release=<same projection>, recovery_link=<exact immutable link> }
```

The producer joins the exact head-version activation audit to its persisted passed Stage 8
evidence, verifies target/recovery identities, immutable runtime/funding lineage, base funding
assignment parity and the evidence-selected recovery link. Any disagreement returns authority
unavailable; it never falls back to an arbitrary prepared link. An active target has exactly one
`paired_recovery`; an active recovery has `paired_recovery=null` because no later pair has been
confirmed by that head transition. The projection deliberately omits full base assignments: the
account-specific extension remains the sole post-cutover write contract.

`PricingReleasePolicyV2` has the following exact shape (all unknown fields are rejected):

```text
policy_id, policy_version, owner_type, owner_id, account_class,
product_id?, billing_mode, schema_version=2,
capability_generation, capability_digest,
catalog_generation?, catalog_digest?, switch_generation?, switch_digest?,
content_digest,
rules[] = { rule_id, rule_digest,
            scope = { scope=global }
                  | { scope=provider, provider_id }
                  | { scope=model, provider_id, canonical_model_id },
            discount_bps, payable_multiplier_bp }
```

Each rule's outer `scope` field contains the strict tagged snake-case object shown above; provider
and model identity are inside that object, not sibling rule fields. Engine validation requires
`payable_multiplier_bp = 10000 - discount_bps`, one global rule for B2C, no global rule for B2B,
and one global zero-discount rule for OpenKeys. Service policy is rule-free, has no product/catalog/
switch pins and uses only `billing_mode=meter_only`.

`PricingReleaseV2` has:

```text
generation, release_kind=target|recovery, schema_version=2,
capability_generation, capability_digest,
main_catalog_generation, main_catalog_digest,
openkeys_catalog_generation, openkeys_catalog_digest,
switch_generation, switch_digest,
inventory_digest, policy_manifest_digest, assignment_manifest_digest,
funding_manifest_digest, minimum_runtime_schema_version, content_digest,
assignments[] = { account_id, account_class, policy_id, policy_version, policy_digest,
                  billing_mode, funding_generation?, purpose?, responsible?,
                  assignment_digest }
```

Prepare runs under the release control-plane advisory lock and rejects any release whose unique
assignments do not equal the exact full engine inventory, including both `active` and `disabled`
accounts. Keeping disabled accounts in the immutable graph guarantees that a later enablement
cannot expose an account without its prepared policy/funding identity. Every balance assignment must
reference an existing funding generation; every service assignment is `meter_only`, has no funding
generation and includes non-empty `purpose`/`responsible`. Main/OpenKeys catalogs, switches,
policies and all digests must already exist with matching capability lineage. A recovery link binds
a prepared `target` generation to a strictly newer prepared `recovery` generation.

`PricingReleaseAssignmentExtensionV2` is the post-cutover provisioning shape (all unknown fields
are rejected):

```text
provisioning_head_generation, provisioning_head_digest, provisioning_head_version,
paired_recovery_generation?, paired_recovery_digest?, extension_group_digest,
members[] = {
  release_generation,
  assignment = { account_id, account_class, policy_id, policy_version, policy_digest,
                 billing_mode, funding_generation?, purpose?, responsible?, assignment_digest },
  extension_digest
}
```

Prepare takes the same pricing-release control-plane advisory lock as activation and accepts
only the exact current head. If that active target's activation evidence selected a recovery link,
`members` must contain that exact atomic active/recovery pair; another prepared link or an omitted
pair returns typed `missing_dependency`. An active recovery contains exactly the active member.
Both members must name
the same account, policy, class, billing mode, funding generation and service metadata while keeping
their own release generation, assignment digest and extension digest. The account must already
exist. For an account absent from both immutable base assignment manifests, every policy/funding
dependency must exist. An account that is present in the base manifest is accepted in exactly two
forms, so the immutable base is never rewritten. The first is an exact policy-version override:
the extension references the same policy identity at a strictly newer version with identical
account class, billing mode, funding generation and service metadata. The second is a B2C-to-B2B
class-changing conversion: the extension references a new B2B policy lineage (a different
`policy_id`, so the strictly-newer-version requirement does not apply to it) while the base side
must be exactly `b2c` with `balance` billing, the extension side exactly `b2b` with `balance`
billing, the funding generation non-null and identical to the base's, and purpose/responsible
metadata identical to the base's (null on both sides for balance classes). Every other class
transition — to or from `openkeys`, to or from `service`, and `b2b` back to `b2c` — and any
billing-mode, funding-generation or metadata mismatch stays rejected as a typed
`missing_dependency`.
Balance accounts take the account funding lock and require the assignment
generation to be the exact active funding head; service accounts remain `meter_only` with no
funding generation.

An exact replay returns `unchanged`, a different body for the same
`(provisioning_head_version, account_id)` returns `version_conflict`, and a request for a head that is
no longer current returns typed `stale` without inserting either member. `GET` performs exact
readback by that tuple. Runtime resolution reads one coherent base assignment or append-only
extension for the active release, preferring the extension when both exist; it never mutates the
immutable release manifest. Reserve snapshots pin the resolved assignment identity, and the
release-v2 snapshot invariants accept either the base lineage or the exact extension lineage
(engine migration 0031). This surface does
not create or activate a head.

`POST /admin/pricing/v2/stage8-evidence/capture` is the producer-first machine transport for the
same schema-v2 report as `claude-api db stage8-evidence`. Its body is strict and contains only
explicit capture inputs; caller-supplied runtime evidence is rejected:

```json
{
  "target_generation": 41,
  "recovery_generation": 42,
  "window_start_ts": 1785700000,
  "window_end_ts": 1785700300,
  "min_samples_per_provider": 100,
  "financial_sample_size": 100,
  "gemini_client_admissions": 27
}
```

Target must be positive and recovery strictly newer. The window is a positive non-empty half-open
past interval; provider minimum is `1..=1000000`, financial sample size `1..=1000`, and Gemini
admissions is a nonnegative external aggregate. The server attaches its compile-fixed
`PricingRuntimeManifestEvidence` from `AppState`; the HTTP caller cannot choose runtime capability
lineage. A bounded `AsyncBilling` reader executes the existing PostgreSQL `REPEATABLE READ READ
ONLY` collector. It never enters the billing writer and cannot update a release head, account,
funding authority, balance, reservation, ledger, activation evidence or traffic state. Stage 8
uses the exact release-v2 assignment plus active funding generation/head/lot aggregates as the
cutover authority. A legacy shadow binding may still have `reconciliation_state=pending`; that
state and the retired `funding_buckets` projection are not duplicate activation preconditions.

A successfully captured report is the unwrapped schema-v2 JSON object with HTTP `200` regardless
of its `passed` value. In particular, `passed=false` plus `blockers[]` is valid durable evidence and
must be persisted by the consumer rather than translated into a transport failure.
Malformed bounds are `400`, a shape/type/unknown-field error is `422`, missing control auth is
`401`, and non-PostgreSQL or unavailable authority is `503`. The report contains signed-i64
nanoUSD JSON numbers. TypeScript consumers must read the response as raw text and parse it with
`json-bigint`; `response.json()` is forbidden because it can round those integers before evidence
digest verification.

After the exact producer SHA reached green `deploy/watchdog`, the commerce consumer was connected
through the strict `packages/contracts` schema and the sole
`EngineClient.capturePricingStage8EvidenceV2` transport. The client bounds the response to 16 MiB,
verifies the canonical integer-preserving shape and explicit request identity, and returns both the
parsed report and its exact raw bytes. `apps/worker` may call the producer only for a durable capture
job explicitly staged through the AdminGuard-protected commerce route
`POST /v1/admin/pricing-stage8-capture-v2/stage`; the paired GET is read-only status with bounded
freshness and sanitized blocker metadata, never raw subject identities. The worker
persists the untouched engine bytes before running the combined commerce/OpenKeys/service
collector, then atomically stores the combined bytes and terminal `passed|blocked` result. An
engine `passed=false` report is therefore a successful capture input. Retry/dead transitions are
bounded, stale leases are recovered, and at most one capture job is processing globally. Migration,
startup, polling and activation staging cannot infer or create a capture job; capture completion
cannot create an activation job or move the release head.

Sanitized engine blocker subjects retain their canonical `sha256:v1` domain, while commerce
authority blockers use the canonical `sha256:v2` Stage 5 digest builder. The combined artifact and
paired GET therefore accept both versioned forms for opaque `subject_digests`; all evidence,
release, inventory and request identities remain canonical `sha256:v2` only.

`POST /admin/pricing/v2/activate` is the only global live mutation. All unknown fields are rejected.
The initial cutover request has this exact shape:

```json
{
  "activation_kind": "cutover",
  "expectation": "absent",
  "evidence": {
    "evidence_digest": "sha256:v2:<combined-commerce-evidence>",
    "target_generation": 41,
    "target_digest": "sha256:v2:<engine-target-release>",
    "recovery_generation": 42,
    "recovery_digest": "sha256:v2:<engine-recovery-release>",
    "engine_inventory_digest": "sha256:v2:<engine-inventory>",
    "funding_digest": "sha256:v2:<funding-manifest>",
    "shadow_digest": "sha256:v2:<shadow-window>",
    "runtime_floor_digest": "sha256:v2:<runtime-floor>",
    "legacy_inflight_count": 7,
    "engine_captured_ts": 1785700000,
    "observed_ts": 1785700010,
    "valid_until_ts": 1785700310
  },
  "operator_id": "pricing-control-worker:<instance>",
  "reason": "activate exact prepared Stage 9 target"
}
```

The combined evidence TTL is at most 300 seconds; its source engine capture may be at most 120
seconds older than `observed_ts` (with at most five seconds of source clock skew). The protected
commerce consumer must first verify the canonical source engine digest, exact persisted
`passed=true` combined row, commerce/service/OpenKeys authority, job backlog and sales runtime.
`evidence_digest` is that combined-row audit identity, while target/recovery digests are the engine
release identities, not commerce plan digests.

For forward recovery, `activation_kind` is `recovery` and `expectation` is the complete exact target
head returned by the cutover:

```json
{
  "exact": {
    "active_generation": 41,
    "active_digest": "sha256:v2:<engine-target-release>",
    "head_version": 1,
    "updated_ts": 1785700012
  }
}
```

Cutover is accepted only from an absent head and only to the prepared target. Recovery is accepted
only as a monotonic CAS from that exact target to its linked newer recovery. Engine runs one
`SERIALIZABLE` transaction under `pricing-release-v2:control-plane`, locks the singleton head,
re-reads the immutable pair/link and active catalog/switch heads, and independently recomputes the
base inventory, funding manifest/parity and live runtime-floor digest. Every live instance must
claim release/funding schema v2, the exact compile-fixed runtime digest and its own current owner
epoch. A recovery also proves that any account created after cutover has the exact atomic
target/recovery assignment extension. With the exact target head active, the Stage 8 engine report
keeps the immutable base inventory identity and validates every later account through that paired
extension and its live funding head, so fresh recovery evidence remains obtainable after the
original 300-second cutover proof expires. Only then does the transaction append the evidence row
and activation audit and insert/update one head row. It does not write accounts, balances, funding
lots, reservations, ledger or usage rows, and it does not take data-plane request locks.

Success is `200` with `result=applied|unchanged` and an `activation` receipt containing the durable
activation id/kind, from identity, expected head version, resulting complete head, evidence digest,
operator/reason and timestamp. Exact replay of the same committed request returns `unchanged` from
the durable audit, including after its original evidence TTL elapsed. Rejections roll back the
whole transaction and return `result=rejected` with one typed code:

- `400 invalid`;
- `409 missing_dependency`, `cas_mismatch`, `evidence_stale`, `evidence_conflict`;
- `409 release_lineage_drift`, `authority_drift`, `inventory_drift`, `funding_drift`,
  `funding_invariant_drift`, `runtime_floor_drift` or `runtime_incompatible`.

After the producer SHA reached green `deploy/watchdog`, `packages/contracts` added the strict
request/receipt/rejection schemas, `packages/engine-client` added the sole typed transport, and
`packages/db/src/pricing-release-activation-jobs.ts` plus `apps/worker` added a durable consumer.
The worker can call this route only after an explicit immutable activation job exists. No API,
startup hook, migration or Stage 8 collection automatically stages that job; a deployed consumer
with an empty queue cannot create a head.

Inventory is ordered by `account_id`, returns at most 500 rows plus `next_after_account_id`, and
contains status, legacy scalar, integer balance/reserved/spent and nullable funding-v2 head identity.
It contains no key secret. Consumers must exhaust the cursor and join this engine inventory with the
authoritative commerce/OpenKeys inventories; a partial page is never release evidence.

Funding normalization is an account-local content-addressed producer and cannot activate pricing.
`GET .../normalization` returns:

```text
normalization = {
  account_id, account_status,
  status = ready|blocked|normalized,
  source = aggregate_paid_only|ledger_replay|legacy_buckets|stored_generation,
  source_state_digest = sha256:v2:...,
  normalization_digest?, funding_generation?, funding_head_version?,
  balance_nano, reserved_nano, spent_nano,
  lots[] = {lot_id, source_type=paid|welcome_bonus, source_ref,
            balance_nano, reserved_nano, spent_nano, version, status},
  blockers[] = {code, detail}
}
```

`POST` accepts a strict body
`{expected_source_state_digest, expected_normalization_digest}`. A successful response has
`result.status=stored|unchanged`; `stale|blocked|conflict` return HTTP 409 together with a freshly
rebuilt `result.normalization`; an unknown account returns 404; a malformed digest/body returns 400/422.
Apply takes the same account funding lock as reserve/settlement/top-up and atomically writes the generation,
lots and initial head. A legacy in-flight request blocks only its own account; a writer that waited for the lock
re-reads the new head and dual-writes into funding v2. There is no global drain.
An unrevoked `signup-bonus:<subject>` remains `welcome_bonus`, but an exact full negative
`bonus-revoke:<subject>` for the same subject converts the entire current aggregate to `paid`: the entitlement
is revoked and historical pre-revoke gaps do not restore it. Partial, mismatched, duplicate
or mixed active/revoked evidence returns `invalid_ledger_evidence`, not a guessed plan.

Assignment-extension typed TS consumers are connected only after the green exact producer SHA.
`packages/contracts` strictly validates the nullable provisioning context and the exact active/recovery pair;
`packages/engine-client` is the sole typed transport and the owner of the canonical Stage 5 v2
policy/assignment digest builder. With a non-null context, the commerce, OpenKeys and service writers
complete the required account-local chain, the exact policy/extension prepare+GET readback and a fresh
final context check before issuing a usable key or declaring a service account available.
OpenKeys first credits the face value, then normalizes funding and persists the global 1:1 policy;
the service policy is rule-free, `meter_only`, without funding/catalog/switch pins and with mandatory
purpose/responsible. An active target gets only the evidence-selected recovery member; an active
recovery gets one member. `apps/api` additionally repeats the commerce key check after the remote issue;
if the head or authority changed, the key is disabled before the raw secret is returned. With `context=null`
the consumer keeps the pre-cutover path and materializes nothing release-v2, so a deploy by itself does not
start the cutover.
Stage 8 evidence already supports zero-drain audit counts, and the activation producer performs one
CAS. The strict contracts/client/durable worker consumer is connected producer-first: the request is persisted before
the network, the complete ACK is persisted before `confirmed`, and a timeout retries only the exact body. The consumer does not
create the job itself; until a separate Stage 8 source-capture/control-plane checkpoint, staging fails closed
on nullable source fields. Data-plane reserve/settlement do not take the release control-plane lock.
After each producer SHA reached a green exact-SHA `deploy/watchdog`, `packages/contracts` gained the
strict release, funding-normalization, assignment-extension and activation wire schemas, while
`packages/engine-client` gained typed prepare/read, account-local normalization/extension and the
single activation method. The bounded application jobs are separate `apps/worker` consumers:
it runs only for an explicitly staged target-release job, re-GETs exact plan digests before every
POST, excludes service `meter_only` accounts and confirms only complete funding-manifest coverage.
The activation lane likewise runs only for an explicitly staged immutable request and persists its
full receipt. Merely having a typed client or a deployed worker does not materialize an account or
move the release head.

### Error codes
`400` invalid body (explicit handler validation) · `401` missing/incorrect control key · `404`
account/key/version not found · `409` CAS/version conflict or a limit below what has already been
charged+reserved · `422` JSON is syntactically valid but does not match the body schema
(unknown field under `deny_unknown_fields`, wrong type) · `423` immutable
pricing policy locked · `503` billing is disabled or the billing authority is unavailable.
On the client `/v1`: `402` balance ≤ 0.

### Example: full cycle (bash)
```bash
CTL=<CONTROL_KEY>; B=http://127.0.0.1:8790
AID=$(curl -s -XPOST $B/admin/account -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d '{"handle":"acme","mult_bp":2000}' | jq -r .account)
curl -s -XPOST $B/admin/account/$AID/credit -H "x-api-key: $CTL" -H 'content-type: application/json' \
     -d '{"amount_nano":"25000000000","ref":"stripe:pi_123"}'        # credit $25 (idempotent)
KEY=$(curl -s -XPOST $B/admin/key -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d "{\"account_id\":\"$AID\",\"label\":\"prod\"}" | jq -r .key)   # issue to the client
curl -s $B/admin/account/$AID -H "x-api-key: $CTL"               # balance for the dashboard
curl -s $B/admin/account/$AID/ledger -H "x-api-key: $CTL"        # history
```

---

## 6. What's NOT there yet (doesn't block the start)

- **Push stream of usage** engine→your service is absent. The commerce worker polls the cursor-based
  `GET /admin/account/{id}/ledger?after_id=...` and acknowledges the processed cursor via
  `POST /admin/account/{id}/ledger/ack`; delivery is idempotent.
- **Cross-host TLS/private networking** remains part of Phase 3. The current HTTP Control origin is reachable
  only via loopback on the same host and is not published by Caddy to the outside.
- Rotating `CONTROL_KEY` requires updating the engine, API and worker env consistently, then performing
  the regular engine/API blue-green and worker stop/start per `docs/ops/DEPLOYMENT.md`; a lone manual restart
  will create a window with mismatched keys.

Questions about the contract — they are all covered by this engine; if something is missing for the website,
we'll build it on our side.
