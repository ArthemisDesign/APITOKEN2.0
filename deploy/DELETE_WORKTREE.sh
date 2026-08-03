#!/usr/bin/env bash
# DELETE_WORKTREE: persistent, fail-closed cleanup for merged task worktrees and explicitly
# registered standalone clones of this repository.
set -euo pipefail

SCRIPT_PATH=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/$(basename -- "${BASH_SOURCE[0]}")
SCRIPT_ROOT=$(cd -- "$(dirname -- "$SCRIPT_PATH")/.." && pwd)
LABEL=sale.apitoken.DELETE_WORKTREE
DEFAULT_INTERVAL_SECONDS=15
DEFAULT_SETTLE_SECONDS=30
LOCK_HELD=0
RUN_LOCK=
RUN_LOCK_HOST=

dw_log() { printf '[DELETE_WORKTREE] %s\n' "$*" >&2; }
dw_die() { dw_log "ERROR: $*"; exit 1; }

dw_usage() {
  cat >&2 <<'USAGE'
usage:
  deploy/DELETE_WORKTREE.sh install [--interval-seconds N] [--settle-seconds N] [--dry-run]
  deploy/DELETE_WORKTREE.sh uninstall [--state-dir PATH]
  deploy/DELETE_WORKTREE.sh once [--repo PATH] [--state-dir PATH] [--settle-seconds N] [--dry-run]
  deploy/DELETE_WORKTREE.sh daemon [--repo PATH] [--state-dir PATH]
                                    [--interval-seconds N] [--settle-seconds N]
  deploy/DELETE_WORKTREE.sh register-clone PATH [--repo PATH] [--state-dir PATH] [--allow-ignored]
  deploy/DELETE_WORKTREE.sh unregister-clone PATH [--state-dir PATH]
  deploy/DELETE_WORKTREE.sh status [--repo PATH] [--state-dir PATH]
  deploy/DELETE_WORKTREE.sh render-plist [--repo PATH] [--state-dir PATH]
                                           [--interval-seconds N] [--settle-seconds N]

Worktrees are removed only through deploy/agent-worktree.sh finish after two stable observations.
Standalone clones are never discovered or deleted implicitly; register-clone is an explicit
allow-list operation.
USAGE
}

