# Production deploy controller

For copy-paste production commands, preflight, worker handling, rollback, backups, and the final
verification gate, start with [`../DEPLOYMENT.md`](../docs/ops/DEPLOYMENT.md). This file documents controller
internals and first-environment procedures.

## Merge client (agent-merge.sh)

`agent-merge.sh` is the only path into `master` for contributors. Run it from your own
worktree with a clean tree and an upstream set: it reads the live `deploy/watchdog` state via
the GitHub API (reusing the credential from `git credential`), runs the path-aware local gate
for the lanes your diff touches (TypeScript/Rust/deployment classifiers from
`deploy/watchdog-lib.sh`, always-on static lane with `bash -n`, `git diff --check`, and
`deploy/docs-check.sh`), takes the machine merge-lock, rebases onto the latest
`origin/master`, re-runs the gate on the exact SHA it pushes, and holds the lock until the
host reports a green `deploy/watchdog` on that SHA. A red SHA is never retried: fix forward
with a new commit on a new branch. The full contributor workflow is in
[`../CONTRIBUTING.md`](../CONTRIBUTING.md).

### Recovering an interrupted merge (agent-merge-recover.sh)

That rebase onto the latest `origin/master` is where a moving trunk shows up: it can stop on a
conflict and exit, leaving the worktree mid-rebase. `.claude/hooks/guard-git.sh` denies both ways
out — `git rebase --continue` and `--abort` are indistinguishable to it from a history rewrite —
so recovery is delegated to a reviewed script, the same arrangement the guard already relies on for
worktree cleanup. The delegation is deliberately narrow: it performs no merge, no push and no
branch switch, and only touches a sequencer operation in the worktree the caller stands in.

```bash
./deploy/agent-merge-recover.sh                  # report the state, change nothing (exit 2)
./deploy/agent-merge-recover.sh --continue       # finish a rebase whose conflicts are staged
./deploy/agent-merge-recover.sh --abort          # restore the branch as it was before the rebase
```

Neither action can lose committed work, `--continue` refuses while any path is still conflicted,
and the shared primary clone is refused without `--allow-primary-tree`. The behaviour is pinned by
`deploy/agent-merge-recover.test.sh` against real repositories.

## Contributor worktree lifecycle and Rust cache

`agent-worktree.sh` is the managed boundary around task worktrees:

- `create <type/task> [name]` fetches `origin`, creates `${AGENT_WORKTREE_ROOT:-$HOME/wt}/<name>`
  from the exact current `origin/master`, rejects protected/existing branches and paths, and records
  creation/owner metadata in Git's per-worktree administrative directory;
- `finish [path]` requires one explicit non-primary, non-protected, unlocked, clean worktree whose
  branch is an ancestor of fresh `origin/master`; it optionally fast-forwards a clean primary
  `master`, removes only that worktree, and atomically deletes the branch only if its ref did not
  change after validation;
- `doctor` refreshes `origin` and classifies primary/current, missing, locked, detached, protected,
  dirty, unmerged, recent-merged, and cleanup-eligible worktrees without deleting or rewriting any
  local worktree or branch;
- `gc` is dry-run by default. `gc --apply` serializes with create/finish, prunes unlocked missing
  registrations without deleting unique branches, removes only clean merged worktrees older than
  the grace period (24 hours by default), and deletes only unchanged, unowned local branch refs
  already merged into `origin/master`. `master`, `main`, `comp/*`, dirty, unmerged, detached,
  current, primary, and locked worktrees remain untouched.

`DELETE_WORKTREE.sh` installs the per-user macOS LaunchAgent
`sale.apitoken.DELETE_WORKTREE`. It polls fresh repository state every 15 seconds and reclaims a
clean merged worktree after two identical idle observations over 30 seconds. It requires `lsof` to
show no open path below the candidate and delegates the final mutation to `agent-worktree.sh
finish`, preserving all lifecycle locks and exact-ref checks. Standalone same-origin clones are
handled only through an explicit allow-list and stricter clean/stash/all-local-refs-in-master proof.
Installation, logs, recovery behavior, and clone registration are documented in
[`../docs/ops/DELETE_WORKTREE.md`](../docs/ops/DELETE_WORKTREE.md).

The manager deliberately does not delete the retired clone-wide Cargo build directory. Existing
cache data remains an explicit one-time operator cleanup; deploying this change never removes local
files. Going forward, `sccache-cargo.sh` shares only the checksum-pinned binary and bounded 10 GiB
content-addressed compiler cache under the git-common-dir. It always clears an inherited
`CARGO_BUILD_BUILD_DIR`, while Cargo intermediates and linked artifacts remain in the current
worktree's `target/` or the caller's explicit `CARGO_TARGET_DIR`. Worktree removal therefore
reclaims task build output without allowing fingerprints or linked artifacts to mix across paths.

