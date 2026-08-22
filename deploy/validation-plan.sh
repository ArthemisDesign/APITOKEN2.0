#!/usr/bin/env bash
set -euo pipefail

# This helper is deliberately pure: it reads one exact Git range and prints a strict validation
# envelope. The installed controller and the candidate controller both run it, then the watchdog
# takes the union. Candidate policy can therefore add validation but can never weaken the policy
# already trusted by the host.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [[ -r $SCRIPT_DIR/watchdog-lib.sh ]]; then
  # Repository and extracted-candidate layout.
  # shellcheck source=deploy/watchdog-lib.sh
  source "$SCRIPT_DIR/watchdog-lib.sh"
elif [[ -r $SCRIPT_DIR/../watchdog-lib.sh ]]; then
  # Installed controller layout.
  # shellcheck source=deploy/watchdog-lib.sh
  source "$SCRIPT_DIR/../watchdog-lib.sh"
else
  printf 'validation planner cannot find watchdog-lib.sh\n' >&2
  exit 1
fi

[[ $# -eq 7 ]] || wd_die \
  "usage: validation-plan.sh <repo> <target> <processed> <engine> <backend> <sales> <openkeys>"

REPO=$1
TARGET=$2
PROCESSED_BASE=$3
ENGINE_BASE=$4
BACKEND_BASE=$5
SALES_BASE=$6
OPENKEYS_BASE=$7

[[ -d $REPO/.git ]] || wd_die "validation-plan repository is missing: $REPO"
for sha in "$TARGET" "$PROCESSED_BASE" "$ENGINE_BASE" "$BACKEND_BASE" \
  "$SALES_BASE" "$OPENKEYS_BASE"; do
  wd_validate_sha "$sha"
  if ! git -c safe.directory="$REPO" -C "$REPO" cat-file -e "$sha^{commit}" 2>/dev/null; then
    # The privileged controller extracted this exact planner from TARGET immediately before
    # invoking it and validated every baseline before passing it here. If the isolated CI user
    # cannot read either a freshly fetched target or an older baseline loose object (for example
    # after a restrictive inherited umask), fail closed to every validation lane. This lets a
    # candidate repair the isolation boundary without allowing its policy to weaken the already
    # trusted host plan.
    fallback_policy=$(
      printf 'validation-plan-unreadable-target-v1\ntarget=%s\n' "$TARGET" | wd_sha256_stdin
    )
    printf 'validation_plan_format=1\n'
    printf 'validation_policy_sha256=%s\n' "$fallback_policy"
    printf 'typescript_required=1\n'
    printf 'typescript_full=1\n'
    printf 'typescript_base=%s\n' "$PROCESSED_BASE"
    printf 'rust_required=1\n'
    printf 'static_required=1\n'
    printf 'engine_artifacts_required=1\n'
    exit 0
  fi
done

TYPESCRIPT_REQUIRED=0
TYPESCRIPT_FULL=0
TYPESCRIPT_BASE=$PROCESSED_BASE
RUST_REQUIRED=0
STATIC_REQUIRED=0
ENGINE_ARTIFACTS_REQUIRED=0
infrastructure_validation_changed=0
unknown_validation_path=0

choose_older_typescript_base() {
  local candidate=$1
  if git -c safe.directory="$REPO" -C "$REPO" merge-base --is-ancestor \
    "$candidate" "$TYPESCRIPT_BASE"; then
    TYPESCRIPT_BASE=$candidate
  elif ! git -c safe.directory="$REPO" -C "$REPO" merge-base --is-ancestor \
    "$TYPESCRIPT_BASE" "$candidate"; then
    # Component baselines should share protected master history. Incomparable histories cannot
    # produce a trustworthy package closure, so validate the complete workspace.
    TYPESCRIPT_FULL=1
  fi
}

wd_range_has_class "$REPO" "$PROCESSED_BASE" "$TARGET" wd_path_is_typescript \
  && TYPESCRIPT_REQUIRED=1
if wd_range_changes_typescript_gate "$REPO" "$PROCESSED_BASE" "$TARGET"; then
  TYPESCRIPT_REQUIRED=1
  TYPESCRIPT_FULL=1
fi
wd_range_has_class "$REPO" "$PROCESSED_BASE" "$TARGET" wd_path_is_infrastructure \
  && infrastructure_validation_changed=1
wd_range_has_class "$REPO" "$PROCESSED_BASE" "$TARGET" wd_path_is_merge_workflow \
  && STATIC_REQUIRED=1
wd_range_has_unknown_validation_path "$REPO" "$PROCESSED_BASE" "$TARGET" \
  && unknown_validation_path=1
if wd_range_has_class "$REPO" "$PROCESSED_BASE" "$TARGET" \
  wd_path_requires_control_api_acceptance; then
  RUST_REQUIRED=1
  ENGINE_ARTIFACTS_REQUIRED=1
fi
if wd_range_has_class "$REPO" "$PROCESSED_BASE" "$TARGET" \
  wd_path_requires_router_engine_replay; then
  RUST_REQUIRED=1
  ENGINE_ARTIFACTS_REQUIRED=1
fi
if wd_range_has_class "$REPO" "$ENGINE_BASE" "$TARGET" wd_path_is_engine; then
  RUST_REQUIRED=1
  ENGINE_ARTIFACTS_REQUIRED=1
fi
if wd_range_has_class "$REPO" "$BACKEND_BASE" "$TARGET" wd_path_is_backend; then
  TYPESCRIPT_REQUIRED=1
  choose_older_typescript_base "$BACKEND_BASE"
fi
if wd_range_has_class "$REPO" "$SALES_BASE" "$TARGET" wd_path_is_sales; then
  TYPESCRIPT_REQUIRED=1
  choose_older_typescript_base "$SALES_BASE"
fi
if wd_range_has_class "$REPO" "$OPENKEYS_BASE" "$TARGET" wd_path_is_openkeys; then
  TYPESCRIPT_REQUIRED=1
  choose_older_typescript_base "$OPENKEYS_BASE"
fi

(( infrastructure_validation_changed == 0 )) || STATIC_REQUIRED=1
if (( unknown_validation_path == 1 )); then
  TYPESCRIPT_REQUIRED=1
  TYPESCRIPT_FULL=1
  RUST_REQUIRED=1
  STATIC_REQUIRED=1
  # An unknown path may be a newly introduced engine input. Build the production binaries as part
  # of the fail-closed plan so a newer controller can reuse this exact candidate after handoff.
  ENGINE_ARTIFACTS_REQUIRED=1
fi
if (( TYPESCRIPT_REQUIRED == 1 )) \
  && wd_range_requires_full_typescript_scope "$REPO" "$TYPESCRIPT_BASE" "$TARGET"; then
  TYPESCRIPT_FULL=1
fi

# Bind the marker to the exact candidate policy implementation, not merely to the resulting flags.
# The format stays stable while policy evolves; a future envelope format requires a staged upgrade.
policy_sha256=$(
  for path in \
    deploy/validation-plan.sh \
    deploy/watchdog-lib.sh \
    deploy/contour-config.sh \
    deploy/contour-config.py \
    deploy/contour-config.schema.json \
    deploy/contour-production.json \
    deploy/contour-stage.json \
    deploy/contour-config.test.sh \
    deploy/stage-unit-renderer.py \
    deploy/stage-unit-renderer.test.sh \
    deploy/stage-unit-whitelist.json \
    deploy/test-fixtures/contour-config/production-resolved.txt \
    deploy/test-fixtures/contour-config/stage-resolved.txt \
    deploy/test-fixtures/contour-config/stage-safe.json \
    deploy/test-fixtures/contour-config/shell-injection.json \
    deploy/typescript-scope.mjs \
    deploy/next-cache.sh \
    deploy/typescript-build-contexts.sh \
    deploy/typescript-test-groups.sh \
    deploy/commerce-release-bundle.sh \
    deploy/change-plan.sh \
    deploy/repository-invariants.py \
    deploy/docs-check.sh \
    deploy/docs-check.py \
    tests/control_api_engine_client_acceptance.sh \
    packages/engine-client/acceptance/control-api.mjs \
    tests/router_engine_replay.py \
    tests/router_engine_replay_mock.py \
    tests/router_engine_replay_semantics.test.py \
    tests/fixtures/router-engine-replay-v1.json \
    deploy/engine-commerce-compatibility.contract \
    deploy/release-tree-digest.mjs; do
    blob=$(git -c safe.directory="$REPO" -C "$REPO" rev-parse "$TARGET:$path" 2>/dev/null \
      || printf 'missing')
    printf '%s=%s\n' "$path" "$blob"
  done | wd_sha256_stdin
)

printf 'validation_plan_format=1\n'
printf 'validation_policy_sha256=%s\n' "$policy_sha256"
printf 'typescript_required=%s\n' "$TYPESCRIPT_REQUIRED"
printf 'typescript_full=%s\n' "$TYPESCRIPT_FULL"
printf 'typescript_base=%s\n' "$TYPESCRIPT_BASE"
printf 'rust_required=%s\n' "$RUST_REQUIRED"
printf 'static_required=%s\n' "$STATIC_REQUIRED"
printf 'engine_artifacts_required=%s\n' "$ENGINE_ARTIFACTS_REQUIRED"
