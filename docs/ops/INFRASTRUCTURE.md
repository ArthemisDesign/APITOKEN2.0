# Production infrastructure

This document records the non-secret production topology for `apitoken.sale`. Credentials, OAuth
secrets, payment-provider keys, subscription tokens and database passwords must never be added to
the repository. Commerce secrets live in root-readable files below `/etc/apitoken`; engine secrets
live below `/srv/claude-api/data` on the host.

## Current topology

```text
api.apitoken.sale --------------------------> engine balancer 127.0.0.1:8790
                                                |-> Anthropic slot 127.0.0.1:8787
                                                `-> Anthropic slot 127.0.0.1:8788

openai.api.apitoken.sale -------------------> OpenAI origin 127.0.0.1:8792
                                                |-> OpenAI slot 127.0.0.1:8793
                                                `-> OpenAI slot 127.0.0.1:8797

gemini.api.apitoken.sale -------------------> Gemini origin 127.0.0.1:8794
                                                |-> Gemini slot 127.0.0.1:8795
                                                `-> Gemini slot 127.0.0.1:8799

future browser at apitoken.sale
        |
        `-> backend.apitoken.sale ----------> commerce balancer 127.0.0.1:8791
                                                |-> NestJS API slots 127.0.0.1:3000/3001
                                                |-> payment/email/pricing worker ------.
                                                |                                      |
                                                |   stable Control API 127.0.0.1:8790 <-'
                                                |          |-> engine slot :8787
                                                |          `-> engine slot :8788
                                                `-> PostgreSQL 18 on 127.0.0.1:5433
                                                    |-> commerce DB/role
                                                    `-> claude_engine DB/isolated role

content-studio.apitoken.sale ---------------> Caddy managed auth
                                                |-> Next.js workspace 127.0.0.1:3500
                                                `-> commerce balancer 127.0.0.1:8791

admin.apitoken.sale /proxy-admin/* ---------> Caddy managed auth
                                                `-> authbot proxy lifecycle 127.0.0.1:8806

