# Pricing release backfill — retiring existing accounts to the direct strict path (phase 2.2)

The runbook for moving the PRE-EXISTING account fleet off the release pricing path. New
accounts graduate at registration (phase 2.1, `docs/commerce/PRICING.md`); this lane handles
everyone who already existed: commerce B2C/B2B accounts via the `apps/worker` arm lane, and
pre-existing OpenKeys engine accounts via the admin-triggered sweep in `apps/openkeys`.
Service accounts are intentionally OUT OF SCOPE — see the last section.

## How the commerce lane works

One slow-sweep pass of the pricing worker (`flushPricingBackfill` in
`apps/worker/src/pricing-worker.service.ts`, canonical module
`packages/db/src/pricing-backfill.ts`) takes up to `PRICING_BACKFILL_BATCH_SIZE` candidates
and advances each independently:

1. **Align the dormant scalar** — the account's live release policy is resolved first and
   the rule-less-scope fallback is DERIVED from it (`deriveReleaseFallbackBp`: the global
   rule's payable — B2C 5000 by engine validation — else full price; release B2B policies
   cannot carry a global rule, so 10000 is the only honest strict fallback). The engine
   scalar is then written idempotently via `account_set_mult_bp` and the commerce mirrors
   (`customer_profiles.multiplier_bp`, `engine_accounts.mult_bp`) are synced in the
   materialization transaction. This cannot change current billing: the release reserve
   takes its multiplier from the release resolution and never reads `accounts.mult_bp`
   (verified in `crates/forward/src/proxy.rs` — `resolution.payable_multiplier_bp()`); the
   scalar matters only after the opt-out, where the strict/policy_v1 admission applies it
   to scopes with no matching policy rule (e.g. glm/kimi for a strict policy whose provider
   rules cover anthropic/google/openai only). The legacy `engine_pricing_jobs` stream is
   deliberately NOT used for this write: `claimNextPricingJob` drains scalar jobs for
   strict-bound accounts without an engine write — this lane is itself the durable,
   resumable driver and re-asserts the value in step 3.
2. **Materialize** — the account's managed policy is re-pinned and materialized at the live
   catalog head through the same writer registration uses
   (`materializeProvisionedUserPolicy` with arming disabled): B2C takes the current head of
   `policy:main:global-b2c`, B2B the account's own `b2b_client` policy with per-model/provider
   scopes preserved exactly. Already-strict bindings skip this step.
