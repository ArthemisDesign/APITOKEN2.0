#!/usr/bin/env bash

# Shared helpers for immutable release deployment scripts.

log() {
  printf '[deploy] %s\n' "$*"
}

warn() {
  printf '[deploy] WARNING: %s\n' "$*" >&2
}

die() {
  printf '[deploy] ERROR: %s\n' "$*" >&2
  exit 1
}

print_command() {
  printf '[deploy] +'
  printf ' %q' "$@"
  printf '\n'
}

run() {
  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    print_command "$@"
    return 0
  fi
  "$@"
}

validate_sha() {
  local sha=$1
  [[ "$sha" =~ ^[0-9a-f]{40}$ ]] || die "release SHA must be the full 40-character lowercase commit hash"
}

validate_timeout() {
  local timeout=$1
  [[ "$timeout" =~ ^[1-9][0-9]*$ ]] || die "readiness timeout must be a positive integer"
}

validate_readiness_interval() {
  local interval=$1
  [[ "$interval" =~ ^([1-9]|10)$ ]] || die "readiness interval must be an integer from 1 through 10 seconds"
}

normalize_absolute_path() {
  local input=$1
  local component
  local -a components=()
  local -a normalized=()

  [[ "$input" == /* ]] || return 1
  IFS='/' read -r -a components <<<"$input"
  for component in "${components[@]}"; do
    case "$component" in
      ''|.) ;;
      ..)
        [[ ${#normalized[@]} -gt 0 ]] || return 1
        unset 'normalized[${#normalized[@]}-1]'
        ;;
      *) normalized+=("$component") ;;
    esac
  done

  if [[ ${#normalized[@]} -eq 0 ]]; then
    printf '/\n'
  else
    local joined
    joined=$(IFS=/; printf '%s' "${normalized[*]}")
    printf '/%s\n' "$joined"
  fi
}

canonicalize_path() {
  local input=$1
  local normalized probe base component
  local -a suffix=()

  normalized=$(normalize_absolute_path "$input") || die "path must be an absolute path without escaping above /: $input"
  probe=$normalized

  while [[ ! -e "$probe" ]]; do
    [[ ! -L "$probe" ]] || die "path contains a broken symlink: $probe"
    [[ "$probe" != "/" ]] || die "cannot canonicalize path: $input"
    suffix=("$(basename -- "$probe")" "${suffix[@]}")
    probe=$(dirname -- "$probe")
  done

  base=$(realpath -- "$probe") || die "cannot canonicalize existing path component: $probe"
  for component in "${suffix[@]}"; do
    base="$base/$component"
  done
  normalize_absolute_path "$base" || die "cannot normalize canonical path: $base"
}

canonicalize_release_root() {
  local root=$1
  local fixed_prefix=$2
  local label=$3
  local canonical_root canonical_prefix

  canonical_root=$(canonicalize_path "$root")
  canonical_prefix=$(canonicalize_path "$fixed_prefix")
  case "$canonical_root" in
    "$canonical_prefix"/*) ;;
    *) die "$label release root must remain under $canonical_prefix: $canonical_root" ;;
  esac
  printf '%s\n' "$canonical_root"
}

path_is_direct_release() {
  local root=$1
  local path=$2
  local name

  case "$path" in
    "$root"/*) ;;
    *) return 1 ;;
  esac
  name=${path#"$root"/}
  [[ "$name" != */* && "$name" =~ ^[0-9a-f]{40}$ ]]
}

stat_owner_uid() {
  local path=$1
  if stat -c '%u' -- "$path" >/dev/null 2>&1; then
    stat -c '%u' -- "$path"
  else
    stat -f '%u' -- "$path"
  fi
}

validate_fixed_lock_file() {
  local lock_file=$1
  local expected=$2
  local label=$3
  local owner

  [[ "$lock_file" == "$expected" ]] || die "$label lock path is fixed and may not be overridden: $expected"
  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    log "dry-run: would validate root-owned lock file $lock_file"
    return 0
  fi

  [[ -e "$lock_file" ]] || die "$label lock file is missing; create root-owned $lock_file before deploying"
  [[ ! -L "$lock_file" ]] || die "$label lock file must not be a symlink: $lock_file"
  [[ -f "$lock_file" ]] || die "$label lock path is not a regular file: $lock_file"
  owner=$(stat_owner_uid "$lock_file")
  [[ "$owner" == "0" ]] || die "$label lock file must be owned by root: $lock_file"
}

