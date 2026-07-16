# Immutable release layout

Production releases are addressed by the full lowercase 40-character Git commit SHA. A release is assembled in a same-filesystem staging directory, validated, marked, made read-only, and atomically renamed to its final SHA path. Finalized and active release directories are never built in or otherwise modified by deploy or rollback.

## Commerce API

```text
/opt/apitoken/
├── repo/                                      fetch-only deployment checkout
└── releases/
    ├── <sha>/                                 detached checkout + installed dependencies + build output
    │   ├── .release-sha
    │   ├── apps/api/dist/main.js
    │   └── packages/db/dist/migrate.js        prebuilt migration entry point
    ├── current -> /opt/apitoken/releases/<active-sha>
    └── previous -> /opt/apitoken/releases/<prior-sha>
```

`apitoken-api@3000.service` uses `WorkingDirectory=/opt/apitoken/releases/current/apps/api` and starts the relative `dist/main.js` entry point. Readiness additionally verifies that the unit's `MainPID` has `/proc/<pid>/cwd` in the requested SHA release.

Commerce migration runs before API activation as:

```bash
node "/opt/apitoken/releases/<sha>/packages/db/dist/migrate.js"
```

The database package is built in staging. Deployment never calls the package's `db:migrate` script from a final release because that script runs a build first and would violate immutability.

## Rust engine

```text
/srv/claude-api/releases/
├── <sha>/
│   ├── .release-sha
│   └── claude-api                             executable release binary
├── current -> /srv/claude-api/releases/<active-sha>
└── previous -> /srv/claude-api/releases/<prior-sha>
```

After the one-time database cutover, `claude-api@8787.service` and `claude-api@8788.service` start
`/srv/claude-api/releases/current/claude-api serve` with fixed port/instance identities. Exact-unit
readiness verifies the target `MainPID` executable. A running old slot retains its already-resolved
immutable binary after `current` moves; `engine-bluegreen.sh` admits the target before draining it.
`claude-api.service` exists only as the bridge through the first SQLite-to-PostgreSQL cutover.

## Link validity

`current` and `previous` may be either:

- absent; or
- symlinks resolving to an existing direct child named by a full SHA under their component's canonical release root.

A broken symlink, a regular file/directory at either link path, a target outside the root, or a nested/non-SHA target is invalid and aborts preflight before mutation. Missing `current` is accepted only by the explicit first-release bootstrap path; normal deploy directs the operator to bootstrap.

Release roots are canonicalized. Commerce releases must stay below `/opt/apitoken/`; engine releases must stay below `/srv/claude-api/`.

## Finalization

For a new release the controller:

1. checks out the exact fetched commit into a temporary path;
2. installs/builds inside that temporary path;
3. validates the API entry point and prebuilt migration, or the engine binary;
4. writes `.release-sha` while still staging;
5. recursively removes write permission from the staged tree;
6. atomically renames staging to `<root>/<sha>`;
7. validates the final directory again before migration, activation, or reuse.

An existing SHA directory is never overwritten. It is reused only after its marker and required artifacts pass validation.

## Activation journal and recovery

Before changing any selected `current` or `previous`, the controller captures the original state of all such links for every selected component. Only after full preflight does it install `ERR`, `EXIT`, `INT`, and `TERM` traps and enter activation.

When the target differs from `current`:

1. the original `current` becomes `previous` when one exists;
2. a temporary symlink is atomically moved over `current`;
3. legacy engine mode may restart one unit; PostgreSQL mode leaves both engine slots untouched;
4. `engine-bluegreen.sh` or `api-bluegreen.sh` exact-unit gates the inactive target before pre-drain.

If the target already equals `current`, neither `current` nor `previous` is rewritten. This preserves the real prior rollback target during a same-SHA deploy or no-op rollback.

On any link-selection error or signal, recovery restores every changed link to its captured state.
Slot controllers add their own availability-first recovery: before admission they preserve the old
process; after admission they retain the verified new one. Neither PostgreSQL engine nor commerce
selection restarts a serving slot.

Database migrations are intentionally outside rollback. They must remain additive and backward-compatible with the prior application release.

## First-release bootstrap invariant

Neither symlink-based unit may be installed or first-started before both of these exist and validate:

```text
/opt/apitoken/releases/current
/srv/claude-api/releases/current
```

`deploy.sh --bootstrap` validates the staged service identity and snapshots the unit files it will replace before activation. It then creates both links, validates them, installs the new units, reloads systemd, starts/verifies the engine, and finally hands the API port from the active legacy unit to `apitoken-api@3000.service`. A failed handoff restores the original links, unit files, engine enabled/running state, and legacy API.

## Retention and cleanup

Keep at least:

- the target of `current`;
- the target of `previous`;
- any release needed for incident investigation or database-compatibility rollback.

Old SHA directories may be removed manually only after confirming that neither link targets them and no process is running from them. Never make an old release writable and never delete or modify a release while a process uses it.

The scripts perform no automatic garbage collection. Retention remains an explicit, non-destructive operator decision.
