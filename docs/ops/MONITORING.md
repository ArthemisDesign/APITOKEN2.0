# Production monitoring

The production host runs a self-contained Grafana, Prometheus, Alertmanager, Loki, Alloy, Node
Exporter, PostgreSQL Exporter, and Blackbox Exporter stack. Every monitoring listener is bound to
loopback. The only public entry point is `https://monitoring.apitoken.sale`, and Caddy admits it
through the same database-backed managed-admin authentication used by the other private consoles.

This design intentionally keeps customer identifiers, request contents, API keys, balances, and
payment payloads out of labels and dashboards. The custom collector exports aggregate queue and
database state only. Grafana users are auto-provisioned as viewers.

## Coverage and retention

- Host: CPU, memory, disk, inodes, clock, processes, systemd units, timers, and restart loops.
- Ingress: Caddy per-host request metrics plus TLS and HTTP synthetic probes.
- Engine and router: request totals, upstream 429/auth/5xx failures, breaker state, in-flight work,
  Claude/Codex/Gemini pool health, serial fallback continuations and their bounded reasons,
  exact per-plane `not_started` proofs, settlement backlog, and expired leases.
- Commerce: database health plus credit, adjustment, pricing, email, webhook, and checkout state,
  including the release-v2 pricing control queues (release control/activation jobs, funding
  normalizations, shadow rollouts, and managed Stage 8 captures).
- Sales: API/web health, email queue, referral reconciliation buffer, and payout-batch failures.
- CRM, Content Studio, public web, support, and mail: process/systemd health, HTTP probes, and logs.
- Deployments and recovery: watchdog/timer state and current backups for `commerce`,
  `claude_engine`, `sales`, and `apitoken_crm`. The collector also exports the delivery pipeline's
  own state — quarantine, status freshness, current phase, and uncommitted-migration marker — so a
  failed or stalled deployment pages the operator instead of waiting to be noticed in GitHub.
- Logs: host journald in Loki for 14 days. Prometheus metrics are retained for 30 days or 24 GB,
  whichever limit is reached first. The journal itself is persistent and bounded by
  `systemd/journald-apitoken.conf` (8 GB, 90 days); without that drop-in journald falls back to
  `Storage=auto`, which put the journal in tmpfs and discarded it on every reboot. Caddy's active
  health-check logger is excluded in `deploy/Caddyfile`: blue-green keeps one slot of each pair
  stopped on purpose, so those probes failed once per second forever and drowned out real entries.
  `ProxyUpstreamPairDown` covers the condition those lines were nominally reporting.
- Every terminal non-2xx `/v1/*` response to a recognized metered API key emits one JSON journal
  event with `event=customer_http_error`. It includes the engine `account_id`, non-secret `key_id`,
  status, static reason, fixed route template, server request ID, and the live account/key budget
  snapshot. Every such response also carries that same id back to the caller in `x-request-id`, so a
  customer quoting it can be found directly (`--grep='<request-id>'`) instead of by guessing at
  timestamps. It never includes the raw API key, email, key label, prompt/body, query string, or a
  client-controlled path ID. Internal retries are not events; only the status returned to the caller
  is recorded. Investigate directly with:

  ```bash
  journalctl -u 'claude-api-anthropic@*.service' -u 'claude-api-openai@*.service' \
    -u 'claude-api-gemini@*.service' \
    --since today --grep='"event":"customer_http_error"'
  ```

The Grafana and telemetry volumes are not business records. Dashboards and alert rules are
provisioned from Git, so telemetry can be rebuilt.

## Dashboard layout

`observability/grafana/dashboards/production-overview.json` (uid `apitoken-production`) is
status-first and section-based so an operator answers "is anything wrong right now?" without
scrolling:

- **Status row** (always expanded, top): firing-alert counts by severity, failed services,
  probes down, oldest backup, queued jobs, devbot delivery health (heartbeat age, Telegram
  send failures, last webhook age), a per-endpoint synthetic probe table (status, latency,
  24 h uptime), and the full alert list. Stat cards link to their runbook anchors.
- **Collapsed domain sections** below: Host & Platform · Database · Engine (pool, traffic &
  capacity) · Affinity, Redis & Codex History · Revenue & Business State · Codex (OpenAI) ·
  Gemini · Kimi & GLM (default-off) · Router & Unified API · Delivery Pipeline · Devbot &
  Notification Delivery · Billing Writer · Logs & Journals.
- **`$provider` template variable** filters the engine pool/traffic panels
  (`claude_api_*{provider=~"$provider"}`); the default is all providers.
- **Annotations** on every graph: firing alerts (red), `agent-merge` deploy events from Loki
  (blue), and systemd unit restarts (purple) — so incidents can be correlated with changes.
- Dashboard links: runbooks, devbot design, and this dashboard's Git source.

Rows are collapsed by default to keep the initial view compact; every stat that can be wrong
is thresholded green/yellow/red. The four PostgreSQL custom-format dumps remain
the authoritative hourly recovery artifacts and are validated again before database migrations.

## Failure-domain limitation

All monitoring was explicitly requested on the production server. Consequently, a total host,
power, network, or Docker failure also stops Alertmanager and cannot send outage notifications. The
synthetic probes detect application, TLS, routing, and dependency failures while the host remains
able to run. A truly independent host-down notification requires one minimal off-host dead-man or
external uptime check; this stack does not pretend to provide that guarantee from inside the same
failure domain.

## Routine operations

```sh
sudo docker compose --env-file /etc/apitoken/monitoring.env \
  -f /etc/apitoken/monitoring/compose.yaml ps
sudo systemctl status apitoken-monitoring-collector.timer
sudo journalctl -u apitoken-monitoring-collector.service --since '30 minutes ago'
curl --fail http://127.0.0.1:9090/-/ready
curl --fail http://127.0.0.1:9093/-/ready
curl --fail http://127.0.0.1:3101/ready
curl --fail http://127.0.0.1:3600/api/health
```

The watchdog treats `observability/`, `deploy/`, and `systemd/` changes as infrastructure. It tests
the exact candidate, installs the monitoring definitions, validates every component configuration
with its pinned container image, starts the stack, reloads Caddy, applies migrations, deploys the
affected applications, and finally checks the protected monitoring vhost and loopback readiness.

To test mail delivery without manufacturing an application outage:

```sh
sudo docker compose --env-file /etc/apitoken/monitoring.env \
  -f /etc/apitoken/monitoring/compose.yaml exec -T alertmanager \
  amtool alert add alertname=MonitoringDeliveryTest severity=warning component=monitoring \
  summary='Operator-requested delivery test'
```

Delete or let the short-lived test alert resolve. Never put SMTP,
Grafana, engine, or PostgreSQL credentials into the repository; they live in root-only environment
or rendered files under `/etc/apitoken`.

## Incident triage

1. Acknowledge the alert time, component, and first failure in Grafana.
2. Check correlated service logs for the same five-minute window.
3. Confirm whether the last watchdog deployment overlaps the incident with
   `sudo apitoken-watchdog status` and the deployment journal.
4. Stabilize traffic and data integrity before restarting anything. For payment, settlement, or
   payout alerts, preserve durable rows and retry through the owning application workflow.
5. Record cause, impact window, mitigation, and a follow-up threshold or test change.

## MonitoringTargetDown

Open Prometheus Targets and inspect the scrape error. Confirm the exporter container is running,
the listener is loopback-only, and its dependency is healthy. Do not expose exporter ports to fix
a networking error.

## PublicEndpointDown

Run the failing URL locally with `curl --resolve <host>:443:127.0.0.1`. Check Caddy and the named
upstream unit. A protected endpoint must return 401 without credentials; a 200 is an authentication
bypass and should be treated as a security incident.

## ProxyUpstreamPairDown

Both commerce API slots are failing their Caddy health check, so stable loopback origin
`127.0.0.1:8791` has nothing to route to. During the one-release provider bridge, Anthropic's
8787/8788 addresses also carry an intentionally opposite OpenAI health verdict; the unambiguous
`MonitoringTargetDown{provider="anthropic"}` alert covers stable 8790 until bridge cleanup.

Exactly one slot per pair is supposed to be running — the other is stopped and disabled by the
blue-green controller — so this alert means the *serving* slot died, not that a spare is idle.

```bash
systemctl is-active apitoken-api@3000.service apitoken-api@3001.service
curl -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8791/v1/ready
sudo apitoken-watchdog status
```

