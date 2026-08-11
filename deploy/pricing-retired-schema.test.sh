#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
RUNBOOK="$ROOT/docs/ops/PRICING_RETIREMENT.md"

die() {
  printf 'pricing retired-schema contract failed: %s\n' "$*" >&2
  exit 1
}

engine_tables=(
  account_funding_head_v2
  account_policy_bindings
  funding_ledger_allocations_v2
  funding_reservation_allocations_v2
  ledger_funding_allocations
  pricing_catalog_heads
  pricing_release_assignment_extensions_v2
  pricing_release_assignments
  pricing_release_head_v2
  pricing_release_recovery_links
  pricing_request_funding_allocations_v2
  pricing_shadow_admission_evaluations
  provider_switch_entries
  provider_switch_head
  reservation_funding_allocations
  funding_buckets
  funding_lots_v2
  funding_reservation_snapshots_v2
  pricing_admission_snapshots
  pricing_release_activations_v2
  pricing_request_snapshots_v2
  account_funding_generations_v2
  account_policy_rules
  pricing_catalog_entries
  pricing_release_policy_rules
  pricing_stage8_evidence_v2
  account_policy_versions
  pricing_release_policy_versions
  pricing_release_versions
  pricing_catalog_versions
  provider_switch_versions
)

commerce_tables=(
  account_policy_reconciliations
  account_policy_rules
  business_invite_policy_bindings
  business_invite_policy_snapshots_v2
  engine_catalog_jobs
  engine_policy_jobs
  engine_switch_jobs
  pricing_funding_normalizations_v2
  pricing_months
  pricing_policy_heads
  pricing_policy_rules
  pricing_policy_rules_v2
  pricing_release_activation_receipts_v2
  pricing_release_assignments_v2
  pricing_release_control_jobs_v2
  pricing_release_orchestrations_v2
  pricing_shadow_policy_jobs_v2
  pricing_stage5_blockers_v2
  pricing_stage5_prepare_acks_v2
  pricing_stage8_capture_artifacts_v2
  pricing_usage_funding_allocations
  product_catalog_heads
  provider_capability_aliases
  provider_capability_head
  provider_switch_entries
  provider_switch_head
  service_account_inventory_v2
  account_policy_bindings
  pricing_policy_documents_v2
  pricing_shadow_rollouts_v2
  pricing_stage8_capture_jobs_v2
  pricing_stage8_evidence_v2
  pricing_usage_attributions
  product_catalog_entries
  account_policy_versions
  pricing_release_plans_v2
  pricing_stage5_runs_v2
  provider_capability_entries
  pricing_policy_versions
  provider_switch_versions
  pricing_policies
  product_catalog_versions
  provider_capability_versions
)

# Keep the Drizzle export names paired with commerce_tables. SQL table-name scanning alone is not
# sufficient: a future reader could import one of these symbols without repeating its SQL name.
commerce_symbols=(
  accountPolicyReconciliations
  accountPolicyRules
  businessInvitePolicyBindings
  businessInvitePolicySnapshotsV2
  engineCatalogJobs
  enginePolicyJobs
  engineSwitchJobs
  pricingFundingNormalizationsV2
  pricingMonths
  pricingPolicyHeads
  pricingPolicyRules
  pricingPolicyRulesV2
  pricingReleaseActivationReceiptsV2
  pricingReleaseAssignmentsV2
  pricingReleaseControlJobsV2
  pricingReleaseOrchestrationsV2
  pricingShadowPolicyJobsV2
  pricingStage5BlockersV2
  pricingStage5PrepareAcksV2
  pricingStage8CaptureArtifactsV2
  pricingUsageFundingAllocations
  productCatalogHeads
  providerCapabilityAliases
  providerCapabilityHead
  providerSwitchEntries
  providerSwitchHead
  serviceAccountInventoryV2
  accountPolicyBindings
  pricingPolicyDocumentsV2
  pricingShadowRolloutsV2
  pricingStage8CaptureJobsV2
  pricingStage8EvidenceV2
  pricingUsageAttributions
  productCatalogEntries
  accountPolicyVersions
  pricingReleasePlansV2
  pricingStage5RunsV2
  providerCapabilityEntries
  pricingPolicyVersions
  providerSwitchVersions
  pricingPolicies
  productCatalogVersions
  providerCapabilityVersions
)

