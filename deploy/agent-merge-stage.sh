#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
LOCK=${AGENT_MERGE_STAGE_LOCK:-$HOME/.claude-api/stage-merge.lock}
mkdir -p "$(dirname -- "$LOCK")"
if ! mkdir "$LOCK" 2>/dev/null; then
  echo 'agent-merge-stage: another serial stage batch owns the lock' >&2
  exit 1
fi
trap 'rmdir "$LOCK" 2>/dev/null || true' EXIT
git -C "$ROOT" fetch origin master stage 2>/dev/null || git -C "$ROOT" fetch origin master
master=$(git -C "$ROOT" rev-parse origin/master)
stage=$(git -C "$ROOT" rev-parse origin/stage 2>/dev/null || true)
[[ -z $stage || $stage == "$master" ]] \
  || { echo "agent-merge-stage: stage is frozen at unpromoted SHA $stage" >&2; exit 1; }
export AGENT_MERGE_TARGET=master
export AGENT_MERGE_LOCK=${LOCK}.validation
export AGENT_MERGE_REQUIRED_CONTEXT=deploy/watchdog
export AGENT_MERGE_VALIDATION_ENVIRONMENT=staging-candidate-validation
"$ROOT/deploy/agent-merge.sh" --validate-only "$@"
candidate=$(git -C "$ROOT" rev-parse HEAD)
lease=${stage:-0000000000000000000000000000000000000000}
git -C "$ROOT" push --force-with-lease="refs/heads/stage:$lease" origin HEAD:refs/heads/stage
for waited in $(seq 0 5 1200); do
  token=$(GIT_TERMINAL_PROMPT=0 GIT_ASKPASS=true SSH_ASKPASS=true git -C "$ROOT" credential fill 2>/dev/null <<'EOF' | sed -n 's/^password=//p' | head -n1
protocol=https
host=github.com

EOF
)
  state=$(curl -fsSL --max-time 30 -K - \
    "https://api.github.com/repos/3xcalibur-tech/Claude_API/commits/$candidate/status" <<EOF 2>/dev/null \
    | node -e 'let s="";process.stdin.on("data",c=>s+=c);process.stdin.on("end",()=>{try{const d=JSON.parse(s);const x=(d.statuses||[]).find(v=>v.context==="deploy/stage");process.stdout.write(x?.state||"pending")}catch{process.stdout.write("unknown")}})'
header = "Authorization: Bearer $token"
header = "Accept: application/vnd.github+json"
EOF
  )
  case "$state" in
    success) printf 'agent-merge-stage: deploy/stage is GREEN for %s\n' "$candidate"; exit 0 ;;
    failure|error) echo "agent-merge-stage: deploy/stage is RED for $candidate" >&2; exit 1 ;;
  esac
  printf 'agent-merge-stage: waiting for deploy/stage on %s (%ss)\n' "$candidate" "$waited"
  sleep 5
done
echo "agent-merge-stage: deploy/stage did not settle for $candidate" >&2
exit 1