These scripts finalize immutable SHA-addressed releases, move release links atomically, and activate
processes with exact-systemd-unit readiness gates. The automatic watchdog passes
`--tested-candidate`: `deploy.sh` validates and promotes the frozen build instead of compiling it a
second time. The commerce lane first reduces that candidate to a content-addressed production-only
bundle: one shared pnpm virtual store, compiled API/worker/database files, migrations, and the
Content Studio standalone trace. Content Studio pins both `outputFileTracingRoot` and Turbopack to
the repository workspace root, so Next emits the stable `apps/content-studio/server.js` trace path
regardless of whether the candidate lives under a host cache, managed worktree, or primary clone.
Production reflink-copies only that roughly 105 MiB tree rather than walking the full roughly 636 MiB
candidate, then writes only the release marker; the bundle
was already frozen before its digest entered the trusted marker. Manual/bootstrap use retains the
standalone checkout-and-build fallback. Commerce and
PostgreSQL-backed engine deploys are two-phase: `deploy.sh` selects the release without touching
their serving slots, then the matching blue-green controller owns admission, pre-drain, and
shutdown. They never restart PostgreSQL, never write into a finalized release, and never treat an
arbitrary HTTP 2xx on the expected port as proof that the selected unit started.

Engine and commerce releases also carry the closed
`.engine-commerce-compatibility-v1` capability contract assembled from
`deploy/engine-commerce-compatibility.contract`. Selection and blue-green admission compare the target
against every counterpart generation resolved from active systemd PIDs; `current` alone is never
trusted because it moves before the serving process does. A direct mixed-version transition that
lacks a required pricing HTTP generation fails before migration, link activation, or cutover.
Known markerless rollback anchors are ancestry-classified only from the bounded scalar migration
window; older or unavailable history fails closed. The exact matrix and bridge order are in
[`RELEASES.md`](RELEASES.md) and [`../docs/ops/DEPLOYMENT.md`](../docs/ops/DEPLOYMENT.md).

Run them on the production host as the `deploy` operator from `/opt/apitoken/repo`, with narrowly scoped `sudo` access for application-unit and unit-file operations.

The dedicated candidate-validator can consume up to two exact-SHA `candidate-validation` requests
while the serialized production watchdog is deploying a parent. Each request is rebased by the
merge client onto the latest committed `master`, tested only across that feature delta, and frozen
under the same SHA-keyed marker used by normal delivery. Workers have separate disposable
PostgreSQL and Cargo slots and run below production CPU/I/O priority. A per-SHA lock lets a later
unchanged `master` deployment wait for and reuse an in-flight candidate instead of rebuilding it.
For TypeScript, each affected runtime context (commerce, sales, OpenKeys, Vercel web, or admin)
produces a
complete artifact set, while unrelated contexts are omitted. Shared workspace libraries build once
before the selected context builds overlap; per-context marker digests let each release promoter
verify only the artifacts it consumes. Typecheck/tests still use the changed package closure
(workspace consumers plus their prerequisites). Shared inputs, selector changes, deletions, and
unknown scopes force every context. The five Next.js apps restore host-local `.next/cache` archives
before building and publish only complete, symlink-free archives afterward; cache damage is treated
as a miss, never a deployment failure. A second content-addressed cache stores each context's
complete runtime outputs under an exact tracked-input, toolchain, platform, and build-environment
key. Exact hits skip shared and application builds; every cached file and executable mode is
manifest-verified. Cache trees stay symlink-free; generated links are recreated only from verified
workspace-relative metadata whose installed target remains inside the candidate. Misses, corruption,
unsafe links, and save contention fall back to the normal build. Entries are isolated by context
and bounded to six recent keys.
TypeScript tests run in four joined isolation groups: commerce, sales, OpenKeys, and database-free
packages. Each selected database migration now runs at the head of its own group, so all three
migrations and the database-free tests overlap; that group's tests begin only after its own schema
is ready. Database groups stay serial internally, candidate tests suppress package `pretest`
dependency rebuilds because the complete artifacts were already verified, and every selected lane
is reaped before the candidate receives a verdict. Operational regression helpers run once per
static gate rather than being nested again inside the watchdog library suite.
No-change GitHub contexts are published concurrently and joined before rollout. After component
controllers have gated their own exact releases, the watchdog derives a final verification plan
from the surfaces that changed: engine work checks runtime/panel/monitoring/Codex, Caddy checks
routing/monitoring/Codex, other application lanes check monitoring, and controller-only or
documentation deliveries do not re-probe an unchanged serving runtime. Independent read-only
smokes overlap and are all joined before the overall green status; possible engine reconciliation
still completes first. A disabled Codex metric is recognized directly instead of spending the full
retry window waiting for a filtered positive series.
Operational self-updates are content-aware and composable. Controller definitions, Caddy,
systemd, and monitoring each have a narrow root transaction; a mixed range runs only the selected
transactions in canonical order. Caddy and monitoring continue in the same cycle, controller work
hands the held lock directly to the installed controller, and only a systemd concern waits for the
next five-second manager invocation. Deletions, privileged/stateful definitions, and unknown
deployment files still fail closed to the complete installer. After its first RED delivery, the fixed GPT
Image 2 public-smoke controller is a non-network corrective inspector: a change to its own path can only
validate the existing producer-SHA fence, accept an exact `preflight`/no-dispatch withdrawal, or surface
bounded journal state/dispatch flags before the overall verdict. It cannot load credentials or replay
generation/edit. A fresh paid-smoke controller gets a distinct producer-SHA evidence root and may dispatch
its one generation-plus-edit attempt only when its own controller path triggers the gate; any recorded attempt
permanently fences replay. Every transaction records its exact tested infrastructure SHA only after every
selected concern succeeds.

