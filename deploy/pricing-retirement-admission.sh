#!/usr/bin/env bash
set -euo pipefail

# Root-owned, fail-closed admission bridge for the two one-time pricing-schema contractions. It is
# deliberately inert for every other migration and after the named contraction has been recorded.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=deploy/watchdog-lib.sh
source "$SCRIPT_DIR/watchdog-lib.sh"

STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
COMMERCE_APPLIED_MANIFEST=$STATE_ROOT/database-migrations.manifest
ENGINE_RELEASE_ROOT=/srv/claude-api/releases
COMPOSE_FILE=/usr/local/lib/apitoken-watchdog/controller/commerce-postgres.compose.yaml
POSTGRES_ENV=/etc/apitoken/postgres.env
COMMERCE_CONTRACTION_REL=packages/db/migrations/0048_retire_pricing_schema.sql
COMMERCE_CONTRACTION_TAG=0048_retire_pricing_schema
ENGINE_CONTRACTION_REL=crates/registry/migrations_pg/0049_retire_pricing_schema.sql
ENGINE_CONTRACTION_VERSION=49
ENGINE_PREDECESSOR_VERSION=48
PREFLIGHT_REL=deploy/pricing-retirement-preflight.sh
PGOPTIONS_RO='-c default_transaction_read_only=on -c statement_timeout=120000 -c lock_timeout=5000 -c timezone=UTC -c datestyle=ISO'

CANDIDATE=
MARKER=
CANDIDATE_MANIFEST=

pra_require_root() {
  [[ ${EUID:-$(id -u)} -eq 0 ]] \
    || wd_die "pricing retirement admission must run as root through a fixed migration runner"
}

