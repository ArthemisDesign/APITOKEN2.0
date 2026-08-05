# Source-context and large-file audit — 2026-08-05

**Subject.** First-party source concentration, files that impose the largest context and navigation
burden on coding agents, and a behavior-preserving decomposition roadmap.

**Measured code snapshot.** `origin/master @ 7b000805a961e7b7aa54f38c4c2bd268bfdc11fa`.
The report was filed from `b0662dc7998115959cd4c4af636b7d6a9b46321d`; no files under
`crates/`, `apps/`, `packages/`, or `deploy/` changed between those revisions.

**Method.** The audit was performed in an isolated read-only worktree. The inventory started from
all 1,428 paths returned by `git ls-files`. The primary ranking excludes documentation, research,
lockfiles, generated/build output, vendored code, migration snapshots, static media, and standalone
tests/scripts. Five parallel scoped reviews then inspected Registry, Forward, server/router, Authbot,
and TypeScript/data hotspots. A final independent review ranked proposed increments by context
reduction, behavioral risk, and reviewability. Physical lines are used as a navigation proxy; they are
not claimed to equal model tokens.

No files were changed and no tests were run during the audit itself.

---

## Executive verdict

The repository's context burden is highly concentrated rather than uniformly distributed. The primary
source set contains 645 files and 317,040 physical lines, but `crates/forward` and `crates/registry`
alone contain 155,147 lines, or 48.9% of the total. All twenty largest primary source files are Rust.

Two different problems are mixed together:

1. large production protocols genuinely combine transactions, locks, cancellation guards, streaming,
   provider semantics, and durable state transitions; and
2. several production entry files also contain thousands of lines of inline tests, making agents load
   test matrices while investigating runtime behavior.

The safest first improvement is therefore not an architectural rewrite. It is to move the inline test
module beginning at `crates/registry/src/pg.rs:7826` into a child module at
`crates/registry/src/pg/tests.rs`, unchanged. This reduces the normal PostgreSQL production navigation
target from 18,165 lines to approximately 7,826 lines: 10,339 lines, or 56.9%, leave the primary file
without changing repository LOC, test names, private access, SQL, migrations, transactions, public APIs,
or runtime behavior.

Test extraction is an initial context-separation wave, not the final architecture. After it, several
production files remain 5,000–8,000 lines and should be split along existing domain seams while keeping
lock, transaction, ownership, and provider boundaries intact.

---

## Source concentration

### By bounded context

| Context | Files | Lines | Share |
|---|---:|---:|---:|
| `crates/forward` | 71 | 97,643 | 30.8% |
| `crates/registry` | 17 | 57,504 | 18.1% |
| `packages/db` | 41 | 27,781 | 8.8% |
| `crates/authbot` | 13 | 22,725 | 7.2% |
| `crates/server` | 9 | 18,806 | 5.9% |
| `apps/web` | 178 | 18,473 | 5.8% |
| `apps/admin` | 66 | 15,787 | 5.0% |
| `crates/router` | 15 | 10,245 | 3.2% |
| `apps/api` | 47 | 7,800 | 2.5% |

At the top level, Rust engine crates account for 217,971 lines (68.8%), applications for 60,729
(19.2%), and shared packages for 38,340 (12.1%).

### Largest primary source files

| Rank | File | Total lines | Approx. production lines | Principal responsibility |
|---:|---|---:|---:|---|
| 1 | `crates/registry/src/pg.rs` | 18,165 | 7,825 | PostgreSQL authority, reservations, settlement, migrations, calibration |
| 2 | `crates/registry/src/lib.rs` | 12,396 | 7,906 | Registry facade and SQLite authority |
| 3 | `crates/server/src/http.rs` | 10,431 | 5,400 | HTTP composition, metrics, capacity and operational projections |
| 4 | `crates/forward/src/billing.rs` | 8,708 | 5,963 | Serialized billing/calibration command actors |
| 5 | `crates/authbot/src/bot.rs` | 7,996 | 6,330 | Telegram purchase and provider-handoff orchestration |
| 6 | `crates/forward/src/gemini/api.rs` | 6,996 | 3,061 | Native Gemini protocol and request lifecycle |
| 7 | `crates/registry/src/pricing/postgres.rs` | 6,650 | 4,824 | PostgreSQL pricing and release persistence |
| 8 | `crates/authbot/src/gemini_oauth.rs` | 5,186 | 3,734 | OAuth, admission, verification and publication |
| 9 | `crates/forward/src/proxy.rs` | 4,887 | ~3,011 | Anthropic forwarding transaction and settlement |
| 10 | `crates/forward/src/codex/api.rs` | 4,693 | 3,379 | Codex Responses HTTP and stream projection |
| 11 | `crates/forward/src/kimi/gateway.rs` | 4,378 | 2,638 | KIMI request, quota, streaming and billing |
| 12 | `crates/authbot/src/db.rs` | 4,299 | 3,064 | Authbot SQLite transactional workflow state |
| 13 | `crates/forward/src/glm/gateway.rs` | 4,262 | 2,377 | GLM request, quota, streaming and billing |
| 14 | `crates/router/src/main.rs` | 4,073 | ~397 | Router composition plus a large integration suite |
| 15 | `packages/db/src/schema.ts` | 3,203 | 3,203 | Commerce Drizzle schema graph |
| 16 | `packages/db/src/pricing.ts` | 2,988 | 2,988 | Pricing, invitations, attribution and ledger synchronization |