The watchdog reconverges the provider cohort and commerce pair on its next idle cycle through the
readiness-gated controllers. If it cannot, recover with `deploy/engine-bluegreen.sh` or
`deploy/api-bluegreen.sh`. Caddy no longer logs individual failed health probes —
that logger is excluded because a stopped slot is expected state — so use the unit journals and
`caddy_reverse_proxy_upstreams_healthy` rather than looking for proxy log lines.

## CertificateExpiresSoon

Inspect Caddy certificate automation logs and DNS reachability. Confirm the certificate served on
loopback and externally before forcing renewal.

## BusinessCollectorStale

Inspect `apitoken-monitoring-collector.service`, Docker/PostgreSQL availability, and SQL errors. The
collector preserves its previous textfile on failure, so the stale timestamp is reliable.

## HostDiskSpaceLow

Use `du` and Docker system reporting to identify growth. Preserve PostgreSQL dumps and live database
storage. Prune only verified disposable build caches, old immutable releases, or telemetry beyond
retention policy.

## HostMemoryPressure

Identify the largest resident processes and recent OOM messages. Avoid blind restarts of
PostgreSQL or in-flight payment workers.

## HostCpuSaturated

Correlate CPU by process with traffic, PostgreSQL query rate, and deployment activity. Rate-limit or
isolate the responsible workload before changing capacity.

## HostClockSkew

Check the host time-sync service and upstream NTP reachability. Clock correctness is safety-critical
for engine leases, auth expiry, payment events, and deployment timestamps.

## ProjectSystemdUnitFailed

Inspect `systemctl status` and the unit journal. Verify its immutable release symlink and environment
file before restarting through its owning deployment controller.

## CriticalTimerFailed

Re-enable the named timer only after checking why it stopped and whether missed backups, watchdog
runs, or metric collections must be caught up.

## ServiceRestartLoop

Inspect the earliest failure in the unit journal, not only the latest restart. Stop the loop if it is
amplifying load, then use the watchdog’s last known-good release path.

## PostgresUnavailable

Check `apitoken-postgres.service`, the Docker container health, disk space, and PostgreSQL logs. Do
not recreate or remove its host data directory.

## PostgresConnectionsHigh

Group `pg_stat_activity` by application/state, identify leaked or idle transactions, and reduce the
offending pool. Raising `max_connections` is not the first response.

## PostgresDeadlocksDetected

Correlate PostgreSQL deadlock details with the owning transaction logs. Preserve the conflicting
statement order for a code-level lock-order fix.

## EngineCircuitBreakerOpen

Check upstream 429, auth, and 5xx counters plus engine logs. The breaker is protective; correct the
subscription or upstream condition before overriding it.

## EngineHasNoSubscriptions

Inspect subscription loading and durable auth state. Replace or repair credentials through the
engine’s controlled admin workflow.

## EngineAllSubscriptionsCooling

Inspect per-subscription capacity, rate limiting, and reset windows. Reduce demand or add verified
capacity; do not bypass cooling safeguards.

## EngineUpstreamRateLimited

Correlate request rate, model mix, cooling state, and upstream resets. Confirm routing is spreading
requests without defeating sticky-cache behavior.

## EngineUpstreamAuthFailures

Treat new upstream 401/403 responses as a credential or account-health incident. Inspect durable
auth state and remove confirmed-dead subscriptions from service.

## EngineUpstreamServerErrors

Compare failures across subscriptions and public upstream status. The engine breaker should limit
cascading damage while the condition persists.

## ClaudeStoreFallbackFailing

This alert covers the Anthropic and OpenAI targets and can fire only after the corresponding normal
subscription rotation has already become terminal. For `provider="anthropic"`, compare the counters
with `EngineHasNoSubscriptions`, `EngineAllSubscriptionsCooling`, smooth-wait, the circuit breaker
and local upstream errors. For `provider="openai"`, inspect Codex home health/quota and local
transport failures. A healthy local provider pool should leave the attempt counter unchanged.

Check ClaudeStore availability, account credit and the plane-specific root-owned credential without
printing the key or upstream response body. Repeated Anthropic failures are contained with
`CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED=0`; OpenAI failures use
`CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED=0`. Apply the switch through the normal config rollout
and start a watchdog-controlled engine cycle. Do not add another external cascade or replay a stream
that has already delivered model output.

## EngineAffinityRedisErrors

Check `apitoken-affinity-redis.service`, its container health and disk space. Engine traffic is
intentionally fail-open on local affinity, so restore Redis without restarting healthy engine slots;
expect only a temporary reduction in cross-slot prompt-cache hits.
Infrastructure delivery preserves a healthy Redis container unless
`systemd/apitoken-affinity-redis.service` or `deploy/affinity-redis.compose.yaml` changed. The managed
sysctl definition also keeps `vm.overcommit_memory=1`, allowing Redis background persistence to fork
without a false allocation refusal; verify it with `sysctl vm.overcommit_memory`.
This alert covers every engine plane. It was previously scoped to `{provider="anthropic"}`, which
silently excluded the OpenAI plane — the one plane where Redis also carries Codex response history
and where its loss is customer-visible rather than a cache-hit regression.

## AffinityRedisDown

Two Redis instances now run side by side, and the `instance_role` label says which one fired:

| role | listener | exporter | holds | loss impact |
|---|---|---|---|---|
| `history` | 6379 | 9121 | Codex response history | customer-visible 400 |
| `affinity` | 6380 | 9122 | cache-lineage affinity | prompt-cache hit rate only |

The exporter is unavailable or cannot reach its instance. Check the `redis` targets in Prometheus,
then inspect `apitoken-monitoring-redis-exporter-1` / `apitoken-monitoring-redis-exporter-affinity-1`
and `apitoken-affinity-redis.service`. The password map remains root-owned mode `0600`; both
exporters run as UID 0 solely to read that bind mount and drop every Linux capability before
startup. A `permission denied` for `/run/secrets/affinity_redis_password` means this sandbox
contract or the file mode drifted; do not make the secret world-readable.

The monitoring installer must prove both exporter `/metrics` endpoints, `sum(redis_up == 1) == 2`,
and all eleven fixed internal targets before committing a new configuration. If that
admission passed but the alert fires later, confirm with `systemctl status
apitoken-affinity-redis.service` and `docker compose -f
/usr/local/lib/apitoken-watchdog/controller/affinity-redis.compose.yaml ps`.
Losing the `affinity` instance degrades Claude, Gemini and OpenAI alike in prompt-cache hit rate
only. Losing the `history` instance is sharper: Codex `previous_response_id` continuity falls back
to per-process memory, so a conversation started on one slot cannot be continued on the other. Do
not restart engine slots to "fix" that — restarting discards the local history that is then the
only copy.

## AffinityRedisEvictingKeys

An instance reached its `maxmemory` and began deleting keys. Read `instance_role` first, because the
consequence differs completely:

- `affinity` — short digests under `allkeys-lru`. Losing them costs prompt-cache hits and money,
  not correctness. Raise `--maxmemory` for that service if it is chronic.
- `history` — whole conversations, up to 16 MiB each (`MAX_SERIALIZED_HISTORY_BYTES`), retained for
  `CLAUDE_API_CODEX_HISTORY_TTL_SECS` (24 h by default). Losing one makes `prepare_turn` answer
  `previous_response_id was not found` as a 400, which well-behaved clients treat as permanent and
  respond to by discarding the conversation. Treat this as customer-visible.

The two ran in one instance until the split; a few dozen large conversations could fill it and evict
affinity, and affinity churn could delete conversations. They now have independent budgets
(128 MiB affinity, 512 MiB history) so each can be sized on its own evidence. The first split keeps
the existing 6379 Compose service identity, command and data directory unchanged and adds 6380 with
an in-place `docker compose up`; it must not use `systemctl restart`, which would run Compose
`down` and make stored response history temporarily unavailable.

Inspect with `redis-cli -p 6379 --no-auth-warning info memory` (history) and
`redis-cli -p 6380 ...` (affinity). Mitigations, in order of preference: raise `--maxmemory` for the
affected service in `deploy/affinity-redis.compose.yaml`, or for `history` lower
`CLAUDE_API_CODEX_HISTORY_TTL_SECS`. Restarting Redis is not a mitigation — it discards everything
at once.

## AffinityRedisMemoryHigh

