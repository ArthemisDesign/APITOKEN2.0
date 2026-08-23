#!/usr/bin/env bash
# Ubuntu-only proofs for host installers. Runs as root inside deploy/host-image.
# Do not invoke this script on the production host or on Darwin.
set -euo pipefail

SRC=${HOST_IMAGE_SRC:-/src}
die() { printf 'host-image-proof: %s\n' "$*" >&2; exit 1; }
proof() { printf 'host-image-proof: %s\n' "$*"; }

[[ ${EUID:-$(id -u)} -eq 0 ]] || die 'must run as root'
[[ -d $SRC/deploy && -d $SRC/systemd ]] || die "repository is not mounted at $SRC"
[[ -x $SRC/deploy/install-observe.sh ]] || die 'install-observe.sh is missing'

# Every deploy/install-*.sh must have an explicit proof or an explicit skip.
# A new installer that is neither listed here nor skipped is a red gate.
declare -A INSTALLER_PROOF=()
declare -A INSTALLER_SKIP=()
INSTALLER_PROOF[install-observe.sh]=1
INSTALLER_PROOF[install-tmpfiles.sh]=1
INSTALLER_PROOF[install-sysctl.sh]=1
INSTALLER_PROOF[install-sudoers.sh]=1
INSTALLER_PROOF[install-watchdog.sh]=1
INSTALLER_PROOF[install-staging-foundation.sh]=1
INSTALLER_PROOF[install-staging-twin.sh]=1
INSTALLER_PROOF[install-caddy.sh]=1
INSTALLER_SKIP[install-monitoring.sh]='Compose pull of the monitoring stack is not Ubuntu identity'

