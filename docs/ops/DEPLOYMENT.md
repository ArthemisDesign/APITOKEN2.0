# Production deployment runbook

This is the operator runbook for `84.32.48.2`. Controller internals live in
[`deploy/README.md`](../../deploy/README.md), immutable layout rules in
[`deploy/RELEASES.md`](../../deploy/RELEASES.md), and the Stage 2 authority design in
[`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`](../engine/STAGE2_POSTGRES_AUTHORITY.md).

Pushing or merging to `master` triggers the production-host watchdog. It tests an isolated exact
commit, takes fresh validated database backups, applies commerce migrations, then health-gated
blue-green deploys only the affected engine and/or backend. Engine migrations run transactionally
inside the inactive slot and must pass readiness before admission. It reports every stage on the
GitHub commit without using a paid Actions runner. The manual component controllers below are recovery and
explicit operator tools, not the normal contributor workflow. See [`CONTRIBUTING.md`](../../CONTRIBUTING.md).

## Normal automatic delivery

Contributors and AI agents only merge production-ready work to `master`, then watch these GitHub
commit-status contexts:

Schema-dependent delivery uses two merges: an additive migration-only commit must complete first;
only after its migration and overall statuses are green may dependent application code reach
`master`. Contract cleanup is a later release.

| Context | Gate |
|---|---|
| `deploy/tests` | Path-selected isolated TypeScript/database, Rust, and operational test lanes |
| `deploy/migration` | Validated database backups plus exact tested commerce migrator, or no commerce migration needed |
| `deploy/engine` | Exact-release engine rollout, or no engine change |
| `deploy/backend` | Exact-release API/worker rollout, or no backend change |
| `deploy/sales` | Exact tested Sales release rollout, or no sales change |
| `deploy/openkeys` | Exact tested OpenKeys release rollout, or no OpenKeys change |
| `deploy/admin` | Exact tested admin panel release rollout, or no admin change |
| `deploy/devbot` | Exact tested devbot release rollout, no devbot change, or devbot disabled (`/etc/apitoken/devbot.env` not yet provisioned) |
| `deploy/watchdog` | End-to-end result |

Affected stages also appear as GitHub deployment records in the `production-database`,
`production-engine`, `production-backend`, `production-sales`, `production-openkeys`,
`production-admin`, and
`production-devbot`
environments. This is reporting only: builds and deployments run on the existing production host,
so no paid GitHub runner is used.

Alongside the serialized production watchdog, a low-priority candidate-validator service may run
two transient `candidate-validation` deployments concurrently for exact SHAs reachable from pushed
feature branches. The merge client first rebases onto the latest committed `master`, then creates
that request before its fail-closed path-aware local gate, so local validation, trusted-host
validation, and an active parent rollout may overlap. Each host worker has its own disposable
PostgreSQL port, Cargo target, status file, and immutable candidate tree. Git fetches are briefly
serialized, and work for the same SHA has a per-SHA lock so production waits for and reuses that
exact build instead of rebuilding it.

The candidate selector tests only the feature delta from the committed parent. Before reporting
green, the worker builds the complete deployable TypeScript artifact set but limits typecheck and
tests to the changed package, its workspace dependents, and their internal prerequisites. Shared
pnpm/TypeScript inputs, deletions, unknown paths, and selector changes force the full workspace.
Each fresh candidate restores the four Next.js `.next/cache` trees from fixed host-local archives
and atomically refreshes them after a successful build. Concurrent validators are last-writer-wins
between complete archives; an absent, corrupt, or symlink-containing cache degrades to a clean build.
The immutable marker records whether coverage was full and, for a filtered run, its exact base SHA;
production reuses it only for compatible coverage. The worker then fetches `master` again and
requires the current committed tip still to be an ancestor of the candidate. The locked merge still
requires the parent’s `deploy/watchdog` verdict to be green; if `master` moves incompatibly, the
client rebases and both exact-SHA gates run again. A failed candidate request is isolated from
production: it reports a red request and `deploy/tests` for that feature SHA without writing the
production quarantine marker or changing `deploy/watchdog`. Candidate work is CPU/I/O deprioritized;
actual production remains one SHA at a time under its existing deployment lock.

Both watchdogs poll every five seconds. The candidate queue uses one state-bearing GitHub query per
poll rather than one request per historical deployment. A production failure quarantines that SHA
and stops the pipeline; neither later migrations nor application cutovers are attempted. This holds for every
abnormal termination — a failing command, an interrupt, or a validation failure raised internally —
so a stopped pipeline always leaves a quarantine marker and a red commit status rather than stopping
silently. A failure before any commit is selected (an unreachable remote, a missing state file) is
an infrastructure fault rather than a verdict on a commit: it is logged and retried on the next
cycle without quarantining anything. Commerce migration
failure always blocks the backend. Engine migration or readiness failure leaves the serving engine
slot untouched. Expensive retention and production-alignment checks remain on a separate one-minute
idle cadence, where the watchdog requires exactly one slot for Anthropic, OpenAI, and supported
Gemini to be active, ready, selected on the recorded release, enabled, and running their
fixed provider modes. If an out-of-band service command
reactivates the inactive slot, the watchdog reconverges through the same readiness-gated controller;
it never stops the availability anchor before another current slot is verified. Normal releases
require no SSH command.

```bash
# Operator observation only
sudo apitoken-watchdog status
sudo apitoken-watchdog logs
```

Runtime operational-definition changes (`deploy/`, `systemd/`, `observability/`) are also automatic.
After the exact immutable candidate passes the selected path-aware gate, a fixed root-owned bridge
verifies its SHA/tree/test marker, independently derives the exact install scope, and installs only
the selected controller, Caddy, systemd, and/or monitoring definitions. Mixed narrow concerns
compose in one transaction; deletions, privileged/stateful definitions, and unknown files still use
the complete bootstrap installer. The
installed and exact-candidate controllers each emit a versioned validation plan; the host runs
their union, and the frozen marker binds both the effective plan and the candidate policy digest.
A controller-only update then transfers the already-held deployment lock directly to the new
root-owned controller, without another poll or validation pass. Caddy and monitoring updates
continue in the same process. Any systemd scope keeps `deploy/watchdog` pending until the next
five-second poll because only a fresh manager invocation receives the updated service sandbox.
The root `compose.yaml` is a local-development definition and does not reinstall production.
Test-only deployment scripts,
documentation, and the contributor-side merge workflow still run the operational regression lane
but do not reinstall the production controller. A changed Caddy template is rendered with the
existing production secrets, validated, reloaded with automatic rollback, and never copied with
repository placeholders. GitHub workflow changes do not alter the production host and therefore
need no host-install stage.

