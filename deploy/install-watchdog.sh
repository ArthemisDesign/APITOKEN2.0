#!/usr/bin/env bash
set -euo pipefail

# One-time root installer for the host-local, free GitHub polling watchdog.
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'run as root' >&2; exit 1; }
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command -v systemctl >/dev/null || { echo 'systemd is required' >&2; exit 1; }
id deploy >/dev/null 2>&1 || { echo 'deploy user is required' >&2; exit 1; }
id apitoken-ci >/dev/null 2>&1 || useradd --system --home-dir /var/lib/apitoken/watchdog/ci-home --create-home --shell /usr/sbin/nologin apitoken-ci

install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/controller
install -d -o deploy -g deploy -m 0750 /var/lib/apitoken/watchdog /var/lib/apitoken/watchdog/candidates /var/lib/apitoken/watchdog/ci-home
install -d -o root -g root -m 0755 /opt/apitoken-watchdog
install -o root -g root -m 0755 "$ROOT/deploy/watchdog.sh" /usr/local/lib/apitoken-watchdog/watchdog.sh
install -o root -g root -m 0644 "$ROOT/deploy/watchdog-lib.sh" /usr/local/lib/apitoken-watchdog/watchdog-lib.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-test-db.sh" /usr/local/lib/apitoken-watchdog/watchdog-test-db
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-migrate.sh" /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh
install -o root -g root -m 0755 "$ROOT/deploy/watchdog-control.sh" /usr/local/bin/apitoken-watchdog
install -o root -g root -m 0755 "$ROOT/deploy/deploy.sh" /usr/local/lib/apitoken-watchdog/controller/deploy.sh
install -o root -g root -m 0644 "$ROOT/deploy/lib.sh" /usr/local/lib/apitoken-watchdog/controller/lib.sh
install -o root -g root -m 0755 "$ROOT/deploy/api-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/api-bluegreen.sh
install -o root -g root -m 0755 "$ROOT/deploy/engine-bluegreen.sh" /usr/local/lib/apitoken-watchdog/controller/engine-bluegreen.sh
install -o root -g root -m 0644 "$ROOT/systemd/apitoken-deploy-watchdog.service" /etc/systemd/system/apitoken-deploy-watchdog.service
install -o root -g root -m 0644 "$ROOT/systemd/apitoken-deploy-watchdog.timer" /etc/systemd/system/apitoken-deploy-watchdog.timer
install -d -o root -g deploy -m 0775 /run/lock
for lock in apitoken-watchdog apitoken-deploy; do
  touch "/run/lock/$lock.lock"; chown root:deploy "/run/lock/$lock.lock"; chmod 0664 "/run/lock/$lock.lock"
done
[[ -d /opt/apitoken/repo/.git ]] || { echo 'missing /opt/apitoken/repo checkout' >&2; exit 1; }
[[ -d /opt/apitoken-watchdog/rust-toolchain/bin ]] || { echo 'install Rust in /opt/apitoken-watchdog/rust-toolchain first' >&2; exit 1; }
[[ -d /opt/apitoken/repo/packages/db/migrations ]] || { echo 'missing migration directory' >&2; exit 1; }
manifest=/var/lib/apitoken/watchdog/database-migrations.manifest
( cd /opt/apitoken/repo && while IFS= read -r p; do sha256sum "$p" | awk '{print $1 "  " $2}'; done < <(find packages/db/migrations -type f -print | sort) ) >"$manifest"
chown root:deploy "$manifest"; chmod 0640 "$manifest"
systemctl daemon-reload
systemctl enable --now apitoken-deploy-watchdog.timer
echo 'watchdog installed and timer enabled; verify with: sudo apitoken-watchdog status'
