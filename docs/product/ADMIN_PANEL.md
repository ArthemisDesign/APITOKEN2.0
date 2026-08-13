# Admin panel — internal admin console (admin.apitoken.sale)

Next.js application `apps/admin` (`@claude-api/admin`). The UI was extracted from the
Rust engine's built-in admin panel (`crates/server/src/admin-panel.html`) into a separate
bounded context with its own release lifecycle — like sales (`apps/sales-web`) and
OpenKeys (`apps/openkeys`).

The closed sales calculator is available at `https://admin.apitoken.sale/sales/calculator`.
It compares Claude Pro/Max, paid ChatGPT and paid Gemini plans using live 5h/7d calibration,
produces a stable 30-day API-dollar equivalent and computes the discount, unused quota,
customer savings, foregone revenue and the gross difference. The page's money arithmetic is
integer nanoUSD. Cold anchors and Claude priors do not count as a measurement. If the plan
itself is not yet calibrated but the provider has another measured plan, the calculator
scales its API capacity by the official quota ratio and explicitly marks the result with a
`≈` sign and the status "calculated". An own measurement always takes priority and
automatically replaces the calculated value.

Calculation coefficients and their authority:

- Claude: Pro / Max 5× / Max 20× = `1:5:20` per [Anthropic pricing](https://www.anthropic.com/pricing).
- ChatGPT: Plus / Pro 5× / Pro 20× = `1:5:20`; Business has the same published 5-hour
  quota as Plus, per [OpenAI pricing](https://learn.chatgpt.com/docs/pricing). The runtime
  plan `chatgpt_pro` is the Authbot-purchased $200 subscription, i.e. Pro 20×; Pro 5×
  currently exists in the calculator only as the calculated line `chatgpt_pro_5x` at $100.
- Google AI: only the Pro and Ultra plans actually used by the pool are present in the
  commerce matrix. Google publishes for Ultra up to `20×` higher Gemini limits relative to
  Pro, so the calculated Pro / Ultra coefficient is `1:20` per
  [Google AI plans](https://one.google.com/about/google-ai-plans/). Code Assist
  Standard/Enterprise and Workspace Ultra are not surfaced in the calculator.

Scaling is performed separately for 5h, 7d and 30d exclusively through integer BigInt. The
only sources are direct measurements of plans from the same provider; already-calculated
values are never used recursively. If there are several direct anchors, the average of the
normalized values is taken. The published coefficients describe the 5-hour Claude/OpenAI
limits and the upper bound of Google AI Ultra; additional weekly limits and
workload-dependent consumption may differ, so the 7d/30d calculation does not count as a
calibration until the plan has its own live measurement.

## Composition

- Application: `apps/admin`, Next.js, listens on `127.0.0.1:3700`.
- No database or secrets of its own: neither migrations nor an env file (unlike
  sales/OpenKeys).
- No workspace dependencies besides Next/React — in TypeScript contexts this is a separate
  `admin` context with root `apps/admin` (like `web` → `apps/web`).
- Health endpoint: `GET /api/health` → 200 `{"ok":true}`.

The `/system` page obtains supply from `/overview`, which uses the same exact Claude
authority as `/capacity`. If canonical remaining is unavailable, the UI shows `—` and a
warning rather than `$0` or an old pool prior/EMA; the separate duplicate browser request
to `/capacity` has been removed.

## Realtime invalidation contract

The producer side exposes same-origin, credential-injected SSE feeds for the admin's shared
request cache: `/admin/events` (commerce), `/partner-admin/events` (sales),
`/openkeys-admin/events` (OpenKeys), `/proxy-admin/events` (Authbot) and engine feeds
`/events/{engine,openai,gemini,kimi}`. Each payload contains only `source`, affected resource
prefixes and an optional bounded reason/table identifier; it never carries the underlying admin
data or credentials. Engine changes come from real roster/health/quota transitions and from
successful authority writes for settlements/provider turns. Commerce, Sales and OpenKeys changes are commit-bound
PostgreSQL notifications; Authbot emits only when provider inventory or renewal results change.

Every connection begins with `resync`; listener lag or database reconnect produces another one
because event delivery is an invalidation hint, not durable state. Heartbeats are transport-only.
Engine resync/change delivery evicts the matching short-lived server response cache before the
browser refetches, so push cannot immediately return a stale cached projection.
Because invalidation delivery is deliberately not durable, the app also revalidates only the
currently mounted URL resources every 30 seconds while the tab is visible and online. Returning to
a visible tab or restoring network access triggers the same mounted-only refresh immediately.
Every admin request uses browser `cache: no-store`. This fallback bounds staleness after a lost or
suspended event without polling hidden pages or unmounted cohorts; SSE remains the immediate path.
The UI consumer keeps last-good data, deduplicates by actual request URL and revalidates only
mounted resources whose URL matches an emitted prefix. Multi-source screens subscribe to independent
URL resources, so a ready section does not wait for the slowest endpoint and one failed source does
not block its neighbors. A failed revalidation keeps that source's last-good section visible while
Error Center reports the refresh failure; only an initial failure without data renders the source as
unavailable. A request whose last
subscriber unmounts is aborted, while cached successful data survives navigation. Heartbeats never
enter the invalidation handler. The sidebar reports aggregate feed health and its explicit refresh
button is limited to resources mounted on the current screen. Timestamps exposed by a resource group
advance only after a successful response; starting or failing a refresh cannot make stale data look
newer than it is.
When more than three mounted sources fail together, Error Center collapses them into one
expandable alert with per-source retry and dismiss controls; it does not cover the page with an
unbounded notification stack. Initial failures say that no data exists, while failed revalidation
explicitly identifies retained last-good data.

## Release cycle (watchdog lane `admin`)

- Path classification: `wd_path_is_admin` in `deploy/watchdog-lib.sh` (`apps/admin/**`
  plus shared build files — a dependency bump rebuilds the release). The backend lane does
  not capture `apps/admin`.
- Baseline: `/var/lib/apitoken/watchdog/admin.sha`. Until the file exists, the first
  watchdog run deploys the context unconditionally (like OpenKeys).
- Release root: `/opt/apitoken/admin-releases/<sha>`, atomic `current` symlink.
- Deploy script: `deploy/admin-deploy.sh <sha>` — promotion of the tested candidate,
  atomic symlink, `systemctl restart apitoken-admin.service`, health gate on
  `http://127.0.0.1:3700/api/health`, symlink rollback on failure. No migrations.
- Unit: `systemd/apitoken-admin.service` (`User=deploy`, `next start -H 127.0.0.1 -p 3700`,
  hardening like `apitoken-openkeys.service`, including `AF_NETLINK`; no `EnvironmentFile`).
- GitHub: status context `deploy/admin`, deployment environment `production-admin`.

## First launch on the server

```bash
# 1. Roll out the updated units, sudoers and controllers (root)
deploy/install-watchdog.sh

# 2. Enable the unit once
systemctl enable apitoken-admin.service

# 3. From then on rollout is automatic: the watchdog will see changes in apps/admin
#    and call admin-deploy.sh
```

## Domain

`admin.apitoken.sale` is served by `apps/admin` on `127.0.0.1:3700` and is entirely closed
behind `managed_admin_auth`: login/password is verified by commerce internal auth with
domain grants. Caddy same-origin proxies the depersonalized `/capacity`, `/codex-subs`,
`/gemini-subs`, `/kimi-subs` and `/glm-subs` to the provider runtimes. KIMI is a backend-only
plane with its own stable loopback origin `127.0.0.1:8803`; GLM remains a backend inside the
Anthropic runtime. Caddy adds
the server keys; the browser never receives control keys, OAuth, Google project, KIMI/GLM
subject, keys or proxy. `/tripo3d-subs` and `/suno-subs` are fetched the same way but
intentionally have no Caddy origin yet — the Tripo3D and Suno planes are dormant, and the
subscriptions page degrades to its null state until the planes are activated. Full account
email has one narrow exception
described below for the
closed managed-admin `/proxies` response; the other subscription routes remain masked. The
protection applies to all pages, including `/sales/calculator`.

## Proxy lifecycle

The `/proxies` UI consumes authbot's separate proxy lifecycle contract and renders the full
producer-validated account email for each visible proxy. The authbot-owned
`GET /proxy-admin/inventory` and invalidation-only `GET /proxy-admin/events` routes listen on
loopback `127.0.0.1:8806`. The feed emits `resync` on connect and `change` only after provider
inventory or renewal results actually change; its keepalive is never a request trigger. Its
stable raw authorization secret is `/etc/apitoken/proxy-admin.key`, provisioned atomically before
the systemd unit and Caddy configuration. The `/etc/apitoken` parent is root-owned and
non-deploy-writable, unlike the deploy-writable `/srv/claude-api/data` parent; the key is a
`root:root` `0600` regular non-symlink file with exactly 64 lowercase hex bytes plus an optional
final LF. The installer migrates one exact legacy `AUTH_BOT_PROXY_ADMIN_KEY` assignment out of
`authbot.env` and fails on malformed, duplicate, or divergent input. It rejects either
`AUTH_BOT_PROXY_ADMIN_KEY` or `AUTH_BOT_PROXY_ADMIN_KEY_FILE` in `server.env` rather than accepting
that file as credential authority.

Systemd creates authbot's private per-service copy with
`LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key`. After loading `authbot.env`,
`engine-postgres.env` and optional `server.env`, its `ExecStart=/usr/bin/env ...` command assignment
pins `AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key`; it is not an `Environment=` assignment, so
an env file cannot redirect the path. Authbot's bounded parser accepts only that file. The root-run
Caddy installer and renderer use the same `/etc/apitoken/proxy-admin.key`; the renderer matches an
existing live `X-Proxy-Admin-Key` header name case-insensitively and rejects duplicate or mismatched
values. Caddy preserves `/proxy-admin/*`, sends the dedicated `X-Proxy-Admin-Key`, and forwards only
the actor established by `managed_admin_auth`; neither value exists in the Next.js environment or
browser bundle, and sibling services receive no copy of the dedicated value.
`ProtectProc=invisible` and `ProcSubset=pid` remain in force. After operator-subcommand early-return
handling and before loading daemon secrets, Linux authbot calls `prctl(PR_SET_DUMPABLE, 0)`, blocking
same-UID `ptrace`, `process_vm_readv`, and sensitive proc-memory access. Code already executing inside
authbot itself is within the same trust boundary, and no defense can protect against such in-process
code. Caddy also
overwrites `x-api-key` with the shared engine key only for mixed-version rollout and rollback to the
previous authbot binary. The dedicated key must differ from `CLAUDE_API_CONTROL_KEY`: the new authbot
ignores that compatibility header, accepts only the dedicated key for incoming proxy-admin requests,
and uses the shared key only on its outgoing `/codex-subs` and `/gemini-subs` status reads.
Shared-key holders therefore cannot read `account_email` from the new producer. The allowlisted item key set now includes
`account_email`, restricted to an ASCII local part using alphanumerics plus
``.!#$%&'*+/=?^_`{|}~-`` (no edge/consecutive dots, at most 64 bytes), followed by DNS-style domain
labels (alphanumeric/hyphen, no edge hyphen, at most 63 bytes each and 254 bytes total). It is the
sole full-identity managed-admin exception, allowed only in the closed `/proxies` response with
`Cache-Control: no-store` and in-memory handling; it must not be persisted to SQLite or logs. The UI
validates the same grammar and renders the entire value in the `Аккаунт` column; search includes it.
Proxy IP, proxy URL, credentials, token, subject, project and every other identity or secret remain
forbidden. Rows otherwise use opaque inventory IDs plus stable hashed proxy hints, masked order
hints, provider/plan, subscription and proxy expiry, liveness, binding status and a fail-closed
renewal reason.

Inventory serializes only subscription-backed Claude/GPT/Gemini rows durably and exactly bound to
an existing IPRoyal allocation (`binding_status=bound`) with liveness other than `dead`. The UI repeats
that boundary fail-closed and drops `dead`, `unbound`, `mismatch` and `unknown` binding rows if a
producer regression emits them; those states are not offered as filters. Unmatched IPRoyal
allocations, external/unbound/mismatched subscriptions and dead subscriptions are absent.
Legacy/external unique-IP reconciliation may still run in the background, but a row does not appear
until the exact binding has been written durably. GPT remains public `provider=gpt`, while its
existing durable binding namespace is `codex`. A legacy `gpt` binding migrates in place only on one
exact local id + order + allocation-IP match, preserving `inventory_id`; unresolved, mismatched or
ambiguous rows stay untouched. Claude and Gemini use the same name in both places.
Claude and GPT subscriptions expire 30 days after acquisition. Gemini `google_ai_pro` expires after
18 Gregorian UTC calendar months; Ultra and all other Gemini plans expire after 30 days. The UI marks
subscription expiry and proxy expiry independently with distinct operational colors and an inset
marker. Each cell warns when its non-null value is expired or at most exactly `3 * 86400` seconds from
the reference time; valid `inventory.observed_at` is authoritative, otherwise the browser uses
`Date.now()`. A value one second outside the boundary and a null value are not marked, and selection
or hover does not cover either warning.

GPT and Gemini liveness comes from an authoritative opaque-id join: authbot calls the sanitized
loopback defaults `http://127.0.0.1:8792/codex-subs` and
`http://127.0.0.1:8794/gemini-subs` with the shared engine control key and trusts only id plus status.
Unavailable, malformed, missing or duplicate evidence closes that provider. GPT accepts exactly
`account_state=healthy|suspect|dead`; an empty or unknown value is schema drift and closes the whole
GPT source. Gemini `authenticated!=true` is dead, while `disabled=true` is degraded and
nonrenewable rather than dead. The overrides
`AUTH_BOT_PROXY_ADMIN_CODEX_RUNTIME_URL` and
`AUTH_BOT_PROXY_ADMIN_GEMINI_RUNTIME_URL` accept only HTTP with a literal loopback host, the exact
provider path and no credentials/query/fragment/external origin.

Renewal is never automatic. New IPRoyal purchases set `auto_extend=false`, and authbot's free
background guard disables unexpected auto-extend on every ISP order. The admin UI uses the additive
`operator_renewable` decision for its count, bulk selection, checkbox and row action, so local
subscription expiry alone never disables manual renewal. Every new paid request carries
`allow_inactive_subscription=true`; an uncertain replay retains that exact flag with the original
UUID. The confirmation explicitly warns that the provider balance will be spent and the same proxy
order will be extended even when the local subscription is expired. Authbot still repeats exact
durable-binding, authoritative-liveness and provider-order checks before spending; only the local
expiry check is bypassed by this explicit operator flag. It snapshots exact allocation IPs privately
and groups selected allocations by order. For each group authbot validates every selected IP against
the exact IPRoyal order and sends it in the paid request's `proxies` list, so a selected allocation
can be renewed without forcing its unselected siblings. Already-disabled auto-extend is not toggled
again; only an enabled setting is disabled and exact-refetched before payment. A selection absent
from the exact order, another preflight failure, or explicit provider 4xx is reported as failed;
ambiguous transport, provider 5xx, or a
missing/invalid expiry confirmation after dispatch stays uncertain. After exact
same-key replay handling, a different UUID overlapping a queued or active inventory ID or exact
order/allocation gets safe `409 renewal_selection_busy` before insertion and cannot later be claimed
for a second spend; disjoint selections can proceed. Claiming repairs legacy or corrupt overlap in
one `IMMEDIATE` transaction: an explicit direct target wins, background work chooses the oldest
`(created_at,id)`, and existing `in_progress` work wins over pending rows. Every overlapping pending
sibling becomes terminal `indeterminate`, replays as uncertain without a provider call, and can never
later spend. Idempotency is unchanged; ambiguous provider outcomes are reported as `uncertain` and
are not automatically replayed. The contract may
show the exact IPRoyal reseller balance as nanoUSD, but never card metadata.

The existing `/subscriptions` page additionally shows acquisition/expiry/days-left lifecycle fields
from `/capacity`, `/codex-subs` and `/gemini-subs`; these are display data and do not by themselves
make a proxy renewable.

## B2B pricing

There is no `/pricing` page, policy editor, provider-switch UI, service-policy UI or release-cycle
control room. Those surfaces were deleted with the policy/catalog/release authority and must not
be restored from this document. The live model is `docs/commerce/PRICING_MODEL.md`.

The `/business` page owns B2B invitations and existing B2B terms:

- a new invitation records one scalar default discount; email-bound and copy-only invitations use
  the same idempotent create/revoke/resend workflow;
- an existing B2B client's dialog reads `GET /admin/business-users/:id/pricing`, shows the stored
  default and one optional field for each canonical provider (Anthropic, OpenAI, Google, KIMI,
  GLM), and writes them together through one `PATCH`;
- an empty provider field removes that override and falls back to the default; every value is a
  whole percentage, with the backend enforcing the corresponding `0..10000` multiplier bound;
- one save is one commerce transaction. It records the default and all provider rows together and
  enqueues a separately fenced delivery job for each changed target, so a partial editor save
  cannot become a permanent half-deal;
- the customer table is one row per B2B user even though delivery has several target jobs. Its
  delivery badge summarizes the complete bundle: `confirmed` is shown only when every existing
  default/provider target is confirmed; otherwise retry, processing and pending are surfaced in
  that priority order with the relevant error;
- the `/users` B2C→B2B action records the negotiated scalar and its delivery job. A new account is
  usable once its idempotent engine account exists with that scalar; no policy ACK or release head
  is involved.
- the `/users` table and its CSV derive both B2C and B2B labels from the persisted
  `multiplier_bp`. B2C has no tier ladder, but a dormant/historical `4000` row is displayed as a
  60% discount rather than being hidden behind the common `5000`/50% value; absent evidence is
  shown as absent, not replaced by a default.

Pipeline delivery/drift status is shown through `GET /admin/pipeline-health` on the dashboard and
finance surfaces. Official provider tariff changes are an engine operator action, not a browser
pricing editor (`docs/engine/CONTROL_API.md`).

## Partner payout readiness

The read-only `/partners` page combines the current payout list with the additive
`/partner-admin/payouts/engine` chain proof. Operators see the public hot-wallet address, exact USDT
balance in nanoUSD, exact BNB balance in wei, USDT required by eligible rows, BNB required at the
backend's pinned gas-per-transfer bound, and the send window. Integer strings stay integer through
all comparisons and formatting. Unavailable or malformed chain evidence is rendered as unavailable
and blocks the readiness verdict; it is never coerced to `$0.00`. The backend rechecks all balances
and accounting fences before any irreversible send, so this projection is operational guidance,
not an authorization to transfer.

## Gift credits

The `/users` action labelled `+ подарок` creates `admin-credit:*` funding. Its confirmation states
the invariant before submission: this is platform gift credit, not evidence of an external payment,
so it is excluded from revenue and partner-commission basis and is spent free-first. New audit rows
use the explicit reason `admin panel gift credit (not an external payment)`. A real off-platform B2B
payment must use a future typed payment workflow with durable evidence; it must never be represented
by this action or retroactively inferred from its free-form reason.

## Paying customers

The `/paying-users` page is a separate compact read-only control room, not a filter of the
general `/users` table. It receives an expand-only snapshot from commerce, defaults to
`GET /admin/finance/paying-users?days=1|7|30&funding=spenders&include_usage=true`, retains the
selected funding filter and always sends `include_usage=true`. The consumer was wired only after
GREEN exact producer SHA `d27033effc237156bce91a38d1ca0ff5b6e66cbd`. Search,
provider/status/funding filters, sorting and pagination stay on the server and therefore do not
degrade as the customer base grows; from the default cohort a search such as
`q=wwwvatroke@gmail.com` retains `funding=spenders` and `include_usage=true`.

The default cohort contains every positive selected-window commerce spender, including
mixed/other/legacy/unattributed evidence. Producer-authored `bonus_only` remains the strict
classification for zero-money rows whose complete immutable modern attribution is entirely bonus;
`spend_only` instead means spend without that strict classification and is never presented as
bonus-only. The `all` filter remains available and is labelled `деньги + строгий бонус`; the other
historical funding filters are unchanged. Funding colors encode evidence rather than account health:
green is reserved for the `payments` leg confirmed by a payment provider; a mixed row renders that
green provider-payment badge beside a neutral manual-top-up badge. Manual-only stays neutral,
strict bonus-only is blue, and unclassified spend is amber. The customer identity dot is neutral for
an active account and red only when disabled, so it cannot make a non-payment row look paid.

Each paginated row receives a minimal `usage` projection covering every distinct account found on
that user's window events. Commerce rows expand into exact producer/model details. `complete` means
all accounts are represented; `partial` shows `available_account_count/account_count`, warns that
both its totals and model table cover only the available part; `unavailable` says that data is
unavailable and never substitutes `$0`. A complete report with no models still shows its exact
request/official/charged totals. Provider is displayed only from the wire (`null` → `не указан`),
never inferred from model. Request/token counters and nanoUSD values stay decimal strings throughout
the consumer; only bounded coverage counts and the 0–10000 provider-share basis points use JS
`number`.

The top ledger separates lifetime money received (`paid_nano`, including the manual amount
in its copy) from strict selected-window bonus-only (`bonus_only_spent_nano` and
`bonus_only_users`); bonus spend is not revenue. A separate window card and the proportional
Claude/GPT/Gemini/Kimi rail cover all selected spenders, explicitly including mixed, legacy and
unattributed rows. The page therefore never calls this cohort "money-paying customers".
Provider segments also serve as quick filters. A named provider is subtracted from the residual
`другое / legacy` bucket in the same query that names it — otherwise its spend would appear twice,
once in its own column and once as residue — so Kimi moving into its own column reduces `другое`
by exactly the amount it now shows. GLM is still counted as residue: it has no column yet.

Commerce CSV preserves the exact funding legs and provider totals, then emits one row per
user × producer-authored provider × model. A complete/partial report without models and an
unavailable report each get one status row. It includes usage status/window/coverage, exact
request and token counters, model and total official/charged nanoUSD. All decimal integer strings
are serialized through `spreadsheetExactInteger`; user/provider/model and other text pass through
`spreadsheetSafeText`. Unavailable usage/model money remains empty rather than becoming a fake zero.

The same page has a separate `OpenKeys` cohort backed by the same-origin read-only
`GET /openkeys-admin/paying-keys?days=1|7|30&limit&offset&q&status&sort&dir`, consumed only after
GREEN exact producer SHA `65f2160f67f8662ec58fbf336444c0ca8b5ff76a`. It shows every non-removed
warehouse or delivered key, with mask/batch/seller, enabled state, explicit `stock|delivered`
lifecycle, face value, exact nullable lifetime engine spend, exact charged window total and nullable
delivery time. The default global order is lifetime spend descending; the operator can switch
`spent|nominal|created|delivered|status` and `asc|desc`. Expanding a key shows the producer-authored
provider and model rows, token counters, and official versus charged nanoUSD separately; provider is
never inferred from model or API type. A row-local unavailable report is explicit and is never
displayed as `$0`. CSV keeps one row per key × provider × model (and one row for a key without models),
includes stable key and engine-account IDs plus exact lifetime spend, and labels money columns
`*_nanoUSD_text`; their decimal integers carry a leading apostrophe so spreadsheets preserve every
digit. Untrusted text that could start a spreadsheet formula is apostrophe-prefixed as well. OpenKeys
totals do not enter the commerce ledger summary.

The window switch is shared, while search/status/sort/direction/pagination state is independent per
cohort and a window change resets offsets without clearing filters. Only the visible cohort component and
its realtime-backed request are mounted; the hidden producer is not called, including by the
current-screen refresh control. The page performs no money mutations.

## GPT capacity board on the subscriptions page

The GPT block of `/subscriptions` is a compact operator summary computed entirely from the
backend `/codex-subs` without any money authority of its own:

- the main strip sums `fleet_capacity_nanocredits`/`fleet_remaining_nanocredits` of all
  plan cohorts of the longest available positive window, shows the used share, the regular
  Standard API-equivalent and the maximum plan API-equivalent; both scenarios are labelled
  with the model/tier/context/token kind. If at least one cohort is not yet measured, the
  total nominal stays unknown rather than turning into an understated partial sum;
- the home table shows only the bounded masked email, runtime/integrity state, quota with
  a progress bar and reset, shared-cohort remaining credits and the regular/maximum
  API-equivalent. For different paid plans each email gets pooled capacity only of its own
  cohort. Opaque UUIDs, the raw immutable ledger, schedules and individual noisy capacity
  are not surfaced in the main UI;
- `conversion_models` is used locally only for the two API-equivalent values in the strip
  and the home table. Token-capacity and profitability matrices are not expanded in the
  main UI; the backend keeps publishing the plan catalog as a calculation/audit contract;
- a provider placeholder with a non-positive window is ignored. Until positive quota
  movement appears the UI shows the short `ждём Δquota` ("waiting for Δquota"), never
  substituting zero or a prior.

## Claude, Gemini, KIMI, GLM, Tripo3D and Suno capacity boards

The Claude block of `/subscriptions` deliberately keeps only one compact accounts table:
bounded email hint, routing/auth state, quota+reset and exact available/full API-$
separately for 5h and 7d. Workload evidence, token-only capacity, model profitability and
a local summary strip are not surfaced in the Claude block: the main fleet totals already
live in the unified control room above, and inside Claude the operator needs only the
windows of specific subscriptions. Gemini likewise keeps only the per-profile windows
table; the separate local summary strip, model-quota and profitability tables have been
removed. The old StatCard sets, proxy/transport details and long calibration explanations
are not surfaced on the main screen. In all pools the identity on the left is bounded: for
Claude/GPT/Gemini — an email hint (first four characters of the local part without the
domain), for KIMI, GLM, Tripo3D and Suno — an opaque roster id (email/subject/key/cookie/
session are absent from the wire entirely).

Above the details sits a unified control room of seven
Claude/GPT/Gemini/KIMI/GLM/Tripo3D/Suno cards.
Each card shows the provider's real rails: `5ч` ("5h") and `7д` ("7d") for the pools that
have them (KIMI and GLM label
the rails with the real `duration_secs`: 18000 → `5ч`, 604800 → `7д`, without fictitious
equivalents; Tripo3D has no windows — prepaid balance never resets — so its single rail is
`баланс`, the fail-closed remaining/full of the balance track, and a used share is not
invented for it; Suno labels its rail with the real length of the current monthly credit
window from `window_duration_secs` — 2 592 000 → `30д`, never a synthetic constant),
current remaining / full-window API-$, used share, the number of routable
identities and coverage. This is the main screen for comparing the capacity being sold;
per-account details follow below without additional cache/model/token matrices. Instead of
false money the Claude card immediately shows `N сохраняется` ("N being saved"), `N
потеряно` ("N lost") or an authority error from `calibration_delivery`. Gemini applies the
same fail-closed contract and does not show stale API-$ under pending/degraded exact
authority. Its fresh provider quota/reset remains visible, while the money cell compactly
says `обновляем` ("refreshing"): a dollar-evidence failure must not blind the operator to
the real quota wall. KIMI, GLM, Tripo3D and Suno hold the same contract on their delivery
FIFOs.

Claude is built from `/capacity`:

- `window_totals` and `available_nano` are decimal nanoUSD strings for the shared control
  room; `conversion_models` remains the backend catalog of the authoritative Standard/Fast
  metering rates;
- 5h API-dollar remaining is the accent column of each subscription next to the 5h
  quota/reset; the full window capacity is shown as a compact line `из $…` ("of $…").
  7d remaining/capacity stays as the adjacent comparison column. The table also keeps the
  masked email/plan and routing state. No prior is ever substituted: until exact evidence
  exists the UI shows `ждём данные` ("waiting for data"). A fresh exact quota fraction from
  the runtime gives the current remaining even when Anthropic has not sent a reset — then
  the UI writes `сброс уточняется` ("reset being clarified") rather than a false `0м`. If
  the current snapshot has gone stale due to a lack of requests but its exact provider
  reset is still ahead, the row keeps showing the last percentage, countdown and muted
  API-$ with the label `последнее` ("last known"); a new snapshot replaces them
  immediately. These dollars are diagnostic and do not enter the fleet saleable capacity.
  After the deadline the old percentage is never carried into the new window. The window is
  then empty by construction, so a healthy routable subscription shows the exact `0%`
  published by the backend (`windows[].quota_state = "window_rolled_over"`) instead of
  hiding a measured zero behind `обновляем` — an idle subscription must not look like one
  without evidence. Its money cell still says `обновляем` ("refreshing"), because a
  rolled-over window carries no fresh measurement and nothing may be sold from it.
  A pending/degraded delivery FIFO no longer blanks the quota: the exact percentage and
  countdown stay visible while only the money cell degrades, matching the Gemini/KIMI/GLM
  contract. The money cell names the cause from `windows[].missing_reason` instead of
  collapsing four different failures into one word: `ждём доставку` ("waiting for
  delivery") for a pending/degraded/unavailable FIFO, `ждём probe` ("waiting for a probe")
  for a stale or missing provider snapshot, `нет авторитета` ("no authority") when the
  calibration report cannot be read, and `ждём данные плана` ("waiting for plan evidence")
  before a plan cohort exists. When a window genuinely has no snapshot at all
  (`quota_state = "awaiting_probe"`) the quota cell still shows `обновляем`: a zero must
  never be invented. An exhausted window is not
  hidden by the general runtime-cooling: the row shows the exact `100%` and the countdown
  to the provider reset separately for 5h/7d, and the status is `лимит … исчерпан` ("limit
  … exhausted") without the internal term `cooling`. Until the reset the money stays `вне
  ротации` ("out of rotation"); after the deadline the runtime automatically returns the
  subscription to routing, and the provider event refreshes the mounted panel row. Dead and other
  non-routable accounts still do not look like sellable supply;
- `calibration_evidence` and `conversion_models` keep arriving from the backend as an
  audit/calculation contract, but the main Claude UI does not expand them into additional
  tables.

Gemini is built from `/gemini-subs` and preserves provider-specific semantics:

- workload 5h/weekly API-$ is the realized blend of the observed mixture, not a fixed
  nominal of a Google AI subscription. Fleet totals have canonical `*_nano` strings; float
  fields remain only for display compatibility. In the strip the 5h workload-$ comes
  first, and the per-profile table shows separate 5h and 7d workload-dollar remaining and
  full capacity (`из $…` — "of $…") next to the corresponding quota/reset;
- the profiles table shows the bounded email, auth state, quota/reset for 5h and 7d,
  available/full workload-$ and the number of available models. Private quota bucket ids,
  `remaining_amount`, token rates, Search and profitability are not surfaced in the main
  UI. An unauthorized, account-cooling or fully model-cooling profile keeps its quota for
  diagnostics but shows `вне ротации` ("out of rotation") instead of money and does not
  enter the fleet API-$;
- `conversion_models`, official quotas and their integer amounts keep arriving from the
  backend as an audit/calculation contract. The UI does not divide workload-$ by a token
  price and does not invent Gemini token capacity from a fraction alone;
- the profiles table carries the panel's one mutating control: a per-profile rotation
  switch (`POST /gemini-subs/{profile_id}/disabled`, body
  `{"disabled": bool, "hidden": bool, "reason": string?}`). It is
  gated by the control key, not the read-only panel key, and disabling asks for
  confirmation because it removes pool capacity. The write lands in the engine authority
  (`pool_member_disables`), NOT in the Auth Bot's sealed roster, so it survives the next
  roster publication as well as a slot restart — the roster stays the authority for
  *credentials*, the engine for *routability*. A disabled profile keeps its row so it can
  be put back, reports `disabled: true`, shows `отключён оператором` ("disabled by
  operator") ahead of any automatic diagnosis, is never probed (so a revoked credential
  stops being retried), and leaves every capacity aggregate. Claude subscriptions do not
  use this path: they already carry `active|paused|disabled` and are switched through
  `sub status`, so no subscription ever has two competing switches;
- a permanently dead profile can additionally be hidden from the board (`hidden: true`,
  a `hidden` column on the same row). Hiding is presentation only and is accepted **only
  for an already-disabled member**: hiding one that still serves traffic would remove live
  capacity from the operator's view while it keeps working, so the engine rejects that
  combination with `400` — the request can never succeed, so it must not come back as a
  retryable `503`, and the check runs at the boundary before any gateway/roster lookup.
  Re-enabling drops the flag with the row. The engine keeps reporting
  hidden profiles — the panel filters them and offers "показать скрытые (N)", because an
  endpoint that omitted the rows would make hiding irreversible from the UI. Buttons use
  the shared `.btn` system (`warn` to disable, `ghost` to hide/reveal) so they inherit the
  dark-theme treatment instead of falling back to unstyled browser controls.

KIMI is built from `/kimi-subs` (an `enabled:false` envelope is shown as "KIMI-контур
выключен" — "KIMI plane disabled") and repeats the same compact contract with two
source-specific features. The profile identity is only an opaque roster id and a bounded
plan label (`unreviewed` until the plan is reviewed); subject, email and credential path
are never serialized or displayed. Per-window `window_totals` are not published: fleet
rails and table columns are built from the real `duration_secs` of per-profile
quota/calibration. Used share is the exact provider `used_fraction_units`; API-$ is only
the calibrated `api_nano`/`current_nano` decimal strings, all arithmetic in BigInt. The
unknown stays `ждём данные` ("waiting for data") and never `$0`; pending/dropped delivery
and a persistence error show `сохраняется`/`обновляем` ("saving"/"refreshing") and hide
saleable money, while fresh quota/reset remains visible. The same `обновляем` covers the read
side: when the durable calibration store cannot be read (`calibration_authority_available:false`)
the fleet card turns bad with the caption `калибровка недоступна` ("calibration unavailable") and
money cells degrade instead of pretending the windows were never measured. Dead (`live:false`), cooling on
any of the three axes (auth/transport/quota) and a stale snapshot (>10 minutes) say `вне
ротации`/`обновляем` ("out of rotation"/"refreshing") and do not enter the fleet API-$:
fleet sums are computed only over profiles whose row shows real money, and null on any of
them makes the total unknown rather than a partial sum.

GLM is built from `/glm-subs` (an `enabled:false` envelope is shown as "GLM-контур
выключен" — "GLM plane disabled") and repeats the same compact contract with
source-specific features. The profile identity is only an opaque roster id and a bounded
plan label (Lite/Pro/Max or `unreviewed`); subject (key digest), the key itself, proxy,
base_url and credential path are never serialized or displayed. Unlike KIMI, the backend
publishes fleet `window_totals` for the two canonical windows (300/10080 minutes — a
projection of the exact `duration_secs` 18000/604800) as fail-closed sums: null on any
profile makes the whole window unknown, so the rail says `ждём данные` ("waiting for
data") rather than `$0`, and without `window_totals` the card degrades to coverage-only,
like KIMI. Table columns are built from the real `duration_secs` of per-profile
quota/calibration. Used share is the exact provider `used_fraction_units`; API-$ is only
the calibrated `api_nano`/`current_nano` decimal strings, all arithmetic in BigInt; the
native remainder (microcredits) is shown in a separate compact column as exact integers,
null stays `—`. GLM's auth axes are durable `account_dead`/`account_suspect` flags rather
than a timed quarantine: dead says `вне ротации` ("out of rotation") until the key is
replaced by the Auth Bot, suspect — `под наблюдением` ("under observation") until a fresh
quota probe, and a key without a passed probe (`live:false`) — `ждём данные` ("waiting for
data"); the timed cooling axes are only transport/quota. Pending/dropped delivery and a
persistence error show `сохраняется`/`обновляем` ("saving"/"refreshing") and hide saleable
money, while fresh quota/reset and native counters remain visible. A stale snapshot (>10
minutes) says `обновляем` ("refreshing"); fleet sums in the strip are computed only over
profiles whose row shows real money, and null on any of them makes the total unknown
rather than a partial sum.

Tripo3D is built from `/tripo3d-subs` (an `enabled:false` envelope is shown as
"Tripo3D-контур выключен" — "Tripo3D plane disabled"; the plane is dormant and has no
production Caddy origin yet, so until activation the fetch fails and the board shows the
null state `нет связи` — "no connection" — by design) and repeats the same compact
contract on a windowless balance track: prepaid API credits never reset
(`docs/engine/TRIPO3D_PROVIDER.md` §5.3), so there are no 5h/7d columns and no fictitious
equivalents — one row per identity, one money column per account. The profile identity is
only an opaque roster id and the bounded top-up cohort; subject (key digest), the API key,
proxy, base_url and credential path are never serialized or displayed. The provider's
verbatim balance halves (`balance_raw`/`frozen_raw`) stay visible as live facts — never
recomputed, never zero-filled — while the parsed micro-unit remainder appears in a separate
compact column as exact integers, null stays `—`. Saleable API-$ is only the calibrated
`api_nano`/`current_nano` decimal strings, all arithmetic in BigInt; the unknown stays
`ждём данные` ("waiting for data") and never `$0`. A HARD balance verdict
(`balance_walled`) says `баланс исчерпан` ("balance exhausted"), cooling on any of the
three axes (rate-limit/auth/transport) counts down, a key without a passed probe
(`live:false`) and missing evidence say `ждём данные`, a stale snapshot (>10 minutes) says
`обновляем` ("refreshing"); pending/dropped delivery and a persistence error hide saleable
money behind `сохраняется`/`обновляем`. Fleet sums are computed only over profiles whose
row shows real money, and null on any of them makes the total unknown rather than a
partial sum.

Suno is built from `/suno-subs` (an `enabled:false` envelope is shown as "Suno-контур
выключен" — "Suno plane disabled"; like Tripo3D, the plane is dormant and has no
production Caddy origin yet, so until activation the fetch fails and the board shows the
null state `нет связи` — "no connection" — by design) and repeats the same compact
contract on the monthly credit window of the paid plans (Pro/Premier —
`docs/engine/SUNO_PROVIDER.md` §5.2/§5.3). The profile identity is only an opaque roster
id and the bounded plan label; subject (session-id digest), the cookie, session id, proxy
and credential path are never serialized or displayed. Window identity is the exact
`window_duration_secs` of the evidence-derived `YYYY-MM` period — the UI labels the real
length of the concrete month (2 592 000 → `30д`) and keeps past months as their own
columns without ever multiplying rows: one identity is one row. The used share is computed
only from the provider's verbatim counters (`monthly_usage`/`monthly_limit`) in BigInt,
the reset is shown as `—` because the producer does not publish it, and the native
remainder (`total_credits_left` against `monthly_limit`) stays in a separate compact
column as exact integers — null stays `—`, never 0. Saleable API-$ is only the calibrated
`remaining`/`current_nano` decimal strings; the unknown stays `ждём данные` ("waiting for
data") and never `$0`. Removal from routable on a corroborated Clerk verdict
(`routable:false`) says `вне ротации` ("out of rotation"), a HARD quota verdict
(`quota_walled`) says `квота исчерпана` ("quota exhausted"), cooling on any of the four
axes (rate-limit/auth/captcha/transport) counts down, a session without a passed probe
(`live:false`) and missing evidence say `ждём данные`, a stale snapshot (>10 minutes) says
`обновляем` ("refreshing"); pending/dropped delivery and a persistence error hide saleable
money behind `сохраняется`/`обновляем`. Fleet sums are computed only over profiles whose
row shows real money, and null on any of them makes the total unknown rather than a
partial sum.
