#!/usr/bin/env bash
set -euo pipefail
umask 077

EXPECTED_SHA=264363f7838ddd2d156b14668a320047ad33b6ee
MODEL=gemini-3.7-flash
UNIT=claude-api-gemini-3-7-admission.service
STABLE_PORT=8794
CANARY_PORT=8807
BUDGET_NANOUSD=1574784000
MAX_OUTPUT_TOKENS=256
ADMISSION_SHA256=4679ecfb90948c1ce658c647dbb2c91213b410b72ea3149886d0626b20aaf50d
RUN_LIVE_SHA256=061340cbc323180469a5a4e6f10f70b370f53833d0ad583f325b3b9f7b49fdee
PACKAGE_SHA256=cee5d8232c6da8fa74b0d01b3cfaab40709eed914594889ed68826ce6260a532
TRANSPORT_SHA256=75bbcebd7468e8d0f5d496a8d9121e9dfe780e1d28cb4f0a84ce727f00f6d5f7
UNIT_SHA256=a7262cd8c42ebe044f4453a60455dd15c42d452315939ab1621b508f2cc4d6f8
EMPTY_SHA256=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
WATCHDOG_LIB_SHA256=ef2ca62c23697e504829d7892e47dc1d410465567a1560e58868457a69c56098

GATE_TESTING=${GEMINI_3_7_ADMISSION_GATE_TESTING:-0}
case "$GATE_TESTING" in
  0|1) ;;
  *) printf 'invalid Gemini 3.7 admission gate mode\n' >&2; exit 1 ;;
esac
# Test overrides exist only for the unprivileged hermetic regression. Production execution is
# accepted only from an actual root process, before inherited environment can select any paths.
if [[ $GATE_TESTING == 1 && $EUID -eq 0 ]]; then
  printf 'Gemini 3.7 admission test mode is unavailable to root\n' >&2
  exit 1
fi
if [[ $GATE_TESTING != 1 && $EUID -ne 0 ]]; then
  printf 'Gemini 3.7 admission gate must run directly as root\n' >&2
  exit 1
fi
if [[ $GATE_TESTING == 1 ]]; then
  LIB=${GEMINI_3_7_ADMISSION_GATE_TEST_LIB:?test watchdog library is required}
  CONTROLLER_ROOT=${GEMINI_3_7_ADMISSION_GATE_TEST_CONTROLLER_ROOT:?test controller root is required}
  UNIT_FILE=${GEMINI_3_7_ADMISSION_GATE_TEST_UNIT_FILE:?test unit file is required}
  RELEASE_ROOT=${GEMINI_3_7_ADMISSION_GATE_TEST_RELEASE_ROOT:?test release root is required}
  PRODUCER_ROOT=${GEMINI_3_7_ADMISSION_GATE_TEST_PRODUCER_ROOT:?test producer root is required}
  STATE_TRUST_ROOT=${GEMINI_3_7_ADMISSION_GATE_TEST_STATE_TRUST_ROOT:?test state trust root is required}
  STATE_PARENT=${GEMINI_3_7_ADMISSION_GATE_TEST_STATE_PARENT:?test state parent is required}
  PROC_ROOT=${GEMINI_3_7_ADMISSION_GATE_TEST_PROC_ROOT:?test proc root is required}
  RUNTIME_PARENT=${GEMINI_3_7_ADMISSION_GATE_TEST_RUNTIME_PARENT:?test runtime parent is required}
  SYSTEMCTL=${GEMINI_3_7_ADMISSION_GATE_TEST_SYSTEMCTL:?test systemctl is required}
  CURL=${GEMINI_3_7_ADMISSION_GATE_TEST_CURL:?test curl is required}
  SS=${GEMINI_3_7_ADMISSION_GATE_TEST_SS:?test ss is required}
  STAT=${GEMINI_3_7_ADMISSION_GATE_TEST_STAT:?test stat is required}
  SHA256SUM=${GEMINI_3_7_ADMISSION_GATE_TEST_SHA256SUM:?test sha256sum is required}
  READLINK=${GEMINI_3_7_ADMISSION_GATE_TEST_READLINK:?test readlink is required}
  PYTHON=${GEMINI_3_7_ADMISSION_GATE_TEST_PYTHON:?test python is required}
  SLEEP=${GEMINI_3_7_ADMISSION_GATE_TEST_SLEEP:?test sleep is required}
  TIMEOUT=${GEMINI_3_7_ADMISSION_GATE_TEST_TIMEOUT:?test timeout is required}
  RM=${GEMINI_3_7_ADMISSION_GATE_TEST_RM:?test rm is required}
  RMDIR=${GEMINI_3_7_ADMISSION_GATE_TEST_RMDIR:?test rmdir is required}
  PROFILES_SOURCE=${GEMINI_3_7_ADMISSION_GATE_TEST_PROFILES_SOURCE:?test profiles source is required}
  EXPECTED_OWNER_UID=$(id -u)
  EXPECTED_OWNER_GID=$(id -g)
  LIB_EXPECTED_MODE=755
  STABLE_PORT=${GEMINI_3_7_ADMISSION_GATE_TEST_STABLE_PORT:-8794}
  CANARY_PORT=${GEMINI_3_7_ADMISSION_GATE_TEST_CANARY_PORT:?test canary port is required}
  export GEMINI_ADMISSION_TESTING=1
  export GEMINI_ADMISSION_TEST_PORT=$CANARY_PORT
else
  LIB=/usr/local/lib/apitoken-watchdog/watchdog-lib.sh
  CONTROLLER_ROOT=/usr/local/lib/apitoken-watchdog/controller
  UNIT_FILE=/etc/systemd/system/$UNIT
  RELEASE_ROOT=/srv/claude-api/releases
  PRODUCER_ROOT=/usr/local/lib/apitoken-watchdog/producers/$EXPECTED_SHA
  STATE_TRUST_ROOT=/var/lib/apitoken
  STATE_PARENT=$STATE_TRUST_ROOT/gemini-3-7-admission
  PROC_ROOT=/proc
  RUNTIME_PARENT=/run
  SYSTEMCTL=/usr/bin/systemctl
  CURL=/usr/bin/curl
  if [[ -x /usr/bin/ss ]]; then SS=/usr/bin/ss; else SS=/usr/sbin/ss; fi
  STAT=/usr/bin/stat
  SHA256SUM=/usr/bin/sha256sum
  READLINK=/usr/bin/readlink
  PYTHON=/usr/bin/python3
  SLEEP=/usr/bin/sleep
  TIMEOUT=/usr/bin/timeout
  RM=/usr/bin/rm
  RMDIR=/usr/bin/rmdir
  PROFILES_SOURCE=/srv/claude-api/data/gemini/profiles.json
  EXPECTED_OWNER_UID=0
  EXPECTED_OWNER_GID=0
  LIB_EXPECTED_MODE=644
  unset GEMINI_ADMISSION_TESTING GEMINI_ADMISSION_TEST_PORT
