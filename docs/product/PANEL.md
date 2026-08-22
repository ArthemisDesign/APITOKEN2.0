# PANEL.md — unified admin panel admin.apitoken.sale

`admin.apitoken.sale` is the single control center for the owner: commerce users and
money, engine accounts and capacity, affiliate accounts, the CRM service account,
security, B2B and audit. The legacy `panel.apitoken.sale` was removed after all of its
functions were moved here.

## Architecture

```
browser ──host-only session──▶ Caddy forward_auth ──▶ commerce admin identity store
                          │
                          ▼
                       Caddy (admin.apitoken.sale)
                          ├─ / (entire UI)            → Next.js `apps/admin` :3700
                           ├─ /overview /capacity
                           │  /metrics /subs
                           │  /fleet-history
                           │  /settlement-health       → engine balancer :8790 (+ control key)
                           ├─ /codex-subs              → OpenAI origin :8792 (+ control key)
                           ├─ /gemini-subs             → Gemini origin :8794 (+ control key)
                          ├─ /admin/*                 → commerce balancer :8791 /v1/admin/*
                          │                              (+ commerce admin key + actor)
                          ├─ /openkeys-admin/*        → OpenKeys :3410 /api/internal/admin/*
                          │                              (+ server-side control key + actor)
                          └─ /partner-admin/*         → sales-api :3100 /v1/admin/*
                                                         (+ sales admin key)
```

- The UI is a standalone Next.js app `apps/admin` on `127.0.0.1:3700` with its own deploy
  lane; the engine no longer serves the panel's HTML/JS, only the data routes from the
  diagram above. Security headers are sent by the app itself via `next.config.ts`; Caddy
  adds only the defence-in-depth Permissions Policy and the indexing ban.
- Caddy `forward_auth` is the single human gate. Commerce PostgreSQL stores only password
  hashes, status and grants for the five managed domains. Control, commerce-admin and
  sales-admin keys are injected only server-side and never end up in HTML, browser
  storage, responses or logs.
- Caddy uses the commerce auth producer's `session-v1` contract. Only a trusted Caddy request
  carrying `X-Admin-Auth-Mode: session-v1` enables it; direct legacy calls without that header
  retain the Basic contract for rollback compatibility. The browser contract provides
  same-origin login/logout endpoints and a 180-day host-only
  `HttpOnly; Secure; SameSite=Lax` signed cookie. Every request still rechecks the active
  account and exact domain grant, while password, domain or status changes revoke the
  cookie. Login and logout POSTs require the exact managed origin; for browsers that omit
  `Origin` on a same-origin HTML form, the login surface publishes `Referrer-Policy: same-origin`
  and accepts only a parsed `Referer` with that exact HTTPS origin. A present foreign or `null`
  `Origin`, a foreign/malformed `Referer`, and requests with neither signal remain forbidden.
  Document requests redirect locally to login; API requests receive a challenge-free
  `401` with `X-Admin-Login`, so mobile Safari never opens its native Basic prompt.
- The verified identity is passed to commerce as `x-admin-actor` and `x-admin-account-id`
  via `forward_auth copy_headers`. The global directive order places an anti-spoof
  `request_header` clear before authentication, so Caddy first removes client forgeries,
  then sets the verified identity, and auth stays ahead of the terminal `handle` routes.
  A successful Basic-to-session upgrade bridges its cookie onto the browser response and
  removes the temporary internal header before any application upstream runs.
  The downstream proxy preserves these headers without override, so that audit can
  distinguish operators and self-service password rotation. The internal auth API is
  closed on the public `backend.apitoken.sale` and is reachable by Caddy only via
  loopback.
- External vhosts and applications see only the stable origins `127.0.0.1:8790`
  (Anthropic/control), `127.0.0.1:8791` (commerce) and `127.0.0.1:8792` (OpenAI). Only the
  first two Caddy balancers know the blue-green slot ports; an ordinary application `503`
  does not exclude a live slot — depooling is performed by active `/ready` checks. The
  single-release OpenAI bridge on 8792 is described in `deploy/CADDY.md` and does not
  change admin routing.
