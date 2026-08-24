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
grep -Fq -- '--exec-opt native.cgroupdriver=cgroupfs' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'NoNewPrivileges=no' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'PrivateUsers=no' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'PrivateTmp=yes' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'RestrictNamespaces=no' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_NET=slirp4netns' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_PORT_DRIVER=slirp4netns' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fxq 'Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_DETACH_NETNS=false' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
! grep -Fq '/var/run/docker.sock' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fq 'unix:///run/apitoken-staging/docker.sock' "$ROOT/systemd/apitoken-rootless-docker-stage.service"
grep -Fq '/usr/local/bin/apitoken-observe-stage' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'store-logs)' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq 'if ! output=$(curl -sS' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq 'Host: 10.254.32.2:$port' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq 'apitoken-stage-source-fetch.service|apitoken-stage-source-fetch.timer' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq 'apitoken-stage-watchdog.service|apitoken-stage-watchdog.timer' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq 'ip netns list' "$ROOT/deploy/staging-isolation-live.sh"
grep -Fq 'systemd-run --quiet --wait --pipe --collect --unit=staging-proof-memory' "$ROOT/deploy/staging-pressure-proof.sh"
grep -Fq 'stage-degrade-proof.sh' "$ROOT/deploy/stage-observe-helper.sh"
grep -Fq '/etc/apitoken/server.env' "$ROOT/deploy/staging-isolation-live.sh"
grep -Fq 'memory controller stopped reporting after bounded pressure' "$ROOT/deploy/staging-pressure-proof.sh"
grep -Fq 'apitoken-postgres-stage|apitoken-redis-history-stage|apitoken-redis-affinity-stage' "$ROOT/deploy/stage-store-diagnostics.sh"
grep -Fq '/usr/local/bin/apitoken-stage-ctl' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'systemctl restart apitoken-staging-foundation-install.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'systemctl enable apitoken-stage-source-fetch.timer apitoken-stage-report.path' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'apitoken-stage-watchdog.timer apitoken-stage-emergency-guard.timer' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'systemctl start apitoken-stage-source-fetch.timer apitoken-stage-report.path' "$ROOT/deploy/install-watchdog.sh"
bash "$ROOT/deploy/stage-watchdog.test.sh"
bash "$ROOT/deploy/staging-twin.test.sh"
bash "$ROOT/deploy/stage-degrade-gate.test.sh"
bash "$ROOT/deploy/staging-phase6-drills.test.sh"
bash "$ROOT/deploy/staging-phase7.test.sh"
bash "$ROOT/deploy/staging-phase8.test.sh"
grep -Fq 'for stage_unit in staging.slice apitoken-rootless-docker-stage.service' "$ROOT/deploy/install-watchdog.sh"
grep -Fq 'install -d -o root -g deploy-stage -m 0750 /usr/local/lib/apitoken-watchdog/stage' "$ROOT/deploy/install-watchdog.sh"
! grep -Fq '/usr/local/lib/apitoken-watchdog/stage /usr/local/bin' "$ROOT/systemd/apitoken-deploy-watchdog.service"
grep -Fq '/run/lock/apitoken-stage-watchdog.lock' "$I"
grep -Fq 'systemctl start --no-block apitoken-rootless-docker-stage.service' "$I"
for host_state_path in deploy/install-staging-foundation.sh deploy/staging-postgres.compose.yaml deploy/staging-redis.compose.yaml; do
  if bash -c 'source "$1/deploy/watchdog-lib.sh"; wd_path_is_controller_definition "$2"' \
      _ "$ROOT" "$host_state_path"; then
    echo "stateful stage definition classified controller-only: $host_state_path" >&2; exit 1
  fi
done
grep -Fq 'phase2-prerequisite-apply-v2' "$I"
grep -Fq 'phase2-userns-apply-v1' "$I"
grep -Fq 'phase2-detach-apply-v1' "$I"
grep -Fq 'phase2-sandbox-apply-v1' "$I"
grep -Fq 'phase2-tmp-apply-v1' "$I"
grep -Fq 'phase2-offline-apply-v1' "$I"
grep -Fq 'phase2-seed-tags-apply-v1' "$I"
grep -Fq 'phase2-seed-id-apply-v1' "$I"
grep -Fq 'phase2-source-tag-apply-v1' "$I"
grep -Fq 'phase2-local-tags-apply-v1' "$I"
grep -Fq 'phase2-explicit-seed-apply-v1' "$I"
grep -Fq 'phase2-cgroup-apply-v1' "$I"
grep -Fq 'phase2-cgroupfs-apply-v1' "$I"
grep -Fq 'phase2-postgres-volume-apply-v1' "$I"
grep -Fq 'phase2-pgdata-apply-v1' "$I"
grep -Fq 'phase2-pgdata-subdir-apply-v1' "$I"
grep -Fq 'phase2-postgres-named-apply-v1' "$I"
for prerequisite in slirp4netns fuse-overlayfs newuidmap newgidmap; do
  grep -Fq "$prerequisite" "$I"