After any required commerce migration and engine pre-deploy backup finish, component delivery uses
joined failure-isolated lanes. Engine and commerce stay serial because both blue-green controllers
own the same deployment lock. Sales and OpenKeys use separate databases, release roots, symlinks,
units, and rollback paths, so their health-gated rollouts may run concurrently with that core lane.
Final cross-component verification and the overall green verdict run only after every started lane
succeeds.

An operator can request an immediate poll or retry a proven transient failure:

```bash
sudo apitoken-watchdog run
sudo apitoken-watchdog retry <full-40-character-sha>
```

Prefer a new corrective commit for code/test/migration failures. A retry is not permission to alter
the immutable candidate or production database by hand.

### One-time GitHub reporting credential

Use a fine-grained personal access token limited to this repository with only **Commit statuses:
read/write** and **Deployments: read/write** (GitHub adds metadata read automatically). No Actions
permission, hosted runner, webhook, or paid GitHub feature is required. Store it only in the
root-owned file consumed by the reporting bridge:

```bash
sudo install -o root -g root -m 0600 /dev/null /etc/apitoken/github-watchdog.env
sudoedit /etc/apitoken/github-watchdog.env
```

```dotenv
GITHUB_REPOSITORY=OWNER/REPOSITORY
GITHUB_TOKEN=github_pat_REDACTED
```

Never put this token in a systemd environment, candidate checkout, application environment, shell
command line, or repository file. The watchdog calls a fixed root-owned bridge through narrowly
scoped sudo; tested candidate code runs as `apitoken-ci` and cannot read the credential. Revoke and
replace it immediately if its file permissions, logs, or host boundary are compromised.

### Least-privilege sudo policy

The `deploy` operator holds only the privileges the pipeline actually uses, defined in
`deploy/sudoers.d/95-apitoken-deploy`. This is what makes the sentence above true: with an
unrestricted `NOPASSWD: ALL` grant, `deploy` can read the GitHub credential and every
`/etc/apitoken/*.env` secret, and can replace the root-owned controllers that are meant to be fixed
trust anchors.

Applying the policy is a deliberate operator action, not an automatic deployment step — a policy
that locks out the pipeline cannot be repaired by the pipeline it governs:

```bash
sudo deploy/install-sudoers.sh --check
sudo deploy/install-sudoers.sh
sudo apitoken-watchdog status
```

The installer validates the candidate with `visudo -c` (treating warnings as fatal, since an unused
alias means an intended privilege is silently not granted), saves timestamped rollback copies under
`/root/sudoers-backups`, removes the legacy unrestricted grant, then verifies every privilege the
pipeline needs and every privilege it must not have. If any check fails it restores the previous
policy automatically and exits non-zero. It also removes `apitoken-ci` from the `deploy` group, so
candidate-derived test code can no longer write group-writable files in the deployment checkout.

The policy deliberately permits `deploy` to re-run the installer at its fixed root-owned path.
Without that, removing the unrestricted grant would be irreversible without console access.

When changing the policy, run `--check` first and keep a second session open until
`sudo apitoken-watchdog status` and a real watchdog cycle have both succeeded.

If you guard the change with a `systemd-run --on-active` timer that restores the old policy, note
that the installed policy deliberately does not permit stopping arbitrary units, so you cannot
cancel that timer once the policy is active. Either let it fire and re-run the installer afterwards
(its own path is permitted), or cancel the timer before applying the policy.

## Non-negotiable rules

- Use a full 40-character Git SHA that passed the integrated suite.
- Deploy engine and commerce API independently. Do not use unqualified `deploy.sh` after the
  PostgreSQL cutover.
- Never restart `apitoken-postgres.service` during an application deploy or rollback.
- Never manually restart API/engine between release selection and blue-green cutover.
- Never edit a finalized directory below `/opt/apitoken/releases` or `/srv/claude-api/releases`.
- Commerce migrations are append-only, expand/contract, and forward-only; the watchdog backs up and
  applies them automatically before application cutover. Binary rollback never reverses them.
- Engine migrations are ordered, advisory-locked, per-version transactional, and forward-only. The
  fixed root migration helper applies them before the inactive candidate starts while the old slot
  remains serving; readiness failure prevents Caddy admission and preserves the old slot. Engine
  startup only verifies the schema and never issues DDL. Never edit an already-applied migration.
- The one-time SQLite-to-PostgreSQL cutover is complete. Do not rerun it for a normal release.

## Pricing evaluation-shadow rollout

The Stage 3B1c.3 application release is safe to deploy with the producer disabled. Shipping the
binary is not authorization to activate it: production activation is a separate observed config
checkpoint after the default-off SHA has a green exact-SHA `deploy/watchdog`. The producer may run
only on a PostgreSQL-backed fixed Anthropic, OpenAI or Gemini plane with billing enabled; live
SQLite composition remains unsupported. Because the fleet env file is shared by every plane,
enabling the shadow env leaves non-pricing planes (KIMI) inert with an explicit startup notice
instead of failing their boot. The durable provider identity of the Gemini plane is
`google`, never `gemini`. Each plane keeps an independent default-off config and must complete the
same observed rollout ladder after the producer SHA is green.

This checkpoint produces immutable legacy-scalar actual snapshots and target-policy shadow
evaluations for all three planes. It does not enable strict Gemini, create a release-v2
reserve/settlement snapshot or authorize Stage 9. Stage 8/9 must not proceed until 100% target
shadow coverage includes real Google snapshot/evaluation rows and the remaining release/funding
runtime is delivered. External Gemini usage/admission counters are audit context, not a substitute
for those rows.