The raw ranking would otherwise be distorted by generated migration snapshots, lockfiles, vendored
code, CSS, localized article data, and dashboard JSON. Those are large tracked text artifacts, but they
are not the best executable-logic refactoring targets.

---

## Finding 1 — co-located tests dominate several entry files

The clearest low-risk reductions are files where a trailing child test module is a large fraction of the
physical file:

| File | Approx. test share | Lines removable from primary file |
|---|---:|---:|
| `crates/router/src/main.rs` | 90.3% | 3,676 |
| `crates/registry/src/pg.rs` | 56.9% | 10,339 |
| `crates/forward/src/gemini/api.rs` | 56.2% | 3,934 |
| `crates/server/src/http.rs` | 48.2% | 5,031 |
| `crates/registry/src/lib.rs` | 36.2% | ~4,490 |

Rust child modules preserve access to parent-private helpers. Replacing an inline block with
`#[cfg(test)] mod tests;` and moving its body unchanged therefore avoids widening production visibility
or inventing Cargo integration-test APIs.

The first five moves above remove approximately 27,470 lines from frequently opened production entry
files. Repository LOC does not fall, and broad searches still see the tests; the gain is that targeted
implementation work no longer requires loading them.

### Recommended first increment

Move only the `crates/registry/src/pg.rs:7826` test module:

- replace the inline module with `#[cfg(test)] mod tests;`;
- create `crates/registry/src/pg/tests.rs` with the existing body;
- retain `use super::*`, all test names, fixtures, and private access;
- do not split the new 10,339-line test file in the same change.

This has the largest absolute context reduction at the same mechanical risk as the smaller router or
Gemini moves. Keeping the first extraction verbatim makes review straightforward and establishes the
module-layout pattern before fixture modules are reorganized.

### Follow-up extraction order

Perform one file per review:

1. `crates/router/src/main.rs` → `crates/router/src/tests.rs`;
2. `crates/forward/src/gemini/api.rs` → `crates/forward/src/gemini/api/tests.rs`;
3. `crates/registry/src/lib.rs` → `crates/registry/src/tests.rs`;
4. `crates/server/src/http.rs` → `crates/server/src/http/tests.rs`.

The server move must preserve the suite's process-global dashboard-cache lock and reset behavior. Router
tests should remain a child of the binary crate root rather than move to `crates/router/tests/`, which
would require exposing or duplicating internals.

---

## Finding 2 — test extraction leaves genuine production hotspots

After the mechanical wave, production contexts remain too large for efficient focused navigation:

- `crates/registry/src/lib.rs`: approximately 7,906 lines;
- `crates/registry/src/pg.rs`: approximately 7,825 lines;
- `crates/authbot/src/bot.rs`: approximately 6,330 lines;
- `crates/forward/src/billing.rs`: approximately 5,963 lines;
- `crates/server/src/http.rs`: approximately 5,400 lines;
- `crates/registry/src/pricing/postgres.rs`: approximately 4,824 lines.

These files are large partly because they encode correctness protocols. Splitting must preserve the
lexical and transactional behavior rather than merely chase line counts.

### Registry

- Move provider calibration persistence from approximately `crates/registry/src/pg.rs:4997-6999`,
  with directly owned row mappers, into `crates/registry/src/pg/calibration.rs` as inherent
  `impl PgStore` blocks. Keep event insertion and cumulative subject-spend advancement in one
  transaction.
- Move release-v2 PostgreSQL persistence from approximately
  `crates/registry/src/pricing/postgres.rs:2486-4823` into
  `crates/registry/src/pricing/postgres/release_v2.rs`. Preserve caller-owned transactions,
  `GenericClient` compatibility, advisory-lock ordering, isolation levels, and narrow internal
  re-exports used by `pg.rs` and `stage8.rs`.

