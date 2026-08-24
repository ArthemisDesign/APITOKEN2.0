# Monitoring verify vs blue-green cutover — 2026-08-24

Status: resolved

## Executive summary

SHA `289993c38e93a788850be08965c44452389e0103` was the diagnostic follow-up to
`6ef38441116571d46c2ee95e4c903bed43687177`. The inner lane `wd_die` now reached GitHub:
`phase=verifying; [watchdog] ERROR: monitoring targets, synthetics, or business collector are not healthy`.
Engine admission was GREEN again. The host rolled back. Observe showed retiring OpenAI/Gemini/KIMI
slots still `deactivating` after the cycle. Do not retry `289993c3`. This commit aligns
`final_verify_monitoring` with `MonitoringTargetDown` (job filter + 2-minute window) and names
the failing operand.

## Impact and detection

- Authoritative production engine stayed on the previous verified release after rollback.
  Loopback readiness stayed HTTP 200.
- `origin/master` moved to `289993c3`. `deploy/engine` was GREEN. `deploy/watchdog` was RED
  with the inner monitoring wrapper (the 140-character headline now includes that `wd_die`).
- Observe after the cycle: `claude-api-openai@8797`, `claude-api-gemini@8799`, and
  `claude-api-kimi@8804` were `deactivating`. Stable origins `8790/8792/8794/8803/8802` were
  HTTP 200.

## Timeline

- `289993c3` staged, attested, and fast-forwarded `master` as the repair for quarantined
  `6ef38441` (`--fix-red`).
- Trusted-host tests passed. Controller install of the diagnostic `watchdog-lib.sh` ran
  before engine admission, so this cycle published the inner error.
- Engine admission GREEN, then `CURRENT_PHASE=verifying` failed the combined monitoring
  query after its 12×5s window.
- Rollback; later polls quarantine `289993c3`.

`6ef38441` failed the same combined query. Its inner lane was unknown until this SHA.

## Root cause

`final_verify_monitoring` required `min(up) == 1` over every Prometheus job for 60 seconds.
That is stricter than production:

- `MonitoringTargetDown` is `up{job!~"claude-router|devbot"} == 0` `for: 2m`.
- Devbot has no listener until provisioned. `claude-router` has `RouterMetricsDown`.
- Engine blue-green plus the 60s business-collector timer can leave a scrape or
  `apitoken_monitoring_collector_last_success_unixtime` sample stale for more than 60s
  while the serving origins are already ready.

The combined `and` query also hid which operand failed.

## Why existing safeguards missed it

- `deploy/engine` proves serving/converged slots. It does not wait for Prometheus to
  scrape the new process or for the next collector minute.
- Grafana/Prometheus loopback health already retried 60s after the 2026-08-07 flake.
  Target aggregation used the same 60s window and still used bare `min(up)`.
- `6ef38441` published only the outer verification wrapper, so the monitoring lane was
  not visible until `289993c3`.

## Correction

Do not retry `289993c3`. Land a newer descendant that:

1. queries scrape `up`, HTTP synthetics, and collector freshness separately;
2. filters `up` with the same `job!~"claude-router|devbot"` set as `MonitoringTargetDown`;
3. waits 24×5s (2 minutes), matching the alert `for:`.

The Responses PNG mask from `6ef38441` remains in this descendant.

## Executable guardrail

- `deploy/watchdog.sh` `final_verify_monitoring` — split queries, job filter, 24-step window.
- `deploy/monitoring-config.test.sh` greps those three queries, the 24-step loop, and the
  absence of bare `min(up) == 1`.

Command: `bash deploy/monitoring-config.test.sh`.

## Remaining risk

- Staging veth (`staging-veth`) stays in `min(up{...})` because `MonitoringTargetDown` still
  pages it. A down staging sink can still quarantine an otherwise healthy production SHA.
- A collector run that stays red for more than 2 minutes after engine admission is a real
  monitoring failure and must still fail closed.
