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

Provider quota and saleable money have separate authorities, and only money is gated on delivery.
`windows[].used_fraction_units`, `resets_at`, `snapshot_fresh` and the top-level `util5h`/`util7d`,
`reset5h_in`/`reset7d_in` are published from the exact provider snapshot regardless of FIFO state,
so a dollar-evidence failure never hides the real utilization wall. Under pending/degraded delivery
every money field — `remaining_nano`, `remaining_low_nano`/`remaining_high_nano`,
`last_known_remaining_nano`, `rem5h_nano`/`rem7d_nano`, fleet remaining and horizons — stays `null`
with `missing_reason` naming the delivery cause. This matches the Gemini/KIMI/GLM contract.

The last exact provider snapshot remains available only as a diagnostic display-state until
its future reset: `windows[].used_fraction_units`, `resets_at`, `last_known_quota_source` and
`last_known_remaining_nano` keep their previous value, while the top-level `reset5h_in`/`reset7d_in`
keep counting down. This covers both an idle routable subscription and quota-cooling,
when a new probe is deliberately deferred until reset. A new snapshot replaces the display-state immediately;
after the provider deadline the old fraction/reset/last-known remaining are never carried into the
new window. `windows[].snapshot_fresh` stays `false` all this time, and the canonical
`remaining_nano`, fleet remaining and horizon stay `null`: `last_known_remaining_nano` must not be
treated as currently sellable capacity.

Once that provider deadline passes, the window is empty by construction: the provider refilled it at
that instant. A subscription that is `auth_state="healthy"`, routable and not cooling therefore
publishes an exact `used_fraction_units: 0` for the new window with `resets_at: null` — not a `null`
fraction, which would be indistinguishable from never having measured. A dead/suspect token or a
cooling window keeps the previous silence, because their reset is no evidence that quota was
refilled for us. A rolled-over window is never priced: it has no fresh measurement, so
`remaining_nano` and fleet remaining stay `null` and `snapshot_fresh` stays `false`.

The additive `windows[].quota_state` names why an exact current fraction is absent, independently of
the money-side `missing_reason`: `awaiting_probe` (no usable provider snapshot yet),
`last_known_before_reset` (stale snapshot retained until its provider deadline), or
`window_rolled_over` (deadline passed, window empty by construction). It is absent when the snapshot
is fresh. The additive `windows[].displayed_quota_source` names the origin of whatever fraction is
being shown (`runtime_quota_snapshot`, `durable_calibration_snapshot` or `provider_window_rollover`).
Consumers must ignore unknown values of both fields.
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

An account has one balance plus explicit accounting accumulators; the engine maintains the
per-account invariant:
```
balance + reserved + spent - uncollected = topups + adjustments
```
- **balance_nano** — customer balance available to reserve; it may be negative within the shared
  account floor, or lower after an explicit debit/clawback records debt.
- **reserved_nano** — temporarily held for "in-flight" requests (the engine reserves a ceiling before
  the request and returns the difference after). You don't touch this — just know that "in the moment" the balance
  may be slightly lower by the amount of in-flight requests.
- **spent_nano** — total full billed usage (monotonically increasing).
- **uncollected_nano** — the part of full billed usage the account-wide settlement floor prevented
  the customer balance from collecting. It is explicit pool-loss evidence, not customer payment.
- **mult_bp** — the account's payable default multiplier in basis points, bounded to `0..10000`:
  `2000 = ×0.20`. A row in `account_provider_discounts` overrides it for one canonical provider;
  otherwise this scalar prices the request. It is live state, not a migration fallback.

Admission and settlement lock the same account row and atomically enforce the shared −$1 floor.
Settlement records full delivered usage and any amount it could not collect separately, rather
than multiplying the floor by the number of concurrent requests or silently discarding cost. An
explicit debit/clawback can take an already-spent account lower as recorded debt; a non-positive
account is blocked from new paid requests. Service accounts are the explicit zero-charge exception.

---

## 4. Canonical scenarios

### A. Client registration
1. A user registers on your website → you create a record in YOUR OWN DB.
2. `POST /admin/account` → the engine returns `account_id` (`acct_…`). Store it next to the user.
   A retry with the same non-empty `handle` returns the same account, so registration recovery
   is idempotent and does not create orphaned accounts.
