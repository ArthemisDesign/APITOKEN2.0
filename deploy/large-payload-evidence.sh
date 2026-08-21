#!/usr/bin/env bash
set -euo pipefail
usage() { echo 'usage: large-payload-evidence.sh <unit> <spool-root> <output-json>' >&2; exit 2; }
[[ $# == 3 ]] || usage
unit=$1 spool=$2 output=$3
[[ $unit =~ ^(claude-router|claude-api-(anthropic|openai|gemini))@[0-9]+\.service$ ]] || usage
[[ $spool == /var/lib/apitoken/spool/* && -d $spool && ! -L $spool ]] || exit 1
[[ $output == /* ]] || usage
cg=$(systemctl show "$unit" -p ControlGroup --value)
[[ $cg == /* && -d /sys/fs/cgroup$cg ]] || exit 1
root=/sys/fs/cgroup$cg
number() { local value; value=$(<"$1"); [[ $value =~ ^[0-9]+$ ]] || exit 1; printf '%s' "$value"; }
current=$(number "$root/memory.current")
peak=$(number "$root/memory.peak")
max=$(<"$root/memory.max")
events=$(awk '{printf "%s%s:%s",sep,$1,$2;sep=","}' "$root/memory.events")
files=$(find "$spool" -mindepth 1 -maxdepth 1 -type f -printf . | wc -c)
fds=$(find "/proc/$(systemctl show "$unit" -p MainPID --value)/fd" -mindepth 1 -maxdepth 1 -printf . 2>/dev/null | wc -c)
printf '{"schema":"large-payload-cgroup-v1","unit":"%s","memory":{"current":%s,"peak":%s,"max":"%s","events":"%s"},"spool_files":%s,"open_fds":%s}\n' \
  "$unit" "$current" "$peak" "$max" "$events" "$files" "$fds" >"$output.tmp"
mv -f -- "$output.tmp" "$output"
