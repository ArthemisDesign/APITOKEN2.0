# Production deploy controller

These scripts build immutable SHA-addressed releases, move release links atomically, and activate processes with exact-systemd-unit readiness gates. A normal commerce deploy is deliberately two-phase: `deploy.sh` finalizes/migrates/selects the release without touching API processes, then `api-bluegreen.sh` owns the slot lifecycle and zero-downtime cutover. They never restart PostgreSQL, never write into a finalized release, and never treat an arbitrary HTTP 2xx on the expected port as proof that the selected unit started.

Run them on the production host as the `deploy` operator from `/opt/apitoken/repo`, with narrowly scoped `sudo` access for application-unit and unit-file operations.

## Host mapping

| Component | Immutable release | Active unit | Readiness probe |
|---|---|---|---|
| Commerce API | `/opt/apitoken/releases/<sha>` | `apitoken-api@3000.service` / `apitoken-api@3001.service` | `http://127.0.0.1:<port>/v1/ready` |
| Rust engine | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api.service` | `http://127.0.0.1:8787/ready` |
| Commerce worker | `/opt/apitoken/repo` (mutable Git checkout; not `releases/current`) | `apitoken-worker.service` | managed separately |
| PostgreSQL | `/var/lib/apitoken/postgres` | `apitoken-postgres.service` | forbidden to these scripts |

`deploy.sh` activates the Rust engine but only prepares and selects a commerce API release. `api-bluegreen.sh` alone starts, signals, and stops normal commerce API slots; `--with-worker` may separately restart the repository-based worker. Any service name containing `postgres` is rejected before work begins, including through environment overrides.

## One-time prerequisites

The lock paths are fixed. They must be regular, root-owned files and are opened read/write without truncation. Install a tmpfiles rule so they are recreated safely after reboot:

```bash
sudo tee /etc/tmpfiles.d/apitoken-deploy.conf >/dev/null <<'EOF'
f /run/lock/apitoken-deploy.lock 0660 root deploy -
f /run/lock/apitoken-db-migrate.lock 0660 root deploy -
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

Always supply the full SHA of a commit that has already passed integrated build and test:

```bash
deploy/deploy.sh --dry-run <sha>
deploy/deploy.sh <sha>
deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

The last two commands are required whenever the selected release includes the commerce API; `deploy.sh` intentionally leaves the old API process untouched. Deploy components independently after bootstrap when only one changed:

```bash
deploy/deploy.sh --api-only <sha>
deploy/api-bluegreen.sh
deploy/deploy.sh --engine-only <sha>
```

A commerce release contains a prebuilt database migrator. Deployment invokes it directly:

```bash
node "/opt/apitoken/releases/<sha>/packages/db/dist/migrate.js"
```

It never invokes `pnpm db:migrate` from a finalized release because that package script recompiles before running. `--skip-migrate` is an explicit operator override for a release known not to require a migration. Migrations are additive and are never reversed by application rollback.

A normal deploy:

1. validates roots, lock files, unit names, timeout, and poll interval;
2. preflights and captures both `current` and `previous` for every selected component, rejecting broken, non-symlink, out-of-root, or non-SHA targets before builds or migrations;
3. builds only in staging, or strictly validates every required artifact in an existing SHA release;
4. runs the locked, prebuilt commerce migration before moving the API release link;
5. installs `ERR`, `EXIT`, `INT`, and `TERM` recovery traps before the first link mutation;
6. when the target differs from `current`, records the old `current` as `previous` and atomically changes `current`;
7. restarts only the selected Rust engine and requires its exact unit to be loaded from a real fragment, configured through `releases/current`, running from the requested SHA, and passing readiness;
8. does **not** start, stop, restart, or readiness-probe a commerce API slot—the old API process keeps serving the immutable release from which it was started;
9. disables recovery traps after link activation and any selected engine restart succeed, then instructs the operator to run `api-bluegreen.sh`.

A same-SHA deploy does not rewrite `previous`. It restarts and verifies the engine when selected, but still leaves commerce API slot lifecycle to `api-bluegreen.sh`.

## Exact-unit readiness

The HTTP probe is necessary but not sufficient. When `deploy.sh` activates the engine, and when `api-bluegreen.sh` starts/verifies an API target slot, the readiness loop also requires:

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
deploy/rollback.sh
deploy/rollback.sh --api-only
deploy/rollback.sh --engine-only
```

Or select an existing immutable SHA explicitly:

```bash
deploy/rollback.sh --api-only <sha>
deploy/rollback.sh --engine-only <sha>
```

Rollback fully preflights every selected target before mutating anything: release directory, `.release-sha`, API and migration artifacts or engine binary, plus original `current` and `previous` states for all selected components. It then activates under the same `ERR`/`EXIT`/`INT`/`TERM` recovery trap and the same exact-unit readiness gate.

If the target equals `current`, rollback is a link-bookkeeping no-op and does not overwrite `previous`. If activation, restart, readiness, or a signal fails, every changed link is restored to its captured original state and selected services are restarted best-effort. Rollback never changes database state.

## Overrides and dry-run safety

Release roots are canonicalized before use. Commerce roots must remain under `/opt/apitoken/`; engine roots must remain under `/srv/claude-api/`. The systemd unit directory and both lock paths are fixed. Service overrides are validated and may never name a PostgreSQL unit.

Supported operational overrides include:

- `READINESS_TIMEOUT_SECONDS` and `READINESS_INTERVAL_SECONDS`;
- `SOURCE_REPO`, `COMMERCE_RELEASE_ROOT`, and `ENGINE_RELEASE_ROOT`, subject to the fixed prefixes;
- `API_SERVICE`, `ENGINE_SERVICE`, `API_READY_URL`, and `ENGINE_READY_URL`, subject to unit validation;
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

`--with-worker` does **not** ship or bind the worker to `/opt/apitoken/releases/current`. The existing `apitoken-worker.service` runs from the mutable Git checkout under `/opt/apitoken/repo/apps/worker`; the option merely stops and starts that repository-based unit after the API cutover, with no worker overlap:

```bash
deploy/api-bluegreen.sh --with-worker
```

Rollback is availability-first and never casually restarts the old slot through the moved symlink. Until the new slot is verified ready, serving current, and has passed the Caddy inclusion window, the old process remains running. A pre-commit failure stops only the failed target and leaves/confirms the old slot ready. After pre-drain or old-slot stop has committed traffic to the new process, recovery retains the verified new slot. Only if the old slot has already died/drained **and** no verified new slot remains will recovery restart the old unit; it emits a critical warning that this restart launches `releases/current` (the possibly bad new release), then requires exact-release readiness.

Both API instances may overlap safely during the Caddy grace period: they are stateless and share the same PostgreSQL database. Database operations that require singleton coordination use PostgreSQL advisory locks, so the brief blue-green overlap does not require separate databases or application-level leader selection.