3. `POST /admin/key` with this `account_id` → the engine returns **`sk-pool-…`**. Show the key to
   the user **once** (it is their API key, a secret). An account can have many keys.

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
POST /admin/account                     {"handle"?, "mult_bp":0..10000?} → 200 {account, mult_bp, handle}
POST /admin/accounts/query              {"account_ids":["acct_…"]}   → 200 {accounts:[{account,
                                                                            balance_nano,spent_nano,
                                                                            reserved_nano,balance,mult_bp,
                                                                            status,handle}]} (1..500 id;
                                                                            400 on an empty/invalid list)
GET  /admin/account/{id}                                             → 200 {account, balance_nano, spent_nano,
                                                                            reserved_nano, balance, mult_bp, status, handle} | 404
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
GET  /admin/account/{id}/discounts                                    → 200 {account,mult_bp,
                                                                            providers:{<provider_id>:mult_bp}} | 404
POST /admin/account/{id}/discounts      {"provider_id":"anthropic|openai|google|kimi|glm",
                                         "mult_bp":0..10000|null}     → 200 {account,provider_id,mult_bp,changed}
                                                                        | 400 | 404
                                        (the whole pricing policy: an account default plus one
                                         override per provider whose terms differ. `mult_bp:null`
                                         removes the override. A write is live on the next
                                         request — there is no version to activate and no
                                         snapshot that can disagree with the balance. Model —
                                         docs/commerce/PRICING_MODEL.md)
GET  /admin/account/{id}/keys                                        → 200 {keys:[{key_id,key_masked,label,status,
                                                                            spent_nano,spent,reserved_nano,
                                                                            spend_limit_nano,expires_ts,
                                                                            created_ts,last_used_ts}]}
GET  /admin/account/{id}/ledger?limit=N[&after_id=ID]                 → 200 {entries:[{id,kind,request_id,
                                                                            amount_nano,ref,ts,provider,
                                                                            official_nano,uncollected_nano,...}]}
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
returned oldest-first with `id > after_id`; this is the durable worker cursor for usage accounting
and referral commission. It is not a pricing authority.

A new ledger row persists the expand-compatible top-level `request_id`, `provider`, `official_nano`
and non-negative `uncollected_nano`. For a charge, `amount_nano` remains the full billed actual and
`uncollected_nano` is the part the account-wide settlement floor could not collect; the collected
customer debit is therefore `amount_nano - uncollected_nano`. Old producer rows and mixed-version
responses omit the additive field or return zero, and consumers must treat omission as zero. A
shortfall is pool loss evidence, not paid/free customer funding and never a partner-commission basis.
For a pre-column charge with `ledger.provider IS NULL`, the reader first looks up immutable `usage_events` by the
exact `account_id + request_id`. For an older pair where both `request_id IS NULL`, only
the full settlement fingerprint is allowed: the same account, null-safe key/ref/model, exact
`charge_nano = amount_nano` and a timestamp difference of no more than one second. The provider is returned
only if all candidates contain the same non-empty value; ambiguity stays `null`,
and a disagreement between the recovered value and the persisted ledger provider closes the read with an error.
The model participates only in the full fingerprint and is never converted into a provider.
The Control API does not expose the retired policy attribution or funding-allocation payloads.
All `*_nano` and ledger IDs are integer JSON values; `packages/contracts` normalizes them into
decimal strings before they reach JavaScript business logic.

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
                                         "spend_limit_nano"?, "expires_ts"?}
                                                                     → 200 {key:"sk-pool-…", key_id:"key_…", account,
                                                                            label,spend_limit_nano,expires_ts}
                                                                       | 400 | 409  (key is visible 1 time!)
POST /admin/key-id/{key_id}/status      {"status":"active"|"disabled"}
                                                                     → 200 {key_id,status,updated} | 400 | 404 | 409 (recommended)
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

`spend_limit_nano` is an optional positive decimal string and caps lifetime charged platform spend
for that key. `expires_ts` is an optional future Unix timestamp in seconds. The engine enforces both
again inside the atomic reservation transaction, including in-flight holds, so concurrent requests
cannot cross a key's cap. `NULL` means unlimited/no expiration and preserves legacy behavior.
The policy endpoint is an account-scoped full replacement: both nullable fields are required.
It can increase or clear a limit and extend or clear expiry without changing key status. A new
limit below `spent_nano + reserved_nano` is rejected atomically with `409` and code
`limit_below_committed`, so an edit cannot invalidate an in-flight reservation.

### Scalar and per-provider pricing

The account default and provider overrides documented under **Accounts** are the complete live
customer-pricing control surface. Writes are bounded to `0..10000`, apply to the next authorization
read and require no prepare/activate sequence. Hot tariff routes below control official upstream
costs; they do not replace an account's payable multiplier.

