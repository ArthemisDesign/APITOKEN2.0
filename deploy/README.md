# Production deploy controller

These scripts build immutable SHA-addressed releases and activate them with exact-systemd-unit readiness gates. They never restart PostgreSQL, never write into a finalized release, and never treat an arbitrary HTTP 2xx on the expected port as proof that the selected unit started.

Run them on the production host as the `deploy` operator from `/opt/apitoken/repo`, with narrowly scoped `sudo` access for application-unit and unit-file operations.

## Host mapping

| Component | Immutable release | Active unit | Readiness probe |
|---|---|---|---|
| Commerce API | `/opt/apitoken/releases/<sha>` | `apitoken-api@3000.service` | `http://127.0.0.1:3000/v1/ready` |
| Rust engine | `/srv/claude-api/releases/<sha>/claude-api` | `claude-api.service` | `http://127.0.0.1:8787/ready` |
| Commerce worker | repository-based existing deployment | `apitoken-worker.service` | managed separately |
| PostgreSQL | `/var/lib/apitoken/postgres` | `apitoken-postgres.service` | forbidden to these scripts |

The deploy controller activates only the commerce API and Rust engine. Any service name containing `postgres` is rejected before work begins, including through environment overrides.

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
8. install activation traps, create both `current` symlinks atomically, and validate them;
9. only now install the symlink-based systemd unit files and run `daemon-reload`;
10. enable/start the engine and require its exact unit and release to pass readiness;
11. stop the still-running legacy API, enable/start `apitoken-api@3000.service`, immediately require that exact unit to be active, then require exact-release readiness;
12. disable the legacy unit only after the template is healthy.

If any link change, unit installation, restart/start, exact-unit check, HTTP probe, or signal fails after activation begins, the trap restores every changed `current`/`previous` link. During bootstrap it also stops/disables the template API, stops the engine whose original `current` link was absent, and enables/restarts `apitoken-api.service`. Each failed restoration or recovery action is reported separately.

Manual recovery if automatic recovery is incomplete:

```bash
sudo systemctl disable --now apitoken-api@3000.service
sudo systemctl stop claude-api.service
sudo systemctl enable apitoken-api.service
sudo systemctl restart apitoken-api.service
sudo systemctl is-active apitoken-api.service
```

Inspect the warnings from the failed bootstrap before retrying. Unit files may remain installed after a failed handoff, but they must not be started again until both `current` links are valid.

## Normal deploy

Always supply the full SHA of a commit that has already passed integrated build and test:

```bash
deploy/deploy.sh --dry-run <sha>
deploy/deploy.sh <sha>
```

Deploy components independently after bootstrap when only one changed:

```bash
deploy/deploy.sh --api-only <sha>
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
4. runs the prebuilt commerce migration before API activation;
5. installs `ERR`, `EXIT`, `INT`, and `TERM` recovery traps before the first link mutation;
6. when the target differs from `current`, records the old `current` as `previous` and atomically changes `current`;
7. restarts only selected non-PostgreSQL units;
8. requires every exact selected unit to be active, loaded from a real fragment, configured through `releases/current`, running from the requested SHA, and passing its HTTP readiness endpoint;
9. disables recovery traps only after all selected services pass.

A same-SHA deploy does not rewrite `previous`; it restarts and verifies the selected exact unit while preserving the real rollback target.

## Exact-unit readiness

The HTTP probe is necessary but not sufficient. For each selected service the readiness loop also requires:

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

After `deploy.sh` has built and finalized the desired commerce release and `/opt/apitoken/releases/current` points to it, cut the API over separately between the two Caddy health-gated slots:

```bash
deploy/deploy.sh --api-only --dry-run <full-40-character-sha>
deploy/deploy.sh --api-only <full-40-character-sha>
deploy/api-bluegreen.sh --dry-run
deploy/api-bluegreen.sh
```

`api-bluegreen.sh` detects the slot that is both systemd-active and returning HTTP 200 from `/v1/ready`, starts the other slot against `releases/current`, requires that exact unit to be running from the current immutable release, and waits for readiness. It then gives Caddy five seconds to include the new healthy upstream before stopping the old instance. The application changes `/v1/ready` to 503 during shutdown, so Caddy depools the old slot while systemd's bounded stop and the application's drain handler allow in-flight requests to finish.

Use `--target-port 3000` or `--target-port 3001` to choose the final serving slot explicitly. Without it, a single healthy slot is cut over to the other port; if neither slot is healthy, the bootstrap target defaults to 3000. `--timeout SECONDS` controls each bounded readiness wait.

To move the commerce worker to the same current release without overlapping worker processes, add `--with-worker`:

```bash
deploy/api-bluegreen.sh --with-worker
```

The worker is handled after the API cutover as a separate stop-then-start operation. This intentionally permits a short worker pause and prevents concurrent old/new email or pricing jobs.

Any error or signal before final verification triggers availability-first rollback. The script first ensures the old API slot is running and ready, then stops the failed target; it never intentionally removes the last ready slot. During a first-slot bootstrap, where no old process exists, recovery tries the other slot before stopping anything. If worker replacement began, rollback also ensures the worker is active. Recovery failures are logged prominently for immediate operator action.

Both API instances may overlap safely during the Caddy grace period: they are stateless and share the same PostgreSQL database. Database operations that require singleton coordination use PostgreSQL advisory locks, so the brief blue-green overlap does not require separate databases or application-level leader selection.
