#!/usr/bin/env bash
# Developer-facing projection of the exact path-aware validation policy.
# This command is read-only. It reuses deploy/watchdog-lib.sh instead of maintaining a second
# path map, and it fails closed in the same way as the merge/watchdog selectors.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$ROOT/deploy/watchdog-lib.sh"

usage() {
  cat <<'EOF'
usage: deploy/change-plan.sh --base <commit> [--head <commit>] [--format text|json]

Print the committed change scope and validation plan for the unique merge base of BASE and HEAD.
The command does not fetch, mutate the worktree, run tests, or inspect uncommitted files.
EOF
}

die() {
  printf 'change-plan: %s\n' "$*" >&2
  exit 2
}

BASE=
HEAD=HEAD
FORMAT=text
while (( $# > 0 )); do
  case "$1" in
    --base)
      (( $# >= 2 )) || die '--base requires a commit'
      BASE=$2
      shift 2
      ;;
    --head)
      (( $# >= 2 )) || die '--head requires a commit'
      HEAD=$2
      shift 2
      ;;
    --format)
      (( $# >= 2 )) || die '--format requires text or json'
      FORMAT=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) die "unknown argument: $1" ;;
  esac
done

[[ -n $BASE ]] || die 'missing required --base <commit>'
[[ $FORMAT == text || $FORMAT == json ]] || die "unsupported format: $FORMAT"

resolve_commit() {
  local label=$1 ref=$2 resolved
  resolved=$(git -C "$ROOT" rev-parse --verify --end-of-options "$ref^{commit}" 2>/dev/null) \
    || die "$label does not resolve to a commit: $ref"
  [[ $resolved =~ ^[0-9a-f]{40}$ ]] || die "$label did not resolve to one full SHA: $ref"
  printf '%s\n' "$resolved"
}

BASE_SHA=$(resolve_commit base "$BASE")
HEAD_SHA=$(resolve_commit head "$HEAD")
mapfile -t MERGE_BASES < <(git -C "$ROOT" merge-base --all "$BASE_SHA" "$HEAD_SHA")
(( ${#MERGE_BASES[@]} == 1 )) \
  || die "base and head require one unique merge base; found ${#MERGE_BASES[@]}"
MERGE_BASE=${MERGE_BASES[0]}
mapfile -t PATHS < <(wd_range_files "$ROOT" "$MERGE_BASE" "$HEAD_SHA")

TYPESCRIPT=0
TYPESCRIPT_FULL=0
RUST=0
DEPLOYMENT=0
UNKNOWN=0
wd_range_has_class "$ROOT" "$MERGE_BASE" "$HEAD_SHA" wd_path_is_typescript && TYPESCRIPT=1
wd_range_has_class "$ROOT" "$MERGE_BASE" "$HEAD_SHA" wd_path_is_engine && RUST=1
if wd_range_has_class "$ROOT" "$MERGE_BASE" "$HEAD_SHA" wd_path_is_infrastructure \
    || wd_range_has_class "$ROOT" "$MERGE_BASE" "$HEAD_SHA" wd_path_is_merge_workflow; then
  DEPLOYMENT=1
fi
if wd_range_has_unknown_validation_path "$ROOT" "$MERGE_BASE" "$HEAD_SHA"; then
  UNKNOWN=1
  TYPESCRIPT=1
  TYPESCRIPT_FULL=1
  RUST=1
  DEPLOYMENT=1
elif (( TYPESCRIPT == 1 )) \
    && wd_range_requires_full_typescript_scope "$ROOT" "$MERGE_BASE" "$HEAD_SHA"; then
  TYPESCRIPT_FULL=1
fi

COMPONENTS=none
if (( TYPESCRIPT == 1 )); then
  COMPONENTS=$(wd_typescript_components_for_range "$ROOT" "$MERGE_BASE" "$HEAD_SHA" "$TYPESCRIPT_FULL")
fi

join_path_classes() {
  local path=$1 classes=() class
  wd_path_is_engine "$path" && classes+=(engine)
  wd_path_is_typescript "$path" && classes+=(typescript)
  wd_path_is_backend "$path" && classes+=(commerce)
  wd_path_is_sales "$path" && classes+=(sales)
  wd_path_is_openkeys "$path" && classes+=(openkeys)
  wd_path_is_web "$path" && classes+=(web)
  wd_path_is_admin "$path" && classes+=(admin)
  wd_path_is_devbot "$path" && classes+=(devbot)
  wd_path_is_infrastructure "$path" && classes+=(infrastructure)
  wd_path_depends_on_ubuntu_host "$path" && classes+=(ubuntu-host)
  wd_path_is_merge_workflow "$path" && classes+=(merge-workflow)
  wd_path_is_validation_neutral "$path" && classes+=(validation-neutral)
  if (( ${#classes[@]} == 0 )); then
    printf 'unknown\n'
  else
    local IFS=,
    class=${classes[*]}
    printf '%s\n' "$class"
  fi
}

if [[ $FORMAT == text ]]; then
  printf 'change-plan: base=%s\n' "$BASE_SHA"
  printf 'change-plan: head=%s\n' "$HEAD_SHA"
  printf 'change-plan: merge_base=%s\n' "$MERGE_BASE"
  printf 'change-plan: lanes static=1 typescript=%s typescript_full=%s rust=%s deployment=%s unknown=%s\n' \
    "$TYPESCRIPT" "$TYPESCRIPT_FULL" "$RUST" "$DEPLOYMENT" "$UNKNOWN"
  printf 'change-plan: typescript_components=%s\n' "$COMPONENTS"
  printf 'change-plan: committed_paths=%s\n' "${#PATHS[@]}"
  for path in "${PATHS[@]}"; do
    printf '  %s [%s]\n' "$path" "$(join_path_classes "$path")"
  done
  exit 0
fi

python3 - "$BASE_SHA" "$HEAD_SHA" "$MERGE_BASE" "$TYPESCRIPT" "$TYPESCRIPT_FULL" \
  "$RUST" "$DEPLOYMENT" "$UNKNOWN" "$COMPONENTS" "${PATHS[@]}" <<'PY'
import json
import sys
base, head, merge_base, ts, ts_full, rust, deployment, unknown, components, *paths = sys.argv[1:]
print(json.dumps({
    "format_version": 1,
    "base_sha": base,
    "head_sha": head,
    "merge_base_sha": merge_base,
    "paths": paths,
    "lanes": {
        "static": True,
        "typescript": ts == "1",
        "typescript_full": ts_full == "1",
        "rust": rust == "1",
        "deployment": deployment == "1",
        "unknown": unknown == "1",
    },
    "typescript_components": [] if components == "none" else components.split(","),
}, indent=2, sort_keys=True))
PY
