#!/usr/bin/env bash
set -euo pipefail

LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
[[ -r $LIB ]] || { printf 'watchdog library is not installed\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

STATE_ROOT=/var/lib/apitoken/watchdog
CANDIDATE_ROOT=$STATE_ROOT/candidates
GITHUB_HELPER=/usr/local/lib/apitoken-watchdog/watchdog-github
DIRECT_ADMISSION_GATE=/usr/local/lib/apitoken-watchdog/controller/gemini-3-7-admission-gate.sh
DIRECT_ADMISSION_PRODUCER_SHA=264363f7838ddd2d156b14668a320047ad33b6ee
DIRECT_ADMISSION_TRIGGER=deploy/gemini-3-7-admission-trigger
DIRECT_ADMISSION_TRIGGER_BLOB=9b8e56096ace94bbc0daf90f1914647518c4b55b
DIRECT_ADMISSION_GATE_SHA256=7e4a18857ffeb218986ffbc7b799c74e9497dfcbdbfb792bc041dbe5a56742d6
DIRECT_ADMISSION_TRANSPORT_SHA256=75bbcebd7468e8d0f5d496a8d9121e9dfe780e1d28cb4f0a84ce727f00f6d5f7
DIRECT_ADMISSION_PACKAGE_SHA256=cee5d8232c6da8fa74b0d01b3cfaab40709eed914594889ed68826ce6260a532
DIRECT_ADMISSION_STATE_SHA256=4679ecfb90948c1ce658c647dbb2c91213b410b72ea3149886d0626b20aaf50d
DIRECT_ADMISSION_RUN_LIVE_SHA256=061340cbc323180469a5a4e6f10f70b370f53833d0ad583f325b3b9f7b49fdee
DIRECT_ADMISSION_UNIT_SHA256=7af20848cb5b018dedaf55e08905f1ffb5cb6c4f4742985e1039b96e63fa95b3
DIRECT_ADMISSION_STATE_ROOT=/var/lib/apitoken/gemini-3-7-admission
DIRECT_ADMISSION_DELIVERY_FILE=$DIRECT_ADMISSION_STATE_ROOT/delivery.sha
DIRECT_ADMISSION_EVIDENCE_ROOT=/var/lib/apitoken/gemini-3-7-admission/$DIRECT_ADMISSION_PRODUCER_SHA
REQUESTED_MODE=auto

# DIRECT_ADMISSION_TRIGGER_FUNCTION_BEGIN
direct_admission_trigger_required() {
  local repo=$1 base=$2 target=$3 commits parents_line commit _commit parent second_parent remainder
  local delta entry status path extra involved trigger_count=0 trigger_sha=none
  commits=$(git -c safe.directory="$repo" -C "$repo" rev-list --first-parent --reverse \
    "$base..$target") || return 1
  if [[ -z $commits ]]; then
    printf 'none\n'
    return 0
  fi
  while IFS= read -r commit; do
    [[ $commit =~ ^[0-9a-f]{40}$ ]] || return 1
    parents_line=$(git -c safe.directory="$repo" -C "$repo" rev-list --parents -n 1 \
      "$commit") || return 1
    read -r _commit parent second_parent remainder <<<"$parents_line"
    [[ $_commit == "$commit" && -n ${parent:-} ]] || return 1
    delta=$(git -c safe.directory="$repo" -C "$repo" diff --name-status --no-renames \
      "$parent" "$commit") || return 1
    involved=0
    while IFS=$'\t' read -r status path extra; do
      [[ -n $path ]] || continue
      if [[ $path == "$DIRECT_ADMISSION_TRIGGER" \
         || ${extra:-} == "$DIRECT_ADMISSION_TRIGGER" ]]; then
        involved=1
      fi
    done <<<"$delta"
    (( involved == 1 )) || continue
    [[ -z ${second_parent:-} && -z ${remainder:-} ]] \
      || wd_die "Gemini 3.7 admission trigger requires one direct parent"
    [[ $delta == $'A\t'"$DIRECT_ADMISSION_TRIGGER" ]] \
      || wd_die "Gemini 3.7 admission trigger commit contains a mixed or non-additive delta"
    entry=$(git -c safe.directory="$repo" -C "$repo" ls-tree "$commit" \
      -- "$DIRECT_ADMISSION_TRIGGER") || return 1
    [[ $entry == "100644 blob $DIRECT_ADMISSION_TRIGGER_BLOB"$'\t'"$DIRECT_ADMISSION_TRIGGER" ]] \
      || wd_die "Gemini 3.7 admission trigger mode or exact bytes are invalid"
    trigger_count=$((trigger_count + 1))
    trigger_sha=$commit
  done <<<"$commits"
  (( trigger_count <= 1 )) \
    || wd_die "infrastructure range contains multiple Gemini 3.7 admission triggers"
  printf '%s\n' "$trigger_sha"
}
# DIRECT_ADMISSION_TRIGGER_FUNCTION_END

infrastructure_durability_barrier() {
  local path
  local roots=()
  [[ $(/usr/bin/uname -s) == Linux ]] \
    || wd_die "infrastructure durability barrier requires Linux syncfs semantics"
  for path in /usr/local /etc /var/lib /opt /srv; do
    [[ -e $path && ! -L $path ]] && roots+=("$path")
  done
  (( ${#roots[@]} > 0 )) || wd_die "no infrastructure durability roots exist"
  /usr/bin/sync -f "${roots[@]}" \
    || wd_die "installed operational definitions could not be made durable"
}

require_direct_admission_file() {
  local path=$1 mode=$2 expected=$3 label=$4 actual
  [[ -f $path && ! -L $path \
     && $(stat -c '%u:%g:%a:%h' -- "$path") == "0:0:$mode:1" ]] \
    || wd_die "$label metadata is unsafe"
  actual=$(/usr/bin/sha256sum -- "$path") || wd_die "$label digest is unavailable"
  [[ ${actual%% *} == "$expected" ]] || wd_die "$label differs from the admitted implementation"
}

validate_direct_admission_implementation() {
  local layout=$1 root=$2
  case "$layout" in
    candidate)
      require_direct_admission_file "$root/deploy/gemini-3-7-admission-gate.sh" 755 \
        "$DIRECT_ADMISSION_GATE_SHA256" "candidate Gemini 3.7 gate"
      require_direct_admission_file "$root/deploy/gemini-3-7-admission-transport.py" 644 \
        "$DIRECT_ADMISSION_TRANSPORT_SHA256" "candidate Gemini 3.7 transport"
      require_direct_admission_file "$root/tools/gemini_calibration/__init__.py" 644 \
        "$DIRECT_ADMISSION_PACKAGE_SHA256" "candidate Gemini 3.7 package marker"
      require_direct_admission_file "$root/tools/gemini_calibration/admission.py" 755 \
        "$DIRECT_ADMISSION_STATE_SHA256" "candidate Gemini 3.7 state machine"
      require_direct_admission_file "$root/tools/gemini_calibration/run_live.py" 644 \
        "$DIRECT_ADMISSION_RUN_LIVE_SHA256" "candidate Gemini 3.7 evidence parser"
      require_direct_admission_file "$root/systemd/claude-api-gemini-3-7-admission.service" 644 \
        "$DIRECT_ADMISSION_UNIT_SHA256" "candidate Gemini 3.7 unit"
      ;;
    installed)
      require_direct_admission_file "$DIRECT_ADMISSION_GATE" 755 \
        "$DIRECT_ADMISSION_GATE_SHA256" "installed Gemini 3.7 gate"
      require_direct_admission_file \
        /usr/local/lib/apitoken-watchdog/controller/gemini-3-7-admission-transport.py 755 \
        "$DIRECT_ADMISSION_TRANSPORT_SHA256" "installed Gemini 3.7 transport"
      require_direct_admission_file \
        /usr/local/lib/apitoken-watchdog/controller/gemini_calibration/__init__.py 644 \
        "$DIRECT_ADMISSION_PACKAGE_SHA256" "installed Gemini 3.7 package marker"
      require_direct_admission_file \
        /usr/local/lib/apitoken-watchdog/controller/gemini_calibration/admission.py 644 \
        "$DIRECT_ADMISSION_STATE_SHA256" "installed Gemini 3.7 state machine"
      require_direct_admission_file \
        /usr/local/lib/apitoken-watchdog/controller/gemini_calibration/run_live.py 644 \
        "$DIRECT_ADMISSION_RUN_LIVE_SHA256" "installed Gemini 3.7 evidence parser"
      require_direct_admission_file /etc/systemd/system/claude-api-gemini-3-7-admission.service 644 \
        "$DIRECT_ADMISSION_UNIT_SHA256" "installed Gemini 3.7 unit"
      ;;
    *) wd_die "unknown direct-admission implementation layout" ;;
  esac
}

fsync_file_and_parent() {
  /usr/bin/python3 -I -P -B -S - "$1" <<'PY'
import os
import stat
import sys
from pathlib import Path

path = Path(sys.argv[1])
info = path.lstat()
if not stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode) or info.st_nlink != 1:
    raise SystemExit(1)
descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
try:
    os.fsync(descriptor)
finally:
    os.close(descriptor)
parent = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
try:
    os.fsync(parent)
finally:
    os.close(parent)
PY
}

