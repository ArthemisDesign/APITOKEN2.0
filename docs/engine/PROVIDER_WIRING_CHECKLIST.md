# Provider wiring checklist — mechanical map

`docs/engine/PROVIDER_ONBOARDING.md` answers the question **what must be proven**. This file answers
the question **what exactly to edit**: exact paths, exact symbols, order, and traps, each of which
actually fired while wiring KIMI (`docs/engine/KIMI_PROVIDER.md`).

Read both. Principles without the map give a correct but slow traversal; the map without principles
gives a fast but wrong provider.

## 0. The completeness rule

The result is a working provider, not green tests. A passed stage, a merged commit, and
a button that appeared are progress, but not readiness. Do not hand control back until the
terminal state is reached: an operator completes a purchase through Auth Bot, the resulting profile
becomes routable capacity, traffic is metered at the official price, and calibration writes evidence
from real quota movement. The only legitimate earlier stop is the absence of a live subscription:
in that case, finish everything that can be done without it, and name the exact blocked live gate.

**The plan lives on disk, not in context.** A long wiring outlives any context window, and
a plan that exists only in the conversation disappears at the first reset — this is the mechanical
reason an agent abandons the chain halfway. Maintain
`research/<PROVIDER>_PLANE_PROGRESS.md` and after any reset read **it first** — do not
reconstruct the plan from memory. It contains: the terminal state, a table of what is done **with
a SHA on every row**, open seams in plain text, exactly one concrete next action, the queue, and
what is blocked by a human. Update it in the same commit as the step itself: a separate
"commit about progress" is stale the moment it is written, and a "done" row without a SHA is
unverifiable.

**This does not mean "one big merge".** The schema and its first reader never ride together: the
migration goes alone and waits for green `deploy/migration` and `deploy/watchdog`. Drive the merge
chain yourself, but do not collapse it — an agent that merged the schema together with its reader
did not speed up; it removed the rollback path.

Two states that look like readiness and are not:

- **A seam.** Product buttons with no handler that completes the deal; a sealed profile with
  no plane reading the roster. The provider looks wired and delivers nothing. If a change
  creates such a seam — write about it in the commit body and close it in the next one.
- **"Tests are green".** Mocks prove the guards, but not the provider's contract. Explicitly name
  preview versus GA and list every `unknown` that blocks its own live gate.

## 1. Before opening the first file

1. **Commit within the first few minutes.** A worktree without commits looks like `clean+merged`,
   and the persistent `DELETE_WORKTREE` (`docs/ops/DELETE_WORKTREE.md`) deletes it together with
   the uncommitted files. During the KIMI wiring this happened twice. The first commit is
   the capability manifest skeleton, even if empty.
2. **Research first, then design.** Price aggregators contradict each other and the provider.
   The tariff is taken only from a provider-owned page, with a review date. Record the public
   release date from a provider-owned source separately: it must travel through model discovery,
   and must not be replaced by a pricing epoch, knowledge cutoff, repository publication date, or
   a date parsed from the model ID.
3. **Check whether the provider has an official OSS client.** It shows real endpoints
   missing from the documentation. With KIMI, that is exactly how `/usages`, `/me`, the device
   flow, and the client id were found. Clone read-only into `mktemp -d`, read with `rg`, run
   nothing, delete after the research, and record URL + full SHA + license in the manifest.

## 2. Delivery order

Strictly producer-first, each item a separate commit:

```
manifest → metering → migration → credential → estimator → authbot → runtime → server → live
```

The migration goes **alone**, and dependent code merges only after green `deploy/migration` and
`deploy/watchdog` on its SHA.

## 3. Metering — `crates/metering`

| Action | Location |
|---|---|
| New module | `src/<provider>.rs` |
| Declaration | `src/lib.rs`: `pub mod <provider>;` |
| Re-export | `src/lib.rs`: the `pub use <provider>::{…};` block |

Mandatory:

- `<PROVIDER>_TARIFF_SCHEDULE_ID` with a review date; epochs with `effective_from`, the first being
  `0`.
- Rates are `$/M × 1000` in integer nanoUSD, fields `i128`.
- Legs do not overlap. Subset counters (reasoning, tool prompt) are **not** priced
  separately and have an explicit invariant.
- An unknown or withdrawn model is absent from the catalog → `None` → fail closed before reserve.
- Arithmetic is `checked_*`, not `saturating_*`; overflow is a typed error.

**The "served ≠ requested" trap.** If the provider can silently move to another model (with KIMI,
disabled thinking moves K3 and K2.7 Code onto K2.6 with different pricing) — the tariff must be
taken from the **served** model in the response. Create `<provider>_prices_for_served_model`. The
test must show the price of the error in numbers.

**The "no price for a leg" trap.** The absence of a published rate (KIMI has no cache-write) is no
reason to silently count zero. Create an explicit field with a conservative value and a test that
pins it down.

Gate: `cargo test -p metering` **in full** — the crate handles money; a subset will not do.

## 4. Migration — `crates/registry`

