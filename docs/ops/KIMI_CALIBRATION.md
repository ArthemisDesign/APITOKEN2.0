# KIMI plane live calibration

`tools/kimi_calibration/run_live.py` is an operator fail-closed run of the KIMI subscription
plane (backend-only, default-off). The runner does not derive capacity from prompt size and
does not trust client-side charging. The sources of truth are the admin-only
`GET /kimi-subs` (control key in `x-api-key`): delivery diagnostics
(`pending_events`/`dropped_events`/`persistence_ok`), immutable
`calibration_recent_turns` (the only API-dollar authority; money amounts are decimal strings,
unknown is `null`, never `0`), and per-window quota observations identified by the exact
`duration_secs` (18000 — rolling 5h, 604800 — weekly; duration is data, not a hardcoded
list). The aggregate `calibration` row of a profile remains estimator statistics and is not
used for attributing an individual request. Profiles appear only by opaque roster id
(1..128, ASCII alnum plus `-`/`_`); `subject_id`/email/phone never appear in the contract
and never enter the report.

The `/kimi-subs` endpoint and the admin-only calibration request headers arrive as a
separate engine change; the runner is written against the frozen contract, and the offline
tests mock the endpoint, so its existence is not required to verify the runner.

## What the run covers

The matrix is built from `--models` (by default the documented served set: `kimi-k3`,
`kimi-k2.7-code`, `kimi-k2.7-code-highspeed`, `kimi-k2.6`) × context mode × reasoning effort
that the plane accepts. The request goes to the exact subscription alias (`kimi-for-coding`,
`kimi-for-coding-highspeed`, `k3-256k`, `k3`) on the Anthropic-compatible
`POST /v1/messages`; the served model is taken only from the immutable event. `k3`: 256k
always, 1m only when the profile's paid plan is in the reviewed `--one-m-plans` list (the
list may be empty — then 1m legs are recorded in `unavailable_capabilities` with evidence
rather than silently skipped), efforts `low`/`high`/`max` plus `off`. The coding family:
`high` (Thinking ON) plus `off`. `kimi-k2.6` has no alias of its own and is covered through
the documented thinking-off re-route (`k3` and `k2.7-code` are served as `kimi-k2.6`);
duplicate legs are deduplicated by the exact alias/context/effort triple.

## Safeguards

- Without `--execute`, the runner prints a deterministic machine-readable dry-run plan
  (`kimi-live-calibration-plan/v1` with per-leg `upper_bound_nanousd`) and exits with code
  0. No live request is sent without `--execute`.
- `--budget-usd` is parsed by a strict decimal parser (no float/exponent) into integer
  nanoUSD, default `0.0001`; values above the hard cap of `0.0001` USD (= `100_000`
  nanoUSD) are a CLI error. The budget is a single aggregate for the entire run.
- Before every paid request, a worst-case upper bound is built from the official rate card
  of the served model: the full accepted input context of the alias at the miss rate (cache
  write == miss, hit is cheaper) plus the entire requested max output. Billing follows the
  served model: for thinking-off legs, `kimi-k2.6` is also among the candidates, and the
  per-class maximum is taken. `charge` verifies `actual <= upper_bound` and never crosses
  the aggregate cap.
- Paid turns go only to an explicit `--profile <opaque id>`; the id format is validated. If
  the endpoint shows the profile as dead (`live=false`), not authenticated, or cooling (any
  non-null `cooling.auth_until`/`transport_until`/`quota_until` in the future) — stop
  before spending.
- Baseline health before every paid leg and after attribution: `enabled=true`,
  `calibration_authority_available=true`, `delivery.pending_events=0`,
  `delivery.dropped_events=0`, `delivery.persistence_ok=true`,
  `calibration_recent_turn_limit >= 512`. Any violation is a fail-closed stop.
- Per paid turn, the runner generates a canonical UUIDv4, verifies its absence in the
  before snapshot, and sends it via the admin-only headers `x-apitoken-calibration-profile`
  (full opaque id, exact target; the engine rejects without spill/rebind) and
  `x-apitoken-calibration-request-id`. Attribution: poll `/kimi-subs` until exactly one new
  turn event appears with this exact request id, profile id, and served model; `0` after
  the timeout or `>1` — fail closed. Parallel foreign events are ignored by id; duplicate
  same-id events — fail closed; rebind (profile/served mismatch) — stop.
- After the durable event and `pending_events==0`, the runner waits for a post-turn window
  observation with `observed_at >= completed_at` and only then computes the per-window
  fraction/native delta. An unresolved snapshot is not zero: the leg is excluded from
  profitability. A change of `resets_at` identity is `reset-crossed` — no delta at all.
- A paid request after transport ambiguity is NEVER retried automatically (exactly one
  attempt). The only safe-stop proof is `x-apitoken-execution-state: not_started` on
  synthetic pre-delivery errors; only such 429/503 stop the run as an explicit transient
  stop. Read-only operations (GET `/kimi-subs`, discovery) are retried in a bounded fashion
  (3 attempts).
- Unique run id `kimi-cal-<ts>-<uuid8>`. The report contains no secrets and no raw prompts
  (only a bounded `prompt_sha256_12`), only the opaque profile id.
- API/control keys are read only from env or disclosed inside the remote shell in
  production SSH mode; they never enter argv, the report, or test fixtures.
- HTTP 400/403/404 of a required capability and a broken cost-vector sum are a fail-closed
  stop with a record in `unavailable_capabilities` (`blocking=true`); unavailable
  capabilities are recorded with evidence, not silently skipped.

