#!/usr/bin/env bash
set -euo pipefail

# One-time migration of legacy app-server CODEX_HOME directories into the sealed Codex roster
# used by the native provider. Sealing itself is `claude-api codex-seal` (Rust): this wrapper
# only discovers the legacy homes, enforces safe filesystem shapes, and runs the migrator with
# the operator's keyring. It never prints token material and deletes a legacy home only after its
# profile landed in the roster.

CODEX_LEGACY_SINGLE_HOME=${CODEX_LEGACY_SINGLE_HOME:-/srv/claude-api/data/codex/home}
CODEX_LEGACY_HOMES_DIR=${CODEX_LEGACY_HOMES_DIR:-/srv/claude-api/data/codex-homes}
CODEX_ROSTER_DIR=${CODEX_ROSTER_DIR:-/srv/claude-api/data/codex}
CODEX_SEAL_ENV=${CODEX_SEAL_ENV:-/srv/claude-api/data/codex-seal.env}
ENGINE_BIN=${ENGINE_BIN:-/srv/claude-api/releases/current/claude-api}

codex_migration_fail() {
  printf '[codex-homes-migrate] ERROR: %s\n' "$*" >&2
  return 1
}

codex_migration_validate_home() {
  local home=$1
  [[ -d $home && ! -L $home ]] \
    || { codex_migration_fail "legacy Codex home is missing or unsafe: $home"; return 1; }
  [[ -f $home/auth.json && ! -L $home/auth.json ]] \
    || { codex_migration_fail "legacy Codex home has no regular auth.json: $home"; return 1; }
}

codex_migration_homes() {
  if [[ -d $CODEX_LEGACY_SINGLE_HOME && ! -L $CODEX_LEGACY_SINGLE_HOME ]]; then
    printf '%s\n' "$CODEX_LEGACY_SINGLE_HOME"
  fi
  if [[ -d $CODEX_LEGACY_HOMES_DIR && ! -L $CODEX_LEGACY_HOMES_DIR ]]; then
    local child
    for child in "$CODEX_LEGACY_HOMES_DIR"/*; do
      [[ -d $child && ! -L $child ]] || continue
      case "$(basename -- "$child")" in .*) continue ;; esac
      printf '%s\n' "$child"
    done
  fi
}

codex_migration_check() {
  local home count=0
  [[ -x $ENGINE_BIN && ! -L $ENGINE_BIN ]] \
    || codex_migration_fail "engine binary is missing or unsafe: $ENGINE_BIN"
  [[ -f $CODEX_SEAL_ENV && ! -L $CODEX_SEAL_ENV ]] \
    || codex_migration_fail "seal keyring env is missing or unsafe: $CODEX_SEAL_ENV"
  while IFS= read -r home; do
    codex_migration_validate_home "$home" || return 1
    count=$((count + 1))
  done < <(codex_migration_homes)
  printf '[codex-homes-migrate] %d legacy home(s) ready to seal into %s\n' "$count" "$CODEX_ROSTER_DIR"
}

codex_migration_apply() {
  local home profile
  set -a
  # shellcheck disable=SC1090
  . "$CODEX_SEAL_ENV"
  set +a
  [[ -n ${CODEX_SEAL_KEYS:-} && -n ${CODEX_SEAL_ACTIVE_KID:-} ]] \
    || codex_migration_fail "CODEX_SEAL_KEYS and CODEX_SEAL_ACTIVE_KID must be set in $CODEX_SEAL_ENV"
  mkdir -p -- "$CODEX_ROSTER_DIR/credentials"
  chmod 0700 "$CODEX_ROSTER_DIR" "$CODEX_ROSTER_DIR/credentials"
  while IFS= read -r home; do
    codex_migration_validate_home "$home" || return 1
    profile=$("$ENGINE_BIN" codex-seal \
      --home "$home" \
      --roster "$CODEX_ROSTER_DIR" \
      --keys "$CODEX_SEAL_KEYS" \
      --active-kid "$CODEX_SEAL_ACTIVE_KID" \
      --delete-home) \
      || codex_migration_fail "could not seal legacy home: $home"
    printf '[codex-homes-migrate] sealed profile %s and removed %s\n' "$profile" "$home"
  done < <(codex_migration_homes)
}

case "${1:-}" in
  --check) codex_migration_check ;;
  --apply) codex_migration_check >/dev/null && codex_migration_apply ;;
  *)
    printf 'Usage: codex-homes-migrate.sh --check|--apply\n' >&2
    exit 2
    ;;
esac
