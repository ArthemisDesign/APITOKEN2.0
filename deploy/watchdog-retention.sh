#!/usr/bin/env bash
set -euo pipefail

# Bounded retention for per-deployment PostgreSQL dumps.
#
# The backup root is root-only (0700) and deliberately unreadable by the deploy operator, so both
# selection and deletion happen here rather than in the watchdog. The hourly `<database>.dump`
# rotation artifacts are the authoritative recovery objects and are NEVER touched; only the
# `<database>.pre-deploy-<sha>.dump` snapshots, which accumulate once per deployment forever, are
# subject to retention. Their `.pre-deploy-<sha>.complete` markers are removed with them.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# Prefer the library shipped alongside this script. Both are installed together from the same tested
# candidate, so the sibling copy is always the matching version; falling back to the installed path
# first would silently pair a new script with an older library.
LIB=$SCRIPT_DIR/watchdog-lib.sh
[[ -r $LIB ]] || LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not available\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"
# shellcheck source=deploy/contour-config.sh
source "${LIB%/*}/contour-config.sh"

BACKUP_ROOT=${WATCHDOG_BACKUP_ROOT:-$CONTOUR_ROOTS_BACKUP}

[[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "dump retention must run as root"
[[ $# -eq 1 ]] || wd_die "usage: $0 <keep-count>"
KEEP=$1
[[ $KEEP =~ ^[0-9]+$ ]] || wd_die "keep count must be a non-negative integer"

[[ -d $BACKUP_ROOT && ! -L $BACKUP_ROOT ]] || wd_die "backup root is missing: $BACKUP_ROOT"

removed=0
failed=0
# Selection runs in a process substitution, where `set -e` cannot propagate a failure to this shell:
# a broken selector would read as "nothing to prune" and exit 0. Materialise the NUL-delimited list
# to a private file first so a failed selection aborts before anything is deleted. (Command
# substitution cannot be used here — shell variables cannot carry NUL bytes.)
selection=$(mktemp)
trap 'rm -f -- "$selection"' EXIT
wd_prunable_predeploy_dumps "$BACKUP_ROOT" "$KEEP" >"$selection" \
  || wd_die "pre-deploy dump selection failed; nothing was removed"

while IFS= read -r -d '' dump; do
  # wd_prunable_predeploy_dumps already restricts output to regular pre-deploy files directly under
  # the backup root. Revalidate at the destructive boundary as defence in depth.
  name=${dump##*/}
  if [[ ${dump%/*} != "$BACKUP_ROOT" || ! -f $dump || -L $dump || $name != *.pre-deploy-*.dump ]]; then
    wd_warn "unsafe dump retention target skipped: $dump"
    failed=$((failed + 1))
    continue
  fi
  sha=${name##*.pre-deploy-}
  sha=${sha%.dump}
  if [[ ! $sha =~ ^[0-9a-f]{40}$ ]]; then
    wd_warn "pre-deploy dump has no valid SHA component; skipped: $name"
    failed=$((failed + 1))
    continue
  fi
  if rm -f -- "$dump"; then
    removed=$((removed + 1))
    # The completion marker is shared by all databases of one deployment. Remove it only once the
    # last dump referencing that SHA is gone, so a partially pruned deployment is never marked
    # complete-but-missing.
    if ! compgen -G "$BACKUP_ROOT/*.pre-deploy-$sha.dump" >/dev/null; then
      rm -f -- "$BACKUP_ROOT/.pre-deploy-$sha.complete"
    fi
  else
    wd_warn "failed to remove expired pre-deploy dump $name"
    failed=$((failed + 1))
  fi
done <"$selection"

if (( removed > 0 || failed > 0 )); then
  wd_log "pre-deploy dump retention finished: removed=$removed failed=$failed keep=$KEEP"
fi
