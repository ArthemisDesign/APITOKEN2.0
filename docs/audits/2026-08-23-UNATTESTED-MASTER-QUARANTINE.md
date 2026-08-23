# Unattested master SHA quarantine — 2026-08-23

Status: mitigated

## Executive summary

Production `deploy/watchdog` quarantined `66c7dd2f9342efadeade8768e91902ea03a7beea` after a
direct `master` fast-forward that skipped stage attestation. Live production stayed on GREEN
`4b8ba93831a52a8cf5af63a138bf676c02305675`. The SHA is blocked in `rejected.sha` and must not
be retried. This commit adds the merge-client gate and admission diagnostic that reject the
same class before GitHub `master` moves again.

## Impact and detection

- Authoritative production SHA remained `4b8ba938`. Readiness probes on the production host
  stayed HTTP 200.
- `origin/master` moved to `66c7dd2f`. GitHub `deploy/tests` was GREEN. `deploy/watchdog` was
  RED with `phase=fetching; exit 1; candidate quarantined`.
- Host status: `phase=quarantined sha=66c7dd2f… detail=failed candidate remains blocked`.
- Observe journal after the first cycle only showed the quarantine poll. Admission stderr was
  not in the 140-character GitHub description because `CURRENT_PHASE` was still `fetching` and
  the helper output was not captured into `WD_LAST_ERROR`.

## Timeline

- `2026-08-23T17:25:52Z` — `66c7dd2f` committed (admin iPhone table-card follow-up).
- Shortly after — merge client fast-forwarded `master` without GREEN `deploy/stage` for that SHA.
- Host selected the SHA, set `CANDIDATE_SHA`, then `promotion-admission.sh` rejected the
  unattested tree while `CURRENT_PHASE=fetching`.
- Later polls: SHA equals `rejected.sha`; watchdog waits for a newer commit or explicit retry.
- `origin/stage` remained `1301d939` (Phase 7 closeout docs, GREEN `stage/deployed`), not an
  ancestor of `66c7dd2f`, so the serial stage client was frozen.

## Root cause

Phase 7 production admission already rejected unattested master SHAs. The merge client still
fast-forwarded `master` from GREEN `deploy/tests` plus a GREEN parent `deploy/watchdog`. That
decision point is `deploy/agent-merge.sh` pushing `HEAD:master` without reading `deploy/stage`.

## Why existing safeguards missed it

- Trusted-host `deploy/tests` validates the candidate tree. It is not promotion admission.
- Parent `4b8ba938` `deploy/watchdog` was GREEN, so `--fix-red` was not required for the parent
  and the client treated `master` as a safe fast-forward base.
- `promotion-admission.sh` did fail closed on the host. The GitHub headline stayed generic
  because admission ran before `CURRENT_PHASE` left `fetching` and sudo stderr was discarded.

## Correction

Do not retry `66c7dd2f`. Land a newer SHA that:

1. names admission failures as `phase=admitting` and captures helper stderr;
2. refuses a `master` push unless `deploy/stage` is GREEN, except `--hotfix`;
3. lets `agent-merge-stage.sh --fix-red` replace a frozen unpromoted `stage` SHA when recovering
   a red `master`;
4. keeps the unique GREEN stage closeout docs from `1301d939`.

Production runtime stays on `4b8ba938` until that newer SHA is staged, attested, and merged.

## Executable guardrail

- `deploy/agent-merge.sh` `am_require_stage_promotable` — refuses `master` without GREEN
  `deploy/stage`.
- `deploy/agent-merge.suite.sh` seeds `pending` and `failure` stage statuses and expects the
  refusal; `--hotfix` is the only documented skip.
- `deploy/staging-phase7.test.sh` requires `CURRENT_PHASE=admitting` and `wd_die` of captured
  admission stderr.
- `deploy/stage-watchdog.test.sh` requires the `--fix-red` frozen-stage warning.

Command: `bash deploy/agent-merge.suite.sh && bash deploy/staging-phase7.test.sh && bash deploy/stage-watchdog.test.sh`.

## Remaining risk

Host copies of `watchdog.sh` still report `phase=fetching` until this SHA is admitted and the
controller installer runs. Until then, diagnose unattested quarantine from this incident and
from `admission-rejected.sha`, not from the generic fetching headline.