Gemini 3.7 Flash uses a narrower direct-root transaction. The bridge commit installs a sealed copy
of producer `264363f7838ddd2d156b14668a320047ad33b6ee`, the private canary unit, and digest-pinned
gate/transport/evidence files. The unit fixes hot tariff overrides off at argv level, keeping its
preflight and immutable event on the compiled official schedule. The bridge deliberately omits
`deploy/gemini-3-7-admission-trigger`; that delivery cannot contact Gemini and spends zero. The trigger is a later one-file commit: regular Git
mode `100644`, exact path `deploy/gemini-3-7-admission-trigger`, exact bytes
`gemini-3.7-flash-admission-v1\n`, and no companion delta. The fixed root infrastructure runner
checks the exact protected production head and the complete uninstalled range, then invokes the
installed gate directly before its durable `infrastructure.sha` handoff. No sudo rule or
unprivileged watchdog dispatch exists for this admission.

Do not merge that trigger until the bridge SHA itself is production `deploy/watchdog` GREEN. This
ordering is an authority boundary, not scheduling advice: the older installed runner has no Gemini
3.7 trigger verifier or gate and could otherwise advance across the trigger without firing it.

The SHA-keyed evidence root is the permanent firing fence. A fresh run may make one free token count
and one paid SSE generation; redirects, reconnects, profile rotation, resends, and retries are
disabled. Re-entry after the fence is an offline inspection before systemd, credentials, or
transport. A terminal failure durably quarantines the attempted delivery before its infrastructure
baseline can advance; only a strictly newer fix-forward delivery can close the retained withdrawal
without a second provider call. Exact request and cost limits are in
`docs/ops/GEMINI_CALIBRATION.md`.
Installed component runners under `controller/` resolve shared watchdog helpers from the fixed parent
directory; the repository and extracted-candidate layout resolves the same files beside the runner.
The deployment regression suite executes both layouts so a controller self-update cannot publish a
runner that fails before its component rollout begins.

Pinned Codex tooling has its own artifact flag inside the engine lane. The isolated candidate builds
and tests the audited upstream pin once. During engine release selection,
`deploy.sh --promote-codex` invokes a fixed least-privilege root helper while holding the shared deploy lock;
the helper verifies the candidate marker and digest, installs a content-addressed executable, and
atomically updates only the Codex path/version/digest lines in the existing secret-bearing engine
environment. Engine slots never consume an untested host-local rebuild.

Before a feature verdict becomes green, the host refetches `master` and requires its current tip to
remain an ancestor of the exact candidate. The locked merge still waits for the parent’s overall
green verdict. An incompatible target move produces a new rebased SHA and a new pair of gates.
Feature-validation failures have a separate trap and never write the production rejection marker.
The production queue remains strictly one SHA at a time.

## Host mapping

