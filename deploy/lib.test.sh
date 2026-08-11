#!/usr/bin/env bash
set -eEuo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

# The trusted controller performs final packaging as the unprivileged deploy account after these
# suites run as the isolated CI owner. Keep the tracked checkout readable even if an older
# controller cloned it under a restrictive transcript umask. Generated dependency trees are not
# part of this handoff and may be changing concurrently, so leave them alone.
find "$ROOT" \
  \( -path "$ROOT/.git" -o -name node_modules \) -prune -o \
  -type d -exec chmod go+rx {} + -o -type f -exec chmod go+r {} +

# shellcheck source=deploy/lib.sh
source "$ROOT/deploy/lib.sh"

TEMP=$(realpath -- "$(mktemp -d)")
trap 'rm -rf -- "$TEMP"' EXIT

# `mapfile` is a bash 4 builtin, and macOS still ships bash 3.2 — contributors run this suite
# locally through the merge gate, so read the file portably instead.
read_lines_into() {
  local array_name=$1 line
  eval "$array_name=()"
  while IFS= read -r line || [[ -n $line ]]; do
    eval "$array_name+=(\"\$line\")"
  done <"$2"
}

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
read_lines_into restore_order "$TEMP/restore-order"
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
    [[ $(realpath -- "$abort_root/current") == "$abort_root/$OLD_A" ]] \
      || fail "activation recovery callback ran before current was restored"
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
read_lines_into abort_restore_order "$TEMP/abort-restore-order"
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

# If engine current itself cannot be restored, recovery must not move authbot back to the original
# release and split it from the still-selected engine target. The callback failure must remain visible.
current_failure_root="$TEMP/current-failure"
make_releases "$current_failure_root"
ln -s -- "$current_failure_root/$OLD_A" "$current_failure_root/current"
set +e
(
  set -eEuo pipefail
  source "$ROOT/deploy/lib.sh"
  reconcile_authbot_release() {
    printf 'reconciled\n' >>"$TEMP/current-failure-authbot"
  }
  guarded_authbot_recovery() {
    reconcile_authbot_after_engine_restore "$current_failure_root" "$current_failure_root/$OLD_A"
  }
  capture_release_link "$current_failure_root" "$current_failure_root/current"
  begin_activation guarded_authbot_recovery
  set_journaled_release_link "$current_failure_root/$NEW_A" "$current_failure_root/current"
  : >"${current_failure_root}/current.tmp.$$"
  false
) >"$TEMP/current-failure.out" 2>&1
current_failure_status=$?
set -e
(( current_failure_status != 0 )) || fail "failed engine current restoration returned success"
assert_link "$current_failure_root/current" "$current_failure_root/$NEW_A"
[[ ! -e "$TEMP/current-failure-authbot" ]] \
  || fail "authbot was reconciled after engine current restoration failed"
grep -Fq 'leaving authbot untouched' "$TEMP/current-failure.out" \
  || fail "failed engine current restoration did not explain why authbot was left aligned"
grep -Fq 'automatic recovery was incomplete' "$TEMP/current-failure.out" \
  || fail "failed engine current restoration did not report incomplete recovery"

# The deploy stops a slot only when it owes nothing, and this parser is what it reads. Two failure
# modes matter and both must degrade rather than block: an old slot whose binary predates the field,
# and a slot that answers something unparseable. Blocking on either would wedge every deploy.
assert_active_requests() {
  local body=$1 expected=$2 actual
  actual=$(printf '%s' "$body" | parse_active_requests)
  [[ $actual == "$expected" ]] \
    || fail "active-request parse of '$body' gave '$actual', expected '$expected'"
}

assert_active_requests '{"ready":true,"active_requests":0}' 0
assert_active_requests '{"ready":false,"reason":"draining","active_requests":7}' 7
assert_active_requests '{"active_requests": 42, "ready": false}' 42
# Pre-field binary: empty means "unknown", which the caller treats as drained.
assert_active_requests '{"ready":false,"reason":"draining"}' ''
assert_active_requests 'not json at all' ''
assert_active_requests '' ''
# A negative or non-numeric value is not a count and must not be mistaken for one.
assert_active_requests '{"active_requests":-1}' ''