direct_admission_evidence_root_is_safe() {
  [[ -d $DIRECT_ADMISSION_EVIDENCE_ROOT && ! -L $DIRECT_ADMISSION_EVIDENCE_ROOT \
     && $(stat -c '%u:%g:%a' -- "$DIRECT_ADMISSION_EVIDENCE_ROOT") == 0:0:700 ]]
}

direct_admission_state_root_is_safe() {
  [[ -d $DIRECT_ADMISSION_STATE_ROOT && ! -L $DIRECT_ADMISSION_STATE_ROOT \
     && $(stat -c '%u:%g:%a' -- "$DIRECT_ADMISSION_STATE_ROOT") == 0:0:700 ]]
}

direct_admission_delivery_file_is_safe() {
  [[ -f $DIRECT_ADMISSION_DELIVERY_FILE && ! -L $DIRECT_ADMISSION_DELIVERY_FILE \
     && $(stat -c '%u:%g:%a:%h:%s' -- "$DIRECT_ADMISSION_DELIVERY_FILE") \
        == 0:0:600:1:41 ]]
}

bind_direct_admission_delivery() {
  local evidence_preexisted=$1 delivery_sha lineage
  direct_admission_state_root_is_safe \
    || wd_die "Gemini 3.7 admission state root is unsafe"
  if (( evidence_preexisted == 1 )); then
    direct_admission_delivery_file_is_safe \
      || wd_die "Gemini 3.7 admission evidence has no safe delivery binding"
    delivery_sha=$(wd_read_sha "$DIRECT_ADMISSION_DELIVERY_FILE") \
      || wd_die "Gemini 3.7 admission delivery binding is invalid"
  else
    # No firing fence means no transport can have started. Bind (or rebind after a pre-gate crash)
    # the upcoming attempt to the exact delivery SHA before the gate is allowed to create evidence.
    wd_atomic_write "$DIRECT_ADMISSION_DELIVERY_FILE" "$SHA" 0600
    chown root:root "$DIRECT_ADMISSION_DELIVERY_FILE"
    fsync_file_and_parent "$DIRECT_ADMISSION_DELIVERY_FILE" \
      || wd_die "Gemini 3.7 admission delivery binding could not be made durable"
    direct_admission_delivery_file_is_safe \
      || wd_die "Gemini 3.7 admission delivery binding is unsafe after replacement"
    delivery_sha=$SHA
  fi
  lineage=$(git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" rev-list --first-parent \
    "$DIRECT_ADMISSION_TRIGGER_SHA^..$SHA") \
    || wd_die "could not validate Gemini 3.7 admission delivery lineage"
  [[ $'\n'$lineage$'\n' == *$'\n'$delivery_sha$'\n'* ]] \
    || wd_die "Gemini 3.7 admission delivery binding is not on the trigger lineage"
  printf '%s\n' "$delivery_sha"
}

reject_infrastructure_candidate() {
  wd_atomic_write "$STATE_ROOT/rejected.sha" "$SHA" 0644
  chown root:deploy "$STATE_ROOT/rejected.sha"
  fsync_file_and_parent "$STATE_ROOT/rejected.sha" \
    || wd_die "terminal admission rejection could not be made durable"
}

record_infrastructure_baseline() {
  wd_atomic_write "$STATE_ROOT/infrastructure.sha" "$SHA" 0640
  chown root:deploy "$STATE_ROOT/infrastructure.sha"
  fsync_file_and_parent "$STATE_ROOT/infrastructure.sha" \
    || wd_die "infrastructure baseline could not be made durable"
}

# DIRECT_ADMISSION_FINALIZE_FUNCTION_BEGIN
complete_infrastructure_transaction() {
  if [[ $DIRECT_ADMISSION_TRIGGER_SHA != none ]]; then
    local evidence_preexisted=0 gate_rc=0 delivery_sha
    if [[ -e $DIRECT_ADMISSION_EVIDENCE_ROOT || -L $DIRECT_ADMISSION_EVIDENCE_ROOT ]]; then
      direct_admission_evidence_root_is_safe \
        || wd_die "Gemini 3.7 admission evidence root is unsafe"
      evidence_preexisted=1
    fi
    delivery_sha=$(bind_direct_admission_delivery "$evidence_preexisted") \
      || wd_die "could not bind Gemini 3.7 admission to its delivery SHA"
    validate_direct_admission_implementation installed
    "$DIRECT_ADMISSION_GATE" "$DIRECT_ADMISSION_PRODUCER_SHA" || gate_rc=$?
    if (( gate_rc != 0 )); then
      direct_admission_evidence_root_is_safe \
        || wd_die "Gemini 3.7 admission failed before creating its permanent firing fence"
      if (( evidence_preexisted == 1 )) && [[ $SHA != "$delivery_sha" ]]; then
        # A descendant can close a previously withdrawn one-shot without another paid attempt.
        # Gate re-entry is offline-only, so this records the dormant model and unblocks fix-forward.
        infrastructure_durability_barrier
        record_infrastructure_baseline
        wd_warn "closed previously withdrawn Gemini 3.7 admission at descendant $SHA"
        return 0
      fi
      # On the trigger itself (or a first attempt observed at a descendant), preserve an explicit
      # RED fence before the baseline. Every crash prefix is then either retryable from the old
      # baseline or rejected at the new one; it can never fall through to processed/GREEN.
      reject_infrastructure_candidate
      infrastructure_durability_barrier
      record_infrastructure_baseline
      wd_die "Gemini 3.7 admission permanently withdrew; candidate remains rejected"
    fi
  fi
  infrastructure_durability_barrier
  record_infrastructure_baseline
}
# DIRECT_ADMISSION_FINALIZE_FUNCTION_END

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

github_helper_parent=${GITHUB_HELPER%/*}
[[ -d $github_helper_parent && ! -L $github_helper_parent \
   && $(stat -c '%u:%g:%a' -- "$github_helper_parent") == 0:0:755 ]] \
  || wd_die "fixed GitHub production-head verifier parent is unsafe"
[[ -f $GITHUB_HELPER && ! -L $GITHUB_HELPER \
   && $(stat -c '%u:%g:%a:%h' -- "$GITHUB_HELPER") == 0:0:755:1 ]] \
  || wd_die "fixed GitHub production-head verifier is unsafe"
"$GITHUB_HELPER" production-head-is "$SHA" \
  || wd_die "candidate is not the exact protected production head"
BASE=$(wd_read_sha "$STATE_ROOT/infrastructure.sha") \
  || wd_die "installed infrastructure baseline is missing"
git -c safe.directory="$CANDIDATE" -C "$CANDIDATE" merge-base --is-ancestor "$BASE" "$SHA" \
  || wd_die "installed infrastructure baseline is not an ancestor of the candidate"
DIRECT_ADMISSION_TRIGGER_SHA=$(direct_admission_trigger_required "$CANDIDATE" "$BASE" "$SHA") \
  || wd_die "could not validate the uninstalled direct-admission trigger range"
[[ $DIRECT_ADMISSION_TRIGGER_SHA == none \
   || $DIRECT_ADMISSION_TRIGGER_SHA =~ ^[0-9a-f]{40}$ ]] \
  || wd_die "invalid direct-admission trigger identity"
if [[ $DIRECT_ADMISSION_TRIGGER_SHA != none ]]; then
  validate_direct_admission_implementation candidate "$CANDIDATE"
fi
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

# This is the controller handoff fence. A trigger commit must first reach terminal gate success;
# every operational write is then flushed before the baseline can make a retry skip installation.
complete_infrastructure_transaction
wd_log "installed operational definitions from tested candidate $SHA"
