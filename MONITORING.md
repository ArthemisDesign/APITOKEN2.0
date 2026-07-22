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
- Engine: request totals, upstream 429/auth/5xx failures, breaker state, in-flight work, subscription
  pool size, cooling subscriptions, settlement backlog, and expired leases.
- Commerce: database health plus credit, adjustment, pricing, email, webhook, and checkout state.
- Sales: API/web health, email queue, referral reconciliation buffer, and payout-batch failures.
- CRM, Content Studio, public web, support, and mail: process/systemd health, HTTP probes, and logs.
- Deployments and recovery: watchdog/timer state and current backups for `commerce`,
  `claude_engine`, `sales`, and `apitoken_crm`.
- Logs: host journald in Loki for 14 days. Prometheus metrics are retained for 30 days or 24 GB,
  whichever limit is reached first.

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

## BackupStale

Run `claude-api-backup.service`, check its result, then validate each dump with `pg_restore
--file=/dev/null` using the production PostgreSQL container toolchain.

## BackupMissing

Treat any missing dump for the four named databases as critical. Run the backup service and verify
ownership, mode 0600, archive readability, size, and timestamp.

## SalesReferralReconciliationBacklog

Check attribution-feed cursors and sales reconciliation logs. Buffered events are deliberately
durable; fix feed ordering/visibility before replay.

## SalesPayoutBatchFailed

Freeze further sends, inspect chain simulation/broadcast state and nonces, then follow the payout
service’s idempotent recovery path. Never rebroadcast blindly.
