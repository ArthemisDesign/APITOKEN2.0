# Stage 5 — materialization of the target pricing release

Status: the two-phase consumer is implemented in
`packages/db/src/pricing-stage5-materializer-v2{,-store}.ts` after separate GREEN producer and
migration checkpoints. The OpenKeys authoritative cursor, the admin-managed service inventory, the
compile-fixed runtime capability generations 3–6, and migration
`0029_pricing_release_two_phase_finalize.sql` are already its deployed prerequisites. Stage 5
prepares the immutable source/ownership/policy authority,
but does not guess changing funding identities and does not change live traffic.

## Input inventories

The planner receives fresh authoritative inventories of all bounded contexts:

- commerce B2C and B2B with full engine account IDs;
- B2B current scalar discount and active invitation snapshots;
- every OpenKeys account from the exact `/api/internal/pricing/v2/inventory`, including disabled,
  removed, and previously considered legacy ones; all pages must have a single full-manifest digest;
- every service account with purpose/responsible metadata;
- the full engine inventory for coverage verification.

Service authority is populated only via `PUT /admin/service-account-inventory/{service_id}`.
The mutation performs two matching full engine scans, takes the status from the engine, rejects
commerce and OpenKeys ownership, and writes a monotonic per-service version/content digest via
exact CAS. Simply being absent from commerce/OpenKeys does not automatically turn an account into a
service one: the metadata must be explicitly registered, and Stage 5 checks the entire complement.
The GET of this admin endpoint returns the canonical aggregate inventory digest. Before cutover
(`provisioning-context=null`) the mutation does not create release-v2 artifacts. After cutover,
before writing the inventory it prepares a rule-free service policy with
`billing_mode=meter_only`, without product/catalog/switch pins, and an exact active/recovery
assignment extension with purpose/responsible; the prepare ACK, the GET readback, and a fresh
context must all match.
The mutation does not create an engine account, release, or activation and does not move the global
head. An exact replay of an existing service identity remains `unchanged`; changing the metadata of
an already immutable active assignment requires the next release generation and is not rewritten in
place.

## Restoring terminal pre-cutover policy delivery

An account created by the old commerce writer before the `strict + legacy_single` fix may have
received an active engine account but a terminal `engine_policy_jobs.status=dead` and remained
`pending` in commerce. Such a historical blocker is restored only via the protected producer
endpoint:

```text
POST /v1/admin/pricing-policy-delivery-repairs
{
  "job_id":"<exact-dead-job-uuid>",
  "expected_effective_version":1,
  "expected_content_digest":"sha256:v1:<exact-job-digest>",
  "reason":"repair reviewed pre-cutover compatibility failure"
}
```

The endpoint requires an AdminGuard key and a verified `x-admin-actor`, and also confirms the
absence of a global release head through the engine provisioning context. In a single
`SERIALIZABLE` transaction it accepts only the current `dead` job with the exact expected identity,
the original payload `strict + legacy_single + verified`, a still unapplied commerce binding
`legacy_scalar + legacy_single + verified`, a terminal `sync_state=failed`, and an unchanged source
policy head. The old job's payload and identity are not rewritten and not re-run: only its
lifecycle status changes to `superseded`, and a new immutable effective version receives the
correct `shadow + legacy_single` payload and an ordinary
durable worker job. The actor, the reason, and both job identities are recorded in `audit_log`; an
exact replay returns `unchanged` via this audit link. A different permanent error, a changed
binding/source, an already applied policy, or a post-cutover state is rejected. Manually modifying
commerce rows or resending the old invalid payload is not a recovery procedure.

After the ordinary worker ACK the binding becomes `confirmed`, and only then does the corresponding
commerce mapping transition `pending → active`. The operation does not change the engine account,
balance, ledger, keys, release head, or client traffic.

The manual assignment matrix is no longer an authority. All owners must follow from the
authoritative inventory. One account in two inventories, an unknown account, an active account
without an owner, or a missing engine account is a typed blocker. Accounts are not excluded from
the target release because of disabled status: upon subsequent re-enabling they must already have
the correct policy.

## Target policy matrix

- B2C: global default `discount_bps=5000` for Anthropic, OpenAI, and Gemini with possible explicit
  provider/model overrides. An exact model rule takes priority over a provider rule, a provider rule
  over the global one.