Usage passed 85 % of that instance's `maxmemory`. This fires before `AffinityRedisEvictingKeys` so
the budget can be adjusted while no data has been lost yet. Raise the budget of the instance named
by `instance_role` rather than both. If this recurs on `history`, it is genuinely undersized for the
retention in force; treat the TTL and the memory budget as one decision.

## CodexHistoryWriteFailures

`claude_api_codex_history_write_failures_total` counts turns whose response history reached local
memory but never the shared store. The turn itself succeeded and the client saw nothing wrong, which
is exactly why this needs an alert: the damage appears later, when a follow-up request lands on the
other slot and cannot resolve `previous_response_id`.
Check `AffinityRedisDown` and `AffinityRedisEvictingKeys` first — both cause this. If Redis is
healthy, check `claude_api_codex_history_redis_errors_total` for timeouts against
`CLAUDE_API_CODEX_HISTORY_REDIS_TIMEOUT_MS` (1000 ms by default).

## CodexHistoryMissesElevated

Lookups of `previous_response_id` are finding nothing in either tier. Every miss becomes a 400 to a
client that already paid to build the conversation. Legitimate causes are an expired TTL and a
client replaying a genuinely old id; illegitimate causes are eviction and a lost shared store.
Correlate with `redis_evicted_keys_total` and with `claude_api_codex_history_write_failures_total`.
If misses rise while writes are failing, the shared store — not the client — is at fault.

## BillingPGCommandLatencyHigh

`claude_api_billing_pg_command_duration_seconds` measures the PostgreSQL reserve, settle and
acquire_capacity commands around the retry wrapper, so reconnects and retries count toward the
latency a request actually pays. The writer is a single thread by design: while one command waits,
every other money command queues behind it. Slow commands almost always mean the database, not the
engine — check `pg_stat_activity` for sessions waiting on `pg_advisory_xact_lock` (reserve and
settle serialize per account through it), look at node-exporter for disk/CPU saturation on the
PostgreSQL host, and only then suspect connection churn (a repeatedly reconnecting writer shows up
as latency spread across all three `op` labels at once). If latency is high and
`BillingWriteQueueBacklog` fires too, treat it as a database incident, not a traffic spike.

## BillingWriteQueueBacklog

`claude_api_billing_write_queue_depth` counts occupied slots of the 4096-slot channel that feeds
the single billing writer thread. Depth above half for five minutes means commands arrive faster
than PostgreSQL drains them. First check `BillingPGCommandLatencyHigh`: if it fires, the queue is a
symptom of a slow database — fix the database. If latency is normal, this is a genuine arrival
burst outgrowing one writer; the queue absorbs it, but sustained growth eventually back-pressures
request handling, so estimate the drain time (depth divided by commands per second from the
histogram `_count`) and prepare to shed load if it exceeds the burst window.

## ExecutionGroupDoubleWinner

Treat any increment as a correctness incident even though the durable fence protected customer
money: two requests in one explicit router fallback group both reached a nonzero settlement path.
The first committed claimant remains the winner; the later attempt is settled as zero and its full
hold is refunded, including its original strict-policy funding buckets.

Preserve the engine and router journals for the affected interval. Correlate the bounded
`event=execution_group_double_winner` record by group, winner request, loser request and attempt;
then inspect why the router received `not_started` or `ConnectionRefused` for the earlier attempt
despite a later nonzero settlement. Confirm all four public vhosts still strip
`x-apitoken-execution-group` and `x-apitoken-attempt`. Do not clear the winner row, replay money,
or enable a broader fallback canary until the execution-state boundary is fixed and verified.

## RouterMetricsDown

Keep fallback disabled while telemetry is unavailable. Check `claude-router.service`, confirm
`127.0.0.1:8802` is listening, and run `curl --fail http://127.0.0.1:8802/metrics` on the production
host. The endpoint is intentionally unauthenticated because both the process and Prometheus are
loopback-only; do not expose `/metrics` through Caddy or add a credential to request labels.

If the endpoint works, inspect the `claude-router` target in Prometheus and reload the provisioned
configuration through the normal watchdog path. The generic `MonitoringTargetDown` rule excludes
this one job to avoid duplicate pages; this alert is its complete scrape-health coverage.

## RouterFallbackRateHigh

Compare `claude_router:fallback_rate5m` with
`claude_api:execution_not_started_rate5m` by plane and the bounded router attempt logs for the same
window. A high rate can be legitimate during a provider capacity event, but it can also show a bad
preset, stale catalog entry, or a plane returning `not_started` too broadly. Preserve labels only at
namespace/plane granularity; never copy credentials, model IDs, request IDs, or execution groups into
Prometheus.

Before increasing canary traffic, confirm `ExecutionGroupDoubleWinner` stayed zero,
`apitoken_balance_divergence_nano` is zero, and settlement/lease alerts are clear. If the rate is
unexpected after GA, roll back `CLAUDE_ROUTER_FALLBACK_ENABLED` through a reviewed unit commit; do
not weaken the retry proof or remove the expand-only contract.

## RouterConnectionRefusedFallback

Use the `from_namespace` and `to_namespace` labels to identify the failed stable origin and the lane
that accepted the continuation. Check the corresponding loopback listener (`8790`, `8792`, or
`8794`), provider systemd slots, and the latest watchdog deployment before restarting anything.
`ConnectionRefused` is the only transport error that proves the request was not accepted; timeouts,
resets, DNS errors, and unsigned 5xx must remain terminal and must never be added to this alert as
safe fallback reasons.

## RouterAdmissionFailures

Compare `claude_router_body_admission_overload_total` and
`claude_router_body_read_timeout_total`, then inspect
`claude_router_active_body_admission_units` beside `claude_router_active_universal_requests`.
Admission is a fixed 64-unit memory guard, not an execution queue: declared bodies reserve their
rounded MiB weight immediately; unknown/chunked bodies start at one unit and grow as bytes arrive.
An overload with 64 active units means real buffered pressure. Repeated timeouts with few bytes are
usually clients that made no byte progress for 15 seconds; steady uploads may run for at most five
minutes. Confirm Caddy/client upload behavior and abusive source patterns without logging
credentials or request contents. Do not increase the budget until RSS headroom is measured, and do
not add a waiting semaphore or provider execution ceiling.

## RouterAuthorityFailures

Split the alert by its fixed metrics: `claude_router_auth_preflight_total{outcome="unavailable"}`,
`claude_router_catalog_refresh_total{outcome="failed|oversized"}`,
`claude_router_pricing_failure_total{reason="unavailable"}` and
`claude_router_policy_failure_total{reason="unavailable"}`. Catalog labels identify only the fixed
namespace; auth/pricing/policy never expose a key or model. Check stable loopback origins 8790,
8792 and 8794, then provider slot health and Caddy logs. An `oversized` catalog is a producer
contract violation (body over 4 MiB or more than 1,024 models), not a reason to
raise consumer bounds blindly. Fresh-cache hits and last-good degradation remain visible through
`claude_router_catalog_cache_hit_total` and `claude_router_catalog_degraded_total`.
Waiting catalog callers share the exact failed in-flight refresh; a later independent request
retries immediately, so a plane outage must not create a serialized refresh convoy or a lasting
router-owned circuit breaker.

Auth latency is the rate of `claude_router_auth_preflight_duration_seconds_sum` divided by the rate
of `claude_router_auth_preflight_total` for the same outcome. Customer auth no longer starts all
three authorities together: Anthropic starts immediately, OpenAI is hedged after 50 ms without a
conclusive result, and Gemini after another 50 ms; an inconclusive response with no useful active
probe advances immediately. A healthy fast Anthropic path should therefore stay below the first
hedge and avoid secondary authority work. Latency clustering just above 50 ms or 100 ms points to
an earlier authority being slow or inconclusive while a later one wins. Latency approaching the
two-second per-probe bound together with `outcome="unavailable"` means all started authorities
failed to return a conclusive schema-v1 success or terminal 401; test 8790, 8792 and 8794
individually and compare their provider-slot and PostgreSQL authority health. The aggregate metric
has no origin label, so do not infer which plane failed from it alone. Personalized pricing and
policy remain uncached and fail closed.

## RouterResponseHeaderTimeout

Use the fixed `namespace` label on `claude_router_response_header_timeout_total` to identify the
read-only `/balance` authority, then inspect that plane's loopback health and shared account-state
latency. Billable data-plane requests no longer contribute to this metric and have no router-owned
response-header deadline: long non-stream generations wait for the provider or client disconnect.

