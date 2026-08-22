#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"
CONTOUR_ROOT=${LIB%/*}
# shellcheck source=deploy/contour-config.sh
source "$CONTOUR_ROOT/contour-config.sh"

STATE_ROOT=$CONTOUR_ROOTS_STATE
CANDIDATE_ROOT=$STATE_ROOT/candidates
REQUESTED_MODE=auto

[[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "infrastructure installation must run as root"
[[ $# -ge 1 && $# -le 2 ]] \
  || wd_die "usage: watchdog-infrastructure.sh <tested-full-sha> [--controller-only|--caddy-only|--apply-caddy]"
SHA=$1
wd_validate_sha "$SHA"
if [[ $# -eq 2 ]]; then
  case "$2" in
    --controller-only) REQUESTED_MODE=controller ;;
    --caddy-only) REQUESTED_MODE=caddy ;;
    --apply-caddy) REQUESTED_MODE=apply-caddy ;;
    *) wd_die "unknown infrastructure option: $2" ;;
  esac
fi

CANDIDATE=$CANDIDATE_ROOT/$SHA
MARKER=$STATE_ROOT/$SHA.tested
[[ -d $CANDIDATE && ! -L $CANDIDATE ]] || wd_die "tested candidate directory is missing"
[[ $(stat -c '%u' -- "$CANDIDATE") == 0 ]] || wd_die "tested candidate must be root-owned"
[[ -f $MARKER && ! -L $MARKER ]] || wd_die "candidate test-success marker is missing"
marker_sha=$(wd_marker_value "$MARKER" sha) || wd_die "candidate marker has no SHA"
marker_tree=$(wd_marker_value "$MARKER" tree) || wd_die "candidate marker has no tree"
[[ $marker_sha == "$SHA" ]] || wd_die "candidate marker SHA mismatch"

candidate_head=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{commit}')
candidate_tree=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-parse 'HEAD^{tree}')
[[ $candidate_head == "$SHA" && $candidate_tree == "$marker_tree" ]] \
  || wd_die "tested candidate identity changed after validation"
[[ -z $(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" status --porcelain --untracked-files=no) ]] \
  || wd_die "tested candidate has tracked modifications"
[[ -x $CANDIDATE/deploy/install-watchdog.sh && ! -L $CANDIDATE/deploy/install-watchdog.sh ]] \
  || wd_die "candidate watchdog installer is missing"

BASE=$(wd_read_sha "$STATE_ROOT/infrastructure.sha") \
  || wd_die "installed infrastructure baseline is missing"
git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" merge-base --is-ancestor "$BASE" "$SHA" \
  || wd_die "installed infrastructure baseline is not an ancestor of the candidate"
INSTALL_SCOPE=$(wd_infrastructure_install_scope "$CANDIDATE" "$BASE" "$SHA") \
  || wd_die "could not derive the exact infrastructure scope"
wd_infrastructure_scope_is_valid "$INSTALL_SCOPE" \
  || wd_die "invalid exact infrastructure scope: $INSTALL_SCOPE"
[[ $INSTALL_SCOPE != none ]] || wd_die "candidate has no infrastructure definitions to install"
CADDY_CHANGED=0
wd_range_has_class "$CANDIDATE" "$BASE" "$SHA" wd_path_is_caddy && CADDY_CHANGED=1

# Compatibility options from the previous controller remain accepted, but the fixed bridge derives
# the transaction itself and refuses a request that would omit part of the tested range.
case "$REQUESTED_MODE" in
  auto) ;;
  controller)
    [[ $INSTALL_SCOPE == controller ]] \
      || wd_die "controller-only request does not cover exact scope $INSTALL_SCOPE"
    ;;
  caddy)
    [[ $INSTALL_SCOPE == caddy ]] \
      || wd_die "caddy-only request does not cover exact scope $INSTALL_SCOPE"
    ;;
  apply-caddy)
    (( CADDY_CHANGED == 1 )) \
      || wd_die "apply-caddy request has no Caddy definition change"
    ;;
esac

if [[ $INSTALL_SCOPE == full ]]; then
  "$CANDIDATE/deploy/install-watchdog.sh"
else
  if wd_infrastructure_scope_has "$INSTALL_SCOPE" systemd; then
    "$CANDIDATE/deploy/install-watchdog.sh" --systemd-only
  fi
  if wd_infrastructure_scope_has "$INSTALL_SCOPE" monitoring; then
    "$CANDIDATE/deploy/install-watchdog.sh" --monitoring-only
  fi
fi
if (( CADDY_CHANGED == 1 )); then
  [[ -f $CANDIDATE/deploy/Caddyfile && ! -L $CANDIDATE/deploy/Caddyfile ]] \
    || wd_die "candidate Caddy template is missing"
  [[ -x $CANDIDATE/deploy/install-caddy.sh && ! -L $CANDIDATE/deploy/install-caddy.sh ]] \
    || wd_die "candidate Caddy installer is missing"
  CADDY_TEMPLATE="$CANDIDATE/deploy/Caddyfile" "$CANDIDATE/deploy/install-caddy.sh" --check
  CADDY_TEMPLATE="$CANDIDATE/deploy/Caddyfile" "$CANDIDATE/deploy/install-caddy.sh"
fi
if [[ $INSTALL_SCOPE != full ]] \
    && wd_infrastructure_scope_has "$INSTALL_SCOPE" controller; then
  # Copy the controller last so a partial transaction never exposes a newer controller before its
  # independent systemd, monitoring, and Caddy concerns have completed.
  "$CANDIDATE/deploy/install-watchdog.sh" --controller-only
fi

# This is the controller handoff fence. Record it only after the installer and the optional Caddy
# transaction both succeed; writing it earlier would make a retry skip a failed infrastructure step.
wd_atomic_write "$STATE_ROOT/infrastructure.sha" "$SHA" 0640
chown root:deploy "$STATE_ROOT/infrastructure.sha"
wd_log "installed operational definitions from tested candidate $SHA"
