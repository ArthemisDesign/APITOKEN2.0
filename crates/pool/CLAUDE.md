# crates/pool — CLAUDE.md

**Role:** subscription pool + rotation logic (item 2). Pure in-memory logic.

**Owner branch:** `comp/pool`.

**Boundaries (hard):**
- Depends only on `registry` (the `Sub` type). NO network, HTTP, DB, or env reading.
- Limit polling (network) and forwarding are NOT here — that is `forward`. Ready-made
  utilization values arrive here via `set_util(...)`.

**What lives inside:** `Pool` (RwLock state: subs/live/legacy **bindings** + capacity priors), `Live` (util/reset/
status/cooling/inflight + calibration), `route_affinity`/legacy `route`, `pick` (rotation/
spill), `mark_used`/`mark_ok`/`mark_healthy`/`end_stream`/`mark_cooling`/`cool`, `set_util`,
`set_quota_snapshots`, `record_spend`, `capacity`, `snapshot`. Shared predicates — `select_best`
(rotation) and `place_best`
(capacity-weighted placement) as free functions over `Inner`.

**Cache-first scheduler (lineage → persona):** the shared binding belongs to `forward::AffinityStore`;
the pool knows no Redis/secrets and receives only an opaque preferred home. `peek_affinity_home` gives a
read-only hint before the distributed claim; `route_affinity` revalidates it and atomically occupies the slot:
- **pin** — a fresh binding to a healthy home → the same subscription (cache is warm, single-user pattern);
- **spill** — the home is "still ours" (cache not cold, no deep ban, under the cap) but temporarily busy
  (burst-cooling < `REBIND_AFTER` or `inflight ≥ MAX_INFLIGHT`) → the request immediately goes to a less
  loaded profile via `select_best`, **the binding is preserved**; if the healthy home is the only one,
  load-threshold fail-open keeps the request on it with no waiting/rejection;
- **(re)binding** — no home / home is dead, at ≥100% hard cap or deeply unavailable →
  `place_best`: capacity-weighted placement (max free USD capacity), where `MAX_INFLIGHT` is only
  a soft routing threshold; if the whole fleet is above it, selection continues by minimum in-flight.