`/balance` cannot execute or reserve money, so its two-second deadline may safely continue to the
next fixed authority. `claude_router_balance_failover_total` records that continuation alongside
transport/5xx failover. A persistent increase means one runtime cannot serve shared account state
within the bounded read path and should be investigated even when clients still receive a successful
balance response from another plane. Never generalize this safe read-only retry to billable traffic;
there only exact `not_started` or TCP `ConnectionRefused` may continue a universal request.

## AnthropicCalibrationPersistenceFailed

`claude_api_anthropic_calibration_persistence_ok` becomes zero when the exact turn FIFO has a
pending head, any event was quarantined/dropped, or the PostgreSQL calibration report cannot be
read. Customer traffic remains available, but `/capacity` deliberately returns `null` for current
remaining/horizons; do not fall back to pool prior/EMA or enter a nominal subscription value.

Inspect `claude_api_anthropic_calibration_pending_events`,
`claude_api_anthropic_calibration_dropped_events_total` and
`claude_api_anthropic_calibration_authority_available`, then check the Anthropic slot journal,
PostgreSQL reachability, owner fencing and migrations 0019/0020. A transient pending queue drains in
FIFO order automatically on the next turn/poll and is retried during graceful shutdown. A nonzero
dropped counter means overflow or immutable request-id conflict: preserve the logs and event rows,
verify request-id uniqueness, and restart only after the cause is understood. Restarting merely
clears process diagnostics and can hide an evidence gap; it is not a calibration repair.

Fleet planning uses `claude_api_anthropic_window_capacity_usd` and
`..._remaining_usd{window_minutes="300|10080"}`. Capacity may remain present while remaining is
omitted: full-window historical evidence is still valid, but the current snapshot/delivery is not.
Coverage is visible through routable/calibrated/snapshot subscription gauges and confidence ratio.

## AnthropicQuotaSnapshotStale

Exact provider quota observations have stopped refreshing for the routable Anthropic fleet. Every
window gauge keeps publishing its last observed fraction, so the numbers can look entirely healthy
while the path that produces them is broken; this alert is the only signal that separates "the
window is 32% used" from "the window was 32% used an hour ago".

`claude_api_anthropic_quota_last_observation_timestamp_seconds` is the newest snapshot across the
fleet and `claude_api_anthropic_quota_snapshot_subscriptions` is how many routable subscriptions
have any snapshot at all; a fleet that has never been probed publishes no timestamp and cannot fire
this alert. The 900-second threshold is the same freshness bound `/capacity` uses to decide whether
current remaining may be priced, so when this fires the panel has already degraded: money cells
read `обновляем` and each window falls back to its last-known percentage or, past the provider
reset, to an exact zero for healthy subscriptions.

This is a diagnosis alert, not an outage: customer traffic keeps routing and money stays
fail-closed rather than wrong. Idle healthy subscriptions are probed every 5–7.5 minutes by the free
count-tokens poller, so exceeding 900 seconds means that path stopped. Check, in order: the
PostgreSQL `subscription-poller` leader lease (only the elected slot probes, so a lost or flapping
lease silently suspends the sweep for the whole fleet), the Anthropic slot journal for `poll_sub
transport failure` (a wedged persona proxy makes probes fail while `polled_ts` still advances, so
the next attempt is deferred a full interval), and whether `CLAUDE_API_POLL` is disabled on the
serving slot. A blue-green cutover or restart clears the in-memory snapshots by design and
resolves itself within a few minutes as the new leader probes the fleet; a value that keeps growing
past that does not.

Distinguish this from **AnthropicCalibrationPersistenceFailed**: that alert means the money
authority is undelivered while the provider quota is still fresh and visible. This alert means the
provider quota itself is frozen. They fire independently and have different causes.

## CodexProviderDown

Only the OpenAI-compatible surface is affected; Claude routing is independent. Check
`systemctl status claude-api-openai.service` and its journal first, then check
`claude_api_codex_home_process_live` per profile to see whether every sealed credential failed to
open or only one. A persistent zero means the roster or the keyring no longer matches
`docs/engine/CODEX_PROVIDER.md` (profiles unreadable, wrong `CLAUDE_API_CODEX_CREDENTIAL_KEYS`, or an
envelope that fails validation). Do not edit envelopes on the host; republish them through the
authbot or `claude-api codex-seal`, and the gateway picks the roster up on its next health tick.
`CLAUDE_API_CODEX_ENABLED=0` is the provider-only kill switch if the surface must be withdrawn while
the cause is investigated.

## CodexNoAvailableHomes

No home would be selected right now, so clients are being told to retry. The counter is computed by
the same predicate selection uses, so it covers every reason a home is unroutable — a dead account, an
unresponsive transport, cooling, or a full window. It previously re-derived a weaker rule and could
report capacity the gateway was refusing to route to. Read
`claude_api_codex_home_cooling_until_seconds` and `claude_api_codex_home_rate_limit_used_percent` to
tell a subscription-limit outage (windows genuinely exhausted — wait for the reset or add a home)
from an authentication outage (`claude_api_codex_home_authenticated == 0` — re-run the device flow).
Never bypass cooling: hammering a limited or rejected ChatGPT profile is a ban signal.

Capacity planning reads `claude_api_codex_window_remaining_usd{window_minutes}` against
`claude_api_codex_window_capacity_usd{window_minutes}`. They exist only after real utilisation
movement has produced an estimate; `claude_api_codex_window_measured_homes` versus
`..._observed_homes` shows how many homes are still unknown. Per-home
`claude_api_codex_home_window_used_ratio`/`..._used_fraction_units`,
`..._observed_spend_usd`/`..._observed_fraction_units`,
`..._estimate_available`, `..._confidence_ratio`, capacity and remaining low/high bounds, data
age and samples explain estimate quality and source. The source is `workload_blend`: Codex
consumption depends on model, context, reasoning and tools, so this is the API-dollar equivalent of
the workload actually served, not a fixed subscription nominal. The compatibility metric named
`..._confidence_ratio` is deterministic evidence quality, not probability. It multiplies sample
maturity, observed workload-envelope stability and fixed-point quantisation resolution. One
fine-grained but highly variable sample therefore cannot look certain merely because it crossed a
whole percentage point.

Canonical `/codex-subs` money fields are integer nanoUSD encoded as decimal strings
(`capacity_nano`, `remaining_nano`, low/high variants, `observed_spend_nano` and
`spend_nano_total`); rounded `*_usd` numbers remain presentation compatibility only. Provider
utilisation is canonical `10^-8` fraction units, not the rounded `used_percent` field. There is
no configured capacity prior, EMA or float-money path. Because provider utilisation can include
activity outside this gateway, investigate an unexpectedly wide workload envelope or divergence
between exact observed fraction and gateway spend; confidence cannot turn foreign usage into known
gateway cost.
An estimate intentionally remains unknown when a new percentage snapshot arrives before its
positive gateway settlement; it should appear as soon as settlement catches up, without waiting for
another percentage point. A cold anchor is unknown by itself; its first complete positive movement
with settled gateway spend publishes the first estimate immediately. A persistent unknown after
such a movement indicates a calibration writer or authority problem rather than a reason to impose
an admission limit. For rolling weekly windows, a material forward reset-at shift together with a
utilisation rollback starts the new calibration window even when the shift is below half of seven
days; small timestamp jitter without that joint rollover signal remains in the current window.

## CodexHomeUnauthenticated

That home's login expired or was revoked. Republish the profile through the authbot's device flow:
the new envelope replaces the stale one in the roster and the gateway picks it up on its next
health tick — no restart, and Claude remains online throughout. Never copy, print or archive an
auth store or an unsealed token, and never point two profiles at one account.

## CodexHomeNearRateLimit

This is an early capacity-planning warning, not the provider's hard admission threshold. The
gateway normally steers away at its jittered soft reserve. Under peak it may relax that reserve:
live verification showed that `usedPercent=100` with `allowed:true` can still serve, so a numeric
100 alone does not hard-exclude the home. Only an explicit provider reached verdict or returned
usage-limit response does; then `claude_api_codex_home_limit_reached=1`, the panel shows
«лимит достигнут» ("limit reached"), and customers receive `429 + Retry-After` for the reset. Confirm other homes
can absorb load; add an authenticated home if measured remaining API-dollar capacity is
insufficient.

