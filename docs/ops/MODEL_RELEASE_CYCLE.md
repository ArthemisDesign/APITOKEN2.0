# Model release cycle — adding a model to the live pricing authority

This is the operator runbook for admitting a new model of an existing provider (the gpt-image-2
path generalized). For a new PROVIDER, start from `docs/engine/PROVIDER_ONBOARDING.md` instead and
expect the pricing-authority extension to be its own multi-commit effort.

Two phases, per the repository's two-stage model rule: the dormant implementation lands first and
proves itself live; publication (catalogs, release advance, storefronts) is a separate step after
a GREEN exact implementation SHA.

## Phase A — dormant implementation

1. Metering tariff in `crates/metering` (the provider catalog module, e.g. `openai_image.rs`):
   official pricing link in a comment, exact-rate tests, i128 nanoUSD only. Gate: ALL of
   `cargo test -p metering`.
2. Dormant transport/adapter code in `crates/forward` if the model needs a new wire shape
   (e.g. the Images API). Dormant = no catalog membership, no product access.
3. Live proof on the exact implementation SHA: generation 2xx with real output, terminal
   authoritative usage, incremental SSE, and the advertised controls — through the existing
   SHA-pinned live/paid smoke gates (see `docs/ops/GPT_IMAGE_2_CANARY.md` for the pattern). A
   failed generation means withdrawal, not publication "for checking".

## Phase B — release generation constants (generated, never hand-edited)

All frozen constants for the next capability generation come from the generator:

```bash
node tools/pricing-next-generation.mjs <spec.json>
```

The spec names the current generation and the added entries (provider, canonical model id,
alias, products). The tool prints the exact blocks for every hand-edit site: the
`packages/contracts/src/index.ts` generation constants, both Rust mirrors
(`crates/forward/src/pricing.rs`, `crates/server/src/config.rs`), the Stage 5 materializer
constants (`packages/db/src/pricing-stage5-materializer-v2.ts`), and the engine-client policy
version/OpenKeys rows. Apply the blocks, then prove exactness with the replay test
(`node --test 'tools/**/*.test.mjs'`). A digest typed by hand is a drift incident waiting to
happen; the generator exists because the gpt-image-2 admission hit several.

One commit carries the constants + mirrors + materializer and merges through the standard gate.

## Phase C/D — the orchestrated release cycle (one durable intent)

With `pricing_release_orchestrations_v2` the whole cycle is one staging call; the pricing worker
drives catalog/switch delivery → Stage 5 materialize → Stage 6 funding → Stage 7 rollout →
Stage 8 capture → Stage 9 activation → verification through the existing durable sub-jobs and
their unchanged gates, re-cycling up to 3 times on inventory drift and re-capturing on evidence
TTL expiry:

```bash
curl -X POST -H "x-admin-key: $COMMERCIAL_ADMIN_KEY" -H "x-admin-actor: <actor>" \
  -H 'content-type: application/json' \
  -d '{"idempotency_key":"<uuid>","capability_generation":<N>,"reason":"release <model>"}' \
  http://127.0.0.1:8791/v1/admin/pricing-release-orchestration-v2/stage
# watch: GET /v1/admin/pricing-release-orchestration-v2
```

The capability generation pin must equal the generation the deployed constants plan for; a
mismatch (forgotten or unmerged constants) dies immediately with the two numbers in `last_error`.
`status=dead` with a non-drift blocker is terminal evidence — fix the producer, then stage a new
intent with a new idempotency key. `materialize_pair` dying with `commerce_status_drift` names an
account whose registration provisioning is stuck in `pending`: since the provisioning repair lane
(commit in the same delivery) the source policy re-pins itself to the live catalog head and the
dead delivery re-materializes on the customer's next action — or an operator can complete it
immediately via any key-issuance path for that user. Only one orchestration is active at a time. Inventory drift is
classified and re-cycled at every stateful step — materialize, funding normalization (a mid-cycle
signup dies there with "engine identity inventory no longer matches"), rollout and capture.
Transient capture blockers (a busy authority window, a briefly unresolved pricing job) trigger a
bounded re-capture of the same pair instead of a fresh cycle; a dead release control job whose
kind confirmed at a newer release generation is obsolete abandoned-pair evidence and no longer
counts as backlog anywhere.

