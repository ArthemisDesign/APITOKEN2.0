# Final verification GitHub wrapper — 2026-08-24

Status: resolved

## Executive summary

Production `deploy/watchdog` quarantined `6ef38441116571d46c2ee95e4c903bed43687177` after
engine admission was GREEN. The host rolled the engine back to
`be6c88b171cbaf27e4b9fb67c9cf1e2186a77b56`. GitHub `deploy/watchdog` published only
`phase=verifying; selected final production verification failed after component admission`.
The inner lane `wd_die` never became the 140-character headline, and check run
`deploy/watchdog-log` was not visible (host PAT lacks Checks: write). Do not retry
`6ef38441`. This commit makes the post-admission wrappers generic and prefers the inner
lane error from the cycle transcript.

## Impact and detection

- Authoritative production engine SHA after rollback: `be6c88b1`. Loopback readiness stayed
  HTTP 200 (`8790`, `8792`, `8794`, `8803`, `8802`).
- `origin/master` stayed on `6ef38441`. GitHub `deploy/tests` and `deploy/engine` were GREEN.
  `deploy/watchdog` was RED at `2026-08-24T12:42:46Z`.
- Host status after the cycle: `phase=quarantined sha=6ef38441… detail=failed candidate remains blocked`.
- Observe journal after the first cycle only showed the quarantine poll (`-n 200`). The
  inner lane error was not in the GitHub description.

Measured GitHub timestamps for `6ef38441`:

- `2026-08-24T12:26:56Z` — production-engine deployment started
- `2026-08-24T12:31:21Z` — `deploy/engine` success (`Engine verified in production`)
- `2026-08-24T12:42:46Z` — `deploy/watchdog` failure (generic wrapper)

Vercel Production already served the customer docs from `master` independently of the host
rollback. Serving engine code returned to `be6c88b1`.

## Timeline

- `2026-08-24T12:22:20Z` — stage GREEN and `promotion/eligible` for `6ef38441`.
- `2026-08-24T12:25:45Z` — trusted-host `deploy/tests` GREEN.
- `2026-08-24T12:31:21Z` — engine blue-green admitted the candidate; `deploy/engine` GREEN.
- `2026-08-24T12:31:21Z`–`12:42:46Z` — `CURRENT_PHASE=verifying`; `run_final_verification_plan`
  ran in a subshell, then `rollback_engine`, then the outer `wd_die`.
- Later polls: SHA equals `rejected.sha`; watchdog waits for a newer commit or explicit retry.

The selected inner lane (panel / routing / monitoring / Codex / Gemini / KIMI) is not
recoverable from GitHub or from the observe 200-line window. Treat that cause as unknown.

## Root cause

`run_final_verification_plan` is invoked in a subshell so an inner `wd_die` does not skip
rollback. That is required. The outer wrapper
`selected final production verification failed after component admission` is not classified
as generic. `wd_github_failure_description` therefore prefers `WD_LAST_ERROR` over the cycle
transcript. The transcript's last `[watchdog] ERROR:` line is also the join wrapper
`final verification lanes failed (panel=…)`, which last-wins over the inner lane `wd_die`.

The same class already exists for commerce: `final_verify_backend` runs in a subshell, then
`failed verification after cutover`.

## Why existing safeguards missed it

- Engine admission, trusted-host tests, and stage attestation were GREEN. They do not own
  the post-admission smokes.
- `wd_error_is_generic` already treated `lanes failed` and `failed (exit N)` as wrappers.
  The post-admission sentence did not match those patterns.
- Check run `deploy/watchdog-log` is fail-open. A missing Checks: write permission still
  quarantines the SHA and still writes the 140-character status, so the headline was the
  only public diagnostic.

## Correction

Do not retry `6ef38441`. Land a newer descendant that:

1. classifies the two post-admission wrappers as generic;
2. skips generic `[watchdog] ERROR:` lines when selecting the cycle-transcript marker, so
   the inner lane `wd_die` wins;
3. keeps rollback-before-fail-closed in the subshell.

The Responses PNG mask from `6ef38441` remains in this descendant. Production runtime stays
on `be6c88b1` until this newer SHA is staged, attested, and has GREEN `deploy/watchdog`.

## Executable guardrail

- `deploy/watchdog-lib.sh` `wd_error_is_generic` — post-admission wrappers.
- `deploy/watchdog-lib.sh` `wd_failure_marker_line` — last specific operational marker.
- `deploy/watchdog-lib.test.sh` seeds an inner Codex `wd_die` plus both wrappers and expects
  the Codex line in `wd_github_failure_description`; same for the commerce cutover wrapper.

Command: `bash deploy/watchdog-lib.test.sh`.

## Remaining risk

- The inner lane that failed on `6ef38441` is still unknown. A flake during blue-green
  scrape/envelope probes can recur; the next RED SHA will name the lane.
- Host PAT still lacks Checks: write, so `deploy/watchdog-log` may stay unpublished. Diagnose
  from the 140-character headline first, then `ssh observe@` if the cycle is still in the
  last 200 journal lines.
- Customer docs on Vercel Production already describe the mask path while the serving engine
  is `be6c88b1` until this SHA is GREEN. `be6c88b1` already clones extra `image_generation`
  keys; the descendant adds fail-closed `file_id` plus docs.
