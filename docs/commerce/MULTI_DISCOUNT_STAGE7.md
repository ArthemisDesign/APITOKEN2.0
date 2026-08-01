# Stage 7 OpenKeys 1:1 cutover

Stage 7 closes every application-level path that could issue a new discounted OpenKeys key. It
does not rewrite, reprice, top up, disable, or otherwise mutate an existing key. Rows backfilled as
`legacy` continue to use their stored `mult_bp` for balance and usage presentation.

## New issuance contract

Every new batch and key is written explicitly as `official_1_to_1` with `mult_bp=10000`. The engine
account is created with the same fixed multiplier and its credit is exactly the requested face
value in nanoUSD. `OPENKEYS_DEFAULT_MULT_BP` is no longer read; request fields such as `multBp`,
`mult_bp`, discount, multiplier, or pricing-contract overrides are rejected before any database or
engine write. `api_type` remains presentation/routing metadata and is not an input to policy
construction or access control.

The reviewed OpenKeys catalog is shared with the Stage 5 catalog builder. It contains the current
Anthropic and OpenAI canonical models and no Gemini entry. Issuance first reads the active catalog
and global provider-switch generation and requires the exact reviewed OpenKeys product scope. The
OpenKeys application never invents or overwrites global catalog/switch authority.

## Policy ACK gate

For each newly created engine account, issuance proceeds in this order:

1. prepare an immutable OpenKeys current policy with provider-level zero-discount rules for
   Anthropic and OpenAI;
2. activate it from the exact `unbound` state and validate the typed ACK identity;
3. read the active policy back and require exact policy and binding equality;
4. credit the exact face value using the idempotent `openkeys:<batch>:<index>` reference;
5. issue the engine key, encrypt the secret, and persist the matching OpenKeys row.

Any missing/drifted authority, rejected/malformed ACK, readback mismatch, multiplier mismatch,
credit failure, or key failure aborts the item. A created account is disabled by the existing saga
compensation path, and no usable secret is returned. Lost-process reconciliation likewise disables
unfinished accounts.

The Stage 7 binding remains `policy_enforcement=shadow`,
`funding_enforcement=legacy_single`, and `reconciliation_state=pending`; Stages 8 and 9 own the
shadow/strict runtime transition. If the reviewed Stage 5 catalog/switch authority has not yet been
activated, new issuance deliberately returns unavailable rather than falling back to scalar
pricing. Existing inventory remains readable and usable under its immutable legacy contract.

## Existing inventory policy cutover

`@claude-api/openkeys` exposes the bounded-context batch command that consumes the exact retained
Stage 5 dry-run result and its approved assignment matrix:

```text
ENGINE_BASE_URL=... ENGINE_CONTROL_KEY=... \
pnpm --filter @claude-api/openkeys pricing:stage7 -- \
  dry_run <stage5-dry-run.json> <assignment-matrix.json>
```

The command rejects any invalid matrix/draft digest, stale plan identity, changed OpenKeys
reference, duplicate account/source, non-canonical official policy, malformed legacy policy, or
active engine catalog/switch authority different from the Stage 5 artifact. It inventories every
OpenKeys account from the artifact, including disabled rows, and reports each as `unbound`, `exact`,
or `conflict`. Dry-run performs no prepare or activation call. A conflict blocks the complete apply
before its first write.

After retaining the conflict-free report, apply uses the same two immutable artifacts:

```text
ENGINE_BASE_URL=... ENGINE_CONTROL_KEY=... \
pnpm --filter @claude-api/openkeys pricing:stage7 -- \
  apply <stage5-dry-run.json> <assignment-matrix.json>
```

For each `unbound` account it prepares the exact candidate, rechecks the CAS precondition,
activates from `unbound`, and requires exact active policy/binding readback. Accounts already exact
are left untouched, so a complete replay returns `unchanged`; an interrupted run is resumed with
the same artifacts. A final full-inventory readback must still find every account exact before the
report succeeds. The command calls only versioned pricing endpoints. It never edits OpenKeys rows,
scalar multipliers, balances, keys, statuses, or history.

Legacy policy IDs remain `policy:openkeys:legacy:<source-id>` with source-specific locked digests.
All `official_1_to_1` rows use `policy:openkeys:official-1-to-1`, owner `official-1-to-1`, and the
same Stage 7 digest domain as live issuance. The fixed production digest test vector is retained in
`docs/commerce/fixtures/openkeys-official-policy-v1.json` and exercised from both Stage 5 and
OpenKeys tests.