while IFS= read -r -d '' installer; do
  name=${installer##*/}
  if [[ -n ${INSTALLER_PROOF[$name]+x} ]]; then
    continue
  fi
  if [[ -n ${INSTALLER_SKIP[$name]+x} ]]; then
    proof "skip $name: ${INSTALLER_SKIP[$name]}"
    continue
  fi
  die "new installer $name has no host-image proof and no documented skip"
done < <(find "$SRC/deploy" -maxdepth 1 -type f -name 'install-*.sh' -print0)

id deploy >/dev/null 2>&1 || useradd --create-home --home-dir /home/deploy \
  --shell /bin/bash --comment 'apitoken deploy' deploy
install -d -o deploy -g deploy -m 0700 /home/deploy/.ssh
if [[ ! -f /home/deploy/.ssh/authorized_keys ]]; then
  ssh-keygen -t ed25519 -N '' -f /tmp/host-image-deploy -q
  install -o deploy -g deploy -m 0600 /tmp/host-image-deploy.pub /home/deploy/.ssh/authorized_keys
fi

install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog
install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/controller
install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/sudoers.d
install -o root -g root -m 0755 "$SRC/deploy/apitoken-observe.sh" \
  /usr/local/lib/apitoken-watchdog/apitoken-observe.sh
install -o root -g root -m 0755 "$SRC/deploy/install-observe.sh" \
  /usr/local/lib/apitoken-watchdog/install-observe.sh
install -o root -g root -m 0755 "$SRC/deploy/install-tmpfiles.sh" \
  /usr/local/lib/apitoken-watchdog/install-tmpfiles.sh
install -o root -g root -m 0755 "$SRC/deploy/install-sysctl.sh" \
  /usr/local/lib/apitoken-watchdog/install-sysctl.sh
install -o root -g root -m 0755 "$SRC/deploy/install-sudoers.sh" \
  /usr/local/lib/apitoken-watchdog/install-sudoers.sh
install -o root -g root -m 0644 "$SRC/systemd/apitoken-tmpfiles.conf" \
  /usr/local/lib/apitoken-watchdog/apitoken-tmpfiles.conf
install -o root -g root -m 0644 "$SRC/systemd/sysctl-apitoken-redis.conf" \
  /usr/local/lib/apitoken-watchdog/sysctl-apitoken-redis.conf
install -o root -g root -m 0440 "$SRC/deploy/sudoers.d/95-apitoken-deploy" \
  /usr/local/lib/apitoken-watchdog/sudoers.d/95-apitoken-deploy

# GNU coreutils: Darwin `stat` has no -c. Production installers require it.
probe=$(mktemp)
echo probe >"$probe"
stat -c '%u:%a' -- "$probe" >/dev/null \
  || die 'GNU stat -c is unavailable'
rm -f -- "$probe"
proof 'GNU stat -c works'

# 2026-08-22 seed: useradd/passwd/shells/home must fail when /etc and /home are read-only,
# the same ProtectSystem=full / ProtectHome=read-only namespace as the watchdog unit.
command -v unshare >/dev/null || die 'unshare is required'
unshare --mount true >/dev/null 2>&1 \
  || die 'unshare --mount is unavailable (Docker must allow privileged SYS_ADMIN)'
ro_status=0
unshare --mount --propagation private bash -c '
  set -euo pipefail
  mount --make-rprivate / || true
  mount --bind /etc /etc
  mount -o remount,bind,ro /etc
  mount --bind /home /home
  mount -o remount,bind,ro /home
  if touch /etc/host-image-ro-probe 2>/dev/null; then
    rm -f /etc/host-image-ro-probe
    printf "etc-still-writable\n" >&2
    exit 2
  fi
  if touch /home/host-image-ro-probe 2>/dev/null; then
    rm -f /home/host-image-ro-probe
    printf "home-still-writable\n" >&2
    exit 2
  fi
  /usr/local/lib/apitoken-watchdog/install-observe.sh
' >/tmp/observe-ro.log 2>&1 || ro_status=$?
(( ro_status != 0 )) || {
  cat /tmp/observe-ro.log >&2 || true
  die 'observe installer succeeded inside a read-only passwd/home namespace'
}
grep -Eiq 'read-only file system|Read-only file system|cannot create|Permission denied' \
  /tmp/observe-ro.log \
  || {
    cat /tmp/observe-ro.log >&2 || true
    die 'read-only observe failure did not mention a read-only filesystem'
  }
id observe >/dev/null 2>&1 && die 'observe user was created inside the read-only namespace'
proof 'observe installer fails when passwd/shells/home are read-only'

/usr/local/lib/apitoken-watchdog/install-observe.sh
id observe >/dev/null 2>&1 || die 'observe user was not created'
getent group observe >/dev/null || die 'observe group was not created'
[[ $(id -gn observe) == observe ]] || die "observe primary group is $(id -gn observe), not observe"
[[ $(getent passwd observe | awk -F: '{print $7}') == /usr/local/bin/apitoken-observe ]] \
  || die 'observe login shell is not the wrapper'
grep -qxF /usr/local/bin/apitoken-observe /etc/shells \
  || die 'wrapper is not listed in /etc/shells'
[[ -d /home/observe && ! -L /home/observe ]] || die '/home/observe must be a real directory'
[[ $(stat -c '%U:%G:%a' -- /home/observe) == observe:observe:750 ]] \
  || die "/home/observe ownership/mode is $(stat -c '%U:%G:%a' -- /home/observe)"
if getent group systemd-journal >/dev/null; then
  id -Gn observe | tr ' ' '\n' | grep -Fxq systemd-journal \
    || die 'observe is not in systemd-journal'
fi
if getent group adm >/dev/null; then
  id -Gn observe | tr ' ' '\n' | grep -Fxq adm \
    || die 'observe is not in adm'
fi
id -Gn observe | tr ' ' '\n' | grep -Fxq deploy \
  && die 'observe must not be in the deploy group'
grep -Fq 'restrict,command="/usr/local/bin/apitoken-observe"' /home/observe/.ssh/authorized_keys \
  || die 'observe authorized_keys is missing ForceCommand'
/usr/local/lib/apitoken-watchdog/install-observe.sh
proof 'observe account is idempotent'

# Adopt an existing observe user that lost group observe (Ubuntu --system has no private group).
getent group observe-stale >/dev/null || groupadd --system observe-stale
usermod -g observe-stale observe
[[ $(id -gn observe) == observe-stale ]] || die 'could not move observe off its group for the adopt proof'
/usr/local/lib/apitoken-watchdog/install-observe.sh
[[ $(id -gn observe) == observe ]] || die 'observe installer did not adopt the existing user into group observe'
proof 'observe installer adopts an existing user into group observe'

# Same useradd line as install-watchdog.sh (full transaction).
install -d -o root -g root -m 0755 /var/lib/apitoken/watchdog
id apitoken-ci >/dev/null 2>&1 || useradd --system \
  --home-dir /var/lib/apitoken/watchdog/ci-home --create-home \
  --shell /usr/sbin/nologin apitoken-ci
if id -Gn apitoken-ci | tr ' ' '\n' | grep -Fxq deploy; then
  die 'apitoken-ci must not be in the deploy group'
fi
[[ -d /var/lib/apitoken/watchdog/ci-home ]] || die 'apitoken-ci home was not created'
proof 'apitoken-ci system user exists and is isolated from deploy'

install -d -o root -g root -m 0755 /etc/tmpfiles.d
/usr/local/lib/apitoken-watchdog/install-tmpfiles.sh
[[ -f /etc/tmpfiles.d/apitoken.conf && ! -L /etc/tmpfiles.d/apitoken.conf ]] \
  || die 'tmpfiles destination was not published'
[[ $(stat -c '%u:%a' -- /etc/tmpfiles.d/apitoken.conf) == 0:644 ]] \
  || die 'tmpfiles destination must be root-owned mode 0644'
[[ -d /run/apitoken ]] || die 'systemd-tmpfiles did not create /run/apitoken'
proof 'tmpfiles installer published /etc/tmpfiles.d/apitoken.conf'

install -d -o root -g root -m 0755 /etc/sysctl.d
sysctl_status=0
/usr/local/lib/apitoken-watchdog/install-sysctl.sh || sysctl_status=$?
[[ -f /etc/sysctl.d/99-apitoken-redis.conf && ! -L /etc/sysctl.d/99-apitoken-redis.conf ]] \
  || die 'sysctl destination was not published'
grep -Fxq 'vm.overcommit_memory = 1' /etc/sysctl.d/99-apitoken-redis.conf \
  || die 'sysctl destination lost the pinned Redis policy'
if (( sysctl_status == 0 )); then
  [[ $(/usr/sbin/sysctl -n vm.overcommit_memory) == 1 ]] \
    || die 'vm.overcommit_memory did not converge after a green sysctl installer'
  proof 'sysctl installer applied vm.overcommit_memory=1'
else
  proof 'sysctl file published; kernel apply is unavailable in this container'
fi

install -d -o root -g root -m 0755 /etc/systemd/system
while IFS= read -r -d '' unit; do
  install -o root -g root -m 0644 "$unit" "/etc/systemd/system/${unit##*/}"
done < <(find "$SRC/systemd" -maxdepth 1 -type f \( -name '*.service' -o -name '*.timer' -o -name '*.slice' \) -print0)

observe_unit=/etc/systemd/system/apitoken-observe-install.service
grep -Fxq 'ExecStart=/usr/local/lib/apitoken-watchdog/install-observe.sh' "$observe_unit" \
  || die 'observe oneshot lost its ExecStart'
if grep -Eq '^(ProtectSystem|ProtectHome)=' "$observe_unit"; then
  die 'observe oneshot must not set ProtectSystem/ProtectHome'
fi
tmpfiles_unit=/etc/systemd/system/apitoken-tmpfiles-install.service
grep -Fxq 'ProtectSystem=full' "$tmpfiles_unit" \
  || die 'tmpfiles oneshot lost ProtectSystem=full'
grep -Fxq 'ReadWritePaths=/etc/tmpfiles.d' "$tmpfiles_unit" \
  || die 'tmpfiles oneshot lost ReadWritePaths=/etc/tmpfiles.d'
sysctl_unit=/etc/systemd/system/apitoken-sysctl-install.service
grep -Fxq 'ReadWritePaths=/etc/sysctl.d' "$sysctl_unit" \
  || die 'sysctl oneshot lost ReadWritePaths=/etc/sysctl.d'
proof 'shipped oneshot units keep the watchdog-namespace split'

# Numeric UID/GID of the official Redis image (same lines as install-watchdog.sh).
install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis
install -d -o 999 -g 1000 -m 0700 /var/lib/apitoken/affinity-redis-l2
[[ $(stat -c '%u:%g:%a' -- /var/lib/apitoken/affinity-redis) == 999:1000:700 ]] \
  || die 'affinity-redis directory is not 999:1000 mode 0700'
[[ $(stat -c '%u:%g:%a' -- /var/lib/apitoken/affinity-redis-l2) == 999:1000:700 ]] \
  || die 'affinity-redis-l2 directory is not 999:1000 mode 0700'
proof 'Redis data directories use 999:1000'

# Real provision_authbot_proxy_admin_key against empty host paths.
install -d -o root -g root -m 0755 /etc/apitoken
install -d -o deploy -g deploy -m 0750 /srv/claude-api/data
fn=$(mktemp)
sed -n '/^proxy_admin_key_file_is_valid()/,/^provision_redis_data_dirs()/{
  /^provision_redis_data_dirs()/d
  p
}' "$SRC/deploy/install-watchdog.sh" >"$fn"
grep -Fq 'provision_authbot_proxy_admin_key()' "$fn" \
  || die 'could not extract provision_authbot_proxy_admin_key'
