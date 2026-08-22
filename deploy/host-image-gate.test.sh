#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

fail() { printf 'host-image-gate.test: %s\n' "$*" >&2; exit 1; }

[[ -f $ROOT/deploy/host-image/Dockerfile ]] || fail 'Dockerfile is missing'
[[ -f $ROOT/deploy/host-image/prove-installers.sh ]] || fail 'prove-installers.sh is missing'
[[ -x $ROOT/deploy/host-image-gate.sh ]] || fail 'host-image-gate.sh must be executable'
[[ -x $ROOT/deploy/host-image/prove-installers.sh ]] \
  || fail 'prove-installers.sh must be executable'
bash -n "$ROOT/deploy/host-image-gate.sh" || fail 'host-image-gate.sh does not parse'
bash -n "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh does not parse'

grep -Fq 'command -v docker' "$ROOT/deploy/host-image-gate.sh" \
  || fail 'host-image-gate.sh does not require docker'
grep -Fq 'docker is required for Ubuntu host-installer proofs' \
  "$ROOT/deploy/host-image-gate.sh" \
  || fail 'host-image-gate.sh lost its fail-closed docker diagnostic'
grep -Fq -- '--privileged' "$ROOT/deploy/host-image-gate.sh" \
  || fail 'host-image-gate.sh must run the Ubuntu proofs privileged'
grep -Fq '"$ROOT:/src:ro"' "$ROOT/deploy/host-image-gate.sh" \
  || fail 'host-image-gate.sh must mount the worktree read-only at /src'
grep -Fq 'run_as_ci bash "$candidate/deploy/host-image-gate.test.sh"' \
  "$ROOT/deploy/watchdog.sh" \
  || fail 'the production gate does not run the host-image wiring suite'
if grep -Fq 'deploy/host-image-gate.sh"' "$ROOT/deploy/watchdog.sh"; then
  fail 'the production host must not run the privileged Ubuntu host-image container'
fi

TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
mkdir -p "$TEMP/bin"
ln -s /bin/bash "$TEMP/bin/bash"
ln -s /usr/bin/python3 "$TEMP/bin/python3" 2>/dev/null \
  || ln -s "$(command -v python3)" "$TEMP/bin/python3"
status=0
PATH="$TEMP/bin" HOME="$TEMP" bash "$ROOT/deploy/host-image-gate.sh" \
  >"$TEMP/no-docker.out" 2>&1 || status=$?
(( status != 0 )) || fail 'host-image-gate.sh succeeded without docker'
grep -Fq 'docker is required for Ubuntu host-installer proofs' "$TEMP/no-docker.out" \
  || fail "missing docker did not fail closed: $(cat "$TEMP/no-docker.out")"

wd_path_depends_on_ubuntu_host systemd/apitoken-observe-install.service \
  || fail 'observe unit is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/install-observe.sh \
  || fail 'install-observe.sh is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/install-sudoers.sh \
  || fail 'install-sudoers.sh is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/sudoers.d/95-apitoken-deploy \
  || fail 'sudoers policy is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/host-image-gate.sh \
  || fail 'the host-image runner is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/host-image/prove-installers.sh \
  || fail 'prove-installers.sh is not an Ubuntu-host path'
wd_path_depends_on_ubuntu_host deploy/Caddyfile \
  || fail 'Caddyfile is not an Ubuntu-host path'
if wd_path_depends_on_ubuntu_host deploy/agent-worktree.sh; then
  fail 'macOS worktree manager must not select the Ubuntu host-image'
fi
if wd_path_depends_on_ubuntu_host deploy/DELETE_WORKTREE.sh; then
  fail 'macOS DELETE_WORKTREE must not select the Ubuntu host-image'
fi
if wd_path_depends_on_ubuntu_host AGENTS.md; then
  fail 'merge-workflow docs must not select the Ubuntu host-image'
fi

if wd_path_requires_infrastructure_install deploy/host-image-gate.sh; then
  fail 'host-image-gate.sh must stay local-only'
fi
if wd_path_requires_infrastructure_install deploy/host-image/Dockerfile; then
  fail 'host-image Dockerfile must stay local-only'
fi
if wd_path_requires_infrastructure_install deploy/host-image/prove-installers.sh; then
  fail 'prove-installers.sh must stay local-only'
fi
wd_path_is_infrastructure deploy/host-image-gate.sh \
  || fail 'host-image-gate.sh must stay in the deployment validation lane'

grep -Fq 'wd_path_depends_on_ubuntu_host' "$ROOT/deploy/agent-merge.sh" \
  || fail 'the merge deployment lane does not consult the Ubuntu-host classifier'
grep -Fq 'deploy/host-image-gate.sh' "$ROOT/deploy/agent-merge.sh" \
  || fail 'the merge deployment lane does not run the host-image gate'
grep -Fq 'am_gate_deployment "$base" "$target"' "$ROOT/deploy/agent-merge.sh" \
  || fail 'am_gate_deployment no longer receives the exact SHA range'

grep -Fq 'groupadd --system observe' "$ROOT/deploy/host-image/prove-installers.sh" \
  || grep -Fq 'install-observe.sh' "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh does not run the observe installer'
grep -Fq 'read-only passwd/home namespace' "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh lost the 2026-08-22 read-only seed'
grep -Fq 'useradd --system' "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh does not create apitoken-ci'
grep -Fq 'install-sudoers.sh' "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh does not run install-sudoers.sh'
grep -Fq 'install-tmpfiles.sh' "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'prove-installers.sh does not run install-tmpfiles.sh'
grep -Fq 'INSTALLER_SKIP[install-monitoring.sh]' \
  "$ROOT/deploy/host-image/prove-installers.sh" \
  || fail 'install-monitoring.sh skip is no longer documented in the proof script'

printf 'host-image-gate.test: passed\n'