fi

# Start from a fixed neutral working directory before sourcing or starting any interpreter, then
# use isolated/safe-path Python for every invocation below.
cd /

[[ -r $LIB && ! -L $LIB ]] || { printf 'watchdog library is unavailable\n' >&2; exit 1; }
lib_parent=${LIB%/*}
[[ -d $lib_parent && ! -L $lib_parent ]] \
  || { printf 'watchdog library trust root is unavailable\n' >&2; exit 1; }
lib_parent_metadata=$($STAT -c '%u:%a' -- "$lib_parent") \
  || { printf 'watchdog library trust root metadata is unavailable\n' >&2; exit 1; }
[[ $lib_parent_metadata == "$EXPECTED_OWNER_UID:755" ]] \
  || { printf 'watchdog library trust root ownership or mode drifted\n' >&2; exit 1; }
lib_metadata=$($STAT -c '%u:%a:%h' -- "$LIB") \
  || { printf 'watchdog library metadata is unavailable\n' >&2; exit 1; }
[[ $lib_metadata == "$EXPECTED_OWNER_UID:$LIB_EXPECTED_MODE:1" ]] \
  || { printf 'watchdog library ownership, mode, or links drifted\n' >&2; exit 1; }
lib_digest=$($SHA256SUM -- "$LIB") \
  || { printf 'watchdog library digest is unavailable\n' >&2; exit 1; }
lib_digest=${lib_digest%% *}
[[ $lib_digest == "$WATCHDOG_LIB_SHA256" ]] \
  || { printf 'watchdog library content drifted\n' >&2; exit 1; }
# shellcheck source=deploy/watchdog-lib.sh
source "$LIB"

[[ $# -eq 1 ]] || wd_die "usage: gemini-3-7-admission-gate.sh <exact-engine-sha>"
SHA=$1
wd_validate_sha "$SHA"
[[ $SHA == "$EXPECTED_SHA" ]] || wd_die "Gemini 3.7 admission is pinned to $EXPECTED_SHA"

ADMISSION_PACKAGE=$CONTROLLER_ROOT/gemini_calibration
PACKAGE_INIT=$ADMISSION_PACKAGE/__init__.py
ADMISSION=$ADMISSION_PACKAGE/admission.py
RUN_LIVE=$ADMISSION_PACKAGE/run_live.py
TRANSPORT=$CONTROLLER_ROOT/gemini-3-7-admission-transport.py
RELEASE=$RELEASE_ROOT/$SHA
RELEASE_BINARY=$RELEASE/claude-api
PRODUCER_BINARY=$PRODUCER_ROOT/claude-api
PRODUCER_DIGEST_ANCHOR=$PRODUCER_ROOT/claude-api.sha256
PRODUCER_MARKER=$PRODUCER_ROOT/.release-sha
ROOT=$STATE_PARENT/$SHA
EVIDENCE=$ROOT/evidence
CAPACITY=$ROOT/capacity.json
PROFILE_ID=$ROOT/profile-id
PLAN_FILE=$ROOT/plan
COUNT_OBSERVATION=$ROOT/count-observation.json
OUTCOME_OBSERVATION=$ROOT/outcome-observation.json
LOCK_FILE=$STATE_PARENT/gate.lock
CLEANUP_MARKER=$ROOT/cleanup.complete
BOUND_PRODUCER_DIGEST=$ROOT/producer-binary.sha256

require_fixed_directory() {
  local path=$1 mode=$2 label=$3 metadata
  [[ -d $path && ! -L $path ]] || wd_die "$label directory is unavailable"
  metadata=$($STAT -c '%u:%a' -- "$path") || wd_die "$label directory metadata is unavailable"
  [[ $metadata == "$EXPECTED_OWNER_UID:$mode" ]] || wd_die "$label directory ownership or mode drifted"
}

require_immutable_parent_directory() {
  local path=$1 label=$2 metadata owner mode
  [[ -d $path && ! -L $path ]] || wd_die "$label directory is unavailable"
  metadata=$($STAT -c '%u:%a' -- "$path") \
    || wd_die "$label directory metadata is unavailable"
  IFS=: read -r owner mode <<<"$metadata"
  [[ $owner == "$EXPECTED_OWNER_UID" && $mode =~ ^[0-7]{3,4}$ ]] \
    || wd_die "$label directory ownership or mode is invalid"
  (( (8#$mode & 8#022) == 0 )) || wd_die "$label directory is writable outside its owner"
}

require_fixed_file() {
  local path=$1 mode=$2 expected_hash=$3 label=$4 metadata digest ignored
  [[ -f $path && ! -L $path ]] || wd_die "$label is unavailable"
  metadata=$($STAT -c '%u:%a:%h' -- "$path") || wd_die "$label metadata is unavailable"
  [[ $metadata == "$EXPECTED_OWNER_UID:$mode:1" ]] || wd_die "$label ownership, mode, or link count drifted"
  read -r digest ignored < <($SHA256SUM "$path") || wd_die "$label digest is unavailable"
  [[ $digest == "$expected_hash" ]] || wd_die "$label differs from the pinned producer"
}

require_fixed_text_file() {
  local path=$1 mode=$2 expected=$3 label=$4 metadata
  local lines=()
  [[ -f $path && ! -L $path ]] || wd_die "$label is unavailable"
  metadata=$($STAT -c '%u:%a:%h' -- "$path") || wd_die "$label metadata is unavailable"
  [[ $metadata == "$EXPECTED_OWNER_UID:$mode:1" ]] \
    || wd_die "$label ownership, mode, or link count drifted"
  mapfile -t lines < "$path" || wd_die "$label is unreadable"
  [[ ${#lines[@]} -eq 1 && ${lines[0]} == "$expected" ]] \
    || wd_die "$label content drifted"
}

require_fixed_directory "$CONTROLLER_ROOT" 755 "controller root"
require_fixed_directory "$ADMISSION_PACKAGE" 755 "admission package"
require_immutable_parent_directory "$STATE_TRUST_ROOT" "admission trust root"
require_fixed_directory "$STATE_PARENT" 700 "admission state"
require_fixed_file "$LOCK_FILE" 600 "$EMPTY_SHA256" "admission process lock"
require_fixed_directory "$PRODUCER_ROOT" 555 "sealed producer"
PRODUCER_ROOT=$($READLINK -f -- "$PRODUCER_ROOT") \
  || wd_die "sealed producer path is unavailable"
PRODUCER_BINARY=$PRODUCER_ROOT/claude-api
PRODUCER_DIGEST_ANCHOR=$PRODUCER_ROOT/claude-api.sha256
PRODUCER_MARKER=$PRODUCER_ROOT/.release-sha
[[ -f $PRODUCER_DIGEST_ANCHOR && ! -L $PRODUCER_DIGEST_ANCHOR \
   && $($STAT -c '%u:%a:%h' -- "$PRODUCER_DIGEST_ANCHOR") == "$EXPECTED_OWNER_UID:444:1" ]] \
  || wd_die "sealed producer digest anchor is unavailable"
mapfile -t producer_digest_lines < "$PRODUCER_DIGEST_ANCHOR"
[[ ${#producer_digest_lines[@]} -eq 1 \
   && ${producer_digest_lines[0]} =~ ^[0-9a-f]{64}$ ]] \
  || wd_die "sealed producer digest anchor is malformed"
PRODUCER_DIGEST=${producer_digest_lines[0]}
require_fixed_text_file "$PRODUCER_MARKER" 444 "$EXPECTED_SHA" "sealed producer marker"
require_fixed_file "$PRODUCER_BINARY" 555 "$PRODUCER_DIGEST" "sealed producer binary"
CLEANUP_MARKER_TEXT=gemini-3.7-admission-cleanup/v3:$EXPECTED_SHA:$PRODUCER_DIGEST
require_fixed_file "$PACKAGE_INIT" 644 "$PACKAGE_SHA256" "admission package marker"
require_fixed_file "$ADMISSION" 644 "$ADMISSION_SHA256" "admission state machine"
require_fixed_file "$RUN_LIVE" 644 "$RUN_LIVE_SHA256" "admission evidence parser"
require_fixed_file "$TRANSPORT" 755 "$TRANSPORT_SHA256" "admission transport"
require_fixed_file "$UNIT_FILE" 644 "$UNIT_SHA256" "admission systemd definition"

if ! "$PYTHON" -I -P -B -S - "$ADMISSION_PACKAGE" 2>/dev/null <<'PY'
import sys
from pathlib import Path

package = Path(sys.argv[1])
if {entry.name for entry in package.iterdir()} != {"__init__.py", "admission.py", "run_live.py"}:
    raise SystemExit(1)
PY
then
  wd_die "admission package contains an unauthenticated entry"
fi

run_admission() {
  "$PYTHON" -I -P -B -S -c \
    'import runpy,sys; sys.path.insert(0, sys.argv.pop(1)); runpy.run_module("gemini_calibration.admission", run_name="__main__")' \
    "$CONTROLLER_ROOT" "$@"
}

inspect_success() {
  local summary
  summary=$(run_admission inspect \
    --evidence-dir "$EVIDENCE" --require-success 2>/dev/null) \
    || wd_die "Gemini 3.7 admission evidence is not an exact terminal success"
  summary=$("$PYTHON" -I -P -B -S -c \
    'import json,sys; value=json.load(sys.stdin); isinstance(value,dict) and "producer_binary_sha256" not in value or sys.exit(1); value["producer_binary_sha256"]=sys.argv[1]; print(json.dumps(value,sort_keys=True,separators=(",",":")))' \
    "$PRODUCER_DIGEST" <<<"$summary" 2>/dev/null) \
    || wd_die "Gemini 3.7 admission summary could not bind its sealed producer"
  printf '%s\n' "$summary"
}

offline_success() {
  require_fixed_text_file "$BOUND_PRODUCER_DIGEST" 600 "$PRODUCER_DIGEST" \
    "admission producer binding"
  require_fixed_text_file "$CLEANUP_MARKER" 600 "$CLEANUP_MARKER_TEXT" \
    "admission cleanup attestation"
  inspect_success
}

# The SHA-keyed directory is the permanent firing fence. Re-entry is deliberately decided before
# systemd, readiness, credentials, or any network operation and can only inspect retained evidence.
if [[ -e $ROOT || -L $ROOT ]]; then
  [[ -d $ROOT && ! -L $ROOT ]] || wd_die "Gemini 3.7 admission evidence root is unsafe"
  offline_success
  exit 0
fi

# The kernel lock closes the only pre-fence race: concurrent direct root invocations share this
# root-owned inode. The parent shell holds the descriptor for its lifetime, and the kernel releases
# the lock on every exit path, including SIGKILL.
exec 8<>"$LOCK_FILE"
if ! "$PYTHON" -I -P -B -S -c \
  'import fcntl; fcntl.flock(8, fcntl.LOCK_EX | fcntl.LOCK_NB)' 2>/dev/null; then
  wd_die "another Gemini 3.7 admission gate owns the one-shot lock"
fi
LOCK_OWNED=1

release_gate_lock() {
  if (( LOCK_OWNED == 1 )); then
    exec 8>&-
    LOCK_OWNED=0
  fi
}

lock_only_cleanup() {
  local rc=$?
  trap - EXIT INT TERM HUP
  release_gate_lock
  exit "$rc"
}
trap lock_only_cleanup EXIT
trap 'exit 130' INT TERM HUP

# A prior invocation may have completed while this process was waiting to acquire the lock.
if [[ -e $ROOT || -L $ROOT ]]; then
  [[ -d $ROOT && ! -L $ROOT ]] || wd_die "Gemini 3.7 admission evidence root is unsafe"
  offline_success
  exit 0
fi

[[ -d $RELEASE && ! -L $RELEASE ]] || wd_die "exact Gemini 3.7 producer release is unavailable"
RELEASE=$($READLINK -f -- "$RELEASE") || wd_die "exact Gemini 3.7 producer path is unavailable"
RELEASE_BINARY=$RELEASE/claude-api
[[ -f $RELEASE/.release-sha && ! -L $RELEASE/.release-sha ]] \
  || wd_die "exact Gemini 3.7 producer marker is unavailable"
mapfile -t release_marker < "$RELEASE/.release-sha"
[[ ${#release_marker[@]} -eq 1 && ${release_marker[0]} == "$SHA" ]] \
  || wd_die "exact Gemini 3.7 producer marker differs from its release"
[[ -f $RELEASE_BINARY && ! -L $RELEASE_BINARY && -x $RELEASE_BINARY ]] \
  || wd_die "exact Gemini 3.7 producer binary is unavailable"
[[ $($STAT -c '%h' -- "$RELEASE_BINARY") == 1 ]] \
  || wd_die "exact producer binary has a foreign hard link"
release_digest=$($SHA256SUM -- "$RELEASE_BINARY") \
  || wd_die "exact producer binary digest is unavailable"
release_digest=${release_digest%% *}
[[ $release_digest == "$PRODUCER_DIGEST" ]] \
  || wd_die "current producer binary differs from its sealed root anchor"
CURRENT=$($READLINK -f -- "$RELEASE_ROOT/current") \
  || wd_die "current engine release is unavailable"
[[ $CURRENT == "$RELEASE" ]] || wd_die "Gemini 3.7 admission requires current engine $SHA"

systemctl_main_pid() {
  local unit=$1 value
  value=$($SYSTEMCTL show "$unit" --property=MainPID --value 2>/dev/null) || return 1
  [[ $value =~ ^[1-9][0-9]*$ ]] || return 1
  REPLY=$value
}

verify_unit_definition() {
  local fragment dropins reload
  fragment=$($SYSTEMCTL show "$UNIT" --property=FragmentPath --value 2>/dev/null) \
    || return 1
  dropins=$($SYSTEMCTL show "$UNIT" --property=DropInPaths --value 2>/dev/null) \
    || return 1
  reload=$($SYSTEMCTL show "$UNIT" --property=NeedDaemonReload --value 2>/dev/null) \
    || return 1
  [[ $fragment == "$UNIT_FILE" && -z $dropins && $reload == no ]]
}

process_identity() {
  local pid=$1 expected_path=$2
  "$PYTHON" -I -P -B -S - "$PROC_ROOT/$pid" "$expected_path" "$PRODUCER_DIGEST" \
    2>/dev/null <<'PY'
import hashlib
import os
import stat
import sys
from pathlib import Path

process, expected, expected_digest = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]

def starttime() -> str:
    raw = (process / "stat").read_text(encoding="ascii")
    close = raw.rfind(")")
    fields = raw[close + 2 :].split() if close >= 0 else []
    if len(fields) < 20 or not fields[19].isdigit():
        raise SystemExit(1)
    return fields[19]

before = starttime()
if Path(os.readlink(process / "exe")) != expected:
    raise SystemExit(1)
expected_info = expected.lstat()
if not stat.S_ISREG(expected_info.st_mode) or expected_info.st_nlink != 1:
    raise SystemExit(1)
descriptor = os.open(process / "exe", os.O_RDONLY | getattr(os, "O_CLOEXEC", 0))
try:
    live_info = os.fstat(descriptor)
    if not stat.S_ISREG(live_info.st_mode) or (live_info.st_dev, live_info.st_ino) != (
        expected_info.st_dev,
        expected_info.st_ino,
    ):
        raise SystemExit(1)
    digest = hashlib.sha256()
    while chunk := os.read(descriptor, 1024 * 1024):
        digest.update(chunk)
finally:
    os.close(descriptor)
after = starttime()
if before != after or Path(os.readlink(process / "exe")) != expected:
    raise SystemExit(1)
if digest.hexdigest() != expected_digest:
    raise SystemExit(1)
print(before, live_info.st_dev, live_info.st_ino, digest.hexdigest())
PY
}

verify_process_identity() {
  local pid=$1 expected_path=$2 expected_identity=$3 actual
  actual=$(process_identity "$pid" "$expected_path") || return 1
  [[ $actual == "$expected_identity" ]]
}

STABLE_UNIT=
STABLE_PID=
active_stable_units=()
for candidate in claude-api-gemini@8795.service claude-api-gemini@8799.service; do
  if $SYSTEMCTL is-active --quiet "$candidate" 2>/dev/null; then
    active_stable_units+=("$candidate")
  fi
done
[[ ${#active_stable_units[@]} -eq 1 ]] || wd_die "stable Gemini must have exactly one active slot"
STABLE_UNIT=${active_stable_units[0]}
systemctl_main_pid "$STABLE_UNIT" || wd_die "stable Gemini PID is unavailable"
STABLE_PID=$REPLY
STABLE_IDENTITY=$(process_identity "$STABLE_PID" "$RELEASE_BINARY") \
  || wd_die "stable Gemini does not run the sealed producer bytes"

ready_once() {
  local port=$1 body
  body=$($CURL --silent --show-error --fail --noproxy '*' --proto '=http' \
    --connect-timeout 2 --max-time 5 "http://127.0.0.1:$port/ready" 2>/dev/null) || return 1
  "$PYTHON" -I -P -B -S -c \
    'import json,sys; value=json.load(sys.stdin); raise SystemExit(0 if value == {"ready": True} else 1)' \
    <<<"$body" 2>/dev/null
}

wait_ready() {
  local port=$1 attempt
  for (( attempt=0; attempt<60; attempt++ )); do
    ready_once "$port" && return 0
    $SLEEP 1
  done
  return 1
}

port_closed() {
  local listeners
  listeners=$($SS -ltnH "sport = :$CANARY_PORT" 2>/dev/null) || return 1
  [[ -z $listeners ]]
}

wait_inactive() {
  local attempt
  for (( attempt=0; attempt<30; attempt++ )); do
    $SYSTEMCTL is-active --quiet "$UNIT" 2>/dev/null || return 0
    $SLEEP 1
  done
  return 1
}

CANARY_OWNED=0
CANARY_PID=
CANARY_IDENTITY=
SECRET_DIR=
SNAPSHOT_ROSTER=
SNAPSHOT_CREDENTIAL_DIR=
ADMIN_KEY=
PANEL_KEY=
STABLE_CREDENTIAL_DIGEST=

write_cleanup_marker() {
  "$PYTHON" -I -P -B -S - "$CLEANUP_MARKER" "$CLEANUP_MARKER_TEXT" 2>/dev/null <<'PY'
import os
import sys
from pathlib import Path

path, text = Path(sys.argv[1]), sys.argv[2]
flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
descriptor = os.open(path, flags, 0o600)
with os.fdopen(descriptor, "wb", closefd=True) as output:
    output.write(text.encode("ascii") + b"\n")
    output.flush()
    os.fsync(output.fileno())
directory = os.open(path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
try:
    os.fsync(directory)
finally:
    os.close(directory)
PY
}

cleanup() {
  local rc=$? cleanup_failed=0 current_pid
  trap - EXIT INT TERM HUP
  set +e
  if (( CANARY_OWNED == 1 )); then
    if ! $TIMEOUT 90 $SYSTEMCTL stop "$UNIT" >/dev/null 2>&1; then
      $SYSTEMCTL kill --kill-whom=all --signal=KILL "$UNIT" >/dev/null 2>&1
      $SYSTEMCTL stop "$UNIT" >/dev/null 2>&1
    fi
    wait_inactive || cleanup_failed=1
  fi
  if [[ -n $SECRET_DIR ]]; then
    "$PYTHON" -I -P -B -S - "$SECRET_DIR" "$RUNTIME_PARENT" "$EXPECTED_OWNER_UID" \
      2>/dev/null <<'PY' || cleanup_failed=1
import os
import re
import stat
import sys
from pathlib import Path

root, runtime_parent, owner = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3])
if not root.is_absolute() or root.parent != runtime_parent or root.name != "apitoken-gemini-3-7-admission":
    raise SystemExit(1)
root_info = root.lstat()
if (
    stat.S_ISLNK(root_info.st_mode)
    or not stat.S_ISDIR(root_info.st_mode)
    or root_info.st_uid != owner
    or stat.S_IMODE(root_info.st_mode) != 0o700
):
    raise SystemExit(1)
allowed = {"admin.key", "panel.key", "profiles.json", "credentials"}
entries = {entry.name: entry for entry in os.scandir(root)}
if not set(entries).issubset(allowed):
    raise SystemExit(1)

credentials = root / "credentials"
if credentials.exists() or credentials.is_symlink():
    info = credentials.lstat()
    if (
        stat.S_ISLNK(info.st_mode)
        or not stat.S_ISDIR(info.st_mode)
        or info.st_uid != owner
        or stat.S_IMODE(info.st_mode) not in {0o500, 0o700}
    ):
        raise SystemExit(1)
    os.chmod(credentials, 0o700, follow_symlinks=False)
    for entry in os.scandir(credentials):
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}\.json", entry.name):
            raise SystemExit(1)
        child_info = entry.stat(follow_symlinks=False)
        if (
            entry.is_symlink()
            or not stat.S_ISREG(child_info.st_mode)
            or child_info.st_nlink != 1
            or child_info.st_uid != owner
            or stat.S_IMODE(child_info.st_mode) not in {0o400, 0o600}
        ):
            raise SystemExit(1)
        os.unlink(entry.path)
    os.rmdir(credentials)

for name in ("admin.key", "panel.key", "profiles.json"):
    path = root / name
    if not path.exists() and not path.is_symlink():
        continue
    info = path.lstat()
    if (
        stat.S_ISLNK(info.st_mode)
        or not stat.S_ISREG(info.st_mode)
        or info.st_nlink != 1
        or info.st_uid != owner
        or stat.S_IMODE(info.st_mode) not in {0o400, 0o600}
    ):
        raise SystemExit(1)
    os.unlink(path)
os.rmdir(root)
PY
  fi
  port_closed || cleanup_failed=1
  if ! $SYSTEMCTL is-active --quiet "$STABLE_UNIT" 2>/dev/null; then
    cleanup_failed=1
  elif systemctl_main_pid "$STABLE_UNIT"; then
    current_pid=$REPLY
    [[ $current_pid == "$STABLE_PID" ]] || cleanup_failed=1
    verify_process_identity "$current_pid" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
      || cleanup_failed=1
  else
    cleanup_failed=1
  fi
  ready_once "$STABLE_PORT" || cleanup_failed=1
  if [[ -n $STABLE_CREDENTIAL_DIGEST && -n $PANEL_KEY ]]; then
    final_credential_digest=$(credential_digest_once "$STABLE_PORT") \
      || cleanup_failed=1
    [[ $final_credential_digest == "$STABLE_CREDENTIAL_DIGEST" ]] \
      || cleanup_failed=1
  fi
  unset GEMINI_ADMISSION_ADMIN_KEY GEMINI_ADMISSION_PANEL_KEY
  ADMIN_KEY=
  PANEL_KEY=
  if (( rc == 0 && cleanup_failed == 0 )); then
    write_cleanup_marker || cleanup_failed=1
  fi
  release_gate_lock
  if (( cleanup_failed != 0 )); then
    printf 'Gemini 3.7 admission cleanup failed safely.\n' >&2
    rc=1
  fi
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

ready_once "$STABLE_PORT" || wd_die "stable Gemini is not ready before admission"
verify_unit_definition || wd_die "Gemini 3.7 admission effective unit differs from its pinned fragment"
unit_state=$($SYSTEMCTL is-enabled "$UNIT" 2>/dev/null || true)
[[ $unit_state == static ]] || wd_die "Gemini 3.7 admission unit is not static"
if $SYSTEMCTL is-active --quiet "$UNIT" 2>/dev/null; then
  wd_die "Gemini 3.7 admission canary was active before its one-shot gate"
fi
port_closed || wd_die "Gemini 3.7 admission port was already open"

# Read the already-loaded admin credentials from the exact stable Gemini process without putting
# either value in argv or output. This is local preflight only: the permanent firing fence is
# created and fsynced below before the first protected /gemini-subs authority GET.
SECRET_DIR=$RUNTIME_PARENT/apitoken-gemini-3-7-admission
SNAPSHOT_ROSTER=$SECRET_DIR/profiles.json
SNAPSHOT_CREDENTIAL_DIR=$SECRET_DIR/credentials
require_immutable_parent_directory "$RUNTIME_PARENT" "admission runtime parent"
if ! "$PYTHON" -I -P -B -S - "$RUNTIME_PARENT" "$SECRET_DIR" "$EXPECTED_OWNER_UID" \
  "$EXPECTED_OWNER_GID" \
  2>/dev/null <<'PY'
import os
import stat
import sys
from pathlib import Path

parent, destination = Path(sys.argv[1]), Path(sys.argv[2])
owner, group = int(sys.argv[3]), int(sys.argv[4])
if not parent.is_absolute() or destination.parent != parent:
    raise SystemExit(1)
parent_info = parent.lstat()
if stat.S_ISLNK(parent_info.st_mode) or not stat.S_ISDIR(parent_info.st_mode):
    raise SystemExit(1)
os.mkdir(destination, 0o700)
os.chown(destination, owner, group, follow_symlinks=False)
os.chmod(destination, 0o700, follow_symlinks=False)
descriptor = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
try:
    os.fsync(descriptor)
finally:
    os.close(descriptor)
PY
then
  wd_die "Gemini 3.7 admission runtime could not be reserved"
fi
require_fixed_directory "$SECRET_DIR" 700 "admission runtime"

if ! "$PYTHON" -I -P -B -S - "$PROC_ROOT" "$STABLE_PID" "$SECRET_DIR" \
  2>/dev/null <<'PY'
import os
import sys
from pathlib import Path

proc_root, pid, destination = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
raw = (proc_root / pid / "environ").read_bytes().split(b"\0")
wanted = {b"CLAUDE_API_KEYS": [], b"CLAUDE_API_PANEL_KEY": []}
for entry in raw:
    name, separator, value = entry.partition(b"=")
    if separator and name in wanted:
        wanted[name].append(value)
if any(len(values) != 1 for values in wanted.values()):
    raise SystemExit(1)

def first_key(name: bytes) -> bytes:
    keys = [value.strip() for value in wanted[name][0].split(b",") if value.strip()]
    if not keys:
        raise SystemExit(1)
    value = keys[0]
    if not 1 <= len(value) <= 4096 or any(byte < 0x21 or byte > 0x7e for byte in value):
        raise SystemExit(1)
    return value

def write_once(name: str, value: bytes) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(destination / name, flags, 0o600)
    with os.fdopen(descriptor, "wb", closefd=True) as output:
        output.write(value + b"\n")
        output.flush()
        os.fsync(output.fileno())

write_once("admin.key", first_key(b"CLAUDE_API_KEYS"))
write_once("panel.key", first_key(b"CLAUDE_API_PANEL_KEY"))
directory = os.open(destination, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
try:
    os.fsync(directory)
finally:
    os.close(directory)
PY
then
  wd_die "Gemini 3.7 admission credentials are unavailable"
fi
systemctl_main_pid "$STABLE_UNIT" || wd_die "stable Gemini PID changed during credential acquisition"
[[ $REPLY == "$STABLE_PID" ]] || wd_die "stable Gemini PID changed during credential acquisition"
verify_process_identity "$STABLE_PID" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
  || wd_die "stable Gemini binary changed during credential acquisition"
require_fixed_file "$SECRET_DIR/admin.key" 600 \
  "$($SHA256SUM -- "$SECRET_DIR/admin.key" | { read -r digest _; printf '%s' "$digest"; })" \
  "admission admin credential"
require_fixed_file "$SECRET_DIR/panel.key" 600 \
  "$($SHA256SUM -- "$SECRET_DIR/panel.key" | { read -r digest _; printf '%s' "$digest"; })" \
  "admission panel credential"
IFS= read -r ADMIN_KEY < "$SECRET_DIR/admin.key"
IFS= read -r PANEL_KEY < "$SECRET_DIR/panel.key"
[[ -n $ADMIN_KEY && -n $PANEL_KEY ]] || wd_die "Gemini 3.7 admission credentials are empty"

# The SHA-keyed root is the irreversible dispatch fence. Every digest/snapshot failure from this
# point is a permanent withdrawal: re-entry can only inspect retained terminal success offline.
"$PYTHON" -I -P -B -S - "$STATE_PARENT" "$ROOT" "$BOUND_PRODUCER_DIGEST" \
  "$PRODUCER_DIGEST" <<'PY'
import os
import sys
from pathlib import Path

parent, root, digest_path, digest = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4]
os.mkdir(root, 0o700)
os.chmod(root, 0o700, follow_symlinks=False)
flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
descriptor = os.open(digest_path, flags, 0o600)
with os.fdopen(descriptor, "w", encoding="ascii", closefd=True) as output:
    output.write(digest + "\n")
    output.flush()
    os.fsync(output.fileno())
descriptor = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
try:
    os.fsync(descriptor)
finally:
    os.close(descriptor)
PY

credential_digest_once() {
  local port=$1
  GEMINI_ADMISSION_PANEL_KEY=$PANEL_KEY "$PYTHON" -I -P -B -S - "$port" 2>/dev/null <<'PY'
import http.client
import json
import os
import re
import sys

key = os.environ.pop("GEMINI_ADMISSION_PANEL_KEY", "")
try:
    port = int(sys.argv[1])
except ValueError:
    raise SystemExit(1)
if not 1 <= port <= 65535 or not 1 <= len(key) <= 4096:
    raise SystemExit(1)
connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
try:
    connection.request(
        "GET",
        "/gemini-subs",
        headers={"accept": "application/json", "connection": "close", "x-api-key": key},
    )
    response = connection.getresponse()
    content_type = response.getheader("content-type", "").split(";", 1)[0].strip().lower()
    payload = response.read(3 * 1024 * 1024 + 1)
finally:
    connection.close()
    key = ""
if response.status != 200 or content_type != "application/json" or len(payload) > 3 * 1024 * 1024:
    raise SystemExit(1)
try:
    document = json.loads(payload)
except (UnicodeError, json.JSONDecodeError):
    raise SystemExit(1)
digest = document.get("credential_generation_digest") if isinstance(document, dict) else None
if not isinstance(digest, str) or not re.fullmatch(r"blake3:[0-9a-f]{64}", digest):
    raise SystemExit(1)
print(digest)
PY
}

STABLE_CREDENTIAL_DIGEST=$(credential_digest_once "$STABLE_PORT") \
  || wd_die "stable Gemini credential generation is unavailable before snapshot"
verify_process_identity "$STABLE_PID" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
  || wd_die "stable Gemini binary changed before credential snapshot"

# Construct a new root-private immutable view from the exact production sealed-roster layout. The
# view contains only referenced envelopes; roster paths are rewritten to systemd's flattened %d
# namespace. Source and destination payloads are never printed.
STABLE_OWNER_UID=$($STAT -c '%u' -- "$PROC_ROOT/$STABLE_PID") \
  || wd_die "stable Gemini runtime owner is unavailable"
[[ $STABLE_OWNER_UID =~ ^[0-9]+$ ]] || wd_die "stable Gemini runtime owner is invalid"
STABLE_OWNER_GID=$($STAT -c '%g' -- "$PROC_ROOT/$STABLE_PID") \
  || wd_die "stable Gemini runtime group is unavailable"
[[ $STABLE_OWNER_GID =~ ^[0-9]+$ ]] || wd_die "stable Gemini runtime group is invalid"
if ! PROFILE_DIGEST=$("$PYTHON" -I -P -B -S - \
    "$PROFILES_SOURCE" "$SECRET_DIR" "$STABLE_OWNER_UID" "$STABLE_OWNER_GID" \
    "$EXPECTED_OWNER_UID" "$EXPECTED_OWNER_GID" "/run/credentials/$UNIT" 2>/dev/null <<'PY'
import hashlib
import json
import os
import re
import stat
import sys
from pathlib import Path

source = Path(sys.argv[1])
destination = Path(sys.argv[2])
source_owner = int(sys.argv[3])
source_group = int(sys.argv[4])
snapshot_owner = int(sys.argv[5])
snapshot_group = int(sys.argv[6])
credential_runtime = Path(sys.argv[7])
max_roster = 1024 * 1024
max_envelope = 1024 * 1024
max_total = 32 * 1024 * 1024
max_profiles = 4096
stable_fields = (
    "st_dev",
    "st_ino",
    "st_mode",
    "st_nlink",
    "st_uid",
    "st_gid",
    "st_size",
    "st_mtime_ns",
    "st_ctime_ns",
)

def exact_directory(path: Path, owner: int, group: int) -> os.stat_result:
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise SystemExit(1)
    info = path.lstat()
    if (
        stat.S_ISLNK(info.st_mode)
        or not stat.S_ISDIR(info.st_mode)
        or info.st_uid != owner
        or info.st_gid != group
        or stat.S_IMODE(info.st_mode) != 0o700
    ):
        raise SystemExit(1)
    return info

def private_file(path: Path, owner: int, group: int, maximum: int) -> bytes:
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise SystemExit(1)
    before = path.lstat()
    if (
        stat.S_ISLNK(before.st_mode)
        or not stat.S_ISREG(before.st_mode)
        or before.st_nlink != 1
        or before.st_uid != owner
        or before.st_gid != group
        or stat.S_IMODE(before.st_mode) != 0o600
        or before.st_size > maximum
    ):
        raise SystemExit(1)
    descriptor = os.open(
        path,
        os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0),
    )
    try:
        opened = os.fstat(descriptor)
        if any(getattr(opened, field) != getattr(before, field) for field in stable_fields):
            raise SystemExit(1)
        payload = bytearray()
        while chunk := os.read(descriptor, min(65536, maximum + 1 - len(payload))):
            payload.extend(chunk)
            if len(payload) > maximum:
                raise SystemExit(1)
    finally:
        os.close(descriptor)
    after = path.lstat()
    if any(getattr(after, field) != getattr(before, field) for field in stable_fields):
        raise SystemExit(1)
    return bytes(payload)

if source.name != "profiles.json" or not source.is_absolute():
    raise SystemExit(1)
roster_root = source.parent
credential_root = roster_root / "credentials"
roster_root_before = exact_directory(roster_root, source_owner, source_group)
credential_root_before = exact_directory(credential_root, source_owner, source_group)
roster_raw = private_file(source, source_owner, source_group, max_roster)
try:
    document = json.loads(roster_raw)
except (UnicodeError, json.JSONDecodeError):
    raise SystemExit(1)
if not isinstance(document, dict) or set(document) != {"profiles"}:
    raise SystemExit(1)
profiles = document["profiles"]
if not isinstance(profiles, list) or not 1 <= len(profiles) <= max_profiles:
    raise SystemExit(1)

ids: set[str] = set()
paths: set[Path] = set()
loaded: list[tuple[str, bytes]] = []
total = len(roster_raw)
for profile in profiles:
    if not isinstance(profile, dict) or set(profile) != {"id", "credential_file"}:
        raise SystemExit(1)
    profile_id = profile["id"]
    credential_file = profile["credential_file"]
    if not isinstance(profile_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", profile_id):
        raise SystemExit(1)
    if profile_id in ids or not isinstance(credential_file, str):
        raise SystemExit(1)
    ids.add(profile_id)
    expected = credential_root / f"{profile_id}.json"
    path = Path(credential_file)
    if path != expected or path in paths:
        raise SystemExit(1)
    paths.add(path)
    payload = private_file(path, source_owner, source_group, max_envelope)
    try:
        envelope = json.loads(payload)
    except (UnicodeError, json.JSONDecodeError):
        raise SystemExit(1)
    if not isinstance(envelope, dict):
        raise SystemExit(1)
    total += len(payload)
    if total > max_total:
        raise SystemExit(1)
    loaded.append((profile_id, payload))

roster_root_after = roster_root.lstat()
credential_root_after = credential_root.lstat()
if any(
    getattr(roster_root_after, field) != getattr(roster_root_before, field)
    for field in stable_fields
) or any(
    getattr(credential_root_after, field) != getattr(credential_root_before, field)
    for field in stable_fields
):
    raise SystemExit(1)

destination_info = destination.lstat()
if (
    stat.S_ISLNK(destination_info.st_mode)
    or not stat.S_ISDIR(destination_info.st_mode)
    or destination_info.st_uid != snapshot_owner
    or stat.S_IMODE(destination_info.st_mode) != 0o700
    or {entry.name for entry in os.scandir(destination)} != {"admin.key", "panel.key"}
):
    raise SystemExit(1)
snapshot_credentials = destination / "credentials"
os.mkdir(snapshot_credentials, 0o700)
os.chown(snapshot_credentials, snapshot_owner, snapshot_group, follow_symlinks=False)
flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)

def write_once(path: Path, payload: bytes) -> None:
    descriptor = os.open(path, flags, 0o600)
    with os.fdopen(descriptor, "wb", closefd=True) as output:
        output.write(payload)
        output.flush()
        os.fchown(output.fileno(), snapshot_owner, snapshot_group)
        os.fchmod(output.fileno(), 0o400)
        os.fsync(output.fileno())

for profile_id, payload in loaded:
    write_once(snapshot_credentials / f"{profile_id}.json", payload)

rewritten = {
    "profiles": [
        {
            "id": profile_id,
            "credential_file": str(credential_runtime / f"gemini-credential_{profile_id}.json"),
        }
        for profile_id, _ in loaded
    ]
}
roster_payload = (json.dumps(rewritten, sort_keys=True, separators=(",", ":")) + "\n").encode()
if len(roster_payload) > max_roster:
    raise SystemExit(1)
write_once(destination / "profiles.json", roster_payload)
os.chmod(snapshot_credentials, 0o500, follow_symlinks=False)
for path in (snapshot_credentials, destination):
    descriptor = os.open(
        path,
        os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
    )
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
print(hashlib.sha256(roster_payload).hexdigest())
PY
  )
then
  wd_die "Gemini 3.7 credential snapshot could not be sealed"
fi
[[ $PROFILE_DIGEST =~ ^[0-9a-f]{64}$ ]] \
  || wd_die "Gemini 3.7 credential snapshot digest is invalid"
require_fixed_directory "$SNAPSHOT_CREDENTIAL_DIR" 500 "admission credential snapshot"
require_fixed_file "$SNAPSHOT_ROSTER" 400 "$PROFILE_DIGEST" "admission roster snapshot"

STABLE_CREDENTIAL_DIGEST_AFTER=$(credential_digest_once "$STABLE_PORT") \
  || wd_die "stable Gemini credential generation is unavailable after snapshot"
[[ $STABLE_CREDENTIAL_DIGEST_AFTER == "$STABLE_CREDENTIAL_DIGEST" ]] \
  || wd_die "stable Gemini credential generation changed during snapshot"
verify_process_identity "$STABLE_PID" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
  || wd_die "stable Gemini binary changed during credential snapshot"

CANARY_OWNED=1
$SYSTEMCTL start "$UNIT" >/dev/null 2>&1 || wd_die "Gemini 3.7 admission canary did not start"
verify_unit_definition || wd_die "Gemini 3.7 admission effective unit changed while starting"
wait_ready "$CANARY_PORT" || wd_die "Gemini 3.7 admission canary did not become ready"
verify_unit_definition || wd_die "Gemini 3.7 admission effective unit changed during startup"
systemctl_main_pid "$UNIT" || wd_die "Gemini 3.7 admission canary PID is unavailable"
CANARY_PID=$REPLY
CANARY_IDENTITY=$(process_identity "$CANARY_PID" "$PRODUCER_BINARY") \
  || wd_die "Gemini 3.7 admission canary does not run the sealed producer bytes"
CANARY_CREDENTIAL_DIGEST=$(credential_digest_once "$CANARY_PORT") \
  || wd_die "Gemini 3.7 canary credential generation is unavailable"
[[ $CANARY_CREDENTIAL_DIGEST == "$STABLE_CREDENTIAL_DIGEST" ]] \
  || wd_die "Gemini 3.7 canary credential generation differs from stable"
STABLE_CREDENTIAL_DIGEST_AFTER_CANARY=$(credential_digest_once "$STABLE_PORT") \
  || wd_die "stable Gemini credential generation is unavailable after canary start"
[[ $STABLE_CREDENTIAL_DIGEST_AFTER_CANARY == "$STABLE_CREDENTIAL_DIGEST" ]] \
  || wd_die "stable Gemini credential generation changed while starting canary"
systemctl_main_pid "$STABLE_UNIT" || wd_die "stable Gemini PID changed while starting canary"
[[ $REPLY == "$STABLE_PID" ]] || wd_die "stable Gemini PID changed while starting canary"
verify_process_identity "$STABLE_PID" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
  || wd_die "stable Gemini binary changed while starting canary"

GEMINI_ADMISSION_PANEL_KEY=$PANEL_KEY \
  "$PYTHON" -I -P -B -S "$TRANSPORT" capacity \
  --evidence-dir "$EVIDENCE" --output "$CAPACITY" \
  >/dev/null 2>&1 || wd_die "Gemini 3.7 immutable capacity preflight failed"

# Select one healthy paid profile deterministically without ever emitting its opaque id. Prefer an
# exact quota row with the largest remaining value, then the broadest paid plan, then lexical id.
"$PYTHON" -I -P -B -S - "$CAPACITY" "$PROFILE_ID" "$PLAN_FILE" <<'PY'
import decimal
import json
import os
import re
import sys
from pathlib import Path

capacity_path, profile_path, plan_path = map(Path, sys.argv[1:])
payload = json.loads(capacity_path.read_text(encoding="utf-8"))
now = payload.get("now")
if isinstance(now, bool) or not isinstance(now, int):
    raise SystemExit(1)
plans = {
    "google_ai_ultra": 5,
    "google_ai_pro": 4,
    "workspace_ai_ultra": 3,
    "code_assist_enterprise": 2,
    "code_assist_standard": 1,
}
profile_re = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
candidates = []
for profile in payload.get("profiles", []):
    if not isinstance(profile, dict):
        continue
    profile_id, plan = profile.get("id"), profile.get("plan")
    if not isinstance(profile_id, str) or not profile_re.fullmatch(profile_id) or plan not in plans:
        continue
    if (
        profile.get("authenticated") is not True
        or profile.get("calibration_persistence_ok") is not True
        or profile.get("disabled") is True
        or profile.get("hidden") is True
        or not isinstance(profile.get("cooling_until", 0), int)
        or profile.get("cooling_until", 0) > now
    ):
        continue
    quota = decimal.Decimal(-1)
    for row in profile.get("quotas", []):
        if not isinstance(row, dict) or row.get("model_id") != "gemini-3.7-flash":
            continue
        raw = row.get("remaining_amount")
        if raw is None:
            raw = row.get("remaining_fraction")
        try:
            value = decimal.Decimal(str(raw))
        except decimal.InvalidOperation:
            continue
        quota = max(quota, value)
    candidates.append((quota, plans[plan], profile_id, plan))
if not candidates:
    raise SystemExit(1)
candidates.sort(key=lambda item: (-item[0], -item[1], item[2]))
_, _, profile_id, plan = candidates[0]

def write_once(path: Path, value: str) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8", closefd=True) as output:
        output.write(value + "\n")
        output.flush()
        os.fsync(output.fileno())

write_once(profile_path, profile_id)
write_once(plan_path, plan)
directory = os.open(profile_path.parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
try:
    os.fsync(directory)
finally:
    os.close(directory)
PY
PLAN=
IFS= read -r PLAN < "$PLAN_FILE"
case "$PLAN" in
  google_ai_pro|google_ai_ultra|code_assist_standard|code_assist_enterprise|workspace_ai_ultra) ;;
  *) wd_die "Gemini 3.7 admission selected no supported paid plan" ;;
esac

run_admission init \
  --evidence-dir "$EVIDENCE" --capacity-file "$CAPACITY" --profile-id-file "$PROFILE_ID" \
  --plan "$PLAN" --implementation-sha "$SHA" --release-sha "$SHA" \
  --budget-nanousd "$BUDGET_NANOUSD" --max-output-tokens "$MAX_OUTPUT_TOKENS" --stream \
  >/dev/null 2>&1 || wd_die "Gemini 3.7 admission contract initialization failed"

GEMINI_ADMISSION_ADMIN_KEY=$ADMIN_KEY \
  "$PYTHON" -I -P -B -S "$TRANSPORT" count --library-root "$CONTROLLER_ROOT" \
  --evidence-dir "$EVIDENCE" --output "$COUNT_OBSERVATION" \
  >/dev/null 2>&1 || wd_die "Gemini 3.7 countTokens attempt is terminally unavailable"

run_admission arm-generation --evidence-dir "$EVIDENCE" \
  >/dev/null 2>&1 || wd_die "Gemini 3.7 paid generation could not be armed"
systemctl_main_pid "$UNIT" || wd_die "Gemini 3.7 canary stopped before paid dispatch"
[[ $REPLY == "$CANARY_PID" ]] || wd_die "Gemini 3.7 canary PID changed before paid dispatch"
verify_process_identity "$CANARY_PID" "$PRODUCER_BINARY" "$CANARY_IDENTITY" \
  || wd_die "Gemini 3.7 canary binary changed before paid dispatch"
verify_unit_definition || wd_die "Gemini 3.7 admission effective unit changed before paid dispatch"
systemctl_main_pid "$STABLE_UNIT" || wd_die "stable Gemini stopped before paid dispatch"
[[ $REPLY == "$STABLE_PID" ]] || wd_die "stable Gemini PID changed before paid dispatch"
verify_process_identity "$STABLE_PID" "$RELEASE_BINARY" "$STABLE_IDENTITY" \
  || wd_die "stable Gemini binary changed before paid dispatch"
STABLE_CREDENTIAL_DIGEST_BEFORE_PAID=$(credential_digest_once "$STABLE_PORT") \
  || wd_die "stable Gemini credential generation is unavailable before paid dispatch"
[[ $STABLE_CREDENTIAL_DIGEST_BEFORE_PAID == "$STABLE_CREDENTIAL_DIGEST" ]] \
  || wd_die "stable Gemini credential generation changed before paid dispatch"

GEMINI_ADMISSION_ADMIN_KEY=$ADMIN_KEY GEMINI_ADMISSION_PANEL_KEY=$PANEL_KEY \
  "$PYTHON" -I -P -B -S "$TRANSPORT" generate \
  --library-root "$CONTROLLER_ROOT" --evidence-dir "$EVIDENCE" \
  --output "$OUTCOME_OBSERVATION" \
  >/dev/null 2>&1 || wd_die "Gemini 3.7 paid generation is terminally unavailable"

systemctl_main_pid "$UNIT" || wd_die "Gemini 3.7 canary stopped before evidence inspection"
[[ $REPLY == "$CANARY_PID" ]] || wd_die "Gemini 3.7 canary PID changed before evidence inspection"
verify_process_identity "$CANARY_PID" "$PRODUCER_BINARY" "$CANARY_IDENTITY" \
  || wd_die "Gemini 3.7 canary binary changed before evidence inspection"
inspect_success