## Production command

Paid traffic is FORBIDDEN without explicit human authorization. A live Kimi Code
subscription is a mandatory prerequisite (currently a human-blocked dependency), as is the
engine change with `/kimi-subs` and the calibration headers. Always dry-run first (sends
nothing, exit 0):

```bash
python3 tools/kimi_calibration/run_live.py \
  --profile <opaque-profile-id> \
  --models kimi-k3 kimi-k2.7-code kimi-k2.7-code-highspeed kimi-k2.6
```

Then, only after explicit human permission, a green deploy of the exact runtime SHA, and
the preflight checklist below:

```bash
python3 tools/kimi_calibration/run_live.py \
  --execute \
  --profile <opaque-profile-id> \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 0.0001 \
  --report /tmp/kimi-calibration-report.json
```

`$0.0001` is the hard default ceiling of the aggregate budget for the entire run: a larger
value cannot be passed through the CLI. The production SSH path reads `/kimi-subs` and
sends paid turns to the loopback of the KIMI plane's stable origin (`127.0.0.1:8803`); the
forwarding-admin key is disclosed only inside the remote shell (`${CLAUDE_API_KEYS%%,*}`),
the control key is `$CLAUDE_API_CONTROL_KEY`, and no secret is returned over SSH. 1m legs
require a reviewed plan: `--one-m-plans Allegretto Allegro Vivace` (by default the list is
empty, and 1m is recorded as unavailable).

Preflight checklist (everything is mandatory):

- green production deploy of the exact runtime SHA, `/kimi-subs` answers to the control key;
- `enabled=true`, `calibration_authority_available=true`;
- `delivery.pending_events=0`, `delivery.dropped_events=0`, `delivery.persistence_ok=true`;
- `calibration_recent_turn_limit >= 512`;
- ≥1 profile with `live=true`, `authenticated=true`, no cooling in the future, and a
  non-empty paid plan;
- the exact opaque profile id for `--profile` from `profiles[].id`.

## Capability probes (tools and media)

The plane refuses `tools`, `tool_choice`, `mcp_servers` and media parts with
`kimi_tools_unpriced` / `kimi_media_unpriced`. That is not a missing feature: unknown 8 of the
manifest is "the existence and cost of paid tool/search units on the subscription route", and the
onboarding contract forbids dispatching a paid capability before a finite per-request unit ceiling
is proved. A caller therefore cannot use tools or images on KIMI today.

These probes are how that changes. They carry their own authorization, separate from the coverage
budget, because their cost is exactly what is unproven — letting them share `--budget-usd` would
hide an unbounded spend behind a bounded one:

```bash
python3 tools/kimi_calibration/run_live.py \
  --execute --profile <opaque-profile-id> \
  --production-capacity-over-ssh --production-api-over-ssh \
  --budget-usd 0.0001 \
  --capability-probe-budget-usd 0.0001 \
  --report /tmp/kimi-calibration-report.json
```

Without `--capability-probe-budget-usd` the probes are recorded in `unavailable_capabilities` with
`skipped_before_dispatch: true` and a reason naming the unproven cost — never silently omitted, so
an untested capability can never read as a tested one.

Each probe is the smallest thing that still exercises the surface: one tool with an empty schema
that the model can call at most once, and a single 1x1 PNG part beside the prompt. They must stay
that way. A probe that can fan out prices something other than one call, which is the number the
plane needs before it can lift its refusal.

## Offline verification

```bash
python3 -m unittest tools.kimi_calibration.test_run_live
```

The tests (unittest + mock, the engine is not started) cover the default dry-run without a
single request, the `$0.0001` hard cap and the strict decimal parser, the aggregate guard
across sequential legs, the upper-bound math for every served model (including the k3→k2.6
re-route pricing), exact request-id attribution against the background of foreign events,
fail-closed duplicate/rebind/baseline/cooling/dead, unresolved quota (not zero, excluded
from profitability), reset-crossed, the absence of a paid retry after ambiguous transport,
read-only retries, the absence of secrets in report/argv, writing an incomplete report on
failure, and the end-to-end `Runner.execute_leg` on FakeApi/FakeSubs.

## How to read the result

The `kimi-live-calibration/v1` report (default `/tmp/kimi-calibration-report.json`) is
written even on failure with `complete=false` and partial records: `run_id`,
`budget_nanousd_total` / `spent_nanousd_total` (strings), `profile`, `plan`, `models`,
`records[]` (leg, requested/served model, `context_mode`, `reasoning_effort`, exact
`request_id`, `upper_bound_nanousd`/`actual_nanousd` strings, full usage and api_cost
vectors as strings, per-window `status` (`resolved`/`unresolved`/`reset-crossed`) with
fraction/native delta or `null`), `unavailable_capabilities`, `stops`, `coverage`
(expected/completed/pending legs), `model_profitability`, `final_observations`.

Profitability/remaining-capacity conclusions are admissible only for legs with a positive
distinguishable quota delta and without a foreign immutable turn on the same profile within
the interval: such legs are marked `profitability_eligible=true`, the rest are excluded
from the ranking. `model_profitability` is sorted descending by API nanoUSD per 1% of the
window (exact `window_duration_secs`), separately for each plan × served model × context ×
effort combination. A missing row means insufficient provider resolution/isolation, not a
zero value of the model. `complete=true` is possible only without blocking unavailable
entries and without pending legs; any partial report is grounds for a manual
investigation — repeating a paid request after transport ambiguity is always forbidden.
