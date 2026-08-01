# Stage 6 funding reconciliation

Stage 6 adds a deterministic, engine-owned migration from the legacy aggregate account balance to
`funding_buckets`. It does not change live reserve, settlement, top-up, admission, or refund
behavior. Production application therefore belongs to a drained maintenance window immediately
before the later shadow/strict funding rollout; applying the plan while legacy money writers keep
running would make the buckets stale.

## Classification contract

The source policy is compiled into the engine and included in every content-addressed plan:

- `cryptomus:` and `platega:` are paid only for reviewed B2C/B2B accounts;
- `openkeys:` is paid only for reviewed OpenKeys accounts;
- one exact `signup-bonus:*` credit of `4_000_000_000` nanoUSD is a B2C
  `welcome_track_bonus`;
- every other positive credit is `legacy_restricted`, never paid;
- known charges replay chronologically, consuming free lots before paid funds.

An opening balance, a positive ledger gap, or an unknown positive credit is preserved in a
restricted bucket and leaves the account in `reconciliation_state=exception`. A negative gap,
adjustment, malformed welcome credit, missing `balance_after_nano`, or unsupported ledger row is
not guessed: the planner conservatively quarantines the current non-negative balance and reports a
typed exception. Accounts with live reservations, no reviewed Stage 5 binding, or conflicting
existing buckets are blocked. They receive no new buckets.

For every applicable account the planned bucket balance sum must equal `accounts.balance_nano`
exactly. Negative balances can exist only in the paid bucket and remain non-spendable. No floating
point arithmetic is used.

## Read-only plan

Run against the engine PostgreSQL authority:

```bash
claude-api db plan-funding-reconciliation > funding-plan.json
```

The command uses a serializable read-only transaction and prints:

- the immutable source-policy digest;
- one source-state and account-plan digest per account;
- exact bucket rows and the target reconciliation state;
- typed issues and ready/exception/blocked/replay counts;
- a digest of the whole plan.

Review the complete report. A missing protected B2B/OpenKeys/service policy binding is a blocker,
not an invitation to infer the account class. Re-run the plan after every remediation; never edit
the JSON or reuse its digest after money state changes.

## Apply

Prerequisites:

1. The Stage 5 assignment matrix is reviewed and fully applied.
2. Legacy money writers are stopped and all reservations/outbox work is drained.
3. The latest read-only plan is reviewed and its exact `plan_digest` is approved.
4. A rollback backup and the policy-capable deployment floor are verified.

Safe apply (all accounts must be ready or an exact replay):

```bash
claude-api db apply-funding-reconciliation --plan-digest 'sha256:...'
```

If the reviewed report intentionally contains restricted/blocked accounts, partial application
requires the explicit `--allow-exceptions` flag. Applicable exception accounts receive conservative
buckets and remain `exception`; blocked accounts remain outside strict mode. The engine recomputes
the entire plan under a serializable transaction and advisory lock. Any balance, ledger, binding,
reservation, or existing-bucket drift changes the digest and aborts before mutation. Inserts do not
ignore conflicts. Exact replays insert no duplicate buckets.

After application, verify per-account bucket sums, zero outstanding reservations, exception count,
and a fresh plan consisting only of expected `replay` rows. Do not resume legacy money traffic with
these buckets treated as authoritative; Stage 8 shadow writers/readiness and Stage 9 strict funding
enforcement complete that cutover.

## Rollback boundary

Before strict funding activation, rollback is to keep `funding_enforcement=legacy_single` and
ignore the dormant buckets. Bucket rows and exception evidence stay durable for audit/remediation;
they are not deleted or rewritten. Once provider/model policies and strict funding are active,
scalar-only binaries are outside the allowed rollback floor.