3. **Equivalence** — before anything is armed, the account's release-side resolution
   (assignment extension over base, pinned policy, model → provider → global precedence —
   the engine's own resolution order) must resolve every scope to exactly the payable
   multiplier the strict policy charges, AND the engine scalar must observably equal the
   aligned fallback (`getAccount` read-back) — the proof that the alignment landed before
   the chain proceeds. B2C is the 5000-global identity; B2B is exact
   scope-set equality (both sides were built from the same `pricing_policy_rules` source; the
   comparison walks normalized scope→payable maps, never the stored digests — they live in
   different frozen digest domains). A mismatch is never forced: the account keeps its
   release coverage, the reason lands on the binding's `last_error`, and the next pass
   re-evaluates, so an operator fix is picked up automatically. The scope-walk is skipped
   ONLY for accounts with no release coverage at all under the active head (no extension,
   no base assignment — the broken-window cohort registered between the extension removal
   and the backfill; the release resolver already fails closed on them today, so there is
   nothing to diverge from): they proceed like phase-2.1 new accounts, and the worker logs
   an explicit `no release coverage; direct path only` line per account. A
   present-but-divergent assignment still blocks.
4. **Arm** — `strict_chain_pending` is armed and the UNMODIFIED new-account strict chain
   (`packages/db/src/strict-chain.ts`, fast tick) takes over: shared engine preflight,
   durable strict staging once the shadow delivery confirms, then the one-way engine opt-out
   marker. The opt-out disarms the flag and writes the durable `pricing_release.opt_out`
   `audit_log` entry — the terminal "done" that removes the account from every future sweep.
   Arming happens ONLY for a verifiable binding: the strict chain (and the engine's own
   triggers) require `reconciliation_state='verified'`, and nothing in the live system flips
   'pending' → 'verified' any more (the shadow-rollout lane that did it was deleted with the
   release orchestration). The backfill therefore performs that verification account-locally
   right before arming — the durable ACK proof the rollout used (`sync_state='confirmed'`,
   desired=applied with matching digests, `last_ack_at` present), cross-checked against the
   engine's active policy state (version + digest) — and an unconverged binding
   (desired≠applied, or a cross-check mismatch) is left un-armed and quiet, rotating to the
   back of the queue until the delivery lane converges it. Accounts that can never advance
   inside the lane (today: no managed pricing policy to materialize) are marked
   `terminal: …` in `last_error` ONCE and excluded from every future pass — the hot-loop
   guard; they stay visible in `pipeline-health` (`pricing_backfill.failed`), and an
   operator repair clears the marker (`last_error = NULL`) to return the account to the
   sweep.

   The sweep also covers the ARMED legacy cohort: accounts armed during the
   pre-verification wave (`strict_chain_pending` already true) stuck in
   `reconciliation_state='pending'` — the chain owns them but structurally cannot advance
   them. These are selected too (armed + pending only; armed + verified stays exclusively
   with the fast lane) and get the SAME idempotent steps: materialize — including the
   re-pin to the live catalog head when the binding's pinned catalog generation is stale
   (the engine's strict activation rejects those with a `missing_dependency` on the old
   pin; the re-materialization writes a new desired version at the live head and the
   delivery lane converges it first, quietly) — then the account-local verification with
   the engine cross-check. A verified armed account is handed BACK to the chain, never
   re-armed: the arm write is idempotent by construction, and the sweep reports it as
   `pending` (waiting on the chain), not `armed`. The same rescue applies to ARMED +
   `sync_state='pending'` accounts (strict delivery never confirmed): the materialize step
   runs for strict bindings too — the reuse branch is a no-op when the pinned catalog
   generation is current (no new version, no churn), and re-pins to the live head exactly
   when the pin is stale; the release-covered equivalence gate still applies to them.

New accounts need no alignment pass: B2C registrations are born with `mult_bp=5000`, and B2B
invitee registrations now start at the full-price fallback (10000) — the negotiated discount
lives only in the copied policy rules, so a scope outside them never inherits the negotiated
rate after the strict opt-out (registration: `packages/db/src/auth.ts`,
`packages/db/src/oauth.ts`; the neutral 10000 placeholder matches `rotateBusinessInvite`).

In-flight release reservations are not special-cased: the migration-0016 strict trigger
rejects the engine-side flip while they drain, which surfaces as a retryable 503 in the
control-job lane's exponential backoff — calm retries, never a hot loop, never a dead job.

## Knobs (apps/worker env)

- `PRICING_BACKFILL_ENABLED` (default `true`) — master switch for the arm lane. Flipping to
  `false` halts NEW arming instantly; already-armed chains still finish (the chain flush is
  independent), which is the desired drain behavior.
- `PRICING_BACKFILL_BATCH_SIZE` (default `5`, max 100) — max accounts armed per slow-sweep
  pass (pass cadence = `PRICING_POLL_MS`, default 60s).
- `PRICING_BACKFILL_ACCOUNT_ALLOWLIST` (default empty) — comma-separated engine account ids.
  Non-empty = canary mode: ONLY the listed accounts are ever armed.

## Canary sequence (production)

1. Deploy with `PRICING_BACKFILL_ENABLED=true`,
   `PRICING_BACKFILL_ACCOUNT_ALLOWLIST=acct_<internal_1>,acct_<internal_2>` and
   `PRICING_BACKFILL_BATCH_SIZE=2`. Nothing outside the list can move.
2. Watch the two internal accounts through to done: worker log lines
   `pricing backfill armed the direct strict chain for acct_…` →
   `strict chain completed for <userId>: pricing release opt-out marker applied`, and the
   `pricing_backfill` section of `GET /v1/admin/pipeline-health`
   (`pending`/`in_flight`/`done`/`failed` counts plus the five most recent failures).
3. Widen the allowlist to a second small cohort (a real B2C account, a B2B account with
   model-scoped rules). Repeat the watch. A `failed` count with
   `… resolves to N bp …` in `last_error` means a genuine pricing divergence between the
   release policy and the strict policy — investigate the account, fix the divergence
   (usually a stale assignment extension), and the lane re-checks it on the next pass.
   Do NOT force accounts through. A legacy scalar mismatch
   (`effective multiplier is 4000 bp …`) needs no action: the lane aligns the dormant scalar
   to the release-derived fallback itself and the account passes on a later pass — only a
   mismatch that persists after the alignment is a real divergence.
4. Clear `PRICING_BACKFILL_ACCOUNT_ALLOWLIST` (empty = full eligible sweep) and raise
   `PRICING_BACKFILL_BATCH_SIZE` gradually (5 → 25). At ~173 eligible bindings the full
   sweep drains in tens of passes; completion is `pending = 0, in_flight = 0, failed = 0`.
5. Leave the lane ON after the sweep: it is the standing safety net for any account that
   ever lands outside the registration chain (repairs, imports). An eligible set of zero
   makes every pass a no-op.

Rollback at any point: set `PRICING_BACKFILL_ENABLED=false`. Accounts already opted out stay
opted out (the marker is one-way by design); armed-but-not-finished accounts finish their
chain; everyone else simply keeps release coverage.

## OpenKeys sweep

Pre-existing OpenKeys engine accounts (owned in `openkeys_keys`, never service/meter-only by
construction) are backfilled by the internal admin endpoint, one bounded batch per call:

```
POST /openkeys-admin/api/internal/admin/strict-backfill
{ "limit": 5, "account_ids": ["acct_…", "acct_…"] }
```

(`account_ids` is the canary list; omit it for the warehouse-order sweep. Same Caddy
managed-admin perimeter as the other `/api/internal/admin/*` routes.) Per account the sweep
proves the `openkeys` funding class, runs the shared strict-cutover preflight, activates the
deterministic official 1:1 strict policy by exact CAS over the observed active policy,
re-stamps active keys on the new head, and writes the one-way opt-out marker. Every step is
idempotent (already-official accounts skip straight to the opt-out, which replays as
`unchanged`), one account's failure never blocks the batch, and a drain-blocked flip
(engine 503) is simply retried by the next call. The response carries the per-account
outcome (`opted_out`/`skipped`/`failed` with the reason). Suggested sequence: one internal
warehouse account by `account_ids`, then `limit` 5 → 25 until a batch returns only
`opted_out`/`skipped` with `candidates` below the limit.

## Service accounts stay on release — intentionally

Service accounts (`billing_mode=meter_only`) are excluded everywhere in this phase: the
engine has no meter-only lane outside release-v2, so there is no strict state to move them
to. The commerce lane excludes them twice (the binding identity CHECK never gives a service
binding a `user_id`, and candidates are additionally probed against
`service_account_inventory_v2`); the OpenKeys sweep requires the engine funding snapshot to
prove `account_class='openkeys'` before any mutation. They remain on the release path until
the engine meter-only lane lands (phase 3) — `ensureServicePricingReleaseProvisioningV2`
keeps completing their exact `meter_only` policy/extension as today.
