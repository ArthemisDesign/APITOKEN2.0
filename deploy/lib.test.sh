#!/usr/bin/env bash
set -eEuo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=deploy/lib.sh
source "$ROOT/deploy/lib.sh"

TEMP=$(realpath -- "$(mktemp -d)")
trap 'rm -rf -- "$TEMP"' EXIT

# Production is GNU/Linux and deliberately uses `mv -T` so a release link can never be treated as
# a directory. macOS/BSD `mv` has the same safe replacement behavior for this symlink-only fixture
# but lacks `-T`; adapt only the test process so contributors can run the suite locally.
if ! mv --help 2>&1 | grep -q -- '-T'; then
  mv() {
    if [[ ${1:-} == "-Tf" ]]; then
      shift
      [[ ${1:-} != "--" ]] || shift
      local source=$1 destination=$2
      # BSD mv follows a destination symlink to a directory. Remove only that fixture symlink first
      # to emulate GNU `-T`'s replace-the-link behavior.
      [[ ! -L "$destination" ]] || command rm -f "$destination"
      command mv -f "$source" "$destination"
    else
      command mv "$@"
    fi
  }
fi

OLD_A=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
OLD_B=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
NEW_A=cccccccccccccccccccccccccccccccccccccccc
NEW_B=dddddddddddddddddddddddddddddddddddddddd
OTHER=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee

fail() {
  printf '[deploy-lib-test] ERROR: %s\n' "$*" >&2
  exit 1
}

reset_activation_journal() {
  ACTIVATION_LINKS=()
  ACTIVATION_ROOTS=()
  ACTIVATION_ORIGINAL_STATES=()
  ACTIVATION_ORIGINAL_TARGETS=()
  ACTIVATION_CHANGED=()
  ACTIVATION_ACTIVE=0
  ACTIVATION_COMMITTED=0
  ACTIVATION_RECOVERY_CALLBACK=
}

make_releases() {
  local root=$1
  mkdir -p -- "$root/$OLD_A" "$root/$OLD_B" "$root/$NEW_A" "$root/$NEW_B" "$root/$OTHER"
}

assert_link() {
  local link=$1 expected=$2
  [[ -L "$link" ]] || fail "$link is not a symlink"
  [[ $(realpath -- "$link") == "$expected" ]] \
    || fail "$link does not point to the expected release $expected"
}

