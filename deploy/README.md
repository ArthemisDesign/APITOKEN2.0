# Production deploy controller

For copy-paste production commands, preflight, worker handling, rollback, backups, and the final
verification gate, start with [`../DEPLOYMENT.md`](../DEPLOYMENT.md). This file documents controller
internals and first-environment procedures.

These scripts finalize immutable SHA-addressed releases, move release links atomically, and activate
processes with exact-systemd-unit readiness gates. The automatic watchdog passes
`--tested-candidate`: `deploy.sh` validates and promotes the frozen build instead of compiling it a
second time. Manual/bootstrap use retains the standalone checkout-and-build fallback. Commerce and
PostgreSQL-backed engine deploys are two-phase: `deploy.sh` selects the release without touching
their serving slots, then the matching blue-green controller owns admission, pre-drain, and
shutdown. They never restart PostgreSQL, never write into a finalized release, and never treat an
arbitrary HTTP 2xx on the expected port as proof that the selected unit started.

Run them on the production host as the `deploy` operator from `/opt/apitoken/repo`, with narrowly scoped `sudo` access for application-unit and unit-file operations.

The dedicated candidate-validator can consume up to two exact-SHA `candidate-validation` requests
while the serialized production watchdog is deploying a parent. Each request is rebased by the
merge client onto the latest committed `master`, tested only across that feature delta, and frozen
under the same SHA-keyed marker used by normal delivery. Workers have separate disposable
PostgreSQL and Cargo slots and run below production CPU/I/O priority. A per-SHA lock lets a later
unchanged `master` deployment wait for and reuse an in-flight candidate instead of rebuilding it.
For TypeScript, every deployable artifact is still built, while typecheck/tests run only for the
changed package closure (workspace consumers plus their prerequisites). Shared inputs and deletions
force the full workspace; filtered markers also bind reuse to the exact diff base. The four Next.js
apps restore host-local `.next/cache` archives before building and publish only complete,
symlink-free archives afterward; cache damage is treated as a miss, never a deployment failure.
Operational self-updates are content-aware: controller-only ranges copy the fixed root-owned
controller bundle, and Caddy-only ranges validate/reload only Caddy. Mixed changes, deletions,
systemd/monitoring/stateful definitions, and unknown deployment files fail closed to the complete
installer. Every mode records the exact tested infrastructure SHA before handing off to the next
five-second poll.

Before a feature verdict becomes green, the host refetches `master` and requires its current tip to
remain an ancestor of the exact candidate. The locked merge still waits for the parent’s overall
green verdict. An incompatible target move produces a new rebased SHA and a new pair of gates.
Feature-validation failures have a separate trap and never write the production rejection marker.
The production queue remains strictly one SHA at a time.

## Host mapping

| Component | Immutable release | Active unit | Readiness probe |
|---|---|---|---|
| Commerce API | `/opt/apitoken/releases/<sha>` | `apitoken-api@3000.service` / `apitoken-api@3001.service` | `http://127.0.0.1:<port>/v1/ready` |
| Rust engine | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api@8787.service` / `claude-api@8788.service` | `http://127.0.0.1:<port>/ready` |
| Commerce worker | `/opt/apitoken/releases/<sha>` through `current` | `apitoken-worker.service` | process-active + exact cwd |
| Content Studio | `/opt/apitoken/releases/<sha>` through `current` | `apitoken-content-studio.service` | `http://127.0.0.1:3500/api/health` + exact cwd |
| PostgreSQL | `/var/lib/apitoken/postgres` | `apitoken-postgres.service` | forbidden to these scripts |

The engine owns a separate `claude_engine` database and non-superuser login role in this PostgreSQL
server. Commerce application units receive no engine DSN and continue to communicate only through
the Control API. Both commerce processes use the stable loopback origin `http://127.0.0.1:8790`;
Caddy health-routes that origin to the active engine slot.

After the Stage-2 database cutover, use `deploy.sh --engine-bluegreen` followed by
`engine-bluegreen.sh`; legacy restart mode refuses to run while the PostgreSQL credential is active.
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
```

## One-time Stage-2 engine database cutover

**Production already completed this cutover. Do not run these commands during a normal deploy.**
They remain for a new environment or disaster reconstruction. The authority design and post-cutover
rollback boundary are documented in
[`../docs/STAGE2_POSTGRES_AUTHORITY.md`](../docs/STAGE2_POSTGRES_AUTHORITY.md).

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
```

After configuring the stable origin, start a new commerce API slot and restart the single worker so
both processes load the updated environment. Verify `configure-engine-control-url.sh --check` before
retiring an engine slot. The environment updater makes root-only timestamped rollback copies and
does not print any secret-bearing line.

If import, start, or readiness fails, the trap restores the old unit, moves the credential back to
pending, and restarts SQLite. Once PostgreSQL readiness succeeds and traffic resumes, do not point the
engine back at SQLite: it is then only an audit snapshot. `deploy/apitoken-db-dump` makes independent
custom-format dumps for `commerce` and `claude_engine`. PostgreSQL mode uses fenced template slots;
SQLite mode continues to enforce the host-local `flock` singleton.

A commerce release contains a prebuilt database migrator. Deployment invokes it directly:

```bash
node "/opt/apitoken/releases/<sha>/packages/db/dist/migrate.js"
```

It never invokes `pnpm db:migrate` from a finalized release because that package script recompiles before running. `--skip-migrate` is an explicit operator override for a release known not to require a migration. Migrations are additive and are never reversed by application rollback.

A normal deploy:

1. validates roots, lock files, unit names, timeout, and poll interval;
2. preflights and captures both `current` and `previous` for every selected component, rejecting broken, non-symlink, out-of-root, or non-SHA targets before builds or migrations;
3. promotes the watchdog-tested candidate, builds only in staging for standalone/manual use, or
   strictly validates every required artifact in an existing SHA release;
4. runs the locked, prebuilt commerce migration before moving the API release link;
5. installs `ERR`, `EXIT`, `INT`, and `TERM` recovery traps before the first link mutation;
6. when the target differs from `current`, records the old `current` as `previous` and atomically changes `current`;
7. in legacy mode, restarts the selected Rust engine and exact-unit gates it; in PostgreSQL mode, `--engine-bluegreen` leaves serving slots untouched for `engine-bluegreen.sh`;
8. does **not** start, stop, restart, or readiness-probe a commerce API slot—the old API process keeps serving the immutable release from which it was started;
9. disables recovery traps after link activation and any selected legacy restart succeeds, then instructs the operator to run the matching blue-green controller.

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
```

The first command selects the immutable rollback release without touching either running API slot. The second starts and verifies the inactive slot from that release, lets Caddy admit it, pre-drains the old slot, and then stops the old process. Do not insert an API restart between these commands.

Rollback fully preflights every selected target before mutating anything: release directory, `.release-sha`, API and migration artifacts or engine binary, plus original `current` and `previous` states for all selected components. It activates links under the same `ERR`/`EXIT`/`INT`/`TERM` recovery trap. PostgreSQL engine and commerce slot lifecycles remain exclusively owned by their blue-green controllers.

If the target equals `current`, rollback is a link-bookkeeping no-op and does not overwrite `previous`. If activation, engine restart/readiness, or a signal fails, every changed link is restored to its captured original state and selected engine services are restarted best-effort. Rollback never changes database state.

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
