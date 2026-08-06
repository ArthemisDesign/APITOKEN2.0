# Stage 7 — OpenKeys canonical 1:1

Stage 7 closes all paths that issue OpenKeys at a price other than the official 1:1 and prepares all
existing inventory for the shared Stage 9 cutover.

## New issuance

Every new batch/key has contract `official_1_to_1`, `discount_bps=0` and
`payable_multiplier_bp=10000`. Request/env fields for multiplier, discount, or pricing override are
rejected before any database/engine write. Face-value credit remains an exact integer nanoUSD.

Issuance uses the current OpenKeys product catalog. A new Anthropic/OpenAI/Gemini model appears only
after an explicit catalog generation; a model being present in engine capability does not enable it
automatically.

Before issuing a usable secret, the application must receive an exact prepared/active policy ACK,
re-read the binding, and persist the matching OpenKeys row. Lost-process compensation disables the
unfinished engine account.

## Existing inventory

All existing OpenKeys, including those previously considered legacy, receive the target canonical
1:1 policy. Their past ledger rows and charges are not rewritten. The current live reserve stays on
the old active release until Stage 9; then the entire inventory starts being charged 1:1
simultaneously.

The Stage 7 dry run reconciles the OpenKeys DB inventory against engine accounts, the Stage 5 plan,
and the canonical policy digest. A missing/duplicate/source collision or any discount in the target
policy blocks the complete apply before the first write.

Apply idempotently materializes the exact target bindings and confirms the readback. It does not
move the global active release head, does not change balance/key/status, and does not perform a
separate OpenKeys cutover.

## Durable shadow rollout lane (migration 0035)

Pre-cutover policy alignment of the OpenKeys inventory (including replacement-locked legacy) is
performed only by the durable lane on top of the empty parent/child tables
`pricing_shadow_rollouts_v2` / `pricing_shadow_policy_jobs_v2`. This replaces the abolished
OpenKeys-local backfill with a manual assignment matrix: its module, CLI, and tests have been
removed, because generic prepare/activate fundamentally return `423 locked` for replacement-locked
bindings, and a manual matrix is not a durable authority.

**Lane scope — OpenKeys only.** Commerce B2C/B2B and service lineages are aligned by their managed
policy writers (catalog/switch convergence + managed policy update create gen-aligned versions on
the existing lineage), because the engine never accepts a different policy identity for an account
with an existing lineage (`policy_identity_matches`), and `meter_only` service semantics are not
expressible as a v1 shadow policy at all. The rollout still validates the full Stage 5 inventory
fail-closed, but jobs are created only for OpenKeys assignments.

**Producer.** The only way to create a rollout is the AdminGuard-protected
`POST /v1/admin/pricing-shadow-rollout-v2/stage` in `apps/api` with a UUID `idempotency_key`, exact
`stage5_run_id`, meaningful `reason`, and a verified actor from the `x-admin-actor` header (actor in
the JSON body is forbidden). `packages/db/src/pricing-shadow-rollout-jobs-v2.ts` in a single
`SERIALIZABLE` transaction under an advisory lock reads the exact prepared Stage 5 run (status
`prepared`), the prepared target/recovery release plans, and performs a fresh engine inventory scan:
any digest drift, collision, or missing owner — fail closed before the first write. The rollout pins
the target/recovery generation+digest, catalog/switch generations+digests, engine inventory,
assignment/policy manifest, and the canonical `sha256:v2` rollout digest; per-account jobs carry the
release-policy identity, exact effective version/content digest, expected active from the live
engine read, request digest, and the full byte-exact request payload. Idempotency is by
`idempotency_key` and `rollout_digest`: an exact replay returns the existing rollout without
writing. Staging failures expose only a stable bounded `code` with a sanitized 409/503 message;
raw DB/engine errors and account or policy identities never leave the API response. The fixed
operator bridge consumes that contract only after exact producer SHA
`d85aa225e0846439b85d8ba55fa8cd290d23a472` reached GREEN and prints only an allowlisted code;
unknown or malformed responses are `unclassified`.

