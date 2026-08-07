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
and stops the pipeline; neither later migrations nor application cutovers are attempted. Once a
candidate is selected, this holds for every unhandled abnormal termination — a failing command, an
interrupt, or a validation failure raised internally — so a stopped pipeline always leaves a
quarantine marker and a red commit status rather than stopping silently. Release-retention
housekeeping is the explicit fail-local exception: incomplete process observation, selection, or
lock acquisition skips both release roots and continues candidate processing without quarantining
the candidate. A failure before any commit is selected (an unreachable remote, a missing state file)
is an infrastructure fault rather than a verdict on a commit: it is logged and retried on the next
cycle without quarantining anything. Commerce migration failure always blocks the backend. Engine
migration or readiness failure leaves the serving engine slot untouched. Expensive retention and
production-alignment checks remain on a separate one-minute idle cadence, where the watchdog requires
exactly one slot for Anthropic, OpenAI, and supported Gemini to be active, ready, selected on the
recorded release, enabled, and running their fixed provider modes. If an out-of-band service command
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
continue in the same process. For a combined systemd/Caddy transaction, systemd provisioning runs
first: it atomically creates or validates the canonical raw `/etc/apitoken/proxy-admin.key` before
installing the unit or Caddy definitions. The `/etc/apitoken` parent is root-owned and not writable by
`deploy`, unlike deploy-writable `/srv/claude-api/data`; the installer rejects either
`AUTH_BOT_PROXY_ADMIN_KEY` or `AUTH_BOT_PROXY_ADMIN_KEY_FILE` in `server.env`. Systemd uses
`LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key` to create a private per-service copy.
After all `EnvironmentFile` directives load, the `ExecStart=/usr/bin/env` assignments pin
`AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key` for `claude-authbot.service`. This is deliberately
not an `Environment=` assignment, so env files cannot override the path; sibling units receive
neither the value nor that credential path. On Linux, after any operator subcommand has returned and
before daemon secrets are loaded, authbot calls `prctl(PR_SET_DUMPABLE, 0)`; this blocks same-UID
`ptrace`, `process_vm_readv`, and sensitive `/proc` memory access. `ProtectProc=invisible` and
`ProcSubset=pid` remain as service-level process-isolation layers. Code already executing inside
authbot itself is in the same trust boundary, and no defense can protect secrets from code already
executing there. This also prevents the `deploy` user from dereferencing `/proc/<MainPID>/exe`, so the
narrowly sudoed root-owned `authbot-runtime-state.sh` provides two fixed operations. The helper and
its non-symlink parent must both be exactly root:root mode `0755`. Digest mode hashes only that procfs
entry for engine promotion and emits only `exact`, `different`, or `inactive`. Literal `release-sha`
mode resolves the same entry as root, accepts only the canonical immutable path
`/srv/claude-api/releases/<SHA>/authbot`, and exposes only that lowercase 40-character SHA for
retention. A missing unit is treated as inactive (empty output in release mode). Both modes recheck
load state, active state, and PID for churn; malformed input, unexpected paths, and inspection
failures abort without path or digest output. Controller installation publishes this
backward-compatible helper and verifies its sudo policy before atomically publishing the watchdog
entrypoint that depends on them. Authbot reconciliation is part of the
release-link transaction: an activation abort restores
links first and converges authbot to the captured original engine release only if engine `current`
strictly resolves there; a failed or mismatched `current` restoration leaves authbot untouched. Explicit
`rollback.sh --engine-bluegreen` converges it to the selected rollback release before provider slots
cut over. A reconciliation or restoration failure leaves recovery incomplete rather than reporting a
complete rollback. The root-run Caddy installer is the only other intended consumer of the canonical raw file.
Any systemd
scope keeps `deploy/watchdog` pending until the next five-second poll because only a fresh manager
invocation receives the updated service sandbox.
The root `compose.yaml` is a local-development definition and does not reinstall production.
The private `deploy/gpt-image-2-live-gate.sh` is a one-shot exception within the controller transaction:
only the delivery range changing that file invokes it, after selected production verification and before
the processed SHA/overall GREEN. The withdrawn attempt under engine SHA
`3f67d43c0ae541979fee66823d251e2e3eea33e0` remains fenced in its own recovery root and is never replayed.
The paid attempt under watchdog-GREEN engine SHA
`012fccc471142fc51a46563da3a87564d674b39f` is also terminal: the endpoint returned a parsed image,
but its response metadata did not echo the requested `opaque/low/1024x1024` controls. No output or
checkpoint was published, and the exact attempt is permanently fenced. That recovery remains a
non-network withdrawal record and never permits replay. Diagnostic implementation SHA
`8fcd7c3c6f5dc968bedb7260433f2eaff23f8931` is independently watchdog-GREEN. The active
watchdog-GREEN engine SHA `3c17b31b6dfdcb8867d8def57e7aedc4ebc87644` is its descendant, and the
image canary and Codex image transport files have no diff between those SHAs. Its completed one-shot
controller was pinned to that exact active binary, a fresh SHA-keyed private root, and the then-applicable
`8_560_000` nanoUSD fixed-size authorization. It performed the free preflight and at most one exact-home
generation. A complete checkpoint was accepted as GREEN; a parsed evidence mismatch was accepted only
as a terminal withdrawal with a closed sanitized journal schema and no persisted or published image. Every other outcome fails
the delivery, and no attempt is replayed. Initial controller delivery
`3ba2d941e95419748027bf5fc8a0759821095148` stopped during infrastructure installation before the gate
could run because the sudo-policy installer self-check still named the predecessor SHA. Corrective
delivery `e0618cca78b6b5a650f9a8399c5457572bb44568` installed and verified the exact policy, then stopped
before the gate because the watchdog passed the newer unrelated engine baseline instead of the gate's
pinned implementation SHA; sudo correctly rejected that different argument. Neither delivery performed
the free preflight or a paid image dispatch. Delivery
`237a926b054a5fdd6833fca6668040ab6e0d55a7` performed the separately authorized exact-home request. The
native Codex endpoint returned a parsed opaque/low PNG result with terminal usage but normalized the
requested `1024x1024` size to `1254x1254`; no PNG or checkpoint was persisted or published, and the
sanitized `evidence_controls_mismatch` journal permanently fences that image turn. Its first verifier run
then failed because the optional-usage jq branch used an invalid binding expression. The corrective gate
only validates the existing terminal journal before any environment load or network dispatch; it cannot
replay the request. Auto-size implementation SHA `df58715abb4f1ac52b6c46b1ea6f830c6e11178f` and controller delivery
`afcfca46e22d3b123540462c9b20a2249dc9a56b` are watchdog-GREEN; their immutable private evidence contains
a bounded `opaque/low/auto` PNG, terminal usage, and exact SHA/turn attribution. Edit-capable SHA
`1c48e3769f0fe775e650f60ea3c5839458e5dfe2` and one-shot delivery
`8357ec764d1cdddff652ae4b5d6221267eb14f4e` are watchdog-GREEN; corrective verifier SHA
`354832bc86c3a8365e713faf0f35ad2c239c7087` is also GREEN. The controller consumed the successful
owned generation artifact exactly once under the `64_022_330_000` nanoUSD envelope and persisted a
bounded, byte-different PNG with positive terminal image-input/image-output usage and exact home/turn/SHA
identity. The corrective path is non-network: it accepts only that existing exact success checkpoint and
would make overall delivery RED after validating any terminal withdrawal. The historical gate remains
pinned to the explicit immutable edit SHA rather than a mutable engine baseline.

