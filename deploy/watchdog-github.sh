#!/usr/bin/env bash
set -euo pipefail

# Root-only bridge from the unprivileged watchdog to GitHub's Statuses, Checks, and Deployments APIs.
WATCHDOG_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/contour-config.sh
source "$WATCHDOG_ROOT/contour-config.sh"
CONFIG=${CONTOUR_GITHUB_CONFIG_OVERRIDE:-$CONTOUR_GITHUB_CONFIG_FILE}
[[ ${EUID:-$(id -u)} -eq 0 ]] || { echo 'must run as root' >&2; exit 1; }
[[ -f $CONFIG && ! -L $CONFIG ]] || { echo "missing $CONFIG" >&2; exit 1; }
[[ $(stat -c '%u:%a' "$CONFIG") == 0:600 ]] || { echo "$CONFIG must be root-owned mode 0600" >&2; exit 1; }
# shellcheck disable=SC1090
source "$CONFIG"
: "${GITHUB_TOKEN:?missing GITHUB_TOKEN}"
: "${GITHUB_REPOSITORY:?missing GITHUB_REPOSITORY}"
[[ $GITHUB_REPOSITORY =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || { echo 'invalid GITHUB_REPOSITORY' >&2; exit 1; }

api=https://api.github.com/repos/$GITHUB_REPOSITORY
graphql=https://api.github.com/graphql
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
    contour_require_status_context "$4" || { echo 'invalid context' >&2; exit 2; }
    ((${#5} <= 140)) || { echo 'description is too long' >&2; exit 2; }
    body=$(jq -cn --arg state "$3" --arg context "$4" --arg description "$5" --arg target_url "${6:-}" \
      '{state:$state,context:$context,description:$description} + if $target_url == "" then {} else {target_url:$target_url} end')
    github_curl -o /dev/null -X POST "$api/statuses/$2" -d "$body"
    ;;
  check-run)
    # Uploads the SHA-keyed redacted failure report. The helper never takes a caller-supplied
    # path: a compromised deploy user must not be able to POST /etc/apitoken/*.env to GitHub.
    [[ $# -eq 5 ]] || { echo 'usage: check-run SHA CONCLUSION NAME TITLE' >&2; exit 2; }
    [[ $2 =~ $sha_re ]] || { echo 'invalid SHA' >&2; exit 2; }
    [[ $3 =~ ^(failure|success|cancelled|timed_out|neutral)$ ]] || { echo 'invalid check conclusion' >&2; exit 2; }
    contour_require_status_context "$4" || { echo 'invalid check name' >&2; exit 2; }
    ((${#5} <= 1024)) || { echo 'check title is too long' >&2; exit 2; }
    failure_dir=$CONTOUR_ROOTS_STATE/failures
    summary_file=$failure_dir/$2.summary.md
    text_file=$failure_dir/$2.text
    [[ -d $failure_dir && ! -L $failure_dir ]] || { echo 'failure report directory is missing' >&2; exit 1; }
    [[ -f $summary_file && ! -L $summary_file ]] || { echo 'failure summary is missing' >&2; exit 1; }
    summary_real=$(readlink -f -- "$summary_file")
    [[ $summary_real == "$failure_dir/$2.summary.md" ]] || { echo 'failure summary escaped its directory' >&2; exit 1; }
    (( $(wc -c <"$summary_file") <= 65535 )) || { echo 'failure summary exceeds GitHub output bound' >&2; exit 1; }
    jq_args=( --arg sha "$2" --arg conclusion "$3" --arg name "$4" --arg title "$5" --rawfile summary "$summary_file" )
    if [[ -f $text_file && ! -L $text_file ]]; then
      text_real=$(readlink -f -- "$text_file")
      [[ $text_real == "$failure_dir/$2.text" ]] || { echo 'failure excerpt escaped its directory' >&2; exit 1; }
      (( $(wc -c <"$text_file") <= 65535 )) || { echo 'failure excerpt exceeds GitHub output bound' >&2; exit 1; }
      jq_args+=( --rawfile text "$text_file" )
    else
      jq_args+=( --arg text "" )
    fi
    body=$(jq -n "${jq_args[@]}" \
      '{name:$name,head_sha:$sha,status:"completed",conclusion:$conclusion,
        output:({title:$title,summary:$summary}
          + if $text == "" then {} else {text:$text} end)}')
    github_curl -X POST "$api/check-runs" -d "$body" | jq -er '.html_url'
    ;;
  deployment-create)
    [[ $# -eq 4 ]] || { echo 'usage: deployment-create SHA ENVIRONMENT DESCRIPTION' >&2; exit 2; }
    [[ $2 =~ $sha_re ]] || { echo 'invalid SHA' >&2; exit 2; }
    contour_require_deployment_environment "$3" || { echo 'invalid environment' >&2; exit 2; }
    if [[ $CONTOUR_KIND == production ]]; then
      [[ $3 == production-* || $3 == candidate-validation ]] \
        || { echo 'production contour rejected deployment environment' >&2; exit 2; }
    else
      [[ $3 == staging-* ]] || { echo 'stage contour rejected deployment environment' >&2; exit 2; }
    fi
    body=$(jq -cn --arg ref "$2" --arg environment "$3" --arg description "$4" \
      '{ref:$ref,environment:$environment,description:$description,auto_merge:false,required_contexts:[],transient_environment:($environment | startswith("staging-")),production_environment:($environment | startswith("production-"))}')
    github_curl -X POST "$api/deployments" -d "$body" | jq -er '.id'
    ;;
  deployment-status)
    [[ $# -ge 6 && $# -le 7 ]] || { echo 'usage: deployment-status ID STATE DESCRIPTION ENVIRONMENT ENVIRONMENT_URL [LOG_URL]' >&2; exit 2; }
    [[ $2 =~ ^[1-9][0-9]*$ ]] || { echo 'invalid deployment id' >&2; exit 2; }
    [[ $3 =~ ^(error|failure|inactive|in_progress|queued|pending|success)$ ]] || { echo 'invalid deployment state' >&2; exit 2; }
    ((${#4} <= 140)) || { echo 'description is too long' >&2; exit 2; }
    contour_require_deployment_environment "$5" || { echo 'invalid environment' >&2; exit 2; }
    body=$(jq -cn --arg state "$3" --arg description "$4" --arg environment "$5" \
      --arg environment_url "$6" --arg log_url "${7:-}" \
      '{state:$state,description:$description,environment:$environment,
        auto_inactive:($environment != "candidate-validation")}
       + if $environment_url == "" then {} else {environment_url:$environment_url} end
       + if $log_url == "" then {} else {log_url:$log_url} end')
    github_curl -o /dev/null -X POST "$api/deployments/$2/statuses" -d "$body"
    ;;
  validation-next)
    [[ $# -ge 1 && $# -le 2 ]] || { echo 'usage: validation-next [1|2]' >&2; exit 2; }
    limit=${2:-1}
    [[ $limit =~ ^[1-2]$ ]] || { echo 'validation limit must be 1 or 2' >&2; exit 2; }
    owner=${GITHUB_REPOSITORY%%/*}
    repository=${GITHUB_REPOSITORY#*/}
    body=$(jq -cn --arg owner "$owner" --arg repository "$repository" \
      --arg environment candidate-validation \
      '{
        query:"query($owner:String!,$repository:String!,$environment:String!){repository(owner:$owner,name:$repository){deployments(last:100,environments:[$environment],orderBy:{field:CREATED_AT,direction:ASC}){nodes{databaseId commitOid state latestStatus{state}}}}}",
        variables:{owner:$owner,repository:$repository,environment:$environment}
      }')
    deployments=$(github_curl -X POST "$graphql" -d "$body")
    jq -e '
      (.errors == null)
      and (.data.repository.deployments.nodes | type == "array")
      and all(.data.repository.deployments.nodes[];
        (.databaseId | type == "number")
        and (.commitOid | type == "string")
        and ((.state == null) or
          (.state == "ABANDONED") or (.state == "ACTIVE") or (.state == "DESTROYED") or
          (.state == "ERROR") or (.state == "FAILURE") or (.state == "INACTIVE") or
          (.state == "IN_PROGRESS") or (.state == "PENDING") or (.state == "QUEUED") or
          (.state == "SUCCESS") or (.state == "WAITING"))
        and ((.latestStatus == null) or
          (.latestStatus.state == "ERROR") or (.latestStatus.state == "FAILURE") or
          (.latestStatus.state == "INACTIVE") or (.latestStatus.state == "IN_PROGRESS") or
          (.latestStatus.state == "PENDING") or (.latestStatus.state == "QUEUED") or
          (.latestStatus.state == "SUCCESS") or (.latestStatus.state == "WAITING")))
    ' >/dev/null <<<"$deployments" || { echo 'invalid deployments response' >&2; exit 1; }
    entries=$(jq -r --argjson limit "$limit" '
      .data.repository.deployments.nodes
      | map({
          id: .databaseId,
          sha: .commitOid,
          state: (.latestStatus.state // .state // "QUEUED")
        })
      | map(select(
          (.state == "QUEUED") or (.state == "PENDING") or
          (.state == "IN_PROGRESS") or (.state == "WAITING")))
      | reduce .[] as $entry ([];
          if (map(.sha) | index($entry.sha)) == null
          then . + [$entry]
          else .
          end)
      | .[:$limit][]
      | [.id, .sha]
      | @tsv
    ' <<<"$deployments")
    [[ -n $entries ]] || exit 0
    while IFS=$'\t' read -r deployment_id deployment_sha; do
      [[ $deployment_id =~ ^[1-9][0-9]*$ && $deployment_sha =~ $sha_re ]] \
        || { echo 'invalid candidate validation deployment' >&2; exit 1; }
      printf '%s\t%s\n' "$deployment_id" "$deployment_sha"
    done <<<"$entries"
    ;;
  *)
    echo 'usage: watchdog-github.sh commit-status|check-run|deployment-create|deployment-status|validation-next ...' >&2
    exit 2
    ;;
esac
