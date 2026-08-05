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
`/gemini-subs`, `/kimi-subs` and `/glm-subs` to the three provider runtimes (KIMI and GLM
are backend-only planes inside the Anthropic runtime, there is no separate origin) and adds
the server keys; the browser never receives control keys, full email, OAuth, Google
project, KIMI/GLM subject, keys or proxy. The protection applies to all pages, including
`/sales/calculator`.

## Pricing configurators and B2B policies

The `/pricing` page is the operator surface of the versioned multi-discount authority:

- Global B2C is edited as a full CAS replacement set: default 50%, provider overrides and
  exact model overrides. An exact model rule takes priority over provider, provider — over
  the global default;
- provider switches show the master, product, B2C and B2B gates. Master is visually
  separated, and changing it or disabling any gate requires a separate browser
  confirmation. Saved policy rules are not deleted when a gate is disabled;
- a provider rule does not automatically include future models. The editor offers only
  models from the active product catalog; Gemini does not appear without an explicit
  catalog entry;
- the backend admin API publishes the canonical service inventory and exact-CAS mutation
  under `/admin/service-account-inventory`; it shows `purpose`, `responsible`, last
  verified engine status, all-runtime-model access and `billing_mode=meter_only`. The
  current UI does not automatically classify unknown accounts: until a separate form
  exists, the operator uses the same protected admin contract. Service does not edit
  product discounts and does not depend on balance;
- every save shows the new source version and is not declared applied until targets have
  matching desired/applied versions and an exact ACK. The UI shows job state, last error,
  actor, reason and the version time.

The `/business` page uses the same policy editor for existing B2B clients and active
invitations. Every B2B client owns a managed policy: invitation redemption copies the
invitation snapshot, and a manual B2C→B2B conversion from `/users` provisions a single-rule
Anthropic discount policy from the negotiated multiplier; re-running that conversion on a
client who still lacks the policy (converted before this provisioning existed) repairs it
without touching the active discount. For a converted customer the binding is re-pointed at the
client policy but the engine keeps running the confirmed backfilled lineage: the legacy delivery
lane rejects identity switches, so no new delivery is staged, a drifted desired state is healed
back to the confirmed one, and the scalar multiplier stays authoritative until the release
cutover delivers the identity switch. Saving a B2B client policy whose rules are a uniform
provider-level discount (for example 70% on every provider) also moves that authoritative
scalar and enqueues its engine delivery, so the edit actually changes the price the customer
pays today; the customer's usage page shows that scalar price wherever the policy governs the
provider while the policy is not engine-enforced. A non-uniform policy (per-provider or per-model differences) cannot be
one scalar — it leaves today's price unchanged and activates with the cutover. Post-cutover, a saved B2B policy also advances the live release-v2 authority:
a strictly newer release policy version is pinned through the append-only assignment
extension under the exact current head, and the CAS response reports that outcome. A new invitation is created immediately with a full provider/model policy; a
scalar discount editor does not exist in the active UI. An unredeemed invitation is edited
with CAS replacement versions, resend gets an independent exact snapshot, and after
redemption changing the invitation no longer changes the client policy.
Preview/email/registration describe the provider/model access and the account stays
pending until engine ACK; no usable key is issued until the policy is confirmed.

The admin panel does not perform Stage 5 assignment/backfill itself and does not derive
B2B/service/OpenKeys assignments from names. The commerce producer provides a protected
bounded snapshot of prepared target/recovery, Stage 8 freshness/source completeness,
durable activation jobs/receipts and separately a timestamped engine head via
`GET /admin/pricing-release-activation-v2`. The `/pricing` page polls this snapshot
separately every 5 seconds and shows the release pair, inventory identities, evidence
freshness/blockers, engine head, jobs and validated receipts. A stale/erroneous refresh is
kept only for diagnostics and fail-closed disables mutations.

