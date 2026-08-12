# Tripo3D plane live calibration

`tools/tripo3d_calibration/run_live.py` is an operator fail-closed run of the Tripo3D task-media
plane (backend-only, default-off, dormant in production). The runner never talks to the provider
directly: paid tasks are created through the plane's own bounded REST surface
(`POST /v1/3d/generations` with a forwarding-admin key — the admin path is unmetered for customer
accounts; the provider account pays and the durable calibration ledger records the turn), and all
evidence is read from the admin-only `GET /tripo3d-subs` projection (control key in `x-api-key`).

Sources of truth in the projection: delivery diagnostics
(`pending_events`/`dropped_events`/`persistence_ok`), the free provider preflight
(`profiles[].balance` — the plane's `GET /user/balance` sweep, raw halves verbatim, parsed halves
`null` while the unit is unproven, docs/engine/TRIPO3D_PROVIDER.md §5.2), the fleet counters
(`tracked_tasks`, `inflight_requests`, `inflight_drains`, `missing_consumed_credit`,
`tariff_anomaly`, `undocumented_final`), and the durable cumulative dual-ledger spend
(`calibration.observed_spend_nano` / `observed_spend_native_millicredits`; money is a decimal
string, unknown is `null`, never `0`). Profiles appear only by opaque roster id; the subject/API
key/proxy never enter the contract or the report.

## Attribution: why single-profile, and what it guarantees

Unlike the KIMI plane, the Tripo3D gateway deliberately carries NO admin-only calibration
profile/request-id headers. Exact targeting is therefore structural:

- the run REQUIRES a roster of exactly one owned profile (`fleet.profiles == 1` and its id equals
  `--profile`), so the selector cannot spill or rebind to a neighbour;
- each paid create returns the plane's exact internal request id (`task_id`), and the runner polls
  only that task;
- a leg's spend delta is attributed only when `tracked_tasks` advanced by exactly one across the
  leg and no foreign `inflight` appears at settle time; otherwise the delta is charged to the
  budget, recorded with `attribution: "ambiguous"`, and the matrix stops fail closed. Concurrent
  traffic is never guessed away.

Do not run this against a multi-profile roster or while any other client can reach the plane.

## What the run covers

Default matrix (all prices are the exact published card of docs/engine/TRIPO3D_PROVIDER.md §5.1,
mirrored from `crates/metering/src/tripo3d.rs`):

- `text_to_model` × every reviewed `model_version` (`v3.1-20260211`, `v3.0-20250812`,
  `P1-20260311`, `v2.5-20250123`, `v2.0-20240919`, `Turbo-v1.0-20250506`, `v1.4-20240625`),
  no-texture — the cheapest shape per version (10–30 credits);
- the texture/quality option legs on the cheapest reviewed Standard version (`v2.5-20250123`):
  `texture` standard/detailed/extreme, `smart_low_poly`, `quad`, `generate_parts`,
  `geometry_quality=detailed`. `extreme` acceptance on the public API is unproven (§6.8), so a
  typed refusal is recorded as unavailable, not fatal;
- `refund-probe:image_to_model` — a deliberately unfetchable image URL: the task must finalize
  failed and settle exactly zero (the §4.1 refund evidence). A paid failure or a success is a
  fail-closed anomaly;
- with `--image-url <reviewed url>`: the `image_to_model` sweep across the same versions;
- with `--original-model-task-id <upstream id>`: one `texture_model` standard leg. The plane
  never exposes upstream task ids, so the operator supplies one from a finished model (e.g. the
  provider console). Without the flag, `texture_model` is recorded in
  `unavailable_capabilities` with the reason — never silently skipped.

## Safeguards

- Without `--execute` the runner prints a deterministic machine-readable dry-run plan
  (`tripo3d-live-calibration-plan/v1` with per-leg `reserve_credits`/`worst_case_nanousd`) and
  exits 0. When the control key is in the environment, the dry-run additionally fetches the free
  read-only baseline projection — nothing else.
- `--execute` REQUIRES an explicit `--budget-usd` (strict decimal, integer nanoUSD internally).
  There is no default: the cheapest paid Tripo3D task is 5 credits = $0.05, which exceeds the
  repo's default $0.0001 admission cap (the §7 open admission-budget question), so naming a
  budget IS the operator's authorization. The hard CLI ceiling is `5.00` USD; a larger value is
  a CLI error, not a warning. The budget is a single aggregate for the entire run.
- Before every paid create, the worst-case bound (the exact published card price of the leg's
  combination × $0.01/credit) is printed and checked against the remaining budget. A settled
  delta above the bound, a backwards counter, or an advancing `tariff_anomaly` counter stops the
  run fail closed.
- Baseline health gate before the first leg and before every paid create: `enabled=true`,
  `calibration_authority_available=true`, `delivery.pending_events=0`,
  `delivery.dropped_events=0`, `delivery.persistence_ok=true`; the single profile is `live`, not
  `balance_walled`, has no active cooling axis, `inflight=0`, a non-empty declared cohort, and a
  completed free balance preflight (`balance.observed_at` non-null).
