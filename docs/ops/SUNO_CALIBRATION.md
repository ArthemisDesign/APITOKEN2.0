# Suno plane live calibration

`tools/suno_calibration/run_live.py` is an operator fail-closed run of the Suno session-pool
plane (backend-only, default-off, dormant in production). The runner never talks to the provider
directly: paid generations are created through the plane's own bounded REST surface
(`POST /v1/audio/generations` with a forwarding-admin key — the admin path is unmetered for
customer accounts; the subscription pays and the durable calibration ledger records the turn),
and all evidence is read from the admin-only `GET /suno-subs` projection (control key in
`x-api-key`).

Sources of truth in the projection: delivery diagnostics
(`pending_events`/`dropped_events`/`persistence_ok`), the free provider preflight
(`profiles[].quota` — the plane's `GET /api/billing/info/` sweep: `monthly_limit`,
`monthly_usage`, `total_credits_left` verbatim, `null` while unread, docs/engine/SUNO_PROVIDER.md
§5.2), the fleet counters (`tracked_generations`, `inflight_requests`, `inflight_drains`,
`unattributed_settlements`, `tariff_anomaly`), and the per-window durable cumulative dual-ledger
spend (`calibration[].observed_spend_nano` / `observed_spend_native_millicredits` /
`unattributed_fraction_units`, keyed by exact `window_duration_secs`; money is a decimal string,
unknown is `null`, never `0`). Profiles appear only by opaque roster id; the subject/cookie/
session id/proxy never enter the contract or the report.

## Attribution: why single-profile, and what it guarantees

Unlike the KIMI plane, the Suno gateway deliberately carries NO admin-only calibration
profile/request-id headers. Exact targeting is therefore structural:

- the run REQUIRES a roster of exactly one owned profile (`fleet.profiles == 1` and its id equals
  `--profile`), so the selector cannot spill or rebind to a neighbour;
- each paid create returns the plane's exact internal request id (`generation_id`), and the
  runner polls only that generation;
- a leg's spend delta is attributed only when `tracked_generations` advanced by exactly one
  across the leg and no foreign `inflight` appears at settle time; otherwise the delta is charged
  to the budget, recorded with `attribution: "ambiguous"`, and the matrix stops fail closed.
  Concurrent traffic is never guessed away;
- the per-window calibration rows are one ledger seen through each window: every row present in
  both snapshots must show the SAME delta. A changed row set (reset/cutover), disagreeing rows,
  or missing rows fail closed — an unresolved snapshot is never a zero.

Do not run this against a multi-profile roster or while any other client can reach the plane.

## What the run covers

All prices follow the reviewed DERIVED schedule (docs/engine/SUNO_PROVIDER.md §5.1, mirrored
from `crates/metering/src/suno.rs`): $0.004/credit (Pro unit economics, conservative against
Premier). There is no official API rate card.

- `song` × every reviewed paid model (`v4`, `v4.5`, `v4.5+`, `v5`, `v5.5`) — the published flat
  5 credits/song = $0.02, instrumental description form. The exact wire spellings of the model
  ids are `unknown` until this matrix pins them (§6.4): a typed refusal of a model is recorded
  as unavailable (blocking) and stops the run;
- `lyrics` — unpublished price: the documented conservative 50-credit reserve ($0.20) is the
  preflight bound; settlement is the attributed post-turn credit delta or the reserve fallback;
- `extend` — only with `--continue-clip-id <upstream id>`; same conservative reserve;
- `stems` — only with `--song-id <upstream id>`; same conservative reserve. The plane never
  exposes upstream ids, so the operator supplies them. Without the flags, both legs are recorded
  in `unavailable_capabilities` with the reason — never silently skipped.

Each record carries the settlement path itself: `settlement: "attributed"` when the engine
paired the post-turn credit delta with the turn, or `"reserve-fallback"` when the fleet
`unattributed_settlements` counter advanced (the documented conservative settle, with
`unattributed_fraction_delta` beside it).

## Safeguards

- Without `--execute` the runner prints a deterministic machine-readable dry-run plan
  (`suno-live-calibration-plan/v1` with per-leg `reserve_credits`/`worst_case_nanousd`) and
  exits 0. When the control key is in the environment, the dry-run additionally fetches the free
  read-only baseline projection — nothing else.
- `--execute` REQUIRES an explicit `--budget-usd` (strict decimal, integer nanoUSD internally).
  There is no default: one song is $0.02 derived, which exceeds the repo's default $0.0001
  admission cap (the §7 open admission-budget question), so naming a budget IS the operator's
  authorization. The hard CLI ceiling is `1.00` USD; a larger value is a CLI error. The budget
  is a single aggregate for the entire run. The default matrix worst case is 75 credits = $0.30
  (175 credits = $0.70 with extend+stems).
- Before every paid create, the worst-case bound (published 5 credits for a song, else the
  conservative 50-credit reserve) is printed and checked against the remaining budget. A settled
  delta above the bound, a backwards counter, a changed/disagreeing window-row set, or an
  advancing `tariff_anomaly` counter stops the run fail closed.
