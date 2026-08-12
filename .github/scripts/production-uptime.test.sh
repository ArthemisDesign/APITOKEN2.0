#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
SCRIPT=$ROOT/.github/scripts/production-uptime.sh
WORKFLOW=$ROOT/.github/workflows/production-uptime.yml
readonly ROOT SCRIPT WORKFLOW
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT
readonly TEMP
mkdir -p "$TEMP/bin"

cat >"$TEMP/bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
output=
url=
while (( $# > 0 )); do
  case "$1" in
    --output) output=$2; shift 2 ;;
    http*) url=$1; shift ;;
    *) shift ;;
  esac
done
if [[ -n ${MOCK_BAD_URL:-} && $url == *"$MOCK_BAD_URL"* ]]; then
  printf '{"status":"unavailable"}\n' >"$output"
  printf '503'
  exit 0
fi
case "$url" in
  *backend*) printf '{"status":"ok","database":"up","engine":"up"}\n' >"$output" ;;
  *partners*) printf '{"status":"ok","database":"up"}\n' >"$output" ;;
  *openkeys*) printf '{"status":"ready"}\n' >"$output" ;;
  *apitoken.sale/status) printf '<html>ok</html>\n' >"$output" ;;
  *) printf '{"ok":true}\n' >"$output" ;;
esac
printf '200'
MOCK

cat >"$TEMP/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%q ' "$@" >>"$GH_LOG"
printf '\n' >>"$GH_LOG"
if [[ " $* " == *' --method GET '* && " $* " == *' repos/test/repository/issues/1 '* ]]; then
  if [[ -n ${MOCK_OPEN_ISSUE:-} ]]; then
    printf '%s\n' "$MOCK_OPEN_ISSUE"
  fi
fi
MOCK
chmod +x "$TEMP/bin/curl" "$TEMP/bin/gh" "$SCRIPT"

grep -Fq 'permissions:' "$WORKFLOW"
grep -Fq 'contents: read' "$WORKFLOW"
grep -Fq 'issues: write' "$WORKFLOW"
grep -Fq 'production-uptime.sh?ref=${GITHUB_WORKFLOW_SHA}' "$WORKFLOW"
! grep -Fq 'actions/checkout@' "$WORKFLOW" || {
  printf 'uptime workflow executes a mutable checkout action\n' >&2
  exit 1
}

run_probe() {
  PATH="$TEMP/bin:$PATH" GH_LOG="$TEMP/gh.log" GH_TOKEN=test-token \
    GITHUB_REPOSITORY=test/repository \
    GITHUB_SERVER_URL=https://github.com GITHUB_RUN_ID=42 \
    GITHUB_STEP_SUMMARY="$TEMP/summary" "$SCRIPT"
}

: >"$TEMP/gh.log"
: >"$TEMP/summary"
run_probe
! grep -Fq -- '--method POST' "$TEMP/gh.log" || {
  printf 'healthy probe unexpectedly mutated an issue\n' >&2
  exit 1
}

: >"$TEMP/gh.log"
: >"$TEMP/summary"
if UPTIME_SIMULATE_FAILURE=true run_probe; then
  printf 'synthetic failure unexpectedly passed\n' >&2
  exit 1
fi
grep -Fq -- '--method POST' "$TEMP/gh.log"
grep -Fq 'repos/test/repository/issues' "$TEMP/gh.log"

: >"$TEMP/gh.log"
: >"$TEMP/summary"
if MOCK_BAD_URL=backend.apitoken.sale run_probe; then
  printf 'HTTP failure unexpectedly passed\n' >&2
  exit 1
fi
grep -Fq -- '- Commerce database and engine readiness: HTTP 503' "$TEMP/summary"

: >"$TEMP/gh.log"
: >"$TEMP/summary"
MOCK_OPEN_ISSUE=77 run_probe
grep -Fq 'repos/test/repository/issues/77/comments' "$TEMP/gh.log"
grep -Fq -- '--method PATCH' "$TEMP/gh.log"
grep -Fq 'repos/test/repository/issues/77' "$TEMP/gh.log"

: >"$TEMP/gh.log"
: >"$TEMP/summary"
if MOCK_OPEN_ISSUE=77 UPTIME_SIMULATE_FAILURE=true run_probe; then
  printf 'existing-incident failure unexpectedly passed\n' >&2
  exit 1
fi
[[ $(grep -Fc -- '--method POST' "$TEMP/gh.log") -eq 0 ]] || {
  printf 'repeat failure mutated the existing incident\n' >&2
  exit 1
}

printf 'production external uptime tests passed\n'