A shared cache-root is NOT a lineage: `peek_affinity_home_with_warm` receives several opaque warm homes,
warms at least two competing homes (if the second has ≥70% of the best's free capacity), then
prefers the best warm one. A much freer cold persona wins and itself becomes warm
after a successful response. This is a soft hint; all the usual health/reserve/inflight checks stay the same.

`route_operator_target` is a narrow pure-memory path for forwarding-admin live calibration: the caller
passes a bounded opaque/profile hint and an identifier function, a collision returns `None`. The path
bypasses only the soft new-placement `Reserve`, but not the hard 100% cap, cooling or durable auth-dead;
it is never spilled/rebound. HTTP/auth/headers remain the responsibility of `forward`.

Continuation uses the hard 100% provider cap, while a new placement uses the soft `Reserve`; a busy home
spills immediately, with no local waiting. Legacy `route(u64)` and bounded `bindings`
are kept for compatibility with tests/internal callers. The pool has no network, env or persistence.

**Persistence (surviving a restart):** `export_state`/`import_state` (via `registry::PoolStateRow`)
carry the durable state — cooling (a multi-day ban is not forgotten on deploy), capacity calibration,
spent/util/reset. `import_state` restores cooling only if it is still in the future, and seeds the
calibration anchors with the restored point. The write-through trigger is the `set_on_change` hook (opaque `Fn`, WITHOUT
tokio/DB — layer purity): it is called on cooling transitions (`mark_cooling`/`cool`); the server hangs a
persistence poke on it. Calibration does NOT trigger the hook (too frequent) — it rides on cooling flushes plus the server's safety flush.

**Per-subscription window reserve (`Reserve`, headroom + anti-fingerprint):** we do not route above `1 − base`
(defaults: 5h=10%, 7d=3%) — we leave a buffer so the window never reaches 100% (fewer 429s, and we don't
look like a bot maxing the quota to zero). The cutoff threshold is **jittered deterministically by email**
(`Reserve::caps`) → the whole fleet does NOT cut off at the same percent (that itself would be a
distinguishing mark), and stays stable over time per subscription. `select_best`/`place_best`/`route` compare eff_util against the per-window
per-email ceiling; the "don't stall" relaxation uses `Reserve::FULL` (under peak we'd rather reach 100%
than hand the client a 429). Tuned via env `CLAUDE_API_RESERVE_5H/7D/JITTER`; in tests — jitter=0 for determinism.

**In-flight = an observed counter for the whole life of a stream:** `mark_used` (+1 on pick) → on success
`mark_healthy` (clear cooling, do NOT touch in-flight) → `end_stream` (−1) from forward's tee metering on
stream completion/abort. 4xx → `mark_ok` (−1 immediately). This way `place_best`/`select_best` see the persona's real
parallel load (not "0 right after the headers") → they don't pile extra streams onto
one account (both a limit risk and an anomaly). This is not an admission cap: all candidates above the soft
threshold are selected fail-open and continue in parallel.

**Durable auth-health (detecting a banned token — Claude bans subscriptions):** `AuthState`
(`Healthy`→`Suspect`→`Dead`) on `Live`, persisted in `subs.auth_state` (survives restart/blue-green —
unlike the old ephemeral `auth_dead` bit, which the poller lost on every deploy). The machine is
the pure `apply_probe(l, http, fp, now)` (deterministic in `now` → testable): a single 401/403 is NOT
a verdict (could be a client's broken request / a transient), it takes a streak of `DEAD_STREAK`(2) clean probes over
≥`DEAD_MIN_SECS`(5min) — "2 calls in 5 minutes" (a clean probe without client input → 401 is unambiguous).
`Dead` is EXCLUDED from `route`/`pick`/`place_best` (a guaranteed 401 to the client
is worse than a transparent Overloaded from forward). Public API: `record_probe` (called by the poller; returns
`registry::SubHealth` ONLY on change → the server persists it owner-fenced), `import_health` (start from
the DB), `revive`/`kill` (operator), `is_auth_dead`, `token_fp`. Auto-revive: token change (`token_fp`
changed → `replace_subs`/`apply_probe` reset the verdict) or a successful slow resurrection probe.
`replace_subs` simultaneously removes token-scoped ephemeral quota snapshots so the new credential
does not inherit the old one's current utilization.
`403`→`permission_error` (ban), `401`→`authentication_error` (re-auth) in `dead_reason`. Live 2xx traffic
via `mark_healthy` clears suspicion IN MEMORY (the nearest clean probe fixes the durable state). The liveness
verdict is issued ONLY by the poller with a clean probe — forward on a live 401/403 merely `request_probe`s, it does NOT kill.

**Capacity calibration (USD real-API) and availability:**
- Anthropic does NOT give the absolute window size — only the share (util) and reset. We compute the absolute:
  `record_spend` accumulates monotone real spend; `set_util` on each header calibrates
  `cap = ΔUSD/Δutil` (EMA) — the window anchors move only when Δutil has outgrown the threshold (small
  steps accumulate, nothing is lost). Before the first calibration — a prior matching Max 20x (env-tunable).
  Response headers arrive before the stream settlement, so at `inflight > 1` an observation only
  re-anchors the window and does NOT calibrate. Any sample must be plausible relative to the tariff
  prior and explained by our spend; the first accepted sample also enters the EMA on top of the prior
  rather than replacing it entirely. At startup an implausible legacy cap is quarantined to `0` (meaning: use the
  prior again), `calib_n` is reset, and the server immediately persists the repair with an owner-fenced CAS write.
- **Prior by plan:** before the first calibration the capacity prior is scaled by `Sub.plan`
  (`plan_scale`: max20=1.0, max5=0.25, pro=0.05) — otherwise Pro/Max5 subscriptions would be overrated as
  Max20 and overfilled with traffic → 429 storms. Calibration from real spend then refines it.
- `capacity()` is pure math: "live" util = header + spend_since_then/cap (rollover
  after reset); window remainder = cap·(1−util); availability over horizon H (quota, not rate — the window
  refills): `min(rem5h + n5·cap5h, rem7d + n7·cap7d)`, n = number of resets in (now, now+H].
  Summed over the fleet for any number of subscriptions. The `calibrated` flag = whether a real calibration
  has happened; per-sub `Cap` carries the same safe `plan` as the source `Sub`, so protected reports
  group the measurement without an ambiguous join on masked email.
- `set_quota_snapshots` separately stores the exact fixed-point 5h/7d fraction, resolution, observed time
  and optional reset from response/count-tokens. This is ephemeral current-supply evidence for the server:
  a missing reset does not prevent computing the current remainder via durable cohort capacity, but such a
  snapshot does not calibrate the legacy estimator, is not persisted, and does not prove future horizon resets.

**Selection-logic invariants:**
- Limit windows are Anthropic's source of truth (unified headers). We do NOT compute different 5h/7d
  reference points: each subscription reports its own util+reset. We only consume them.
- **Rollover:** in `pick` the effective utilization = 0 if `now ≥ reset` (the window has already reset),
  even if the poller hasn't refreshed the number yet. Filtering/sorting go by `eff5/eff7`, not raw util.
- `pick`: non-cooling first, then those under the `util_cap` ceiling; sorting
  **eff7 → eff5 → warn(allowed<warning) → inflight → LRU**. Strategy: protect the weekly (7d)
  budget, spread out the 5h one.
- Filters relax gradually (if empty — take the least hot one); the pool NEVER "stalls".
- `mark_used` = +1 in-flight (fan of parallel streams); `mark_ok`/`mark_cooling` = −1 (clamped at 0).
  `cool` — cooling WITHOUT touching in-flight (for the background poller, which did not call `mark_used`).
- `now()` — the single time source for the crate.

**Verification:** `cargo build -p pool`.
