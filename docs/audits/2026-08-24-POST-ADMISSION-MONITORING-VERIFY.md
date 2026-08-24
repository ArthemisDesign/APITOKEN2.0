# Post-admission monitoring verify quarantined three SHAs — 2026-08-24

Status: resolved

GitHub issue: https://github.com/3xcalibur-tech/Claude_API/issues/3

This is the incident-level postmortem for the 2026-08-24 delivery of Responses PNG mask
plus the three RED `deploy/watchdog` cycles that followed engine admission. Per-SHA
snapshots remain append-only:

- [2026-08-24-FINAL-VERIFICATION-STATUS.md](2026-08-24-FINAL-VERIFICATION-STATUS.md) — `6ef38441` generic wrapper
- [2026-08-24-MONITORING-VERIFY-CUTOVER.md](2026-08-24-MONITORING-VERIFY-CUTOVER.md) — `289993c3` monitoring lane
- [2026-08-24-MONITORING-UP-JOB-SET.md](2026-08-24-MONITORING-UP-JOB-SET.md) — `7ee29306` exporter `up`

## Executive summary

Three attested `master` SHAs admitted the engine (`deploy/engine` GREEN) and then failed
post-admission `final_verify_monitoring`. The host rolled each candidate back. Public
loopback readiness stayed HTTP 200 on the previous verified engine
`be6c88b171cbaf27e4b9fb67c9cf1e2186a77b56`. Vercel Production independently published the
mask docs from `master` while the serving engine was still the rollback. The first RED
headline hid the inner lane. The second named monitoring. The third proved probes and
collector were healthy and `up` was not. GREEN `8e6561da8cbf7ba3c4b14e41c217104a9f45954e`
at `2026-08-24T14:14:30Z` now serves the mask and the corrected verification. Do not retry
`6ef38441`, `289993c3`, or `7ee29306`.

## Impact and detection

Measured:

- Authoritative engine after each rollback: `be6c88b1`. Stable origins `8790`, `8791`,
  `8792`, `8794`, `8803`, `8802` stayed HTTP 200.
- `origin/master` moved through `6ef38441` → `289993c3` → `7ee29306` (all quarantined) →
  `8e6561da` (GREEN, idle).
- `deploy/tests`, `deploy/stage`, `promotion/eligible`, and `deploy/engine` were GREEN on
  every quarantined SHA. `deploy/watchdog` was RED in `phase=verifying`.
- First detection: merge client wait on `6ef38441` at `2026-08-24T12:42:46Z`.
- Check run `deploy/watchdog-log` was missing (host PAT lacks Checks: write). Observe
  journal after each cycle was the 200-line quarantine poll, not the inner `wd_die`.
- Vercel Production already contained “Region inpaint” / `input_image_mask` after
  `6ef38441` reached `master`.

Estimates:

- Unverified candidate served only between engine GREEN and rollback on each cycle
  (minutes, not the full quarantine window).
- No customer-money, auth, or privacy defect was observed. The durable contract gap was
  storefront docs describing a mask path the rolled-back engine did not yet publish as
  this SHA’s fail-closed `file_id` behaviour.

## Timeline

UTC, 2026-08-24.

| Time | Event |
|---|---|
| 12:22:20 | `6ef38441` stage GREEN and `promotion/eligible` |
| 12:25:45 | trusted-host `deploy/tests` GREEN |
| 12:26:16 | Vercel Production deployment completed (mask docs) |
| 12:26:56 | production-engine deployment started |
| 12:31:21 | `deploy/engine` GREEN — candidate admitted |
| 12:31:21–12:42:46 | `CURRENT_PHASE=verifying`; subshell plan; rollback; outer wrapper `wd_die` |
| 12:42:46 | `deploy/watchdog` RED: `selected final production verification failed after component admission` |
| later | host `phase=quarantined sha=6ef38441`; engine `be6c88b1` |
| 13:24:17 | `289993c3` quarantined; headline: monitoring targets, synthetics, or collector |
| 13:52:44 | `7ee29306` quarantined; headline: Prometheus scrape targets are not all up |
| 14:14:30 | `8e6561da` idle: `master already processed; production runtime aligned`. `deploy/watchdog` GREEN |

Observe after the `7ee29306` rollback still showed Gemini `@8799` and KIMI `@8804`
`deactivating` (680s drain). Stable origins were HTTP 200. After GREEN `8e6561da`, new
slots were active (`anthropic@8787`, `openai@8797`, `gemini@8799`, `kimi@8804`,
`router@8800`).

## Root cause

Two stacked ownership mistakes. Rollback-before-fail-closed is required and stayed.

### 1. Post-admission wrapper hid the inner `wd_die`