commercial host -- encrypted Borg/SSH --> 84.32.109.82:2223/backup/.repo
```

`api.apitoken.sale` is the public Anthropic-compatible core endpoint. Its public proxy exposes only
`/v1/*`, `/health`, and `/balance`, not the engine Control API. `backend.apitoken.sale` is the browser-facing
commercial API. The Rust engine remains authoritative for API keys, balances, reservations and
usage. The commercial PostgreSQL database owns users, authentication, payment state, B2C/B2B
pricing state and durable jobs. The commercial services access the engine only through Caddy's
explicitly loopback-bound stable Control API at `127.0.0.1:8790`, never through a deployment slot.
The legacy core host is not an upstream or fallback.

The first provider-split release temporarily lets 8792 recognize the still-serving combined slot;
`deploy/CADDY.md` defines that bounded bridge and its mandatory routing/monitoring cleanup. It is not the
steady-state topology shown above.

The Rust engine ran on an interim host (`5.9.59.83`, shared with an unrelated project) until it was
migrated onto this commercial host on 2026-07-14. Anthropic now runs here as `claude-api-anthropic@8787` or
`claude-api-anthropic@8788`, while OpenAI-compatible Codex runs as `claude-api-openai.service` (plus
`claude-authbot.service` and the `claude-api-backup.timer`) under the
`deploy` user, with its
authority in the isolated PostgreSQL `claude_engine` database; the drained SQLite snapshot remains
at `/srv/claude-api/data/subscriptions.db`. Its environment secrets live at
`/srv/claude-api/data/{server.env,config.env,authbot.env,engine-postgres.env}` (separate from the commercial
`/etc/apitoken/*.env`). The dedicated proxy-admin secret is not an environment assignment: its
canonical source is `/etc/apitoken/proxy-admin.key`, a stable `root:root 0600` regular file with no
symlink and exactly 64 lowercase hexadecimal bytes plus an optional LF. Its `/etc/apitoken` parent is
root-owned and not writable by `deploy`; the credential must not be placed below deploy-writable
`/srv/claude-api/data`. The installer provisions it atomically before the authbot unit and Caddy,
migrating one exact legacy assignment out of `authbot.env` and failing on malformed, duplicate, or
divergent state. It rejects either `AUTH_BOT_PROXY_ADMIN_KEY` or
`AUTH_BOT_PROXY_ADMIN_KEY_FILE` in `server.env`. Systemd uses
`LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key` to give only
`claude-authbot.service` a private per-service copy. After its environment files load, the unit's
`ExecStart=/usr/bin/env` pins `AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key`; this is deliberately
not `Environment=`, so environment files cannot redirect it. Sibling units receive neither the value
nor credential path. On Linux, after any operator subcommand has returned and before daemon secrets
are loaded, authbot calls `prctl(PR_SET_DUMPABLE, 0)`; this blocks same-UID `ptrace`,
`process_vm_readv`, and sensitive `/proc` memory access. `ProtectProc=invisible` and `ProcSubset=pid`
remain as service-level process-isolation layers. Code already executing inside authbot itself is in
the same trust boundary, and no defense can protect secrets from code already executing there.
Because non-dumpability also blocks same-UID `/proc/<MainPID>/exe` dereference, the root-owned
`/usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh` is the only deployment bridge
for runtime inspection. The helper and its non-symlink parent must both be exactly root:root mode
`0755`. Its sudo rule accepts either one exact SHA-256 argument, returning only `exact`, `different`,
or `inactive`, or literal `release-sha`, returning only a canonical immutable engine release SHA (and
nothing when the unit is missing or inactive). Both operations inspect only the live procfs
executable and recheck load state, active state, and PID; malformed paths, unexpected load-state
churn, and failures reveal no path or digest. Controller installation publishes the backward-compatible
helper, verifies the sudo policy, and only then atomically publishes the watchdog entrypoint that
depends on them. Engine rollout skips an exact binary and, after a changed restart, fails unless the
exact tested digest remains active. The root-run Caddy installer is the only other intended raw-file
consumer. The interim host keeps a cold pre-migration copy for forensics, not as a
live rollback authority, and no longer runs any product unit. The `claude-api-fingerprint.timer` is intentionally not
enabled here yet (it needs a live `claude` CLI on the host); the fingerprint values in `config.env`
are current.

## Commercial host

| Item | Value |
|---|---|
| Public IPv4 | `84.32.48.2` |
| Location | Chicago, United States |
| CPU | AMD Ryzen 7 7700X, 8 cores / 16 threads |
| RAM | 96 GB DDR5 |
| Storage | 2 x 1 TB NVMe, Linux software RAID 1, about 894 GB usable |
| Network | 10 Gbit/s port, 100 TB egress allowance |
| OS | Ubuntu 24.04 LTS |
| Hostname | `apitokensale` |
| Deployment user | `deploy` (SSH public key only) |

The initial RAID synchronization must finish before planned power interruption. Check it with
`cat /proc/mdstat`.

Installed host tooling:

- Node.js 24 and pnpm 9.7.0;
- current stable Rust toolchain for the `deploy` user;
- Docker Engine with the Compose and Buildx plugins;
- PostgreSQL 16 client, build-essential, Clang, CMake, OpenSSL and SQLite development tools;
- UFW, Fail2ban and unattended security upgrades;
- BorgBackup and Borgmatic;
- Caddy 2 from its official stable package repository.

UFW denies inbound traffic by default and permits only SSH, HTTP and HTTPS. SSH password
authentication is disabled. Root can authenticate by key for recovery, while routine deployment
uses `deploy`. The application API and PostgreSQL bind to loopback. Caddy owns public ports 80/443,
redirects HTTP to HTTPS and terminates TLS for the configured product/mail hostnames.

The active DNS record is `A *.apitoken.sale -> 84.32.48.2`. The apex remains independent for the
future frontend. Exact DNS records override the wildcard if they are added later.

## Host paths

```text
/opt/apitoken/repo             fetch-only deployment checkout and reviewed controller source
/opt/apitoken/releases/<sha>   immutable commerce release directories
/opt/apitoken/releases/current active API/worker/Content Studio commerce release symlink
/srv/claude-api/releases/<sha> immutable Rust engine release directories
/srv/claude-api/releases/current active engine release symlink
/var/lib/apitoken/watchdog     24-hour tested-candidate workspaces, SHA baselines, quarantine and status state
/usr/local/lib/apitoken-watchdog root-owned automatic delivery controller
/var/lib/apitoken/postgres     PostgreSQL container data
/var/lib/apitoken/backups      application-consistent database export staging
/var/log/apitoken              application-owned logs when file output is needed
/etc/apitoken/postgres.env     root-readable PostgreSQL Compose environment
/etc/apitoken/api.env          root-readable API environment
/etc/apitoken/worker.env       root-readable worker environment
/etc/apitoken-backup           Borg identity, passphrase and exported recovery key
/etc/borgmatic/config.yaml     Borgmatic sources and retention policy
```

The repository itself is not a secret store. Environment files are provisioned directly on the
server with mode `0600`.

## Service management

Repository-managed deployment units:

```text
systemd/apitoken-postgres.service
systemd/apitoken-api.service          legacy untemplated API unit
systemd/apitoken-api@.service         release-symlink API unit; instance name is the port
systemd/apitoken-worker.service
systemd/apitoken-content-studio.service
systemd/claude-api.service          one-time SQLite-to-PostgreSQL bridge
systemd/claude-api@.service         disabled-after-cutover combined bridge slots
systemd/claude-api-anthropic@.service PostgreSQL-fenced Anthropic blue/green slots
systemd/claude-api-openai.service   legacy pre-bluegreen singleton unit (superseded)
systemd/claude-api-openai@.service  PostgreSQL-fenced OpenAI/Codex blue/green slots
systemd/claude-api-gemini@.service  PostgreSQL-fenced Gemini active/passive slots
systemd/claude-router@.service      unified router blue-green slots (loopback 8800/8801; stable Caddy 8802)
systemd/claude-router.service       legacy first-handoff/rollback anchor only (loopback 8798)
systemd/claude-api-backup.service
systemd/claude-api-backup.timer
systemd/apitoken-deploy-watchdog.service
systemd/apitoken-deploy-watchdog.timer
deploy/deploy.sh
deploy/api-bluegreen.sh
deploy/engine-bluegreen.sh
deploy/rollback.sh
deploy/install-caddy.sh
deploy/configure-engine-control-url.sh
deploy/apitoken-db-dump
deploy/commerce-postgres.compose.yaml
```

Normal operations:

```bash
sudo systemctl status apitoken-postgres apitoken-worker 'apitoken-api@*' \
  'claude-api-anthropic@*' 'claude-api-openai@*' 'claude-api-gemini@*'
sudo journalctl -u 'apitoken-api@*' -u apitoken-worker \
  -u 'claude-api-anthropic@*' -u 'claude-api-openai@*' --since today
sudo systemctl status caddy
sudo caddy validate --config /etc/caddy/Caddyfile
curl -fsS http://127.0.0.1:8790/ready
curl -fsS http://127.0.0.1:8792/ready
```

The PostgreSQL container publishes only to `127.0.0.1:5433`. API and worker use the same database
and authentication-token encryption key. Both use the server-side engine Control key, which must
never be returned to clients or placed in frontend configuration.

### Current deployment model (verified 2026-08-01)

- Stage 2 is complete: the engine authority is the role-isolated `claude_engine` PostgreSQL database;
  SQLite is a retained audit snapshot and must not be reactivated after production writes.
- Anthropic 8787/8788 and API 3000/3001 are health-gated blue-green slots. OpenAI 8793/8797
  alternates the same way behind stable origin 8792, and Gemini 8795/8799 runs active/passive
  behind stable origin 8794. Only one slot per pair remains enabled after a normal cutover.
- API and worker load `ENGINE_BASE_URL=http://127.0.0.1:8790`; Caddy binds that listener explicitly
  to loopback and routes only ready engine slots.
- Sales and every commerce-facing Caddy route use `COMMERCE_BASE_URL=http://127.0.0.1:8791`;
  public/admin routes never name API slots. Anthropic public/admin routes use 8790, OpenAI uses
  8792, and Gemini uses 8794; no public hostname names a runtime port. 8790, 8791, 8792, and 8794
  all perform health-gated slot selection over their respective slot pairs.
- Public engine, commerce API, Caddy, worker, and the hourly dual-database backup timer are active.
- The core public matcher exposes `/v1/*`, `/health`, and `/balance`; Control/admin routes remain
  private. Public liveness/readiness behavior is described in `deploy/CADDY.md`.
- A dedicated read-only GitHub deploy key at `/home/deploy/.ssh/github_deploy_ed25519` supports
  polling `master`. The host watchdog automatically tests, migrates, and deploys affected
  engine/backend components. A separate root-only least-privilege GitHub credential posts commit
  statuses; untrusted candidate code and application users cannot read it.
- The authoritative operator procedure is `docs/ops/DEPLOYMENT.md`; Stage 2 data/fencing details are in
  `docs/engine/STAGE2_POSTGRES_AUTHORITY.md`.

## Backups

| Item | Value |
|---|---|
| Cherry Backup Storage ID | `931342` |
| Public endpoint | `84.32.109.82:2223` |
| Borg repository | `ssh://backup-user@84.32.109.82:2223/backup/.repo` |
| Encryption | Borg `repokey-blake2` |
| Schedule | Daily, systemd `borgmatic.timer` |
| Retention | 7 daily, 4 weekly, 6 monthly |
| Checks | Repository weekly, recent archives monthly |

Borg backs up `/etc`, `/opt/apitoken`, `/var/lib/apitoken`, `/home/deploy` and `/root`, excluding
replaceable caches, toolchains and the live `/var/lib/apitoken/postgres` data directory. Docker
image/cache data under `/var/lib/docker` is intentionally not included. Once PostgreSQL is deployed,
backups must include application-consistent logical database dumps staged below
`/var/lib/apitoken/backups`; raw live database files are not a restore strategy.

`systemd/claude-api-backup.timer` runs `deploy/apitoken-db-dump` hourly. The script atomically creates
mode-0600 custom-format `commerce.dump` and `claude_engine.dump`; daily Borgmatic includes their
staging directory. Validate both using the matching PostgreSQL container's `pg_restore --list`.

The Borg private identity, repository key and passphrase are required for disaster recovery. An
independent copy exists on the operator workstation and must also be kept in an encrypted password
manager or other off-host location. A real restore of `/etc/hostname` was performed after repository
initialization.

The Cherry backup volume is tied to the Cherry server lifecycle. Do not terminate the server before
copying required archives to independent storage.

## Deployment procedure

[`docs/ops/DEPLOYMENT.md`](DEPLOYMENT.md) is the authoritative operator runbook and
[`CONTRIBUTING.md`](../../CONTRIBUTING.md) is the contributor/AI workflow. Normal delivery is automatic
after `master` changes: isolated tests, validated backups of both databases, migrations before
traffic admission, affected blue-green component cutovers, and exact-release verification. GitHub displays `deploy/tests`,
`deploy/migration`, `deploy/engine`, `deploy/backend`, and overall `deploy/watchdog` contexts.

The watchdog invokes the same two-phase controllers. These direct commands are retained for
recovery; every release remains an exact tested 40-character SHA:

```bash
deploy/deploy.sh --engine-bluegreen <sha>  # build/finalize/select; serving slot untouched
deploy/engine-bluegreen.sh                 # admit target, pre-drain and stop old

deploy/deploy.sh --api-only <sha>          # build, locked prebuilt migration, select
deploy/api-bluegreen.sh                    # admit target, pre-drain and stop old
```

Do not use the unqualified full-stack deploy after Stage 2 and do not manually restart a component
between its two phases. Commerce migration runs automatically from the exact tested candidate before
backend activation, under a backup gate and both file/advisory locks. It is additive and not
reversed by rollback. PostgreSQL is never restarted by an application deploy.

The worker remains single-instance stop-old/start-new, so a backend cutover has a short worker gap,
but it runs from the same exact immutable commerce release as the API. The watchdog builds/selects
that release before `--with-worker` and verifies the worker PID's working directory afterward.
Controller recovery, rollback, verification, and backup commands are all in the runbook. Detailed
availability behavior remains in `deploy/README.md` and `deploy/CADDY.md`.

## Vercel frontend

The customer frontend lives in `apps/web` and deploys independently to Vercel. Configure the Vercel
project with `apps/web` as its Root Directory, use the Next.js framework preset, set
`NEXT_PUBLIC_BACKEND_URL=https://backend.apitoken.sale/v1`, and attach the production apex domain
`apitoken.sale`. Vercel must install from the repository-level pnpm workspace and lockfile.

Keep `apitoken.sale` as the canonical browser origin. The commercial API deliberately allows that
one exact origin for credentialed CORS and state-changing requests. Redirect alternate frontend
hosts such as `www.apitoken.sale` to the apex instead of serving the application from multiple
origins. The Vercel frontend contains no Control API or payment-provider secrets.

## Work still requiring external configuration

- Add external health monitoring for both public routes and PostgreSQL authority readiness.
- Configure SMTP on a separate mail host.
- Add Google and GitHub OAuth application credentials.
- Add Cryptomus credentials and test its deployed webhook.
- Import `apps/web` into Vercel, attach `apitoken.sale`, and update the apex DNS using the exact
  records Vercel provides for the project.
- Add external monitoring/alert delivery and an independent second backup location.
- Before cross-host active/active, move PostgreSQL to synchronous multi-AZ service and add an external
  health-checked load balancer; never share the retained SQLite snapshot over NFS.