The former catalog/switch/policy and `/admin/pricing/v2/*` routes are absent from the server,
`packages/engine-client` and `packages/contracts`. Their tables are retained only as immutable
incident evidence until every gate in `docs/ops/PRICING_RETIREMENT.md` passes. They must not be
treated as an expand-only callable contract or restored for rollback; deployment compatibility is
enforced by the closed scalar marker described in `deploy/RELEASES.md`.

### Hot tariff overrides (`/admin/pricing/tariffs*`)

The append-only `pricing_tariff_overrides` table (migrations 0036/0037) republishes one compiled
`metering` tariff family as data, so a price correction does not require a recompile and redeploy.
Compiled constants are the implicit **version 1** of every family; each row is version >= 2 in a
strict per-family sequence enforced by database triggers, carries an `effective_from` priced-ts
bound, a canonical `sha256:v2` payload digest and operator attribution, and is never updated or
deleted — a correction is a newer version. The runtime now consumes the table: a process-wide
tariff book in `crates/forward` (contract — `crates/forward/CLAUDE.md`, "Hot tariff overrides")
re-reads it through the billing reader actor every few seconds; reserve resolves the override
effective at the priced timestamp and pins `<family>/v<version>`, and settlement replays exactly
that pinned version. i128 money legs are canonical
decimal **strings** in the payload JSON (JSON numbers are rejected); u64/i64 fields stay plain
integers. Like the other pricing routes, no separate audit log is written: the table itself records
`created_by`/`reason`/`created_ts` for every version.

All four routes sit under the same control-key gate as the rest of `/admin/*`. Writes go through
the billing single-writer actor, reads through the bounded reader pool; the authority is
PostgreSQL-only, so an engine on the SQLite fallback answers `503 billing authority unavailable`.

- `GET /admin/pricing/tariffs` → `{"overrides": [...]}` — every row ordered by
  `(tariff_family, version)`; each stored digest is recomputed on read and a mismatch fails closed.
- `GET /admin/pricing/tariffs/compiled` → `{"compiled_ts": <unix>, "families": [{"tariff_family",
  "payload"}, ...]}` — the compiled catalog dump in the exact canonical payload shape the table
  stores, sorted by family, so an auditor can diff DB rows against the code. Read-only and
  authority-free: the answer is built from `metering` alone.
- `POST /admin/pricing/tariffs/override` — publish the next version of one family. Body:
  `{tariff_family, effective_from, payload, created_by, reason}` — **no version field**: the server
  computes `head + 1` (2 when the family has no rows) and retries exactly once on a sequence race
  with the authority-returned `expected_next`. `effective_from` must be `>= now - 60s` (the
  determinism rule; validated early for a clean 400), `created_by`/`reason` non-empty, and the
  payload must parse against the family's schema. Outcomes: `inserted` → 200 with the row;
  `unchanged` (exact replay) → 200; `invalid` → 400; `conflict` (same family+version, different
  content) → 409; `sequence_violation` still failing after the single retry → 409.
- `POST /admin/pricing/tariffs/seed` — bridge compiled constants into the table. Body:
  `{created_by, reason, tariff_family?}`. For the given family — or for **every** compiled family
  when the field is absent — the server builds the payload from the compiled `metering` constants
  (never from operator-typed numbers) and inserts it as **version 2 with `effective_from = 0`**.
  Exact replay returns `unchanged`, so re-seeding is idempotent. A family whose head is already
  past version 2 is **refused** (per-family `refused` outcome, overall HTTP 409): seeding is only
  the bridge from compiled to data, never an overwrite of operator versions. A family name unknown
  to the compiled catalog is a 400. The response is `{"outcomes": [{"tariff_family", "result":
  "inserted"|"unchanged"|"refused"|"rejected", ...}]}`; 200 when every target seeded cleanly,
  409 when at least one was refused/rejected (the remaining families still seed).

The time-bounded `anthropic/standard/sonnet-5-intro` family is published by the compiled catalog
(and therefore seeded) only while the compiled epoch has not flipped (`now < 2026-09-01T00:00:00Z`);
after the flip the intro family is dead — the matcher never emits it again — and the scheduled
epoch change is published as a normal new override version of `anthropic/standard/sonnet-current`.

### Error codes
`400` invalid body (explicit handler validation) · `401` missing/incorrect control key · `404`
account/key/tariff not found · `409` idempotency/sequence conflict or a limit below what has already
been charged+reserved · `422` JSON is syntactically valid but does not match the body schema
(unknown field under `deny_unknown_fields`, wrong type) · `503` billing is disabled or the billing
authority is unavailable.
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