## CodexHomeRateLimited

The provider returned an explicit limit verdict (`limit_reached`/`allowed: false`), so the
gateway took the home out of rotation until that window resets; the panel shows «лимит достигнут»
("limit reached") and `/codex-subs` reports `limit_reached`. This is expected, not a fault. A window merely reading
100% does NOT fire this alert — the provider can report full while still serving, so the gateway
only acts on the provider's own verdict. Check that the remaining homes cover
the load (`claude_api_codex_homes_available`, the panel's remaining API-dollar column) and add an
authenticated home if they cannot. Do not restart the process or reauthenticate — the background
health probe refreshes the snapshot and the home returns by itself after the reset. If the alert
stays on past the reported reset, verify that the `/wham/usage` probe still answers for that home:
recovery depends on that probe succeeding, so a home whose probe is timing out cannot return on its
own. `CodexHomeSnapshotStale` distinguishes the two cases.

## ConversationsForcedOffTheirHome

A conversation that is pinned to a subscription is being moved to another one. This is not an error
path — the request is served — but it is the most expensive routing event we have: the warm prompt
prefix on the old subscription is thrown away, the customer waits longer, and the same prefix is paid
for again on the new one. It therefore surfaces as latency and cost long before it surfaces as a
failure, which is why it is measured separately from ordinary placement rather than folded into it.

`claude_api_route_rebind_total` is a subset of `claude_api_route_place_total`; the remainder is
genuinely new conversations. A sustained high ratio almost always means capacity: subscriptions are
reaching their windows or cooling long enough to cross the rebind threshold, so their conversations
cannot stay where their cache is. Check pool capacity and window utilisation before suspecting the
routing itself. A brief spike right after a deploy or a subscription being retired is expected.

## CodexHomeUnresponsive

The home's native transport is missing request deadlines (connect, first byte, or mid-stream
silence). The gateway has closed admission for new turns while leaving in-flight turns to finish.

This is the failure this alert exists for: a deadline is not an authentication error and not a quota
error, so a home in this state can otherwise look perfectly healthy — its credential opens, its
account is authenticated, and its last quota snapshot still reports whatever it reported before it
went quiet. Read `claude_api_codex_home_transport_degraded` and `..._transport_wedged` to tell
"missing deadlines" from "corroborated unusable", and `claude_api_codex_home_snapshot_age_seconds` to
confirm the refresh path stopped with it.

A wedged transport is rebuilt automatically (the per-profile client and its pooled connections are
recreated; there is no child to reap), and the home returns as soon as one probe or turn succeeds.
If the alert persists past a few sweeps, the account's egress is the suspect: verify the profile's
proxy from the sealed credential before suspecting the upstream.

## CodexHomeSnapshotStale

Quota evidence for this home has stopped being refreshed. Every quota gauge keeps publishing its last
observed value, so the numbers can look entirely healthy while the path that produces them is broken;
this alert is the only signal that separates "the quota is 32%" from "the quota was 32% an hour ago".

Selection already accounts for it — stale evidence never rejects a home and never wins a tie against
current evidence — so this is a diagnosis alert rather than an outage. It usually fires alongside
`CodexHomeUnresponsive`, since the probe that refreshes the snapshot is the same probe that is timing
out. If it fires alone, `/wham/usage` is failing for that home while generation still answers; the
home keeps serving on unknown quota until the endpoint recovers.

## CodexAccountDead

Authentication failed on repeated clean probes spread over at least five minutes, so this is a
revoked, banned or expired subscription and not a transient provider blip. The home is out of
rotation and pool capacity is permanently reduced until an operator acts.

Confirm the remaining homes cover the load, then re-authenticate or replace the subscription exactly
as in **CodexHomeUnauthenticated**. Never bypass the quarantine: repeatedly hammering a rejected
ChatGPT profile is itself a ban signal. The verdict clears automatically the moment one probe
succeeds, so a repaired login needs no manual reset.

## CodexCalibrationPersistenceFailed

The home could not write cumulative spend or a duration/reset observation to the engine authority.
Traffic intentionally remains fail-open, and failed spend credits stay pending for retry, but do not
trust the estimate to survive a restart while this gauge is zero. Check PostgreSQL availability,
schema version 10, writer logs and owner health. Do not seed a manual capacity value: restore the
authority and let the next real observations reconcile durable spend and calibration state.

## GeminiProviderDown

Only `gemini.api.apitoken.sale` is affected; do not restart healthy Claude or OpenAI processes.
Check both `claude-api-gemini@8795.service` and `claude-api-gemini@8799.service`, direct readiness
on 8795/8799, stable readiness on 8794, and both unit journals. Exactly one slot must be active,
ready, enabled, and release-selected in steady state; the other must be stopped and disabled.
Verify that both credential keyrings match, the roster is
readable, every envelope has the exact `credentials/<profile-id>.json` non-symlink 0600 path, and at
least one profile passes Antigravity `loadCodeAssist` health. Never decrypt or print an envelope while testing. Repair according to
`docs/engine/GEMINI_PROVIDER.md` and use the health-gated engine controller to restart the service.

If the surface must be withdrawn during investigation, stop the selected Gemini slot through the
normal provider rollout. A manual stop is only an immediate temporary action because watchdog
reconciliation restores the configured topology. Neither action should change an established provider.

## GeminiNoAvailableProfiles

Every paid subscription profile is cooling. Inspect `claude_api_gemini_profile_cooling_until_seconds` and
`claude_api_gemini_soonest_ready_seconds`, then correlate upstream `429`, auth, and 5xx counters.
Respect `Retry-After`/`google.rpc.RetryInfo`; bypassing cooling amplifies the outage. If capacity is
genuinely exhausted, wait for Google's subscription quota window or authorize another distinct paid
Google subject. The same subject under another project/file is rejected at startup.

## GeminiProfileUnauthenticated

The labeled profile received Google's explicit `400 invalid_grant` refresh verdict, which is the
only refresh response that marks a Gemini credential revoked. A `401`, `403`, another `400`, 5xx or
transport failure leaves the credential authenticated and puts it into bounded cooling instead;
those responses often identify proxy reputation, provider policy or a transient control-plane
failure and must not trigger destructive re-authorization.

Check the bounded `token refresh rejected` journal class without putting tokens, subject, email,
project or proxy in argv/logs/URLs. For `invalid_grant`, re-authorize the exact Google subject
through Auth Bot; its fresh OAuth material replaces the same profile in place. Never hand-edit an
envelope or restart the Gemini slot merely to reload it. If authentication and quota probes are
healthy but exact-profile generation alone fails, use the transactional proxy replacement and
rollback procedure in `docs/engine/GEMINI_PROVIDER.md` before replacing the subscription.

## GeminiUpstreamRateLimited

Use the per-profile cooling gauges to identify the constrained opaque profile, then wait for the
advertised reset or add a distinct authorized paid subject. Do not shorten cooling or duplicate one
account. A sustained increase without exhausted profiles can indicate disproportionate affinity or
load; compare profile in-flight gauges before changing capacity.

## KimiNoLiveProfiles

The KIMI plane is enabled but no roster profile is authenticated, so exact KIMI aliases fail
closed while Claude keeps its existing path. KIMI is default-off: this alert stays silent until
the first enable, and `CLAUDE_API_KIMI_ENABLED=false` keeps the plane dark at any time.

Safely diagnose: read `GET /kimi-subs` with the control key (per-profile `live` and
`cooling.auth_until`), then the unit journal of the Anthropic runtime for the bounded KIMI
failure classes. Check that the roster directory still parses, every envelope decrypts under the
configured keyring, and `/me` answers for at least one profile. Never decrypt or print an
envelope, and never put tokens, subjects or proxy values into logs or argv. Do NOT restart the
healthy Claude path, widen admission to unreviewed models, or hammer a quarantined profile —
repeated provider refusals are themselves a ban signal; the 5-minute auth quarantine clears on
its own once the credential is repaired through Auth Bot.

## KimiNoAvailableProfiles

Live KIMI profiles exist but every one of them is auth-quarantined, transport-cooled or
quota-walled, so selection has nothing eligible and KIMI requests fail closed. KIMI is
default-off: this alert stays silent until the first enable, and `CLAUDE_API_KIMI_ENABLED=false`
keeps the plane dark at any time.