acquire_deploy_lock() {
  local lock_file=$1
  validate_fixed_lock_file "$lock_file" /run/lock/apitoken-deploy.lock deploy

  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    log "dry-run: would acquire deploy lock $lock_file without truncating it"
    return 0
  fi

  exec 9<>"$lock_file"
  flock -n 9 || die "another deploy or rollback is already running ($lock_file)"
}

acquire_migration_lock() {
  local lock_file=$1
  validate_fixed_lock_file "$lock_file" /run/lock/apitoken-db-migrate.lock migration

  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    log "dry-run: would acquire migration lock $lock_file without truncating it"
    return 0
  fi

  exec 8<>"$lock_file"
  flock -x 8
}

atomic_symlink() {
  local target=$1
  local link=$2
  local temporary="${link}.tmp.$$"

  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    print_command ln -s -- "$target" "$temporary"
    print_command mv -Tf -- "$temporary" "$link"
    return 0
  fi

  if [[ -L "$temporary" ]]; then
    rm -f -- "$temporary"
  elif [[ -e "$temporary" ]]; then
    die "refusing to replace unexpected temporary path: $temporary"
  fi

  ln -s -- "$target" "$temporary"
  if ! mv -Tf -- "$temporary" "$link"; then
    rm -f -- "$temporary"
    return 1
  fi
}

release_path_from_link() {
  local root=$1
  local link=$2
  local target

  [[ -L "$link" ]] || return 1
  target=$(realpath -- "$link") || die "$link is broken or cannot be resolved"
  path_is_direct_release "$root" "$target" || die "$link points outside the direct SHA releases under $root: $target"
  [[ -d "$target" ]] || die "$link points to missing release $target"
  printf '%s\n' "$target"
}

optional_release_path_from_link() {
  local root=$1
  local link=$2

  if [[ -L "$link" ]]; then
    release_path_from_link "$root" "$link"
    return 0
  fi
  [[ ! -e "$link" ]] || die "$link exists but is not a symlink"
  return 1
}

required_current_release_path() {
  local root=$1
  optional_release_path_from_link "$root" "$root/current" || die "no current release under $root; use deploy.sh --bootstrap before starting symlink-based units"
}

validate_release_marker() {
  local directory=$1
  local expected_sha=$2
  local marker="$directory/.release-sha"
  local actual

  [[ -f "$marker" ]] || die "release lacks marker: $marker"
  IFS= read -r actual <"$marker" || die "cannot read release marker: $marker"
  [[ "$actual" == "$expected_sha" ]] || die "release marker mismatch in $directory: expected $expected_sha, found $actual"
}

validate_commerce_release() {
  local root=$1
  local directory=$2
  local expected_sha=$3

  path_is_direct_release "$root" "$directory" || die "invalid commerce release path: $directory"
  [[ -d "$directory" ]] || die "commerce release does not exist: $directory"
  validate_release_marker "$directory" "$expected_sha"
  [[ -r "$directory/apps/api/dist/main.js" ]] || die "commerce API artifact is missing: $directory/apps/api/dist/main.js"
  [[ -r "$directory/apps/content-studio/.next/BUILD_ID" ]] || die "content studio artifact is missing: $directory/apps/content-studio/.next/BUILD_ID"
  [[ -r "$directory/packages/db/dist/migrate.js" ]] || die "prebuilt database migration artifact is missing: $directory/packages/db/dist/migrate.js"
}

validate_engine_release() {
  local root=$1
  local directory=$2
  local expected_sha=$3

  path_is_direct_release "$root" "$directory" || die "invalid engine release path: $directory"
  [[ -d "$directory" ]] || die "engine release does not exist: $directory"
  validate_release_marker "$directory" "$expected_sha"
  [[ -x "$directory/claude-api" ]] || die "engine release binary is missing or not executable: $directory/claude-api"
}

freeze_release_tree() {
  local directory=$1
  log "marking finalized release read-only: $directory"
  run chmod -R a-w -- "$directory"
}

