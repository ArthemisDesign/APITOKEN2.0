#!/usr/bin/env bash
set -euo pipefail
usage() { echo 'usage: large-payload-candidate-gate.sh <sha> <url> <unit> <spool-root> <memory-high-bytes> <evidence-dir> <authorization-file>' >&2; exit 2; }
[[ $# == 7 ]] || usage
sha=$1 url=$2 unit=$3 spool=$4 memory_high=$5 evidence_dir=$6 authorization_file=$7
[[ $sha =~ ^[0-9a-f]{40}$ && $url == http://127.0.0.1:*/* && $memory_high =~ ^[1-9][0-9]*$ ]] || usage
[[ $evidence_dir == /* && -d $evidence_dir && ! -L $evidence_dir ]] || exit 1
[[ $authorization_file == /srv/claude-api/data/large-payload-canary.authorization && -f $authorization_file && ! -L $authorization_file ]] || exit 1
[[ $(stat -c '%a' -- "$authorization_file") == 600 ]] || exit 1
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
collector=/usr/local/lib/apitoken-watchdog/controller/large-payload-evidence.sh
[[ -x $collector ]] || collector=$script_dir/deploy/large-payload-evidence.sh
before=$evidence_dir/$sha.before.json; load=$evidence_dir/$sha.load.json; after=$evidence_dir/$sha.after.json
verdict=$evidence_dir/$sha.verdict.json; reason=$evidence_dir/$sha.reason
rm -f -- "$reason"

write_reason() {
  local text=$1
  text=$(printf '%s' "$text" | tr -d '\r' | tr '\n' ' ')
  text=$(printf '%s' "$text" | sed -E $'s/[[:cntrl:]]/ /g')
  if [[ $text != payload-canary:* ]]; then
    text="payload-canary: $text"
  fi
  text=${text:0:120}
  printf '%s\n' "$text" >"$reason.tmp"
  mv -f -- "$reason.tmp" "$reason"
  printf '%s\n' "$text" >&2
}

if ! "$collector" "$unit" "$spool" "$before"; then
  write_reason 'payload-canary: before-snapshot failed'
  exit 1
fi
load_rc=0
python3 "$script_dir/tests/large_payload_mock_load.py" --url "$url" --sizes-mib "8,32,64,128,256" --concurrency 4 --authorization-file "$authorization_file" >"$load" 2>"$load.err" || load_rc=$?
if (( load_rc != 0 )); then
  if grep -Fq 'No such file or directory' "$load.err" 2>/dev/null; then
    write_reason 'payload-canary: load-driver missing'
  else
    write_reason "payload-canary: load-driver exit=$load_rc"
  fi
  rm -f -- "$load.err"
  exit "$load_rc"
fi
rm -f -- "$load.err"
if ! "$collector" "$unit" "$spool" "$after"; then
  write_reason 'payload-canary: after-snapshot failed'
  exit 1
fi
verdict_rc=0
python3 "$script_dir/tests/large_payload_candidate_gate.py" --sha "$sha" --before "$before" --after "$after" --load "$load" --memory-high-bytes "$memory_high" >"$verdict.tmp" || verdict_rc=$?
if [[ -f $verdict.tmp ]]; then
  mv -f -- "$verdict.tmp" "$verdict"
else
  write_reason "payload-canary: evaluator-exit=${verdict_rc:-1}"
  exit "${verdict_rc:-1}"
fi
if (( verdict_rc != 0 )); then
  extracted=$(python3 -c 'import json,sys
reason=json.load(open(sys.argv[1],encoding="utf-8")).get("reason","")
reason=reason.splitlines()[0] if isinstance(reason,str) else ""
print(reason if reason.startswith("payload-canary:") else "")
' "$verdict" || true)
  if [[ $extracted == payload-canary:* ]]; then
    write_reason "$extracted"
  else
    write_reason "payload-canary: evaluator-exit=$verdict_rc"
  fi
  exit "$verdict_rc"
fi
exit 0
