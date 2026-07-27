#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
WRAPPER=$ROOT/deploy/sccache-cargo.sh
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT

fail() { printf 'sccache-cargo.test: %s\n' "$*" >&2; exit 1; }
FIXTURE=$TEMP/repo
mkdir -p "$FIXTURE/deploy" "$TEMP/bin" "$TEMP/state"
cp "$WRAPPER" "$FIXTURE/deploy/sccache-cargo.sh"
git init --quiet "$FIXTURE"
git -C "$FIXTURE" config user.name sccache-test
git -C "$FIXTURE" config user.email sccache-test@example.invalid
git -C "$FIXTURE" add deploy/sccache-cargo.sh
git -C "$FIXTURE" commit --quiet -m fixture

cat >"$TEMP/bin/sccache" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
state=${SCCACHE_TEST_STATE:?}
case "${1:-}" in
  --version)
    printf 'sccache 0.15.0\n'
    ;;
  --start-server)
    [[ ${SCCACHE_TEST_FAIL:-0} != 1 ]] || exit 2
    [[ ! -f $state/ready ]] || exit 0
    if ! mkdir "$state/starting" 2>/dev/null; then
      printf 'sccache: error: Server startup failed: File exists\n' >&2
      exit 2
    fi
    printf 'start\n' >>"$state/starts"
    sleep 0.3
    : >"$state/ready"
    rmdir "$state/starting"
    ;;
  --show-stats)
    [[ ${SCCACHE_TEST_FAIL:-0} != 1 && -f $state/ready ]]
    ;;
  *)
    printf 'unexpected sccache arguments: %s\n' "$*" >&2
    exit 3
    ;;
esac
STUB
cat >"$TEMP/bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ ${1:-} == --version ]]; then
  printf 'cargo 1.91.0 (fixture)\n'
  exit 0
fi
[[ ${EXPECT_UNCACHED:-0} == 1 || -f ${SCCACHE_TEST_STATE:?}/ready ]] \
  || { printf 'cargo started before cache server readiness\n' >&2; exit 4; }
printf 'cargo\n' >>"${SCCACHE_TEST_STATE:?}/cargo-runs"
STUB
chmod +x "$TEMP/bin/sccache" "$TEMP/bin/cargo"

run_wrapper() {
  PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
    SCCACHE_TEST_STATE="$TEMP/state" \
    bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
}

run_wrapper &
first_pid=$!
run_wrapper &
second_pid=$!
first_rc=0
second_rc=0
wait "$first_pid" || first_rc=$?
wait "$second_pid" || second_rc=$?
[[ $first_rc == 0 && $second_rc == 0 ]] \
  || fail "concurrent cold-cache wrappers failed: first=$first_rc second=$second_rc"
[[ $(wc -l <"$TEMP/state/starts" | tr -d ' ') == 1 ]] \
  || fail 'concurrent wrappers started more than one cache server'
[[ $(wc -l <"$TEMP/state/cargo-runs" | tr -d ' ') == 2 ]] \
  || fail 'both Cargo commands did not run after serialized startup'

mkdir -p "$TEMP/failing-state"
PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
  SCCACHE_TEST_STATE="$TEMP/failing-state" SCCACHE_TEST_FAIL=1 EXPECT_UNCACHED=1 \
  bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
[[ -f $TEMP/failing-state/cargo-runs ]] \
  || fail 'a cache-server failure did not fall back to uncached Cargo'
if find "$FIXTURE/.git/codex-tools" -maxdepth 1 -type d -name 'sccache-server-*.lock' \
  | grep -q .; then
  fail 'cache-server startup lock survived its owner'
fi

printf 'sccache-cargo.test: ok\n'
