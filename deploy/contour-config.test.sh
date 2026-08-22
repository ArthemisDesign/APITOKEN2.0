#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

SCHEMA=$ROOT/deploy/contour-config.schema.json
PRODUCTION=$ROOT/deploy/contour-production.json
VALIDATOR=$ROOT/deploy/contour-config.py
EXPECTED=$ROOT/deploy/test-fixtures/contour-config/production-resolved.txt

fail() { printf 'contour-config.test: FAIL: %s\n' "$*" >&2; exit 1; }
expect_reject() {
  local label=$1 expected=$2
  shift 2
  if "$@" >"$TEMP/$label.out" 2>"$TEMP/$label.err"; then
    fail "$label was accepted"
  fi
  grep -Fq -- "$expected" "$TEMP/$label.err" || {
    sed -n '1,20p' "$TEMP/$label.err" >&2
    fail "$label did not report $expected"
  }
}

python3 "$VALIDATOR" --schema "$SCHEMA" --config "$PRODUCTION" --emit shell \
  >"$TEMP/production.env"
diff -u "$EXPECTED" "$TEMP/production.env"

cp "$PRODUCTION" "$TEMP/missing.json"
python3 - "$TEMP/missing.json" <<'PY'
import json, sys
path = sys.argv[1]
value = json.load(open(path))
del value["locks"]
json.dump(value, open(path, "w"))
PY
expect_reject missing 'missing required field(s): locks' \
  python3 "$VALIDATOR" --schema "$SCHEMA" --config "$TEMP/missing.json"

cp "$PRODUCTION" "$TEMP/unknown.json"
python3 - "$TEMP/unknown.json" <<'PY'
import json, sys
path = sys.argv[1]
value = json.load(open(path))
value["unexpected_inventory"] = "/production/drift"
json.dump(value, open(path, "w"))
PY
expect_reject unknown 'unknown inventory field(s): unexpected_inventory' \
  python3 "$VALIDATOR" --schema "$SCHEMA" --config "$TEMP/unknown.json"

for collision in user root lock port unit context environment compose; do
  cp "$ROOT/deploy/test-fixtures/contour-config/stage-safe.json" "$TEMP/stage-$collision.json"
  python3 - "$PRODUCTION" "$TEMP/stage-$collision.json" "$collision" <<'PY'
import json, sys
prod_path, stage_path, collision = sys.argv[1:]
prod = json.load(open(prod_path))
stage = json.load(open(stage_path))
if collision == "user": stage["identity"]["runtime_user"] = prod["identity"]["runtime_user"]
elif collision == "root": stage["roots"]["state"] = prod["roots"]["state"] + "/stage"
elif collision == "lock": stage["locks"]["watchdog"] = prod["locks"]["watchdog"]
elif collision == "port": stage["network"]["namespace"] = prod["network"]["namespace"]; stage["ports"]["postgres"] = prod["ports"]["postgres"]
elif collision == "unit": stage["units"]["watchdog_service"] = prod["units"]["watchdog_service"]
elif collision == "context": stage["github"]["status_contexts"]["watchdog"] = prod["github"]["status_contexts"]["watchdog"]
elif collision == "environment": stage["github"]["deployment_environments"]["database"] = prod["github"]["deployment_environments"]["database"]
elif collision == "compose": stage["compose_projects"]["postgres"] = prod["compose_projects"]["postgres"]
json.dump(stage, open(stage_path, "w"))
PY
  expect_reject "overlap-$collision" 'overlap' \
    python3 "$VALIDATOR" --schema "$SCHEMA" --config "$PRODUCTION" \
      --against "$TEMP/stage-$collision.json"
done

# A future netns can reuse numeric application ports. The overlap rule compares endpoint identity by
# namespace. Roots, users, locks, units, reporting, and Compose identities still remain disjoint.
python3 "$VALIDATOR" --schema "$SCHEMA" --config "$PRODUCTION" \
  --against "$ROOT/deploy/test-fixtures/contour-config/stage-safe.json" >/dev/null

expect_reject injected 'invalid value' \
  python3 "$VALIDATOR" --schema "$SCHEMA" \
    --config "$ROOT/deploy/test-fixtures/contour-config/shell-injection.json" --emit shell

if CONTOUR_CONFIG_FILE="$TEMP/missing.json" bash -c \
    'source "$1/deploy/contour-config.sh"' _ "$ROOT" >"$TEMP/loader.out" 2>"$TEMP/loader.err"; then
  fail 'Bash loader swallowed validator failure'
fi
grep -Fq 'production contour validation failed' "$TEMP/loader.err" \
  || fail 'Bash loader did not fail closed'

python3 -m json.tool "$SCHEMA" >/dev/null
python3 -m json.tool "$PRODUCTION" >/dev/null
printf 'contour-config.test: PASS\n'
