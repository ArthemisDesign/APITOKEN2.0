#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "$0")/.." && pwd)
TEMP=$(mktemp -d); trap 'rm -rf -- "$TEMP"' EXIT
R=$ROOT/deploy/stage-unit-renderer.py
C=$ROOT/deploy/contour-stage.json
W=$ROOT/deploy/stage-unit-whitelist.json
for unit in anthropic openai gemini kimi router api; do
  python3 "$R" --contour "$C" --whitelist "$W" --unit "$unit" >"$TEMP/$unit.service"
  grep -Fxq 'User=deploy-stage' "$TEMP/$unit.service"
  grep -Fxq 'Slice=staging.slice' "$TEMP/$unit.service"
  grep -Fxq 'NetworkNamespacePath=/run/netns/apitoken-stage' "$TEMP/$unit.service"
  grep -Fq '/etc/apitoken-staging/' "$TEMP/$unit.service"
  ! grep -Eq '/etc/apitoken/|/srv/claude-api/releases|/opt/apitoken/releases|/var/run/docker.sock|Privileged=' "$TEMP/$unit.service"
done
if python3 "$R" --contour "$C" --whitelist "$W" --unit unknown >/dev/null 2>&1; then
  echo 'stage renderer accepted unknown unit' >&2; exit 1
fi
cp "$W" "$TEMP/bad.json"
python3 - "$TEMP/bad.json" <<'PY'
import json,sys
p=sys.argv[1]; v=json.load(open(p)); v['units']['api']['unit']='apitoken-api@.service'; json.dump(v,open(p,'w'))
PY
if python3 "$R" --contour "$C" --whitelist "$TEMP/bad.json" --unit api >/dev/null 2>&1; then
  echo 'stage renderer accepted production unit name' >&2; exit 1
fi
printf 'stage-unit-renderer.test: PASS\n'