The manual step-by-step lane below remains as the repair/debug path.

## Phase C — successor release cycle (Stage 5 → 7 converge)

Once the constants are live and deployed, deliver the new catalog/switch generation to the
engine FIRST — the Stage 7 shadow activation rejects it as `missing_dependency` while the heads
still point at the previous generation. Stage the three durable delivery jobs through the
AdminGuard API (the worker confirms them in seconds; verify the engine heads moved):

```bash
curl -X POST -H "x-admin-key: $COMMERCIAL_ADMIN_KEY" -H "x-admin-actor: <actor>" \
  -H 'content-type: application/json' \
  -d '{"product_id":"main","generation":<N>,"reason":"..."}' \
  http://127.0.0.1:8791/v1/admin/pricing-catalog-jobs/stage
# repeat for product_id "openkeys", then pricing-switch-jobs/stage with {"generation":<N>}
```

Then drive the successor pair to `confirmed` with the fixed convergence bridge on the host
(root bridge, exact admission SHA):

```bash
sudo /usr/local/lib/apitoken-watchdog/controller/pricing-stage567-converge-v2-gate.sh <admission-sha>
```

What it does per cycle: fresh private namespace → Stage 5 dry-run + materialize + Stage 6 funding
normalization (the stage56 refresh helper) → Stage 7 OpenKeys shadow rollout (the stage7 refresh
helper). At most 3 cycles; a cycle ends terminal-green, or fenced `engine_inventory_drift` /
`rollout_blocked`, and a fenced cycle is never reused — the next cycle re-scans the live
inventory into a fresh pair.

Failure playbook (all observed during gpt-image-2):

- `engine_inventory_drift` — the account inventory changed mid-run (new registrations). No
  action: the bridge advances to the next cycle by itself.
- `rollout_blocked` with `shadow policy prepare rejected with locked` — a producer bug in the
  engine lock/lineage handling. Do NOT rerun blindly: fix the engine producer first (the
  replacement-lock consumption commits), wait for a GREEN engine deploy, then converge again.
- `rollout_blocked` with `shadow policy activation rejected with missing_dependency` — the new
  catalog/switch generation is not active in the engine yet. Stage the delivery jobs (above),
  verify the heads, then converge again. Delivering DURING a rollout races it: jobs processed
  before the head move still fail, so always deliver first.
- Staging stops with HTTP 409 `openkeys_lock_drift` — a lineage state the rollout contract
  refuses (still-locked canonical lineage, or an unlocked legacy lineage whose live rules are
  not the canonical 1:1 successor). Read the engine lineage for the named account; the fix is in
  the producer, never in editing the lineage by hand.
- `exact terminal Stage 6 result is unavailable or untrusted` / `plan fence is unavailable` —
  the helpers and the bridge disagree about the cycle namespace. The helpers accept the cycle
  root only from the provisioned namespaces; if you introduce a new bridge namespace, extend
  their allowlist in the same commit.
- Stale persisted `stage6-result.json` (inventory churned between a helper run and the bridge
  rerun): re-run the stage56 helper for that cycle root, validate its terminal output with the
  bridge's jq contract, move the stale result to `*.superseded-*` (never delete), and install
  the fresh result atomically (0600 root:root). Then rerun the bridge.
- A fully fenced namespace (all 3 cycles burned) is immutable evidence. The next attempt runs
  from a NEW bridge script with a fresh state root, a new sudoers line, and the controller
  allowlist entry — the v1→v2 pair is the template; the regression suite pins byte-level process
  identity between them.

Stage 7 GREEN = the rollout for the fresh pair is `confirmed` with every shadow policy job
ACKed. Verify in commerce: `pricing_shadow_rollouts_v2.status = 'confirmed'` for the pair.

## Phase D — Stage 8 evidence and Stage 9 head advance

