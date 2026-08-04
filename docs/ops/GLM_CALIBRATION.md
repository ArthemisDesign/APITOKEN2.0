# GLM Coding Plan live calibration

`tools/glm_calibration/run_live.py` is an operator fail-closed run of a live GLM Coding
Plan subscription (Z.ai / open.bigmodel.cn). Unlike the Claude/Gemini runners, it goes
**directly to the provider** rather than through our backend: the engine, engine
PostgreSQL, and customer traffic are untouched, and no synthetic calibration vectors are
created — the runner only reads the provider and prints evidence for the operator. The
target is exactly one subscription, pinned by the triple of `--profile` (the operator's
opaque label) + `--base-url` (an allowlist of two hosts: `https://api.z.ai` or
`https://open.bigmodel.cn`) + a key from the env `GLM_CALIBRATION_API_KEY`.

The runner resolves the `unknown`s from §6 of `docs/engine/GLM_PROVIDER.md`: the shape of
`usage` on the Anthropic route, real SSE incrementality, the unit semantics of the quota
endpoint, and the exact business codes of the quota wall. The paid matrix is three
subscription models (`glm-5.2`, `glm-5-turbo`, `glm-4.7`) × (non-stream + incremental
stream) on `POST {base}/api/anthropic/v1/messages` with Bearer and the Claude Code fleet
fingerprint (`User-Agent: claude-cli/2.1.195 (external, sdk-cli)`,
`anthropic-version: 2023-06-01`, `anthropic-beta: claude-code-20250219`).

## Safeguards

- Without an explicit `--execute`, no paid traffic is sent. Dry-run prints a
  machine-readable plan (legs, per-leg worst-case, total worst-case) and — if the key is
  set — takes only the free read-only quota anchor. Dry-run without a key does not fail
  silently: the plan honestly says `live_possible=false` and why.
- `--budget-usd` is integer nanoUSD, no float. The default hard cap is **$0.05** (the scale
  of the admission micro-smoke from AGENTS.md); raising it to the absolute ceiling of
  **$5** is possible only with an explicit `--i-understand`.
- Before every paid request, the runner computes and prints the worst-case bound from the
  official rate card (the same numbers as `crates/metering/src/glm.rs`): input ≤ prompt
  length in bytes (ASCII) + 32 tokens of framing, output ≤ `max_tokens` (hard limit 1024),
  cache — full miss. The budget guard is checked before dispatch; an actual exceeding the
  bound stops the run.
- Attribution goes through the quota endpoint `GET {base}/api/monitor/usage/quota/limit`
  (`Authorization: <key>` **without Bearer**; an invalid key returns HTTP 200 with
  `code: 401` in the body). Before the leg — a before-observation; after the response — two
  settled observations with a delay (`--quota-poll-delay`, default 5s). The delta is
  attributed to the served model only if every moved counter equals exactly the expected
  integer credits per the official formula (with off-peak ×0.5 on the UTC+8 schedule). Any
  deviation — foreign traffic, accounting lag, or unknown units — is recorded as
  `unattributed` and stops the matrix fail closed, without guessing. A zero delta on a
  sub-credit leg is an honest `below-resolution`, not an error.
- Retry is read-only only: the quota poll is retried up to three times on transport
  failure. A paid request after transport ambiguity is NEVER retried automatically: the leg
  is held at the full worst-case bound (`held-ambiguous`) and is not re-sent even on
  resume — only a new run id makes a new attempt.
- Typed rejections are classified by business code: `1311` (model not in the plan) — proven
  capability unavailability, the remaining models continue; `1308`/`1310` — quota wall:
  evidence is recorded (resolves §6.6), paid traffic stops; `401` — the key is dead, stop
  before any further request. Everything else — fail closed.
- Checkpoint after every leg (atomic write). `--resume <checkpoint>` continues the same run
  id without repeating completed legs; a mismatch of profile/base-url/budget/models/
  max_tokens with the checkpoint — fail closed. A fresh run refuses to overwrite someone
  else's checkpoint.
- The key is read only from env and is never materialized anywhere: not in argv, not in the
  report, not in the checkpoint, not in stdout/stderr; in typed error details the key is
  redacted. The report addresses the target only by the opaque `--profile` + `--base-url`.

## Commands

```bash
# 1. Plan + free quota anchor (no paid traffic):
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai

# 2. Paid matrix (6 legs, total worst-case ~$0.002 at defaults):
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai \
  --execute --budget-usd 0.05 \
  --report /tmp/glm-calibration-report.json

# 3. Interrupted run — continue without repeating legs:
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai \
  --execute --budget-usd 0.05 \
  --resume /tmp/glm-calibration-report.json.checkpoint.json
```

During the run, there must be no other traffic on the subscription: sequential execution
and a single profile are the only attribution guarantee; foreign quota movement converts to
`unattributed` and stops the matrix. To resolve §6.3 (quota endpoint units), a leg with a
delta of ≥ 1 credit is needed: `--max-tokens 1024 --i-understand --budget-usd` above the
default. The quota wall is deliberately not provoked; if the wall is encountered on its
own, its business code will end up in the report.

## Offline verification of the runner

```bash
python3 -m unittest tools.glm_calibration.test_run_live
# or
python3 -m pytest tools/glm_calibration/ -q
```

The tests cover the budget guard and hard caps, run-id resume without repeating legs, the
ban on repeating a paid request after transport ambiguity, secret containment (the key is
not in the report/checkpoint/stdout and is redacted from error details), attribution
against the background of foreign traffic → `unattributed`, dry-run without paid traffic,
the quota parser (HTTP 200 + `code: 401`), report completeness, and the money/credits/
off-peak parity with `crates/metering/src/glm.rs`.

## How to read the result

The report (`glm-live-calibration/v1`) is machine-readable JSON:

- `legs[]` — per-leg exact spend: `api_nanousd` per the rate card from the authoritative
  `usage`, preflight `worst_case_nanousd`, `usage_observed_keys` (the real field names —
  evidence for §6.1), before/after quota observations, `quota_deltas`, `attribution`
  (`attributed`/`below-resolution`/`unattributed`), and for stream — `stream_evidence`
  (frame count, text-delta, first-to-last ms, incrementality flag).
- `coverage` — the status of each model × {non_stream, stream}
  (`ok`/`unavailable`/`held-ambiguous`/`failed`/`not-run`).
- `unavailable_capabilities` — proven unavailability (e.g. `1311`), not zero spend.
- `unattributed_deltas` — raw deltas the runner refused to guess.
- `unknowns` — an explicit status per §6: `usage_form`, `sse_incrementality`,
  `quota_units`, `quota_wall_codes` — each `resolved`/`unresolved` with detail.
  `unresolved` is not a runner failure but an honest "not proven in this run" (e.g. the
  wall was not encountered, and the deltas were below the provider's resolution).
- `complete: false` + `failure` — the run stopped fail closed; the partial report and
  checkpoint are preserved, continuation is via `--resume`. Formally, complete requires
  `ok`/`unavailable` on all legs; a `held-ambiguous` leg keeps the run incomplete forever —
  this is intentional.

`spent_nanousd` is the cost of the run's requests in official API replacement cost, not the
charge against the subscription quota and not the plan price. `held_nanousd` is the
conservative hold at the worst-case bound for legs with an unproven outcome.