- Engine data (`/overview`, `/capacity`, `/subs`, `/metrics`) is defined in
  `crates/server/src/http.rs`. `/overview` without query params still contains the full list of
  engine accounts without API keys. Optional `accounts_limit` / `accounts_offset` page that list
  (limit `0` omits `accounts` and still returns expand-only `accounts_total`, `accounts_active`,
  and `crm`). `/capacity`, `/gemini-subs` and `/kimi-subs` accept `recent_turns=0` to keep
  `calibration_recent_turns` as `[]` while the omitted/default request still returns up to 512
  events. `/fleet-history` serves metrics.db history (minute fleet snapshots for
  90 days) in 24h/7d/30d/90d windows bucketed down to ≤ ~500 points, optionally a per-sub
  series matched by email mask. `/spend-stats`, besides accounts/providers, returns
  `models[]` — the top-20 models by charge for each window (the served model id from
  usage_events, the one actually metered). The optional query params `from`/`to`
  (epoch seconds, mandatory together) add a `custom` block to the response in the same
  shape as a `periods` window, plus an echo of the bounds: the same usage_events
  aggregation over the half-open range [from, to) — adjoining ranges do not double-count
  events. Width ≤ 92 days, a `to` in the future is clamped to now+1, a range entirely in
  the future/from ≥ to/garbage — 400 with a reason text. `custom` is computed per request
  and does not enter the TTL cache (only standard windows are cached), so responses with
  different bounds never mix.
  `/settlement-health` is money diagnostics of the settlement pipeline: settlement_outbox
  counts by state (pending/processing/done/failed), failed overall and over 24h, backlog
  of unsettled rows older than 5 minutes, the last ≤10 failed with last_error (truncated
  to 200 characters, contains no secrets) and the lag of the ledger's pricing consumer
  (max(ledger.id) versus ledger_consumer_checkpoints plus the age of the oldest
  unacknowledged row). A growing backlog/failed/unacked is the signal of "silently stuck"
  money that was previously visible only in stderr.
  `/codex-subs` (per-home status of the GPT/Codex fleet) is served only by the OpenAI
  runtime; a row contains an opaque home id and an email mask made of the first four
  characters without the domain, but not the full ChatGPT email, account id, OAuth or
  proxy. Codex is not configured on the Anthropic process and the endpoint would return
  `enabled:false` there, so Caddy sends this path to the stable OpenAI origin rather than
  to the engine balancer. `/gemini-subs` is likewise read only from the stable Gemini
  origin `127.0.0.1:8794`; the response contains opaque profile/model quota/cooling,
  cache-affinity counters and separate attested gaxios/Undici CLI/Node/JA3/JA4, but not
  Google identity/project/proxy/OAuth.
- Commerce data lives in `apps/api` behind `AdminGuard`; the authoritative live balance
  still lives only in the engine.
- Partner data is read through the sales admin API. The main admin gets only a server-side
  proxy; the separate full affiliate admin remains the same sales-web `/admin` at
  `admin.partners.apitoken.sale`.
- OpenKeys remains a separate bounded context with its own PostgreSQL. The unified panel
  reads its masked catalog via `/openkeys-admin/*`: Caddy passes the route only after
  managed-admin auth and injects the verified actor and the server-side credential. The
  public `openkeys.apitoken.sale/api/internal/*` always returns `404`; full `sk-pool`
  keys and warehouse ciphertext do not enter the internal contract.
- The CRM code lives in a separate repository. The main admin shows its engine service
  account with the handle `crm-parsing` and a link to `crm.apitoken.sale`.
- Independent read sources degrade separately: an error in one API does not replace the
  whole page. The panel shows a dismissible notification, checks the source every 5
  seconds and reloads the page after full recovery. Switching the tab cancels stale
  requests.
- Auto-refresh is enabled only for live pages: overview — 30 seconds, system and
  subscriptions — 10 seconds; in a background tab polling is paused. Users and partner
  accounts load in pages, and live engine balances for the commerce page are read in a
  single batch request.
- Caddy compresses responses with `zstd`/`gzip`; CSP and the other security headers are
  set by the Next.js app itself via `next.config.ts`, and Caddy adds only Permissions
  Policy and the indexing ban.

## Capabilities

- Overview of all planes: commerce, engine, partners and CRM. The partner card also shows
  the plane's money: commissions, the amount payable (requested + approved) and paid out.
- Accounts: all engine/service accounts, all partner accounts (with full source-side
  pagination), the commerce total and a jump to the full user workflow.
- Partners: a read-only view of the sales plane — overview (partners/referrals/turnover,
  commissions, payable, paid out), the payout engine window state, an auto-generated
  "payable for the period" list with eligibility reasons (below minimum, no wallet,
  inactive, window closed) and the eligible total, partner analytics with server-side
  sorting (12 fields) and pagination, payout history and a collapsible list of on-chain
  batches. Sources — `/partner-admin/*`: `overview`, `payouts/engine`, `payout-list`,
  `partner-analytics`, `payouts`, `payouts/batches`; each degrades as a separate warn
  block. Amounts are sales-api nanoUSD strings. No auto-refresh.