# Engine/commerce rollback compatibility is a closed release contract. Exercise the complete
# explicit matrix, parser fail-closed behavior, markerless production-history bridge, and guards
# against the generations that are actually running rather than whichever symlink was selected.
compat_root="$TEMP/compatibility"
mkdir -p "$compat_root"
make_compat_release() {
  local release=$1 requirements=$2 provisions=$3
  mkdir -p "$release"
  printf 'format=1\ncommerce_requires=%s\nengine_provides=%s\n' \
    "$requirements" "$provisions" >"$release/$ENGINE_COMMERCE_COMPATIBILITY_MARKER"
}

scalar_commerce="$compat_root/1111111111111111111111111111111111111111"
policy_commerce="$compat_root/2222222222222222222222222222222222222222"
dual_commerce="$compat_root/3333333333333333333333333333333333333333"
scalar_engine="$compat_root/4444444444444444444444444444444444444444"
policy_engine="$compat_root/5555555555555555555555555555555555555555"
bridge_engine="$compat_root/6666666666666666666666666666666666666666"
make_compat_release "$scalar_commerce" scalar-pricing-v1 scalar-pricing-v1
make_compat_release "$policy_commerce" policy-pricing-v1 scalar-pricing-v1
make_compat_release "$dual_commerce" \
  policy-pricing-v1,scalar-pricing-v1 scalar-pricing-v1
make_compat_release "$scalar_engine" scalar-pricing-v1 scalar-pricing-v1
make_compat_release "$policy_engine" scalar-pricing-v1 policy-pricing-v1
make_compat_release "$bridge_engine" scalar-pricing-v1 \
  policy-pricing-v1,scalar-pricing-v1

require_engine_commerce_release_pair_compatible "$scalar_commerce" "$scalar_engine"
require_engine_commerce_release_pair_compatible "$policy_commerce" "$bridge_engine"
require_engine_commerce_release_pair_compatible "$dual_commerce" "$bridge_engine"
if (require_engine_commerce_release_pair_compatible \
    "$policy_commerce" "$scalar_engine") >"$TEMP/policy-scalar.out" 2>&1; then
  fail "policy commerce was accepted with a scalar-only engine"
fi
grep -Fq 'requires policy-pricing-v1' "$TEMP/policy-scalar.out" \
  || fail "policy/scalar incompatibility did not name the missing capability"
if (require_engine_commerce_release_pair_compatible \
    "$scalar_commerce" "$policy_engine") >"$TEMP/scalar-policy.out" 2>&1; then
  fail "scalar commerce was accepted with a policy-only engine"
fi

for malformed in duplicate unknown invalid symlink; do
  release="$compat_root/malformed-$malformed"
  mkdir -p "$release"
  marker="$release/$ENGINE_COMMERCE_COMPATIBILITY_MARKER"
  case "$malformed" in
    duplicate)
      printf 'format=1\ncommerce_requires=scalar-pricing-v1\ncommerce_requires=scalar-pricing-v1\nengine_provides=scalar-pricing-v1\n' >"$marker"
      ;;
    unknown)
      printf 'format=1\ncommerce_requires=scalar-pricing-v1\nengine_provides=scalar-pricing-v1\nfuture=unsafe\n' >"$marker"
      ;;
    invalid)
      printf 'format=1\ncommerce_requires=Scalar Pricing\nengine_provides=scalar-pricing-v1\n' >"$marker"
      ;;
    symlink)
      printf 'format=1\ncommerce_requires=scalar-pricing-v1\nengine_provides=scalar-pricing-v1\n' \
        >"$release/outside"
      ln -s outside "$marker"
      ;;
  esac
  if (validate_engine_commerce_compatibility_contract "$marker") \
      >"$TEMP/malformed-$malformed.out" 2>&1; then
    fail "compatibility parser accepted $malformed input"
  fi
done

RELEASE_COMPATIBILITY_REPO=$ROOT
historical_release="$compat_root/$LEGACY_SCALAR_ENGINE_FLOOR_SHA"
mkdir -p "$historical_release"
[[ $(release_commerce_requirements "$historical_release") == policy-pricing-v1 ]] \
  || fail "first scalar engine generation was not classified as policy-only commerce"