pra_require_root_owned_candidate() {
  local candidate=$1 mode
  [[ -d $candidate && ! -L $candidate ]] \
    || wd_die "tested pricing-retirement candidate is missing or unsafe: $candidate"
  [[ $(stat -c '%u' -- "$candidate") == 0 ]] \
    || wd_die "tested pricing-retirement candidate must be root-owned"
  mode=$(stat -c '%a' -- "$candidate")
  (( (8#$mode & 8#222) == 0 )) \
    || wd_die "tested pricing-retirement candidate must be immutable"
}

pra_manifest_record() {
  local manifest=$1 kind=$2 value=$3
  awk -v kind="$kind" -v value="$value" '
    $1 == kind && $3 == value && NF == 3 { print }
  ' "$manifest"
}

pra_validate_candidate() {
  local sha=$1 marker_sha marker_tree marker_digest candidate_sha candidate_tree actual_digest

  CANDIDATE=$CANDIDATE_ROOT/$sha
  MARKER=$STATE_ROOT/$sha.tested
  pra_require_root_owned_candidate "$CANDIDATE"
  [[ -f $MARKER && ! -L $MARKER ]] \
    || wd_die "pricing-retirement candidate test marker is missing or unsafe"

  marker_sha=$(wd_marker_value "$MARKER" sha) \
    || wd_die "pricing-retirement candidate marker has no SHA"
  marker_tree=$(wd_marker_value "$MARKER" tree) \
    || wd_die "pricing-retirement candidate marker has no tree"
  marker_digest=$(wd_marker_value "$MARKER" migration_digest) \
    || wd_die "pricing-retirement candidate marker has no migration digest"
  [[ $marker_sha == "$sha" ]] || wd_die "pricing-retirement candidate marker SHA mismatch"

  candidate_sha=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{commit}')
  candidate_tree=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{tree}')
  [[ $candidate_sha == "$sha" && $candidate_tree == "$marker_tree" ]] \
    || wd_die "pricing-retirement candidate identity changed after validation"
  [[ -z $(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" \
    status --porcelain --untracked-files=no) ]] \
    || wd_die "pricing-retirement candidate has tracked modifications"

  CANDIDATE_MANIFEST=$(mktemp "$STATE_ROOT/.pricing-retirement-admission.XXXXXX") \
    || wd_die "cannot create pricing-retirement admission manifest"
  wd_migration_manifest "$CANDIDATE" >"$CANDIDATE_MANIFEST"
  actual_digest=$(wd_manifest_digest "$CANDIDATE_MANIFEST")
  [[ $actual_digest == "$marker_digest" ]] \
    || wd_die "pricing-retirement candidate migrations changed after tests"
}

pra_commerce_is_pending() {
  local contraction=$CANDIDATE/$COMMERCE_CONTRACTION_REL
  local candidate_file candidate_entry applied_file applied_entry

  if [[ ! -e $contraction && ! -L $contraction ]]; then
    wd_log "pricing retirement admission is not applicable to this commerce migration"
    return 1
  fi
  [[ -f $contraction && ! -L $contraction ]] \
    || wd_die "commerce pricing-retirement contraction artifact is unsafe"
  [[ -f $COMMERCE_APPLIED_MANIFEST && ! -L $COMMERCE_APPLIED_MANIFEST ]] \
    || wd_die "applied commerce migration manifest is missing or unsafe"
  wd_manifest_is_append_only "$COMMERCE_APPLIED_MANIFEST" "$CANDIDATE_MANIFEST" \
    || wd_die "commerce migration history is not append-only at pricing-retirement admission"

  # File records have a digest attached to the first field, so validate them separately.
  candidate_file=$(awk -v path="$COMMERCE_CONTRACTION_REL" \
    '$1 ~ /^file=[0-9a-f]{64}$/ && $2 == path && NF == 2 { print }' \
    "$CANDIDATE_MANIFEST")
  [[ $(printf '%s\n' "$candidate_file" | awk 'NF { count++ } END { print count + 0 }') == 1 ]] \
    || wd_die "commerce contraction must have exactly one canonical migration artifact"
  candidate_entry=$(pra_manifest_record "$CANDIDATE_MANIFEST" \
    entry=00000048 "$COMMERCE_CONTRACTION_TAG")
  [[ $(printf '%s\n' "$candidate_entry" | awk 'NF { count++ } END { print count + 0 }') == 1 ]] \
    || wd_die "commerce contraction is not the canonical journal entry 0048"

  applied_file=$(grep -Fxc -- "$candidate_file" "$COMMERCE_APPLIED_MANIFEST" || true)
  applied_entry=$(grep -Fxc -- "$candidate_entry" "$COMMERCE_APPLIED_MANIFEST" || true)
  case "$applied_file:$applied_entry" in
    0:0) return 0 ;;
    1:1)
      wd_log "commerce pricing-retirement contraction is already recorded; admission is a no-op"
      return 1
      ;;
    *) wd_die "applied commerce manifest contains a partial pricing-retirement contraction" ;;
  esac
}

pra_engine_schema_state() {
  [[ -f $COMPOSE_FILE && ! -L $COMPOSE_FILE ]] \
    || wd_die "fixed PostgreSQL Compose definition is missing or unsafe"
  [[ -f $POSTGRES_ENV && ! -L $POSTGRES_ENV ]] \
    || wd_die "fixed PostgreSQL environment is missing or unsafe"
  docker compose --env-file "$POSTGRES_ENV" -f "$COMPOSE_FILE" \
    exec -T -e "PGOPTIONS=$PGOPTIONS_RO" commerce-postgres \
    psql -X -qAt -F $'\t' -v ON_ERROR_STOP=1 -U commerce -d claude_engine -c \
    "SELECT count(*) FILTER (WHERE version = $ENGINE_CONTRACTION_VERSION), COALESCE(max(version), 0) FROM public.engine_schema_migrations;"
}

pra_engine_is_pending() {
  local sha=$1 contraction=$CANDIDATE/$ENGINE_CONTRACTION_REL source marker_flag
  local expected_hash actual_hash release state applied_count max_version extra

  if [[ ! -e $contraction && ! -L $contraction ]]; then
    wd_log "pricing retirement admission is not applicable to this engine migration"
    return 1
  fi
  [[ -f $contraction && ! -L $contraction ]] \
    || wd_die "engine pricing-retirement contraction artifact is unsafe"
  source=$CANDIDATE/crates/registry/src/pg.rs
  [[ -f $source && ! -L $source ]] || wd_die "engine PostgreSQL migration registry is unsafe"
  grep -Fq 'include_str!("../migrations_pg/0049_retire_pricing_schema.sql")' "$source" \
    || wd_die "engine contraction is not embedded as migration 0049"
  grep -Fq 'pub const CURRENT_SCHEMA_VERSION: i64 = 49;' "$source" \
    || wd_die "engine schema version does not declare contraction version 49"
  grep -Fq '(49, MIGRATION_0049),' "$source" \
    || wd_die "engine contraction is absent from the contiguous migration registry"

  marker_flag=$(wd_marker_value "$MARKER" engine_artifacts) \
    || wd_die "pricing-retirement candidate marker lacks engine artifact evidence"
  [[ $marker_flag == 1 ]] || wd_die "engine contraction candidate did not pass the engine artifact lane"
  expected_hash=$(wd_marker_value "$MARKER" engine_binary_sha256) \
    || wd_die "pricing-retirement candidate marker lacks engine binary identity"
  [[ $expected_hash =~ ^[0-9a-f]{64}$ ]] \
    || wd_die "pricing-retirement candidate engine binary identity is malformed"
  release=$ENGINE_RELEASE_ROOT/$sha
  [[ -d $release && ! -L $release && -x $release/claude-api && -f $release/claude-api \
      && ! -L $release/claude-api ]] \
    || wd_die "exact engine migration release is missing or unsafe"
  actual_hash=$(wd_sha256_file "$release/claude-api")
  [[ $actual_hash == "$expected_hash" ]] \
    || wd_die "engine migration release differs from the exact tested artifact"

  state=$(pra_engine_schema_state)
  IFS=$'\t' read -r applied_count max_version extra <<<"$state"
  [[ $applied_count =~ ^[0-9]+$ && $max_version =~ ^[0-9]+$ && -z ${extra:-} ]] \
    || wd_die "engine schema migration state is malformed"
  if [[ $applied_count == 0 && $max_version == "$ENGINE_PREDECESSOR_VERSION" ]]; then
    return 0
  fi
  if [[ $applied_count == 1 ]] && (( max_version >= ENGINE_CONTRACTION_VERSION )); then
    wd_log "engine pricing-retirement contraction is already recorded; admission is a no-op"
    return 1
  fi
  wd_die "engine schema is not at the unique pre/post state for contraction 0049: applied=$applied_count max=$max_version"
}

pra_execute_preflight() {
  local plane=$1 sha=$2
  (
    cd -- "$CANDIDATE"
    "$CANDIDATE/$PREFLIGHT_REL" --final "$plane" "$sha"
  )
}

pra_authorize() {
  local plane=$1 sha=$2 output status auth_count unauthorized_count authorization
  [[ -x $CANDIDATE/$PREFLIGHT_REL && -f $CANDIDATE/$PREFLIGHT_REL \
      && ! -L $CANDIDATE/$PREFLIGHT_REL ]] \
    || wd_die "exact-candidate pricing retirement preflight is missing or unsafe"
  set +e
  output=$(pra_execute_preflight "$plane" "$sha" 2>&1)
  status=$?
  set -e
  if (( status != 0 )); then
    printf '%s\n' "$output" >&2
    wd_die "pricing-retirement final preflight rejected $plane contraction for $sha"
  fi

  auth_count=$(printf '%s\n' "$output" | awk '/^AUTHORIZED:/ { count++ } END { print count + 0 }')
  unauthorized_count=$(printf '%s\n' "$output" \
    | awk '/^NOT AUTHORIZED/ { count++ } END { print count + 0 }')
  authorization=$(printf '%s\n' "$output" | awk '/^AUTHORIZED:/ { print }')
  [[ $auth_count == 1 && $unauthorized_count == 0 ]] \
    || wd_die "pricing-retirement preflight returned an ambiguous authorization verdict"
  [[ $authorization =~ ^AUTHORIZED:${plane}[[:space:]]migration_sha=${sha}[[:space:]]retention_epoch=[0-9]+[[:space:]]all_conjunctive_gates=passed$ ]] \
    || wd_die "pricing-retirement preflight returned a malformed authorization verdict"
  printf '%s\n' "$output"
  wd_log "pricing-retirement $plane contraction admitted for exact candidate $sha"
}

pricing_retirement_admission_main() {
  local plane sha pending=1
  pra_require_root
  [[ $# -eq 2 ]] || wd_die "usage: $0 <commerce|engine> <tested-full-sha>"
  plane=$1
  sha=$2
  [[ $plane == commerce || $plane == engine ]] \
    || wd_die "pricing retirement admission plane must be commerce or engine"
  wd_validate_sha "$sha"

  trap '[[ -z ${CANDIDATE_MANIFEST:-} ]] || rm -f -- "$CANDIDATE_MANIFEST"' EXIT
  pra_validate_candidate "$sha"
  case $plane in
    commerce) pra_commerce_is_pending || pending=0 ;;
    engine) pra_engine_is_pending "$sha" || pending=0 ;;
  esac
  (( pending == 1 )) || return 0
  pra_authorize "$plane" "$sha"
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  pricing_retirement_admission_main "$@"
fi