| Action | Location |
|---|---|
| File | `migrations_pg/00NN_<provider>_window_calibration.sql` |
| `const MIGRATION_00NN` | `src/pg.rs` (next to the previous ones) |
| `CURRENT_SCHEMA_VERSION` | `src/pg.rs` — bump it |
| Row in `ENGINE_MIGRATIONS` | `src/pg.rs` |
| `INSERT INTO engine_schema_migrations(version) VALUES (NN)` | the tail of the SQL itself |

**The "extend or place alongside" decision.** Before adding the provider to
`provider_turn_calibration_events` (migration 0019), check the durable identity. If it lacks
a dimension — paid plan, the requested/served pair, provider bucket, duration, tariff schedule —
build a **new authority alongside** and do not touch the old one. For KIMI two were missing at
once: a single `model_id` column and the absence of a plan.

The schema must contain: immutable turn events, cumulative subject spend, immutable quota
observations, and estimator state. `Unknown` is `NULL`, never `0`. The cold state and the measured
state are separated by a `CHECK`.

**Traps in migration tests:**

- Before the "does not touch someone else's table" check, **strip `--` comments**: the header
  usually explains why the migration sits next to that table and mentions its name.
- `std::ptr::eq` on a `const` is unreliable — constants are inlined at every use site. Compare
  the contents: `assert_eq!(registered, Some(MIGRATION_00NN))`.

## 5. Observation types — `crates/registry/src/<provider>_calibration.rs`

Declaration and re-export — `src/lib.rs` (`mod` next to `provider_calibration`, `pub use …::*`).

**The shape of quota determines everything else:**

| The provider publishes | The estimator model |
|---|---|
| a fraction (percent) | Claude-like: `capacity = SCALE·ΣΔspend / ΣΔfraction` |
| a fraction + native consumption **per turn** | GPT-like dual-ledger, two independent ledgers |
| integer `used`/`limit` per window | Claude-like, but `limit` already gives exact native capacity |

Do not confuse the third case with the second. A per-turn native ledger exists only if the provider
returns native consumption in every response. A window aggregate is not that.

`measurement_resolution` is derived from the provider's **real denominator**
(`ceil(SCALE / limit)`), not set as a constant. Storage in a wide integer does not create precision.

An unknown window time unit — fail closed: a wrong duration would merge two independent
windows into one row.

## 6. Credential — `crates/<provider>-credential`

| Action | Location |
|---|---|
| Crate | `crates/<provider>-credential/{Cargo.toml,src/lib.rs,CLAUDE.md}` |
| Member | the root `Cargo.toml`, the `members` list |

Mandatory: versioned XChaCha20-Poly1305, explicit `kid`, the AAD binds profile id **and** the
credential kind, the keyring reads old keys, a manual `Debug` with `REDACTED` + a test for absence
of secrets, profile id boundaries (no `/`, `..`), bounded fields.

**Rotating refresh family.** If the provider issues a new refresh token on every exchange (KIMI,
Codex) — `rotate()` must reject a response without a new token, and the caller must hold a
per-profile single-flight from refresh through the re-`seal`.

**The unverified plan ladder.** Do not invent which tier unlocks what. Create
`<PROVIDER>_REVIEWED_PLANS` **empty** and give an unverified plan only those capabilities
documented for everyone. A row is added together with a dated live observation.

## 7. Estimator — `crates/forward/src/<provider>_calibration.rs`

Declaration — `crates/forward/src/lib.rs`, **in alphabetical order** within the `mod` list.

The interval state machine must cover: anchor, first complete interval, quota-before-
settlement (hold the anchor once), repeated quota-only movement → `unattributed`, reset,
rolling rollover, jitter, rollback to an old high-water, stale observations, independent
durations, rebuild from immutable history, estimator version change, overflow.

Prior/EMA/WLS/subscription nominal/float money — forbidden.

## 8. Auth Bot — `crates/authbot`

Order of edits, exactly this:

1. **Protocol module** `src/<provider>_oauth.rs` + `mod` in `src/main.rs` + the credential-crate
   dependency in `Cargo.toml`. While nothing calls it — add `#![allow(dead_code)]`
   with an explanation, otherwise the gate will go noisy.
2. **`HandoffKind`** in `src/bot.rs` — add the variant **first of all**. The compiler itself will
   show every place that needs a branch (for KIMI there were exactly three). That is the fail-closed:
   without the variant, the new product would silently have gone down the Claude setup-token path
   and burned a paid subscription.
3. **`handoff_kind()`** — place the provider's rule **above** the others if the plan names are
   generic. For KIMI these are Andante/Moderato/Allegretto: any neighboring substring rule could
   intercept them. The test must verify that a bare tier name is **not** classified.
4. **`tier_name()`**, **`product_kb()`**, **`admin_quick_tier()`**, **`admin_home_kb()`** — codes
   and buttons. The batch menu usually picks up the list by itself.
