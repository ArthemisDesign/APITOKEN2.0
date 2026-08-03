#!/usr/bin/env bash
# Managed lifecycle for contributor/agent worktrees.
#
# create  — make one task branch/worktree from fresh origin/master and mark its creation time;
# finish  — remove one explicitly selected clean worktree only after its branch reached origin/master;
# doctor  — refresh origin and report stale, protected, dirty, unmerged and eligible worktrees
#           without deleting or rewriting local worktrees and branches;
# gc      — dry-run by default; with --apply, prune missing registrations, old clean merged
#           worktrees, and old merged local branch refs while preserving all unique commits.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
REMOTE=origin
TARGET_REF=origin/master
DEFAULT_GRACE_HOURS=${AGENT_WORKTREE_GRACE_HOURS:-24}
MANAGED_MARKER=agent-worktree-managed-v1
LOCK_HELD=0
LOCK_DIR=
LOCK_HOST=

aw_log() { printf '[agent-worktree] %s\n' "$*" >&2; }
aw_die() { aw_log "ERROR: $*"; exit 1; }

aw_print_usage() {
  cat >&2 <<'USAGE'
usage:
  deploy/agent-worktree.sh create <type/task> [worktree-name]
  deploy/agent-worktree.sh finish [--dry-run] [worktree-path]
  deploy/agent-worktree.sh doctor [--grace-hours N]
  deploy/agent-worktree.sh gc [--apply] [--grace-hours N]

create writes under ${AGENT_WORKTREE_ROOT:-$HOME/wt}. gc never mutates without --apply.
USAGE
}

aw_usage() {
  aw_print_usage
  exit 2
}

git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1 \
  || aw_die 'the lifecycle script must run from a Git worktree'
COMMON_DIR=$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
  || aw_die 'cannot resolve the repository git-common-dir'
COMMON_DIR=$(cd -- "$COMMON_DIR" && pwd -P)
MAIN_TOP=$(git -C "$ROOT" worktree list --porcelain | sed -n '1s/^worktree //p')
[[ -n $MAIN_TOP && -d $MAIN_TOP ]] || aw_die 'cannot resolve the primary worktree'
MAIN_TOP=$(cd -- "$MAIN_TOP" && pwd -P)

CURRENT_TOP=$ROOT
if caller_top=$(git -C "${PWD:-$ROOT}" rev-parse --show-toplevel 2>/dev/null); then
  caller_common=$(git -C "$caller_top" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || printf '')
  if [[ -n $caller_common ]]; then
    caller_common=$(cd -- "$caller_common" 2>/dev/null && pwd -P || printf '')
    [[ $caller_common == "$COMMON_DIR" ]] && CURRENT_TOP=$(cd -- "$caller_top" && pwd -P)
  fi
fi

aw_now() { date +%s; }