Public Images API attempt delivery `0dbbfdda054a1a7bda709434c8678b192bf12276` is RED at the fixed
producer gate for `d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6`. Corrective inspector delivery
`5a16ce96e2d1aef242055e88aa5d38f152d0ecd5` observed the exact private journal as `preflight` with both
`generation_dispatched=false` and `edit_dispatched=false`; both request identities are null. Therefore no
paid image operation was dispatched. The producer-SHA root under
`/var/lib/apitoken/watchdog/gpt-image-2-public/` remains a permanent no-replay fence.
`deploy/gpt-image-2-public-smoke-gate.sh` is an exact `--inspect`-only controller with no runtime environment
loader, credential access, timeout, `setpriv`, or CLI/network dispatch. It accepts either complete strict
success evidence or this exact pre-dispatch withdrawal, and otherwise emits bounded state/dispatch flags as
RED. A successor producer adds a separate no-image `--preflight-only` mode and exact pre-dispatch journal
states. Producer `d42fc0e3290c0042a16797626326c250e0f6721c` is deployed and watchdog-GREEN. Its separate
`deploy/gpt-image-2-public-preflight-gate.sh` is pinned to that immutable release and a fresh
`/var/lib/apitoken/watchdog/gpt-image-2-public-preflight/<producer-sha>` fence. The root controller inherits
only `CLAUDE_API_DATABASE_URL` from an active OpenAI slot; the exact binary selects the existing service
credential internally and checks authenticated discovery. Free-preflight delivery
`737d0234fc7d016c31c5b9c56a27e16aef134d83` is RED, so the producer-SHA root is fenced and cannot be
rerun. This mode has no image POST at all; its journal contract also requires both dispatch flags false and
null request identities. The corrective controller is now `--inspect`-only: no `/proc`, environment, binary,
credential or network path. It validates that the root contains only the private journal and publishes its
exact bounded stage as `deploy/gpt-image-2-public-preflight`. Inspector delivery
`77cf6791c92840dc1e45c1aba252820506f63fd4` reported the retained `credential_selecting` stage with false
dispatch flags and null request identities; it did not rerun the selector. Selector hardening
`6629ecd7b3725bcd7306ef7a1dc8675ef9160a43` joins the exact active assignment to the immutable service policy
owner instead of comparing the external service ID with opaque `account.id`, while preserving canonical
meter-only resolution, the OpenAI master switch and the single active unexpired-key requirement. Engine and
trusted-host checks passed, but its overall delivery was RED after the inspector because
`deploy/watchdog-github.sh` rejected the digit in status context `deploy/gpt-image-2-public-preflight`.
Corrective deploy SHA `b0c67351bb25437316afb61d18cd4462c57ef27b` is watchdog-GREEN and now permits lowercase
numbered `deploy/*` contexts; it performed no image request. A separate one-shot
`deploy/gpt-image-2-public-preflight-v2-gate.sh` is pinned to the deployed selector producer
`6629ecd7b3725bcd7306ef7a1dc8675ef9160a43` and the fresh
`/var/lib/apitoken/watchdog/gpt-image-2-public-preflight-v2/<producer-sha>` fence. It inherits only the
PostgreSQL DSN from an active OpenAI slot and executes only `openai-image-public-smoke --preflight-only`;
success requires the sole private journal to be `preflight_success` with both dispatch flags false and null
request identities. Until this new delivery is watchdog-GREEN, the corrected selector has not executed in
production. The v2 root later failed before dispatch and was retired. Handle-based selector producer
`63972f2ddfd5906d7c30a87406053eb3782f4223` ran through the fresh v3 preflight root; corrective delivery
`df924a10edff41b0d047805d18abe16a397b4809` validated its terminal no-dispatch evidence without replay and is
exact watchdog-GREEN. The subsequent paid controller uses a distinct
`/var/lib/apitoken/watchdog/gpt-image-2-public-paid-smoke/<producer-sha>` fence and exactly one `--execute`
invocation through the same sealed pool. Delivery `d2216bfa276d9fe195b0d1f0c8f4f137612bed5a` reached
`generation_received` with a bounded decoded PNG, then failed before complete settlement evidence; edit was
not dispatched. That root is permanently fenced and the execute trigger is retired. Its successor controller
is inspect-only: it reads exactly the private journal and generation PNG, validates the generation-only
withdrawal, and emits bounded dimensions/bytes/SHA-256 without environment, credential, CLI or network
access. Watchdog-GREEN engine producer `ab3b4e557f7b870b93f62a88a53e87e46b49fb4c` adds the separate
read-only `openai-image-settlement-diagnostic`. Its pinned root controller revalidates the original fence,
loads only the PostgreSQL DSN from the active OpenAI slot, sends the retained UUIDv4 to that exact current
binary over stdin, and publishes only a bounded identifier-free status. It performs no image request,
credential lookup, retry, or database mutation. Catalog, router, OpenKeys, site, admin and
public-documentation publication remains forbidden until a future fresh-root production generation+edit
delivery and overall watchdog status are GREEN.

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

