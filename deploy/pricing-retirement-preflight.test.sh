#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
PREFLIGHT="$ROOT/deploy/pricing-retirement-preflight.sh"

fail() {
  printf 'pricing retirement preflight test failed: %s\n' "$*" >&2
  exit 1
}

bash "$PREFLIGHT" --validate-manifest >/dev/null
if bash "$PREFLIGHT" --report extra >/dev/null 2>&1; then
  fail 'diagnostic mode accepted an extra argument'
fi
if bash "$PREFLIGHT" --final neither not-a-sha >/dev/null 2>&1; then
  fail 'final mode accepted an invalid plane/SHA pair'
fi

# The ordinary deployment suite is hermetic and stops here. An audit operator may additionally set
# this to a Docker SSH transport (for example ssh://the documented production alias) to exercise
# every read-only SQL/hash/dependency/health check against the live databases. Release and systemd
# inputs remain isolated fixtures, so this mode cannot select a release or mutate a service.
LIVE_DOCKER_HOST=${PRICING_RETIREMENT_LIVE_DOCKER_HOST:-}
if [[ -z $LIVE_DOCKER_HOST ]]; then
  printf 'pricing retirement preflight contract tests passed (live read-only exercise skipped)\n'
  exit 0
fi

fixture=$(mktemp -d "${TMPDIR:-/tmp}/pricing-preflight-fixture.XXXXXX") \
  || fail 'could not create a live-exercise fixture'
cleanup() {
  rm -rf -- "$fixture"
}
trap cleanup EXIT

head=$(git -C "$ROOT" rev-parse HEAD)
engine_floor=e8cf49ae121b581042c582ddb3621ee29fae8103
commerce_floor=0c236aa2334f539786f53429d815d6b7c791adbe
mkdir -p \
  "$fixture/engine/$head" "$fixture/engine/$engine_floor" \
  "$fixture/commerce/$head" "$fixture/commerce/$commerce_floor" \
  "$fixture/state"
ln -s "$fixture/engine/$head" "$fixture/engine/current"
ln -s "$fixture/engine/$engine_floor" "$fixture/engine/previous"
ln -s "$fixture/commerce/$head" "$fixture/commerce/current"
ln -s "$fixture/commerce/$commerce_floor" "$fixture/commerce/previous"
printf '%s\n' "$head" >"$fixture/state/engine.sha"
printf '%s\n' "$head" >"$fixture/state/backend.sha"
printf '%s\n' "$head" >"$fixture/state/processed.sha"

systemctl() {
  if [[ ${1:-} == is-active && ${3:-} == apitoken-sales-api.service ]]; then return 0; fi
  if [[ ${1:-} == show && ${2:-} == apitoken-sales-api.service ]]; then
    printf '%s\n' '2026-08-11 00:00:00 UTC'
    return 0
  fi
  if [[ ${1:-} == show ]]; then printf '%s\n' not-found; return 0; fi
  return 1
}
journalctl() { return 0; }
sudo() {
  [[ ${1:-} == -n ]] && shift
  "$@"
}
export -f systemctl journalctl sudo

set +e
DOCKER_HOST=$LIVE_DOCKER_HOST \
PRICING_RETIREMENT_POSTGRES_COMPOSE_FILE="$fixture/no-compose" \
PRICING_RETIREMENT_POSTGRES_ENV="$fixture/no-env" \
PRICING_RETIREMENT_POSTGRES_CONTAINER=${PRICING_RETIREMENT_LIVE_CONTAINER:-deploy-commerce-postgres-1} \
PRICING_RETIREMENT_ENGINE_RELEASE_ROOT="$fixture/engine" \
PRICING_RETIREMENT_COMMERCE_RELEASE_ROOT="$fixture/commerce" \
PRICING_RETIREMENT_WATCHDOG_STATE="$fixture/state" \
PRICING_RETIREMENT_SOURCE_REPO="$ROOT" \
PRICING_RETIREMENT_AUTHBOT_RUNTIME_STATE=/usr/bin/true \
bash "$PREFLIGHT" --report >"$fixture/report" 2>&1
status=$?
set -e
[[ $status == 3 ]] || { sed -n '1,200p' "$fixture/report" >&2; fail "live report returned $status, expected 3"; }
for evidence in source-contract rollback-floor watermark:pricing watermark:sales business-health \
  immutable-evidence:engine immutable-evidence:commerce dependency-graph:engine \
  dependency-graph:commerce retention-evidence:engine retention-evidence:commerce 'NOT AUTHORIZED'; do
  grep -Fq "$evidence" "$fixture/report" \
    || { sed -n '1,200p' "$fixture/report" >&2; fail "live report omitted $evidence"; }
done

printf 'pricing retirement preflight contract and live read-only tests passed\n'