[[ $(release_engine_provisions "$historical_release") == policy-pricing-v1,scalar-pricing-v1 ]] \
  || fail "first scalar engine generation was not classified as a bridge engine"
historical_release="$compat_root/$LEGACY_SCALAR_COMMERCE_FLOOR_SHA"
mkdir -p "$historical_release"
[[ $(release_commerce_requirements "$historical_release") == policy-pricing-v1,scalar-pricing-v1 ]] \
  || fail "dual commerce generation lost one of its requirements"
historical_release="$compat_root/$LEGACY_SCALAR_ONLY_COMMERCE_SHA"
mkdir -p "$historical_release"
[[ $(release_commerce_requirements "$historical_release") == scalar-pricing-v1 ]] \
  || fail "scalar-only commerce boundary was classified incorrectly"
historical_release="$compat_root/$LEGACY_SCALAR_ONLY_ENGINE_SHA"
mkdir -p "$historical_release"
[[ $(release_engine_provisions "$historical_release") == scalar-pricing-v1 ]] \
  || fail "scalar-only engine boundary was classified incorrectly"
pre_floor_sha=$(git -C "$ROOT" rev-parse "$LEGACY_SCALAR_ENGINE_FLOOR_SHA^")
historical_release="$compat_root/$pre_floor_sha"
mkdir -p "$historical_release"
if (release_engine_provisions "$historical_release") >"$TEMP/pre-floor.out" 2>&1; then
  fail "markerless release before the compatibility floor was classified"
fi
grep -Fq 'predates the classified compatibility floor' "$TEMP/pre-floor.out" \
  || fail "pre-floor markerless refusal was not explicit"

(
  list_active_engine_releases() { printf '%s\n' "$scalar_engine"; }
  require_commerce_release_compatible_with_active_engines \
    "$scalar_commerce" "$compat_root"
) >"$TEMP/active-engine-green.out"
if (
  list_active_engine_releases() { printf '%s\n' "$scalar_engine"; }
  require_commerce_release_compatible_with_active_engines \
    "$policy_commerce" "$compat_root"
) >"$TEMP/active-engine-red.out" 2>&1; then
  fail "commerce active-engine guard accepted an incompatible generation"
fi
(
  list_active_commerce_releases() { printf '%s\n' "$scalar_commerce"; }
  require_engine_release_compatible_with_active_commerce \
    "$bridge_engine" "$compat_root"
) >"$TEMP/active-commerce-green.out"
if (
  list_active_commerce_releases() { printf '%s\n' "$scalar_commerce"; }
  require_engine_release_compatible_with_active_commerce \
    "$policy_engine" "$compat_root"
) >"$TEMP/active-commerce-red.out" 2>&1; then
  fail "engine active-commerce guard accepted an incompatible generation"
fi

assert_guard_before_mutation() {
  local path=$1 guard_pattern=$2 mutation_pattern=$3 guard_line mutation_line
  guard_line=$(grep -n "$guard_pattern" "$path" | tail -n1 | cut -d: -f1)
  mutation_line=$(grep -n "$mutation_pattern" "$path" | tail -n1 | cut -d: -f1)
  [[ -n "$guard_line" && -n "$mutation_line" && $guard_line -lt $mutation_line ]] \
    || fail "compatibility guard in $path does not precede $mutation_pattern"
}
assert_guard_before_mutation "$ROOT/deploy/api-bluegreen.sh" \
  '^require_commerce_release_compatible_with_active_engines' '^begin_cutover$'
assert_guard_before_mutation "$ROOT/deploy/engine-bluegreen.sh" \
  '^require_engine_release_compatible_with_active_commerce' '^begin_cutover$'
assert_guard_before_mutation "$ROOT/deploy/rollback.sh" \
  '^preflight_compatibility$' '^begin_activation recovery_restart_selected_services$'
assert_guard_before_mutation "$ROOT/deploy/deploy.sh" \
  '^preflight_candidate_compatibility$' \
  '^[[:space:]]*begin_activation recovery_restart_selected_services$'

printf 'deploy/lib.sh engine-commerce compatibility tests passed\n'

printf 'deploy/lib.sh drain-gate parser tests passed\n'

printf 'deploy/lib.sh activation journal tests passed\n'
