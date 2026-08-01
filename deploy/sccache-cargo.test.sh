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
    printf '%s\n' "${SCCACHE_DIR:-}" >"$state/cache-dir"
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
printf '%s|%s|%s\n' \
  "${CARGO_BUILD_BUILD_DIR:-}" \
  "${CARGO_TARGET_DIR:-}" \
  "${CARGO_INCREMENTAL:-}" \
  >>"${SCCACHE_TEST_STATE:?}/cargo-env"
STUB
chmod +x "$TEMP/bin/sccache" "$TEMP/bin/cargo"

run_wrapper() {
  PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
    SCCACHE_TEST_STATE="$TEMP/state" \
    CARGO_BUILD_BUILD_DIR="$TEMP/unsafe-shared-build" \
    CARGO_TARGET_DIR="$FIXTURE/target-local" \
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
fixture_common_dir=$(git -C "$FIXTURE" rev-parse --path-format=absolute --git-common-dir)
[[ -z $(cut -d '|' -f 1 "$TEMP/state/cargo-env" | sort -u) ]] \
  || fail 'the cached path retained an inherited clone-wide Cargo build-dir'
[[ $(cut -d '|' -f 2 "$TEMP/state/cargo-env" | sort -u) == "$FIXTURE/target-local" ]] \
  || fail 'the wrapper changed the caller-owned Cargo target directory'
[[ $(cut -d '|' -f 3 "$TEMP/state/cargo-env" | sort -u) == 0 ]] \
  || fail 'the cached path did not disable incremental compilation for sccache'
[[ $(cat "$TEMP/state/cache-dir") == "$fixture_common_dir/codex-tools/sccache-cache" ]] \
  || fail 'compiler objects are not stored in the bounded clone-wide cache'
[[ ! -e $fixture_common_dir/codex-tools/cargo-build ]] \
  || fail 'the wrapper recreated the retired clone-wide Cargo build directory'

mkdir -p "$TEMP/failing-state"
PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
  SCCACHE_TEST_STATE="$TEMP/failing-state" SCCACHE_TEST_FAIL=1 EXPECT_UNCACHED=1 \
  CARGO_BUILD_BUILD_DIR="$TEMP/unsafe-shared-build" \
  CARGO_TARGET_DIR="$FIXTURE/fallback-target" \
  bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
[[ -f $TEMP/failing-state/cargo-runs ]] \
  || fail 'a cache-server failure did not fall back to uncached Cargo'
[[ -z $(cut -d '|' -f 1 "$TEMP/failing-state/cargo-env") ]] \
  || fail 'a cache-server failure retained a potentially shared Cargo build-dir'
[[ $(cut -d '|' -f 2 "$TEMP/failing-state/cargo-env") == "$FIXTURE/fallback-target" ]] \
  || fail 'a cache-server failure discarded the caller-owned Cargo target directory'
if find "$FIXTURE/.git/codex-tools" -maxdepth 1 -type d -name 'sccache-server-*.lock' \
  | grep -q .; then
  fail 'cache-server startup lock survived its owner'
fi

mkdir -p "$TEMP/disabled-state"
PATH="$TEMP/bin:$PATH" SCCACHE_TEST_STATE="$TEMP/disabled-state" \
  SCCACHE_DISABLE=1 EXPECT_UNCACHED=1 CARGO_BUILD_BUILD_DIR="$TEMP/unsafe-shared-build" \
  CARGO_TARGET_DIR="$FIXTURE/disabled-target" \
  bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
[[ -f $TEMP/disabled-state/cargo-runs ]] \
  || fail 'explicit cache disablement did not run Cargo'
[[ -z $(cut -d '|' -f 1 "$TEMP/disabled-state/cargo-env") ]] \
  || fail 'explicit cache disablement retained a potentially shared Cargo build-dir'
[[ $(cut -d '|' -f 2 "$TEMP/disabled-state/cargo-env") == "$FIXTURE/disabled-target" ]] \
  || fail 'explicit cache disablement discarded the caller-owned Cargo target directory'

printf 'sccache-cargo.test: ok\n'
