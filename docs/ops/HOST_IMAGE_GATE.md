# Ubuntu host-image gate

This is the laptop proof for host installers that depend on Ubuntu userland or
systemd namespaces. It does not replace production apply. It does not wrap
`cargo` or `pnpm`.

## Why it exists

On 2026-08-22 the observe installer aborted with `Read-only file system`. Local
and trusted-host tests stayed green because they grepped scripts and stubbed
`systemctl`. They never ran Ubuntu `useradd` / `groupadd`. The watchdog unit
uses `ProtectSystem=full` and `ProtectHome=read-only`, so `/etc/passwd`,
`/etc/shells`, and `/home/observe` are read-only in that namespace. Ubuntu
`useradd --system` also does not create a matching group.

The host-image gate runs those commands in a disposable Ubuntu 24.04 container
before `deploy/agent-merge.sh` talks to the production host.

## When it runs

`wd_path_depends_on_ubuntu_host` in [`deploy/watchdog-lib.sh`](../../deploy/watchdog-lib.sh)
selects the gate. A match is any of:

- `systemd/*`
- `deploy/install-*.sh`
- `deploy/watchdog-infrastructure.sh`
- `deploy/sudoers.d/*`
- `deploy/apitoken-observe.sh`
- `deploy/affinity-redis.compose.yaml`
- `deploy/commerce-postgres.compose.yaml`
- `deploy/Caddyfile`
- `deploy/render-caddy.awk`
- `deploy/host-image-gate.sh`
- `deploy/host-image/*`

A crate-only, `apps/`-only, or docs-only merge does not start Docker. A change
to `deploy/agent-worktree.sh` or `DELETE_WORKTREE.sh` does not start Docker.

Missing Docker is a hard fail. The gate does not skip.

Manual run from a worktree:

```bash
./deploy/host-image-gate.sh
```

The Darwin wiring suite (classifier, allowlist, fail-closed missing Docker) is
`bash deploy/host-image-gate.test.sh`. It does not start a container.

## What the container executes

[`deploy/host-image/prove-installers.sh`](../../deploy/host-image/prove-installers.sh)
runs as root in a privileged `ubuntu:24.04` image. The worktree is mounted
read-only at `/src`. It:

- fails `install-observe.sh` after remounting `/etc` and `/home` read-only
  (the 2026-08-22 seed)
- creates group `observe` and user `observe`, including `/etc/shells` and
  `/home/observe`
- proves a second run is idempotent and that an existing user is adopted into
  group `observe`
- creates `apitoken-ci` with `useradd --system` and refuses group `deploy`
- runs `install-tmpfiles.sh` and `install-sysctl.sh`
- installs shipped oneshot units and checks the watchdog-namespace split
- creates Redis data directories as `999:1000`
- runs `provision_authbot_proxy_admin_key` on empty host paths
- applies `install-sudoers.sh` (visudo, live `sudo -l` as `deploy`)
- runs `install-caddy.sh --check` against a seeded loopback LIVE file
- runs the observe wrapper with Linux `journalctl` present

`install-monitoring.sh` is an explicit skip: a Compose pull of Grafana/Prometheus
is not Ubuntu identity. A new `deploy/install-*.sh` must gain a proof or a
documented skip, or the container is red.

Kernel `vm.overcommit_memory` is asserted when the container can apply it. If
the Docker kernel refuses, the gate still requires the sysctl file and a
completed installer write.

## What it does not do

- It does not start product slots, Caddy TLS, Postgres, or Redis.
- It does not run on the production host. `deploy/watchdog.sh` runs only
  `host-image-gate.test.sh`. Privileged nested systemd on the live VPS is
  extra blast radius. Production still applies the real installer after tests.
- It does not replace the fast grep suites (`watchdog-lib.test.sh`,
  `apitoken-observe.test.sh`). Those still run on Darwin.

## Local-only paths

`deploy/host-image-gate.sh` and `deploy/host-image/*` are validation-only. They
must not match `wd_path_requires_infrastructure_install`. Installing this
harness onto the VPS is a defect.