- Existing B2B: the current `mult_bp` becomes only the provider rule `anthropic`:
  `discount_bps=10000-mult_bp`. OpenAI/Gemini are not added automatically. When the operator has
  already CAS-extended the live B2B policy head with additional provider/model rules, the target
  policy mirrors the current head exactly instead of the migration baseline, so the cutover never
  reprices or closes already granted live traffic. The head must keep one `provider:anthropic`
  rule equal to the live scalar (`b2b_policy_anthropic_rule_mismatch` otherwise), and any rule the
  release-v2 policy cannot express, such as a legacy `track` mode, is the typed blocker
  `b2b_policy_rule_unsupported`. An anthropic-only head equal to the scalar canonicalizes to the
  baseline policy identity. Release policy documents are immutable per `(policy_id, policy_version)`:
  when the planned content differs from the already persisted baseline version, the planner assigns
  the next free policy version instead of rewriting it, and assignments reference that version.
- B2B invitations: an independent immutable full-policy snapshot; redemption copies the exact
  snapshot.
- OpenKeys: one canonical 1:1 contract (`discount_bps=0`) for all existing and new accounts.
  Old scalar discounts are not carried over into the target release.
- Service: all runtime-capable models and `billing_mode=meter_only`; the balance does not
  participate in admission.

The internal engine provider ID for Gemini is `google`. Frozen capability generation 3 preserves
the original eight tariff-pinned Gemini models. Additive generation 4 added
`gemini-3-flash-preview` but did not become the target after the failed production gate. Generation
4 remains an immutable rejected artifact: Stage 5 does not materialize or finalize a target/recovery
plan on its digest. The fresh Pro+Ultra live matrix authorized additive capability generation 5;
its OpenKeys catalog remains the explicit Anthropic/OpenAI subset without Gemini. After real GPT
Image 2 generation and one-reference edit both passed through the existing sealed Codex OAuth pool,
additive generation 6 adds only `openai/gpt-image-2-2026-04-21`. The current materializer builds
main/OpenKeys catalogs and switches generation 6. Main retains the generation-5 model set and adds
the image snapshot; OpenKeys retains its generation-5 set and adds only that image snapshot at 1:1.
The generation-6 materializer starts from policy version 3. For every planned policy ID it reads the
engine's newest complete immutable policy twice around the second inventory scan, requires both
ordered read sets to match, and reconciles those heads with commerce-local documents. It reuses the
newest version only when its version-aware canonical digest matches the planned policy; otherwise it
allocates exactly one above that newest version. A same-version digest disagreement between engine
and commerce is a terminal evidence conflict. This makes replay after a remote-only partial prepare
idempotent without rewriting immutable policy history, falling back to an older equivalent version,
or submitting a stale baseline version. The typed consumer was connected only after producer SHA
`a7fbd16a0d63b3b16f7049f8aa1ac5b6e739583c` received exact `deploy/watchdog` GREEN.
Capability/catalog preparation alone does not enable traffic or add the image model to buyer/operator
display.

The planner reserves the target generation and the recovery generation of the next monotonic number
and builds an immutable source/policy/assignment plan for both. At this phase balance assignments
deliberately have `funding_generation=NULL`, and `funding_manifest_digest`,
`engine_release_digest`, and the final target/recovery release digests are absent. They cannot be
honestly computed in advance: account-local normalization includes live `balance_nano`,
`reserved_nano`, `spent_nano`, and lots while money writers keep running. The final release
manifests are built only from Stage 6 readback evidence.

## Dry run

The dry run operates in a read-only repeatable snapshot and outputs:

- source/inventory digests;
- full coverage of account classes;
- immutable policy identities;
- the reserved target/recovery generations and the absence of premature release digests;
- typed blockers and an exact writes plan.

The dry run writes nothing and does not require a reviewer field. Any change to a stable inventory
identity makes the result stale; the JSON must not be edited manually. Moving money is deliberately
not part of the plan digest: apply preserves its own fresh full snapshot as evidence, and replaying
the same immutable plan does not replace already recorded evidence with later
balance/reserved/spent values.

The production dry run is launched only through the AdminGuard-protected commerce API. Both POST
requests require a non-empty verified `x-admin-actor`; the mutation additionally requires a
meaningful `reason`:

```text
POST /v1/admin/pricing-stage5-v2/dry-run
{}

POST /v1/admin/pricing-stage5-v2/materialize
{"plan_digest":"sha256:v2:<exact-fresh-plan>","reason":"materialize reviewed full inventory"}
```

The response is a strict summary: source/plan digests, target/recovery generations and plan
digests, the total blocker count, and the full exact blocker list. A Stage 5/6 control failure keeps
the standard `statusCode` and `message` fields and adds a stable machine-readable `code`; consumers
must ignore unknown response fields. The dry run does not write even an audit row. Materialize
rebuilds the full plan, rejects a stale digest, and, atomically with the
local run/plan, writes the attributed audit request; the dormant engine prepare/readback remains
the next part of the same idempotent operation. The runtime takes `DATABASE_URL`,
`ENGINE_BASE_URL`, `ENGINE_CONTROL_KEY`; OpenKeys is read directly on loopback
`OPENKEYS_INTERNAL_BASE_URL` (default `http://127.0.0.1:3410`) with a separate
`OPENKEYS_CONTROL_KEY` or the same server credential.
The package CLI remains only a diagnostic non-production entrypoint and is not a permitted
production control-plane or SSH procedure.