| Component | Immutable release | Active unit | Readiness probe |
|---|---|---|---|
| Commerce API | `/opt/apitoken/releases/<sha>` | `apitoken-api@3000.service` / `apitoken-api@3001.service` | `http://127.0.0.1:<port>/v1/ready` |
| Anthropic provider | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api-anthropic@8787.service` / `claude-api-anthropic@8788.service` | `http://127.0.0.1:<port>/ready`, stable 8790 |
| OpenAI-compatible provider | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api-openai@8793.service` / `claude-api-openai@8797.service` | `http://127.0.0.1:<port>/ready`, stable 8792 |
| Native Gemini provider | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api-gemini@8795.service` / `claude-api-gemini@8799.service` | `http://127.0.0.1:<port>/ready`, stable 8794 |
| Backend-only KIMI provider (default-off) | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api-kimi@8804.service` / `claude-api-kimi@8805.service` | `http://127.0.0.1:<port>/ready`, stable 8803 |
| Unified router | `/srv/claude-api/releases/<sha>/claude-router` | `claude-router@8800.service` / `claude-router@8801.service` | direct `/ready` + `/startup` + exact binary, stable 8802 repeats both data-path probes |
| Commerce worker | `/opt/apitoken/releases/<sha>` through `current` | `apitoken-worker.service` | process-active + exact cwd |
| Content Studio | `/opt/apitoken/releases/<sha>` through `current` | `apitoken-content-studio.service` | `http://127.0.0.1:3500/api/health` + exact cwd |
| Admin panel (admin.apitoken.sale) | `/opt/apitoken/admin-releases/<sha>` through `current` | `apitoken-admin.service` | `http://127.0.0.1:3700/api/health` |
| Devbot (Telegram notifications) | `/opt/apitoken/devbot-releases/<sha>` through `current` | `apitoken-devbot.service` | `http://127.0.0.1:3800/health` |
| PostgreSQL | `/var/lib/apitoken/postgres` | `apitoken-postgres.service` | forbidden to these scripts |

The engine owns a separate `claude_engine` database and non-superuser login role in this PostgreSQL
server. Commerce application units receive no engine DSN and continue to communicate only through
the Control API. Both commerce processes use the stable loopback origin `http://127.0.0.1:8790`;
Caddy health-routes that origin to the active engine slot.

After the Stage-2 database cutover, use `deploy.sh --engine-bluegreen`, then
`engine-bluegreen.sh`, then `router-bluegreen.sh`. The first controller owns the provider cohort: it rolls the Anthropic pair, the
two OpenAI HTTP slots over a persistent per-home Codex daemon cohort, and the isolated active/passive
Gemini slots from one selected SHA. An OpenAI candidate cannot drain its predecessor until the old and
candidate HTTP generations expose the exact same opaque home set. Parity is candidate readiness,
not a minimum-duration soak: the first complete process-fenced match admits the candidate. A single
equal working home is sufficient; a candidate subset is not. Gemini candidates share the sealed
read-only roster, but Caddy's `first` policy admits new work to only one generation at a time; old
SSE stays on the readiness-drained process until bounded shutdown. Gemini is enabled only for
releases carrying its provider marker, and slots require `.gemini-bluegreen-v1`; rollback to the
immediately preceding singleton release uses the retained legacy unit. Legacy
restart mode refuses to run while the
PostgreSQL credential is active.

Every plane stops its old slot on **work, not on a clock**. After the readiness flip to HTTP 503 the
controller polls that slot's own `/ready`, which reports `active_requests` — customer requests still
being served, counted until the last byte of the response body — and issues `systemctl stop` only
once it reads zero. Stopping on a timer instead cut answers mid-flight, and their reservations were
left in `delivering` for the reconciler to charge at the full preflight hold. `ENGINE_DRAIN_WAIT_SECONDS`
(default 900) is an emergency ceiling, not the mechanism: reaching it is logged as a warning and
means something is wedged. A slot whose count cannot be read — an older binary without the field, an
unparseable answer — is stopped immediately, so the gate can never wedge a deploy.

The systemd side of the same contract is pinned by `deploy/shutdown-ladder.test.sh`, which runs in
both the merge gate and the watchdog regression suites. It reads the engine's own budget from
`crates/server/src/config.rs` — the clamp **maxima**, not the defaults, because an env override may
use them — and requires every serving `systemd/claude-api*.service` to allow at least that much plus
headroom for the settlement barriers and the mandatory billing flush. This ordering was inverted
once: the legacy OpenAI unit carried systemd's 90-second default while the engine drained for up to
620 seconds, so `State 'stop-sigterm' timed out. Killing.` landed mid-drain and the abandoned
reservations were charged at the full hold.
Before any target slot is started, `engine-bluegreen.sh` invokes the fixed root-owned
`engine-migrate.sh` helper for the selected release. Engine startup only verifies the installed
schema and never runs DDL; pending migrations are applied explicitly, one version transaction at a
time, while the existing slot remains the serving fallback.
The router controller independently rolls fixed 8800/8801 slots: exact binary, direct readiness and
the exact unauthenticated provider contract on loopback-only `/startup` precede an atomic root-owned
Caddy backend promotion. Stable origin 8802 repeats `/ready` and `/startup` after promotion; only
then is the predecessor SIGTERM-drained. Public traffic and stable metrics origin 8802 share that
one backend.
Paid Gemini project/key provisioning is outside release artifacts and is documented in
[`docs/engine/GEMINI_PROVIDER.md`](../docs/engine/GEMINI_PROVIDER.md).
`api-bluegreen.sh` similarly owns commerce slots; `--with-worker` restarts the single worker and
private Content Studio from the same immutable commerce release. Any service name containing
`postgres` is rejected before work begins.