5. **Seller texts**: `<PROVIDER>_OFFER_GUIDE`, `<PROVIDER>_ACCOUNT_SETUP`,
   `<PROVIDER>_PROXY_PROMPT` + rows in `seller_offer_guide()`, `account_setup_prompt()`,
   `proxy_prompt()`, `accepted_next_step()`.
6. **Wizard steps**: `handoff_steps_for_kind()` — a pair of unique ids (`km_proxy`/`km_ready`).
   An id shared with another provider would let one deal's callback advance another deal.
7. **Readiness handler** — issuing the device code/link, polling, `/me`, seal, atomic
   roster publication, and only then payout completion.

**The menu-test trap.** `every_product_button_resolves_and_classifies` and
`batch_product_menu_covers_every_subscription_variant` are exhaustive: they have a hard counter and
a closed `matches!`. Update both, or the build goes red.

**The seller-text trap.** The word "password" legitimately appears in the explanation of proxy
fields. The "bot does not ask for secrets" test must check request phrasings ("send the password"),
not the word itself — otherwise it catches its own instructions.

The seller never transmits a password, 2FA, cookie, token, or proxy URL. A failed, expired,
or wrong-plan flow leaves neither a credential file nor a roster row and does not complete the
payout.

## 9. Runtime, server, observability

- `crates/forward/src/<provider>/` — transport, pool, stream, billing. If the provider is
  Anthropic-compatible (like KIMI), reuse the native path: a translation layer of the size of
  `gemini/` is not needed.
- `crates/server/src/config.rs` — the **only** place env is read.
- Readiness: **never** check health with an ungated endpoint. With KIMI, `/v1/models` returns
  200 for an invalid key, and generation then gives a 403 — the probe must hit `/messages` or
  `/me`.
- **A subscription rests only on a provider verdict — never on our own inference.** Every pool has
  made this mistake at least once, so it is a checklist item and not a matter of taste:
  - Split cooling into a **hard** axis (the provider said so: 429/`Retry-After`, an explicit quota
    catalogue zero, an explicit "this subscription does not exist") and a **soft** axis
    (auth 401/403 after a successful refresh, transport faults, timeouts, a failed probe).
  - Only the hard axis may deny a request. When normal selection returns nothing, re-select
    ignoring the soft axis, bounded by the already-tried set so each subscription is attempted at
    most once per request. An empty pool must mean real limits, and then the honest answer is a
    429 with `Retry-After` — never a 503 invented from an environmental guess.
  - The soft axis backs off exponentially from a small base (~15 s) to the quarantine ceiling and
    resets on any proven success. Flat, long cooling is what turns one bad wave into a total
    outage.
  - Removing a subscription from the authenticated/routable set is a **terminal verdict** and needs
    an unambiguous provider statement (Google: `invalid_grant` on refresh) or corroboration by both
    a streak and elapsed time (Claude `DEAD_STREAK`/`DEAD_MIN_SECS`, Codex
    `AUTH_DEAD_STREAK`/`AUTH_DEAD_MIN_SECS`). One 401/403 is never a verdict — it may belong to the
    client's request, which also makes blanket cooling a DoS: one crafted request would rest the
    whole fleet.
  - An exhausted selection must wake the health sweep out of band (debounced, ~15 s), so recovery
    is bounded by the probe rather than the background cadence.
  - Cover it with tests that state the invariant: a full-soft-cooling pool still serves; a
    full-hard-cooling pool answers 429; a generation 401/403 does not de-authenticate.
  Reference implementations: `crates/pool` (Claude, strictest — a live 401/403 only calls
  `request_probe`), `crates/forward/src/codex/health.rs`, `crates/forward/src/gemini/pool.rs`.
- Metrics of fixed cardinality only; no profile id, no email, no prompt, no provider error
  text.
- An alert and its same-named section in `docs/ops/MONITORING.md` are added in a single change —
  `deploy/monitoring-config.test.sh` checks this.

## 10. Documentation — in the same commit

- `docs/engine/<PROVIDER>_PROVIDER.md` — the capability manifest with evidence labels.
- A row in `docs/README.md`.
- `crates/<name>/CLAUDE.md` for every new or substantially changed crate.
- `docs/DEPENDENCIES.md` — on a new cross-context connection.
- A "delivery state" section in the manifest: what is done and what is **not** done. Green tests
  read as "ready" unless the opposite is written explicitly.

## 11. Gates before the merge

```bash
cargo build
cargo test -p metering                     # in full, if you touched money
cargo test --locked --workspace
git diff --check origin/master...HEAD
bash -n deploy/*.sh
bash deploy/docs-check.sh "$(git rev-parse origin/master)" "$(git rev-parse HEAD)"
```

Merge — only `git push -u origin HEAD` && `./deploy/agent-merge.sh`, from your own worktree.

## 12. What must not be declared ready

A green build, a mock 200, a model row in the catalog, and a plausible number in the admin panel
are not evidence. GA requires live evidence for every published
plan × model × tier row. Until then the state is called preview, and the open questions are
listed in the manifest — one item per blocked live gate.