engine_retired_functions=(
  'assert_active_funding_account_v2()'
  'assert_funding_generation_v2(text, bigint)'
  'assert_funding_reservation_snapshot_v2(text)'
  'assert_normalized_reservation_funding_v2(text)'
  'assert_pricing_release_assignment_extension_pair_v2()'
  'assert_pricing_request_funding_v2(text)'
  'assert_strict_funding_account(text)'
  'assert_strict_reservation(text)'
  'enforce_account_funding_head_step_v2()'
  'enforce_active_funding_v2_from_account()'
  'enforce_active_funding_v2_from_head()'
  'enforce_funding_generation_v2_from_generation()'
  'enforce_funding_generation_v2_from_lot()'
  'enforce_funding_ledger_v2_account()'
  'enforce_funding_reservation_allocation_v2_update()'
  'enforce_funding_reservation_snapshot_v2_account()'
  'enforce_funding_reservation_v2_from_allocation()'
  'enforce_funding_reservation_v2_from_head()'
  'enforce_funding_reservation_v2_from_reservation()'
  'enforce_funding_reservation_v2_from_snapshot()'
  'enforce_ledger_funding_allocation_account()'
  'enforce_pricing_release_activation_v2()'
  'enforce_pricing_release_assignment_extension_v2()'
  'enforce_pricing_release_head_audit_v2()'
  'enforce_pricing_release_head_step_v2()'
  'enforce_pricing_release_recovery_kinds_v2()'
  'enforce_pricing_release_rule_owner_v2()'
  'enforce_pricing_request_funding_v2_from_allocation()'
  'enforce_pricing_request_funding_v2_from_reservation()'
  'enforce_pricing_request_funding_v2_from_snapshot()'
  'enforce_pricing_request_v2_account()'
  'enforce_pricing_shadow_admission_rule_identity()'
  'enforce_pricing_snapshot_reservation_account()'
  'enforce_release_v2_lineage()'
  'enforce_service_meter_only_policy_rule()'
  'enforce_strict_binding_cutover()'
  'enforce_strict_binding_runtime_floor()'
  'enforce_strict_binding_state()'
  'enforce_strict_funding_account_from_account()'
  'enforce_strict_funding_account_from_bucket()'
  'enforce_strict_key_policy_ack()'
  'enforce_strict_reservation_from_allocation()'
  'enforce_strict_reservation_from_reservation()'
  'reject_funding_reservation_snapshot_v2_update()'
  'reject_immutable_pricing_release_v2_mutation()'
  'reject_pricing_request_v2_update()'
  'reject_pricing_shadow_admission_evaluation_update()'
  'reject_pricing_snapshot_update()'
)

engine_live_functions=(
  'enforce_account_settlement_floor_fence()'
  'enforce_priced_terminal_collection_fence()'
  'enforce_pricing_tariff_override_version()'
  'reject_pricing_tariff_override_mutation()'
)

commerce_retired_functions=(
  'enforce_pricing_policy_rule_v2_owner()'
  'guard_pricing_release_assignment_v2()'
  'guard_pricing_release_plan_v2()'
  'guard_pricing_stage5_run_v2()'
  'notify_pricing_control_job()'
)

commerce_live_functions=(
  'enforce_blog_draft_identity()'
  'enforce_blog_first_publication()'
)

retired_route_fragments=(
  'pricing/v2'
  'pricing-stage5-v2'
  'pricing-stage6-v2'
  'pricing-stage8-capture-v2'
  'pricing-release-activation-v2'
  'pricing-release-orchestration-v2'
  'pricing-shadow-rollout-v2'
  'pricing-policy-delivery-repairs'
  'pricing-catalog-jobs'
  'pricing-switch-jobs'
  'policy-enforcement-cutover'
  'strict-backfill'
)

