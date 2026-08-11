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
    │   ├── .engine-commerce-compatibility-v1
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

## CRM

```text
/opt/apitoken/crm-releases/
├── crm-<sha>/                             tested production bundle (lane-prefixed names)
│   ├── .release-sha
│   └── apps/crm-api|apps/crm-web|...
├── <sha>/                                 legacy plain-SHA release (early lane versions)
├── current -> /opt/apitoken/crm-releases/<active-release>
└── previous -> /opt/apitoken/crm-releases/<prior-release>
```

The CRM lane (separate repository) writes its own immutable releases below this root; the
`systemd/apitoken-crm-api.service` and `systemd/apitoken-crm-web.service` units run from
`/opt/apitoken/crm-releases/current`. This repository's watchdog release retention manages the root
with the same newest-ten keep and live-process protection as the engine and commerce roots; its
selector recognizes both the `crm-<sha>` lane names and the legacy plain-SHA names, and the two CRM
units are observed as live releases exactly like the other managed units.

## Rust engine

```text
/srv/claude-api/releases/
├── <sha>/
│   ├── .release-sha
│   ├── .engine-commerce-compatibility-v1
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
disables all KIMI incarnations, and both units pin `CLAUDE_API_KIMI_ENABLED=1` argv-level so the
plane's on/off state lives only in reviewed unit changes.
`claude-api.service` exists only as the bridge through the first cutover and is disabled afterward.

## Engine/commerce compatibility contract

Every newly assembled commerce and engine release carries the same reviewed source contract from
`deploy/engine-commerce-compatibility.contract` as `.engine-commerce-compatibility-v1`. It is data, not
shell input, and has a closed three-line grammar:

```text
format=1
commerce_requires=scalar-pricing-v1
engine_provides=scalar-pricing-v1
```

Capability lists are non-empty comma-separated lowercase identifiers. Duplicate keys,
capabilities, unknown lines, unsafe symlinks and unsupported formats fail release validation. The
commerce value declares every engine HTTP pricing generation that its API and worker require; the
engine value declares every generation served by its Control API. Compatibility is set inclusion:
all commerce requirements must occur in the engine provision list.

The deploy, rollback and both blue-green controllers check every transitional pair before a link
mutation, migration or process cutover. They resolve active commerce API/worker releases from
`/proc/<MainPID>/cwd` and active Control API releases from `/proc/<MainPID>/exe`, validate each as a
direct immutable release with an exact `.release-sha`, and deduplicate generations. A moved
`current` symlink is deliberately not evidence that a target process is serving.

Markerless rollback anchors are classified from verified Git ancestry in `/opt/apitoken/repo`:

| First included commit | Commerce requirements | Engine provisions |
|---|---|---|
| `2563b04328ce5911f3e7893df298da15535f5e95` | `policy-pricing-v1` | `policy-pricing-v1,scalar-pricing-v1` |
| `261900596666763bee0f5795d4a77ebfe144ddf8` | `policy-pricing-v1,scalar-pricing-v1` | unchanged bridge |
| `e725b51ec6a166a40b8b232f3cc3a0617ba6d9b6` | `scalar-pricing-v1` | unchanged bridge |
| `a6612450dfa521ee236d2ab0ac03a64e15c86557` | unchanged scalar | `scalar-pricing-v1` |

A markerless release outside the ancestry rooted at `2563b043…` is unclassified and rejected. This
bounded exception preserves the known bridge needed for rollback without turning all historical
releases into implicit compatibility claims.

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

In one fail-local transaction under the exclusive deployment lock, the watchdog snapshots every live
release once, completely selects expired directories for the managed engine, commerce, and CRM roots,
and only then removes any selection. It keeps the newest ten immutable releases per root and always
protects:

- the targets of `current` and `previous`;
- recorded component and in-flight deployment SHAs;
- every release identified from a live unit's executable or working directory.

Each active normal unit requires complete, successful `/proc/<MainPID>/exe` and
`/proc/<MainPID>/cwd` observations followed by unchanged load state, active state, and PID. A missing,
inactive, or failed unit contributes no live release; an active unit observed only outside managed
roots is incomplete. Because authbot is non-dumpable, its live engine release is obtained only
through the fixed root-owned runtime helper. Any lock, observation, or selector failure skips deletion
in every managed root without quarantining an otherwise valid candidate. Releases are made
owner-writable only immediately before removal and are never modified in place.

Temporary watchdog build/test candidates under `/var/lib/apitoken/watchdog/candidates` are removed
24 hours after successful test completion while the same lock is held. An incomplete workspace
without a marker ages from its directory mtime. A later explicit retry rebuilds an expired candidate
before using it.
