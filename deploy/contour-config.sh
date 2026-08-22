#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
SCHEMA=${CONTOUR_SCHEMA_FILE:-$SCRIPT_DIR/contour-config.schema.json}
CONFIG=${CONTOUR_CONFIG_FILE:-$SCRIPT_DIR/contour-production.json}
LOADER=${CONTOUR_CONFIG_LOADER:-$SCRIPT_DIR/contour-config.py}

[[ -f $LOADER && ! -L $LOADER ]] || { printf 'contour loader is missing or unsafe: %s\n' "$LOADER" >&2; return 1 2>/dev/null || exit 1; }
[[ -f $SCHEMA && ! -L $SCHEMA ]] || { printf 'contour schema is missing or unsafe: %s\n' "$SCHEMA" >&2; return 1 2>/dev/null || exit 1; }
[[ -f $CONFIG && ! -L $CONFIG ]] || { printf 'contour config is missing or unsafe: %s\n' "$CONFIG" >&2; return 1 2>/dev/null || exit 1; }

contour_output=$(python3 "$LOADER" --schema "$SCHEMA" --config "$CONFIG" --emit shell) \
  || { printf 'production contour validation failed\n' >&2; return 1 2>/dev/null || exit 1; }

while IFS='=' read -r contour_name contour_value; do
  [[ $contour_name =~ ^CONTOUR_[A-Z0-9_]+$ ]] \
    || { printf 'contour loader returned an invalid key: %s\n' "$contour_name" >&2; return 1 2>/dev/null || exit 1; }
  if [[ -v $contour_name ]]; then
    [[ ${!contour_name} == "$contour_value" ]] \
      || { printf 'contour variable already has a different value: %s\n' "$contour_name" >&2; return 1 2>/dev/null || exit 1; }
  else
    printf -v "$contour_name" '%s' "$contour_value"
    readonly "$contour_name"
  fi
done <<<"$contour_output"
unset contour_output contour_name contour_value

contour_list_has() {
  [[ $# -eq 2 ]] || return 2
  local list=$1 needle=$2 item IFS=,
  for item in $list; do
    [[ $item == "$needle" ]] && return 0
  done
  return 1
}

contour_require_status_context() {
  contour_list_has "$CONTOUR_GITHUB_STATUS_CONTEXTS" "$1" \
    || { printf 'contour rejects GitHub status context: %s\n' "$1" >&2; return 1; }
}

contour_require_deployment_environment() {
  contour_list_has "$CONTOUR_GITHUB_DEPLOYMENT_ENVIRONMENTS" "$1" \
    || { printf 'contour rejects GitHub deployment environment: %s\n' "$1" >&2; return 1; }
}

contour_port_pair() {
  [[ $# -eq 1 ]] || return 2
  local pair=$1
  [[ $pair =~ ^[0-9]+,[0-9]+$ ]] || return 1
  printf '%s\n' "$pair"
}