done
! grep -Eq 'systemctl (enable )?--now apitoken-rootless-docker-stage|systemctl start apitoken-rootless-docker-stage' "$I"
for compose in staging-postgres.compose.yaml staging-redis.compose.yaml; do
  grep -Fq 'cgroup_parent: /' "$ROOT/deploy/$compose"
  ! grep -Eq '5434|127\.0\.0\.1|/var/run/docker.sock|privileged:' "$ROOT/deploy/$compose"
done
grep -Fq 'subject.user == "deploy-stage"' "$ROOT/deploy/49-apitoken-stage-cgroup.rules"
grep -Fq 'docker-[0-9a-f]{64}' "$ROOT/deploy/49-apitoken-stage-cgroup.rules"
grep -Fq 'org.freedesktop.systemd1.manage-units' "$ROOT/deploy/49-apitoken-stage-cgroup.rules"
grep -Fq 'apitoken-stage/postgres:18-alpine' "$ROOT/deploy/staging-postgres.compose.yaml"
grep -Fq 'apitoken-stage/redis:7.4.2-alpine' "$ROOT/deploy/staging-redis.compose.yaml"
grep -Fq 'docker image inspect "$source"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'docker pull "$source"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'local source=$1 source_tag=$2 target=$3' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'docker image inspect "$source_tag"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'docker save -o "$archive" "$source_tag"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'docker load -i "$archive"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'docker tag "$source_tag" "$target"' "$ROOT/deploy/staging-image-seed.sh"
grep -Fq 'postgres-stage-data:/var/lib/postgresql' "$ROOT/deploy/staging-postgres.compose.yaml"
grep -Fq 'name: apitoken-postgres-stage-data' "$ROOT/deploy/staging-postgres.compose.yaml"
grep -Fq '10.254.32.2:5433:5432' "$ROOT/deploy/staging-postgres.compose.yaml"
grep -Fq '10.254.32.2:6379:6379' "$ROOT/deploy/staging-redis.compose.yaml"
grep -Fq '10.254.32.2:6380:6379' "$ROOT/deploy/staging-redis.compose.yaml"
[[ -f $ROOT/systemd/apitoken-staging-image-seed.service ]]
for unit in apitoken-postgres-stage.service apitoken-redis-stage.service; do
  grep -Fxq 'Slice=staging.slice' "$ROOT/systemd/$unit"
  grep -Fxq 'ConditionPathIsMountPoint=/var/lib/apitoken-staging' "$ROOT/systemd/$unit"
  grep -Fq -- '--pull never' "$ROOT/systemd/$unit"
done
for user in deploy-stage stage-ci observe-stage stage-ctl; do grep -Fq "$user" "$I"; done
grep -Fq 'make_user stage-ci' "$I" || exit 1
stage_ci_line=$(grep -nF 'make_user stage-ci' "$I" | cut -d: -f1)
bind_line=$(grep -nF 'mount --bind "$src" "$dst"' "$I" | cut -d: -f1)
[[ $stage_ci_line -gt $bind_line ]] || { echo 'stage-ci is created before loopback bind roots' >&2; exit 1; }
bash "$ROOT/deploy/apitoken-stage-ctl.test.sh"
grep -Fq 'attest|sync)' "$ROOT/deploy/stage-ctl-helper.sh"
grep -Fq 'stage-seed.sh reseed' "$ROOT/deploy/stage-ctl-helper.sh"
grep -Fq 'phase-disabled' "$ROOT/deploy/stage-ctl-helper.sh"
grep -Fq 'systemctl stop staging.slice' "$ROOT/deploy/stage-ctl-helper.sh"
if SSH_ORIGINAL_COMMAND='shell' bash "$ROOT/deploy/apitoken-observe-stage.sh" >/dev/null 2>&1; then exit 1; fi
wrapper=$TEMP_OBSERVE_STAGE_FIXTURE
sed -e 's#^HELPER=.*#HELPER=/bin/echo#' -e 's#exec sudo -n "\$HELPER"#exec "\$HELPER"#' \
  "$ROOT/deploy/apitoken-observe-stage.sh" >"$wrapper"
SSH_ORIGINAL_COMMAND='logs apitoken-staging-foundation-install.service --since 10 minutes ago' \
  bash "$wrapper" | grep -Fq '10 minutes ago' || { rm -f "$wrapper"; exit 1; }
SSH_ORIGINAL_COMMAND='store-logs apitoken-postgres-stage' bash "$wrapper" \
  | grep -Fq 'store-logs apitoken-postgres-stage' || { rm -f "$wrapper"; exit 1; }
SSH_ORIGINAL_COMMAND='proof isolation' bash "$wrapper" \
  | grep -Fq 'proof isolation' || { rm -f "$wrapper"; exit 1; }
SSH_ORIGINAL_COMMAND='state' bash "$wrapper" | grep -Fq 'state' || { rm -f "$wrapper"; exit 1; }
rm -f "$wrapper"
if SSH_ORIGINAL_COMMAND='sync' SUDO_USER=stage-ctl bash "$ROOT/deploy/stage-ctl-helper.sh" >/dev/null 2>&1; then exit 1; fi
printf 'staging-foundation.test: PASS\n'