The engine account cursor is exhausted twice. The latest release-policy heads for every planned
policy ID are also read twice and must remain byte-canonically equal; a changing or conflicting
lineage fails before any local evidence or remote prepare is written. Stage 5 identity stability
includes `account_id`, status, and the legacy scalar multiplier, but deliberately does not include the changing
`balance/reserved/spent` and the funding head: full money snapshots are preserved as evidence,
while their final identity belongs to Stage 6. The OpenKeys cursor is also exhausted twice and must
return one unchanged full-manifest digest on all pages of both passes.

## Materialize

Apply operates in a `SERIALIZABLE` transaction, rebuilds the same plan, and accepts the exact
expected source/plan digest. It materializes immutable capability/catalog/switch/policy rows, the
Stage 5 run, release-plan skeletons, and full assignments. For balance assignments the funding
identity remains nullable; the engine release and the Stage 6 parent job are not created in this
independently delivered checkpoint. Only after a GREEN Stage 5 source/policy materialization may a
separate consumer launch Stage 6 by the exact plan digest. The active pricing release head does not
move.

The local plan is first pinned under an advisory lock with a re-check of the commerce/service
snapshot; the same check is mandatory before saving terminal blocker evidence. Then the consumer
performs only a dormant engine prepare for the main/OpenKeys catalogs and provider switches of
generation 6, and for each exact resolved policy version, immediately reads the version back, and
records an ACK only for `stored|unchanged` with a matching digest. The materializer builds the
capability projection, both catalogs, the switches, and the customer and service policies on dormant
capability generation 6. Rejected generation 4 remains compile-fixed immutable history, is not part
of any Stage 5 target/recovery artifact, and does not receive a fictitious capability ACK.
Target/recovery release prepare, the recovery link, and the control job are absent until Stage 6.

A same-version/same-digest replay returns `unchanged`. Same-version/different-digest, incomplete
inventory coverage, a stale source, a policy collision, or an unsupported runtime capability is
rejected before commit. A rejected engine prepare remains fail-closed and exposes only a bounded
`engine_<main_catalog|openkeys_catalog|switches|policy_b2c|policy_b2b|policy_openkeys|policy_service>_prepare_<rejection>`
control error code; it never treats the activation-only `applied` result as a successful dormant
prepare and never puts an
artifact identity, digest, account, or engine message into that code. The B2C/B2B/service/OpenKeys
target is prepared in full; partial apply per class is forbidden. A stable plan with ownership blockers may save only a terminal `blocked` run
and typed blocker rows; the catalog/policy/release skeleton and the remote prepare are not created
in that case. Unstable paired scans are not saved as false evidence and require a new full pass.

## Evidence for the following stages

Until Stage 6/8, restricted operational artifacts are preserved:

- exact inventories;
- the dry-run report and plan digest;
- target/recovery plan skeletons, and after Stage 6 — the finalized release manifests;
- the durable ACK of all prepared identities.

Migration `packages/db/migrations/0028_pricing_stage5_evidence.sql` creates the empty storage for
this evidence in advance: `pricing_stage5_runs_v2` holds the exact inventory/plan artifacts and
both pairs of scan digests, `pricing_stage5_blockers_v2` holds the typed discrepancies, and
`pricing_stage5_prepare_acks_v2` holds only successful prepare+readback identities. A DB constraint
prevents accepting unstable engine/OpenKeys scans or an ACK with a differing readback digest. The
existence of the tables does not start the planner, does not create a release/control job, and does
not move the head.

Migration `packages/db/migrations/0029_pricing_release_two_phase_finalize.sql` permits an honest
two-phase state: the Stage 5 run and release plans may store nullable final identities, and a
balance assignment may store a nullable funding generation. Guard triggers keep the source/policy
plan immutable, permit only the funding generation transition `NULL → positive`, forbid replacing
an already set identity, and prevent moving a release to `prepared` until the assignment graph is
complete and matches the ready Stage 6 rows one-to-one. After the engine prepare/readback, both
release identities and assignments are frozen. The migration starts nothing and does not touch live
money rows.

Stage 5 does not change prices, balances, keys, or access. Live behavior changes only via the
single-head CAS at Stage 9 per `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