- Baseline health gate before the first leg and before every paid create: `enabled=true`,
  `calibration_authority_available=true`, `delivery.pending_events=0`,
  `delivery.dropped_events=0`, `delivery.persistence_ok=true`; the single profile is `live` and
  `routable`, not `quota_walled`, has no active cooling axis (including `captcha_until` — the
  hCaptcha gate is never solved), `inflight=0`, an authoritative reviewed paid plan
  (`Pro`/`Premier`), and a completed free quota preflight (`quota.observed_at` non-null).
- A paid create gets exactly ONE transport attempt. A transport failure without an HTTP status
  is a paid ambiguity: the leg is held at its full worst-case bound (`held-ambiguous`) and is
  never re-sent, even on `--resume`. A typed non-2xx create response is pre-money-boundary
  evidence (the plane's money boundary is the upstream create inside the profile lease) and
  holds nothing: a 400 on the optional legs (lyrics/extend/stems) is recorded as unavailable and
  the matrix continues, a 400 on a required song leg is blocking, 401/403 (session/quota wall)
  and 429 (rate wall) stop the run with a profile stop recorded.
- Only read-only surfaces retry (bounded, 3 attempts): `GET /suno-subs`, generation status
  polls.
- Settlement evidence: after the generation finalizes (`complete`), the runner waits for
  `pending_events=0`, the drain count back to baseline, and a post-turn quota observation
  (`quota.observed_at >=` completion) before computing deltas.
- Unique run id `suno-cal-<ts>-<uuid8>`; a machine-readable checkpoint
  (`suno-live-calibration-checkpoint/v1`) is written atomically after every leg. `--resume
  <checkpoint>` continues the same run id and refuses any identity mismatch
  (profile/api-url/budget/matrix); a fresh run refuses to overwrite an existing checkpoint.
- Keys come only from the environment (`APITOKEN_API_KEY`, `CLAUDE_API_CONTROL_KEY`) or an
  operator-supplied `--capacity-command`; they never enter argv, the report, the checkpoint, or
  logs, and the API key is redacted from typed error details.

## Running it

Prerequisites: the engine running locally with the dormant plane enabled
(`CLAUDE_API_PROVIDER=suno`, `CLAUDE_API_SUNO_ENABLED=1`, a roster dir holding exactly ONE owned
Pro/Premier profile, the PG billing authority, artifact dir), the forwarding-admin key in
`APITOKEN_API_KEY`, the control key in `CLAUDE_API_CONTROL_KEY`, and — for paid traffic —
explicit human authorization of the budget.

Always dry-run first (sends nothing, exit 0):

```bash
python3 tools/suno_calibration/run_live.py --profile <opaque-profile-id>
```

Then, only after explicit human permission:

```bash
python3 tools/suno_calibration/run_live.py \
  --execute \
  --profile <opaque-profile-id> \
  --budget-usd 1.00 \
  --report /tmp/suno-calibration-report.json
```

Add `--continue-clip-id <upstream id>` and `--song-id <upstream id>` to include the extend and
stems legs.

## Offline verification

```bash
python3 -m unittest tools.suno_calibration.test_run_live
```

The tests (unittest + fakes, the engine is not started) cover the strict decimal parser and the
hard $1.00 ceiling, the explicit-budget requirement, the budget/hold guards, the published vs
conservative-reserve pricing, the default matrix composition and its recorded unavailability,
the admitted wire shapes of every operation body, every baseline health gate (including the
captcha axis and the reviewed-plan identity) and the single-profile no-spill guard, exact vs
ambiguous attribution amid concurrent traffic, the window-row pairing rules (missing/changed/
disagreeing rows fail closed), the reserve-fallback settlement recording, the tariff-anomaly and
over-bound fail-closed stops, the never-resend rule for a transport-ambiguous create (including
across resume), typed-error budget neutrality, secret containment (report/checkpoint/stdout and
typed error details), checkpoint identity/resume/overwrite rules, and the incomplete-report
shape.

## How to read the result

The `suno-live-calibration/v1` report (default `/tmp/suno-calibration-report.json`) is written
even on failure with `complete=false` and partial records: `run_id`, `budget_nanousd` /
`spent_nanousd` / `held_nanousd` (strings), `target` (profile, api_url, plan), `records[]`
(leg, operation, model, exact `generation_id`, terminal status, reserve credits,
`upper_bound_nanousd`/`settled_nanousd`/`settled_native_millicredits` strings,
`settlement` (`attributed`/`reserve-fallback`), `unattributed_fraction_delta`,
`attribution` (`exact`/`ambiguous`), `foreign_traffic`, before/after quota counters verbatim
plus `monthly_usage_delta` and `credits_left_drawdown`), `leg_status`, `coverage`
(expected/completed/pending), `unavailable_capabilities`, `stops`, `baseline_observations`,
`final_observations`.

`complete=true` requires every leg `ok` or `unavailable` (non-blocking), no failure, and no
pending legs. A `held-ambiguous` leg makes the report permanently incomplete — repeating that
paid request is forbidden; investigate the plane and the subscription manually. The semantics of
`total_credits_left` vs `monthly_usage` remain an open unknown (docs/engine/SUNO_PROVIDER.md
§6.3) until this matrix runs on an owned subscription: compare `settled_native_millicredits`
(exact, from the attributed delta) against both quota counters to pin them.