## One-time prerequisites

The lock paths are fixed. They must be regular, root-owned files and are opened read/write without truncation. Install a tmpfiles rule so they are recreated safely after reboot:

```bash
sudo tee /etc/tmpfiles.d/apitoken-deploy.conf >/dev/null <<'EOF'
f /run/lock/apitoken-deploy.lock 0660 root deploy -
f /run/lock/apitoken-db-migrate.lock 0660 root deploy -
f /run/lock/apitoken-watchdog.lock 0660 root deploy -
f /run/lock/apitoken-candidate-validator.lock 0660 root deploy -
f /run/lock/apitoken-source-fetch.lock 0660 root deploy -
EOF
sudo systemd-tmpfiles --create /etc/tmpfiles.d/apitoken-deploy.conf
```

Keep the legacy `apitoken-api.service` active until the bootstrap procedure below has prepared and validated both immutable releases and created both `releases/current` symlinks. Do not run `disable --now` on the legacy API first.

## First immutable release: atomic bootstrap

Bootstrap is an explicit full-stack operation. It refuses `--api-only` and `--engine-only`, requires both `current` links to be genuinely absent, and requires the legacy API unit to be active before handoff.

```bash
deploy/deploy.sh --bootstrap --dry-run <full-40-character-sha>
deploy/deploy.sh --bootstrap <full-40-character-sha>
```

The bootstrap order is deliberate:

1. acquire the fixed deploy lock;
2. preflight and record the original absence of both `current` and `previous` links, and require the legacy API to be active;
3. fetch and verify the exact commit;
4. build the commerce tree, including `packages/db/dist/migrate.js`, in staging;
5. build the engine in staging;
6. validate required artifacts, write `.release-sha`, recursively remove write bits, and atomically rename both staging directories to their final SHA paths;
7. run the migration with `node /opt/apitoken/releases/<sha>/packages/db/dist/migrate.js` under the fixed migration lock;
8. validate that the staged engine unit uses the production `deploy` account and snapshot every unit file that bootstrap will replace;
9. install activation traps, create both `current` symlinks atomically, and validate them;
10. only now install the symlink-based systemd unit files and run `daemon-reload`;
11. enable/start the engine, allow a bounded active-state transition, and require its exact unit and release to pass readiness;
12. stop the still-running legacy API, enable/start `apitoken-api@3000.service`, allow a bounded active-state transition, then require exact-release readiness;
13. disable the legacy unit only after the template is healthy.

If any link change, unit installation, restart/start, exact-unit check, HTTP probe, or signal fails after activation begins, the trap restores every changed `current`/`previous` link and every replaced systemd unit file before `daemon-reload`. During bootstrap it also stops/disables the template API, restores the engine's original enabled/running state, and enables/restarts `apitoken-api.service`. Each failed restoration or recovery action is reported separately.

Manual recovery if automatic recovery is incomplete:

```bash
sudo systemctl disable --now apitoken-api@3000.service
sudo systemctl stop claude-api.service
sudo cp /root/zdt-backups/<timestamp>/systemd/claude-api.service /etc/systemd/system/claude-api.service
sudo systemctl daemon-reload
sudo systemctl start claude-api.service
sudo systemctl enable apitoken-api.service
sudo systemctl restart apitoken-api.service
sudo systemctl is-active claude-api.service
sudo systemctl is-active apitoken-api.service
```

Inspect the warnings from the failed bootstrap before retrying. Automatic recovery restores unit files from a root-owned temporary snapshot; the manual copy above is needed only if recovery explicitly reports that unit restoration failed.

## Normal deploy

Always supply the full SHA of a commit that has already passed integrated build and test. After the
PostgreSQL cutover, deploy components independently through their slot controllers:

```bash
deploy/deploy.sh --api-only <sha>
deploy/api-bluegreen.sh
deploy/deploy.sh --engine-bluegreen <sha>
deploy/engine-bluegreen.sh
deploy/router-bluegreen.sh
```

