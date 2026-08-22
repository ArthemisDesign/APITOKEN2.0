#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
CHECK=$ROOT/deploy/repository-invariants.py

fail() { printf 'repository-invariants.test: %s\n' "$*" >&2; exit 1; }
expect_failure() { # $1=fixture root $2=expected diagnostic
  local fixture=$1 expected=$2 output status=0
  output=$(python3 "$CHECK" "$fixture" 2>&1) || status=$?
  (( status == 1 )) || fail "expected invariant failure, got $status: $output"
  grep -Fq "$expected" <<<"$output" || fail "missing diagnostic '$expected': $output"
}

python3 "$CHECK" "$ROOT"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

make_fixture() {
  local fixture=$1
  mkdir -p "$fixture/crates/pool/src" "$fixture/crates/registry/src" \
    "$fixture/crates/forward/src" "$fixture/crates/server/src" \
    "$fixture/apps/api/src" "$fixture/packages/engine-client/src"
  printf '[workspace]\nmembers=[]\n' >"$fixture/Cargo.toml"
  printf '{"private":true}\n' >"$fixture/package.json"
  for crate in pool registry forward server; do
    printf '[package]\nname="%s"\nversion="0.1.0"\nedition="2021"\n[dependencies]\n' "$crate" \
      >"$fixture/crates/$crate/Cargo.toml"
    printf 'pub fn ok() {}\n' >"$fixture/crates/$crate/src/lib.rs"
  done
  printf 'pub fn read() { let _ = std::env::var("OK"); }\n' \
    >"$fixture/crates/server/src/config.rs"
  printf '{"name":"@claude-api/engine-client","private":true}\n' \
    >"$fixture/packages/engine-client/package.json"
  printf '{"name":"@claude-api/api","private":true,"dependencies":{"@claude-api/engine-client":"workspace:*"}}\n' \
    >"$fixture/apps/api/package.json"
}

fixture=$TEMP/valid
make_fixture "$fixture"
python3 "$CHECK" "$fixture" >/dev/null

fixture=$TEMP/network-dependency
make_fixture "$fixture"
printf 'reqwest="1"\n' >>"$fixture/crates/pool/Cargo.toml"
expect_failure "$fixture" 'lower layer pool declares network/HTTP dependencies: reqwest'

fixture=$TEMP/network-source
make_fixture "$fixture"
printf 'pub fn dial() { let _ = std::net::TcpStream::connect("127.0.0.1:1"); }\n' \
  >"$fixture/crates/registry/src/lib.rs"
expect_failure "$fixture" 'lower layer registry uses network/HTTP source token'

fixture=$TEMP/env-owner
make_fixture "$fixture"
printf 'pub fn read() { let _ = std::env::var("FORBIDDEN"); }\n' \
  >"$fixture/crates/forward/src/lib.rs"
expect_failure "$fixture" 'production API-layer env read must live in crates/server/src/config.rs'

fixture=$TEMP/test-env
make_fixture "$fixture"
printf '#[cfg(test)]\nmod tests { #[test] fn reads() { let _ = std::env::var("TEST_ONLY"); } }\n' \
  >>"$fixture/crates/forward/src/lib.rs"
python3 "$CHECK" "$fixture" >/dev/null

fixture=$TEMP/consumer
make_fixture "$fixture"
mkdir -p "$fixture/apps/rogue/src"
printf '{"name":"@claude-api/rogue","private":true,"dependencies":{"@claude-api/engine-client":"workspace:*"}}\n' \
  >"$fixture/apps/rogue/package.json"
expect_failure "$fixture" 'undeclared bounded-context consumer of @claude-api/engine-client'

fixture=$TEMP/bypass
make_fixture "$fixture"
printf 'fetch(`${engineBaseUrl}/admin/account/x`);\n' >"$fixture/apps/api/src/direct.ts"
expect_failure "$fixture" 'direct Engine Control API fetch bypasses @claude-api/engine-client'

printf 'repository-invariants.test: passed\n'
