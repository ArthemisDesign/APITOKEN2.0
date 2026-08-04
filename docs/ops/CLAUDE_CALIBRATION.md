# Live calibration of the Claude pool

`tools/claude_calibration/run_live.py` is an operator load run against the real Claude pool.
It checks all published Claude model IDs and, for every actually supported tier, executes the
full matrix of fresh input/output, cache write/read 5m, cache write/read 1h, and Web Search.
Fast Mode is sent with the mandatory `fast-mode-2026-02-01` beta only for Opus 5 and Opus 4.8;
the official conversion catalog does not show Fast for the other models. Then, budget
permitting, the runner collects additional measurable signal with standard 1h cache-write
requests from Fable 5. The resulting JSON contains the exact spend, the 5h/7d quota movement,
the actually unavailable capabilities, and a model × tier ranking by the observed API-dollar
equivalent per 1% of the 5-hour window.

This is not a frontend test and not a calculation based on prompt size. The sources of truth
are the backend `/capacity`, the immutable `calibration_recent_turns`, and provider quota
snapshots. The aggregate `calibration_evidence` remains statistics of the entire accumulated
mix, but it is not used to attribute an individual test request. `/capacity` separates two
kinds of evidence: reset-bearing durable snapshots build the estimator/history, while a fresh
exact fraction without a reset can determine only the current remaining. It does not prove the
next window's time, so the horizon fields stay `null` until the provider returns a real reset.

## Safeguards

- Without an explicit `--execute` the script exits before any live request.
- `--budget-usd` accepts no more than `40`; money amounts are converted to integer nanoUSD
  without floats.
- Before every generation a free `/v1/messages/count_tokens` is executed. Anthropic rejects
  server-side Web Search in this endpoint, so the runner removes only that tool schema from
  the preflight, and then reserves the model's full `max_input_tokens` from `/v1/models`:
  search results are added after the preflight and are also metered as input. A missing or
  contradictory model limit stops the run. The worst case separately includes a full cache
  miss of the required TTL, the entire `max_tokens` output, and all permitted Web Search
  calls.
- The guard checks the free budget of **every** healthy subscription, not only the expected
  sticky home: an unexpected affinity rebind cannot spill a request onto an account that has
  already exhausted its test limit.
- Before generation the runner records all visible immutable `request_id`s. After the
  response, the next request is forbidden until exactly one exact
  `profile + model + tier + full token vector` appears among the new backend events. Therefore
  any parallel customer traffic, including traffic hitting the same aggregate row, does not
  change the cost of the test turn. Two new fully identical events remain an honest ambiguity
  and stop the run fail closed.
- A rebind, cooling/dead, degraded/pending delivery, or an actual cost above the preflight
  bound stops the specific unsafe branch fail closed. A provider quota wall removes only the
  corresponding subscription from the remaining load, without depriving the other profiles of
  data. An expected 400/403/429 of the first Fast request is recorded as
  `unavailable_capabilities` and is not retried for the remaining token legs of the same
  model/profile pair. A missing required token class does not interrupt the rest of the
  matrix, but marks the leg as `coverage_ok=false` and the whole report as incomplete.
- Between turns of one subscription a 16-second pause is kept — longer than the 15-second
  backend probe debounce. This gives the post-turn poll a chance to tie the exact spend to the
  new quota fraction.
- The cache payload includes a unique `run_id`: the write/read of one pair share one key, but
  a new run cannot mistake a still-live 5m/1h cache of a previous run for its own cache write.
- A brief transport failure before execution is automatically retried up to three times only
  for the read-only `/models`, `count_tokens`, and `/capacity`. A paid `/messages` is never
  retried after an SSH/HTTP transport ambiguity: the runner stops to rule out a double charge.
- The API key and the panel/control key are read only from env/remote shell and never appear
  in the report. The email already arrives from `/capacity` in a bounded mask without the
  domain.
- Production mode sends generation over SSH directly to the stable loopback router with a
  forwarding-admin key that is exposed only inside the remote shell. The admin-only header
  addresses a bounded four-character profile hint, is stripped before the upstream, and is
  fail-closed on collision. Therefore normal customer routing/reserve does not interfere with
  measuring a specific subscription, and a calibration request never spills/rebinds onto a
  neighbouring one.

## Production command

The control snapshot and live generation are safely executed on the production host: the
remote shell loads `/srv/claude-api/data/server.env`; the panel key is used only for the JSON
`/capacity`, and the forwarding-admin key — only for loopback `/v1/*`. No secret is printed
over SSH or returned to the local process. `APITOKEN_API_KEY` is needed only for the
legacy/public mode without `--production-api-over-ssh`.

```bash
python3 tools/claude_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --report /tmp/claude-calibration-report.json
```

Before launching, a green production deploy and baseline are mandatory:

- `calibration_delivery.pending_events=0`;
- `calibration_delivery.dropped_events=0`;
- `calibration_delivery.persistence_ok=true`;
- at least one `per_sub` row with `routable=true`, without `dead`/`cooling`.

Every routable row must have a non-empty paid plan. The backend uses the OAuth profile first;
for an inference-only token with a 403, only the unanimous plan of the remaining subscriptions
of that fleet is acceptable. If the fleet is mixed or entirely unlabelled, the load is not
started until an authoritative plan appears.

In production SSH mode the runner creates a separate `x-session-id` for every healthy
subscription and verifies with a first micro-request that the admin-only exact target matched
the backend attribution. The target bypasses only the soft routing reserve; the hard 100%
provider cap, cooling, and auth-dead remain impassable. In legacy/public mode the runner still
tries to place the new sessions via ordinary capacity placement. If not all homes could be
obtained, the load matrix does not start.

## Offline runner verification

```bash
python3 -m unittest tools.claude_calibration.test_run_live
```

The tests cover the budget/rebind guard, exact immutable-event attribution against competing
traffic, fail-closed ambiguity, cost vector integrity, all Claude token classes, tariff
alias/ceiling, the full coverage plan, and the sorting of observed profitability.

## How to read the result

`spent_nano_per_profile` — only the cost of this run's requests in official API equivalent.
It does not equal the client balance charge: the client's discount/multiplier is deliberately
not involved here.

Each `records[]` row contains:

- requested/served model, tier, and token leg;
- exact usage and `actual_nano` from backend evidence;
- the count-tokens worst-case `upper_bound_nano`;
- the observed `fraction_delta_5h` and `fraction_delta_7d` before/after the turn.

`model_profitability` is sorted by `api_nano_per_1pct_5h` descending. `null` does not mean
zero profitability — it means no distinguishable quota movement: the provider fraction is
coarser than this segment. `unavailable_capabilities` separates verified Fast unavailability
on a specific profile from zero spend, and `profile_stops` records a real provider quota
wall/cooling without spilling onto a neighbour. For a commercial conclusion, only rows with a
positive observed delta and a sufficient number of turns are compared; the full-window
`final_capacity.window_totals` remains a pooled realized-workload estimate, not a universal
subscription nominal for any future mix.
