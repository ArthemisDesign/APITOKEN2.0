# Monitoring exporter `up` vs blackbox scrape timeouts — 2026-08-24

Status: resolved

## Executive summary

SHA `7ee2930665e3c1c7ef4d187f2af62d8ee40a834e` split the monitoring operands. Probes and
collector passed. GitHub `deploy/watchdog` was RED with
`Prometheus scrape targets are not all up (MonitoringTargetDown job set)`. Engine admission
was GREEN. The host rolled back. Do not retry `7ee29306`. This commit stops treating blackbox
`*-http` `up` and staging veth as a production deploy gate, and lists the down exporter jobs
in the headline.

## Impact and detection

- Authoritative production engine stayed on the previous verified release after rollback.
  Loopback readiness stayed HTTP 200.
- `origin/master` moved to `7ee29306`. `deploy/engine` GREEN. `deploy/watchdog` RED at
  `phase=verifying` with the exporter-`up` wrapper (jobs unnamed).
- Observe after the cycle: Gemini `@8799` and KIMI `@8804` were still `deactivating`.
  Stable origins `8790/8792/8794/8803/8802` were HTTP 200.

`6ef38441` and `289993c3` failed the earlier combined query. `7ee29306` proved the remaining
operand is `up`, not `probe_success` or collector freshness.

## Root cause

`min(up{job!~"claude-router|devbot"})` includes every remaining Prometheus job: exporters
and blackbox probe jobs (`public-http`, `openai-http`, `gemini-http`, `protected-http`,
`support-http`, `openkeys-http`, `loopback-http`) and `staging-veth`.

For a blackbox job, `up` is "did this scrape complete". `probe_success` is "did the HTTP
check pass". A scrape timeout sets `up=0` and does not update `probe_success`, so the last
value stays 1. After engine cutover, one rotating timeout among ~20 probe targets makes
`min(up)` stay 0 for the whole 2-minute window while `min(probe_success)==1` still holds.
That is the observed split on `7ee29306`.

Staging `10.254.32.2:3901` is not a production serving origin. A down stage sink must not
quarantine a production SHA. `MonitoringTargetDown` still pages it.

## Why existing safeguards missed it

- `deploy/engine` proves serving/converged slots and `/ready`. It does not prove every
  blackbox scrape completed in the same instant.
- The previous SHA already split operands and waited 2 minutes. It still used the alert's
  full `up` selector, including probe jobs whose `up` is the wrong signal for a deploy gate.
- The headline named the operand but not the down `job`, so the next SHA could not exclude
  a proven culprit.

## Correction

Do not retry `7ee29306`. Land a newer descendant that:

1. checks exporter `up` with
   `job!~"claude-router|devbot|staging-veth|.*-http"`;
2. keeps HTTP health on `probe_success`;
3. on `up` failure, queries `up{...} == 0` and puts the unique job names in `wd_die`.

The Responses PNG mask from `6ef38441` remains in this descendant.

## Executable guardrail

- `deploy/watchdog.sh` `final_verify_monitoring` — exporter filter, down-job listing.
- `deploy/monitoring-config.test.sh` and `deploy/watchdog-lib.test.sh` grep the filter,
  the `up{...} == 0` query, and `Prometheus scrape targets down:`.

Command: `bash deploy/monitoring-config.test.sh && bash deploy/watchdog-lib.test.sh`.

## Remaining risk

- A real exporter that stays down for 2 minutes still fail-closes, now with its job name.
- Staging veth is no longer a production deploy gate. Diagnose a down stage sink from
  `MonitoringTargetDown`, not from `deploy/watchdog`.
- `openkeys-http` is a probe job and is therefore excluded from exporter `up`; its HTTP
  health still rides on `probe_success` only if that job is in the probe selector. It is
  not in the current probe selector (loopback-http covers OpenKeys on `:3410` instead).
