#!/usr/bin/env bash
set -euo pipefail

# Root-only bridge from the unprivileged watchdog to GitHub's free Statuses and Deployments APIs.
CONFIG=/etc/apitoken/github-watchdog.env
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'must run as root' >&2; exit 1; }
[[ -f $CONFIG && ! -L $CONFIG ]] || { echo "missing $CONFIG" >&2; exit 1; }
[[ $(stat -c '%u:%a' "$CONFIG") == 0:600 ]] || { echo "$CONFIG must be root-owned mode 0600" >&2; exit 1; }
# shellcheck disable=SC1090
source "$CONFIG"
: "${GITHUB_TOKEN:?missing GITHUB_TOKEN}"
: "${GITHUB_REPOSITORY:?missing GITHUB_REPOSITORY}"
[[ $GITHUB_REPOSITORY =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo 'invalid GITHUB_REPOSITORY' >&2; exit 1; }

api=https://api.github.com/repos/$GITHUB_REPOSITORY
sha_re='^[0-9a-f]{40}$'

# Read the authorization header from stdin so the token never appears in argv or service logs.
github_curl() {
  curl -fsS -K - "$@" <<EOF
header = "Authorization: Bearer $GITHUB_TOKEN"
header = "Accept: application/vnd.github+json"
header = "X-GitHub-Api-Version: 2022-11-28"
EOF
}

case "${1:-}" in
  commit-status)
    [[ $# -ge 5 && $# -le 6 ]] || { echo 'usage: commit-status SHA STATE CONTEXT DESCRIPTION [URL]' >&2; exit 2; }
    [[ $2 =~ $sha_re ]] || { echo 'invalid SHA' >&2; exit 2; }
    [[ $3 =~ ^(error|failure|pending|success)$ ]] || { echo 'invalid commit state' >&2; exit 2; }
    [[ $4 =~ ^deploy/[a-z-]+$ ]] || { echo 'invalid context' >&2; exit 2; }
    ((${#5} <= 140)) || { echo 'description is too long' >&2; exit 2; }
    body=$(jq -cn --arg state "$3" --arg context "$4" --arg description "$5" --arg target_url "${6:-}" \
      '{state:$state,context:$context,description:$description} + if $target_url == "" then {} else {target_url:$target_url} end')
    github_curl -o /dev/null -X POST "$api/statuses/$2" -d "$body"
    ;;
  deployment-create)
    [[ $# -eq 4 ]] || { echo 'usage: deployment-create SHA ENVIRONMENT DESCRIPTION' >&2; exit 2; }
    [[ $2 =~ $sha_re ]] || { echo 'invalid SHA' >&2; exit 2; }
    [[ $3 =~ ^production-(database|engine|backend|sales|openkeys)$ ]] || { echo 'invalid environment' >&2; exit 2; }
    body=$(jq -cn --arg ref "$2" --arg environment "$3" --arg description "$4" \
      '{ref:$ref,environment:$environment,description:$description,auto_merge:false,required_contexts:[],transient_environment:false,production_environment:true}')
    github_curl -X POST "$api/deployments" -d "$body" | jq -er '.id'
    ;;
  deployment-status)
    [[ $# -ge 6 && $# -le 7 ]] || { echo 'usage: deployment-status ID STATE DESCRIPTION ENVIRONMENT ENVIRONMENT_URL [LOG_URL]' >&2; exit 2; }
    [[ $2 =~ ^[1-9][0-9]*$ ]] || { echo 'invalid deployment id' >&2; exit 2; }
    [[ $3 =~ ^(error|failure|inactive|in_progress|queued|pending|success)$ ]] || { echo 'invalid deployment state' >&2; exit 2; }
    [[ $5 =~ ^(candidate-validation|production-(database|engine|backend|sales|openkeys))$ ]] || { echo 'invalid environment' >&2; exit 2; }
    body=$(jq -cn --arg state "$3" --arg description "$4" --arg environment "$5" \
      --arg environment_url "$6" --arg log_url "${7:-}" \
      '{state:$state,description:$description,environment:$environment,auto_inactive:true}
       + if $environment_url == "" then {} else {environment_url:$environment_url} end
       + if $log_url == "" then {} else {log_url:$log_url} end')
    github_curl -o /dev/null -X POST "$api/deployments/$2/statuses" -d "$body"
    ;;
  validation-next)
    [[ $# -eq 1 ]] || { echo 'usage: validation-next' >&2; exit 2; }
    deployments=$(github_curl "$api/deployments?environment=candidate-validation&per_page=100")
    jq -e '
      type == "array"
      and all(.[];
        (.id | type == "number")
        and (.sha | type == "string")
        and (.environment == "candidate-validation"))
    ' >/dev/null <<<"$deployments" || { echo 'invalid deployments response' >&2; exit 1; }
    entries=$(jq -r 'reverse[] | [.id, .sha] | @tsv' <<<"$deployments")
    [[ -n $entries ]] || exit 0
    while IFS=$'\t' read -r deployment_id deployment_sha; do
      [[ $deployment_id =~ ^[1-9][0-9]*$ && $deployment_sha =~ $sha_re ]] \
        || { echo 'invalid candidate validation deployment' >&2; exit 1; }
      statuses=$(github_curl "$api/deployments/$deployment_id/statuses?per_page=1")
      jq -e 'type == "array"' >/dev/null <<<"$statuses" \
        || { echo 'invalid deployment statuses response' >&2; exit 1; }
      state=$(jq -r 'if length == 0 then "queued" else (.[0].state // "") end' <<<"$statuses")
      case "$state" in
        queued|pending|in_progress)
          printf '%s\t%s\n' "$deployment_id" "$deployment_sha"
          exit 0
          ;;
        error|failure|inactive|success) ;;
        *) echo 'invalid deployment status state' >&2; exit 1 ;;
      esac
    done <<<"$entries"
    ;;
  *)
    echo 'usage: watchdog-github.sh commit-status|deployment-create|deployment-status|validation-next ...' >&2
    exit 2
    ;;
esac