**Locked-OpenKeys path.** A job for a replacement-locked legacy account (`owner_context=openkeys`,
`pricing_contract=legacy` in the exact Stage 5 inventory) contains only the
`locked_openkeys_transition` payload: both the successor and the expected active are built from the
account's exact live engine lineage (read at staging; no historical digest reconstruction — the real
stored identity and binding); the successor is +1 exact version, the same immutable policy identity,
managed provider-only 1:1 rules, without a replacement lock.
The worker delivers it exclusively via
`POST /admin/pricing/policy/{account_id}/locked-openkeys-transition` after a fresh readback:
active policy diverging from the durable expectation, a lost replacement lock, or a typed rejection
(400/409/423) — terminal `blocked` with `last_error`.

**Generic path (canonical OpenKeys).** For already-canonical OpenKeys accounts (`official_1_to_1`)
the job carries the `policy_shadow` payload: the successor is built on the account's EXISTING engine
lineage (the same policy identity, next monotonic version, exact current active as expected active),
with rules converted from the release policy, and pins of the exact Stage 5 catalog/switch.
The worker reads the engine state, confirms an already-exact
policy with a single readback without mutation, otherwise performs prepare → exact readback →
activate with a CAS expectation from fresh state. Any version conflict, digest mismatch, newer
engine policy, or typed rejection — `blocked`; transient transport — bounded `retry` with a lease;
an expired lease is reclaimed, the final attempt goes to `dead`.

**ACK evidence and terminal state.** Every confirmed job stores the canonical `sha256:v2` ACK digest
and the full ACK payload (engine ACK or exact readback evidence). When all jobs are terminal, the
rollout atomically becomes `confirmed` (all confirmed), `blocked`, or `dead`. Read-only status —
`GET /v1/admin/pricing-shadow-rollout-v2`: a bounded snapshot of subjects only as `sha256:v2`
digests, without raw account identities. Startup, migration, polling, and the read endpoint do not
create rollouts/jobs. The lane does not change the live price, funding authority, release head,
balances, or OpenKeys rows.
Worker bounds: `PRICING_SHADOW_ROLLOUT_POLL_MS=5000` (`1000..60000`),
`PRICING_SHADOW_ROLLOUT_LEASE_MS=300000` (`30000..3600000`),
`PRICING_SHADOW_ROLLOUT_RETRY_MS=15000` (`1000..3600000`),
`PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS=10` (`1..100`),
`PRICING_SHADOW_ROLLOUT_BATCH_SIZE=25` (`1..500`); the defaults are production-safe and validated at
worker startup.

Production admission is performed only after exact GREEN Stage 5/6 evidence through the fixed,
root-owned bridge:

```bash
ssh -o BatchMode=yes apitokensale \
  'sudo -n /usr/local/lib/apitoken-watchdog/controller/pricing-stage7-admission-gate.sh 3f412e33d631f2956a575e40f7f28f8b0b592106'
```

The bridge reads the immutable terminal Stage 5 run through the bounded read-only producer added by
GREEN SHA `70d785e66e915cdbaaf9d4a60bc00492511b95d3`, requires the exact prepared target/recovery
lineage and zero blockers, stages one idempotent rollout, and polls the bounded control API until
every durable job is confirmed. It never replays the Stage 5 materialization mutation. It uses the
admin credential only from the active exact-release process environment via procfs/curl stdin; its
output is limited to release identities, aggregate counts, status and completion time. Do not
replace it with SQL, direct engine calls, SSH loops or a copied credential.

The stage request deliberately rejects `engine_inventory_drift`: the fresh exhaustive engine
identity must equal both scans recorded by Stage 5. Recovery is not a retry or relaxed comparison.
Run the separately delivered Stage 5/6 inventory-refresh bridge after its exact deployment is GREEN,
wait for a new immutable prepared target/recovery pair, then deliver a new Stage 7 bridge pinned to
those terminal identities in a later GREEN checkpoint. The completed replacement pair is `g23` target
(`sha256:v2:bab313f8619d856de9073c13d6dabbf9663a59aa3e4200ab25f7758202b5892c`) and `g24`
recovery (`sha256:v2:36c48d7c909882d8a01d47131a5e63ce04d0d5bdebbbefefc8fe04dfdcddd8b1`); its fixed
consumer is `pricing-stage7-refresh-gate.sh`, with an independent root-only request fence. Earlier
runs, releases and private fences remain unchanged.

## Invariants

- The target release contains no source-specific discounted legacy policy.
- Existing and new OpenKeys share the same 1:1 economics.
- OpenKeys does not inherit global B2C/provider/model discounts.
- OpenKeys usage does not participate in referral commission.
- Neither the admin API nor batch issuance accepts a multiplier field.
- The live change happens only via the shared CAS from `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
