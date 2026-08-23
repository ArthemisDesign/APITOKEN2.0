#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
for file in agent-merge-stage.sh stage-watchdog.sh stage-watchdog-validate.sh stage-source-fetch.sh \
  watchdog-github-stage.sh stage-sync.sh promotion-attest.sh; do
  bash -n "$ROOT/deploy/$file"
done
grep -Fq 'AGENT_MERGE_VALIDATION_ENVIRONMENT=candidate-validation' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq '"$ROOT/deploy/agent-merge.sh" --validate-only' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'stage is frozen at unpromoted SHA' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'stage-merge.lock.d' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'mkdir "$LOCK"' "$ROOT/deploy/agent-merge-stage.sh"
! grep -Fq 'flock -n 9' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'rev-parse --verify refs/remotes/origin/stage' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'HEAD:refs/heads/stage' "$ROOT/deploy/agent-merge-stage.sh"
grep -Fq 'VALIDATE_ONLY=0' "$ROOT/deploy/agent-merge.sh"
grep -Fq 'validate-only: exact SHA' "$ROOT/deploy/agent-merge.sh"
grep -Fq 'stage/direct-push-dry-run' "$ROOT/deploy/watchdog-github-stage.sh"
! grep -Fq 'deploy/watchdog|' "$ROOT/deploy/watchdog-github-stage.sh"
! grep -Fq 'deploy/tests|' "$ROOT/deploy/watchdog-github-stage.sh"
grep -Fq 'SUDO_USER:-} == deploy-stage' "$ROOT/deploy/watchdog-github-stage.sh"
grep -Fq 'candidate.sha' "$ROOT/deploy/watchdog-github-stage.sh"
grep -Fq 'deploy-stage ALL=(root) NOPASSWD: APITOKEN_STAGE_REPORT' "$ROOT/deploy/sudoers.d/96-apitoken-stage"
grep -Fq 'host-global candidate path rejected' "$ROOT/deploy/stage-watchdog-validate.sh"
grep -Fq 'runuser -u deploy -- git' "$ROOT/deploy/stage-source-fetch.sh"
grep -Fq 'safe.directory="$SOURCE"' "$ROOT/deploy/stage-source-fetch.sh"
grep -Fq 'safe.directory="$SOURCE/.git"' "$ROOT/deploy/stage-source-fetch.sh"
grep -Fq 'clone --no-hardlinks --no-checkout "$SOURCE/.git"' "$ROOT/deploy/stage-source-fetch.sh"
grep -Fq 'safe.directory="$TARGET"' "$ROOT/deploy/stage-source-fetch.sh"
grep -Fq "refs/heads/stage:refs/remotes/origin/stage" "$ROOT/deploy/stage-source-fetch.sh"
grep -Fxq 'NoNewPrivileges=yes' "$ROOT/systemd/apitoken-stage-source-fetch.service"
grep -Fxq 'OnUnitInactiveSec=15s' "$ROOT/systemd/apitoken-stage-source-fetch.timer"
grep -Fq 'phase-disabled' "$ROOT/deploy/stage-sync.sh"
grep -Fq 'phase-disabled' "$ROOT/deploy/promotion-attest.sh"
grep -Fxq 'User=deploy-stage' "$ROOT/systemd/apitoken-stage-watchdog.service"
grep -Fxq 'Slice=staging.slice' "$ROOT/systemd/apitoken-stage-watchdog.service"
grep -Fxq 'NoNewPrivileges=no' "$ROOT/systemd/apitoken-stage-watchdog.service"
grep -Fxq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$ROOT/systemd/apitoken-stage-watchdog.service"
grep -Fxq 'OnUnitInactiveSec=15s' "$ROOT/systemd/apitoken-stage-watchdog.timer"
python3 "$ROOT/deploy/contour-config.py" --schema "$ROOT/deploy/contour-config.schema.json" \
  --config "$ROOT/deploy/contour-stage.json" --against "$ROOT/deploy/contour-production.json" >/dev/null
printf 'stage-watchdog.test: PASS\n'