The only mutation endpoint is the explicit `POST .../stage` with a verified actor/reason
and a canonical evidence digest. For cutover and recovery the operator enters a meaningful
reason and the exact phrase tied to the kind, generation and the suffix of the evidence
digest. Before the POST the browser re-reads the control snapshot and requires an
available engine, fresh passed/source-complete evidence without local blockers, a zero
pricing backlog and the correct global head state; recovery additionally requires the
exact target head and a durable cutover receipt. The backend remains the final authority
and repeats the checks atomically. `accepted` means a durable job, not a dry-run: the
worker may execute the global CAS immediately after fresh first-delivery revalidation.
Per-account canary and maintenance-mode controls do not exist and are forbidden.

Before activation, the same page hosts a separate `Managed Stage 8 capture` control. Every
five seconds it reads the bounded `/admin/pricing-stage8-capture-v2`, shows queue counts,
immutable request identities, attempts, exact engine/combined digests, freshness and only
a sanitized blocker summary. Raw engine/combined JSON and the original account/request
identities are never delivered to the browser. A new capture is staged only from an
explicit form with a new UUID, target/recovery, a closed epoch window,
provider/financial/Gemini bounds and a meaningful reason. The browser cross-checks the
window against commerce database time, requires the exact confirmation phrase, repeats a
fresh GET before POST and fail-closed forbids a second job while `pending|processing|retry`
has not become terminal. The backend remains the authority for strict shape, time and
idempotency conflict.

Capture is a read-only evidence workflow: it does not create an activation job, does not
move the head, does not change accounts/balances/policies and does not require a
traffic/money-writer drain. `blocked` is displayed as terminal evidence; retry is allowed
only by the worker state machine for uncertain failures. Activation remains a separate
section with a separate explicit confirmation below, so a successful capture by itself
does not switch clients over.

## Paying customers

The `/paying-users` page is a separate compact read-only control room, not a filter of the
general `/users` table. It receives an expand-only snapshot from commerce and explicitly
requests `GET /admin/finance/paying-users?days=1|7|30&funding=all`. This combines the
lifetime money-funded cohort (confirmed payments and admin-issued manual engine top-ups)
with strict bonus-only spenders in the selected window. A bonus-only user has no lifetime
payment/manual funding, positive window spend, and every event is complete immutable modern
`policy_v1|release_v2` attribution with `paid=0`, `bonus=spent`, `other=0` and
`unattributed=0`; legacy, mixed and incomplete rows are excluded. Search,
provider/status/funding filters, sorting and pagination stay on the server and therefore do
not degrade as the customer base grows. The additive bonus contract is consumed only after
GREEN exact producer SHA `b12a08fe872fb08a88943d7ade0a75a3e567b579`.

The top ledger separates lifetime money received (`paid_nano`, including the manual amount
in its copy) from the neutral selected-window bonus-only card (`bonus_only_spent_nano` and
`bonus_only_users`); bonus spend is not revenue. Window spend and the proportional
Claude/GPT/Gemini rail cover the whole selected cohort, and provider segments also serve as
quick filters. The table branches bonus rows only on producer-authored
`funding_kind='bonus_only'`, never by inferring from zero money, while money rows distinguish
payments, payment plus manual, and manual funding. It shows exact charged nanoUSD separately
for Anthropic/OpenAI/Google. Provider authority is taken from immutable pricing attribution,
and for legacy pricing — from the stored top-level provider engine ledger; the worker also
reprocesses previously provisional `unattributed` rows after producer evidence appears.
Versioned recovery v2 re-selects old `unavailable` rows created by the weak request-ID-only
algorithm, recovers still-available 30-day rows by the strict settlement fingerprint and
marks each exact/exhausted result with version `2`. A model ID is never converted into a
provider; remaining ambiguity is not lost and is shown as `другое` ("other"). Commerce CSV
keeps raw decimal nanoUSD strings and adds `funding_kind` plus the exact
`paid_funded_spent_nano`, `bonus_funded_spent_nano`, `other_funded_spent_nano` and
`unattributed_spent_nano` legs; JS `number` is used only for the bounded 0–10000
basis-points fraction.

