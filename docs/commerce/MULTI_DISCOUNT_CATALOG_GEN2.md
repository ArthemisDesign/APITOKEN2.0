# Multi-discount catalog generation 2

Catalog generation 2 adds `claude-opus-5` and `claude-fable-5` (provider `anthropic`) to the
multi-discount pricing catalog. It is delivered fully inert: the code, the pinned constants, and
the metering capability exist in production, but no durable head changes until the activation
procedure below is executed. Generation 1 remains the only production authority until then.

## Fixed identity

All generation-2 artifacts are deterministic from the reviewed constants in
`@claude-api/contracts` (`MULTI_DISCOUNT_GEN2_*`) and carry these pinned digests:

- capability generation `2`, schema version `1`, content digest
  `sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0`;
- `main` product catalog generation `2`, content digest
  `sha256:v1:807fbe80c12a03e773e2f5067bc04a66b5a41e42d4bfdc8f85fe5656a5013616`;
- `openkeys` product catalog generation `2`, content digest
  `sha256:v1:3b019fc3cfd619b5d4a81451aceafebf0c40de3b8c2cc150aa5b7a28b0102760`;
- provider-switch generation `2`, content digest
  `sha256:v1:ddbe078beec31d4f8b77e027ff3e9dad5477be6d10dafd4c99956abd9a74febd`.

Both product catalogs contain the same twelve models: the seven Anthropic canonical models
(`claude-fable-5`, `claude-haiku-4-5`, `claude-opus-4-7`, `claude-opus-4-8`, `claude-opus-5`,
`claude-sonnet-4-6`, `claude-sonnet-5`) and the five OpenAI models from generation 1, all
`enabled=true`. The capability projection keeps the single `gpt-5.6 -> gpt-5.6-sol` alias.
Switch generation 2 is identical to generation 1 except that every scoped entry re-pins
`catalog_generation` to `2`; the master switch keeps `catalog_generation = null`.

## Why the deploy alone changes nothing

- The generation-1 constants in `packages/contracts` are untouched byte for byte; Stage 5 keeps
  its own planner and its own digests.
- The engine runtime manifest (built into `claude-api`, never taken from HTTP or the database)
  lists both capability generations side by side as manifest generation `2`. Adding a member is
  inert: the resolver already had to know generation 1, and knowing generation 2 only means the
  resolver will not fail closed once commerce activates it. Admission behavior is unchanged.
- The metering allow-list addition only removes the `strict_pricing_unsupported_model` refusal
  for the two model IDs; without a catalog/policy that enables them, requests still fail closed
  downstream.
- No account policy is rebuilt by activation. Existing bindings keep pinning catalog generation 1
  and resolve through the dual-lineage choreography; the two new models stay gated by the policy
  catalog until a later policy generation explicitly enables them.

## Activation procedure

Prerequisites:

1. The engine (`claude-api`) deployed on a SHA that contains manifest generation `2`. Without it
   the engine fails closed on any generation-2 catalog/switch activation attempt.
2. Commerce (`apps/api`, `apps/worker`) deployed on the same change; the pricing worker must be
   running — it is the only actor that delivers the durable jobs to the engine.
3. OpenKeys deployed with the reviewed constants; its issuance assert accepts catalog generation 1
   or 2, each only with its own exact reviewed identity.

Steps:

1. **Dry run.** From a host with the commerce `DATABASE_URL`:

   ```text
   pnpm --filter @claude-api/db pricing:catalog-gen2 -- dry_run
   ```

   Dry run opens a `REPEATABLE READ READ ONLY` transaction, writes nothing, and prints the full
   plan plus the durable foundation. Verify:
   - `foundation.matches_reviewed_generation_1` is `true` (or `foundation.already_materialized`
     is `true` if generation 2 was already applied);
   - `foundation.capability`, both `foundation.catalogs`, and `foundation.switches` report
     generation `1` with the Stage 5 digests (`main`
     `sha256:v1:8f8446d7ba49e9ccc3ac8211d607e3a1d4121995cd756931eea1e9a24cca5910`, `openkeys`
     `sha256:v1:0bb25e5a19c9a67284cee9b384bf47b1fd61225ae6a46190fc6965fd0c46d956`);
   - every `plan` digest equals the pinned identity above.

   Any other foundation state means the production heads drifted from the reviewed generation-1
   identity: stop and reconcile before applying.

2. **Apply.**

   ```text
   pnpm --filter @claude-api/db pricing:catalog-gen2 -- apply
   ```

   Apply re-reads the foundation inside one `SERIALIZABLE` transaction, refuses
   (`foundation_mismatch`) unless it still matches reviewed generation 1 exactly (or generation 2
   is already materialized, which makes the run an idempotent replay), then materializes with the
   Stage 5 `ensure*` semantics: capability generation 2, both product catalogs, switch
   generation 2, plus two `engine_catalog_jobs` rows and one `engine_switch_jobs` row, all
   `pending`. The result prints `writes_committed: true`.

3. **Watch worker delivery.** The pricing worker claims catalog jobs before the switch job (its
   claim query requires the referenced catalog jobs to be `confirmed` first), so the engine
   authority walks the supported catalog → switches order on its own:

   ```sql
   SELECT product_id, generation, status FROM engine_catalog_jobs ORDER BY id;
   SELECT generation, status FROM engine_switch_jobs ORDER BY id;
   ```

   Wait until both generation-2 catalog jobs and the generation-2 switch job are `confirmed`.
   `retry` is self-healing; anything stuck in `failed`/`superseded` requires operator attention
   before proceeding.

4. **Verify engine heads.** Through the authenticated engine Control API
   (`docs/engine/CONTROL_API.md`):

   ```text
   GET /admin/pricing/catalog/main/active
   GET /admin/pricing/catalog/openkeys/active
   GET /admin/pricing/switches/active
   ```

   Both catalogs must report generation `2` with the pinned content digests, and the switches
   must report generation `2`.

## Operational notes

- **Fail-closed window.** Between the catalog confirmations and the switch confirmation, scoped
  switches still pin `catalog_generation = 1` while catalogs already serve generation 2. Authority
  resolution for identities that require a matching catalog/switch pair (OpenKeys issuance assert)
  fails closed with `switch_identity_mismatch` during that window. The worker delivers the jobs
  back to back, so the window is bounded by one worker pass; do not run OpenKeys batch issuance
  until step 4 is green.
- **Idempotent replay.** Re-running `apply` after a successful activation is safe: the foundation
  reports `already_materialized`, the `ensure*` reads confirm byte-identical rows, and no new jobs
  are enqueued (`ON CONFLICT DO NOTHING` plus read-back assertion).
- **No rollback by re-activation.** Heads are monotonic; generation 1 cannot be re-activated over
  generation 2. Rolling back the deploy before activation is always safe because nothing durable
  has changed. After activation, a rollback of the engine binary is only safe if it still contains
  manifest generation `2`; otherwise the resolver fails closed on the active generation-2 heads.
- **New models stay closed.** Activation only extends the catalogs. Accounts receive
  `claude-opus-5`/`claude-fable-5` access exclusively through a later policy generation that
  enables them; OpenKeys enables them only through its own product-catalog cutover path.
