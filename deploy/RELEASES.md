# Immutable release layout

Production releases are addressed by the full lowercase 40-character Git commit SHA. A release is assembled in a same-filesystem staging directory, validated, marked, made read-only, and atomically renamed to its final SHA path. Finalized and active release directories are never built in or otherwise modified by deploy or rollback.

## Commerce API

```text
/opt/apitoken/
├── repo/                                      fetch-only deployment checkout
└── releases/
    ├── <sha>/                                 compact tested production bundle
    │   ├── .release-sha
    │   ├── .release-bundle-format
    │   ├── node_modules/                      one shared production pnpm virtual store
    │   ├── apps/api/dist/main.js
    │   ├── apps/worker/dist/main.js
    │   ├── apps/content-studio/.next/
    │   │   ├── BUILD_ID
    │   │   └── standalone/apps/content-studio/server.js
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

The trusted TypeScript lane assembles and hashes the complete commerce bundle before freezing the
candidate. Promotion copies only `.deploy-artifacts/commerce-release`, using a same-filesystem
reflink when supported. Content Studio runs its traced standalone server; a fixed launcher falls
back to `next start` when an older full-tree release is selected for rollback.

## Rust engine

```text
/srv/claude-api/releases/
├── <sha>/
│   ├── .release-sha
│   └── claude-api                             executable release binary
├── current -> /srv/claude-api/releases/<active-sha>
└── previous -> /srv/claude-api/releases/<prior-sha>
```

After the one-time database cutover, `claude-api-anthropic@8787.service` and `claude-api-anthropic@8788.service` start
`/srv/claude-api/releases/current/claude-api serve` in fixed `anthropic` mode with unique port and
instance identities. `claude-api-openai@8793/8797` and `claude-api-gemini@8795/8799` start the same
immutable binary in fixed `openai` and `gemini` modes; each process has its own PostgreSQL owner
identity. Exact-unit readiness verifies both the target
`MainPID` executable and its process environment. A running old Anthropic slot retains its
already-resolved immutable binary after `current` moves; `engine-bluegreen.sh` admits every provider
target before readiness-draining its predecessor. Gemini slot activation additionally requires
`.gemini-bluegreen-v1`; the old singleton unit remains installed only for rollback to the immediately
preceding marker-less Gemini release. Rollback to a pre-Gemini release stops and disables all Gemini
incarnations. KIMI mirrors that shape: `claude-api-kimi@8804/8805` run the same binary in fixed
`kimi` mode behind stable loopback origin 8803, slot activation requires `.kimi-bluegreen-v1`, the
`claude-api-kimi` singleton on 8804 is the rollback anchor, rollback to a pre-KIMI release stops and
disables all KIMI incarnations, and both units pin `CLAUDE_API_KIMI_ENABLED=0` argv-level so the
plane ships default-off until a reviewed unit change.
`claude-api.service` exists only as the bridge through the first cutover and is disabled afterward.

The unified `claude-router` binary in the same engine release runs through
`claude-router@8800/8801.service`. `/etc/caddy/router-active.caddy` names exactly one slot for both
the public vhost and stable loopback origin 8802. `router-bluegreen.sh` admits an exact-release
candidate before the root helper atomically reloads Caddy, then SIGTERM-drains the predecessor.
The old `claude-router.service:8798` is retained only as the first-handoff/rollback anchor.

The native Codex provider has no sidecar artifact: its wire identity and credentials discipline
ship inside the tested engine binary, so a Codex-affecting change is gated by the same engine lane
as everything else. Profiles are sealed envelopes under `/srv/claude-api/data/codex`, shared
read-only between generations, so a failed handoff cannot split homes between two engine slots.

## Link validity

`current` and `previous` may be either:

- absent; or
- symlinks resolving to an existing direct child named by a full SHA under their component's canonical release root.

A broken symlink, a regular file/directory at either link path, a target outside the root, or a nested/non-SHA target is invalid and aborts preflight before mutation. Missing `current` is accepted only by the explicit first-release bootstrap path; normal deploy directs the operator to bootstrap.

Release roots are canonicalized. Commerce releases must stay below `/opt/apitoken/`; engine releases must stay below `/srv/claude-api/`.

## Finalization

For a new release the controller:

1. validates the root-owned watchdog candidate and its complete bundle digest when
   `--tested-candidate` is supplied, or checks out the exact fetched commit into a temporary path
   for standalone/manual use;
2. reflink-copies the pre-frozen compact bundle, or installs/builds inside the manual staging path;
3. validates the API entry point and prebuilt migration, or the engine binary;
4. writes `.release-sha` while still staging;
5. removes write permission from the new marker and bundle root (the tested bundle contents were
   already frozen), or recursively freezes a standalone/manual build;
6. atomically renames staging to `<root>/<sha>`;
7. validates the final directory again before migration, activation, or reuse.

An existing SHA directory is never overwritten. It is reused only after its marker and required artifacts pass validation.

## Activation journal and recovery

Before changing any selected `current` or `previous`, the controller captures the original state of all such links for every selected component. Only after full preflight does it install `ERR`, `EXIT`, `INT`, and `TERM` traps and enter activation.

When the target differs from `current`:

1. the original `current` becomes `previous` when one exists;
2. a temporary symlink is atomically moved over `current`;
3. legacy engine mode may restart one unit; PostgreSQL mode leaves both engine slots untouched;
4. `engine-bluegreen.sh`, `router-bluegreen.sh`, or `api-bluegreen.sh` exact-unit gates the inactive target before pre-drain.

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

The scripts perform no automatic garbage collection of immutable production releases. Retention
remains an explicit, non-destructive operator decision. This does not apply to temporary watchdog
build/test candidates under `/var/lib/apitoken/watchdog/candidates`: the watchdog removes those
workspaces and their test markers automatically 24 hours after successful test completion, while
holding its exclusive lock. An incomplete workspace without a marker ages from its directory mtime.
A later explicit retry rebuilds an expired candidate before using it.