The same page has a separate `OpenKeys` cohort backed by the same-origin read-only
`GET /openkeys-admin/paying-keys?days=1|7|30&limit&offset&q&status`. It shows only delivered,
non-removed keys, with mask/batch/seller, enabled state, face value, exact charged window total
and delivery time. Expanding a key shows the producer-authored provider and model rows, token
counters, and official versus charged nanoUSD separately; provider is never inferred from model
or API type. A row-local unavailable report is explicit and is never displayed as `$0`. CSV keeps
one row per key × provider × model (and one row for a key without models), includes stable key and
engine-account IDs, and labels money columns `*_nanoUSD_text`; their decimal integers carry a leading
apostrophe so spreadsheets preserve every digit. Untrusted text that could start a spreadsheet formula
is apostrophe-prefixed as well. OpenKeys totals do not enter the commerce ledger summary.

The window switch is shared, while search/status/pagination state is independent per cohort and
an interval change resets offsets without clearing filters. Only the visible cohort component and
its 30-second poller are mounted; the hidden producer is not called, including by the global
refresh control. The page performs no money mutations.

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

## Claude, Gemini, KIMI and GLM capacity boards

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
domain), for KIMI and GLM — an opaque roster id (email/subject/key are absent from the
wire entirely).

Above the details sits a unified control room of five Claude/GPT/Gemini/KIMI/GLM cards.
Each has only two equally readable rails: `5ч` ("5h") and `7д` ("7d") (KIMI and GLM label
the rails with the real `duration_secs`: 18000 → `5ч`, 604800 → `7д`, without fictitious
equivalents), current remaining / full-window API-$, used share, the number of routable
identities and coverage. This is the main screen for comparing the capacity being sold;
per-account details follow below without additional cache/model/token matrices. Instead of
false money the Claude card immediately shows `N сохраняется` ("N being saved"), `N
потеряно` ("N lost") or an authority error from `calibration_delivery`. Gemini applies the
same fail-closed contract and does not show stale API-$ under pending/degraded exact
authority. Its fresh provider quota/reset remains visible, while the money cell compactly
says `обновляем` ("refreshing"): a dollar-evidence failure must not blind the operator to
the real quota wall. KIMI and GLM hold the same contract on their delivery FIFOs.

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
  After the deadline the old value disappears and until the next probe the UI shows
  `обновляем` ("refreshing"), so it is not carried over into the new window. A missing
  reset and a pending/degraded FIFO also show `обновляем`. An exhausted window is not
  hidden by the general runtime-cooling: the row shows the exact `100%` and the countdown
  to the provider reset separately for 5h/7d, and the status is `лимит … исчерпан` ("limit
  … exhausted") without the internal term `cooling`. Until the reset the money stays `вне
  ротации` ("out of rotation"); after the deadline the runtime automatically returns the
  subscription to routing, and the panel's next poll shows it active. Dead and other
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
  switch (`POST /gemini-subs/{profile_id}/disabled`, body `{"disabled": bool}`). It is
  gated by the control key, not the read-only panel key, and disabling asks for
  confirmation because it removes pool capacity. The write lands in the engine authority
  (`pool_member_disables`), NOT in the Auth Bot's sealed roster, so it survives the next
  roster publication as well as a slot restart — the roster stays the authority for
  *credentials*, the engine for *routability*. A disabled profile keeps its row so it can
  be put back, reports `disabled: true`, shows `отключён оператором` ("disabled by
  operator") ahead of any automatic diagnosis, is never probed (so a revoked credential
  stops being retried), and leaves every capacity aggregate. Claude subscriptions do not
  use this path: they already carry `active|paused|disabled` and are switched through
  `sub status`, so no subscription ever has two competing switches.

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
saleable money, while fresh quota/reset remains visible. Dead (`live:false`), cooling on
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