Tested full and controller-only infrastructure transactions apply the policy automatically through
the isolated root `apitoken-sudoers-install.service`. They first publish the backward-compatible
fixed helper, then validate and live-verify the policy, and only after that succeeds atomically
publish the watchdog entrypoint that depends on it. These commands remain useful as manual preflight
or recovery procedures:

```bash
sudo deploy/install-sudoers.sh --check
sudo deploy/install-sudoers.sh
sudo apitoken-watchdog status
```

The installer validates the candidate with `visudo -c` (treating warnings as fatal, since an unused
alias means an intended privilege is silently not granted), saves timestamped rollback copies under
`/root/sudoers-backups`, removes the legacy unrestricted grant, then verifies every privilege the
pipeline needs and every privilege it must not have. If validation, verification, interruption, or
any other pre-commit step fails, it restores the previous policy and exits non-zero; the outer
transaction also restores the prior helper. It also removes `apitoken-ci` from the `deploy` group, so
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

## Pricing release cycle — removed

The managed Stage 5/6 preparation and Stage 8/9 operator procedures that lived here were deleted
together with the cycle's firing pins: the commerce routes (`/v1/admin/pricing-stage5-v2*`,
`/v1/admin/pricing-stage6-v2*`, `/v1/admin/pricing-stage8-capture-v2*`,
`/v1/admin/pricing-shadow-rollout-v2*`, `/v1/admin/pricing-release-activation-v2*`,
`/v1/admin/pricing-release-orchestration-v2*`), the funding-normalization / Stage 8 capture /
shadow-rollout / activation / orchestration worker lanes, the admin-panel pricing control room,
and the fixed root bridges (`pricing-stage56-admission-gate.sh`,
`pricing-stage56-refresh-gate.sh`, `pricing-stage567-converge{,-v2,-v3}-gate.sh`,
`pricing-stage7-admission-gate.sh`, `pricing-stage7-refresh-gate.sh`,
`pricing-stage7-identity-diagnostic-gate.sh`, including their sudoers stanzas).

