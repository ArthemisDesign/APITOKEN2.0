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
before=$evidence_dir/$sha.before.json; load=$evidence_dir/$sha.load.json; after=$evidence_dir/$sha.after.json; verdict=$evidence_dir/$sha.verdict.json
"$collector" "$unit" "$spool" "$before"
python3 "$script_dir/tests/large_payload_mock_load.py" --url "$url" --sizes-mib "8,32,64" --concurrency 4 --authorization-file "$authorization_file" >"$load"
"$collector" "$unit" "$spool" "$after"
verdict_rc=0
python3 "$script_dir/tests/large_payload_candidate_gate.py" --sha "$sha" --before "$before" --after "$after" --load "$load" --memory-high-bytes "$memory_high" >"$verdict.tmp" || verdict_rc=$?
mv -f -- "$verdict.tmp" "$verdict"
exit "$verdict_rc"