The startup validator rejects unknown boolean spellings, incoherent enabled/sample pairs, and every
value outside these bounds:

| Environment variable | Default | Accepted bound |
|---|---:|---:|
| `CLAUDE_API_PRICING_SHADOW_ENABLED` | `false` | strict `0|1|false|true` |
| `CLAUDE_API_PRICING_SHADOW_SAMPLE_BP` | `0` | disabled: `0`; enabled: `1..=10000` |
| `CLAUDE_API_PRICING_SHADOW_QUEUE_CAPACITY` | `256` | `1..=4096` |
| `CLAUDE_API_PRICING_SHADOW_WORKER_CONCURRENCY` | `2` | `1..=32`, not above queue capacity |
| `CLAUDE_API_PRICING_SHADOW_TIMEOUT_MS` | `750` | `10..=15000` |
| `CLAUDE_API_PRICING_SHADOW_MAX_QUEUE_AGE_SECS` | `300` | `1..=86399` (`<24h`) |
| `CLAUDE_API_PRICING_SHADOW_MAX_FIELD_BYTES` | `512` | `64..=4096` |
| `CLAUDE_API_PRICING_SHADOW_MAX_ITEM_BYTES` | `16384` | `1024..=131072`, not below field limit |
| `CLAUDE_API_PRICING_SHADOW_RATE_PER_SEC` | `20` | `1..=10000` |
| `CLAUDE_API_PRICING_SHADOW_RATE_BURST` | `40` | `1..=rate_per_sec*60` |
| `CLAUDE_API_PRICING_SHADOW_DB_READ_CONNECTIONS` | `2` | `1..=8` |