Safely diagnose: `GET /kimi-subs` shows which cooling axis holds each profile
(`cooling.auth_until` / `transport_until` / `quota_until`), and the aggregate gauges
`claude_api_kimi_auth_quarantined_profiles`, `claude_api_kimi_transport_cooling_profiles` and
`claude_api_kimi_quota_cooling_profiles` say which axis fired fleet-wide. Quota walls clear at
the exact provider `resets_at`; transport cools clear in seconds once the egress path recovers.
Do NOT shorten cooling, route KIMI aliases into the Claude pool, or add an unreviewed plan label
to `KIMI_REVIEWED_PLANS` without a dated live observation. If capacity is genuinely exhausted,
wait for the provider window or authorize another distinct subscription through Auth Bot.

## KimiCalibrationPersistenceFailed

The KIMI turn FIFO could not persist spend or quota evidence to PostgreSQL. Traffic intentionally
remains fail-open, but measured window capacity may not survive a restart while this gauge is
zero. KIMI is default-off: this alert stays silent until the first enable, and
`CLAUDE_API_KIMI_ENABLED=false` keeps the plane dark at any time.

Check PostgreSQL availability, that migration `0027_kimi_window_calibration.sql` is applied, the
billing writer logs and the owner fencing. Do not seed manual capacity numbers: restore the
authority and let the next real observations reconcile durable spend and calibration state. Do
not delete the pending FIFO head from outside — exact replay is idempotent and the retained head
drains by itself.

## KimiCalibrationBacklog

The bounded KIMI turn FIFO has held pending events for ten minutes, and the provider `/usages`
read is suspended by design until the head drains — quota freshness will degrade next, so this
alert usually precedes **KimiQuotaStale**. KIMI is default-off: this alert stays silent until the
first enable, and `CLAUDE_API_KIMI_ENABLED=false` keeps the plane dark at any time.

This almost always shares a root cause with **KimiCalibrationPersistenceFailed**: diagnose the
PostgreSQL authority first. A permanently conflicting head (same request id, different payload)
is quarantined as poisoned and dropped — check `claude_api_kimi_calibration_dropped_events_total`
and the journal for the exact class; never hand-edit durable rows to unblock the queue.

## KimiQuotaStale

The newest KIMI `/usages` observation is older than three default poll intervals (3 × 300 s =
900 s), so every quota reading reports a frozen value: the fleet can look perfectly healthy while
the refresh path is broken. KIMI is default-off: this alert stays silent until the first enable,
and `CLAUDE_API_KIMI_ENABLED=false` keeps the plane dark at any time.

Safely diagnose: a pending calibration FIFO blocks the quota read by contract — check
**KimiCalibrationBacklog** first. Otherwise inspect the maintenance loop journal: profiles with
in-flight customer turns skip polling legitimately, and an auth-quarantined or transport-cooled
fleet (see **KimiNoAvailableProfiles**) stops observing too. Do not treat the last published
fraction as current when selling capacity, and do not restart the whole Anthropic runtime for a
KIMI-only stall; the loop resumes on its own once the blocking condition clears.

## GlmNoLiveProfiles

The GLM plane is enabled but no roster profile is authenticated, so exact GLM aliases fail
closed while Claude keeps its existing path. GLM is default-off: this alert stays silent until
the first enable, and `CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any time.

Safely diagnose: read `GET /glm-subs` with the control key (per-profile `live`, `account_dead`
and `account_suspect`), then the unit journal of the Anthropic runtime for the bounded GLM
failure classes. Check that the roster directory still parses, every envelope decrypts under the
configured keyring, and the free quota probe answers for at least one profile on its own console
origin. Never decrypt or print an envelope, and never put API keys, subject digests or proxy
values into logs or argv. Do NOT restart the healthy Claude path, widen admission to unreviewed
models, or hammer a dead profile — repeated provider refusals are themselves a risk-control
signal; a durably refused key is replaced through the Auth Bot, not retried.

## GlmNoAvailableProfiles

Live GLM profiles exist but every one of them is account-dead, account-suspect,
transport-cooled or quota-walled, so selection has nothing eligible and GLM requests fail
closed. GLM is default-off: this alert stays silent until the first enable, and
`CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any time.

Safely diagnose: `GET /glm-subs` shows which axis holds each profile (`account_dead` /
`account_suspect` / `cooling.transport_until` / `cooling.quota_until`), and the aggregate gauges
`claude_api_glm_account_dead_profiles`, `claude_api_glm_account_suspect_profiles`,
`claude_api_glm_transport_cooling_profiles` and `claude_api_glm_quota_cooling_profiles` say
which axis fired fleet-wide. Quota walls clear at the exact provider `resets_at`; transport
cools clear in seconds once the egress path recovers; a suspect flag clears on the next passing
quota probe; a dead key does not clear on any timer — see **GlmAccountDead**. Do NOT shorten
cooling, route GLM aliases into the Claude pool, or mark a suspect profile healthy by hand. If
capacity is genuinely exhausted, wait for the provider window or authorize another distinct
subscription through the Auth Bot.

## GlmCalibrationPersistenceFailed

The GLM turn FIFO could not persist dual-ledger spend or quota evidence to PostgreSQL. Traffic
intentionally remains fail-open, but measured window capacity may not survive a restart while
this gauge is zero. GLM is default-off: this alert stays silent until the first enable, and
`CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any time.

Check PostgreSQL availability, that migration `0029_glm_window_calibration.sql` is applied, the
billing writer logs and the owner fencing. Do not seed manual capacity numbers: restore the
authority and let the next real observations reconcile durable spend and calibration state. Do
not delete the pending FIFO head from outside — exact replay is idempotent and the retained head
drains by itself.

## GlmCalibrationBacklog

The bounded GLM turn FIFO has held pending events for ten minutes, and the provider quota read
is suspended by design until the head drains — quota freshness will degrade next, so this alert
usually precedes **GlmQuotaStale**. GLM is default-off: this alert stays silent until the first
enable, and `CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any time.

This almost always shares a root cause with **GlmCalibrationPersistenceFailed**: diagnose the
PostgreSQL authority first. A permanently conflicting head (same request id, different payload)
is quarantined as poisoned and dropped — check
`claude_api_glm_calibration_dropped_events_total` and the journal for the exact class; never
hand-edit durable rows to unblock the queue.

## GlmQuotaStale