validate_service_unit() {
  local unit=$1
  local lower=${unit,,}

  [[ "$unit" =~ ^[A-Za-z0-9_.@:-]+\.service$ ]] || die "invalid systemd service unit name: $unit"
  [[ "$lower" != *postgres* ]] || die "PostgreSQL units are forbidden in deploy/rollback service lists: $unit"
}

systemctl_raw() {
  local systemctl_bin=${SYSTEMCTL_BIN:-systemctl}
  local sudo_bin=${SUDO_BIN-sudo}
  if [[ ${EUID:-$(id -u)} -eq 0 || -z "$sudo_bin" ]]; then
    "$systemctl_bin" "$@"
  else
    "$sudo_bin" "$systemctl_bin" "$@"
  fi
}

systemctl_command() {
  local systemctl_bin=${SYSTEMCTL_BIN:-systemctl}
  local sudo_bin=${SUDO_BIN-sudo}
  if [[ ${EUID:-$(id -u)} -eq 0 || -z "$sudo_bin" ]]; then
    run "$systemctl_bin" "$@"
  else
    run "$sudo_bin" "$systemctl_bin" "$@"
  fi
}

privileged_command() {
  local sudo_bin=${SUDO_BIN-sudo}
  if [[ ${EUID:-$(id -u)} -eq 0 || -z "$sudo_bin" ]]; then
    run "$@"
  else
    run "$sudo_bin" "$@"
  fi
}