grep -Fq 'proxy_admin_key_files_equal()' "$fn" \
  || die 'could not extract proxy_admin_key_files_equal'
# shellcheck disable=SC1090
source "$fn"
rm -f -- "$fn"
provision_authbot_proxy_admin_key \
  || die 'provision_authbot_proxy_admin_key failed on an empty host'
[[ $(stat -c '%u:%g:%a' -- /etc/apitoken/proxy-admin.key) == 0:0:600 ]] \
  || die 'proxy-admin.key must be root:root mode 0600'
proxy_admin_key_file_is_valid /etc/apitoken/proxy-admin.key \
  || die 'proxy-admin.key is not a 64-lowercase-hex key'
if [[ -f /srv/claude-api/data/server.env ]] \
  && grep -Eq '^[[:space:]]*AUTH_BOT_PROXY_ADMIN_KEY(_FILE)?[[:space:]]*=' \
    /srv/claude-api/data/server.env; then
  die 'server.env must not contain proxy-admin credential settings'
fi
proof 'proxy-admin key provision writes root:root 0600 and stays out of server.env'

# Stage the two helpers install-sudoers.sh treats as mandatory trust anchors.
install -o root -g root -m 0755 "$SRC/deploy/pricing-retirement-postdrop.sh" \
  /usr/local/lib/apitoken-watchdog/pricing-retirement-postdrop.sh