## One-time Stage-2 engine database cutover

**Production already completed this cutover. Do not run these commands during a normal deploy.**
They remain for a new environment or disaster reconstruction. The authority design and post-cutover
rollback boundary are documented in
[`../docs/engine/STAGE2_POSTGRES_AUTHORITY.md`](../docs/engine/STAGE2_POSTGRES_AUTHORITY.md).

Only run this after the exact release passed the workspace suite and the real PostgreSQL fault matrix.
The first script creates the isolated role/database and stages, but does not activate, its root-only
credential. The second takes a final SQLite snapshot, drains the singleton, refuses anonymous holds,
imports and reconciles monetary aggregates, installs the PostgreSQL-aware legacy and slot units, and
requires `/ready`. Then install the validated dual-upstream Caddy config and move the legacy process
to a template slot without dropping traffic:

```bash
sudo deploy/engine-postgres-provision.sh
sudo deploy/engine-postgres-cutover.sh
sudo deploy/install-caddy.sh --check
sudo deploy/install-caddy.sh
sudo deploy/configure-engine-control-url.sh
deploy/engine-bluegreen.sh
deploy/router-bluegreen.sh
```

After configuring the stable origin, start a new commerce API slot and restart the single worker so
both processes load the updated environment. Verify `configure-engine-control-url.sh --check` before
retiring an engine slot. The environment updater makes root-only timestamped rollback copies and
does not print any secret-bearing line.

If import, start, or readiness fails, the trap restores the old unit, moves the credential back to
pending, and restarts SQLite. Once PostgreSQL readiness succeeds and traffic resumes, do not point the
engine back at SQLite: it is then only an audit snapshot. `deploy/apitoken-db-dump` makes independent
custom-format dumps for `commerce`, `claude_engine`, `sales`, `openkeys`, and `apitoken_crm` when
present. PostgreSQL mode uses fenced template slots;
SQLite mode continues to enforce the host-local `flock` singleton.

A commerce release contains a prebuilt database migrator. Deployment invokes it directly:

```bash
node "/opt/apitoken/releases/<sha>/packages/db/dist/migrate.js"
```

It never invokes `pnpm db:migrate` from a finalized release because that package script recompiles before running. `--skip-migrate` is an explicit operator override for a release known not to require a migration. Migrations are additive and are never reversed by application rollback.

The automatic watchdog path additionally invokes the fixed root-owned
`pricing-retirement-admission.sh` after its exact-SHA backup and before the prebuilt migrator. The
helper is an explicit no-op for ordinary migrations. Only the immutable commerce 0049 retired
pricing-schema contraction activates its exact-candidate final preflight; a failure or any verdict
other than the single bounded `AUTHORIZED:commerce` line stops before Node is invoked. Engine
migration 0049 uses the same helper after its migration lock and before `db migrate-engine`, with
the watchdog-created exact-SHA backup and tested engine binary bound to the admission. New engine
releases carry an immutable `.pricing-retirement-admission-v1` value, so only
`contraction-0049` activates the destructive gate while legacy and explicit `pre-contraction`
releases remain usable after their watchdog candidate is pruned. See
`docs/ops/PRICING_RETIREMENT.md` for the retained-object manifest and post-retention sequence.

The same closeout has a separate post-drop fence. Between the selected final production checks and
the `processed.sha` write, the watchdog detects whether the exact processed-to-candidate range newly
added commerce 0049 or engine 0049 and invokes the fixed root-owned
`pricing-retirement-postdrop.sh`. One delivery cannot select both. The helper revalidates the exact
candidate and recovery archives, inspects both PostgreSQL planes read-only, scans application
journals, waits for a collector run newer than the contraction proof, rejects targeted active
business alerts, and records current database sizes. Exact candidate-latest journal checks allow a
new append-only migration to repair an already committed contraction without weakening the proof.
Failure quarantines the SHA but deliberately does not roll back a binary onto the forward-only
contracted schema; a later forward fix remains unprocessed and therefore re-runs the same stage
proof.

A normal deploy:

1. validates roots, lock files, unit names, timeout, and poll interval;
2. preflights and captures both `current` and `previous` for every selected component, rejecting broken, non-symlink, out-of-root, or non-SHA targets before builds or migrations;
3. promotes the watchdog-tested candidate, builds only in staging for standalone/manual use, or
   strictly validates every required artifact in an existing SHA release; tested commerce
   promotion copies only its pre-frozen compact bundle with same-filesystem reflinks;
