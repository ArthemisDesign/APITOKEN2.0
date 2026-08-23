#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
TEMP_OBSERVE_STAGE_FIXTURE=$(mktemp)
trap 'rm -f -- "$TEMP_OBSERVE_STAGE_FIXTURE"' EXIT
I=$ROOT/deploy/install-staging-foundation.sh
bash -n "$I" "$ROOT/deploy/apitoken-observe-stage.sh" "$ROOT/deploy/stage-observe-helper.sh" \
  "$ROOT/deploy/apitoken-stage-ctl.sh" "$ROOT/deploy/stage-ctl-helper.sh"
for locked in 'fallocate -l 80G' '10.254.32.1/30' '10.254.32.2/30' \
  'ip netns add apitoken-stage' 'policy drop' 'mount -o loop,nodev,nosuid' \
  '/opt/apitoken-staging' '/srv/claude-api-staging' '/var/lib/apitoken-staging' \
  'type filter hook output priority 0; policy drop;' 'ct state established,related accept'; do
  grep -Fq "$locked" "$I" || { echo "staging foundation lost: $locked" >&2; exit 1; }
done
! grep -Eq '5434|13000|18787|/var/run/docker.sock|systemctl (start|restart|stop) apitoken-postgres\.service' "$I"
for unit in staging.slice apitoken-staging-foundation-install.service apitoken-rootless-docker-stage.service; do
  [[ -f $ROOT/systemd/$unit ]] || exit 1
done
grep -Fxq 'MemoryMax=32G' "$ROOT/systemd/staging.slice"
grep -Fxq 'MemoryHigh=28G' "$ROOT/systemd/staging.slice"
grep -Fxq 'CPUQuota=400%' "$ROOT/systemd/staging.slice"
grep -Fxq 'TasksMax=16384' "$ROOT/systemd/staging.slice"
grep -Fxq 'IOWeight=10' "$ROOT/systemd/staging.slice"
grep -Fxq 'Slice=staging.slice' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Delegate=yes' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_NET=slirp4netns' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_PORT_DRIVER=slirp4netns' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
! grep -Fq '/var/run/docker.sock' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fq 'unix:///run/apitoken-staging/docker.sock' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fq '/usr/local/bin/apitoken-observe-stage' "$ROOT/deploy/install-watchdog.sh"
grep -Fq '/usr/local/bin/apitoken-stage-ctl' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'systemctl restart apitoken-staging-foundation-install.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'systemctl start --no-block apitoken-rootless-docker-stage.service' "$I"
! grep -Eq 'systemctl (enable )?--now apitoken-rootless-docker-stage|systemctl start apitoken-rootless-docker-stage' "$I"
for compose in staging-postgres.compose.yaml staging-redis.compose.yaml; do
  grep -Fq 'cgroup_parent: staging.slice' "$ROOT/deploy/$compose"
  ! grep -Eq '5434|127\.0\.0\.1|/var/run/docker.sock|privileged:' "$ROOT/deploy/$compose"
done
grep -Fq '10.254.32.2:5433:5432' "$ROOT/deploy/staging-postgres.compose.yaml"
grep -Fq '10.254.32.2:6379:6379' "$ROOT/deploy/staging-redis.compose.yaml"
grep -Fq '10.254.32.2:6380:6379' "$ROOT/deploy/staging-redis.compose.yaml"
for unit in apitoken-postgres-stage.service apitoken-redis-stage.service; do
  grep -Fxq 'Slice=staging.slice' "$ROOT/systemd/$unit"
  grep -Fxq 'ConditionPathIsMountPoint=/var/lib/apitoken-staging' "$ROOT/systemd/$unit"
done
for user in deploy-stage stage-ci observe-stage stage-ctl; do grep -Fq "$user" "$I"; done
grep -Fq 'make_user stage-ci' "$I" || exit 1
stage_ci_line=$(grep -nF 'make_user stage-ci' "$I" | cut -d: -f1)
bind_line=$(grep -nF 'mount --bind "$src" "$dst"' "$I" | cut -d: -f1)
[[ $stage_ci_line -gt $bind_line ]] || { echo 'stage-ci is created before loopback bind roots' >&2; exit 1; }
grep -Fq 'attest|sync|reseed' "$ROOT/deploy/stage-ctl-helper.sh"
grep -Fq 'phase-disabled' "$ROOT/deploy/stage-ctl-helper.sh"
grep -Fq 'systemctl stop staging.slice' "$ROOT/deploy/stage-ctl-helper.sh"
if SSH_ORIGINAL_COMMAND='shell' bash "$ROOT/deploy/apitoken-observe-stage.sh" >/dev/null 2>&1; then exit 1; fi
wrapper=$TEMP_OBSERVE_STAGE_FIXTURE
sed -e 's#^HELPER=.*#HELPER=/bin/echo#' -e 's#exec sudo -n "\$HELPER"#exec "\$HELPER"#' \
  "$ROOT/deploy/apitoken-observe-stage.sh" >"$wrapper"
SSH_ORIGINAL_COMMAND='logs apitoken-staging-foundation-install.service --since 10 minutes ago' \
  bash "$wrapper" | grep -Fq '10 minutes ago' || { rm -f "$wrapper"; exit 1; }
rm -f "$wrapper"
if SSH_ORIGINAL_COMMAND='sync' SUDO_USER=stage-ctl bash "$ROOT/deploy/stage-ctl-helper.sh" >/dev/null 2>&1; then exit 1; fi
printf 'staging-foundation.test: PASS\n'
