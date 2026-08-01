# Stage 5 multi-discount backfill

Stage 5 is an application-controlled, fail-closed reconciliation. It is not an automatically
executed data migration. The commerce schema is already expanded; this checkpoint materializes
immutable authority rows and the durable Stage 4 jobs only after their complete source snapshot is
known.

## Inputs and fixed runtime identity

The reconciler accepts one JSON inventory with schema version `1`:

- every engine account returned by the authenticated engine Control API, including its exact
  `account_id`, scalar `multiplier_bp`, and active/disabled status;
- every OpenKeys engine account exported by the OpenKeys bounded context, including its stable
  OpenKeys source identity, scalar multiplier, status, and `legacy` or `official_1_to_1` contract.

The inventory must contain no duplicate identities. Commerce accounts are joined to this engine
inventory by the full engine account ID. OpenKeys identities are accepted as external evidence;
the commerce code never opens the OpenKeys database.

The initial runtime pins are deliberately constants in the executable planner:

- pricing schema version `1`;
- capability generation `1`;
- capability digest
  `sha256:v1:88da6b622727dda8aac0e1cd1749524f4929f7738f097c2dd3b81ba1cc14e7fd`;
- main and OpenKeys catalog generation `1`;
- provider-switch generation `1`.

Both product catalogs contain only the reviewed Anthropic and OpenAI canonical models from
`docs/commerce/MULTI-DISCOUNT.md`. `gpt-5.6` is stored only as an alias of `gpt-5.6-sol`. Gemini has no
capability projection entry, catalog entry, switch, rule, or job.

These pins describe generation 1 and stay its permanent identity. Catalog generation 2
(`claude-opus-5`, `claude-fable-5`) is a separate additive tooling with its own planner and
runbook in `docs/commerce/MULTI_DISCOUNT_CATALOG_GEN2.md`; it never rewrites the generation-1
rows this Stage 5 reconciler produces.

## Dry run and assignment approval

`@claude-api/db` exposes `pricing:stage5`. It requires `DATABASE_URL` and accepts the mode plus the
inventory file:

```text
pnpm --filter @claude-api/db pricing:stage5 -- dry_run <inventory.json>
```

Dry run opens a PostgreSQL `REPEATABLE READ READ ONLY` transaction and emits:

- the exact capability, catalog, switch, source-policy, and effective-policy specifications;
- B2C account policies using each account's exact current scalar multiplier for both `track`
  provider rules;
- independent Anthropic-only snapshots for every unconsumed, non-revoked, non-superseded invitation;
- protected B2B candidates with Anthropic-only static rules;
- protected OpenKeys candidates: source-specific locked legacy rules for `legacy` rows and the one
  shared Stage 7 canonical identity for `official_1_to_1` rows;
- every engine identity not claimed by commerce or OpenKeys;
- typed blockers, the source/inventory/plan digests, and an assignment-matrix draft.

No protected identity is inferred. A reviewed matrix must repeat the exact B2B and OpenKeys
references from the plan, classify every remaining active account as service with a complete
static-only policy, and may exclude only inventory accounts already marked disabled. The matrix
binds reviewer, time, reason, the whole plan digest, all decisions, and its own canonical SHA-256
digest. Any field change invalidates it.

## Apply modes and atomicity

`safe` materializes only the shared capability/catalog/switch graph, B2C policies, and invitation
snapshots. B2B, service, and OpenKeys assignments remain untouched:

```text
pnpm --filter @claude-api/db pricing:stage5 -- safe <inventory.json>
```

`approved` additionally materializes commerce B2B and service policies and requires the exact
reviewed matrix:

```text
pnpm --filter @claude-api/db pricing:stage5 -- approved <inventory.json> <assignment-matrix.json>
```

OpenKeys account-policy writes remain outside commerce and are consumed by the Stage 7 OpenKeys
cutover. Their exact candidates are nevertheless part of the approved matrix so the two bounded
contexts cannot silently classify the same engine account differently.

The official 1:1 candidate is built by `buildOfficialOpenKeysPolicy` from
`packages/engine-client`, the same pure builder used by live OpenKeys issuance and the Stage 7
batch. Its policy ID, owner, rules, source digest, and effective digest are therefore byte-for-byte
identical across both stages. Legacy candidates deliberately remain source-specific and
`replacement_locked=true`.

Every apply holds source-table share locks inside one `SERIALIZABLE` transaction. The transaction
stores and verifies immutable versions, moves only monotonic desired heads, and inserts the catalog
→ switches → account-policy jobs used by the Stage 4 worker. A crash or any conflict rolls back the
whole apply. Exact replay returns the same plan and leaves row counts unchanged. The same version
with a different digest, scalar drift among engine/profile/commerce evidence, cross-context
identity collision, unsupported legacy multiplier, missing inventory identity, malformed approval,
or active unresolved account fails before commit.

All resulting bindings stay in `shadow` / `legacy_single` with reconciliation pending for funding.
Stage 5 does not enable strict enforcement, change live balances, issue keys, or alter historical
charges.

## Rollout evidence

Before any apply, retain the exact inventory JSON, dry-run output, and approved assignment matrix
as restricted operational artifacts. After safe/approved apply:

1. wait for both catalog jobs and the switch job to receive exact ACKs;
2. wait for every staged policy job to receive its exact full-identity ACK;
3. verify no stale/dead job and no desired/applied mismatch;
4. verify the engine active heads match the plan digests;
5. confirm the shadow outcome is economic parity for every account where parity is required;
6. preserve the matrix digest for Stage 7, Stage 8 classification, and the final 20-criterion audit.

Reverse data mutation is not the rollback mechanism. Before strict enforcement, rollback means
leaving the immutable rows present and keeping bindings on legacy scalar behavior. Once policies
diverge by provider/model, only policy-capable binaries may serve; Stage 9 installs that rollback
floor explicitly.
