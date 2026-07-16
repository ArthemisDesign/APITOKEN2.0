# Production infrastructure

This document records the non-secret production topology for `apitoken.sale`. Credentials, OAuth
secrets, payment-provider keys, subscription tokens and database passwords must never be added to
the repository. Commerce secrets live in root-readable files below `/etc/apitoken`; engine secrets
live below `/srv/claude-api/data` on the host.

## Current topology

```text
api.apitoken.sale --------------------------> commercial host 84.32.48.2 (Chicago)
                                                | reverse proxy
                                                |-> Rust core slot 127.0.0.1:8787
                                                `-> Rust core slot 127.0.0.1:8788

future browser at apitoken.sale
        |
        `-> backend.apitoken.sale ----------> commercial host 84.32.48.2
                                                |-> NestJS API slots 127.0.0.1:3000/3001
                                                |-> payment/email/pricing worker ------.
                                                |                                      |
                                                |   stable Control API 127.0.0.1:8790 <-'
                                                |          |-> engine slot :8787
                                                |          `-> engine slot :8788
                                                `-> PostgreSQL 18 on 127.0.0.1:5433
                                                    |-> commerce DB/role
                                                    `-> claude_engine DB/isolated role

commercial host -- encrypted Borg/SSH --> 84.32.109.82:2223/backup/.repo
```

`api.apitoken.sale` is the public Anthropic-compatible core endpoint. Its public proxy exposes only
`/v1/*`, `/health`, and `/balance`, not the engine Control API. `backend.apitoken.sale` is the browser-facing
commercial API. The Rust engine remains authoritative for API keys, balances, reservations and
usage. The commercial PostgreSQL database owns users, authentication, payment state, B2C/B2B
pricing state and durable jobs. The commercial services access the engine only through Caddy's
explicitly loopback-bound stable Control API at `127.0.0.1:8790`, never through a deployment slot.
The legacy core host is not an upstream or fallback.

The Rust engine ran on an interim host (`5.9.59.83`, shared with an unrelated project) until it was
migrated onto this commercial host on 2026-07-14. The engine now runs here as `claude-api@8787` or
`claude-api@8788` (plus `claude-authbot.service` and the `claude-api-backup.timer`) under the
`deploy` user, with its
authority in the isolated PostgreSQL `claude_engine` database; the drained SQLite snapshot remains
at `/srv/claude-api/data/subscriptions.db`. Its secrets live at
`/srv/claude-api/data/{server.env,config.env,authbot.env,engine-postgres.env}` (separate from the commercial
`/etc/apitoken/*.env`). The interim host keeps a cold pre-migration copy for forensics, not as a
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
/opt/apitoken/repo             deployment checkout; also the current mutable worker runtime
/opt/apitoken/releases/<sha>   immutable commerce release directories
/opt/apitoken/releases/current active commerce release symlink
/srv/claude-api/releases/<sha> immutable Rust engine release directories
/srv/claude-api/releases/current active engine release symlink
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
systemd/claude-api.service          one-time SQLite-to-PostgreSQL bridge
systemd/claude-api@.service         PostgreSQL-fenced blue/green slots
systemd/claude-api-backup.service
systemd/claude-api-backup.timer
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
sudo systemctl status apitoken-postgres apitoken-worker 'apitoken-api@*' 'claude-api@*'
sudo journalctl -u 'apitoken-api@*' -u apitoken-worker -u 'claude-api@*' --since today
sudo systemctl status caddy
sudo caddy validate --config /etc/caddy/Caddyfile
curl -fsS http://127.0.0.1:8790/ready
```

The PostgreSQL container publishes only to `127.0.0.1:5433`. API and worker use the same database
and authentication-token encryption key. Both use the server-side engine Control key, which must
never be returned to clients or placed in frontend configuration.

### Current deployment model (verified 2026-07-16)

- Stage 2 is complete: the engine authority is the role-isolated `claude_engine` PostgreSQL database;
  SQLite is a retained audit snapshot and must not be reactivated after production writes.
- Engine 8787/8788 and API 3000/3001 are health-gated blue-green slots. Only one slot per component
  remains enabled after a normal cutover.
- API and worker load `ENGINE_BASE_URL=http://127.0.0.1:8790`; Caddy binds that listener explicitly
  to loopback and routes only ready engine slots.
- Public engine, commerce API, Caddy, worker, and the hourly dual-database backup timer are active.
- The core public matcher exposes `/v1/*`, `/health`, and `/balance`; Control/admin routes remain
  private. Public liveness/readiness behavior is described in `deploy/CADDY.md`.
- A dedicated read-only GitHub deploy key at `/home/deploy/.ssh/github_deploy_ed25519` supports
  `git fetch`; pushing a commit does not automatically deploy it.
- The authoritative operator procedure is `DEPLOYMENT.md`; Stage 2 data/fencing details are in
  `docs/STAGE2_POSTGRES_AUTHORITY.md`.

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

[`DEPLOYMENT.md`](DEPLOYMENT.md) is the authoritative copy-paste runbook. In short, every release is
an exact tested 40-character SHA and every stateless component uses two explicit phases:

```bash
deploy/deploy.sh --engine-bluegreen <sha>  # build/finalize/select; serving slot untouched
deploy/engine-bluegreen.sh                 # admit target, pre-drain and stop old

deploy/deploy.sh --api-only <sha>          # build, locked prebuilt migration, select
deploy/api-bluegreen.sh                    # admit target, pre-drain and stop old
```

Do not use the unqualified full-stack deploy after Stage 2 and do not manually restart a component
between its two phases. Commerce migration runs from the immutable release as
`node /opt/apitoken/releases/<sha>/packages/db/dist/migrate.js`; it is additive and not reversed by
rollback. PostgreSQL is never restarted by an application deploy.

The worker remains separately managed and mutable until it receives a release-symlink unit. Its
checked-out SHA and workspace dependencies must be built before stop-old/start-new; `--with-worker`
does not build it. Controller recovery, rollback, verification, and backup commands are all in the
runbook. Detailed availability behavior remains in `deploy/README.md` and `deploy/CADDY.md`.

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
