# DEVBOT.md — Telegram dev bot design (`apps/devbot`)

Status: **stages 1–3 implemented** (alert webhook + commands, deploy poller, journald/silence —
the `apps/devbot` application; systemd unit, watchdog lane, Alertmanager rendering and heartbeat
alert — the deploy/monitoring plane). Remaining is **stage 4** — positive commerce business
events; it requires a new expand-only contract from commerce and does not start until that
contract exists (see "Open questions"). Every event source below is confirmed by a file in the
repository — if the code diverges from this map during implementation, the code is correct and
the document is updated in the same commit.

## 1. Goals and overview

Today the operator learns about problems in two ways: email from Alertmanager
(`observability/alertmanager/alertmanager.yml.template`, the single receiver
`production-email`) and GitHub commit statuses (`deploy/watchdog` and the per-phase contexts).
There are no positive events at all: a successful deploy, a completed migration, and recovery
after quarantine are not notified anywhere. Email is slow, and GitHub statuses have to be
checked manually.

Goal: one Telegram group with topics (a forum group) where the `apps/devbot` bot delivers all
project lifecycle events — alerts, deploys, migrations, engine incidents, validation
results — with sensible deduplication, and answers commands about the current state of the
systems.

Non-goals:

- the bot does not replace Alertmanager and Prometheus rules — it is a consumer of their
  signals, not a new alerting system;
- the bot does not write to production systems (no actions like "restart the service" from
  chat in the early stages); commands only read state, the single exception being `/silence`
  in Alertmanager (stage 3);
- the bot does not duplicate runbooks — it links to them.

## 2. Event source map

Three transports deliver events to the bot:

| Transport | What it carries | Integration point |
|---|---|---|
| **Alertmanager webhook** | All 43 alert rules (26 critical, 17 warning) | a new `webhook_configs` receiver in `observability/alertmanager/alertmanager.yml.template` pointing at the bot's loopback port |
| **GitHub API poller** | `deploy/*` commit statuses and `production-*` Deployments | the same contract as `deploy/agent-merge.sh` (`am_status`) and `deploy/watchdog-github.sh`; 30–60 s interval |
| **Journald tail** (stage 3) | manual interventions and rollbacks not reflected in GitHub | the `[watchdog]`, `[agent-merge]`, `[admin-deploy]`, `[sales-deploy]`, `[openkeys-deploy]` prefixes; read via `journalctl -f` |

### 2.1 Deploy pipeline (host-watchdog)

The source of truth is `deploy/watchdog.sh` (the `systemd/apitoken-deploy-watchdog.timer`
timer, polling `origin/master` every ~5 s). Events:

| Event | Signal | Where to read | Severity |
|---|---|---|---|
| New candidate in master | `pending deploy/watchdog` + `pending deploy/tests` (`watchdog.sh:113-122`) | Commit Status API | info |
| Test lane failed (TypeScript/Rust/Codex/Static) | `failure deploy/tests`, SHA in quarantine (`rejected.sha`, `watchdog.sh:279-295`) | Status API | critical |
| Commerce/engine migration | `pending/success/failure deploy/migration` (`watchdog.sh:1992-2001`); a deferred one — `pending-migration.sha` | Status API | high/critical |
| Component rollout | Deployments `production-{database,engine,backend,sales,openkeys,admin}` (`watchdog.sh:87-104`) | Deployments API | info/high |
| Health-gate rollback | `health check FAILED … rolled back` vs "rollback target also unhealthy — manual intervention required" (`deploy/admin-deploy.sh:95-105`, analogues in sales/openkeys) | journald | warning / **critical** |
| SHA quarantine | `failure deploy/watchdog` + the `DeployQuarantined` alert | Status API + Alertmanager | critical |
| Pipeline green | `success deploy/watchdog` "All selected production components verified" (`watchdog.sh:2300`) | Status API | info |
| Pipeline stalled | the `DeployPipelineStale`, `DeployStuckInPhase` alerts | Alertmanager | high |
| Manual retry / rollback | `apitoken-watchdog retry` (`deploy/watchdog-control.sh:48-57`), `deploy/rollback.sh` | journald | warning (human intervention) |

