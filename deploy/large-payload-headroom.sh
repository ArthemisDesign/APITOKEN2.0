#!/usr/bin/env bash
set -euo pipefail

# Fail-closed pre-start gate for inactive large-payload slots. This script is repository-owned and
# runs before systemctl start; it never changes live units or traffic.
MIN_AVAILABLE_BYTES=${LARGE_PAYLOAD_MIN_AVAILABLE_BYTES:-12884901888} # 12 GiB
SPOOL_ROOT=${1:?usage: large-payload-headroom.sh <spool-root> [parent-slice]}
PARENT_SLICE=${2:-}

[[ $SPOOL_ROOT == /* && ! -L $SPOOL_ROOT ]] || exit 1

# Path-aware spool policy. Uniform 16 GiB + tmpfs/ramfs reject took down Anthropic cutover on
# 0e822c9d: engine-bluegreen still gates `/run/claude-api-anthropic-*` on the host's 10 GiB /run
# tmpfs. Disk-backed 8 MiB spill (router @, future Gemini @) keeps the 16 GiB floor.
DISK_SPOOL_PREFIX=/var/lib/apitoken/spool/
RUN_ANTHROPIC_PREFIX=/run/claude-api-anthropic-
if [[ $SPOOL_ROOT == "${DISK_SPOOL_PREFIX}"* ]]; then
  MIN_SPOOL_BYTES=${LARGE_PAYLOAD_MIN_SPOOL_BYTES:-17179869184} # 16 GiB
  REJECT_VOLATILE_FS=1
elif [[ $SPOOL_ROOT == "${RUN_ANTHROPIC_PREFIX}"* ]]; then
  MIN_SPOOL_BYTES=${LARGE_PAYLOAD_MIN_SPOOL_BYTES:-8589934592}  # 8 GiB
  REJECT_VOLATILE_FS=0
else
  exit 1
fi

[[ $MIN_AVAILABLE_BYTES =~ ^[1-9][0-9]*$ && $MIN_SPOOL_BYTES =~ ^[1-9][0-9]*$ ]] || exit 2
[[ $REJECT_VOLATILE_FS == 0 || $REJECT_VOLATILE_FS == 1 ]] || exit 2
# RuntimeDirectory is created by systemd during the start we are gating and disappears after a slot
# stops. When absent, inspect its existing parent filesystem; never create or follow a substitute.
if [[ -d $SPOOL_ROOT ]]; then
  spool_probe=$SPOOL_ROOT
else
  [[ ! -e $SPOOL_ROOT ]]
  spool_probe=${SPOOL_ROOT%/*}
  [[ -d $spool_probe && ! -L $spool_probe ]] || exit 1
fi
available_kib=$(awk '$1=="MemAvailable:" {print $2}' /proc/meminfo)
[[ $available_kib =~ ^[0-9]+$ ]] || exit 1
available_bytes=$((available_kib * 1024))
(( available_bytes >= MIN_AVAILABLE_BYTES )) || exit 1
# GNU df rejects combining POSIX -P with --output. Use the portable POSIX column shape; with one
# filesystem the available 1K blocks are field 4 on the final line.
spool_kib=$(df -Pk -- "$spool_probe" | awk 'END {print $4}')
[[ $spool_kib =~ ^[0-9]+$ ]] || exit 1
spool_bytes=$((spool_kib * 1024))
(( spool_bytes >= MIN_SPOOL_BYTES )) || exit 1
if [[ $REJECT_VOLATILE_FS == 1 ]] && command -v findmnt >/dev/null; then
  fstype=$(findmnt -n -o FSTYPE --target "$spool_probe")
  [[ $fstype != tmpfs && $fstype != ramfs && -n $fstype ]] || exit 1
fi
if [[ -n $PARENT_SLICE ]]; then
  [[ $PARENT_SLICE =~ ^[A-Za-z0-9@_.-]+\.slice$ ]] || exit 2
  current=$(systemctl show "$PARENT_SLICE" -p MemoryCurrent --value)
  maximum=$(systemctl show "$PARENT_SLICE" -p MemoryMax --value)
  [[ $current =~ ^[0-9]+$ && $maximum =~ ^[0-9]+$ ]] || exit 1
  (( current < maximum )) || exit 1
fi
printf 'headroom-ok mem=%s spool=%s\n' "$available_bytes" "$spool_bytes"