aw_grace_seconds() {
  local hours=$1 normalized=$1
  [[ $hours =~ ^[0-9]+$ ]] \
    || aw_die 'grace hours must be a non-negative integer'
  while [[ ${#normalized} -gt 1 && $normalized == 0* ]]; do
    normalized=${normalized#0}
  done
  # Bound arithmetic input so a typo cannot wrap a positive retention window into an immediate
  # cleanup. Nine decimal digits already permit more than 100,000 years.
  (( ${#normalized} <= 9 )) \
    || aw_die 'grace hours are too large'
  printf '%s\n' "$((10#$normalized * 3600))"
}

aw_mtime() {
  local value
  value=$(stat -c '%Y' -- "$1" 2>/dev/null || printf '')
  [[ $value =~ ^[0-9]+$ ]] || value=$(stat -f '%m' -- "$1" 2>/dev/null || printf '')
  [[ $value =~ ^[0-9]+$ ]] || value=0
  printf '%s\n' "$value"
}

aw_is_protected_branch() {
  case "$1" in
    master|main|comp/*) return 0 ;;
    *) return 1 ;;
  esac
}

aw_fetch() {
  git -C "$ROOT" fetch --quiet "$REMOTE" \
    || aw_die "cannot refresh $TARGET_REF; refusing lifecycle decisions from stale remote state"
  git -C "$ROOT" rev-parse --verify --quiet "$TARGET_REF" >/dev/null \
    || aw_die "$TARGET_REF is unavailable"
}

aw_fetch_for_report() {
  if ! git -C "$ROOT" fetch --quiet "$REMOTE"; then
    aw_log "WARNING: fetch failed; report is comparing against the on-disk $TARGET_REF"
  fi
  git -C "$ROOT" rev-parse --verify --quiet "$TARGET_REF" >/dev/null \
    || aw_die "$TARGET_REF is unavailable"
}

aw_release_lock() {
  local owner_pid owner_host
  (( LOCK_HELD == 1 )) || return 0
  owner_pid=$(cat "$LOCK_DIR/pid" 2>/dev/null || printf '')
  owner_host=$(cat "$LOCK_DIR/host" 2>/dev/null || printf '')
  if [[ $owner_pid == "$$" && $owner_host == "$LOCK_HOST" ]]; then
    rm -f -- "$LOCK_DIR/pid" "$LOCK_DIR/host"
    rmdir "$LOCK_DIR" 2>/dev/null || true
  fi
  LOCK_HELD=0
}

aw_acquire_lock() {
  local attempt=0 owner_pid owner_host age now modified
  LOCK_DIR="$COMMON_DIR/agent-worktree-manager.lock"
  LOCK_HOST=$(hostname)
  while (( attempt < 300 )); do
    if mkdir "$LOCK_DIR" 2>/dev/null; then
      printf '%s\n' "$$" >"$LOCK_DIR/pid"
      printf '%s\n' "$LOCK_HOST" >"$LOCK_DIR/host"
      LOCK_HELD=1
      trap aw_release_lock EXIT
      trap 'exit 130' INT
      trap 'exit 143' TERM
      return 0
    fi

    owner_pid=$(cat "$LOCK_DIR/pid" 2>/dev/null || printf '')
    owner_host=$(cat "$LOCK_DIR/host" 2>/dev/null || printf '')
    now=$(aw_now)
    modified=$(aw_mtime "$LOCK_DIR")
    age=$(( now > modified ? now - modified : 0 ))
    if (( age > 60 )) \
      && { [[ -z $owner_pid || -z $owner_host ]] \
        || { [[ $owner_host == "$LOCK_HOST" ]] && ! kill -0 "$owner_pid" 2>/dev/null; }; }; then
      aw_log "breaking stale lifecycle lock (${age}s old, owner ${owner_pid:-unknown}@${owner_host:-unknown})"
      rm -f -- "$LOCK_DIR/pid" "$LOCK_DIR/host"
      rmdir "$LOCK_DIR" 2>/dev/null || true
      attempt=$((attempt + 1))
      continue
    fi

    attempt=$((attempt + 1))
    sleep 0.1
  done
  aw_die 'another lifecycle operation held the manager lock for 30 seconds'
}

aw_admin_dir() {
  git -C "$1" rev-parse --path-format=absolute --git-dir 2>/dev/null
}

aw_write_marker() {
  local worktree=$1 branch=$2 base=$3 admin marker staged owner
  admin=$(aw_admin_dir "$worktree") || return 1
  marker="$admin/$MANAGED_MARKER"
  staged="$marker.tmp.$$.$RANDOM"
  owner=${AGENT_WORKTREE_OWNER:-${USER:-unknown}}
  case "$owner" in
    *$'\n'*|*$'\r'*) owner=unknown ;;
  esac
  umask 077
  {
    printf 'version=2\n'
    printf 'created=%s\n' "$(aw_now)"
    printf 'owner=%s\n' "$owner"
    printf 'branch=%s\n' "$branch"
    printf 'path=%s\n' "$worktree"
    printf 'base=%s\n' "$base"
  } >"$staged" || return 1
  mv -f -- "$staged" "$marker"
}

aw_marker_value() {
  local worktree=$1 key=$2 admin marker
  admin=$(aw_admin_dir "$worktree" 2>/dev/null) || return 1
  marker="$admin/$MANAGED_MARKER"
  [[ -f $marker ]] || return 1
  sed -n "s/^${key}=//p" "$marker" | head -1
}

aw_managed_creation_state() {
  local worktree=$1 branch=$2 admin marker version base marker_branch marker_path current_head
  admin=$(aw_admin_dir "$worktree" 2>/dev/null) || { printf 'invalid\n'; return 0; }
  marker="$admin/$MANAGED_MARKER"
  [[ -f $marker ]] || { printf 'legacy\n'; return 0; }
  version=$(aw_marker_value "$worktree" version 2>/dev/null || printf '')
  case "$version" in
    1)
      # Version 1 predates creation-base tracking. Preserve its established cleanup behavior so
      # already completed worktrees do not become permanently ineligible after this upgrade.
      printf 'legacy\n'
      ;;
    2)
      base=$(aw_marker_value "$worktree" base 2>/dev/null || printf '')
      marker_branch=$(aw_marker_value "$worktree" branch 2>/dev/null || printf '')
      marker_path=$(aw_marker_value "$worktree" path 2>/dev/null || printf '')
      current_head=$(git -C "$ROOT" rev-parse "refs/heads/$branch" 2>/dev/null || printf '')
      if [[ $marker_branch != "$branch" || $marker_path != "$worktree" ]] \
        || { [[ ! $base =~ ^[0-9a-f]{40}$ ]] && [[ ! $base =~ ^[0-9a-f]{64}$ ]]; } \
        || [[ -z $current_head ]] \
        || [[ $(git -C "$ROOT" rev-parse --verify "$base^{commit}" 2>/dev/null || printf '') != "$base" ]]; then
        printf 'invalid\n'
      elif [[ $current_head == "$base" ]]; then
        printf 'unstarted\n'
      else
        printf 'started\n'
      fi
      ;;
    *)
      # Unknown or incomplete managed metadata must never make automatic deletion less strict.
      printf 'invalid\n'
      ;;
  esac
}

aw_activity_epoch() {
  local worktree=$1 branch=$2 latest=0 value admin marker
  value=$(aw_mtime "$worktree")
  (( value > latest )) && latest=$value
  value=$(git -C "$ROOT" log -1 --format='%ct' "refs/heads/$branch" 2>/dev/null || printf 0)
  [[ $value =~ ^[0-9]+$ ]] || value=0
  (( value > latest )) && latest=$value
  if admin=$(aw_admin_dir "$worktree" 2>/dev/null); then
    # `git status` may refresh the administrative index. Counting that mtime as user activity would
    # make every doctor/gc inspection renew the grace period and prevent old clean trees from ever
    # becoming eligible. The root directory, branch commit, and managed creation marker are stable.
    for marker in "$admin/$MANAGED_MARKER"; do
      [[ -e $marker ]] || continue
      value=$(aw_mtime "$marker")
      (( value > latest )) && latest=$value
    done
  fi
  value=$(aw_marker_value "$worktree" created 2>/dev/null || printf 0)
  [[ $value =~ ^[0-9]+$ ]] || value=0
  (( value > latest )) && latest=$value
  printf '%s\n' "$latest"
}

aw_grace_elapsed() {
  local worktree=$1 branch=$2 grace_seconds=$3 now activity
  (( grace_seconds == 0 )) && return 0
  now=$(aw_now)
  activity=$(aw_activity_epoch "$worktree" "$branch")
  (( activity > 0 && now >= activity && now - activity >= grace_seconds ))
}

aw_age_hours() {
  local worktree=$1 branch=$2 now activity
  now=$(aw_now)
  activity=$(aw_activity_epoch "$worktree" "$branch")
  if (( activity <= 0 || now < activity )); then
    printf '0'
  else
    printf '%s' "$(( (now - activity) / 3600 ))"
  fi
}

aw_worktree_lock_state() {
  local target=$1 listing line path='' locked=0
  if ! listing=$(git -C "$ROOT" worktree list --porcelain 2>/dev/null); then
    printf 'unknown\n'
    return 0
  fi
  while IFS= read -r line; do
    case "$line" in
      'worktree '*)
        if [[ $path == "$target" ]]; then
          (( locked == 1 )) && printf 'locked\n' || printf 'unlocked\n'
          return 0
        fi
        path=${line#worktree }
        locked=0
        ;;
      'locked'*) locked=1 ;;
    esac
  done <<<"$listing"
  if [[ $path == "$target" ]]; then
    (( locked == 1 )) && printf 'locked\n' || printf 'unlocked\n'
  else
    printf 'unknown\n'
  fi
}

aw_checked_out_branch() {
  local listing
  # A failed ownership check must preserve the branch. Returning success here means "treat it as
  # checked out" to the cleanup caller.
  listing=$(git -C "$ROOT" worktree list --porcelain 2>/dev/null) || return 0
  grep -Fqx "branch refs/heads/$1" <<<"$listing"
}

aw_branch_activity_epoch() {
  local branch=$1 value
  # Commit time is not branch age: a branch created today at an old merged commit must still receive
  # the full grace period. Local worktrees normally have reflogs; without one there is no reliable
  # branch-age signal, so preserve the ref for the default grace policy. An explicit grace of zero
  # can still clean it after the merged/unowned checks.
  value=$(git -C "$ROOT" reflog show -1 --date=unix --format='%gd' \
    "refs/heads/$branch" 2>/dev/null \
    | sed -n 's/.*@{\([0-9][0-9]*\)}$/\1/p' \
    || printf '')
  [[ $value =~ ^[0-9]+$ ]] || value=$(aw_now)
  printf '%s\n' "$value"
}

aw_create() {
  [[ $# -ge 1 && $# -le 2 ]] || aw_usage
  local branch=$1 name=${2:-} worktree_root target expected
  git check-ref-format --branch "$branch" >/dev/null 2>&1 \
    || aw_die "invalid branch name: $branch"
  aw_is_protected_branch "$branch" \
    && aw_die "protected branch cannot be a task worktree: $branch"
  [[ -n $name ]] || name=${branch//\//-}
  case "$name" in
    ''|.|..|*/*|*[!A-Za-z0-9._-]*)
      aw_die 'worktree-name must be one safe path segment (letters, digits, dot, underscore, dash)'
      ;;
  esac

  worktree_root=${AGENT_WORKTREE_ROOT:-${HOME:?HOME is required}/wt}
  aw_acquire_lock
  aw_fetch
  git -C "$ROOT" show-ref --verify --quiet "refs/heads/$branch" \
    && aw_die "local branch already exists: $branch"
  git -C "$ROOT" show-ref --verify --quiet "refs/remotes/$REMOTE/$branch" \
    && aw_die "remote branch already exists: $REMOTE/$branch"
  [[ ! -e $worktree_root || -d $worktree_root ]] \
    || aw_die "worktree root is not a directory: $worktree_root"
  mkdir -p -- "$worktree_root"
  worktree_root=$(cd -- "$worktree_root" && pwd -P)
  target="$worktree_root/$name"
  [[ ! -e $target && ! -L $target ]] || aw_die "worktree path already exists: $target"

  expected=$(git -C "$ROOT" rev-parse "$TARGET_REF")
  aw_log "creating $target on $branch from $TARGET_REF ($expected)"
  git -C "$ROOT" worktree add "$target" -b "$branch" "$TARGET_REF" >&2
  if ! aw_write_marker "$target" "$branch" "$expected"; then
    aw_log 'marker creation failed; rolling back the new empty worktree'
    git -C "$ROOT" worktree remove "$target" 2>/dev/null || true
    git -C "$ROOT" update-ref -d "refs/heads/$branch" "$expected" 2>/dev/null || true
    aw_die 'could not record managed worktree metadata'
  fi
  printf '%s\n' "$target"
}

aw_resolve_target() {
  local requested=${1:-$CURRENT_TOP} top target_common
  [[ -d $requested ]] || aw_die "worktree path does not exist: $requested"
  top=$(git -C "$requested" rev-parse --show-toplevel 2>/dev/null) \
    || aw_die "not a Git worktree: $requested"
  top=$(cd -- "$top" && pwd -P)
  target_common=$(git -C "$top" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
    || aw_die "cannot resolve git-common-dir for $top"
  target_common=$(cd -- "$target_common" && pwd -P)
  [[ $target_common == "$COMMON_DIR" ]] \
    || aw_die "worktree belongs to a different repository: $top"
  printf '%s\n' "$top"
}

aw_sync_primary() {
  local main_branch main_status
  main_branch=$(git -C "$MAIN_TOP" rev-parse --abbrev-ref HEAD 2>/dev/null || printf HEAD)
  main_status=$(git -C "$MAIN_TOP" status --porcelain 2>/dev/null || printf unavailable)
  if [[ $main_branch != master ]]; then
    aw_log "WARNING: primary worktree is on $main_branch, not master; skipping its fast-forward"
  elif [[ -n $main_status ]]; then
    aw_log 'WARNING: primary worktree is not clean; skipping its fast-forward'
  elif ! git -C "$MAIN_TOP" merge --ff-only "$TARGET_REF"; then
    aw_log "WARNING: primary master could not fast-forward to $TARGET_REF; cleanup continues"
  fi
}

aw_finish() {
  local dry_run=0 requested='' target branch expected status_output lock_state
  while (( $# > 0 )); do
    case "$1" in
      --dry-run) dry_run=1 ;;
      -*) aw_usage ;;
      *) [[ -z $requested ]] || aw_usage; requested=$1 ;;
    esac
    shift
  done
  target=$(aw_resolve_target "$requested")
  [[ $target != "$MAIN_TOP" ]] || aw_die 'refusing to finish the primary worktree'

  (( dry_run == 1 )) || aw_acquire_lock
  aw_fetch
  target=$(aw_resolve_target "$target")
  branch=$(git -C "$target" rev-parse --abbrev-ref HEAD)
  [[ $branch != HEAD ]] || aw_die 'detached worktrees cannot be finished automatically'
  aw_is_protected_branch "$branch" \
    && aw_die "protected branch cannot be finished automatically: $branch"
  lock_state=$(aw_worktree_lock_state "$target")
  [[ $lock_state == unlocked ]] \
    || aw_die "worktree lock state is $lock_state; refusing to finish: $target"
  status_output=$(git -C "$target" status --porcelain) \
    || aw_die "cannot inspect worktree status: $target"
  [[ -z $status_output ]] || aw_die "worktree is not clean: $target"
  git -C "$ROOT" merge-base --is-ancestor "refs/heads/$branch" "$TARGET_REF" \
    || aw_die "branch is not merged into $TARGET_REF: $branch"
  expected=$(git -C "$ROOT" rev-parse "refs/heads/$branch")

  if (( dry_run == 1 )); then
    aw_log "would remove $target and delete merged branch $branch"
    return 0
  fi

  aw_sync_primary
  cd -- "$MAIN_TOP"
  git -C "$MAIN_TOP" worktree remove "$target"
  if ! git -C "$MAIN_TOP" update-ref -d "refs/heads/$branch" "$expected"; then
    aw_die "worktree was removed, but branch changed concurrently and was preserved: $branch"
  fi
  aw_log "removed $target and deleted merged branch $branch"
  if [[ $target == "$CURRENT_TOP" ]]; then
    aw_log "caller was inside the removed worktree; continue from $MAIN_TOP"
  fi
}

RECORD_PATH=
RECORD_BRANCH=
RECORD_DETACHED=0
RECORD_LOCKED=0
CLASS_STATUS=
CLASS_DETAIL=
CLASS_ACTION=
GC_APPLY=0
GC_GRACE_SECONDS=0
GC_WORKTREE_CANDIDATES=0
GC_REGISTRATION_CANDIDATES=0
GC_REMOVED=0
GC_FAILED=0

aw_reset_record() {
  RECORD_PATH=
  RECORD_BRANCH=
  RECORD_DETACHED=0
  RECORD_LOCKED=0
}

aw_classify_record() {
  local status_output age owner creation_state creation_base
  CLASS_STATUS=
  CLASS_DETAIL=
  CLASS_ACTION=
  [[ -n $RECORD_PATH ]] || return 0

  if [[ $RECORD_PATH == "$MAIN_TOP" ]]; then
    CLASS_STATUS=PRIMARY
    CLASS_DETAIL=protected
  elif [[ $RECORD_PATH == "$CURRENT_TOP" ]]; then
    CLASS_STATUS=CURRENT
    CLASS_DETAIL=protected
  elif [[ ! -d $RECORD_PATH ]]; then
    if (( RECORD_LOCKED == 1 )); then
      CLASS_STATUS=MISSING_LOCKED
      CLASS_DETAIL='registration preserved'
    else
      CLASS_STATUS=STALE_REGISTRATION
      CLASS_DETAIL='branch will be preserved unless separately merged and expired'
      CLASS_ACTION=prune-registration
    fi
  elif (( RECORD_DETACHED == 1 )) || [[ -z $RECORD_BRANCH ]]; then
    CLASS_STATUS=DETACHED
    CLASS_DETAIL='manual review required'
  elif aw_is_protected_branch "$RECORD_BRANCH"; then
    CLASS_STATUS=PROTECTED_BRANCH
    CLASS_DETAIL=protected
  elif (( RECORD_LOCKED == 1 )); then
    CLASS_STATUS=LOCKED
    CLASS_DETAIL='explicitly protected'
  elif ! status_output=$(git -C "$RECORD_PATH" status --porcelain 2>/dev/null); then
    CLASS_STATUS=UNAVAILABLE
    CLASS_DETAIL='status check failed'
  elif [[ -n $status_output ]]; then
    CLASS_STATUS=DIRTY
    CLASS_DETAIL='never auto-removed'
  elif ! git -C "$ROOT" merge-base --is-ancestor "refs/heads/$RECORD_BRANCH" "$TARGET_REF" 2>/dev/null; then
    CLASS_STATUS=UNMERGED
    CLASS_DETAIL='unique commits preserved'
  else
    creation_state=$(aw_managed_creation_state "$RECORD_PATH" "$RECORD_BRANCH")
    case "$creation_state" in
      unstarted)
        creation_base=$(aw_marker_value "$RECORD_PATH" base 2>/dev/null || printf unknown)
        CLASS_STATUS=UNSTARTED
        CLASS_DETAIL="base=$creation_base; automatic cleanup waits for a task commit"
        ;;
      invalid)
        CLASS_STATUS=INVALID_METADATA
        CLASS_DETAIL='managed creation metadata is invalid; manual review required'
        ;;
      *)
        age=$(aw_age_hours "$RECORD_PATH" "$RECORD_BRANCH")
        owner=$(aw_marker_value "$RECORD_PATH" owner 2>/dev/null || printf legacy)
        if aw_grace_elapsed "$RECORD_PATH" "$RECORD_BRANCH" "$GC_GRACE_SECONDS"; then
          CLASS_STATUS=MERGED_CANDIDATE
          CLASS_DETAIL="age=${age}h owner=$owner"
          CLASS_ACTION=remove-worktree
        else
          CLASS_STATUS=RECENT_MERGED
          CLASS_DETAIL="age=${age}h owner=$owner grace=$((GC_GRACE_SECONDS / 3600))h"
        fi
        ;;
    esac
  fi
}

aw_doctor_record() {
  aw_classify_record
  printf '%-20s\t%s\t%s\t%s\n' \
    "$CLASS_STATUS" "${RECORD_BRANCH:-detached}" "$RECORD_PATH" "$CLASS_DETAIL"
}

aw_gc_record() {
  local expected status_output current_head lock_state
  aw_classify_record
  case "$CLASS_ACTION" in
    prune-registration)
      GC_REGISTRATION_CANDIDATES=$((GC_REGISTRATION_CANDIDATES + 1))
      if (( GC_APPLY == 0 )); then
        aw_log "would prune missing registration $RECORD_PATH [${RECORD_BRANCH:-detached}]"
      elif git -C "$ROOT" worktree remove "$RECORD_PATH"; then
        aw_log "pruned missing registration $RECORD_PATH; branch preserved"
        GC_REMOVED=$((GC_REMOVED + 1))
      else
        aw_log "WARNING: failed to prune registration $RECORD_PATH"
        GC_FAILED=$((GC_FAILED + 1))
      fi
      ;;
    remove-worktree)
      GC_WORKTREE_CANDIDATES=$((GC_WORKTREE_CANDIDATES + 1))
      if (( GC_APPLY == 0 )); then
        aw_log "would remove old clean merged worktree $RECORD_PATH [$RECORD_BRANCH];" \
          'its unchanged merged branch would then become eligible for deletion'
        return 0
      fi
      expected=$(git -C "$ROOT" rev-parse "refs/heads/$RECORD_BRANCH" 2>/dev/null || printf '')
      current_head=$(git -C "$RECORD_PATH" rev-parse HEAD 2>/dev/null || printf '')
      status_output=$(git -C "$RECORD_PATH" status --porcelain 2>/dev/null || printf unavailable)
      lock_state=$(aw_worktree_lock_state "$RECORD_PATH")
      if [[ -z $expected || $current_head != "$expected" || -n $status_output ]] \
        || [[ $lock_state != unlocked ]] \
        || ! git -C "$ROOT" merge-base --is-ancestor "$expected" "$TARGET_REF" 2>/dev/null; then
        aw_log "WARNING: $RECORD_PATH changed after classification; preserving it"
        GC_FAILED=$((GC_FAILED + 1))
      elif git -C "$ROOT" worktree remove "$RECORD_PATH"; then
        aw_log "removed old clean merged worktree $RECORD_PATH; branch cleanup follows"
        GC_REMOVED=$((GC_REMOVED + 1))
      else
        aw_log "WARNING: failed to remove $RECORD_PATH"
        GC_FAILED=$((GC_FAILED + 1))
      fi
      ;;
  esac
}

aw_scan_worktrees() {
  local callback=$1 listing line
  listing=$(git -C "$ROOT" worktree list --porcelain 2>/dev/null) \
    || aw_die 'cannot enumerate repository worktrees'
  aw_reset_record
  while IFS= read -r line; do
    case "$line" in
      'worktree '*)
        if [[ -n $RECORD_PATH ]]; then
          "$callback"
          aw_reset_record
        fi
        RECORD_PATH=${line#worktree }
        ;;
      'branch refs/heads/'*) RECORD_BRANCH=${line#branch refs/heads/} ;;
      detached) RECORD_DETACHED=1 ;;
      'locked'*) RECORD_LOCKED=1 ;;
    esac
  done <<<"$listing"
  [[ -z $RECORD_PATH ]] || "$callback"
}