### 2.2 Prometheus/Alertmanager alerts

The full inventory is 43 rules in two files; each has a runbook anchor
`docs/ops/MONITORING.md#<alertname>` (consistency is gated by
`deploy/monitoring-config.test.sh`).

`observability/prometheus/rules/application.yml` (25 rules):

- **Engine (Anthropic)**: `EngineCircuitBreakerOpen` (critical), `EngineHasNoSubscriptions`
  (critical), `EngineAllSubscriptionsCooling` (critical), `EngineUpstreamAuthFailures`
  (critical), `EngineUpstreamRateLimited`/`EngineUpstreamServerErrors`/
  `EngineAffinityRedisErrors` (warning).
- **Codex provider**: `CodexProviderDown`, `CodexNoAvailableHomes`, `CodexHomeUnresponsive`,
  `CodexAccountDead` (critical); `CodexHomeUnauthenticated`, `CodexHomeNearRateLimit`,
  `CodexHomeRateLimited`, `CodexHomeSnapshotStale`, `CodexCalibrationPersistenceFailed`,
  `ConversationsForcedOffTheirHome` (warning).
- **Gemini provider**: `GeminiProviderDown`, `GeminiNoAvailableProfiles` (critical);
  `GeminiProfileUnauthenticated`, `GeminiUpstreamRateLimited` (warning).
- **Money and durable state**: `DurableQueueBacklog` (warning),
  `DurableQueueOldestItemStale`/`DurableQueueDeadItems` (critical), `FailedWebhooksPresent`
  (critical), `StaleCheckoutSessions` (warning), `EngineSettlementBacklog` (critical),
  `EngineExpiredLeasePresent` (critical), `BalanceDivergenceDetected` (critical),
  `BackupStale`/`BackupMissing` (critical), `DeployQuarantined`/`DeployPipelineStale`
  (critical), `DeployStuckInPhase` (warning), `DeployMigrationUncommitted` (critical),
  `SalesReferralReconciliationBacklog` (warning), `SalesPayoutBatchFailed` (critical),
  `CaddyUpstreamFiveXxRateHigh` (warning).

`observability/prometheus/rules/operations.yml` (17 rules): `MonitoringTargetDown`,
`PublicEndpointDown`, `ProxyUpstreamPairDown`, `BusinessCollectorStale`,
`BusinessCollectorMissing`, `SystemdCollectorFailed`, `HostClockSkew`, `HostDiskSpaceCritical`,
`PostgresUnavailable`, `ProjectSystemdUnitFailed`, `CriticalTimerFailed` (critical);
`CertificateExpiresSoon`, `JournalDeliveryFailing`, `HostDiskSpaceLow`, `HostInodesLow`,
`HostMemoryPressure`, `HostCpuSaturated`, `ServiceRestartLoop`, `PostgresConnectionsHigh`,
`PostgresDeadlocksDetected` (warning).

All of them are already structured (alertname, severity, component, summary, description,
runbook anchor) and arrive at a single webhook — the bot does not poll Prometheus itself.

### 2.3 Engine and commerce runtime incidents

Almost everything is covered by the alerts above (we do not duplicate them). Additional
signals available to the bot's commands (not pushes):

- the engine's `GET /ready` per slot with reason `draining`/`authority_unavailable`/
  `provider_unavailable` (`crates/server/src/http.rs:2999-3023`);
- `GET /settlement-health` — settlement outbox, failed in the last 24 h, backlog
  (`crates/server/src/http.rs`, control key);
- `GET /pool`, `/codex-subs`, `/gemini-subs` — subscription pool state (control/readonly
  keys);
- `GET /health|/ready` of commerce (`apps/api/src/health.controller.ts` — DB + engine),
  sales-api (`apps/sales-api/src/health.controller.ts`), openkeys
  (`apps/openkeys/src/app/api/ready/route.ts`);