install -o root -g root -m 0755 "$SRC/deploy/authbot-runtime-state.sh" \
  /usr/local/lib/apitoken-watchdog/controller/authbot-runtime-state.sh
install -o root -g root -m 0755 "$SRC/deploy/codex-homes-migrate.sh" \
  /usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh
APITOKEN_SUDOERS_SOURCE=$SRC/deploy/sudoers.d/95-apitoken-deploy \
  /usr/local/lib/apitoken-watchdog/install-sudoers.sh
[[ -f /etc/sudoers.d/95-apitoken-deploy && ! -L /etc/sudoers.d/95-apitoken-deploy ]] \
  || die 'sudoers policy was not published'
visudo -c >/dev/null || die 'installed sudoers policy does not validate'
sudo -n -u deploy sudo -n -l -- /usr/bin/systemctl daemon-reload >/dev/null \
  || die 'deploy cannot sudo systemctl daemon-reload after install-sudoers.sh'
proof 'sudoers installer passed visudo and live deploy verification'

# Caddy --check: seed a LIVE file that only supplies the header_up rows the renderer reads.
install -d -o root -g root -m 0755 /etc/caddy
proxy_hex=$(tr -d '\n' </etc/apitoken/proxy-admin.key)
{
  printf '\theader_up x-api-key host-image-control-key\n'
  printf '\theader_up x-admin-key host-image-admin-key-at-least-32-chars\n'
  printf '\theader_up x-sales-admin-key host-image-sales-admin-key\n'
  printf '\theader_up X-Proxy-Admin-Key "%s"\n' "$proxy_hex"
} >/etc/caddy/Caddyfile
printf '(router_backend) {\n\treverse_proxy 127.0.0.1:8800\n}\n' \
  >/etc/caddy/router-active.caddy