aw_branch_candidates() {
  local apply=$1 grace_seconds=$2 listing branch expected activity now age
  local candidates=0 removed=0 failed=0
  listing=$(git -C "$ROOT" for-each-ref --merged="$TARGET_REF" \
    --format='%(refname:short)%09%(objectname)' refs/heads 2>/dev/null) \
    || aw_die "cannot enumerate local branches merged into $TARGET_REF"
  now=$(aw_now)
  while IFS=$'\t' read -r branch expected; do
    [[ -n $branch && -n $expected ]] || continue
    aw_is_protected_branch "$branch" && continue
    aw_checked_out_branch "$branch" && continue
    activity=$(aw_branch_activity_epoch "$branch")
    age=$(( now > activity ? now - activity : 0 ))
    (( grace_seconds == 0 || age >= grace_seconds )) || continue
    candidates=$((candidates + 1))
    if (( apply == 0 )); then
      aw_log "would delete old merged local branch $branch"
    elif git -C "$ROOT" update-ref -d "refs/heads/$branch" "$expected"; then
      aw_log "deleted old merged local branch $branch"
      removed=$((removed + 1))
    else
      aw_log "WARNING: branch changed concurrently and was preserved: $branch"
      failed=$((failed + 1))
    fi
  done <<<"$listing"
  BRANCH_CANDIDATES=$candidates
  BRANCH_REMOVED=$removed
  BRANCH_FAILED=$failed
}