- A paid create gets exactly ONE transport attempt. A transport failure without an HTTP status
  is a paid ambiguity: the leg is held at its full worst-case bound (`held-ambiguous`) and is
  never re-sent, even on `--resume`. A typed non-2xx create response is pre-money-boundary
  evidence (the plane's money boundary is the upstream `code:0 + task_id` create; every error
  response is produced before it) and holds nothing: a 400 on an optional leg is recorded as
  unavailable and the matrix continues, a 400 on a required `text_to_model` leg is blocking,
  401/403 (auth / provider balance wall) and 429 (rate/concurrency wall) stop the run with a
  profile stop recorded.
- Only read-only surfaces retry (bounded, 3 attempts): `GET /tripo3d-subs`, task status polls.
- Settlement evidence: after the task finalizes, the runner waits for `pending_events=0`, the
  drain count back to baseline, and a post-turn balance observation (`balance.observed_at >=`
  task completion) before computing deltas. An unresolved snapshot is not zero: the leg fails as
  a post-spend poll error.
- Unique run id `tripo3d-cal-<ts>-<uuid8>`; a machine-readable checkpoint
  (`tripo3d-live-calibration-checkpoint/v1`) is written atomically after every leg.
  `--resume <checkpoint>` continues the same run id and refuses any identity mismatch
  (profile/api-url/budget/matrix); a fresh run refuses to overwrite an existing checkpoint.
- Keys come only from the environment (`APITOKEN_API_KEY`, `CLAUDE_API_CONTROL_KEY`) or an
  operator-supplied `--capacity-command`; they never enter argv, the report, the checkpoint, or
  logs, and the API key is redacted from typed error details.

## Running it

Prerequisites: the engine running locally with the dormant plane enabled
(`CLAUDE_API_PROVIDER=tripo3d`, `CLAUDE_API_TRIPO3D_ENABLED=1`, a roster dir holding exactly ONE
owned profile with a topped-up balance, the PG billing authority, artifact dir), the
forwarding-admin key in `APITOKEN_API_KEY`, the control key in `CLAUDE_API_CONTROL_KEY`, and —
for paid traffic — explicit human authorization of the budget.

Always dry-run first (sends nothing, exit 0):

```bash
python3 tools/tripo3d_calibration/run_live.py --profile <opaque-profile-id>
```

Then, only after explicit human permission:

```bash
python3 tools/tripo3d_calibration/run_live.py \
  --execute \
  --profile <opaque-profile-id> \
  --budget-usd 5.00 \
  --report /tmp/tripo3d-calibration-report.json
```

The default full matrix worst case is ~305 credits ≈ $3.05 (~$4.75 with `--image-url`), so
`5.00` covers it; the per-leg guard stops the matrix safely if a smaller budget runs out.

## Offline verification

```bash
python3 -m unittest tools.tripo3d_calibration.test_run_live
```

The tests (unittest + fakes, the engine is not started) cover the strict decimal parser and the
hard $5.00 ceiling, the explicit-budget requirement, the budget/hold guards, the exact card
pricing vectors per tier and option, the default matrix composition and its recorded
unavailability, every baseline health gate and the single-profile no-spill guard, exact vs
ambiguous attribution amid concurrent traffic, the tariff-anomaly and over-bound fail-closed
stops, the refund probe both ways, the never-resend rule for a transport-ambiguous create
(including across resume), typed-error budget neutrality, read-only retries, secret containment
(report/checkpoint/stdout and typed error details), checkpoint identity/resume/overwrite rules,
and the incomplete-report shape.

## How to read the result

The `tripo3d-live-calibration/v1` report (default `/tmp/tripo3d-calibration-report.json`) is
written even on failure with `complete=false` and partial records: `run_id`, `budget_nanousd` /
`spent_nanousd` / `held_nanousd` (strings), `target` (profile, api_url, cohort), `records[]`
(leg, kind, model_version, exact `task_id`, terminal task status, reserve credits,
`upper_bound_nanousd`/`settled_nanousd`/`settled_native_millicredits` strings,
`attribution` (`exact`/`ambiguous`), `foreign_traffic`, `missing_consumed_credit`,
`undocumented_final`, before/after balance halves verbatim plus parsed micro-units and the
drawdown), `leg_status`, `coverage` (expected/completed/pending), `unavailable_capabilities`,
`stops`, `baseline_observations`, `final_observations`.

`complete=true` requires every leg `ok` or `unavailable` (non-blocking), no failure, and no
pending legs. A `held-ambiguous` leg makes the report permanently incomplete — repeating that
paid request is forbidden; investigate the plane and the provider account manually. The
balance-halves unit remains an open unknown (docs/engine/TRIPO3D_PROVIDER.md §6.1) until this
matrix runs on an owned account: compare `settled_native_millicredits` (exact, from
`consumed_credit`) against `balance_drawdown_micro_units` to prove the unit.
