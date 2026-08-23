#!/usr/bin/env bash
set -euo pipefail
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'staging-foundation: root required' >&2; exit 1; }
trap 'rc=$?; echo "staging-foundation: failed line=${LINENO} status=$rc" >&2' ERR
ROOT=/usr/local/lib/apitoken-watchdog
PROD=$ROOT/contour-production.json
STAGE=$ROOT/contour-stage.json
python3 "$ROOT/contour-config.py" --schema "$ROOT/contour-config.schema.json" \
  --config "$STAGE" --against "$PROD" >/dev/null
if ! command -v slirp4netns >/dev/null || ! command -v fuse-overlayfs >/dev/null \
    || ! command -v newuidmap >/dev/null; then
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    slirp4netns fuse-overlayfs uidmap
fi
# phase2-prerequisite-apply-v2: keep stateful foundation changes on the full trusted lane.
# phase2-userns-apply-v1: replay after the rootless user-namespace sandbox change.
# phase2-detach-apply-v1: replay after detached-netns compatibility changed.
# phase2-sandbox-apply-v1: replay after RootlessKit namespace filtering changed.
# phase2-tmp-apply-v1: replay after the RootlessKit private tmp change.
# phase2-offline-apply-v1: replay after Compose offline mode changed.
# phase2-seed-tags-apply-v1: replay after rootless digest-reference repair.
# phase2-seed-id-apply-v1: replay after content-ID reference repair.
# phase2-source-tag-apply-v1: replay after stable source-tag export changed.
# phase2-local-tags-apply-v1: replay after local-only Compose tags changed.
# phase2-explicit-seed-apply-v1: replay after explicit source tag mapping changed.
# phase2-cgroup-apply-v1: replay after delegated container cgroup parent changed.
# phase2-cgroupfs-apply-v1: replay after delegated cgroupfs changed.
# phase2-postgres-volume-apply-v1: replay after stage PostgreSQL data mount changed.
# phase2-pgdata-apply-v1: replay after PostgreSQL 18 PGDATA changed.
# phase2-pgdata-subdir-apply-v1: replay after clean PGDATA child changed.
# phase2-postgres-named-apply-v1: replay after rootless named volume changed.
for command in rootlesskit slirp4netns fuse-overlayfs newuidmap newgidmap dockerd-rootless.sh; do
  command -v "$command" >/dev/null || { echo "staging-foundation: missing rootless Docker prerequisite: $command" >&2; exit 1; }
done

make_user() {
  local user=$1 home=$2 shell=$3
  getent group "$user" >/dev/null || groupadd --system "$user"
  id "$user" >/dev/null 2>&1 || useradd --system --gid "$user" --create-home \
    --home-dir "$home" --shell "$shell" --comment 'apitoken staging identity' "$user"
  usermod -g "$user" --home "$home" --shell "$shell" "$user"
}
make_user deploy-stage /home/deploy-stage /usr/sbin/nologin
make_user observe-stage /home/observe-stage /usr/local/bin/apitoken-observe-stage
make_user stage-ctl /home/stage-ctl /usr/local/bin/apitoken-stage-ctl
for user in deploy-stage observe-stage stage-ctl; do
  for group in deploy docker apitoken-ci observe adm systemd-journal; do
    if getent group "$group" >/dev/null && id -Gn "$user" | tr ' ' '\n' | grep -Fxq "$group"; then
      gpasswd -d "$user" "$group" >/dev/null || true
    fi
  done
done
for db in /etc/subuid /etc/subgid; do
  touch "$db"; grep -Eq '^deploy-stage:[0-9]+:65536$' "$db" || echo 'deploy-stage:231072:65536' >>"$db"
done
install -o root -g root -m 0755 "$ROOT/apitoken-observe-stage.sh" /usr/local/bin/apitoken-observe-stage
install -o root -g root -m 0755 "$ROOT/apitoken-stage-ctl.sh" /usr/local/bin/apitoken-stage-ctl
for wrapper in /usr/local/bin/apitoken-observe-stage /usr/local/bin/apitoken-stage-ctl; do
  grep -qxF "$wrapper" /etc/shells || echo "$wrapper" >>/etc/shells
done
for user in observe-stage stage-ctl; do
  home=/home/$user; install -d -o "$user" -g "$user" -m 0750 "$home"
  install -d -o "$user" -g "$user" -m 0700 "$home/.ssh"
  tmp=$(mktemp); wrapper=/usr/local/bin/apitoken-$user
  [[ $user != stage-ctl ]] || wrapper=/usr/local/bin/apitoken-stage-ctl
  if [[ -f /home/deploy/.ssh/authorized_keys && ! -L /home/deploy/.ssh/authorized_keys ]]; then
    awk -v wrapper="$wrapper" '
      match($0, /(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256|sk-ssh-ed25519@openssh.com|sk-ecdsa-sha2-nistp256@openssh.com) [A-Za-z0-9+\/=]+/) {
        printf "restrict,command=\"%s\" %s\n", wrapper, substr($0,RSTART,RLENGTH)
      }' /home/deploy/.ssh/authorized_keys >"$tmp"
  fi
  install -o "$user" -g "$user" -m 0600 "$tmp" "$home/.ssh/authorized_keys"; rm -f "$tmp"
done