(( ${#engine_tables[@]} == 31 )) || die "engine manifest has ${#engine_tables[@]} tables, expected 31"
(( ${#commerce_tables[@]} == 43 )) || die "commerce manifest has ${#commerce_tables[@]} tables, expected 43"
(( ${#commerce_symbols[@]} == ${#commerce_tables[@]} )) \
  || die 'commerce Drizzle symbol manifest is not paired with the table manifest'
(( ${#engine_retired_functions[@]} == 48 )) || die 'engine retired-function manifest must contain 48 functions'
(( ${#engine_live_functions[@]} == 4 )) || die 'engine live-function allowlist must contain four functions'
(( ${#commerce_retired_functions[@]} == 5 )) \
  || die 'commerce retired-function manifest must contain five functions'
(( ${#commerce_live_functions[@]} == 2 )) || die 'commerce live-function allowlist must contain two functions'
[[ -f $RUNBOOK ]] || die "runbook is missing: $RUNBOOK"

TEMP=$(mktemp -d "${TMPDIR:-/tmp}/pricing-retired-schema.XXXXXX") \
  || die 'could not create a temporary scan directory'
cleanup() {
  rm -f -- "$TEMP/engine-patterns" "$TEMP/commerce-patterns" "$TEMP/route-patterns"
  rmdir -- "$TEMP"
}
trap cleanup EXIT

printf '%s\n' "${engine_tables[@]}" >"$TEMP/engine-patterns"
printf '%s\n' "${commerce_tables[@]}" "${commerce_symbols[@]}" >"$TEMP/commerce-patterns"
printf '%s\n' "${retired_route_fragments[@]}" >"$TEMP/route-patterns"

grep_count() {
  local needle=$1 file=$2 count status
  set +e
  count=$(grep -Fxc -- "$needle" "$file")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die "could not inspect $file" ;;
  esac
}

text_count() {
  local needle=$1 body=$2 count status
  set +e
  count=$(grep -Fxc -- "$needle" <<<"$body")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die 'could not inspect a runbook manifest section' ;;
  esac
}

manifest_line_count() {
  local body=$1 count status
  set +e
  count=$(grep -Ec '^- `public\.[^`()]+`$' <<<"$body")
  status=$?
  set -e
  case $status in
    0|1) printf '%s\n' "$count" ;;
    *) die 'could not count runbook manifest lines' ;;
  esac
}

scan_tracked_source() {
  [[ $# -ge 3 ]] || die 'tracked-source scan requires a mode, pattern file, and path'
  local mode=$1 pattern_file=$2 path output status matches=''
  shift 2
  while IFS= read -r -d '' path; do
    case $mode in
      engine)
        [[ $path == crates/*/src/* ]] || continue
        case $path in
          */tests.rs|*/tests/*) continue ;;
        esac
        ;;
      commerce)
        case $path in
          packages/db/src/schema.ts|*/migrations/*|*.test.*|*.spec.*|*/__tests__/*|*/dist/*|*.md)
            continue
            ;;
        esac
        ;;
      routes)
        case $path in
          */migrations/*|*.test.*|*.spec.*|*/tests.rs|*/tests/*|*/__tests__/*|*/dist/*|*.md)
            continue
            ;;
        esac
        ;;
      *) die "unknown tracked-source scan mode: $mode" ;;
    esac

    set +e
    output=$(LC_ALL=C grep -IHnF -f "$pattern_file" -- "$path")
    status=$?
    set -e
    case $status in
      0)
        [[ -z $matches ]] || matches+=$'\n'
        matches+=$output
        ;;
      1) ;;
      *) die "could not inspect tracked source file $path" ;;
    esac
  done < <(git ls-files -z -- "$@")
  printf '%s' "$matches"
}

engine_manifest=$(sed -n '/^## Engine drop set /,/^## Commerce drop set /p' "$RUNBOOK")
commerce_manifest=$(sed -n '/^## Commerce drop set /,/^## Mandatory pre-drop gates$/p' "$RUNBOOK")
[[ -n $engine_manifest && -n $commerce_manifest ]] || die 'runbook manifest sections are missing'

[[ $(manifest_line_count "$engine_manifest") == 31 ]] \
  || die 'runbook engine manifest contains extra, missing, or malformed table lines'
[[ $(manifest_line_count "$commerce_manifest") == 43 ]] \
  || die 'runbook commerce manifest contains extra, missing, or malformed table lines'

for table in "${engine_tables[@]}"; do
  [[ $(text_count "- \`public.$table\`" "$engine_manifest") == 1 ]] \
    || die "runbook engine manifest must name public.$table exactly once"
done
for table in "${commerce_tables[@]}"; do
  [[ $(text_count "- \`public.$table\`" "$commerce_manifest") == 1 ]] \
    || die "runbook commerce manifest must name public.$table exactly once"
done

for function in "${engine_retired_functions[@]}" "${engine_live_functions[@]}"; do
  [[ $(text_count "- \`public.$function\`" "$engine_manifest") == 1 ]] \
    || die "runbook engine function manifest must name public.$function exactly once"
done
for function in "${commerce_retired_functions[@]}"; do
  [[ $(text_count "- \`public.$function\`" "$commerce_manifest") == 1 ]] \
    || die "runbook commerce function manifest must name public.$function exactly once"
done
for function in "${commerce_live_functions[@]}"; do
  [[ $commerce_manifest == *"\`public.$function\`"* ]] \
    || die "runbook commerce live-function allowlist is missing public.$function"
done

for index in "${!commerce_tables[@]}"; do
  table=${commerce_tables[$index]}
  symbol=${commerce_symbols[$index]}
  [[ $(grep_count "export const $symbol = pgTable(\"$table\", {" \
    "$ROOT/packages/db/src/schema.ts") == 1 ]] \
    || die "Drizzle manifest mismatch for $symbol -> $table"
done

cd "$ROOT"

engine_matches=$(scan_tracked_source engine "$TEMP/engine-patterns" crates)
if [[ -n $engine_matches ]]; then
  expected_engine_include='const MIGRATION_0041: &str = include_str!("../migrations_pg/0041_strict_funding_lots_v2.sql");'
  [[ $(printf '%s\n' "$engine_matches" | wc -l | tr -d ' ') == 1 \
    && $engine_matches == crates/registry/src/pg.rs:*":$expected_engine_include" ]] \
    || die "engine runtime source references retired tables:\n$engine_matches"
fi

commerce_matches=$(scan_tracked_source commerce "$TEMP/commerce-patterns" apps packages)
[[ -z $commerce_matches ]] \
  || die "commerce runtime source references retired tables:\n$commerce_matches"

# A future controller/client must not resurrect the withdrawn policy/release endpoints as a
# rollback shim. Historical migrations and tests may name them; deployable source may not.
route_matches=$(scan_tracked_source routes "$TEMP/route-patterns" apps packages crates)
[[ -z $route_matches ]] || die "runtime source resurrects retired pricing routes:\n$route_matches"

printf 'pricing retired-schema manifest and source-reader checks passed\n'