4. runs the locked, prebuilt commerce migration before moving the API release link;
5. installs `ERR`, `EXIT`, `INT`, and `TERM` recovery traps before the first link mutation;
6. when the target differs from `current`, records the old `current` as `previous` and atomically changes `current`;
7. reconciles authbot to the selected engine release through the fixed exact-runtime helper, preserving an exact process and restarting plus exact-verifying a changed or inactive one;
8. in legacy mode, restarts the selected Rust engine and exact-unit gates it; in PostgreSQL mode, `--engine-bluegreen` leaves serving slots untouched for `engine-bluegreen.sh`;
9. does **not** start, stop, restart, or readiness-probe a commerce API slot—the old API process keeps serving the immutable release from which it was started;
10. disables recovery traps after link activation and any selected legacy restart succeeds, then instructs the operator to run the matching blue-green controller. If activation aborts after engine selection, recovery first restores the links and reconciles authbot to the captured original engine release only after verifying engine `current` resolves there. A failed or mismatched `current` restoration leaves authbot untouched and is reported as incomplete recovery.

A same-SHA deploy does not rewrite `previous`. Legacy engine mode may restart its unit; PostgreSQL
engine and commerce selection leave slot lifecycle to their blue-green controllers.

## Exact-unit readiness

The HTTP probe is necessary but not sufficient. When either blue-green controller starts/verifies a
target (or legacy `deploy.sh` activates its one engine), the readiness loop also requires:

- `systemctl is-active <exact-unit>`;
- `releases/current` resolves to the requested SHA directory;
- `FragmentPath` names a loaded regular unit file;
- the API `WorkingDirectory`/`ExecStart`, or engine `ExecStart`, is bound through the expected `releases/current` path;
- the exact unit's `MainPID` is running from the requested release (`/proc/<pid>/cwd` for the API, `/proc/<pid>/exe` for the engine);
- the loopback HTTP readiness endpoint returns success.

This prevents a legacy process that still owns the port from producing a false-positive deployment result.

`READINESS_TIMEOUT_SECONDS` and `--timeout` are positive integer deadlines. `READINESS_INTERVAL_SECONDS` must be an integer from 1 through 10. Before every curl and sleep the loop recomputes remaining time; curl receives `--max-time min(5, remaining)`.

## Roll back

Roll back to each component's recorded `previous` release:

```bash
deploy/rollback.sh --api-only
deploy/rollback.sh --engine-bluegreen
```

Or select an existing immutable SHA explicitly:

```bash
deploy/rollback.sh --api-only <sha>
deploy/rollback.sh --engine-bluegreen <sha>
```

Commerce rollback is deliberately two-phase, like a normal deploy:

```bash
deploy/rollback.sh --api-only [<sha>]
deploy/api-bluegreen.sh
```

Engine rollback is the same pattern:

```bash
deploy/rollback.sh --engine-bluegreen [<sha>]
deploy/engine-bluegreen.sh
deploy/router-bluegreen.sh
```

The first command selects the immutable rollback release without touching either running API slot. The second starts and verifies the inactive slot from that release, lets Caddy admit it, pre-drains the old slot, and then stops the old process. Do not insert an API restart between these commands.

Rollback fully preflights every selected target before mutating anything: release directory, `.release-sha`, API and migration artifacts or engine binary, plus original `current` and `previous` states for all selected components. It activates links under the same `ERR`/`EXIT`/`INT`/`TERM` recovery trap. PostgreSQL engine and commerce slot lifecycles remain exclusively owned by their blue-green controllers, but authbot is an engine-release singleton: `--engine-bluegreen` reconciles it to the selected rollback release before the later provider-slot cutover.

Preflight also enforces permanent Git-ancestry floors for the retired pricing schema. An engine
target must descend from `e8cf49ae121b581042c582ddb3621ee29fae8103`; a commerce target must
descend from `0c236aa2334f539786f53429d815d6b7c791adbe`. This applies equally to explicit SHAs,
`previous`, and the watchdog's automatic post-admission recovery. Compatibility markers do not
override these floors: a bridge binary that can speak scalar HTTP may still read tables that the
retention-gated contraction will remove.

If the target equals `current`, rollback is a link-bookkeeping no-op and does not overwrite `previous`. If activation, authbot reconciliation, engine restart/readiness, or a signal fails, recovery attempts to restore every changed link, reconciles authbot to the original engine release only when engine `current` verifies there, and restarts selected legacy engine services best-effort. A failed link restoration or reconciliation is reported as incomplete recovery; authbot remains untouched when engine `current` cannot be verified at the original release. Rollback never changes database state.

## Overrides and dry-run safety