IMAGE=/var/lib/apitoken-staging.img; MOUNT=/mnt/apitoken-staging
install -d -m 0755 "$MOUNT"
if [[ ! -e $IMAGE ]]; then fallocate -l 80G "$IMAGE"; mkfs.ext4 -F -m 0 -L apitoken-staging "$IMAGE" >/dev/null; fi
[[ -f $IMAGE && ! -L $IMAGE && $(stat -c %s "$IMAGE") == 85899345920 ]] || exit 1
mountpoint -q "$MOUNT" || mount -o loop,nodev,nosuid "$IMAGE" "$MOUNT"
for item in opt:/opt/apitoken-staging srv:/srv/claude-api-staging var:/var/lib/apitoken-staging; do
  src=$MOUNT/${item%%:*}; dst=${item#*:}
  install -d -o deploy-stage -g deploy-stage -m 0750 "$src"
  if [[ -e $dst || -L $dst ]]; then
    [[ -d $dst && ! -L $dst ]] || { echo "staging-foundation: unsafe bind target $dst" >&2; exit 1; }
  else
    mkdir -p "$dst"
    chown deploy-stage:deploy-stage "$dst"
    chmod 0750 "$dst"
  fi
  mountpoint -q "$dst" || mount --bind "$src" "$dst"
done
install -d -o root -g deploy-stage -m 0750 /etc/apitoken-staging
install -d -o deploy-stage -g deploy-stage -m 0750 /etc/apitoken-staging/caddy \
  /var/lib/apitoken-staging/{backups,caddy,logs} /srv/claude-api-staging/releases \
  /opt/apitoken-staging/releases
install -o root -g deploy-stage -m 0640 "$ROOT/staging-Caddyfile" /etc/apitoken-staging/caddy/Caddyfile
for env in anthropic openai gemini kimi router api worker sales-api sales-web openkeys admin authbot devbot sinks; do
  path=/etc/apitoken-staging/$env.env
  if [[ ! -e $path ]]; then install -o root -g deploy-stage -m 0600 /dev/null "$path"; fi
done
make_user stage-ci /var/lib/apitoken-staging/watchdog/ci-home /usr/sbin/nologin
for group in deploy docker apitoken-ci observe adm systemd-journal; do
  if getent group "$group" >/dev/null && id -Gn stage-ci | tr ' ' '\n' | grep -Fxq "$group"; then
    gpasswd -d stage-ci "$group" >/dev/null || true
  fi
done
install -d -o deploy-stage -g deploy-stage -m 0700 /var/lib/apitoken-staging/{docker,postgres,redis,spool,watchdog}
install -d -o deploy-stage -g deploy-stage -m 0700 /run/apitoken-staging
install -o deploy-stage -g deploy-stage -m 0600 /dev/null /run/lock/apitoken-stage-watchdog.lock

ip netns list | awk '{print $1}' | grep -Fxq apitoken-stage || ip netns add apitoken-stage
if ! ip link show veth-stage-host >/dev/null 2>&1; then
  ip link add veth-stage-host type veth peer name veth-stage-ns
  ip link set veth-stage-ns netns apitoken-stage
fi
ip addr replace 10.254.32.1/30 dev veth-stage-host; ip link set veth-stage-host up
ip netns exec apitoken-stage ip link set lo up
ip netns exec apitoken-stage ip addr replace 10.254.32.2/30 dev veth-stage-ns
ip netns exec apitoken-stage ip link set veth-stage-ns up
ip netns exec apitoken-stage ip route del default >/dev/null 2>&1 || true
ip netns exec apitoken-stage nft delete table inet apitoken_stage >/dev/null 2>&1 || true
ip netns exec apitoken-stage nft -f - <<'NFT'
table inet apitoken_stage {
  chain output {
    type filter hook output priority 0; policy drop;
    oifname "lo" accept
    ct state established,related accept
  }
}
NFT

install -o root -g root -m 0644 "$ROOT/staging.slice" /etc/systemd/system/staging.slice
install -d -o root -g root -m 0755 /etc/polkit-1/rules.d
install -o root -g root -m 0644 "$ROOT/49-apitoken-stage-cgroup.rules" \
  /etc/polkit-1/rules.d/49-apitoken-stage-cgroup.rules
install -o root -g root -m 0644 "$ROOT/apitoken-rootless-docker-stage.service" /etc/systemd/system/apitoken-rootless-docker-stage.service
install -o root -g root -m 0440 "$ROOT/96-apitoken-stage" /etc/sudoers.d/96-apitoken-stage
visudo -c >/dev/null
if [[ ! -s /etc/apitoken-staging/postgres.password ]]; then
  umask 077
  openssl rand -hex 32 >/etc/apitoken-staging/postgres.password
fi
if [[ ! -s /etc/apitoken-staging/redis.env ]]; then
  umask 077
  printf 'STAGE_REDIS_PASSWORD=%s\n' "$(openssl rand -hex 32)" >/etc/apitoken-staging/redis.env
fi
chown root:deploy-stage /etc/apitoken-staging/postgres.password /etc/apitoken-staging/redis.env
chmod 0640 /etc/apitoken-staging/postgres.password /etc/apitoken-staging/redis.env
systemctl daemon-reload
loginctl enable-linger deploy-stage || echo 'staging-foundation: linger deferred to rootless Docker activation' >&2
# Do not synchronously start a unit that Requires this still-activating oneshot. Queue no-block jobs
# after the foundation has committed, so dependency ordering cannot deadlock the manager transaction.
systemctl start --no-block apitoken-rootless-docker-stage.service \
  apitoken-staging-image-seed.service apitoken-postgres-stage.service apitoken-redis-stage.service \
  apitoken-stage-safe-sinks.service apitoken-stage-caddy.service
printf 'staging-foundation: ready\n'
