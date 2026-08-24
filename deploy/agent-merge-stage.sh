#!/usr/bin/env bash
# Serial landing of the current branch onto stage.
#
# Production admission is a later step: this script never writes master. It takes a stage-only
# lock, freezes one unpromoted SHA, runs the ordinary exact-SHA gates through
# agent-merge.sh --validate-only, then force-with-lease-updates origin/stage and waits for
# informational deploy/stage. Promotion still needs operator attestation and agent-merge.sh.
#
# --fix-red may replace a frozen unpromoted stage SHA only when origin/master deploy/watchdog is
# RED. It does not skip attestation. --hotfix is a master-only override and is refused here.
#
# Usage:  deploy/agent-merge-stage.sh [--allow-primary-tree] [--dry-run] [--fix-red]
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
LOCK=${AGENT_MERGE_STAGE_LOCK:-$HOME/.claude-api/stage-merge.lock.d}
mkdir -p "$(dirname -- "$LOCK")"
if ! mkdir "$LOCK" 2>/dev/null; then
  owner=$(cat -- "$LOCK/pid" 2>/dev/null || printf '')
  if [[ -n $owner ]] && ! kill -0 "$owner" 2>/dev/null; then
    echo "agent-merge-stage: WARNING: breaking a stale stage lock left by dead pid $owner" >&2
    rm -rf -- "$LOCK"
    mkdir "$LOCK"
  else
    echo 'agent-merge-stage: another serial stage batch owns the lock' >&2
    exit 1
  fi
fi
printf '%s\n' "$$" >"$LOCK/pid"
trap 'rm -rf -- "$LOCK"' EXIT

FIX_RED=0
for argument in "$@"; do
  case "$argument" in
    --fix-red) FIX_RED=1 ;;
    --hotfix)
      echo 'agent-merge-stage: --hotfix is a master-only override; it does not move stage' >&2
      exit 1
      ;;
  esac
done

# Prints the GitHub commit-status state for $1 (sha) $2 (context). Tests inject
# AGENT_MERGE_STAGE_STATUS_CMD, which is eval'd in this function so it can read $1 and $2.
ams_status() {
  if [[ -n ${AGENT_MERGE_STAGE_STATUS_CMD:-} ]]; then
    eval "$AGENT_MERGE_STAGE_STATUS_CMD"
    return
  fi
  local sha=$1 context=$2 token state
  token=$(GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true SSH_ASKPASS=true git -C "$ROOT" credential fill 2>/dev/null <<'EOF' | sed -n 's/^password=//p' | head -n1
protocol=https
host=github.com

EOF
)
  [[ -n $token ]] || { printf 'unknown'; return; }
  state=$(curl -fsSL --max-time 30 -K - \
    "https://api.github.com/repos/3xcalibur-tech/Claude_API/commits/$sha/status" <<EOF 2>/dev/null \
    | node -e 'let s="";process.stdin.on("data",c=>s+=c);process.stdin.on("end",()=>{try{const d=JSON.parse(s);const ctx=process.argv[1];const x=(d.statuses||[]).find(v=>v.context===ctx);process.stdout.write(x?.state||"pending")}catch{process.stdout.write("unknown")}})' -- "$context"
header = "Authorization: Bearer $token"
header = "Accept: application/vnd.github+json"
EOF
  )
  printf '%s' "${state:-unknown}"
}

git -C "$ROOT" fetch origin master stage 2>/dev/null || git -C "$ROOT" fetch origin master
master=$(git -C "$ROOT" rev-parse origin/master)
stage=$(git -C "$ROOT" rev-parse --verify refs/remotes/origin/stage 2>/dev/null || true)
if [[ -n $stage && $stage != "$master" ]]; then
  if (( FIX_RED )); then
    watchdog=$(ams_status "$master" deploy/watchdog)
    case "$watchdog" in
      failure|error)
        echo "agent-merge-stage: WARNING: replacing frozen unpromoted SHA $stage because --fix-red was given and origin/master deploy/watchdog is RED" >&2
        ;;
      *)
        echo "agent-merge-stage: --fix-red requires origin/master deploy/watchdog to be RED (got ${watchdog:-unknown}); stage stays frozen at $stage" >&2
        exit 1
        ;;
    esac
  else
    echo "agent-merge-stage: stage is frozen at unpromoted SHA $stage" >&2
    exit 1
  fi
fi
export AGENT_MERGE_TARGET=master
export AGENT_MERGE_LOCK=${LOCK}.validation
export AGENT_MERGE_REQUIRED_CONTEXT=deploy/watchdog
# Production master remains the trusted-validation baseline. A separate staging-candidate-validation
# environment is not used here: stage has no independent green production-equivalent parent.
export AGENT_MERGE_VALIDATION_ENVIRONMENT=candidate-validation
"$ROOT/deploy/agent-merge.sh" --validate-only "$@"
candidate=$(git -C "$ROOT" rev-parse HEAD)
lease=${stage:-0000000000000000000000000000000000000000}
git -C "$ROOT" push --force-with-lease="refs/heads/stage:$lease" origin HEAD:refs/heads/stage
for waited in $(seq 0 5 1200); do
  state=$(ams_status "$candidate" deploy/stage)
  case "$state" in
    success) printf 'agent-merge-stage: deploy/stage is GREEN for %s\n' "$candidate"; exit 0 ;;
    failure|error) echo "agent-merge-stage: deploy/stage is RED for $candidate" >&2; exit 1 ;;
  esac
  printf 'agent-merge-stage: waiting for deploy/stage on %s (%ss)\n' "$candidate" "$waited"
  sleep 5
done
echo "agent-merge-stage: deploy/stage did not settle for $candidate" >&2
exit 1