chown root:root /etc/caddy/Caddyfile /etc/caddy/router-active.caddy
chmod 0644 /etc/caddy/Caddyfile /etc/caddy/router-active.caddy
command -v caddy >/dev/null || die 'caddy is required for install-caddy.sh --check'
[[ $(stat -c '%u:%g:%a' -- /etc/apitoken/proxy-admin.key) == 0:0:600 ]] \
  || die 'proxy-admin.key must stay root:root 0600 before Caddy render'
render_probe=$(mktemp /etc/caddy/.host-image-render.XXXXXX)
awk -v proxy_admin_key_file=/etc/apitoken/proxy-admin.key \
  -v render_output="$render_probe" \
  -f "$SRC/deploy/render-caddy.awk" /etc/caddy/Caddyfile "$SRC/deploy/Caddyfile" \
  || die "Caddy renderer failed with GNU awk/stat (exit $?)"
! grep -q '<[A-Z_]*PLACEHOLDER>' "$render_probe" \
  || die 'Caddy renderer left a placeholder in the candidate'
rm -f -- "$render_probe"
proof 'Caddy renderer replaced placeholders using GNU stat-backed keys'
CADDY_TEMPLATE=$SRC/deploy/Caddyfile CADDY_CONFIG=/etc/caddy/Caddyfile \
  "$SRC/deploy/install-caddy.sh" --check
proof 'install-caddy.sh --check passed GNU stat and caddy validate'

# Linux journalctl exists here; Darwin wrapper tests accept exit 127.
status=0
SSH_ORIGINAL_COMMAND='help' bash "$SRC/deploy/apitoken-observe.sh" >/tmp/observe-help.out \
  || status=$?
(( status == 0 )) || die "observe wrapper help failed with $status"
grep -Fq 'log-only host session' /tmp/observe-help.out \
  || die 'observe wrapper help lost its banner'
status=0
SSH_ORIGINAL_COMMAND='logs claude-api-anthropic@8787.service' \
  bash "$SRC/deploy/apitoken-observe.sh" >/tmp/observe-logs.out 2>&1 || status=$?
(( status == 0 || status == 1 )) \
  || die "observe wrapper logs failed unexpectedly with $status (127 is Darwin-only)"
proof 'observe wrapper runs with Linux journalctl'

# Phase 2 staging foundation static and scaled-loopback proofs. The real 80G file and live netns are
# created only by the trusted master-sourced manager oneshot on the host.
bash "$SRC/deploy/staging-foundation.test.sh"
scaled=$(mktemp /tmp/staging-loopback.XXXXXX)
truncate -s 64M "$scaled"
mkfs.ext4 -F -m 0 "$scaled" >/dev/null
[[ $(stat -c %s "$scaled") == 67108864 ]] || die 'scaled staging loopback has wrong size'
rm -f "$scaled"
proof 'staging foundation contracts and scaled loopback passed'

bash "$SRC/deploy/staging-twin.test.sh"
if "$SRC/deploy/install-staging-twin.sh"; then
  die 'install-staging-twin.sh ran without a stage netns'
fi
proof 'install-staging-twin.sh fails closed without the stage netns'

proof 'all Ubuntu host proofs passed'