The newest GLM quota observation is older than three default poll intervals (3 × 300 s =
900 s), so every quota reading reports a frozen value: the fleet can look perfectly healthy while
the refresh path is broken. GLM is default-off: this alert stays silent until the first enable,
and `CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any time.

Safely diagnose: a pending calibration FIFO blocks the quota read by contract — check
**GlmCalibrationBacklog** first. Otherwise inspect the maintenance loop journal: profiles with
in-flight customer turns skip polling legitimately, and a dead, suspect or transport-cooled
fleet (see **GlmNoAvailableProfiles**) stops observing too. Do not treat the last published
fraction as current when selling capacity, and do not restart the whole Anthropic runtime for a
GLM-only stall; the loop resumes on its own once the blocking condition clears.

## GlmAccountDead

A GLM subscription key was durably refused by the provider (invalid key or expired plan): the
profile is out of rotation with no timer to clear it — only fresh auth evidence (a replacement
key published through the Auth Bot) revives it. Other profiles may still serve, so this is an
early per-subscription signal rather than a fleet outage. GLM is default-off: this alert stays
silent until the first enable, and `CLAUDE_API_GLM_ENABLED=false` keeps the plane dark at any
time.

Safely diagnose: `GET /glm-subs` shows which opaque profile ids carry `account_dead`, and
`claude_api_glm_account_dead_profiles` counts them fleet-wide. Confirm the refusal class in the
unit journal (business code wins over the HTTP class), then rotate the key through the Auth Bot
seller path; the roster reload admits the replacement only after a passing quota probe. Do NOT
delete the profile from the roster to silence the alert, retry the refused key in a loop, or
lower the plane's admission set; a plan that lapsed needs a paid renewal, not engineering.

## DurableQueueBacklog

Check the owning worker/service, its database lock/lease fields, retry schedule, and downstream
dependency. A growing queue with a live process usually means dependency or poison-job failure.

For the pricing control queues (`engine_catalog_jobs`, `engine_switch_jobs`,
`pricing_release_control_jobs_v2`, `pricing_funding_normalizations_v2`,
`pricing_shadow_policy_jobs_v2`, `pricing_shadow_rollouts_v2`,
`pricing_stage8_capture_jobs_v2`) the owner is the `apitoken-worker` unit (pricing worker,
funding normalization worker, shadow rollout worker, and Stage 8 capture worker; check
`node_systemd_unit_state` and the unit journal). These are control-plane lanes, not customer
traffic: a sustained backlog means a release rollout, activation, or evidence capture is not
draining. The `/pricing` admin control room shows the same queue counts via the bounded
`GET /admin/pricing-release-activation-v2` and `GET /admin/pricing-stage8-capture-v2` snapshots.

## DurableQueueOldestItemStale

Inspect the oldest row without editing it. Confirm lock expiry and worker heartbeat, then use the
application’s idempotent retry path.

On the pricing control queues a stale oldest item usually means the owning `apitoken-worker`
lane is down, crash-looping, or every claim fails and re-arms `next_attempt_at`. All v2 queues
use lease recovery plus `FOR UPDATE SKIP LOCKED` claiming, so once the cause is fixed the worker
re-claims the row on its own — never update `status` or lock fields by hand.

## DurableQueueDeadItems

Review the terminal error and associated audit/payment event. Resolve the cause before manually
requeueing; all money mutations must remain idempotent. Intentionally obsolete commerce email jobs
use the separate `canceled` state and `apitoken_queue_canceled` metric. Never relabel a genuine
delivery failure as canceled merely to silence this alert.

On the release-v2 pricing control queues the dead gauge maps the terminal failure state of each
state machine: `dead` for `pricing_release_control_jobs_v2` and
`pricing_stage8_capture_jobs_v2`, `blocked` or `dead` for `pricing_shadow_policy_jobs_v2` and
`pricing_shadow_rollouts_v2` (`blocked` is a permanent delivery failure there), and `blocker`
for `pricing_funding_normalizations_v2` (the engine reported a normalization blocker; the release
cannot pass Stage 8 evidence while any account is blocked). Read `last_error` (and the `blockers`
JSON for funding normalizations) on the row, fix the cause, and re-drive the work through the
application's own idempotent paths — never by editing `status` directly. A dead activation or
capture row blocks the release; the `/pricing` admin control room shows the same terminal states
with their digests. Note the deliberate exception: a Stage 8 capture ending `blocked` (evidence
gate did not pass) is terminal evidence, not a lane failure, and does not raise this alert.

## PricingPolicyDeliveryStale

A new customer's `account_policy_bindings` row has waited unconfirmed for over two minutes.
Expected causes, in order: the `apitoken-worker` unit is down or crash-looping (check
`node_systemd_unit_state` and the unit journal for `pricing worker` lines), the LISTEN/NOTIFY
connection and the periodic sweep are both unhealthy (the journal shows repeated
`pricing-control notify listener reconnecting` warnings), or the engine Control API rejects
deliveries (`pricing-control job … failed permanently` in the same journal). Do not update the
binding row by hand: once the cause is fixed the worker re-claims the durable job on its own
(lease recovery plus `FOR UPDATE SKIP LOCKED` claiming make replay idempotent). Confirm recovery
by watching `apitoken_pricing_policy_oldest_pending_seconds` return to single digits and the
binding reach `sync_state = 'confirmed'`.

## PricingPolicyDeliveryFailed

The engine rejected a policy activation permanently (`sync_state = 'failed'`). Read
`last_error` on the binding row and the corresponding `engine_policy_jobs.last_error` first.
Typical causes are an invalid desired policy payload or a superseded desired version. Fix the
cause, then re-drive the desired policy through the application's own idempotent write path
(`packages/db` pricing policy write), never by editing `sync_state` directly.

## PricingChargeMismatch

A settled ledger row billed an amount that disagrees with the `payable_multiplier_bp` recorded on
that same row, by more than the one basis point of tolerance integer rounding needs. Either a
customer was overcharged or revenue was lost, so treat it as a money defect rather than a
reporting one.

Find the rows and read what they claim:

```sql
SELECT to_timestamp(ts), account_class, payable_multiplier_bp,
       round(amount_nano::numeric / official_nano * 10000) AS charged_bp,
       amount_nano, official_nano, rule_id, policy_id, policy_version
FROM ledger
WHERE ts > EXTRACT(EPOCH FROM now())::bigint - 3600
  AND official_nano > 0 AND amount_nano > 0
  AND ABS(amount_nano::numeric / official_nano * 10000 - payable_multiplier_bp) > 1