dw_is_uint() {
  [[ $1 =~ ^[0-9]+$ && ${#1} -le 9 ]]
}

dw_validate_interval() {
  dw_is_uint "$1" || dw_die 'interval seconds must be an integer from 5 through 3600'
  (( 10#$1 >= 5 && 10#$1 <= 3600 )) \
    || dw_die 'interval seconds must be an integer from 5 through 3600'
}

dw_validate_settle() {
  dw_is_uint "$1" || dw_die 'settle seconds must be an integer from 0 through 3600'
  (( 10#$1 <= 3600 )) || dw_die 'settle seconds must be an integer from 0 through 3600'
}

dw_safe_scalar() {
  case "$1" in
    *$'\n'*|*$'\r'*|*$'\t'*) return 1 ;;
    *) return 0 ;;
  esac
}

dw_canonical_dir() {
  [[ -d $1 && ! -L $1 ]] || return 1
  (cd -- "$1" && pwd -P)
}

dw_default_state_dir() {
  printf '%s\n' "${DELETE_WORKTREE_STATE_DIR:-${HOME:?HOME is required}/Library/Application Support/DELETE_WORKTREE}"
}

dw_resolve_repository() {
  local requested=$1 top listing main
  top=$(git -C "$requested" rev-parse --show-toplevel 2>/dev/null) \
    || dw_die "not a Git worktree: $requested"
  top=$(dw_canonical_dir "$top") || dw_die "cannot canonicalize repository path: $top"
  listing=$(git -C "$top" worktree list --porcelain 2>/dev/null) \
    || dw_die "cannot enumerate worktrees from $top"
  main=$(sed -n '1s/^worktree //p' <<<"$listing")
  [[ -n $main ]] || dw_die "cannot resolve the primary worktree from $top"
  MAIN_TOP=$(dw_canonical_dir "$main") || dw_die "primary worktree is unavailable: $main"
  MANAGER=$MAIN_TOP/deploy/agent-worktree.sh
  [[ -x $MANAGER ]] || dw_die "managed lifecycle script is unavailable or not executable: $MANAGER"
}

dw_prepare_state() {
  dw_safe_scalar "$STATE_DIR" || dw_die 'state directory contains a forbidden control character'
  mkdir -p -- "$STATE_DIR"
  chmod 700 "$STATE_DIR"
  STATE_DIR=$(dw_canonical_dir "$STATE_DIR") || dw_die 'cannot canonicalize the state directory'
  CANDIDATES_FILE=$STATE_DIR/candidates.tsv
  CLONES_FILE=$STATE_DIR/clones
  REPO_FILE=$STATE_DIR/repo
  [[ -e $CLONES_FILE ]] || : >"$CLONES_FILE"
  chmod 600 "$CLONES_FILE"
}

dw_release_run_lock() {
  local owner_pid owner_host
  (( LOCK_HELD == 1 )) || return 0
  owner_pid=$(cat "$RUN_LOCK/pid" 2>/dev/null || printf '')
  owner_host=$(cat "$RUN_LOCK/host" 2>/dev/null || printf '')
  if [[ $owner_pid == "$$" && $owner_host == "$RUN_LOCK_HOST" ]]; then
    rm -f -- "$RUN_LOCK/pid" "$RUN_LOCK/host"
    rmdir "$RUN_LOCK" 2>/dev/null || true
  fi
  LOCK_HELD=0
}

dw_acquire_run_lock() {
  local owner_pid owner_host
  RUN_LOCK=$STATE_DIR/run.lock
  RUN_LOCK_HOST=$(hostname)
  if ! mkdir "$RUN_LOCK" 2>/dev/null; then
    owner_pid=$(cat "$RUN_LOCK/pid" 2>/dev/null || printf '')
    owner_host=$(cat "$RUN_LOCK/host" 2>/dev/null || printf '')
    if [[ $owner_host == "$RUN_LOCK_HOST" && $owner_pid =~ ^[0-9]+$ ]] \
      && ! kill -0 "$owner_pid" 2>/dev/null; then
      rm -f -- "$RUN_LOCK/pid" "$RUN_LOCK/host"
      rmdir "$RUN_LOCK" 2>/dev/null || true
      mkdir "$RUN_LOCK" 2>/dev/null || dw_die 'another DELETE_WORKTREE pass is active'
    else
      dw_die 'another DELETE_WORKTREE pass is active'
    fi
  fi
  printf '%s\n' "$$" >"$RUN_LOCK/pid"
  printf '%s\n' "$RUN_LOCK_HOST" >"$RUN_LOCK/host"
  LOCK_HELD=1
  trap dw_release_run_lock EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
}

dw_make_temp() {
  mktemp "$STATE_DIR/$1.XXXXXX"
}

dw_capture_active_paths() {
  local target=$1 source_file=${DELETE_WORKTREE_ACTIVE_PATHS_FILE:-}
  if [[ -n $source_file ]]; then
    [[ -f $source_file ]] || dw_die "active-path fixture is unavailable: $source_file"
    cp -- "$source_file" "$target"
    return 0
  fi
  command -v lsof >/dev/null 2>&1 \
    || dw_die 'lsof is required for fail-closed active-process detection'
  if ! lsof -n -P -u "$(id -un)" -F n 2>/dev/null | sed -n 's/^n//p' >"$target"; then
    dw_die 'lsof could not enumerate active paths; refusing automatic deletion'
  fi
}

dw_path_is_active() {
  local candidate=$1 active_path
  while IFS= read -r active_path; do
    case "$active_path" in
      "$candidate"|"$candidate"/*) return 0 ;;
    esac
  done <"$ACTIVE_PATHS_FILE"
  return 1
}

dw_git_operation_in_progress() {
  local repository=$1 git_dir marker
  git_dir=$(git -C "$repository" rev-parse --path-format=absolute --git-dir 2>/dev/null) \
    || return 0
  for marker in \
    index.lock HEAD.lock MERGE_HEAD CHERRY_PICK_HEAD REVERT_HEAD BISECT_LOG rebase-apply rebase-merge; do
    [[ ! -e $git_dir/$marker ]] || return 0
  done
  return 1
}

dw_previous_first_seen() {
  local kind=$1 fingerprint=$2 path=$3
  [[ -f $CANDIDATES_FILE ]] || return 0
  awk -F '\t' -v kind="$kind" -v fingerprint="$fingerprint" -v path="$path" '
    $1 == kind && $2 == fingerprint && $4 == path { print $3; exit }
  ' "$CANDIDATES_FILE"
}

dw_record_candidate() {
  printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >>"$NEXT_CANDIDATES_FILE"
}

dw_candidate_is_stable() {
  local kind=$1 fingerprint=$2 path=$3 now=$4 first_seen
  first_seen=$(dw_previous_first_seen "$kind" "$fingerprint" "$path")
  if [[ ! $first_seen =~ ^[0-9]+$ || $now -lt $first_seen ]]; then
    first_seen=$now
    dw_record_candidate "$kind" "$fingerprint" "$first_seen" "$path"
    return 1
  fi
  if (( now - first_seen < SETTLE_SECONDS )); then
    dw_record_candidate "$kind" "$fingerprint" "$first_seen" "$path"
    return 1
  fi
  CANDIDATE_FIRST_SEEN=$first_seen
  return 0
}

dw_size_kib() {
  du -sk "$1" 2>/dev/null | awk '{print $1+0}'
}

dw_worktree_fingerprint() {
  local path=$1 branch head
  branch=$(git -C "$path" symbolic-ref --quiet --short HEAD 2>/dev/null) || return 1
  head=$(git -C "$path" rev-parse HEAD 2>/dev/null) || return 1
  printf 'worktree\n%s\n%s\n%s\n' "$path" "$branch" "$head" | git hash-object --stdin
}

dw_scan_worktrees() {
  local report_file=$1 now=$2 status branch path detail normalized fingerprint size
  while IFS=$'\t' read -r status branch path detail; do
    normalized=$(printf '%s' "$status" | tr -d '[:space:]')
    [[ $normalized == MERGED_CANDIDATE ]] || continue
    dw_safe_scalar "$path" || { dw_log 'preserving a candidate with an unsafe path'; continue; }
    [[ -d $path && ! -L $path ]] || continue
    path=$(dw_canonical_dir "$path") || continue
    if dw_path_is_active "$path"; then
      dw_log "preserving active worktree: $path"
      continue
    fi
    if dw_git_operation_in_progress "$path"; then
      dw_log "preserving worktree with an in-progress Git operation: $path"
      continue
    fi
    fingerprint=$(dw_worktree_fingerprint "$path") || {
      dw_log "preserving worktree whose branch fingerprint is unavailable: $path"
      continue
    }
    if ! dw_candidate_is_stable worktree "$fingerprint" "$path" "$now"; then
      dw_log "observing merged worktree before deletion: $path"
      continue
    fi
    if (( DRY_RUN == 1 )); then
      dw_log "dry run: would finish merged worktree $path"
      dw_record_candidate worktree "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
      continue
    fi
    dw_capture_active_paths "$ACTIVE_PATHS_FILE"
    if dw_path_is_active "$path" || dw_git_operation_in_progress "$path"; then
      dw_log "preserving worktree that became active during final validation: $path"
      dw_record_candidate worktree "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
      continue
    fi
    size=$(dw_size_kib "$path")
    if (cd / && "$MANAGER" finish "$path"); then
      dw_log "freed approximately ${size} KiB by finishing merged worktree: $path"
    else
      dw_log "WARNING: final lifecycle validation rejected worktree; preserving it: $path"
      dw_record_candidate worktree "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
    fi
  done <"$report_file"
}

dw_normalize_remote() {
  local value=$1
  value=$(printf '%s' "$value" | sed -E \
    -e 's#^[A-Za-z][A-Za-z0-9+.-]*://##' \
    -e 's#^[^/@]+@##' \
    -e 's#^([^/:]+):#\1/#' \
    -e 's#[?#].*$##' \
    -e 's#/*$##' \
    -e 's#\.git$##')
  printf '%s\n' "$value" | tr '[:upper:]' '[:lower:]'
}

dw_repository_identity() {
  local remote
  remote=$(git -C "$1" remote get-url origin 2>/dev/null) || return 1
  dw_normalize_remote "$remote"
}

dw_is_standalone_clone() {
  local path=$1 top common count
  [[ -d $path/.git && ! -L $path/.git ]] || return 1
  top=$(git -C "$path" rev-parse --show-toplevel 2>/dev/null) || return 1
  top=$(dw_canonical_dir "$top") || return 1
  [[ $top == "$path" ]] || return 1
  common=$(git -C "$path" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || return 1
  common=$(dw_canonical_dir "$common") || return 1
  [[ $common == "$path/.git" ]] || return 1
  count=$(git -C "$path" worktree list --porcelain 2>/dev/null | grep -c '^worktree ' || true)
  [[ $count == 1 ]]
}

dw_clone_fingerprint() {
  local path=$1 primary_identity clone_identity target listing remote_tags ref tip commit
  dw_is_standalone_clone "$path" || return 1
  primary_identity=$(dw_repository_identity "$MAIN_TOP") || return 1
  clone_identity=$(dw_repository_identity "$path") || return 1
  [[ -n $primary_identity && $clone_identity == "$primary_identity" ]] || return 1
  git -C "$path" fetch --quiet origin master || return 1
  target=$(git -C "$path" rev-parse --verify refs/remotes/origin/master 2>/dev/null) || return 1
  [[ -z $(git -C "$path" status --porcelain 2>/dev/null) ]] || return 1
  [[ -z $(git -C "$path" stash list 2>/dev/null) ]] || return 1
  git -C "$path" symbolic-ref --quiet HEAD >/dev/null 2>&1 || return 1
  dw_git_operation_in_progress "$path" && return 1

  listing=$(git -C "$path" for-each-ref --format='%(refname)%09%(objectname)' refs/heads 2>/dev/null) \
    || return 1
  while IFS=$'\t' read -r ref tip; do
    [[ -n $ref && -n $tip ]] || continue
    git -C "$path" merge-base --is-ancestor "$tip" "$target" 2>/dev/null || return 1
  done <<<"$listing"

  remote_tags=$(git -C "$path" ls-remote --tags origin 2>/dev/null) || return 1
  listing=$(git -C "$path" for-each-ref --format='%(refname)%09%(objectname)' refs/tags 2>/dev/null) \
    || return 1
  while IFS=$'\t' read -r ref tip; do
    [[ -n $ref && -n $tip ]] || continue
    grep -Fqx -- "$tip"$'\t'"$ref" <<<"$remote_tags" || return 1
    commit=$(git -C "$path" rev-parse "$ref^{commit}" 2>/dev/null) || return 1
    git -C "$path" merge-base --is-ancestor "$commit" "$target" 2>/dev/null || return 1
  done <<<"$listing"

  {
    printf 'clone\n%s\n%s\n' "$path" "$target"
    git -C "$path" for-each-ref --format='%(refname)%09%(objectname)' refs/heads refs/tags
  } | git hash-object --stdin
}

dw_clone_path_is_safe() {
  local path=$1 canonical parent base primary_identity clone_identity
  dw_safe_scalar "$path" || return 1
  [[ -d $path && ! -L $path ]] || return 1
  canonical=$(dw_canonical_dir "$path") || return 1
  [[ $canonical == "$path" ]] || return 1
  case "$canonical" in
    /|"${HOME:?}"|"$MAIN_TOP"|"$STATE_DIR"|"$STATE_DIR"/*) return 1 ;;
  esac
  parent=$(dirname -- "$canonical")
  base=$(basename -- "$canonical")
  [[ -n $base && $base != . && $base != .. && -d $parent ]] || return 1
  grep -Fqx -- "$canonical" "$CLONES_FILE" || return 1
  dw_is_standalone_clone "$canonical" || return 1
  primary_identity=$(dw_repository_identity "$MAIN_TOP") || return 1
  clone_identity=$(dw_repository_identity "$canonical") || return 1
  [[ -n $primary_identity && $clone_identity == "$primary_identity" ]]
}

dw_remove_clone() {
  local path=$1
  dw_clone_path_is_safe "$path" || return 1
  rm -rf -- "$path"
  [[ ! -e $path && ! -L $path ]]
}

dw_scan_clones() {
  local now=$1 next_registry path canonical fingerprint confirmed size
  next_registry=$(dw_make_temp clones)
  : >"$next_registry"
  while IFS= read -r path || [[ -n $path ]]; do
    [[ -n $path ]] || continue
    if ! dw_safe_scalar "$path" || [[ ! -d $path || -L $path ]]; then
      dw_log "dropping unavailable or unsafe clone registration: $path"
      continue
    fi
    canonical=$(dw_canonical_dir "$path") || { printf '%s\n' "$path" >>"$next_registry"; continue; }
    if [[ $canonical != "$path" ]]; then
      dw_log "preserving clone whose canonical path changed: $path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    if dw_path_is_active "$path"; then
      dw_log "preserving active registered clone: $path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    fingerprint=$(dw_clone_fingerprint "$path" 2>/dev/null) || {
      printf '%s\n' "$path" >>"$next_registry"
      continue
    }
    if ! dw_candidate_is_stable clone "$fingerprint" "$path" "$now"; then
      dw_log "observing fully merged registered clone before deletion: $path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    if (( DRY_RUN == 1 )); then
      dw_log "dry run: would delete fully merged registered clone $path"
      dw_record_candidate clone "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    dw_capture_active_paths "$ACTIVE_PATHS_FILE"
    if dw_path_is_active "$path" || dw_git_operation_in_progress "$path"; then
      dw_log "preserving clone that became active during validation: $path"
      dw_record_candidate clone "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    confirmed=$(dw_clone_fingerprint "$path" 2>/dev/null || printf '')
    if [[ -z $confirmed || $confirmed != "$fingerprint" ]]; then
      dw_log "preserving clone that changed during validation: $path"
      printf '%s\n' "$path" >>"$next_registry"
      continue
    fi
    size=$(dw_size_kib "$path")
    if dw_remove_clone "$path"; then
      dw_log "freed approximately ${size} KiB by deleting registered merged clone: $path"
    else
      dw_log "WARNING: final clone validation or deletion failed; preserving registration: $path"
      dw_record_candidate clone "$fingerprint" "$CANDIDATE_FIRST_SEEN" "$path"
      printf '%s\n' "$path" >>"$next_registry"
    fi
  done <"$CLONES_FILE"
  chmod 600 "$next_registry"
  mv -f -- "$next_registry" "$CLONES_FILE"
}

dw_once() {
  local report_file doctor_errors now
  dw_resolve_repository "$REPO_PATH"
  dw_prepare_state
  dw_acquire_run_lock
  printf '%s\n' "$MAIN_TOP" >"$REPO_FILE"
  chmod 600 "$REPO_FILE"

  report_file=$(dw_make_temp doctor)
  doctor_errors=$(dw_make_temp doctor-errors)
  ACTIVE_PATHS_FILE=$(dw_make_temp active-paths)
  NEXT_CANDIDATES_FILE=$(dw_make_temp candidates)
  : >"$NEXT_CANDIDATES_FILE"
  if ! (cd / && "$MANAGER" doctor --grace-hours 0) >"$report_file" 2>"$doctor_errors"; then
    sed 's/^/[DELETE_WORKTREE] /' "$doctor_errors" >&2 || true
    rm -f -- "$report_file" "$doctor_errors" "$ACTIVE_PATHS_FILE" "$NEXT_CANDIDATES_FILE"
    dw_die 'managed worktree inspection failed; nothing was deleted'
  fi
  sed 's/^/[DELETE_WORKTREE] /' "$doctor_errors" >&2 || true
  dw_capture_active_paths "$ACTIVE_PATHS_FILE"
  now=$(date +%s)
  dw_scan_worktrees "$report_file" "$now"
  dw_scan_clones "$now"
  chmod 600 "$NEXT_CANDIDATES_FILE"
  mv -f -- "$NEXT_CANDIDATES_FILE" "$CANDIDATES_FILE"
  rm -f -- "$report_file" "$doctor_errors" "$ACTIVE_PATHS_FILE"
  dw_log 'cleanup pass complete'
}

dw_xml_escape() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
}

dw_render_plist() {
  local script_xml repo_xml state_xml log_xml error_log_xml
  script_xml=$(printf '%s' "$MAIN_TOP/deploy/DELETE_WORKTREE.sh" | dw_xml_escape)
  repo_xml=$(printf '%s' "$MAIN_TOP" | dw_xml_escape)
  state_xml=$(printf '%s' "$STATE_DIR" | dw_xml_escape)
  log_xml=$(printf '%s' "$STATE_DIR/DELETE_WORKTREE.log" | dw_xml_escape)
  error_log_xml=$(printf '%s' "$STATE_DIR/DELETE_WORKTREE.error.log" | dw_xml_escape)
  cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$script_xml</string>
    <string>daemon</string>
    <string>--repo</string>
    <string>$repo_xml</string>
    <string>--state-dir</string>
    <string>$state_xml</string>
    <string>--interval-seconds</string>
    <string>$INTERVAL_SECONDS</string>
    <string>--settle-seconds</string>
    <string>$SETTLE_SECONDS</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>ThrottleInterval</key>
  <integer>5</integer>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StandardOutPath</key>
  <string>$log_xml</string>
  <key>StandardErrorPath</key>
  <string>$error_log_xml</string>
</dict>
</plist>
PLIST
}

dw_install() {
  local plist_dir plist_path staged domain
  [[ $(uname -s) == Darwin ]] || dw_die 'install is supported only on macOS launchd'
  command -v launchctl >/dev/null 2>&1 || dw_die 'launchctl is unavailable'
  dw_resolve_repository "$REPO_PATH"
  dw_prepare_state
  [[ -x $MAIN_TOP/deploy/DELETE_WORKTREE.sh ]] \
    || dw_die 'install must run after DELETE_WORKTREE is present in the primary worktree'
  if (( DRY_RUN == 1 )); then
    dw_render_plist
    return 0
  fi
  printf '%s\n' "$MAIN_TOP" >"$REPO_FILE"
  chmod 600 "$REPO_FILE"
  plist_dir=${HOME:?HOME is required}/Library/LaunchAgents
  mkdir -p -- "$plist_dir"
  plist_path=$plist_dir/$LABEL.plist
  staged=$(mktemp "$plist_dir/$LABEL.plist.XXXXXX")
  dw_render_plist >"$staged"
  plutil -lint "$staged" >/dev/null || { rm -f -- "$staged"; dw_die 'generated launchd plist is invalid'; }
  chmod 644 "$staged"
  mv -f -- "$staged" "$plist_path"
  domain=gui/$(id -u)
  launchctl bootout "$domain/$LABEL" >/dev/null 2>&1 || true
  launchctl bootstrap "$domain" "$plist_path"
  launchctl kickstart -k "$domain/$LABEL"
  dw_log "installed and started $LABEL; state=$STATE_DIR interval=${INTERVAL_SECONDS}s settle=${SETTLE_SECONDS}s"
}

dw_uninstall() {
  local plist_path domain
  [[ $(uname -s) == Darwin ]] || dw_die 'uninstall is supported only on macOS launchd'
  plist_path=${HOME:?HOME is required}/Library/LaunchAgents/$LABEL.plist
  domain=gui/$(id -u)
  launchctl bootout "$domain/$LABEL" >/dev/null 2>&1 || true
  rm -f -- "$plist_path"
  dw_log "uninstalled $LABEL; state and clone allow-list were preserved at $STATE_DIR"
}

dw_daemon() {
  local stopped=0
  trap 'stopped=1' INT TERM HUP
  dw_log "daemon started: interval=${INTERVAL_SECONDS}s settle=${SETTLE_SECONDS}s"
  while (( stopped == 0 )); do
    "$SCRIPT_PATH" once --repo "$REPO_PATH" --state-dir "$STATE_DIR" \
      --settle-seconds "$SETTLE_SECONDS" || dw_log 'WARNING: cleanup pass failed; retrying'
    (( stopped == 0 )) || break
    sleep "$INTERVAL_SECONDS" &
    wait $! || true
  done
  dw_log 'daemon stopped'
}

dw_register_clone() {
  local requested=$1 path primary_identity clone_identity staged ignored
  dw_resolve_repository "$REPO_PATH"
  dw_prepare_state
  dw_safe_scalar "$requested" || dw_die 'clone path contains a forbidden control character'
  path=$(dw_canonical_dir "$requested") || dw_die "clone path is unavailable or is a symlink: $requested"
  [[ $path != "$MAIN_TOP" && $path != "${HOME:?}" && $path != / ]] \
    || dw_die "refusing to register protected path: $path"
  dw_is_standalone_clone "$path" || dw_die "not a standalone single-worktree clone: $path"
  primary_identity=$(dw_repository_identity "$MAIN_TOP") || dw_die 'primary origin identity is unavailable'
  clone_identity=$(dw_repository_identity "$path") || dw_die 'clone origin identity is unavailable'
  [[ -n $primary_identity && $clone_identity == "$primary_identity" ]] \
    || dw_die 'clone origin does not match the primary repository'
  ignored=$(git -C "$path" status --porcelain --ignored 2>/dev/null | sed -n 's/^!! //p')
  if [[ -n $ignored && $ALLOW_IGNORED != 1 ]]; then
    dw_die 'clone contains ignored files; review them and repeat register-clone with --allow-ignored to authorize their eventual deletion'
  fi
  if grep -Fqx -- "$path" "$CLONES_FILE"; then
    dw_log "clone is already registered: $path"
    return 0
  fi
  staged=$(dw_make_temp clones)
  cat "$CLONES_FILE" >"$staged"
  printf '%s\n' "$path" >>"$staged"
  chmod 600 "$staged"
  mv -f -- "$staged" "$CLONES_FILE"
  dw_log "registered standalone clone for fail-closed cleanup: $path"
}

dw_unregister_clone() {
  local requested=$1 path staged
  dw_prepare_state
  dw_safe_scalar "$requested" || dw_die 'clone path contains a forbidden control character'
  if [[ -d $requested && ! -L $requested ]]; then
    path=$(dw_canonical_dir "$requested") || path=$requested
  else
    path=$requested
  fi
  staged=$(dw_make_temp clones)
  awk -v path="$path" '$0 != path' "$CLONES_FILE" >"$staged"
  chmod 600 "$staged"
  mv -f -- "$staged" "$CLONES_FILE"
  dw_log "unregistered clone without deleting it: $path"
}

dw_status() {
  local domain count=0
  dw_resolve_repository "$REPO_PATH"
  dw_prepare_state
  [[ ! -f $CLONES_FILE ]] || count=$(awk 'NF {count++} END {print count+0}' "$CLONES_FILE")
  printf 'label\t%s\nrepo\t%s\nstate\t%s\nregistered_clones\t%s\n' \
    "$LABEL" "$MAIN_TOP" "$STATE_DIR" "$count"
  if [[ $(uname -s) == Darwin ]]; then
    domain=gui/$(id -u)/$LABEL
    if launchctl print "$domain" >/dev/null 2>&1; then
      printf 'launchd\trunning\n'
    else
      printf 'launchd\tnot-loaded\n'
    fi
  fi
  (cd / && "$MANAGER" doctor --grace-hours 0)
}

REPO_PATH=$SCRIPT_ROOT
STATE_DIR=$(dw_default_state_dir)
INTERVAL_SECONDS=$DEFAULT_INTERVAL_SECONDS
SETTLE_SECONDS=$DEFAULT_SETTLE_SECONDS
DRY_RUN=0
ALLOW_IGNORED=0

command_name=${1:-}
[[ -n $command_name ]] || { dw_usage; exit 2; }
shift
positionals=()
while (( $# > 0 )); do
  case "$1" in
    --repo)
      shift; (( $# > 0 )) || { dw_usage; exit 2; }; REPO_PATH=$1 ;;
    --state-dir)
      shift; (( $# > 0 )) || { dw_usage; exit 2; }; STATE_DIR=$1 ;;
    --interval-seconds)
      shift; (( $# > 0 )) || { dw_usage; exit 2; }; INTERVAL_SECONDS=$1 ;;
    --settle-seconds)
      shift; (( $# > 0 )) || { dw_usage; exit 2; }; SETTLE_SECONDS=$1 ;;
    --dry-run) DRY_RUN=1 ;;
    --allow-ignored) ALLOW_IGNORED=1 ;;
    -h|--help) dw_usage; exit 0 ;;
    -*) dw_usage; exit 2 ;;
    *) positionals+=("$1") ;;
  esac
  shift
done

dw_validate_interval "$INTERVAL_SECONDS"
dw_validate_settle "$SETTLE_SECONDS"
dw_safe_scalar "$STATE_DIR" || dw_die 'state directory contains a forbidden control character'
if (( ALLOW_IGNORED == 1 )) && [[ $command_name != register-clone ]]; then
  dw_usage
  exit 2
fi

case "$command_name" in
  install)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_install
    ;;
  uninstall)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_uninstall
    ;;
  once)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_once
    ;;
  daemon)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_daemon
    ;;
  register-clone)
    (( ${#positionals[@]} == 1 )) || { dw_usage; exit 2; }
    dw_register_clone "${positionals[0]}"
    ;;
  unregister-clone)
    (( ${#positionals[@]} == 1 )) || { dw_usage; exit 2; }
    dw_unregister_clone "${positionals[0]}"
    ;;
  status)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_status
    ;;
  render-plist)
    (( ${#positionals[@]} == 0 )) || { dw_usage; exit 2; }
    dw_resolve_repository "$REPO_PATH"
    dw_prepare_state
    dw_render_plist
    ;;
  help)
    dw_usage
    ;;
  *)
    dw_usage
    exit 2
    ;;
esac