- Administrators: creating identities with one or several domain grants, an exact filter
  by domain, password rotation of any account (including the current one), enable/disable
  and protection of the last active main admin from lockout.
- Users: server-side search/filters and bounded pagination, server-side sorting (`sort` ∈
  created_at/last_seen_at/paid_total/topup_total/spent_30d, `dir` ∈ asc/desc; the engine's
  live balance/spent fields are not sortable), live balance/spend, payments, keys,
  effective global/provider/model discount, 2FA, balance crediting, revoke of all
  sessions, 2FA reset, enable/disable. The "balance/spend of visible rows" cards sum only
  the displayed page and next to them hint at the platform totals from `/overview`
  (demand.balance_usd/spent_usd). The «CSV» ("CSV") button exports the currently loaded
  page (`users-YYYY-MM-DD.csv`).
- OpenKeys: a separate list of issued keys with mandatory display of label/batch/seller,
  server-side filters by batch, status and usage (`unused`, `used`, `exhausted`, no-live),
  bounded pagination and reversible enable/disable. Live balances are read via engine
  batch requests, not N+1 per key.
- Money: confirmed payments, engine credit state, unfinished checkouts; the server-side
  filters of `GET /admin/topups` — `q` (email substring), `provider`, `status` (one filter
  for both lists), `limit`/`offset` pagination with `payments_total`/`checkouts_total`
  (one «Назад/Дальше» — "Back/Next" — paginates both lists). The «CSV» button exports the
  current page of both lists as a single file (`topups-YYYY-MM-DD.csv`) with a
  kind=payment|checkout column.
- Finance: prepay metrics on a single screen — 30-day revenue with a delta against the
  previous 30, ARPU/ARPPU, paying share and the distribution of customers by effective
  discount/source rule; an SVG daily-revenue chart (7/30/90 windows) with a per-provider
  breakdown; the checkout funnel (created → paid/cancelled/error/expired) with conversion,
  average time to payment and average check; top customers by top-ups and by spend with
  their share of the window total; refunds and disputes with pagination; weekly
  registration cohorts and churn signals of paying customers. Sources — read-only commerce
  endpoints behind AdminGuard: `GET
  /admin/finance/{overview,revenue,funnel,top-customers,cohorts,churn-signals}` and `GET
  /admin/refunds`. Amounts are integer nanoUSD strings, aggregation on the PostgreSQL
  side. The authority for a refund's status is `payments.status`; engine_adjustments
  (engine debit on refund) is not yet fully populated. No auto-refresh. At the bottom of
  the tab — the health of the money pipelines: verdict, cards and recent failures of `GET
  /admin/pipeline-health` (engine credits, webhooks, mail, pricing jobs) plus engine
  settlement from `GET /settlement-health` (outbox pending/backlog/failed, pricing
  consumer lag — the delay in transferring spend to commerce); when verdict≠ok or
  settlement failed/backlog the overview shows a warn/bad banner with a link to this tab.
  The «Кто тратит» ("Who is spending") modal (/spend-stats) also shows the "by models"
  table — the top-20 served models of the active window with charge, real-API equivalent
  and discount. Next to the d1/d7/d30 tabs — an arbitrary "from/to" date range ("to"
  inclusive, a half-open +1-day bound is sent to the server): a `custom` block is rendered
  with the same accounts/providers/models subtables, and a validation 400 is shown as text
  in the modal.
- Pipeline health: `GET /admin/pipeline-health` behind AdminGuard — a read-only summary of
  money pipeline failures (engine_credits/webhook_events/email_outbox/
  engine_pricing_jobs: counts by status, dead, retry backlog, recent failures without
  payload, the nano sum of stuck credits) with an overall ok/warn/bad verdict; amounts are
  integer nanoUSD strings.
- B2B: one-time invite links with an individual discount. Email is optional: with an
  email the link is bound to the address and the letter is atomically queued into the
  durable outbox; without an email the panel creates a shareable link and copies it
  immediately. An active invite can be copied again, revoked or replaced with a new link
  and resent. The list shows delivery status/error, and B2B clients show
  pending/retry/failed/confirmed price synchronization with the engine.
- Subscriptions: a separate page for the three fleets. Claude — lifecycle (added/peaks/
  days until replacement), live util/reset/cooling by 5h/7d windows and proxy; GPT (OpenAI
  Codex) — per-home status, decimal primary/secondary utilisation, email mask as the
  primary operator label with the opaque home id below, exact nanoUSD official-price
  spend, realized workload-blend capacity/remaining, envelope/evidence/confidence;
  Gemini — per-profile auth/inflight, per-model availability/cooling, official quota
  remaining/reset/type, probe freshness, missing-usage settlement counter and exact
  gaxios/Undici transport attestations.