ORDER BY ts DESC;
```

The row carries the whole decision — rule, policy version, catalog and switch generations — so the
question is which of them disagreed with the charge, not which request it was. A cluster starting at
one timestamp points at a pricing change; scattered rows on sub-cent charges are rounding and belong
in the tolerance instead.

Never edit `ledger`: it is the money record. Correct a customer through an adjustment, and fix the
rule that produced the charge.

This alert replaced the shadow evaluation lane. That lane compared new pricing against the live one
ahead of a rollout, wrote 2654 rows nothing ever read, and was not running during the cutover it
existed to protect. This one reads settled money on a rolling hour, so it cannot be skipped — and if
the series stops appearing at all, `MonitoringTargetDown` covers the collector.

## FailedWebhooksPresent

Verify provider signature/audit data and whether the event’s intended payment mutation committed.
Replay only through the verified webhook workflow.

## StaleCheckoutSessions

Compare provider state, reconciliation-worker logs, and webhook arrival. Do not mark a checkout paid
without authoritative provider verification.

## EngineSettlementBacklog

Protect money integrity first. Inspect settlement outbox attempts, reservation ownership, and the
active single-writer engine topology before retrying.

## EngineExpiredLeasePresent

Confirm the owning engine instance is dead/fenced and let the authority reconciliation workflow
recover the lease. Never delete live reservations directly.

## BalanceDivergenceDetected

Stop manual credits, refunds, and destructive maintenance until the affected authority state is
understood. The gauge is the maximum absolute per-account difference between durable
`topup`/`adjust` funding and `balance_nano + spent_nano + reserved_nano`; any non-zero value violates
money conservation.

Take validated `commerce` and `claude_engine` dumps before investigating. Compare the account's
funding ledger, completed charge total, and non-terminal reservation holds without editing rows.
Correlate every top-up or adjustment reference with its commerce payment, bonus, promo, refund, or
admin audit record. Repair only through an idempotent application workflow and keep the original
evidence; never silence the alert by directly changing account aggregates or deleting ledger rows.

## BackupStale

Run `claude-api-backup.service`, check its result, then validate each dump with `pg_restore
--file=/dev/null` using the production PostgreSQL container toolchain.

## BackupMissing

Treat any missing dump for the four named databases as critical. Run the backup service and verify
ownership, mode 0600, archive readability, size, and timestamp.

## DeployQuarantined

A commit failed production delivery and is blocked from automatic retry. Read the failing stage with
`sudo apitoken-watchdog status` and `sudo apitoken-watchdog logs`; the quarantined SHA is in
`rejected.sha`. Production is unchanged for whichever stages did not run — a failed test never
deploys, a failed commerce migration never starts the backend, and a failed engine migration or
readiness check never receives traffic.

Fix the cause with a **new commit**. Do not retry the same SHA to repair a code, test, or migration
failure. `sudo apitoken-watchdog retry <sha>` is only for a failure proven transient, and never
permission to edit the immutable candidate or the production database by hand.

This alert covers every abnormal termination of the pipeline, including validation failures raised
by `wd_die`, which terminate with `exit` rather than a failing command. The failure handler is
registered on `EXIT` as well as `ERR` so both routes quarantine and report identically; a
successful cycle exits zero and is exempt.

## DeployPipelineStale

The watchdog writes its status file every cycle, roughly once a minute, so an age above fifteen
minutes means delivery has stopped. Check `apitoken-deploy-watchdog.timer` is active and the service
is not stuck: a cycle holds an exclusive `flock`, so one hung run blocks all later polls. Inspect the
current phase before intervening — a long `testing` or `deploying-*` phase is a running deployment,
not a stall, and killing it mid-cutover is far worse than waiting.

## DeployStuckInPhase

Delivery has stayed in one non-idle phase for 45 minutes, which exceeds the unit's own
`TimeoutStartSec`. Identify the phase from the alert label, then inspect that stage's work: a hung
build or test as `apitoken-ci`, a migration waiting on a PostgreSQL lock, or a blue-green controller
waiting on a readiness probe that never passes. Prefer letting systemd enforce its timeout over
killing the unit, and never restart an application slot manually between release selection and
cutover.

## DeployMigrationUncommitted

The `pending-migration.sha` marker survived a full cycle, meaning a migration began but its manifest
was never committed. Treat the commerce schema as possibly ahead of the recorded manifest. Do not
deploy, retry, or hand-edit the database. Compare the applied Drizzle journal against
`database-migrations.manifest`, and recover from the pre-deployment dump preserved for that SHA under
`/var/lib/apitoken/backups` if the schema is genuinely inconsistent.

## SalesReferralReconciliationBacklog

Check attribution-feed cursors and sales reconciliation logs. Buffered events are deliberately
durable; fix feed ordering/visibility before replay.

## SalesPayoutBatchFailed

Freeze further sends, inspect chain simulation/broadcast state and nonces, then follow the payout
service’s idempotent recovery path. Never rebroadcast blindly.


## DevBotHeartbeatMissing

The dev Telegram bot (`apitoken-devbot.service`, `apps/devbot`) is active in systemd but its
textfile heartbeat (`devbot_heartbeat_timestamp_seconds{job="node"}` in
`/var/lib/apitoken/monitoring/textfile/devbot.prom`, rewritten every 60 seconds) is absent or
older than five minutes. Alertmanager email keeps flowing regardless — this alert means the
bot process itself is not publishing liveness. Delivery-level failures are covered separately:
`DevBotTelegramSendFailures` (bot → Telegram), `DevBotWebhookDeliveryFailing` and
`DevBotWebhookSilent` (Alertmanager → bot), `DevBotMetricsDown` (scrape health).

First distinguish the two normal states from the broken one:

- **Bot intentionally disabled** — no `/etc/apitoken/devbot.env` on the host. The unit has
  `ConditionPathExists=/etc/apitoken/devbot.env` and stays cleanly inactive, so this alert does
  not fire. Nothing to do.
- **Bot enabled but heartbeat broken** — this alert. Check `systemctl status
  apitoken-devbot.service` and `journalctl -u apitoken-devbot.service -n 100`: the process may be
  up (unit reads `active`) while the heartbeat writer is failing — usually a permissions problem
  on `/var/lib/apitoken/monitoring/textfile` (must be writable by group `deploy`), a full disk,
  or a crash between restarts. Confirm the unit runs from the expected release:
  `readlink -f /opt/apitoken/devbot-releases/current`. Two concrete signatures and their causes:
  - bot journal shows `EACCES` on `devbot.prom.tmp-*` → the directory lost its
    `root:deploy 0775` ownership; both `install-monitoring.sh` and the minutely
    `collect-monitoring-metrics.sh` must keep it group-deploy writable (a collector regression
    here re-roots the directory every minute — pinned by `deploy/monitoring-config.test.sh`);
  - node-exporter logs `permission denied` opening `devbot.prom` → the file itself is not
    world-readable; the bot must publish it 0644 (its unit runs `UMask=0077`, and node-exporter
    scrapes as `nobody`).

To provision or repair the bot from scratch:

1. Place `/etc/apitoken/devbot.env` (mode 0600, owner `deploy`) with at least
   `DEVBOT_TELEGRAM_TOKEN`, `DEVBOT_CHAT_ID`, `DEVBOT_ADMIN_IDS`, and `DEVBOT_AM_SECRET` — see
   `docs/ops/DEVBOT.md` §7. Keep `DEVBOT_AM_SECRET` identical to the value the monitoring
   installer renders into the Alertmanager webhook path.
2. Roll the release with the watchdog lane (any `apps/devbot/**` change, or wait for the next
   master commit — until the env file exists the lane deliberately skips without advancing
   `devbot.sha`, so the first real deploy happens automatically once provisioning lands), or run
   it directly: `sudo /usr/local/lib/apitoken-watchdog/controller/devbot-deploy.sh
   $(git -C /opt/apitoken/repo rev-parse origin/master)`.
3. `sudo systemctl start apitoken-devbot.service`, then verify
   `curl -s http://127.0.0.1:3800/health` returns `{"ok":true}` and that
   `devbot_heartbeat_timestamp_seconds` appears in `http://127.0.0.1:9100/metrics` within two
   minutes. Reinstall monitoring (`deploy/install-monitoring.sh` runs via the watchdog
   infrastructure lane) so Alertmanager picks up `DEVBOT_AM_SECRET` for the webhook receiver.

## DevBotTelegramSendFailures

The bot process is up and its heartbeat is fresh, but Telegram send/edit attempts are being
dropped after retries (`devbot_telegram_send_failures_total` increased in the last fifteen
minutes, scraped from the bot's own `/metrics` at `127.0.0.1:3800`, job `devbot`). The process
and heartbeat look healthy while nothing reaches the group — this is exactly the failure the
heartbeat cannot see.

Check the unit journal for the send-error lines (`telegram ...: 429` / `network error
(attempt ...)`), then in order: the bot token against @BotFather (`DEVBOT_TELEGRAM_TOKEN` in
`/etc/apitoken/devbot.env`), the bot's membership and admin rights in the forum group
(`DEVBOT_CHAT_ID`), and outbound reachability of `api.telegram.org` from the host. A revoked
token or a kicked bot is a provisioning fix, not a restart. A persistent 429 flood means the
outbound queue is exceeding Telegram's group limits — coalescing handles the excess, but the
group's message history may show gaps while it drops.

## DevBotWebhookDeliveryFailing

Alertmanager tried to POST a notification to the devbot webhook and failed
(`alertmanager_notifications_failed_total{integration="webhook"}` increased in the
last fifteen minutes; the pinned Alertmanager v0.32.1 keeps the dedicated failed counter). Email keeps flowing — the Telegram fan-out specifically is degraded.

First check the bot itself: `systemctl status apitoken-devbot.service` and
`curl -fsS http://127.0.0.1:3800/health`. A dead bot is the most common cause (see
`DevBotHeartbeatMissing` / `ProjectSystemdUnitFailed`). Then verify the webhook path secret
matches: the `DEVBOT_AM_SECRET` in `/etc/apitoken/devbot.env` must equal the value rendered
into `http://127.0.0.1:3800/alerts/{secret}` in the Alertmanager configuration; a drift makes
Alertmanager POST to a path the bot answers with 404. Reinstall monitoring
(`deploy/install-monitoring.sh` via the watchdog infrastructure lane) so the renderer reads the
current env file. Do not open port 3800 to the network to "fix" delivery.

## DevBotWebhookSilent

Slow-drift tripwire: the unit is active and Alertmanager has active alerts, but no valid
webhook delivery reached the bot for over 24 hours (`devbot_last_webhook_seconds` in the bot's
`/metrics` is older than a day). This fires when the fast signals cannot — typically the
devbot block was stripped from the rendered `alertmanager.yml` (the renderer omits it whenever
`DEVBOT_AM_SECRET` is absent from the env file), the path secret drifted, or the bot's intake
server stopped accepting while the process stayed up.

Verify: `curl -fsS http://127.0.0.1:9093/-/ready` (Alertmanager itself), compare
`devbot_last_webhook_seconds` against the current time, and confirm the rendered webhook URL
still contains the same secret as `/etc/apitoken/devbot.env`. Reinstall monitoring so
`DEVBOT_AM_SECRET` is rendered again, then confirm the next firing alert updates
`devbot_last_webhook_seconds`. This is a deliberate 24-hour signal, not an outage detector —
the immediate failures are covered by `DevBotWebhookDeliveryFailing` (Alertmanager side) and
`DevBotTelegramSendFailures` (bot side).

## DevBotMetricsDown

The scrape of the bot's `/metrics` at `127.0.0.1:3800` (job `devbot`) fails while the unit is
active, so `devbot_telegram_send_failures_total` and `devbot_last_webhook_seconds` are
unavailable and the alerts built on them go blind. The bot was deliberately excluded from the
generic `MonitoringTargetDown` because an unprovisioned bot has no listener on 3800; this alert
covers the active-but-unscrapable case instead.

Check `systemctl status apitoken-devbot.service` and `curl -fsS http://127.0.0.1:3800/metrics`:
the process may be up while the metrics server fails (port conflict, crash between restarts,
or the listener bound to the wrong interface). Confirm the scrape job targets
`127.0.0.1:3800` in `observability/prometheus/prometheus.yml` and that the watchdog
infrastructure lane reinstalled the monitoring definitions after the last change.