Both run through the AdminGuard commerce API (the pricing control room surfaces them; the raw
endpoints are `POST /v1/admin/pricing-stage8-capture-v2/stage` and
`POST /v1/admin/pricing-release-activation-v2/stage` on the commerce admin origin
`http://127.0.0.1:8791`, with a verified `x-admin-actor`):

1. Stage the Stage 8 capture for the exact fresh target/recovery pair (closed window by commerce
   DB time, `min_samples_per_provider` 8, `financial_sample_size` 100, the Gemini admissions
   audit count for the window). The durable worker collects the read-only engine evidence,
   persists the raw bytes first, runs the double OpenKeys scan and the funding/runtime-floor
   checks, and completes `passed|blocked`. `blocked` is terminal evidence — read the blockers in
   the control room and fix the producer; never weaken the check. The capture mode is selected
   by the engine head alone: absent head is cutover evidence, head equal to the target is
   recovery evidence, and a head BEHIND the pair is successor evidence — the mode that every
   post-cutover model admission uses. A successor capture skips the frozen legacy shadow/binding
   gates and instead requires the exact full live inventory in BOTH base manifests: an account
   registered after pair preparation fails the capture closed, and the remedy is a fresh
   converge cycle (Phase C), never a hand edit.
2. Stage the Stage 9 activation with `activation_kind=successor` (cutover exists only for the
   absent head; recovery only for the forward step inside one pair), the canonical combined
   Stage 8 evidence digest from step 1, and a meaningful reason. The worker re-validates the
   full authority and performs the single-head CAS inside the evidence TTL (300 s) — stage the
   activation immediately after a passed capture. An exact replay returns `unchanged`; a lost
   ACK resends the durable body.
3. If the worker reports the job `dead` with a receipt/assertion error but the engine head
   already moved (check `GET /admin/pricing/v2/head` on the engine Control API), the CAS
   committed and only the commerce receipt is missing. Repair it with
   `POST /v1/admin/pricing-release-activation-v2/reconcile` (`job_id` + reason): it re-reads the
   engine provisioning context, requires an exact attestation of the job's immutable request,
   and durably stores the receipt. Without the receipt every later advance is blocked, because
   both the successor and the recovery expectations are read only from durable receipts.
4. Post-activation smoke on the exact SHA: a paid request on the new model settles with the
   release-v2 discount (B2C global 50% unless an override applies), the ledger row carries the
   new release generation, and `deploy/watchdog` is green.

Stage 8/9 failure playbook (all observed during gpt-image-2):

- `pricing_control_job_backlog_or_failure` with only old `dead` catalog/switch/policy jobs —
  check whether the same lineage later confirmed (same product/switch/binding at a newer
  generation/effective version). Since the recovered-lineage fix such jobs no longer block; a
  dead job WITHOUT a confirmed successor still does, and so does any dead release control job.
- `successor_target_not_newer_than_active_head` — the capture aimed at a pair at or behind the
  live head; stage a fresh pair instead.
- `target_release_identity_drift` listing `inventory` — the inventory moved after pair
  preparation (new account, status/multiplier change). Re-run the converge cycle for a fresh
  pair; the stale pair never activates.
- Activation staging `409 ... expired` — the 300 s evidence TTL elapsed; re-capture (step 1) and
  stage again immediately.

## Phase E — publication

Only after the advance is live and proven:

- `apps/web/src/lib/models.ts` + the docs portal and all storefront copy (grep the old model
  list, do not work from memory);
- OpenKeys: the model sells 1:1 through the OpenKeys catalog generation — check
  `assertOpenKeysCatalog` and its tests;
- `apps/admin` calculator `PRODUCT_CATALOG`;
- `crates/router/routing-presets.json` if the router exposes it;
- `docs/commerce/PRICING.md` and the provider doc;
- walk the "New model (in an existing provider)" checklist in `docs/CHANGE_CHECKLISTS.md` in
  full and state the applied items in the commit body.

## What this runbook deliberately does not do

No canary accounts, no traffic stop, no manual per-account migration, no hand-computed digests,
no edits of existing migrations or fenced operator state. If a step feels like it needs one of
those, the correct move is a producer fix and a fresh cycle, not a workaround.
