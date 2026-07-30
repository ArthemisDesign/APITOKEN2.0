#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
WRAPPER=$ROOT/deploy/sccache-cargo.sh
REAL_CARGO=$(command -v cargo)
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT

fail() { printf 'sccache-cargo.test: %s\n' "$*" >&2; exit 1; }
FIXTURE=$TEMP/repo
LINKED_FIXTURE=$TEMP/repo-linked
mkdir -p "$FIXTURE/deploy" "$FIXTURE/src" "$TEMP/bin" "$TEMP/state"
cp "$WRAPPER" "$FIXTURE/deploy/sccache-cargo.sh"
cat >"$FIXTURE/Cargo.toml" <<'TOML'
[package]
name = "sccache-worktree-fixture"
version = "0.0.0"
edition = "2021"
TOML
cat >"$FIXTURE/src/lib.rs" <<'RS'
pub fn fixture() {}
RS
git init --quiet "$FIXTURE"
git -C "$FIXTURE" config user.name sccache-test
git -C "$FIXTURE" config user.email sccache-test@example.invalid
git -C "$FIXTURE" add deploy/sccache-cargo.sh Cargo.toml src/lib.rs
git -C "$FIXTURE" commit --quiet -m fixture
git -C "$FIXTURE" worktree add --quiet --detach "$LINKED_FIXTURE" HEAD

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
printf '%s\n' "${CARGO_BUILD_BUILD_DIR:-}" >>"${SCCACHE_TEST_STATE:?}/build-dirs"
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
fixture_common_dir=$(git -C "$FIXTURE" rev-parse --path-format=absolute --git-common-dir)
expected_build_dir="$fixture_common_dir/codex-tools/cargo-build/{workspace-path-hash}"
[[ $(sort -u "$TEMP/state/build-dirs") == "$expected_build_dir" ]] \
  || fail 'Cargo build-dir is not isolated by workspace path'

metadata_build_dir() {
  local repo=$1 output
  output=$(
    cd "$repo"
    PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
      SCCACHE_TEST_STATE="$TEMP/state" \
      bash "$repo/deploy/sccache-cargo.sh" "$REAL_CARGO" metadata --format-version 1 --no-deps
  )
  printf '%s\n' "$output" \
    | sed -n 's/.*"build_directory":"\([^"]*\)".*/\1/p'
}

fixture_build_dir=$(metadata_build_dir "$FIXTURE")
fixture_build_dir_again=$(metadata_build_dir "$FIXTURE")
linked_build_dir=$(metadata_build_dir "$LINKED_FIXTURE")
[[ -n $fixture_build_dir && $fixture_build_dir == "$fixture_build_dir_again" ]] \
  || fail 'one workspace path did not resolve to one stable Cargo build-dir'
[[ $fixture_build_dir == "$fixture_common_dir/codex-tools/cargo-build/"* ]] \
  || fail 'primary worktree build-dir escaped the clone-wide cache root'
[[ $linked_build_dir == "$fixture_common_dir/codex-tools/cargo-build/"* ]] \
  || fail 'linked worktree build-dir escaped the clone-wide cache root'
[[ $fixture_build_dir != "$linked_build_dir" ]] \
  || fail 'linked worktrees resolved to the same Cargo build-dir'

override_root="$TEMP/explicit-build-root"
override_build_dir=$(CARGO_BUILD_BUILD_DIR="$override_root" metadata_build_dir "$FIXTURE")
[[ $override_build_dir == "$override_root/"* && $override_build_dir != "$override_root" ]] \
  || fail 'an explicit Cargo build root bypassed workspace isolation'

mkdir -p "$TEMP/failing-state"
PATH="$TEMP/bin:$PATH" SCCACHE_BIN="$TEMP/bin/sccache" \
  SCCACHE_TEST_STATE="$TEMP/failing-state" SCCACHE_TEST_FAIL=1 EXPECT_UNCACHED=1 \
  CARGO_BUILD_BUILD_DIR="$TEMP/unsafe-shared-build" \
  bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
[[ -f $TEMP/failing-state/cargo-runs ]] \
  || fail 'a cache-server failure did not fall back to uncached Cargo'
[[ -z $(head -n 1 "$TEMP/failing-state/build-dirs") ]] \
  || fail 'a cache-server failure retained a potentially shared Cargo build-dir'
if find "$FIXTURE/.git/codex-tools" -maxdepth 1 -type d -name 'sccache-server-*.lock' \
  | grep -q .; then
  fail 'cache-server startup lock survived its owner'
fi

mkdir -p "$TEMP/disabled-state"
PATH="$TEMP/bin:$PATH" SCCACHE_TEST_STATE="$TEMP/disabled-state" \
  SCCACHE_DISABLE=1 EXPECT_UNCACHED=1 CARGO_BUILD_BUILD_DIR="$TEMP/unsafe-shared-build" \
  bash "$FIXTURE/deploy/sccache-cargo.sh" "$TEMP/bin/cargo" test
[[ -f $TEMP/disabled-state/cargo-runs ]] \
  || fail 'explicit cache disablement did not run Cargo'
[[ -z $(head -n 1 "$TEMP/disabled-state/build-dirs") ]] \
  || fail 'explicit cache disablement retained a potentially shared Cargo build-dir'

printf 'sccache-cargo.test: ok\n'
