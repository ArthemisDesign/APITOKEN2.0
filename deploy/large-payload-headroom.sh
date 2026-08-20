#!/usr/bin/env bash
set -euo pipefail

# Fail-closed pre-start gate for inactive large-payload slots. This script is repository-owned and
# runs before systemctl start; it never changes live units or traffic.
MIN_AVAILABLE_BYTES=${LARGE_PAYLOAD_MIN_AVAILABLE_BYTES:-12884901888} # 12 GiB
MIN_SPOOL_BYTES=${LARGE_PAYLOAD_MIN_SPOOL_BYTES:-17179869184}          # 16 GiB
SPOOL_ROOT=${1:?usage: large-payload-headroom.sh <spool-root> [parent-slice]}
PARENT_SLICE=${2:-}

[[ $MIN_AVAILABLE_BYTES =~ ^[1-9][0-9]*$ && $MIN_SPOOL_BYTES =~ ^[1-9][0-9]*$ ]] || exit 2
[[ $SPOOL_ROOT == /* && -d $SPOOL_ROOT && ! -L $SPOOL_ROOT ]] || exit 1
available_kib=$(awk '$1=="MemAvailable:" {print $2}' /proc/meminfo)
[[ $available_kib =~ ^[0-9]+$ ]] || exit 1
available_bytes=$((available_kib * 1024))
(( available_bytes >= MIN_AVAILABLE_BYTES )) || exit 1
spool_bytes=$(df -PB1 --output=avail -- "$SPOOL_ROOT" | awk 'NR==2 {print $1}')
[[ $spool_bytes =~ ^[0-9]+$ ]] || exit 1
(( spool_bytes >= MIN_SPOOL_BYTES )) || exit 1
if [[ -n $PARENT_SLICE ]]; then
  [[ $PARENT_SLICE =~ ^[A-Za-z0-9@_.-]+\.slice$ ]] || exit 2
  current=$(systemctl show "$PARENT_SLICE" -p MemoryCurrent --value)
  maximum=$(systemctl show "$PARENT_SLICE" -p MemoryMax --value)
  [[ $current =~ ^[0-9]+$ && $maximum =~ ^[0-9]+$ ]] || exit 1
  (( current < maximum )) || exit 1
fi
printf 'headroom-ok mem=%s spool=%s\n' "$available_bytes" "$spool_bytes"
