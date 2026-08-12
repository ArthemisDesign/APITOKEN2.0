#!/usr/bin/env bash
# GitHub-hosted public readiness probe. It intentionally uses no production credential: the
# public contracts are enough to detect host/network/TLS failure and independently traverse the
# Commerce, Sales, OpenKeys and engine PostgreSQL authority paths.
set -euo pipefail
export LC_ALL=C

INCIDENT_TITLE='[uptime] Production public readiness is failing'
GITHUB_WEB=${GITHUB_SERVER_URL:-https://github.com}
GH_HOST=${GITHUB_WEB#https://}
GH_HOST=${GH_HOST#http://}
GH_HOST=${GH_HOST%/}

for tool in curl jq gh; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'required tool is unavailable: %s\n' "$tool" >&2
    exit 2
  }
done

[[ ${GITHUB_REPOSITORY:-} == */* ]] || {
  printf 'GITHUB_REPOSITORY is unavailable or malformed\n' >&2
  exit 2
}
[[ -n ${GH_TOKEN:-} ]] || {
  printf 'GH_TOKEN is unavailable\n' >&2
  exit 2
}
[[ -n $GH_HOST && $GH_HOST != */* ]] || {
  printf 'GITHUB_SERVER_URL is unavailable or malformed\n' >&2
  exit 2
}

probe_dir=$(mktemp -d)
trap 'rm -rf -- "$probe_dir"' EXIT
failures=()

record_failure() {
  failures+=("$1")
  printf 'FAIL %s\n' "$1" >&2
}

fetch() {
  local name=$1 url=$2 response=$3 code
  if ! code=$(curl --silent --show-error --location --max-redirs 2 \
    --connect-timeout 5 --max-time 15 --header 'Cache-Control: no-cache' \
    --user-agent 'apitoken-external-uptime/1' --output "$response" \
    --write-out '%{http_code}' "$url"); then
    record_failure "$name: transport/TLS failure"
    return 1
  fi
  if [[ $code != 200 ]]; then
    record_failure "$name: HTTP $code"
    return 1
  fi
}

probe_json() {
  local name=$1 url=$2 filter=$3 response="$probe_dir/$4"
  fetch "$name" "$url" "$response" || return 0
  if ! jq -e "$filter" "$response" >/dev/null 2>&1; then
    record_failure "$name: response contract mismatch"
    return 0
  fi
  printf 'PASS %s\n' "$name"
}

probe_http() {
  local name=$1 url=$2 response="$probe_dir/$3"
  if fetch "$name" "$url" "$response"; then
    printf 'PASS %s\n' "$name"
  fi
}

probe_json 'Anthropic engine origin' 'https://api.apitoken.sale/health' \
  '.ok == true' anthropic.json
probe_json 'OpenAI engine origin' 'https://openai.api.apitoken.sale/health' \
  '.ok == true' openai.json
probe_json 'Gemini engine origin' 'https://gemini.api.apitoken.sale/health' \
  '.ok == true' gemini.json
probe_json 'Unified router origin' 'https://router.apitoken.sale/health' \
  '.ok == true' router.json
probe_json 'Commerce database and engine readiness' 'https://backend.apitoken.sale/v1/ready' \
  '.status == "ok" and .database == "up" and .engine == "up"' commerce.json
probe_json 'Sales database readiness' 'https://partners.apitoken.sale/v1/ready' \
  '.status == "ok" and .database == "up"' sales.json
probe_json 'OpenKeys database, contract and engine readiness' 'https://openkeys.apitoken.sale/api/ready' \
  '.status == "ready"' openkeys.json
probe_http 'Vercel status surface' 'https://apitoken.sale/status' web.html

if [[ ${UPTIME_SIMULATE_FAILURE:-} == true ]]; then
  record_failure 'Synthetic delivery drill requested by workflow_dispatch'
fi

issue_state=$(gh api --hostname "$GH_HOST" --method GET \
  "repos/$GITHUB_REPOSITORY/issues/1" \
  --jq 'select(.title == "[uptime] Production public readiness is failing") | .state' \
  2>/dev/null || true)
if [[ -n $issue_state && $issue_state != open && $issue_state != closed ]]; then
  printf 'reserved uptime incident has an invalid state: %s\n' "$issue_state" >&2
  exit 2
fi
observed_at=$(date -u +'%Y-%m-%dT%H:%M:%SZ')
run_url="$GITHUB_WEB/$GITHUB_REPOSITORY/actions/runs/${GITHUB_RUN_ID:-unknown}"

if (( ${#failures[@]} > 0 )); then
  failure_lines=$(printf -- '- %s\n' "${failures[@]}")
  if [[ -z $issue_state ]]; then
    body=$(printf 'The independent GitHub-hosted production probe failed at `%s`.\n\n%s\nWorkflow: %s\n\nThis issue remains open without per-run comments until a healthy probe closes it.' \
      "$observed_at" "$failure_lines" "$run_url")
    gh api --hostname "$GH_HOST" --method POST \
      "repos/$GITHUB_REPOSITORY/issues" -f title="$INCIDENT_TITLE" -f body="$body" >/dev/null
    printf 'Opened production uptime incident.\n'
  elif [[ $issue_state == closed ]]; then
    gh api --hostname "$GH_HOST" --method PATCH \
      "repos/$GITHUB_REPOSITORY/issues/1" -f state=open >/dev/null
    printf 'Reopened production uptime incident #1.\n'
  else
    printf 'Production uptime incident is already open: #1\n'
  fi
  if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
    {
      printf '## Production readiness failed\n\n%s\n' "$failure_lines"
      printf '\nAn incident issue is open. [Workflow run](%s).\n' "$run_url"
    } >>"$GITHUB_STEP_SUMMARY"
  fi
  exit 1
fi

if [[ $issue_state == open ]]; then
  recovery=$(printf 'All independent public readiness probes recovered at `%s`.\n\nWorkflow: %s' \
    "$observed_at" "$run_url")
  gh api --hostname "$GH_HOST" --method POST \
    "repos/$GITHUB_REPOSITORY/issues/1/comments" -f body="$recovery" >/dev/null
  gh api --hostname "$GH_HOST" --method PATCH \
    "repos/$GITHUB_REPOSITORY/issues/1" \
    -f state=closed -f state_reason=completed >/dev/null
  printf 'Closed recovered production uptime incident #1.\n'
fi

if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
  printf '## Production readiness is healthy\n\nAll eight independent public probes passed at `%s`.\n' \
    "$observed_at" >>"$GITHUB_STEP_SUMMARY"
fi