Do not introduce a generic SQLite/PostgreSQL backend abstraction. PostgreSQL advisory locks and
isolation levels and SQLite `BEGIN IMMEDIATE` semantics are intentionally different.

### Server and forwarding

- Move exact capacity calculation into `crates/server/src/http/capacity.rs` and re-export
  `capacity_value` so `crates/server/src/poller.rs` keeps one canonical authority.
- Move protected provider JSON projections into `crates/server/src/http/provider_status.rs`, while
  retaining provider-specific implementations internally.
- Extract only pure request normalization and validation into
  `crates/forward/src/gemini/native_request.rs` and pure response/SSE conversion into
  `crates/forward/src/gemini/native_wire.rs`.
- Extract pure Codex event conversion into `crates/forward/src/codex/responses_projection.rs`.

Do not initially move cancellation, stream ownership, profile leases, first-byte transitions, history,
or settlement. `crates/forward/src/proxy.rs` relies on the lexical drop timing of `InflightGuard` and
`HoldGuard`; arbitrary async helper extraction can change money and capacity behavior without a useful
compiler error.

### Authbot and Commerce

- Split Authbot by complete provider or transactional aggregate: `bot/kimi.rs`, `bot/glm.rs`,
  `db/gemini.rs`, and `db/batches.rs`. Do not create a generic provider handoff state machine.
- Extract pure ledger-attribution parsing and validation from approximately
  `packages/db/src/pricing.ts:148-694` into `packages/db/src/pricing-ledger-attribution.ts`, preserving
  exports through `pricing.ts` and keeping `applyPricingLedgerPage` transaction ownership there.
- Split `packages/db/src/schema.ts` only leaf-first while retaining `schema.ts` as the Drizzle entrypoint
  and preserving every exported symbol. This is lower priority because the safest Content Studio leaf
  removes only about 127 lines.

---

## Constraints for all decomposition work

The refactor must not:

- change `registry ← pool ← forward ← server` dependency direction;
- move persistence out of Registry or make Router import engine crates;
- split one reserve, settlement, payment, publication, or ledger transaction into independently
  committed service calls;
- genericize KIMI and GLM gateways or status projections solely because their structure looks similar;
- edit, reorder, regenerate, or consolidate historical migrations;
- change public route behavior, response fields, package subpaths, root symbols, stored representations,
  callback data, or cross-context contracts;
- move private unit suites to Cargo integration tests when that would require wider visibility.

The objective is smaller truthful contexts, not a larger abstraction surface.

---

## Verification plan

For the first Registry test extraction:

```bash
cargo fmt --all -- --check
cargo test -p registry
cargo build
git diff --check
bash deploy/docs-check.sh \
  "$(git rev-parse origin/master)" \
  "$(git rev-parse HEAD)"
```

With a disposable PostgreSQL test database, retain and run the existing fault and provider calibration
matrices under their current `pg::tests::*` names. The final Rust gate remains:

```bash
bash deploy/sccache-cargo.sh cargo test --locked --workspace
```

Router and server moves additionally require their crate tests and the existing router/rotation/universal
smokes. Commerce DB moves require package build, typecheck, unit tests, PostgreSQL integration tests, and
the dependent workspace checks.

---

## Preventing regression

An absolute rule such as “no source file over 1,000 lines” is not recommended. It would grandfather the
current burden, penalize cohesive protocol/state-machine modules, and reward meaningless file splitting.

After the initial extraction wave, add an advisory source-context report that:

- scans tracked first-party source only;
- excludes dependencies, vendor code, generated output, migrations, snapshots, lockfiles, and static
  fixtures;
- reports the largest files by physical lines and bytes;
- shows base-to-target deltas for changed files;
- does not initially fail the gate.

If the metric proves stable, promote it later to a ratchet rather than a universal limit: fail only when a
change increases aggregate excess context burden above the base revision. The per-file report must remain
visible so shrinking one unrelated file cannot conceal a newly growing hotspot. The reporting script and
any gate integration should be a separate operational change with its own regression suite.

---

## Recommended delivery sequence

1. Extract `crates/registry/src/pg.rs` tests unchanged.
2. Extract Router, Gemini API, Registry root, and server HTTP tests one file per review.
3. Split PostgreSQL calibration and release-v2 persistence along the identified transaction-preserving
   seams.
4. Split server projections and pure provider codecs, leaving ownership-bearing orchestration intact.
5. Address Authbot and Commerce hotspots by provider/transactional aggregate.
6. Introduce the advisory context report; consider a ratchet only after observing it on real changes.

This sequence prioritizes immediate context reduction, keeps each diff reviewable, and delays semantic
movement until the repository has already gained the low-risk navigation benefit.