`run_final_verification_plan` runs in a subshell so an inner `wd_die` does not skip
`rollback_engine`. The outer wrapper
`selected final production verification failed after component admission` was not
classified as generic. `wd_github_failure_description` therefore preferred
`WD_LAST_ERROR` over the cycle transcript. The transcript last-wins on
`[watchdog] ERROR:`, which was the join wrapper
`final verification lanes failed (panel=…)`, not the lane `wd_die`.

Decision point: `deploy/watchdog.sh` after the subshell, plus
`wd_error_is_generic` / `wd_failure_marker_line` in `deploy/watchdog-lib.sh`.

### 2. Deploy gate used the wrong Prometheus signal

`final_verify_monitoring` required `min(up) == 1` (later
`min(up{job!~"claude-router|devbot"}) == 1`) over every remaining job, including
blackbox `*-http` jobs and `staging-veth`.

For a blackbox job, `up` is “did this scrape complete”. `probe_success` is “did the HTTP
check pass”. A scrape timeout sets `up=0` and does not update `probe_success`, so the last
value stays 1. After engine cutover, one rotating timeout among ~20 probe targets keeps
`min(up)` at 0 for the whole retry window while `min(probe_success)==1` still holds.
`7ee29306` measured that split: probes and collector passed; `up` failed.

Staging `10.254.32.2:3901` is not a production serving origin. A down stage sink must not
quarantine a production SHA.

Decision point: `final_verify_monitoring` PromQL in `deploy/watchdog.sh`.

## Why existing safeguards missed it

- Stage, trusted-host tests, and engine admission prove tree, attestation, and serving
  slots. They do not own post-admission Prometheus scrapes.
- `wd_error_is_generic` already treated `lanes failed` and `failed (exit N)`. The
  post-admission sentence did not match.
- `deploy/watchdog-log` is fail-open. Missing Checks: write still quarantines and still
  writes the 140-character status, so the headline was the only public diagnostic.
- Grafana/Prometheus loopback health already retried 60s after the 2026-08-07 flake. Target
  aggregation used the same window and still used bare `min(up)`.
- Aligning the job filter with `MonitoringTargetDown` still included probe-job `up`, which
  is the wrong signal for a deploy gate (`7ee29306`).

## Correction

Do not retry `6ef38441`, `289993c3`, or `7ee29306`. GREEN `8e6561da` contains:

1. Post-admission wrappers classified as generic; cycle-transcript marker skips generic
   `[watchdog] ERROR:` lines so the inner lane `wd_die` wins.
2. Split operands: exporter `up`, HTTP `probe_success`, collector freshness; 24×5s window.
3. Exporter `up` filter
   `job!~"claude-router|devbot|staging-veth|.*-http"`.
4. On `up` failure, query `up{...} == 0` and put unique job names in `wd_die`.

The Responses PNG mask from `6ef38441` is on that GREEN SHA. HTTP Images `mask` stays
rejected. Responses `file_id` stays 400 (`documented_limitation`).

## Executable guardrail

- `deploy/watchdog-lib.sh` `wd_error_is_generic` — post-admission wrappers.
- `deploy/watchdog-lib.sh` `wd_failure_marker_line` — last specific operational marker.
- `deploy/watchdog-lib.test.sh` seeds an inner Codex `wd_die` plus both wrappers and
  expects the Codex line; same for the commerce cutover wrapper; greps the exporter
  filter and `Prometheus scrape targets down:`.
- `deploy/watchdog.sh` `final_verify_monitoring` — split queries, exporter filter,
  down-job listing.
- `deploy/monitoring-config.test.sh` greps the three queries, the 24-step loop, the
  exporter filter, and the absence of bare `min(up) == 1`.

Command:

```bash
bash deploy/watchdog-lib.test.sh && bash deploy/monitoring-config.test.sh
```

Those suites run in the local merge deployment lane and in trusted-host static
regression when selected.

Present-tense rules (already in owning docs, not duplicated into `AGENTS.md`):

- `docs/ops/DEPLOYMENT.md` — post-admission wrappers are generic; headline prefers the
  inner lane `wd_die`.
- `docs/ops/MONITORING.md` `#MonitoringTargetDown` — deploy gate uses exporter `up` plus
  `probe_success`, not blackbox `up`.

## Remaining risk

- Host PAT still lacks Checks: write. `deploy/watchdog-log` may stay unpublished. Diagnose
  from the 140-character headline first, then `ssh observe@` if the cycle is still in the
  last 200 journal lines.
- Staging veth is no longer a production deploy gate. Diagnose a down stage sink from
  `MonitoringTargetDown`, not from `deploy/watchdog`.
- A real exporter that stays down for 2 minutes still fail-closes, now with its job name.
- `openkeys-http` is a probe job and is excluded from exporter `up`. Public OpenKeys HTTP
  health is not in the current `probe_success` selector; loopback-http covers `:3410`.
- The inner lane that failed on `6ef38441` was not recovered from GitHub or observe. Treat
  it as the same monitoring `up` class only by later proof, not as a measured fact for that
  SHA.