Prices and discounts no longer ride a release cycle: prices are hot tariff overrides
(`POST /admin/pricing/tariffs/override`, see `docs/engine/CONTROL_API.md`) and discounts are
managed policy rules delivered through the durable pricing-control jobs. Admitting a NEW model
still advances the engine release pair; the remaining manual path is documented in
`docs/ops/MODEL_RELEASE_CYCLE.md`. The durable release/evidence rows from past cycles stay in
the database as immutable historical evidence; never edit or delete them.

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

Before a Caddy-only install can run, the full/systemd watchdog installer must have atomically
provisioned `/etc/apitoken/proxy-admin.key` as a `root:root 0600` regular file with no symlink,
containing exactly 64 lowercase hexadecimal bytes and optionally one trailing LF. Its
`/etc/apitoken` parent is root-owned and not writable by `deploy`; do not move the canonical key into
deploy-writable `/srv/claude-api/data`. During upgrade, the installer migrates one exact legacy
`AUTH_BOT_PROXY_ADMIN_KEY=<64-lowercase-hex>` assignment out of `authbot.env`; malformed or duplicate
legacy rows, or a legacy value divergent from an existing canonical file, fail before unit/Caddy
installation. It also rejects `AUTH_BOT_PROXY_ADMIN_KEY` and `AUTH_BOT_PROXY_ADMIN_KEY_FILE` settings
in `server.env`. The Caddy installer receives only the raw canonical file path, not the value in argv
or output. The renderer matches the live `X-Proxy-Admin-Key` header name case-insensitively; any
existing occurrence must have the exact canonical value or rendering fails without publishing a
partial candidate. The rendered proxy-admin upstream
also carries the shared `x-api-key` for only the previous authbot binary during mixed-version rollout
or rollback; the new binary ignores it, accepts only the dedicated header, and reads that key only
through its bounded dedicated-file parser. The installer then validates the candidate, saves a
timestamped rollback copy, and reloads rather than stop/start. Ports 8790, 8792, 8794, and 8803 must
be bound to `127.0.0.1`, never `*`.

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

An explicit existing SHA may follow the selector. Engine release selection also reconciles the
singleton authbot to that exact release before provider slots cut over; if selection aborts, link
recovery then reconciles it back to the captured original release, and any failure is reported as an
incomplete rollback. Rollback changes links and slots but never reverses a database migration. A
release whose migration breaks the prior binary is not rollout-safe.
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
dump per database. Nothing else removes them, so the watchdog runs retention at the start of each
delivery cycle and on its periodic idle cadence.

- Build candidates: removed after 24 hours (measured from test completion when a marker exists).
- Immutable releases: the newest ten per component root are kept. This covers the engine root
  `/srv/claude-api/releases`, the commerce root `/opt/apitoken/releases`, and the CRM root
  `/opt/apitoken/crm-releases` (whose lane names releases `crm-<sha>` and whose live units
  `apitoken-crm-api`/`apitoken-crm-web` are observed like every other active unit).
- Pre-deployment dumps: the newest ten per database are kept.

`current`, `previous`, the recorded component SHAs, and any release backing a live process are
always retained regardless of those counts. Release retention acquires the deployment lock itself,
snapshots live SHAs once, and completely materializes checked engine and commerce selections before
removing anything from either root. Each active normal unit requires successful, independent
`/proc/<pid>/exe` and `/proc/<pid>/cwd` resolution followed by unchanged load state, active state, and
PID; each path inside a managed root must name an exact release SHA, and both are retained when they
differ. Missing, inactive, and failed units are skipped. An active unit observed only outside the
managed roots, an unreadable procfs entry, an ambiguous path, or state/PID churn makes the observation
incomplete. Non-dumpable authbot is never inspected that way by `deploy`: its fixed root helper
returns only a strictly validated engine release SHA. Any lock, observation, helper, or selector
failure skips release deletion in both roots and continues candidate processing without quarantine.
The hourly `<database>.dump` rotation artifacts are never pruned — they remain the authoritative
recovery objects.

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