Release roots are canonicalized before use. Commerce roots must remain under `/opt/apitoken/`; engine roots must remain under `/srv/claude-api/`. The systemd unit directory and both lock paths are fixed. Service overrides are validated and may never name a PostgreSQL unit.

Supported operational overrides include:

- `READINESS_TIMEOUT_SECONDS` and `READINESS_INTERVAL_SECONDS`;
- `SOURCE_REPO`, `COMMERCE_RELEASE_ROOT`, and `ENGINE_RELEASE_ROOT`, subject to the fixed prefixes;
- `API_SERVICE`/`API_READY_URL` for deploy and blue-green operations, plus `ENGINE_SERVICE`/`ENGINE_READY_URL`, subject to unit validation;
- `API_ENV_FILE`, `SYSTEMCTL_BIN`, and `SUDO_BIN`.

`DEPLOY_LOCK_FILE`, `MIGRATION_LOCK_FILE`, and `SYSTEMD_UNIT_DIR` may be present in the environment only with their fixed production values; alternate paths are rejected.

`--dry-run` prints fetch/build/migration/link/unit/service/readiness operations without creating directories, loading the secret environment file, opening locks, writing markers, changing symlinks, installing units, restarting services, or touching the database.

See [RELEASES.md](./RELEASES.md) for layout and retention rules.

## Blue-green API deploy

A production API release is always two commands and two distinct phases:

```bash
deploy/deploy.sh --api-only --dry-run <full-40-character-sha>
deploy/deploy.sh --api-only <full-40-character-sha>   # build/finalize, locked migration, move current
deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh                               # start/verify/pre-drain/stop slots
```

Do not insert an API `systemctl restart` between those commands. Both template units name `/opt/apitoken/releases/current/apps/api`, but a process that is already running keeps its resolved working directory, loaded JavaScript, and open files in the immutable release from which it started. Moving `releases/current` therefore does not mutate the old process. Blue-green remains safe only while that old slot is **not restarted** onto the new symlink before the target has been admitted.

`api-bluegreen.sh` performs the cutover in this order:

1. detect the ready old slot and choose the inactive target;
2. unless the target already proves it serves `releases/current`, stop its unit unconditionally to clear any stale process, require it stopped, and start it fresh;
3. require the exact target unit, `MainPID` working directory, current SHA, and direct `/v1/ready` HTTP 200;
4. wait 6 seconds—Caddy's 2-second health interval plus 2-second health timeout plus margin—then re-verify the target and commit it as the availability anchor;
5. send `SIGUSR1` to the old unit, which flips only its readiness to HTTP 503 while leaving its listener and in-flight work alive;
6. wait another 6 seconds for Caddy to depool the old upstream, then require old readiness to be exactly HTTP 503 **and** require the new slot to remain ready on the current release;
7. only then stop the old unit; systemd/Nest complete the bounded application drain.

Use `--target-port 3000` or `--target-port 3001` to choose the final serving slot explicitly. Without it, a single healthy slot is cut over to the other port; if neither slot is healthy, the bootstrap target defaults to 3000. `--timeout SECONDS` controls each bounded readiness wait.

When adding `127.0.0.1:3001` to Caddy for the first time, keep `apitoken-api@3001.service` stopped during the validated Caddy reload. Its first active check marks the new upstream down. Caddy's dial-failure retry window covers the short gap before that health result is recorded; only a later `api-bluegreen.sh` run should start and admit the slot.

`--with-worker` restarts `apitoken-worker.service` from `/opt/apitoken/releases/current/apps/worker`
after the API cutover, with no worker overlap. Final verification requires the worker PID's cwd to
resolve to the exact selected immutable release:

```bash
deploy/api-bluegreen.sh --with-worker
```

Build and select the immutable worker release before using this option; it does not run `pnpm
install`, compile TypeScript, or copy artifacts. Exact commands are in the top-level deployment
runbook.

Rollback is availability-first and never casually restarts the old slot through the moved symlink. Until the new slot is verified ready, serving current, and has passed the Caddy inclusion window, the old process remains running. A pre-commit failure stops only the failed target and leaves/confirms the old slot ready. After pre-drain or old-slot stop has committed traffic to the new process, recovery retains the verified new slot. Only if the old slot has already died/drained **and** no verified new slot remains will recovery restart the old unit; it emits a critical warning that this restart launches `releases/current` (the possibly bad new release), then requires exact-release readiness.

Both API instances may overlap safely during the Caddy grace period: they are stateless and share the same PostgreSQL database. Database operations that require singleton coordination use PostgreSQL advisory locks, so the brief blue-green overlap does not require separate databases or application-level leader selection.