- structured `customer_http_error` events in journald
  (`crates/server/src/http.rs:399-458`) — stage 3, an aggregated digest, not a push of every
  event.

### 2.4 Business events (stage 4, requires new sources)

Positive events (a successful payment, a new customer, a successful deploy as a business
milestone) do not exist in the current observability plane — `deploy/collect-monitoring-metrics.sh`
collects only failure aggregates. Candidate sources: commerce DB durable queues
(the `pending/paid/canceled/refunded` states of webhooks, `packages/payments/src/provider.ts`),
the payout cycle (`apps/sales-api/src/payout/payout.service.ts`). This is a new cross-context
link (commerce → devbot), formalized separately under the expand-only rules (see "Open
questions").

## 3. Group topic structure

The group is a forum group (topics enabled). Telegram assigns the topic id when the topic is
created; ids are pinned in the bot's env config (see section 7), not in code.

| Topic | What lands there | Policy |
|---|---|---|
| **🚨 Critical** | All critical alerts (firing + resolved), SHA quarantine, migration failure, "rollback target also unhealthy" | Immediate push. Resolved — a mandatory closing message |
| **🚀 Deploys** | New SHA, pipeline start, per-phase statuses, component rollouts, green finale, manual retry/rollback, deferred migrations | A push for every milestone; the phases of one SHA collapse into a single editable message |
| **⚠️ Warnings** | All warning alerts (firing + resolved) | Push with repeat collapsing (section 4); resolved — an edit of the original message |
| **💰 Commerce** | Money alerts (`FailedWebhooksPresent`, `StaleCheckoutSessions`, `SalesPayoutBatchFailed`, `DurableQueue*`, `BalanceDivergenceDetected`, `EngineSettlement*`) — duplicated from their severity topic; positive events (stage 4) | A duplicate carries only the header + a reply link to the message in Critical/Warnings |
| **📊 Digest** | Daily summary (see `/digest`) | One message per day |

Severity criteria: the rule's severity label (`critical`/`warning`) — from Alertmanager;
deploy events are classified by the bot per the table in section 2.1 (quarantine and "manual
intervention required" = critical → duplicated into 🚨 Critical).

The "one glance" principle: 🚨 Critical always shows everything critical across all domains;
domain topics provide context. Duplicating a critical into 💰 Commerce is done as a reply to
the original message, not a copy of the text, so resolved statuses cannot drift out of sync.

## 4. Notification composition

Format: `parse_mode: HTML`, `disable_web_page_preview: true` — as in the existing client
`crates/authbot/src/tg.rs`. Maximum 4096 characters per message (the Bot API limit) — the
alert description is truncated with an ellipsis; the full version is reachable via the runbook
link.

### 4.1 Alert (firing)

```
🔴 <b>EngineCircuitBreakerOpen</b> [critical · claude-engine]
<i>Аварийный размыкатель движка открыт ≥1 мин</i>
{annotations.description}
Runbook: {link to docs/ops/MONITORING.md#enginecircuitbreakeropen on GitHub}
Started: {startsAt, local tz} · Fingerprint: {short hash}
```

The `<i>...</i>` line is the alert's summary annotation as configured in the Prometheus
rules (Russian: "Engine circuit breaker open ≥1 min"); it is passed through verbatim.

Resolved: an edit of the original message (if it is within 48 h — the Bot API edit limit) —
header `🟢 RESOLVED` + duration; otherwise a new reply message. For 🚨 Critical, resolved
always goes as a separate message (it is important to see the closure in the feed).

### 4.2 Deploy milestone

One editable message per SHA in the 🚀 Deploys topic. While the deploy is running:

```
🚀 <b>Deploy</b> <code>1bd14c3</code> — feat(registry): expand provider calibration…
👤 <b>3xcalibur @3xcalibur-tech</b> · <i>Started 13:44</i> · <a href="{commit url}">commit</a>
✅ tests · ✅ migration · 🔄 engine · ⏳ backend
⏳ sales · ⏳ openkeys · ⏳ admin · ⏳ devbot
```

👤 — the commit author: git author name, plus `@login` when the commit email is linked to a
GitHub account (for agent addresses GitHub returns `author: null` — the git name remains).
The checklist is two fixed lines of 4+4 so wrapping does not split a phase in the middle. For
all eight phases the authority is the corresponding `deploy/*` commit status: that way a green
skip/no-change is visible immediately. The `production-*` Deployment is used only as an early
fallback while this phase's commit status has not been published yet, and it cannot roll an
already known status back to pending.

Every phase status is an edit of this message (the number of edits is unlimited). The finale
on a green `deploy/watchdog` is a compact summary without intermediate phases:

```
✅ <b>Deployed</b> <code>1bd14c3</code> — feat(registry): expand provider calibration…
👤 <b>3xcalibur @3xcalibur-tech</b> · <i>done in 12m</i> · <a href="{commit url}">commit</a>
```

The state stays truthful: all phases without an explicit failure are marked success (a green
watchdog means every lane passed, including those whose last visible status is still 🔄/⏳).
Quarantine: header `❌ <b>Deploy failed</b>`, the checklist IS PRESERVED (this is
diagnostics) + the failed-phase line + a separate message in 🚨 Critical with the author and
the first ~500 characters of the reason (the `wd_die` line from journald, stage 3; before
stage 3 — only the phase and a link to the status).

The finale is guaranteed even when HEAD moves ahead: agent-merge pushes the next master
immediately after the previous one goes green, so the previous SHA's `deploy/watchdog=success`
almost always escapes the HEAD diff. The poller keeps a "tail" — the previous SHA with a
non-terminal watchdog — and polls its statuses in parallel with HEAD, emitting its
phase/green/quarantine events; the router stores `previousDeploy`, and tail events edit the
message of exactly that deploy. One slot is enough: the next master cannot be pushed until the
previous one reaches a terminal state (merge-lock + the green check in agent-merge). A late
phase for a finished deploy is ignored — the final summary is not expanded back.

Events of one snapshot are processed strictly sequentially, and the poller waits for the whole
batch before saving the snapshot: `new-sha` first gets a Telegram `message_id`, then the phase
edits are executed, and the terminal watchdog comes last. This eliminates losing the finale of
a fast no-change/docs deploy and the reordering of concurrent `editMessageText`.

All operator timestamps and the daily digest use the IANA zone `DEVBOT_TIME_ZONE` (default
`Asia/Tbilisi`), regardless of the production host's timezone.

### 4.3 Deduplication and collapsing

- Alert dedup key: the `fingerprint` from the Alertmanager webhook. A repeated firing with the
  same fingerprint (Alertmanager repeat_interval: 4 h for warning, 1 h for critical) — not a
  new message, but an edit of the existing one: suffix `×N, last: {time}`.
- Alert storm: if more than 5 distinct criticals arrive within 60 s — after the fifth the bot
  sends one summary "🔥 N active critical alerts: {list of alertnames}" and then collapses
  everything into it until the storm ends (10 min without new ones). Protection against an
  avalanche during a cascading failure (`MonitoringTargetDown` across many jobs).
- Warnings with a frequency above 1/10 min per alertname — forced collapsing with a counter,
  minimum edit interval 5 min (Telegram rate-limits edits).

### 4.4 Telegram-side throttling

- An outbound queue limited to 1 message/s into the group and 20/min total (Bot API limits
  for groups); excess is coalesced into a summary.
- Send retries: a 429 from Telegram → honour `retry_after`; network errors — exponential
  backoff up to 5 attempts, after which the event is dropped with a log entry (the bot must
  not crash because Telegram is unavailable).
- The token is stripped from all error strings before logging — the `redact()` practice from
  `crates/authbot/src/tg.rs:75-77`; otherwise the token leaks into journalctl via the request
  URL.

## 5. Bot commands

Commands are received via long polling (`getUpdates`; the bot's webhook is explicitly deleted —
as in `crates/authbot/src/tg.rs:200-203`), processed only from the allowed chat_id and only
from user ids in the allowlist (`DEVBOT_ADMIN_IDS`); other messages are ignored silently.

| Command | Response | Sources |
|---|---|---|
| `/status` | Summary: pipeline phase (latest `deploy/watchdog` status + SHA), active alerts by severity, readiness of all planes (engine slots `/ready`, router, commerce `/health`, sales, openkeys, admin) | GitHub Status API, Alertmanager API `GET /api/v2/alerts`, HTTP probes of loopback endpoints |
| `/alerts` | List of active alerts: severity, alertname, age; grouped as in Alertmanager | Alertmanager API |
| `/deploys [N]` | The last N (default 5) SHAs with phase statuses and outcome | GitHub Deployments API + Status API |
| `/pool` | Subscription pool: live/cooling/dead by provider (Anthropic `/pool`, `/codex-subs`, `/gemini-subs`) | Engine Control API with a readonly key |
| `/settlement` | Money diagnostics from `/settlement-health`: outbox by state, failed in the last 24 h, backlog | Engine Control API with a control key |
| `/silence <alertname> <duration>` (stage 3) | Create a silence in Alertmanager | Alertmanager API `POST /api/v2/silences` |
| `/digest` (and daily at 10:00 `DEVBOT_TIME_ZONE`) | A 24 h summary in the 📊 Digest topic: deploys (success/quarantine), fired alerts by count, top recurring warnings | The bot's own event journal (state file) |
| `/help` | List of commands | — |

Command replies go to the topic where the command was invoked (`message_thread_id` from the
update).

## 6. `apps/devbot` architecture

A new application in the pnpm workspace: `apps/devbot`. Node 24, TypeScript, without NestJS
(no DI graph, queues, or DB — a plain service; the only dependency is the built-in `fetch`).
The Bot API is called through a thin client modelled on `crates/authbot/src/tg.rs` — no
Telegram frameworks: the surface is small (send/edit/answer/getUpdates), and a hand-rolled
client is already proven in the repo.

```
apps/devbot/src/
  main.ts            — wiring, config (zod, like apps/api/src/config.ts)
  tg.ts              — Bot API client: send/edit, thread ids, retry/429, token redact
  am-webhook.ts      — HTTP server 127.0.0.1:DEVBOT_PORT, Alertmanager webhook intake
  github-poller.ts   — commit statuses/deployments poller (30–60 s), milestone diff logic,
                       tail polling of the previous SHA until deploy/watchdog terminal
  journald.ts        — (stage 3) journalctl tail, prefix parsers
  router.ts          — event → topic/formatter routing
  dedup.ts           — fingerprint store, collapsing, storm coalescing
  commands.ts        — long polling, admin gate, commands
  state.ts           — JSON state file /var/lib/apitoken/devbot/state.json
```

Event flows:

1. **Alertmanager → webhook**: a new receiver in `alertmanager.yml.template`
   (`webhook_configs` pointing at `127.0.0.1:DEVBOT_PORT/alerts/{secret}`); routing continues
   along the existing tree — email receives everything as before; the webhook receives the same
   groups (expand-only: the email branch is unchanged). Grouping and inhibition stay on
   Alertmanager — the bot receives already-grouped notifications.
2. **GitHub poller**: reads statuses for the `origin/master` HEAD and the deployments list
   (`production-*`); a diff against the last known state in the state file → an ordered batch
   of deploy milestones, which the router fully processes before the next snapshot. The token
   is a separate read-only PAT (see section 7); it does not reuse
   `/etc/apitoken/github-watchdog.env` (root-only, different owner).
3. **Journald** (stage 3): `journalctl -f -o json` with filters on the syslog identifiers of
   the watchdog/deploy scripts; "rolled back", "manual intervention", `retry` events, and
   `rollback.sh` launches.
4. **Commands**: long polling → allowlist → handler → loopback probes and APIs.

State: a single JSON file (the last processed SHA and its phase message, a fingerprint store
with a 48 h TTL, counters for the digest). No DB is needed; losing the state file means
re-sending the current status, not a catastrophe.

Boundaries (per `docs/DEPENDENCIES.md`): devbot is a consumer of the Alertmanager webhook
contract, the GitHub API, and the public/loopback HTTP health endpoints and the engine Control
API (readonly/control GET only). The bot does NOT access engine PostgreSQL or the
commerce/sales/openkeys DBs. During implementation, the links `alertmanager → devbot`,
`github → devbot`, `engine Control API → devbot` are added to `docs/DEPENDENCIES.md`, and
`apps/devbot` is added to the `AGENTS.md` map in the same commit.

## 7. Security

- **Bot token**: `DEVBOT_TELEGRAM_TOKEN`, a separate bot from the sales one
  (`TELEGRAM_BOT_TOKEN` in `apps/sales-api` is taken by the login widget, see
  `apps/sales-api/src/telegram.ts`). Env file `/etc/apitoken/devbot.env`, mode 0600, owned by
  the service user; only keys go into `.env.example` in the repo.
- **Redaction**: the token is stripped from errors/logs (the pattern from
  `crates/authbot/src/tg.rs`); management keys, tokens, and internal account/key ids never
  reach group messages (an account id in alert data is acceptable — there are no secrets
  there; the alert format is already privacy-safe, see the test
  `customer_error_event_is_structured_and_redacts_request_data` as a sample requirement).
- **Alertmanager webhook**: binds only `127.0.0.1`; the path contains a 128-bit secret
  (`DEVBOT_AM_SECRET`); other paths get 404. Not exposed externally through Caddy.
- **Group**: the bot processes updates only from `DEVBOT_CHAT_ID`; commands — only from
  `DEVBOT_ADMIN_IDS` (user id, not username — usernames can change). The bot cannot be added
  to another group with any effect: a foreign chat_id is ignored silently.
- **GitHub**: a fine-grained PAT, scope limited to reading commit statuses/deployments/
  repository metadata; stored in the same env file.
- **Time zone**: `DEVBOT_TIME_ZONE` — a valid IANA zone for message timestamps and the daily
  digest; default `Asia/Tbilisi`; the host timezone does not affect the output.
- **Control API**: for `/pool`, `/settlement` — engine readonly/control keys via env; only
  GET endpoints are used.
- Outbound connections only to `api.telegram.org` and `api.github.com`; the systemd unit uses
  `NoNewPrivileges`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/apitoken/devbot` —
  modelled on the existing units in `systemd/`.

## 8. Deploy and observability of the bot itself

- The `systemd/apitoken-devbot.service` unit: `Restart=always`, env from
  `/etc/apitoken/devbot.env`, `After=network-online.target`. Port `DEVBOT_PORT=3800`
  (loopback; occupied ports: 8790–8799 engine planes and router, 3000/3001,
  3100, 3200, 3300, 3410, 3500, 3600, 3700 applications, 9090–9187, 9115, 12345 monitoring
  stack) — pinned in `docs/DEPENDENCIES.md`. Until `/etc/apitoken/devbot.env` is provisioned,
  the unit stays inactive via `ConditionPathExists` (no crash-loop), and the watchdog lane
  finishes with a green skip without moving `devbot.sha` — the first real rollout happens
  automatically once the secrets appear.
- Deploy — a separate host-watchdog lane modelled on `deploy/admin-deploy.sh`
  (single-instance, health gate `GET /health` of the bot itself with symlink rollback): runner
  `deploy/devbot-deploy.sh`, release root `/opt/apitoken/devbot-releases`, status context
  `deploy/devbot`, deployment environment `production-devbot`. Implemented in
  `deploy/watchdog.sh` (the `wd_path_is_devbot` classifier — `deploy/watchdog-lib.sh`).
  A candidate without the TypeScript lane (a deploy/observability/engine-only diff) does not
  carry a built `apps/devbot/dist` — by the classifier's construction its devbot code is
  identical to the running release, so the lane finishes with a green deferral WITHOUT moving
  `devbot.sha`; the real rollout happens on the nearest master with the TypeScript lane.
  Moving the baseline here is not allowed: otherwise every TypeScript-less master would go to
  quarantine after provisioning (`devbot-deploy.sh` fails on the missing dist).
- Bot observability:
  - a unit failure is caught by the existing `ProjectSystemdUnitFailed` (the `apitoken-*`
    pattern);
  - heartbeat: every 60 s the bot atomically rewrites
    `/var/lib/apitoken/monitoring/textfile/devbot.prom`
    (`devbot_heartbeat_timestamp_seconds`); the `DevBotHeartbeatMissing` alert (warning) in
    `observability/prometheus/rules/application.yml` fires when the unit is active but the
    heartbeat is absent or older than 300 s; the runbook section is
    `docs/ops/MONITORING.md#devbotheartbeatmissing`, consistency is gated by
    `deploy/monitoring-config.test.sh`. The file is published with mode 0644 regardless of the
    unit's `UMask=0077` — node-exporter reads the textfile as `nobody`; the directory is kept
    group-deploy writable (`root:deploy 0775`) by both `install-monitoring.sh` and the root
    collector `collect-monitoring-metrics.sh` (it recreates the directory every minute —
    reverting ownership breaks the heartbeat with EACCES);
  - Telegram API degradation is visible in the send-error log (journald) — no separate alert
    in stage 1.
- Last-resort channel: if the bot is dead, Alertmanager's email receiver keeps working —
  therefore the email branch in the Alertmanager config is never removed (expand-only).

## 9. Rollout plan

Each stage is a separate merge via `deploy/agent-merge.sh`; the stages are independent in
value (after stage 1 the bot is already useful).

**Stage 1 — alerts (MVP).** `apps/devbot`: tg client, am-webhook, router, dedup, the
`/status`, `/alerts`, `/help` commands. Alertmanager config: a webhook receiver next to email
(expand-only), `deploy/render-alertmanager.mjs` + `install-monitoring.sh` — rendering of the
new env (`DEVBOT_AM_SECRET`). systemd unit, `.env.example`, watchdog lane, the
`DevBotHeartbeatMissing` alert + runbook + monitoring-config test. Docs: this file (status →
partially implemented), `docs/README.md`, `docs/DEPENDENCIES.md`, `AGENTS.md` (the map).

**Stage 2 — deploys.** github-poller, the 🚀 Deploys topic, the `/deploys` command,
collapsible phase messages, quarantine duplication into 🚨 Critical.

**Stage 3 — journald and silence.** journald.ts (rollbacks, retry, rollback.sh, agent-merge
events), the `/silence` command, a `customer_http_error` digest by reasons.

**Stage 4 — business events.** Positive commerce events (payments, registrations, payout
batches) — requires an expand-only contract from commerce (webhook/outbox); it is formalized
producer-first per the `AGENTS.md` rules, and the link is added to `docs/DEPENDENCIES.md`.
The scheduled daily `/digest`.

## Open questions

- **Positive business events** (stage 4): no ready transport exists — a new contract from
  commerce is needed (a webhook to the bot or reading the durable queues). Do not start stage
  4 until this is resolved.
- **Duplicating criticals into 💰 Commerce**: if it turns out noisy in practice — collapse it
  down to the single 🚨 Critical topic; the decision is based on operational experience, and
  changing the topic structure is cheap (env config).
- **Log alerts from Loki**: there are currently no log-based rules (except
  `JournalDeliveryFailing`); a potential source for the warning topic via the Loki ruler — not
  used, requires a separate design.
- **Vercel deploys of `apps/web`**: statuses are posted by Vercel outside this repo
  (`deploy/agent-merge.sh:293-298` does not trust the combined status). Reading the Vercel
  Deployments API is possible, but that is a new external contract — not included in
  stages 1–3.
