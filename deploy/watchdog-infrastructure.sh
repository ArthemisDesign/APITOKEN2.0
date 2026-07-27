#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
INSTALL_MODE=full

[[ ${EUID:-$(id -u)} -eq 0 ]] || wd_die "infrastructure installation must run as root"
[[ $# -ge 1 && $# -le 2 ]] \
  || wd_die "usage: watchdog-infrastructure.sh <tested-full-sha> [--controller-only|--caddy-only|--apply-caddy]"
SHA=$1
wd_validate_sha "$SHA"
if [[ $# -eq 2 ]]; then
  case "$2" in
    --controller-only) INSTALL_MODE=controller ;;
    --caddy-only) INSTALL_MODE=caddy ;;
    --apply-caddy) INSTALL_MODE=full-caddy ;;
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

case "$INSTALL_MODE" in
  controller) "$CANDIDATE/deploy/install-watchdog.sh" --controller-only ;;
  full|full-caddy) "$CANDIDATE/deploy/install-watchdog.sh" ;;
esac
if [[ $INSTALL_MODE == caddy || $INSTALL_MODE == full-caddy ]]; then
  [[ -f $CANDIDATE/deploy/Caddyfile && ! -L $CANDIDATE/deploy/Caddyfile ]] \
    || wd_die "candidate Caddy template is missing"
  [[ -x $CANDIDATE/deploy/install-caddy.sh && ! -L $CANDIDATE/deploy/install-caddy.sh ]] \
    || wd_die "candidate Caddy installer is missing"
  CADDY_TEMPLATE="$CANDIDATE/deploy/Caddyfile" "$CANDIDATE/deploy/install-caddy.sh" --check
  CADDY_TEMPLATE="$CANDIDATE/deploy/Caddyfile" "$CANDIDATE/deploy/install-caddy.sh"
fi

# This is the controller handoff fence. Record it only after the installer and the optional Caddy
# transaction both succeed; writing it earlier would make a retry skip a failed infrastructure step.
wd_atomic_write "$STATE_ROOT/infrastructure.sha" "$SHA" 0640
chown root:deploy "$STATE_ROOT/infrastructure.sha"
wd_log "installed operational definitions from tested candidate $SHA"