- System: verdict, 1d/5h/7d supply, headroom, coverage, fleet demand, recommendations and
  all engine accounts; the detailed per-sub view is moved to «Подписки» ("Subscriptions").
- Trends: fleet history from metrics.db (24h/7d/30d/90d windows) — SVG charts of available
  capacity, utilisation, subscription deficit (gap/subs_needed) and customer balances with
  potential demand; the per-sub cap/util series matched by email mask shows the
  degradation of a subscription's capacity. No auto-refresh — only manual refresh and
  window switching.
- Audit: operator/user/provider events and the reasons for administrative actions; the
  server-side filters of `GET /admin/audit` — `action`, `actor_type`, `q` (substring over
  target_id and metadata::text), `from`/`to` (ISO 8601), `limit`/`offset` pagination with
  `total`; `GET /admin/audit/actions` — the distinct list of actions for the dropdown (the
  panel fetches it lazily once). The «CSV» button exports the current page
  (`audit-YYYY-MM-DD.csv`).

The panel's CSV exports («Пользователи» — "Users", «Пополнения» — "Top-ups", «Аудит» —
"Audit") are always the currently loaded page, without auto-loading the rest: `;`
separator, quoting/escaping of separators/newlines per RFC 4180, a BOM at the start of the
file for correct UTF-8 in Excel, money as raw USD numbers, dates as ISO 8601.

Manual crediting accepts whole USD, a UUID idempotency key and a mandatory reason. A
positive credit is idempotent; a gift does not count as a paid top-up. Disabling a user
first blocks the authoritative engine account, then the commerce mapping and sessions.

Creating a B2B invite also accepts a UUID idempotency key and a reason. Only a whole
discount of 0–95% and a term of 1–30 days are allowed. A repeat with the same key returns
the original link rather than creating a second one. Converting an existing B2C client to
B2B accepts the negotiated discount in the same atomic action; actor, reason, old and new
rate are recorded in the audit.

## Domains

- `admin.apitoken.sale` — the single main admin console.
- `admin.partners.apitoken.sale` — the former partner admin content, only the hostname
  changed.
- `partners.apitoken.sale` — the public affiliate site, unchanged.
- `crm.apitoken.sale` — the former CRM content, only the hostname changed; access is
  granted via a separate domain grant and does not appear on a main-admin identity
  automatically.
- `panel.apitoken.sale`, `partners.panel.apitoken.sale`, `crm.panel.apitoken.sale` —
  removed without redirect and must be treated as a production verification error if they
  start being served again.

## How to add a data source

1. Engine: a new endpoint behind `control_authed`/`readonly_authed`, then allow the path
   in the `@admin_data` Caddy matcher.
2. Commerce: an endpoint behind `AdminGuard`; `/admin/*` is already proxied to
   `/v1/admin/*`.
3. Sales: an endpoint behind `AdminKeyGuard`; the main admin uses `/partner-admin/*`.
4. OpenKeys: the internal endpoint verifies the credential injected by Caddy after the
   human gate; the public OpenKeys vhost must block `/api/internal/*`.
5. UI: a page/widget in `apps/admin`. A partial source must show a degraded state rather
   than ask the operator for a secret.
6. Deploy: an ordinary push to master. The watchdog applies Caddy, checks the engine data
   routes (`/overview` answers 401 with its own auth gate), the 401 human-auth gate on the
   four active managed hosts and the absence of the three retired hosts.

## Secrets

| Secret | Where it lives | Who verifies it |
|---|---|---|
| admin password hashes + domain grants | commerce PostgreSQL | `apps/api` internal auth |
| engine control key | live Caddy + engine env | `control_authed` |
| OpenKeys internal credential (the same engine control key) | live Caddy + `openkeys.env` | OpenKeys internal route |
| `COMMERCIAL_ADMIN_KEY` | live Caddy + commerce env | `AdminGuard` + domain-separated admin-session HMAC |
| `SALES_ADMIN_KEY` | live Caddy + sales env | `AdminKeyGuard` |

`deploy/render-caddy.awk` carries the service keys from the live Caddy config via
placeholders. Values are never added to the repository. Human admins are created and
changed in the «Админы» ("Admins") tab; new and rotated passwords are hashed with
Argon2id. A one-time cutover importer migrates the old Caddy bcrypt rows before reload
and aborts the cutover if main-admin or CRM access was not preserved.