# Direct journal restoration must unwind every changed link in reverse capture order while leaving
# a captured-but-unchanged link alone.
restore_root="$TEMP/restore"
make_releases "$restore_root"
ln -s -- "$restore_root/$OLD_A" "$restore_root/current"
ln -s -- "$restore_root/$OLD_B" "$restore_root/previous"
capture_release_link "$restore_root" "$restore_root/current"
capture_release_link "$restore_root" "$restore_root/previous"
capture_release_link "$restore_root" "$restore_root/current"
[[ ${#ACTIVATION_LINKS[@]} == 2 ]] || fail "capturing the same release link was not idempotent"
set_journaled_release_link "$restore_root/$NEW_A" "$restore_root/current"
set_journaled_release_link "$restore_root/$NEW_B" "$restore_root/previous"

# Wrap the production restore primitive only to observe call order; the real implementation still
# performs each mutation.
eval "$(declare -f restore_target_link | sed '1s/restore_target_link/original_restore_target_link/')"
restore_target_link() {
  printf '%s\n' "$2" >>"$TEMP/restore-order"
  original_restore_target_link "$@"
}
restore_activation_links || fail "restoring changed release links failed"
assert_link "$restore_root/current" "$restore_root/$OLD_A"
assert_link "$restore_root/previous" "$restore_root/$OLD_B"
mapfile -t restore_order <"$TEMP/restore-order"
[[ ${#restore_order[@]} == 2 ]] || fail "restore did not visit both changed links"
[[ ${restore_order[0]} == "$restore_root/previous" && ${restore_order[1]} == "$restore_root/current" ]] \
  || fail "release links were not restored in reverse capture order"

# Restore the production function before the remaining cases.
eval "$(declare -f original_restore_target_link | sed '1s/original_restore_target_link/restore_target_link/')"

# A link that did not exist before activation must disappear again.
reset_activation_journal
absent_root="$TEMP/absent"
make_releases "$absent_root"
capture_release_link "$absent_root" "$absent_root/previous"
set_journaled_release_link "$absent_root/$NEW_A" "$absent_root/previous"
assert_link "$absent_root/previous" "$absent_root/$NEW_A"
restore_activation_links || fail "restoring an originally absent link failed"
[[ ! -e "$absent_root/previous" && ! -L "$absent_root/previous" ]] \
  || fail "an originally absent release link survived restoration"

# The preflight snapshot is a TOCTOU guard: an operator or concurrent controller changing the link
# after capture must make activation fail closed without overwriting that newer target.
toctou_root="$TEMP/toctou"
make_releases "$toctou_root"
ln -s -- "$toctou_root/$OLD_A" "$toctou_root/current"
if (
  source "$ROOT/deploy/lib.sh"
  capture_release_link "$toctou_root" "$toctou_root/current"
  atomic_symlink "$toctou_root/$OTHER" "$toctou_root/current"
  set_journaled_release_link "$toctou_root/$NEW_A" "$toctou_root/current"
) >"$TEMP/toctou.out" 2>&1; then
  fail "activation accepted a release link changed after preflight"
fi
grep -Fq 'changed after preflight; refusing activation' "$TEMP/toctou.out" \
  || fail "TOCTOU refusal did not report its reason"
assert_link "$toctou_root/current" "$toctou_root/$OTHER"

# Only current/previous may be journaled and only direct 40-character SHA releases may be targets.
invalid_root="$TEMP/invalid"
make_releases "$invalid_root"
ln -s -- "$invalid_root/$OLD_A" "$invalid_root/current"
if (
  source "$ROOT/deploy/lib.sh"
  capture_release_link "$invalid_root" "$invalid_root/not-current"
) >"$TEMP/unexpected-link.out" 2>&1; then
  fail "an unexpected release link was accepted into the activation journal"
fi
grep -Fq 'refusing to journal unexpected release link' "$TEMP/unexpected-link.out" \
  || fail "unexpected-link refusal did not report its reason"

for invalid_target in "$TEMP/$NEW_A" "$invalid_root/not-a-sha" "$invalid_root/$NEW_A/nested"; do
  if (
    source "$ROOT/deploy/lib.sh"
    capture_release_link "$invalid_root" "$invalid_root/current"
    set_journaled_release_link "$invalid_target" "$invalid_root/current"
  ) >"$TEMP/invalid-target.out" 2>&1; then
    fail "invalid release target was accepted: $invalid_target"
  fi
  grep -Fq 'refusing to point' "$TEMP/invalid-target.out" \
    || fail "invalid-target refusal did not report its reason: $invalid_target"
  assert_link "$invalid_root/current" "$invalid_root/$OLD_A"
done

# Exercise the real ERR/EXIT trap state machine. A failure after two mutations must restore both
# links, invoke recovery exactly once, and preserve the original non-zero status.
abort_root="$TEMP/abort"
make_releases "$abort_root"
ln -s -- "$abort_root/$OLD_A" "$abort_root/current"
set +e
(
  set -eEuo pipefail
  source "$ROOT/deploy/lib.sh"
  recovery_callback() {
    printf 'recovered\n' >>"$TEMP/recovery-calls"
  }
  eval "$(declare -f restore_target_link | sed '1s/restore_target_link/original_restore_target_link/')"
  restore_target_link() {
    printf '%s\n' "$2" >>"$TEMP/abort-restore-order"
    original_restore_target_link "$@"
  }
  restore_absent_link() {
    printf '%s\n' "$1" >>"$TEMP/abort-restore-order"
    if [[ -L "$1" ]]; then
      rm -f -- "$1"
    elif [[ -e "$1" ]]; then
      return 1
    fi
  }

  capture_release_link "$abort_root" "$abort_root/current"
  capture_release_link "$abort_root" "$abort_root/previous"
  begin_activation recovery_callback
  set_journaled_release_link "$abort_root/$NEW_A" "$abort_root/current"
  set_journaled_release_link "$abort_root/$NEW_B" "$abort_root/previous"
  false
) >"$TEMP/abort.out" 2>&1
abort_status=$?
set -e
(( abort_status != 0 )) || fail "a failed activation lost its non-zero status"
assert_link "$abort_root/current" "$abort_root/$OLD_A"
[[ ! -e "$abort_root/previous" && ! -L "$abort_root/previous" ]] \
  || fail "activation abort did not restore an originally absent link"
[[ $(grep -Fc 'recovered' "$TEMP/recovery-calls") == 1 ]] \
  || fail "activation recovery callback did not run exactly once"
mapfile -t abort_restore_order <"$TEMP/abort-restore-order"
[[ ${abort_restore_order[0]} == "$abort_root/previous" \
    && ${abort_restore_order[1]} == "$abort_root/current" ]] \
  || fail "activation abort did not unwind links in reverse order"
grep -Fq 'activation aborted by ERR' "$TEMP/abort.out" \
  || fail "activation abort did not identify the triggering trap"

# Once committed, EXIT must not undo the promoted link or call recovery.
commit_root="$TEMP/commit"
make_releases "$commit_root"
ln -s -- "$commit_root/$OLD_A" "$commit_root/current"
(
  source "$ROOT/deploy/lib.sh"
  should_not_recover() {
    printf 'called\n' >>"$TEMP/committed-recovery"
  }
  capture_release_link "$commit_root" "$commit_root/current"
  begin_activation should_not_recover
  set_journaled_release_link "$commit_root/$NEW_A" "$commit_root/current"
  commit_activation
)
assert_link "$commit_root/current" "$commit_root/$NEW_A"
[[ ! -e "$TEMP/committed-recovery" ]] || fail "committed activation invoked recovery"

# A poisoned temporary path must make that restoration fail, but failure accounting must continue
# restoring the other links and still call the recovery callback.
partial_root="$TEMP/partial"
make_releases "$partial_root"
ln -s -- "$partial_root/$OLD_A" "$partial_root/current"
set +e
(
  set -eEuo pipefail
  source "$ROOT/deploy/lib.sh"
  partial_recovery() {
    printf 'recovered\n' >>"$TEMP/partial-recovery"
  }
  capture_release_link "$partial_root" "$partial_root/previous"
  capture_release_link "$partial_root" "$partial_root/current"
  begin_activation partial_recovery
  set_journaled_release_link "$partial_root/$NEW_B" "$partial_root/previous"
  set_journaled_release_link "$partial_root/$NEW_A" "$partial_root/current"
  : >"${partial_root}/previous.tmp.$$"
  false
) >"$TEMP/partial.out" 2>&1
partial_status=$?
set -e
(( partial_status != 0 )) || fail "activation with an incomplete restoration returned success"
assert_link "$partial_root/current" "$partial_root/$OLD_A"
assert_link "$partial_root/previous" "$partial_root/$NEW_B"
[[ $(grep -Fc 'recovered' "$TEMP/partial-recovery") == 1 ]] \
  || fail "partial restoration skipped or repeated the recovery callback"
grep -Fq 'automatic recovery was incomplete' "$TEMP/partial.out" \
  || fail "partial restoration did not surface the need for operator intervention"

printf 'deploy/lib.sh activation journal tests passed\n'
