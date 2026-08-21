#!/usr/bin/env bash
set -euo pipefail

# Fail-closed pre-start gate for inactive large-payload slots. This script is repository-owned and
# runs before systemctl start; it never changes live units or traffic.
MIN_AVAILABLE_BYTES=${LARGE_PAYLOAD_MIN_AVAILABLE_BYTES:-12884901888} # 12 GiB
# RuntimeDirectory currently lives on the host's 10 GiB /run tmpfs. The in-process production
# spool budget is still pinned to 512 MiB and threshold=request, so require 8 GiB free here. The
# 16 GiB authority belongs to the later disk-backed threshold rollout on a quota-backed filesystem.
MIN_SPOOL_BYTES=${LARGE_PAYLOAD_MIN_SPOOL_BYTES:-8589934592}           # 8 GiB
SPOOL_ROOT=${1:?usage: large-payload-headroom.sh <spool-root> [parent-slice]}
PARENT_SLICE=${2:-}

[[ $MIN_AVAILABLE_BYTES =~ ^[1-9][0-9]*$ && $MIN_SPOOL_BYTES =~ ^[1-9][0-9]*$ ]] || exit 2
[[ $SPOOL_ROOT == /* && ! -L $SPOOL_ROOT ]] || exit 1
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
if [[ -n $PARENT_SLICE ]]; then
  [[ $PARENT_SLICE =~ ^[A-Za-z0-9@_.-]+\.slice$ ]] || exit 2
  current=$(systemctl show "$PARENT_SLICE" -p MemoryCurrent --value)
  maximum=$(systemctl show "$PARENT_SLICE" -p MemoryMax --value)
  [[ $current =~ ^[0-9]+$ && $maximum =~ ^[0-9]+$ ]] || exit 1
  (( current < maximum )) || exit 1
fi
printf 'headroom-ok mem=%s spool=%s\n' "$available_bytes" "$spool_bytes"
