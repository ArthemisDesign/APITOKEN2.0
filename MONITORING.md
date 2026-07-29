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
- Engine: request totals, upstream 429/auth/5xx failures, breaker state, in-flight work, Claude/Codex/
  Gemini pool health, settlement backlog, and expired leases.
- Commerce: database health plus credit, adjustment, pricing, email, webhook, and checkout state.
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
  snapshot. It never includes the raw API key, email, key label, prompt/body, query string, or a
  client-controlled path ID. Internal retries are not events; only the status returned to the caller
  is recorded. Investigate directly with:

  ```bash
  journalctl -u 'claude-api-anthropic@*.service' -u claude-api-openai.service \
    -u claude-api-gemini.service \
    --since today --grep='"event":"customer_http_error"'
  ```

The Grafana and telemetry volumes are not business records. Dashboards and alert rules are
provisioned from Git, so telemetry can be rebuilt. The four PostgreSQL custom-format dumps remain
the authoritative hourly recovery artifacts and are validated again before database migrations.

## Failure-domain limitation

All monitoring was explicitly requested on the production server. Consequently, a total host,
power, network, or Docker failure also stops Alertmanager and cannot send an outage email. The
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

Delete or let the short-lived test alert resolve after confirming the email. Never put SMTP,
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

## EngineAffinityRedisErrors

Check `apitoken-affinity-redis.service`, its container health and disk space. Engine traffic is
intentionally fail-open on local affinity, so restore Redis without restarting healthy engine slots;
expect only a temporary reduction in cross-slot prompt-cache hits.

## CodexProviderDown

Only the OpenAI-compatible surface is affected; Claude routing is independent. Check
`systemctl status claude-api-openai.service` and its journal first, then check
`claude_api_codex_home_process_live` per home to see whether every child failed or only one.
Restarting a child is automatic and lazy, so a persistent zero means the binary attestation, the
`CODEX_HOME` permissions, or the pinned version no longer matches `docs/CODEX_APP_SERVER.md`. Do not
edit the pinned build on the host; correct it with a commit and let the watchdog roll the provider
cohort. If the journal reports that a home is already owned, do not delete the lock file: find and
stop the competing process through its systemd unit. Advisory ownership is attached to the open file
descriptor, not the directory entry.
`CLAUDE_API_CODEX_ENABLED=0` is the provider-only kill switch if the surface must be withdrawn while
the cause is investigated.

## CodexNoAvailableHomes

Every home is cooling or outside its window headroom, so clients are being told to retry. Read
`claude_api_codex_home_cooling_until_seconds` and `claude_api_codex_home_rate_limit_used_percent` to
tell a subscription-limit outage (windows genuinely exhausted — wait for the reset or add a home)
from an authentication outage (`claude_api_codex_home_authenticated == 0` — re-run the device flow).
Never bypass cooling: hammering a limited or rejected ChatGPT profile is a ban signal.

Capacity planning reads `claude_api_codex_window_remaining_usd{slot}` (pool sellable capacity left
in the current windows, in official-price USD) against `claude_api_codex_window_capacity_usd{slot}`;
`claude_api_codex_window_capacity_calibrated` tells whether those figures are measured or still the
configured prior (`CLAUDE_API_CODEX_WINDOW_CAP_USD`). Per-home splits carry the same
`claude_api_codex_home_window_*` gauges plus `claude_api_codex_home_spend_usd_total`. Note that a
home's subscription window is also consumed by usage outside the gateway (the account owner's own
Codex), which calibration deliberately excludes from the estimate.

## CodexHomeUnauthenticated

That home's device login expired or was revoked. New homes should be republished through the authbot's
hidden staging flow. For an in-place manual reauthentication, stop only
`claude-api-openai.service`, authenticate as the unprivileged engine user against that home's own
`CODEX_HOME`, then start the same unit and require 8792 readiness. Claude remains online throughout.
Never copy, print or archive an auth store, never replace `/run/apitoken/codex-home.lock`, and never
point two homes at one store.

## CodexHomeNearRateLimit

The home stops being admitted at `CLAUDE_API_CODEX_ADMIT_BELOW_USED_PERCENT` (95% by default), which
is expected behaviour rather than a fault. Confirm the remaining homes can absorb the load before the
window is reached; if they cannot, the pool needs another authenticated home rather than a lower
headroom.

## GeminiProviderDown

Only `gemini.api.apitoken.sale` is affected; do not restart healthy Claude or OpenAI processes.
Check `claude-api-gemini.service`, direct readiness on 8795, stable readiness on 8794, and the unit
journal. Verify that both credential keyrings match, the roster is
readable, every envelope has the exact `credentials/<profile-id>.json` non-symlink 0600 path, and at
least one profile passes `loadCodeAssist` health. Never decrypt or print an envelope while testing. Repair according to
`docs/GEMINI_PROVIDER.md` and use the health-gated engine controller to restart the service.

If the surface must be withdrawn during investigation, stop `claude-api-gemini.service` through the
normal provider rollout. A manual stop is only an immediate temporary action because watchdog
reconciliation restores the configured topology. Neither action should change an established provider.

## GeminiNoAvailableProfiles

Every paid subscription profile is cooling. Inspect `claude_api_gemini_profile_cooling_until_seconds` and
`claude_api_gemini_soonest_ready_seconds`, then correlate upstream `429`, auth, and 5xx counters.
Respect `Retry-After`/`google.rpc.RetryInfo`; bypassing cooling amplifies the outage. If capacity is
genuinely exhausted, wait for Google's subscription quota window or authorize another distinct paid
Google subject. The same subject under another project/file is rejected at startup.

## GeminiProfileUnauthenticated

The labeled profile returned repeated `401` after one single-flight refresh, or `403`. Check the
Google account entitlement and OAuth app status without putting tokens, subject, email, project or
proxy in argv/logs/URLs. Re-authorize through Auth Bot; never hand-edit an envelope. Keep the old
profile quarantined until the roster is safely repaired.

## GeminiUpstreamRateLimited

Use the per-profile cooling gauges to identify the constrained opaque profile, then wait for the
advertised reset or add a distinct authorized paid subject. Do not shorten cooling or duplicate one
account. A sustained increase without exhausted profiles can indicate disproportionate affinity or
load; compare profile in-flight gauges before changing capacity.

## DurableQueueBacklog

Check the owning worker/service, its database lock/lease fields, retry schedule, and downstream
dependency. A growing queue with a live process usually means dependency or poison-job failure.

## DurableQueueOldestItemStale

Inspect the oldest row without editing it. Confirm lock expiry and worker heartbeat, then use the
application’s idempotent retry path.

## DurableQueueDeadItems

Review the terminal error and associated audit/payment event. Resolve the cause before manually
requeueing; all money mutations must remain idempotent. Intentionally obsolete commerce email jobs
use the separate `canceled` state and `apitoken_queue_canceled` metric. Never relabel a genuine
delivery failure as canceled merely to silence this alert.

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