restart_units() {
  local unit
  [[ $# -gt 0 ]] || return 0
  for unit in "$@"; do
    validate_service_unit "$unit"
  done
  log "restarting $*"
  systemctl_command restart "$@"
}

best_effort_restart_units() {
  local unit
  local failed=0

  for unit in "$@"; do
    if ! validate_service_unit "$unit"; then
      failed=1
      continue
    fi
    if systemctl_raw restart "$unit"; then
      log "recovery restarted $unit"
    else
      warn "recovery failed to restart $unit"
      failed=1
    fi
  done
  return "$failed"
}

systemctl_show_value() {
  local unit=$1
  local property=$2
  systemctl_raw show --property="$property" --value "$unit" 2>/dev/null
}

wait_for_unit_active() {
  local unit=$1
  local timeout=$2
  local deadline now remaining

  validate_service_unit "$unit"
  validate_timeout "$timeout"

  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    log "dry-run: would wait up to ${timeout}s for $unit to become active"
    return 0
  fi

  deadline=$(( $(date +%s) + timeout ))
  while true; do
    if systemctl_raw is-active --quiet "$unit" >/dev/null 2>&1; then
      log "$unit is active"
      return 0
    fi
    if systemctl_raw is-failed --quiet "$unit" >/dev/null 2>&1; then
      warn "$unit entered failed state before becoming active"
      return 1
    fi

    now=$(date +%s)
    remaining=$(( deadline - now ))
    (( remaining > 0 )) || break
    sleep 1
  done

  warn "$unit did not become active within ${timeout}s"
  return 1
}

unit_release_binding_ok() {
  local role=$1
  local unit=$2
  local root=$3
  local expected_release=$4
  local current fragment exec_start working_directory main_pid runtime_path

  systemctl_raw is-active --quiet "$unit" >/dev/null 2>&1 || return 1
  current=$(realpath -- "$root/current" 2>/dev/null) || return 1
  [[ "$current" == "$expected_release" ]] || return 1

  fragment=$(systemctl_show_value "$unit" FragmentPath) || return 1
  exec_start=$(systemctl_show_value "$unit" ExecStart) || return 1
  working_directory=$(systemctl_show_value "$unit" WorkingDirectory) || return 1
  main_pid=$(systemctl_show_value "$unit" MainPID) || return 1

  [[ "$fragment" == /* && -f "$fragment" ]] || return 1
  [[ "$main_pid" =~ ^[1-9][0-9]*$ ]] || return 1

  case "$role" in
    api)
      [[ "$working_directory" == "$root/current/apps/api" ]] || return 1
      [[ "$exec_start" == *"dist/main.js"* ]] || return 1
      runtime_path=$(realpath -- "/proc/$main_pid/cwd" 2>/dev/null) || return 1
      [[ "$runtime_path" == "$expected_release/apps/api" ]] || return 1
      ;;
    engine)
      [[ "$exec_start" == *"$root/current/claude-api"* ]] || return 1
      runtime_path=$(realpath -- "/proc/$main_pid/exe" 2>/dev/null) || return 1
      [[ "$runtime_path" == "$expected_release/claude-api" ]] || return 1
      ;;
    *) return 1 ;;
  esac
}

wait_for_release_service() {
  local label=$1
  local role=$2
  local unit=$3
  local root=$4
  local expected_release=$5
  local url=$6
  local timeout=$7
  local interval=${READINESS_INTERVAL_SECONDS:-2}
  local deadline now remaining curl_timeout sleep_for

  validate_service_unit "$unit"
  validate_timeout "$timeout"
  validate_readiness_interval "$interval"

  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    log "dry-run: would require $unit active with its loaded fragment and process bound to $root/current -> $expected_release"
    log "dry-run: would probe $label readiness at $url for up to ${timeout}s"
    return 0
  fi

  deadline=$(( $(date +%s) + timeout ))
  while true; do
    now=$(date +%s)
    remaining=$(( deadline - now ))
    (( remaining > 0 )) || break

    if unit_release_binding_ok "$role" "$unit" "$root" "$expected_release"; then
      curl_timeout=$remaining
      (( curl_timeout > 5 )) && curl_timeout=5
      if curl -fsS --max-time "$curl_timeout" "$url" >/dev/null; then
        log "$label is active on $unit, serving $(basename -- "$expected_release"), and ready at $url"
        return 0
      fi
    fi

    now=$(date +%s)
    remaining=$(( deadline - now ))
    (( remaining > 0 )) || break
    sleep_for=$interval
    (( sleep_for > remaining )) && sleep_for=$remaining
    sleep "$sleep_for"
  done

  warn "$label did not become active on $unit and ready for release $(basename -- "$expected_release") within ${timeout}s"
  return 1
}

ACTIVATION_LINKS=()
ACTIVATION_ROOTS=()
ACTIVATION_ORIGINAL_STATES=()
ACTIVATION_ORIGINAL_TARGETS=()
ACTIVATION_CHANGED=()
ACTIVATION_ACTIVE=0
ACTIVATION_COMMITTED=0
ACTIVATION_RECOVERY_CALLBACK=

journal_index_for_link() {
  local link=$1
  local index
  for index in "${!ACTIVATION_LINKS[@]}"; do
    if [[ "${ACTIVATION_LINKS[$index]}" == "$link" ]]; then
      printf '%s\n' "$index"
      return 0
    fi
  done
  return 1
}

capture_release_link() {
  local root=$1
  local link=$2
  local target index

  if journal_index_for_link "$link" >/dev/null; then
    return 0
  fi
  case "$link" in
    "$root/current"|"$root/previous") ;;
    *) die "refusing to journal unexpected release link: $link" ;;
  esac

  if [[ -L "$link" ]]; then
    target=$(release_path_from_link "$root" "$link")
    ACTIVATION_ORIGINAL_STATES+=(target)
    ACTIVATION_ORIGINAL_TARGETS+=("$target")
  elif [[ -e "$link" ]]; then
    die "$link exists but is not a symlink"
  else
    ACTIVATION_ORIGINAL_STATES+=(absent)
    ACTIVATION_ORIGINAL_TARGETS+=("")
  fi

  index=${#ACTIVATION_LINKS[@]}
  ACTIVATION_LINKS+=("$link")
  ACTIVATION_ROOTS+=("$root")
  ACTIVATION_CHANGED+=(0)
  log "captured original link state for $link"
}

captured_link_target() {
  local link=$1
  local index
  index=$(journal_index_for_link "$link") || die "link was not captured before activation: $link"
  if [[ "${ACTIVATION_ORIGINAL_STATES[$index]}" == "target" ]]; then
    printf '%s\n' "${ACTIVATION_ORIGINAL_TARGETS[$index]}"
  fi
}

journaled_link_is_unchanged() {
  local index=$1
  local link=${ACTIVATION_LINKS[$index]}
  local root=${ACTIVATION_ROOTS[$index]}
  local current

  if [[ "${ACTIVATION_ORIGINAL_STATES[$index]}" == "absent" ]]; then
    [[ ! -L "$link" && ! -e "$link" ]]
    return
  fi
  [[ -L "$link" ]] || return 1
  current=$(realpath -- "$link" 2>/dev/null) || return 1
  [[ "$current" == "${ACTIVATION_ORIGINAL_TARGETS[$index]}" ]]
}

set_journaled_release_link() {
  local target=$1
  local link=$2
  local index root current=

  index=$(journal_index_for_link "$link") || die "link was not captured before activation: $link"
  root=${ACTIVATION_ROOTS[$index]}
  path_is_direct_release "$root" "$target" || die "refusing to point $link at invalid release path: $target"
  journaled_link_is_unchanged "$index" || die "$link changed after preflight; refusing activation"

  if [[ -L "$link" ]]; then
    current=$(realpath -- "$link") || die "$link became broken after preflight"
  fi
  if [[ "$current" == "$target" ]]; then
    log "$link already points to $(basename -- "$target"); leaving link bookkeeping unchanged"
    return 0
  fi

  ACTIVATION_CHANGED[$index]=1
  atomic_symlink "$target" "$link"
}

restore_absent_link() {
  local link=$1
  local temporary="${link}.tmp.$$"

  if [[ -L "$temporary" ]]; then
    rm -f -- "$temporary" || return 1
  elif [[ -e "$temporary" ]]; then
    return 1
  fi

  if [[ -L "$link" ]]; then
    rm -f -- "$link"
  elif [[ -e "$link" ]]; then
    return 1
  fi
}

restore_target_link() {
  local target=$1
  local link=$2
  local temporary="${link}.tmp.$$"

  if [[ -L "$temporary" ]]; then
    rm -f -- "$temporary" || return 1
  elif [[ -e "$temporary" ]]; then
    return 1
  fi
  ln -s -- "$target" "$temporary" || return 1
  if ! mv -Tf -- "$temporary" "$link"; then
    rm -f -- "$temporary"
    return 1
  fi
}

restore_activation_links() {
  local index link state target
  local failed=0

  for (( index=${#ACTIVATION_LINKS[@]}-1; index>=0; index-- )); do
    [[ "${ACTIVATION_CHANGED[$index]}" == "1" ]] || continue
    link=${ACTIVATION_LINKS[$index]}
    state=${ACTIVATION_ORIGINAL_STATES[$index]}
    target=${ACTIVATION_ORIGINAL_TARGETS[$index]}

    if [[ "$state" == "target" ]]; then
      if restore_target_link "$target" "$link"; then
        warn "restored $link -> $target"
      else
        warn "FAILED to restore $link -> $target"
        failed=1
      fi
    else
      if restore_absent_link "$link"; then
        warn "restored original absence of $link"
      else
        warn "FAILED to restore original absence of $link"
        failed=1
      fi
    fi
  done
  return "$failed"
}

activation_abort() {
  local status=$1
  local reason=$2
  local recovery_failed=0

  trap - ERR EXIT INT TERM
  (( status != 0 )) || status=1
  set +e

  if [[ "$ACTIVATION_ACTIVE" == "1" && "$ACTIVATION_COMMITTED" != "1" ]]; then
    warn "activation aborted by $reason; restoring every changed release link and restarting affected services"
    restore_activation_links || recovery_failed=1
    if [[ -n "$ACTIVATION_RECOVERY_CALLBACK" ]]; then
      if ! "$ACTIVATION_RECOVERY_CALLBACK"; then
        warn "one or more recovery service actions failed"
        recovery_failed=1
      fi
    fi
    (( recovery_failed == 0 )) || warn "automatic recovery was incomplete; operator intervention is required"
  fi

  exit "$status"
}

begin_activation() {
  local recovery_callback=$1
  [[ "$ACTIVATION_ACTIVE" == "0" ]] || die "activation traps are already installed"
  [[ -z "$recovery_callback" || $(type -t "$recovery_callback") == function ]] || die "recovery callback is not a function: $recovery_callback"

  ACTIVATION_ACTIVE=1
  ACTIVATION_COMMITTED=0
  ACTIVATION_RECOVERY_CALLBACK=$recovery_callback
  trap 'activation_abort "$?" ERR' ERR
  trap 'activation_abort "$?" EXIT' EXIT
  trap 'activation_abort 130 INT' INT
  trap 'activation_abort 143 TERM' TERM
}

commit_activation() {
  ACTIVATION_COMMITTED=1
  ACTIVATION_ACTIVE=0
  trap - ERR EXIT INT TERM
}