**Never set these in a shared engine `EnvironmentFile`.** `/srv/claude-api/data/server.env` and
`config.env` are read by *every* engine slot, and the startup validator rejects the shadow producer
on any plane other than Anthropic, OpenAI or Gemini (`crates/server/src/main.rs`, "pricing shadow
producer requires a fixed Anthropic, OpenAI, or Gemini provider plane"). A shared enablement
therefore makes the KIMI slot — and any future non-producer plane — fail to start. Because the
release is verified only after traffic is committed, the whole engine rollout then rolls back and
quarantines whatever candidate SHA happened to be in flight, regardless of what that commit changed.
This happened on 2026-08-04: the switch was added to `server.env`, and the next unrelated candidate
was quarantined with `claude-api-kimi@8805.service did not become ready on current release`.

Enable the shadow the same way every other plane-scoped switch is enabled: pinned argv-level in the
reviewed unit of the one producing plane, exactly as `CLAUDE_API_KIMI_ENABLED=1` is pinned in
`systemd/claude-api-kimi@.service`. Argv assignments cannot be overridden by a shared
`EnvironmentFile`, and the plane's state stays visible in its own unit instead of leaking sideways
into planes that must never run it.

Current production state: all three producing planes pin
`CLAUDE_API_PRICING_BRIDGE_ENABLED=1 CLAUDE_API_PRICING_BRIDGE_SAMPLE_BP=10000
CLAUDE_API_PRICING_SHADOW_ENABLED=1 CLAUDE_API_PRICING_SHADOW_SAMPLE_BP=10000` argv-level in
`systemd/claude-api-anthropic@.service`, `systemd/claude-api-openai@.service` and
`systemd/claude-api-gemini@.service`, because the Stage 8 evidence gate requires full shadow
coverage of supported traffic. The bounded producer (queue drop, never traffic backpressure) ran
at a 100% sample on the Anthropic plane for two hours before this pinning with no customer-facing
impact; KIMI and any future non-producer plane stay inert by construction.

Keep every ceiling fixed while changing only one rollout dimension on one fixed plane at a time.
For Anthropic, OpenAI and Gemini the required order is: default-off binary → bridge small sample →
bridge target sample → bridge 100% of eligible traffic → shadow small sample → wider shadow sample
→ 100% of snapshot-bearing eligible traffic. Observe a complete peak traffic interval between
steps. Before each increase, compare customer admission and reserve p95/p99, customer
5xx/status/body, billing FIFO depth, engine PostgreSQL connections and lock waits, shadow queue
depth/high-water/age, enqueue drop ratio, processing/read/write errors, CPU, and memory against the
recorded pre-activation baseline. Early shadow before policy backfill validates transport and
persistence only; it is not Stage 8 financial-parity evidence.

Disable the independent shadow switch immediately, without changing schema or using shadow output
as a rollback input, when any of these stop criteria occurs:

- customer response, readiness, reserve, settlement, or actual charge depends on a shadow result;
- sustained queue saturation/drop ratio exceeds the pre-recorded allowance;
- PostgreSQL connection/lock pressure, admission/reserve p95/p99, customer 5xx, CPU, or memory
  materially regresses from baseline;
- an idempotency conflict, invariant alert, continuous read/write error storm, or unexplained
  timeout/cancellation spike appears;
- an actual snapshot reference or resolved lineage cannot be proven from durable rows.

An eligible atomic-bridge DB/constraint failure is a separate bridge stop condition: disable the
bridge rather than falling back to a second reserve. Queue full/closed, rate/size drops, shadow
timeouts, and shadow read/write failures remain metrics-only and must not alter customer traffic or
money. A funding-capped actual remains eligible and applies the same immutable ceiling to the policy
candidate; an actual above the checked scalar quote is an invariant failure. Use the
`claude_api_pricing_shadow_*` bounded series and the single runtime manifest info sample for rollout
evidence; account, key, request, and model identities belong only in protected durable attribution,
never metric labels or error storms.

## Managed Stage 5/6 preparation

Production Stage 5/6 is driven only through the AdminGuard-protected commerce API. Do not run the
package CLIs over SSH, invoke migration SQL manually, pause traffic, stop money writers or wait for
zero inflight reservations. The API is reachable through the authenticated admin Caddy route
(`/admin/pricing-stage5-v2/*`, `/admin/pricing-stage6-v2*`) and as `/v1/admin/*` on the protected
loopback commerce origin. Caddy or the server operator supplies the admin credential; every request
also requires the verified `x-admin-actor`. Never print either credential.

Use this order:

1. `POST /v1/admin/pricing-stage5-v2/dry-run` with `{}`. Review the returned exact
   `plan_digest`, all source digests, target/recovery plan identities, `blocker_count` and the full
   exact blocker list. Do not infer an omitted owner or classification.
2. Do not materialize a report with unresolved ownership/inventory blockers. Fix the authority by
   its normal producer, repeat the full dry-run, and use only the newest digest.
3. `POST /v1/admin/pricing-stage5-v2/materialize` with exact `plan_digest` and a meaningful
   `reason`. The server repeats both exhaustive inventories. A stale digest fails before local
   commit. A stable blocker-free plan persists dormant target/recovery skeletons and exact engine
   prepare/readback ACKs; it does not create Stage 6, move a head, change a balance or affect
   admission. The local request and operator audit commit together.
4. Read `GET /v1/admin/pricing-stage6-v2?plan_digest=...`. Stage only an exact fully ACKed Stage 5
   run in `materializing` state.
5. `POST /v1/admin/pricing-stage6-v2/stage` with the same digest and a meaningful `reason`. Job
   creation and attributed audit commit together; replay is idempotent and returns the same job.
6. Poll the paired GET until the parent is `confirmed`, both releases are `prepared`, every balance
   account is `ready`, all pending/processing/retry/blocker counts are zero, and both funding
   manifest digests are present and equal. A `dead` parent or blocker is diagnosed and fixed in
   code/authority without stopping unrelated accounts.

Stage 5/6 completion is this production evidence, not deployed dormant code or a successful local
integration test. Neither operation stages Stage 8/9 activation; the global release head remains
unchanged throughout.

## Stage 8 synchronization evidence

Stage 8 is read-only full-inventory evidence for the zero-downtime release described in
`docs/commerce/MULTI-DISCOUNT.md`. It does not seed data, change a head, migrate funding or deploy
manually. There is no reviewed assignment matrix: authoritative commerce/OpenKeys/service
inventories must cover every engine account exactly once.

Choose a `window_start_ts` after the last target release materialization. Authority updates do not
require a traffic freeze, but any update changes the source generation and makes a captured report
stale. Keep bridge and target shadow at 100% coverage, observe at least one complete peak interval,
and choose its exclusive `window_end_ts`. Traffic, top-ups and v2 reservations continue normally.

Production capture uses the managed commerce queue; do not create a file handoff or run either CLI
over SSH. The normal operator surface is `https://admin.apitoken.sale/pricing` → `Managed Stage 8
capture`: it reads the AdminGuard-protected `GET /v1/admin/pricing-stage8-capture-v2` snapshot,
requires exact bounds plus a confirmation phrase, repeats a fresh preflight and stages exactly one
immutable request through `POST /v1/admin/pricing-stage8-capture-v2/stage`. The underlying request
has a new UUID idempotency key, verified `x-admin-actor`, explicit reason and this strict body:

```json
{
  "idempotency_key": "<uuid>",
  "target_generation": 41,
  "recovery_generation": 42,
  "window_start_ts": 1785700000,
  "window_end_ts": 1785700300,
  "min_samples_per_provider": 100,
  "financial_sample_size": 100,
  "gemini_client_admissions": 27,
  "reason": "capture reviewed full-inventory Stage 8 window"
}
```

The window must already be closed according to commerce database time. `gemini_client_admissions`
is a bounded independently aggregated client-edge audit count for the same half-open window and
must never contain identities; it does not substitute for Google provider coverage. Exact replay of
the same idempotency key and body returns the original job, while any conflicting reuse fails
closed. The POST writes only the durable job and attributed audit event; it does not call the engine
inline.

`apps/worker` claims at most one Stage 8 capture globally and calls the protected
`POST /admin/pricing/v2/stage8-evidence/capture` producer with the seven explicit capture values.
The engine attaches the deployed compile-fixed runtime manifest server-side and returns the
unwrapped schema-v2 report with HTTP 200 even when `passed=false`. The typed client reads bounded raw
text and parses with `json-bigint`, never `response.json()`, because the artifact contains signed-i64
nanoUSD JSON numbers. Before any local collection the worker stores those exact response bytes and
their verified digest in the append-only attempt artifact. It then immediately runs the combined
collector and atomically stores the combined bytes plus terminal `passed|blocked` job state.

The collector verifies the canonical Rust
`sha256:v2` length-prefixed engine digest, rejects an engine source older than 120 seconds and
exhausts both engine and OpenKeys cursors twice around one commerce `SERIALIZABLE` snapshot. It
recomputes commerce/service identities and verifies exact ownership/status, B2B scalar parity,
OpenKeys 1:1 in the prepared target policy, the prepared target/recovery generations, semantic assignment lineage, engine
release/funding identities and control-job backlog. После cutover immutable base manifest не
переписывается: каждый новый account обязан иметь exact target/recovery assignment extension,
matching policy и active funding generation/head/aggregates. Live balances не входят в стабильный
engine identity digest и потому normal traffic/money writes не создают ложный inventory drift.
Account/request/binding subjects are emitted only as digests; neither command prints a database
DSN.

Pre-cutover OpenKeys source/engine scalars remain legacy runtime state until the one-head CAS and
are not rewritten early. The authority gate instead requires every target OpenKeys assignment to
reference the canonical `openkeys` policy with exactly one global 0%-discount/10000-bp rule. The
same release-v2 proof replaces legacy binding `reconciliation_state=verified` and the retired
`funding_buckets` projection: a valid shadow binding may remain `pending`, while exact target
assignment coverage and funding generation/head/lot parity stay mandatory.

Новая evidence row обязана хранить тот же service-inventory digest в
`pricing_stage8_evidence_v2.service_inventory_digest`. Legacy row с `NULL` не допускается к
activation staging; непосредственно перед первой delivery worker повторно вычисляет digest и
сравнивает его с persisted evidence. Это отдельная проверка от immutable target plan identity и
закрывает post-cutover service-account drift.

The combined schema-v2 report is valid for 300 seconds. When both local release plans exist, the
consumer stores its identity immutably in `pricing_stage8_evidence_v2`, including blocked reports
with `passed=false`; if either local release is absent it returns `write_result=not_persisted`.
`legacy_inflight_reservations` and `legacy_inflight_outbox_rows` remain exact audit evidence and
contribute to `legacy_inflight_count`, but a nonzero count is not a blocker: do not pause traffic,
drain writers or wait for zero before collecting Stage 8.
Blockers are terminal captured evidence (`status=blocked`), while malformed/tampered evidence fails
closed (`dead`). Uncertain transport/authority failures retry with bounded delay, lease and attempt
count; expired leases below the limit return to `retry`, and an expired final attempt becomes
`dead`. Poll the paired GET until the requested job is terminal and review its engine/combined
digests, freshness and sanitized blocker source/code/count/digests. The response exposes the exact
total plus at most 100 blocker details and a truncation flag. Stage 9 accepts only the exact,
unexpired persisted combined row with
`passed=true`.
Engine subject hashes use their established canonical `sha256:v1` domain and commerce authority
subjects use canonical `sha256:v2`; the bounded status contract accepts both opaque forms without
relaxing any `sha256:v2` evidence/release identity.

Worker bounds are validated at startup: `STAGE8_CAPTURE_POLL_MS=5000` (`1000..60000`),
`STAGE8_CAPTURE_LEASE_MS=300000` (`30000..3600000`),
`STAGE8_CAPTURE_RETRY_MS=15000` (`1000..3600000`) and
`STAGE8_CAPTURE_MAX_ATTEMPTS=10` (`1..100`). Defaults are production-safe; changing them is a
separate observed worker configuration change, not part of staging a capture.

Migration `0033_pricing_stage8_managed_capture.sql` supplies the immutable job inputs and
append-only original engine/combined JSON per attempt. It is storage-only: migration, worker
startup, polling, activation staging and read-only status never create a capture job. Capture never
creates an activation job, changes a release head, account, balance, policy, traffic or money
writer. The legacy `claude-api db stage8-evidence` and
`pnpm --filter @claude-api/db pricing:stage8-evidence` commands remain parity/diagnostic tools for
controlled non-production tests, not the production control-plane.

Migration `0035_pricing_shadow_rollout_jobs.sql` supplies the durable lane for the required
generation-3 pre-cutover policy alignment of the OpenKeys inventory, whose accounts are not
commerce-local bindings. Commerce B2C/B2B and service lineages are aligned by their managed policy
writers instead: the engine never accepts a different policy identity on an account with an
existing lineage, and the v1 shadow format cannot express service `meter_only`. The lane is
delivered: the only rollout
producer is the AdminGuard-protected `POST /v1/admin/pricing-shadow-rollout-v2/stage` in
`apps/api` (UUID idempotency key, exact `stage5_run_id`, verified `x-admin-actor`, reason), which
pins a prepared target/recovery pair and persists the complete policy/binding/CAS body of every
OpenKeys job before any delivery; the bounded `apps/worker` consumer claims per-account jobs with a
lease, advances canonical OpenKeys lineages in place at the next monotonic version through
prepare/readback/activate and
replacement-locked legacy OpenKeys only through `locked-openkeys-transition`, stores exact ACK
digests/payloads and atomically closes the rollout `confirmed|blocked|dead`. The paired
`GET /v1/admin/pricing-shadow-rollout-v2` exposes only bounded aggregates and subject digests.
Startup, migration, polling and the read endpoint never create a rollout or job; the lane never
moves the release head, balances, live price or money writers. Operators must not emulate the lane
with SQL, SSH loops or direct per-account mutations.

Shadow rollout worker bounds are validated at startup: `PRICING_SHADOW_ROLLOUT_POLL_MS=5000`
(`1000..60000`), `PRICING_SHADOW_ROLLOUT_LEASE_MS=300000` (`30000..3600000`),
`PRICING_SHADOW_ROLLOUT_RETRY_MS=15000` (`1000..3600000`),
`PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS=10` (`1..100`) and
`PRICING_SHADOW_ROLLOUT_BATCH_SIZE=25` (`1..500`). Defaults are production-safe; changing them is
a separate observed worker configuration change, not part of staging a rollout.

`sales_contract_digest` binds the intended B2C `paid_funded_nano`/no-welcome-bonus commission
contract. It is a contract identity, not proof that the sales runtime consumer is deployed; exact
sales v2 runtime evidence remains a separate pre-cutover requirement.

Required target evidence includes:

- Anthropic, OpenAI and Gemini capability/catalog/switch lineages;
- global B2C 50% plus exact provider/model override vectors;
- every B2B policy, canonical OpenKeys 1:1 and service `meter_only` assignment;
- Stage 6 funding generation for every account;
- exact format-aware counts for unfinished legacy reservations/outbox rows; nonzero legacy и
  active-v2 work допускаются и продолжают settlement по immutable reserve-time snapshots;
- 100% shadow coverage, exact nanoUSD parity and no unresolved outcome;
- exact prepared target/recovery release and recovery-link digests, with equal runtime/funding
  lineage and one assignment for every active or disabled engine account;
- current engine inventory and target funding-manifest digests, active funding heads and exact
  aggregate/lot parity;
- target release rule precedence against every shadow evaluation;
- release/funding schema v2 claims and a nonempty runtime digest on every live engine instance,
  in addition to compile-fixed pricing capability support;
- no pending/processing/retry/dead pricing control jobs.

Gemini traffic is required product evidence, not a substitute for durable coverage. Google must have the configured minimum
of immutable snapshots and matching evaluations just like Anthropic and OpenAI; usage/outbox or the
external aggregate alone cannot pass coverage. Any missing provider sample, stale ACK, unclassified
account, funding mismatch, runtime drift or catalog/policy mismatch fails the report. Do not edit
rows to make it green.

The Stage 8 producer checkpoint is intentionally fail-closed before the Stage 9 runtime checkpoint:
the current runtime claim writer does not yet populate the release-v2/funding-v2 columns, so
`live_runtime_below_release_v2_floor` is expected until that compatible runtime is deployed on all
live slots. Do not weaken this blocker and do not treat a producer-only report as completed Stage 8.

Immediately before Stage 9, regenerate the engine input and combined report and require a fresh,
persisted `passed=true` combined identity. Первый claim activation job повторяет full authority
capture непосредственно перед network delivery. Если delivery могла состояться, expired lease
повторяет exact durable request без новой TTL/authority проверки, чтобы безопасно получить
`unchanged` после lost ACK. Stage 9
changes one global active release head; it does not select a canary list and does not require a
maintenance window or zero active v2 reservations. The complete apply/recovery procedure is
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`.

## Local pre-push test gate

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm typecheck
pnpm test
cargo test --workspace
bash -n deploy/*.sh deploy/apitoken-db-dump
git diff --check
```

The Stage 2 host E2E destroys only a uniquely named temporary role/database. Run it only on a host
with the test PostgreSQL container, never against `claude_engine`:

```bash
sudo deploy/test-stage2-e2e.sh /path/to/test/claude-api
```

## Manual recovery: select the production SHA

```bash
ssh deploy@84.32.48.2
cd /opt/apitoken/repo
git fetch origin master
git merge --ff-only origin/master
SHA=$(git rev-parse origin/master)
test ${#SHA} -eq 40
printf '%s\n' "$SHA"
```

The controllers fetch and verify the supplied commit again. API, worker, and Content Studio processes run from
the immutable release selected by `/opt/apitoken/releases/current`; the host checkout is only the
controller source and may retain reviewed host-specific files.

## Manual recovery: deploy the Rust engine

```bash
deploy/deploy.sh --engine-bluegreen --dry-run "$SHA"
deploy/deploy.sh --engine-bluegreen "$SHA"

deploy/engine-bluegreen.sh --dry-run
deploy/engine-bluegreen.sh
deploy/router-bluegreen.sh --dry-run
deploy/router-bluegreen.sh
```

Phase 1 builds and finalizes `/srv/claude-api/releases/<sha>`, then atomically selects it without
touching either provider. Before Phase 2 starts a slot, `engine-bluegreen.sh` applies pending engine
PostgreSQL migrations through the fixed root helper. Phase 2 then starts the inactive 8787/8788 Anthropic slot, proves its exact
`MainPID`, binary and startup-fixed mode, admits it through Caddy, flips the old slot to 503 readiness
with `SIGUSR1`, and fully stops its cgroup. It then health-gates the inactive OpenAI slot and, for
releases carrying `.gemini-bluegreen-v1`, the inactive Gemini slot, proving the same selected binary
in each startup-fixed provider mode before draining the predecessor. Releases carrying
`.kimi-bluegreen-v1` then health-gate the inactive KIMI slot (8804/8805) the same way; the KIMI
plane is enabled by the reviewed argv pin `CLAUDE_API_KIMI_ENABLED=1` in its units, so the cutover
itself never changes KIMI behavior.
On the first split, this order guarantees the old combined process releases every Codex home before
OpenAI starts; Gemini and KIMI remain separate subscription-pool failure domains throughout.

Phase 3 rolls the unified stateless router independently on fixed ports 8800/8801. The controller
starts the inactive slot, proves direct readiness and the exact selected `claude-router` executable,
then invokes the fixed root helper to atomically publish `/etc/caddy/router-active.caddy`, validate
and gracefully reload Caddy. Both the public hostname and stable loopback `127.0.0.1:8802` now send
new requests only to the target. The predecessor remains alive for established connections until a
post-cutover SIGTERM completes Axum's bounded drain. The first run uses the still-serving singleton
on 8798 as its old anchor; infrastructure installation never restarts it.

```bash
curl -fsS http://127.0.0.1:8790/ready
curl -fsS http://127.0.0.1:8792/ready
curl -fsS http://127.0.0.1:8794/ready
curl -fsS http://127.0.0.1:8803/ready
curl -fsS http://127.0.0.1:8802/ready
curl -fsS https://api.apitoken.sale/health
openai_probe=$(mktemp)
openai_status=$(curl -sS -o "$openai_probe" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d '{}' \
  https://openai.api.apitoken.sale/v1/responses)
jq -e --arg status "$openai_status" '.error.type == "invalid_request_error" and
  (($status == "401" and .error.code == "invalid_api_key") or
   ($status == "404" and .error.code == "model_not_found"))' "$openai_probe"
rm -f "$openai_probe"
curl -sS --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  -H 'content-type: application/json' -d '{}' \
  https://gemini.api.apitoken.sale/v1beta/models/gemini-provider-probe:generateContent \
  | jq -e '(.error.status == "UNAUTHENTICATED" and .error.code == 401)
      or (.error.status == "NOT_FOUND" and .error.code == 404)'
systemctl list-units 'claude-api-anthropic@*.service' 'claude-api@*.service'
systemctl list-unit-files 'claude-api-anthropic@*.service' 'claude-api@*.service'
systemctl status 'claude-api-openai@*.service'
systemctl status 'claude-api-gemini@*.service'
systemctl status 'claude-api-kimi@*.service'
systemctl status 'claude-router@*.service'
```

All provider slots alternate. Consumers must never hard-code 8787/8788, 8793/8797, 8795/8799, or
8804/8805; commerce always uses `http://127.0.0.1:8790`. OpenAI and Gemini clients use only their public
hostnames; stable origins 8792/8794 and every runtime slot remain loopback-only. KIMI is
backend-only: no public hostname or router namespace, and its stable origin 8803 with slots
8804/8805 stays loopback-only as well.
Provision paid Antigravity OAuth profiles first as documented in `docs/engine/GEMINI_PROVIDER.md`.

## Manual recovery: deploy the commerce API

```bash
deploy/deploy.sh --api-only --dry-run "$SHA"
deploy/deploy.sh --api-only "$SHA"

deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

Phase 1 builds `/opt/apitoken/releases/<sha>`, runs its prebuilt migration under the single migration
lock, and selects it without restarting the old API. Phase 2 alternates 3000/3001, exact-release
gates and admits the target, pre-drains the old process, and moves systemd boot persistence to the
verified target.

```bash
curl -fsS https://backend.apitoken.sale/v1/ready
systemctl list-units 'apitoken-api@*.service'
systemctl list-unit-files 'apitoken-api@*.service'
```

## Worker changes

The worker remains single-instance, but runs from
`/opt/apitoken/releases/current/apps/worker`. Its stop/start creates a short processing gap but no
overlap; durable jobs remain in PostgreSQL.

Build/select the exact immutable SHA and its required workspace packages before restarting it:

```bash
pnpm install --frozen-lockfile
pnpm --filter @claude-api/contracts build
pnpm --filter @claude-api/db build
pnpm --filter @claude-api/engine-client build
pnpm --filter @claude-api/payment-worker build
```

If API and worker changed together, replace the plain API cutover with:

```bash
deploy/api-bluegreen.sh --with-worker --dry-run
deploy/api-bluegreen.sh --with-worker
```

If only the worker changed, first select/build its immutable commerce release, then restart it:

```bash
deploy/deploy.sh --api-only --skip-migrate "$SHA"
sudo systemctl stop apitoken-worker.service
sudo systemctl start apitoken-worker.service
systemctl is-active apitoken-worker.service
```

`--with-worker` restarts the worker and Content Studio, then verifies the studio health endpoint and
both exact working directories. Phase 1 (`deploy.sh --api-only`) must already have built and selected
the release.

## Changes spanning engine and API

Both sides must remain compatible with the prior version during rollout. Prefer additive protocol
and schema changes. Ordinarily deploy and verify the engine first, then deploy the commerce API. If
a change needs the reverse order, the old engine must already understand the new API calls. There is
no atomic cross-component switch.

## ClaudeStore emergency fallback

The binary is safe to deploy with neither credential: both strict switches default to disabled.
Enable a plane only after the exact implementation SHA is watchdog-green and its authorization/live
evidence in `docs/engine/CLAUDESTORE_FALLBACK.md` is complete. Put secrets only in root-owned
`/srv/claude-api/data/server.env` through the host secret-provisioning path; never place them in Git,
a command line, chat transcript, fixture or deploy log. Keep that file mode `0600`.

The Claude pair is `CLAUDE_API_CLAUDESTORE_FALLBACK_ENABLED` plus
`CLAUDE_API_CLAUDESTORE_API_KEY`; it belongs only to Anthropic/Combined. The GPT pair is independent:
`CLAUDE_API_CLAUDESTORE_CODEX_FALLBACK_ENABLED` plus
`CLAUDE_API_CLAUDESTORE_CODEX_API_KEY`; it belongs only to OpenAI/Combined and also requires the
normal Codex provider enabled with a nonempty sealed roster. ClaudeStore must switch that second key
to Codex tier. Never reuse or retier the working Basic/Claude fallback key: doing so disables its
Messages access. Enabled-without-own-key or a switch on the wrong fixed plane fails startup.

Because secret/config env files are shared, provider isolation is argv-level: OpenAI units pin only
the Claude switch to `0` and intentionally inherit the GPT switch; Anthropic slots pin the GPT switch
to `0`; Gemini singleton/template units pin both. Do not move these overrides to `Environment=`,
which a later `EnvironmentFile` may supersede.

Use the normal watchdog-controlled cycle so a fresh inactive slot reads the selected pair, then run
the bounded authenticated smoke from the provider document. For GPT, first verify key-scoped models,
then one minimal non-stream request with terminal usage and a real incremental SSE request for each
allowed model/control; no production enable is permitted while
`research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md` remains credential-blocked. Confirm one
attempt/settlement and zero local subscription calibration attribution. Roll back immediately by
setting the plane's switch to `0` (or removing its secret) through the same controlled path and
cycling that plane; no database rollback is required. Observe
`claude_api_claudestore_fallback_{attempts,successes,failures}_total` and the
`ClaudeStoreFallbackFailing` runbook while enabled.

## Caddy and systemd definition changes

Application deploys do not silently replace host infrastructure definitions. If `deploy/Caddyfile`
changed, use the secret-preserving installer; never copy the placeholder-bearing repository file
directly over production:

```bash
sudo deploy/install-caddy.sh --check
sudo deploy/install-caddy.sh
systemctl is-active caddy
sudo ss -ltnH 'sport = :8790'
sudo ss -ltnH 'sport = :8792'
sudo ss -ltnH 'sport = :8794'
sudo ss -ltnH 'sport = :8803'
```

The installer extracts the existing host-only bcrypt/control-key lines without printing them,
validates the rendered candidate, saves a timestamped rollback copy, and performs a Caddy reload
rather than stop/start. Ports 8790, 8792, 8794, and 8803 must be bound to `127.0.0.1`, never `*`.

Normal release selection also does not reinstall systemd templates. When a reviewed template itself
changes, verify and install it before the matching blue-green cycle; `daemon-reload` does not replace
the already-running process:

```bash
sudo systemd-analyze verify systemd/claude-api.service systemd/claude-api@.service \
  systemd/claude-api-anthropic@.service systemd/claude-api-openai.service \
  systemd/claude-api-openai@.service systemd/claude-api-gemini.service \
  systemd/claude-api-gemini@.service systemd/claude-api-kimi.service \
  systemd/claude-api-kimi@.service systemd/apitoken-api@.service
sudo install -o root -g root -m 0644 systemd/claude-api.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-anthropic@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-openai.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-openai@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-gemini.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-gemini@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-kimi.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-kimi@.service /etc/systemd/system/
sudo install -d -o deploy -g deploy -m 0700 \
  /srv/claude-api/data/gemini /srv/claude-api/data/gemini/credentials \
  /srv/claude-api/data/kimi /srv/claude-api/data/kimi/credentials
sudo install -o root -g root -m 0644 systemd/apitoken-api@.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/apitoken-tmpfiles.conf /etc/tmpfiles.d/apitoken.conf
sudo systemd-tmpfiles --create /etc/tmpfiles.d/apitoken.conf
sudo systemctl daemon-reload
```

Install only definitions actually changed. The subsequent component controller starts the inactive
slot under the new definition and preserves the old running slot as the rollback anchor.

## Rollback

Engine rollback to `previous`:

```bash
deploy/rollback.sh --engine-bluegreen --dry-run
deploy/rollback.sh --engine-bluegreen
deploy/engine-bluegreen.sh --dry-run
deploy/engine-bluegreen.sh
deploy/router-bluegreen.sh --dry-run
deploy/router-bluegreen.sh
```

API rollback:

```bash
deploy/rollback.sh --api-only --dry-run
deploy/rollback.sh --api-only
deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

An explicit existing SHA may follow the selector. Rollback changes links and slots but never reverses
a database migration. A release whose migration breaks the prior binary is not rollout-safe.
Rolling back to a release predating the KIMI plane additionally stops and disables every
`claude-api-kimi` incarnation (the 8804 singleton anchor and both slot units), mirroring the
pre-Gemini rollback branch; the plane's default-off argv pin means this never interrupts traffic.

### Automatic post-admission rollback

Before traffic is admitted, the blue-green controllers already fail closed: the old slot keeps
serving and nothing is rolled back. Once the new slot has been admitted and the old one drained,
however, a failed final verification would otherwise leave an unverified release serving with no
automatic way back. In that window only, the watchdog re-selects `previous` and re-runs the same
health-gated provider controller and then the atomic router controller.

This never masks the failure: the candidate is still quarantined and `deploy/watchdog` still goes
red. A successful rollback only changes what is serving while you investigate. If the rollback
itself fails, the warning says so explicitly — inspect the slots before any further mutation.

## Retention

Every delivery creates an immutable release per affected component and a validated pre-deployment
dump per database. Nothing else removes them, so the watchdog prunes both at the start of each
cycle, while it holds the exclusive lock and no deploy, rollback, or migration is in flight.

- Build candidates: removed after 24 hours (measured from test completion when a marker exists).
- Immutable releases: the newest ten per component root are kept.
- Pre-deployment dumps: the newest ten per database are kept.

`current`, `previous`, the recorded component SHAs, and any release backing a live process are
always retained regardless of those counts. Live releases are resolved from each unit's `MainPID`
through `/proc/<pid>/exe` and `/proc/<pid>/cwd`, exactly like the readiness gates, rather than
trusting the symlinks alone. The hourly `<database>.dump` rotation artifacts are never pruned — they
remain the authoritative recovery objects.

## Failure behavior

- A phase-1 failure restores original `current`/`previous` links.
- If phase 1 succeeded but phase 2 has not run, the old in-memory process keeps serving. Run the
  matching blue-green controller; do not restart old through the moved symlink.
- Before admission, a phase-2 failure stops only the failed target and preserves the old slot.
- After admission/pre-drain, recovery keeps the verified new slot rather than reviving a drained one.
- If automatic recovery reports it is incomplete, inspect that warning and the exact unit journal
  before making another mutation.

```bash
sudo journalctl -u 'claude-api-anthropic@*' -u claude-api-openai -u claude-api-gemini \
  -u 'claude-api@*' -u 'apitoken-api@*' -u apitoken-worker --since today
sudo caddy validate --config /etc/caddy/Caddyfile
systemctl is-active caddy apitoken-worker claude-api-backup.timer
```

## Backups

The hourly timer writes custom-format dumps of `commerce` and `claude_engine`; daily Borgmatic then
copies `/var/lib/apitoken/backups` off-host.

```bash
sudo install -o root -g root -m 0644 systemd/claude-api-backup.service /etc/systemd/system/
sudo install -o root -g root -m 0644 systemd/claude-api-backup.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now claude-api-backup.timer
sudo systemctl start claude-api-backup.service
systemctl show claude-api-backup.service -p Result
```

`commerce.dump` and `claude_engine.dump` must be non-empty, mode 0600, and readable by the matching
PostgreSQL `pg_restore --list`. Replication is not a backup; future HA still needs independent PITR.

## Final post-deploy gate

```bash
sudo deploy/configure-engine-control-url.sh --check
curl -fsS http://127.0.0.1:8790/ready
curl -fsS http://127.0.0.1:8792/ready
curl -fsS http://127.0.0.1:8794/ready
curl -fsS http://127.0.0.1:8803/ready
curl -fsS https://api.apitoken.sale/health
openai_probe=$(mktemp)
openai_status=$(curl -sS -o "$openai_probe" -w '%{http_code}' \
  -H 'content-type: application/json' \
  -d '{}' \
  https://openai.api.apitoken.sale/v1/responses)
jq -e --arg status "$openai_status" '.error.type == "invalid_request_error" and
  (($status == "401" and .error.code == "invalid_api_key") or
   ($status == "404" and .error.code == "model_not_found"))' "$openai_probe"
rm -f "$openai_probe"
curl -sS --resolve gemini.api.apitoken.sale:443:127.0.0.1 \
  -H 'content-type: application/json' -d '{}' \
  https://gemini.api.apitoken.sale/v1beta/models/gemini-provider-probe:generateContent \
  | jq -e '(.error.status == "UNAUTHENTICATED" and .error.code == 401)
      or (.error.status == "NOT_FOUND" and .error.code == 404)'
curl -fsS https://backend.apitoken.sale/v1/ready
systemctl is-active caddy apitoken-worker claude-api-openai claude-api-gemini claude-api-backup.timer
git status --short
git rev-parse HEAD
```

At idle, expect one live Anthropic owner, one live OpenAI owner, and one live Gemini subscription
provider, with no pending settlement work, leaked active capacity leases/inflight count, reserved
money, or duplicate charge request IDs. See the Stage 2 document
for data-level verification and the caveat that nonzero counts can be legitimate during traffic.