aw_doctor() {
  local grace_hours=$DEFAULT_GRACE_HOURS
  while (( $# > 0 )); do
    case "$1" in
      --grace-hours)
        shift
        (( $# > 0 )) || aw_usage
        grace_hours=$1
        ;;
      *) aw_usage ;;
    esac
    shift
  done
  GC_GRACE_SECONDS=$(aw_grace_seconds "$grace_hours")
  aw_fetch_for_report
  printf 'STATUS\tBRANCH\tPATH\tDETAIL\n'
  aw_scan_worktrees aw_doctor_record
  aw_branch_candidates 0 "$GC_GRACE_SECONDS" >/dev/null
  aw_log "doctor complete: $BRANCH_CANDIDATES old merged local branch candidate(s); no changes made"
}

aw_gc() {
  local grace_hours=$DEFAULT_GRACE_HOURS
  GC_APPLY=0
  while (( $# > 0 )); do
    case "$1" in
      --apply) GC_APPLY=1 ;;
      --dry-run) GC_APPLY=0 ;;
      --grace-hours)
        shift
        (( $# > 0 )) || aw_usage
        grace_hours=$1
        ;;
      *) aw_usage ;;
    esac
    shift
  done
  GC_GRACE_SECONDS=$(aw_grace_seconds "$grace_hours")
  if (( GC_APPLY == 1 )); then
    aw_acquire_lock
    aw_fetch
  else
    aw_fetch_for_report
    aw_log 'dry-run only; pass --apply to perform the reported cleanup'
  fi

  aw_scan_worktrees aw_gc_record
  aw_branch_candidates "$GC_APPLY" "$GC_GRACE_SECONDS"
  aw_log "gc summary: registrations=$GC_REGISTRATION_CANDIDATES worktrees=$GC_WORKTREE_CANDIDATES branches=$BRANCH_CANDIDATES removed=$((GC_REMOVED + BRANCH_REMOVED)) failed=$((GC_FAILED + BRANCH_FAILED))"
  (( GC_FAILED + BRANCH_FAILED == 0 ))
}

command_name=${1:-}
[[ -n $command_name ]] || aw_usage
shift
case "$command_name" in
  create) aw_create "$@" ;;
  finish) aw_finish "$@" ;;
  doctor) aw_doctor "$@" ;;
  gc) aw_gc "$@" ;;
  -h|--help|help) aw_print_usage ;;
  *) aw_usage ;;
esac
