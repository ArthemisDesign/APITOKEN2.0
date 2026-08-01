#!/usr/bin/env bash
set -euo pipefail

# Run workspace tests in independent isolation domains. Each selected database context migrates its
# own disposable database inside the same background lane as its tests, so migrations overlap each
# other and the database-free tests without creating a global barrier.

die() { printf '[typescript-tests] ERROR: %s\n' "$*" >&2; exit 1; }
log() { printf '[typescript-tests] %s\n' "$*"; }

[[ $# -ge 1 ]] || die "usage: $0 <workspace-root> [exact-package-name ...]"
[[ -d $1 && ! -L $1 ]] || die "workspace root must be a real directory: $1"
WORKSPACE=$(cd -- "$1" && pwd)
shift
[[ -f $WORKSPACE/package.json && ! -L $WORKSPACE/package.json ]] \
  || die "workspace package.json is missing"

TEST_COMPONENTS=${TYPESCRIPT_TEST_COMPONENTS:-}
SELECTED_COMPONENTS=()
if [[ -n $TEST_COMPONENTS ]]; then
  IFS=, read -r -a SELECTED_COMPONENTS <<<"$TEST_COMPONENTS"
  seen_components=()
  for selected_component in "${SELECTED_COMPONENTS[@]}"; do
    case "$selected_component" in
      commerce|sales|openkeys|web|admin|devbot) ;;
      *) die "unknown TypeScript test component: $selected_component" ;;
    esac
    for seen_component in ${seen_components[@]+"${seen_components[@]}"}; do
      [[ $seen_component != "$selected_component" ]] \
        || die "duplicate TypeScript test component: $selected_component"
    done
    seen_components+=("$selected_component")
  done
  canonical_components=
  for expected_component in commerce sales openkeys web admin devbot; do
    for selected_component in "${SELECTED_COMPONENTS[@]}"; do
      [[ $selected_component != "$expected_component" ]] || {
        if [[ -n $canonical_components ]]; then canonical_components+=,; fi
        canonical_components+=$expected_component
        break
      }
    done
  done
  [[ $canonical_components == "$TEST_COMPONENTS" ]] \
    || die "TypeScript test components are not canonical: $TEST_COMPONENTS"
fi

PURE_PACKAGES=(
  @claude-api/content-studio
  @claude-api/web
  @claude-api/engine-client
  @claude-api/payments
  @claude-api/admin
  @claude-api/devbot
)
COMMERCE_PACKAGES=(
  @claude-api/db
  @claude-api/commercial-api
  @claude-api/payment-worker
)
SALES_PACKAGES=(
  @claude-api/sales-db
  @claude-api/sales-api
)
OPENKEYS_PACKAGES=(
  @claude-api/openkeys
)
TESTLESS_PACKAGES=(
  @claude-api/sales-web
  @claude-api/contracts
  @claude-api/openkeys-db
)
TEST_PACKAGES=(
  "${PURE_PACKAGES[@]}"
  "${COMMERCE_PACKAGES[@]}"
  "${SALES_PACKAGES[@]}"
  "${OPENKEYS_PACKAGES[@]}"
)
KNOWN_PACKAGES=("${TEST_PACKAGES[@]}" "${TESTLESS_PACKAGES[@]}")
REQUESTED_PACKAGES=("$@")

# Keep the grouping fail-closed. A newly added workspace package, a removed test script, or tests
# added to an explicitly testless package must be classified before the gate can pass.
node - "$WORKSPACE" "${TEST_PACKAGES[@]}" -- "${TESTLESS_PACKAGES[@]}" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const root = process.argv[2];
const declarations = process.argv.slice(3);
const divider = declarations.indexOf("--");
if (divider < 0) throw new Error("test-group declaration separator is missing");
const tested = new Set(declarations.slice(0, divider));
const testless = new Set(declarations.slice(divider + 1));
const expected = new Set([...tested, ...testless]);
if (expected.size !== tested.size + testless.size) {
  throw new Error("a workspace package belongs to more than one test policy");
}

const discovered = new Set();
for (const parent of ["apps", "packages"]) {
  const parentPath = path.join(root, parent);
  if (!fs.existsSync(parentPath)) continue;
  for (const entry of fs.readdirSync(parentPath, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifestPath = path.join(parentPath, entry.name, "package.json");
    if (!fs.existsSync(manifestPath)) continue;
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (typeof manifest.name !== "string" || discovered.has(manifest.name)) {
      throw new Error(`workspace package has an unsafe or duplicate name: ${manifestPath}`);
    }
    discovered.add(manifest.name);
    const test = manifest.scripts?.test;
    if (tested.has(manifest.name) && (typeof test !== "string" || test.trim() === "")) {
      throw new Error(`${manifest.name} lost its required test script`);
    }
    if (testless.has(manifest.name) && typeof test === "string" && test.trim() !== "") {
      throw new Error(`${manifest.name} now has tests and needs an isolation group`);
    }
    if (!expected.has(manifest.name)) {
      throw new Error(`${manifest.name} has no test isolation policy`);
    }
  }
}
for (const name of expected) {
  if (!discovered.has(name)) throw new Error(`declared workspace package is missing: ${name}`);
}
NODE

package_is_known() {
  local expected=$1 known
  for known in "${KNOWN_PACKAGES[@]}"; do
    [[ $known != "$expected" ]] || return 0
  done
  return 1
}

package_is_requested() {
  local expected=$1 requested
  (( ${#REQUESTED_PACKAGES[@]} > 0 )) || return 0
  for requested in ${REQUESTED_PACKAGES[@]+"${REQUESTED_PACKAGES[@]}"}; do
    [[ $requested != "$expected" ]] || return 0
  done
  return 1
}

component_is_selected() {
  local expected=$1 selected
  for selected in ${SELECTED_COMPONENTS[@]+"${SELECTED_COMPONENTS[@]}"}; do
    [[ $selected != "$expected" ]] || return 0
  done
  return 1
}

run_component_migration() {
  local label=$1 dsn='' variable='' artifact=''
  component_is_selected "$label" || return 0
  case "$label" in
    commerce)
      dsn=${TEST_DATABASE_URL:-}
      variable=DATABASE_URL
      artifact=packages/db/dist/migrate.js
      ;;
    sales)
      dsn=${TEST_SALES_DATABASE_URL:-}
      variable=SALES_DATABASE_URL
      artifact=packages/sales-db/dist/migrate.js
      ;;
    openkeys)
      dsn=${TEST_OPENKEYS_DATABASE_URL:-}
      variable=OPENKEYS_DATABASE_URL
      artifact=packages/openkeys-db/dist/migrate.js
      ;;
    *) return 0 ;;
  esac
  [[ -n $dsn ]] || die "$label test component has no disposable database URL"
  [[ -f $WORKSPACE/$artifact && ! -L $WORKSPACE/$artifact ]] \
    || die "$label migration artifact is missing: $artifact"
  log "migrating the $label disposable database"
  env "$variable=$dsn" node "$WORKSPACE/$artifact"
  log "$label disposable database migrated"
}

# Раскрытие через ${x[@]+…}: в bash 3.2 (macOS) обращение к пустому массиву под
# set -u — ошибка «unbound variable», а пустой список пакетов здесь нормален.
for requested in ${REQUESTED_PACKAGES[@]+"${REQUESTED_PACKAGES[@]}"}; do
  [[ $requested =~ ^@[a-z0-9][a-z0-9._~-]*/[a-z0-9][a-z0-9._~-]*$ ]] \
    || die "unsafe package selector: $requested"
  package_is_known "$requested" || die "unclassified workspace package selector: $requested"
done

run_group() {
  local label=$1 concurrency=$2 package migration_required=0
  local filters=()
  shift 2
  for package in "$@"; do
    package_is_requested "$package" || continue
    filters+=("--filter=$package")
  done
  case "$label" in
    commerce|sales|openkeys)
      component_is_selected "$label" && migration_required=1
      ;;
  esac
  if (( ${#filters[@]} == 0 && migration_required == 0 )); then
    log "$label group skipped by package scope"
    return 0
  fi
  log "starting $label lane (${#filters[@]} test package(s), migration=$migration_required)"
  run_component_migration "$label"
  if (( ${#filters[@]} == 0 )); then
    log "$label lane passed with migration only"
    return 0
  fi
  # Candidate builds already produced and verified dependency artifacts. Suppress package
  # pretest hooks here so application tests do not rebuild those dependencies a second time.
  pnpm --config.enable-pre-post-scripts=false --dir "$WORKSPACE" "${filters[@]}" -r \
    --workspace-concurrency="$concurrency" --if-present --fail-if-no-match test
}

GROUP_PIDS=()
GROUP_LABELS=()
start_group() {
  local label=$1
  shift
  run_group "$label" "$@" &
  GROUP_PIDS+=("$!")
  GROUP_LABELS+=("$label")
}

terminate_groups() {
  local pid
  trap - HUP INT TERM
  for pid in "${GROUP_PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${GROUP_PIDS[@]}"; do wait "$pid" 2>/dev/null || true; done
  exit 130
}
trap terminate_groups HUP INT TERM

# Database groups remain serial internally. Pure unit/browser packages have no shared mutable
# service and may use bounded pnpm workspace parallelism as well.
start_group pure 4 "${PURE_PACKAGES[@]}"
start_group commerce 1 "${COMMERCE_PACKAGES[@]}"
start_group sales 1 "${SALES_PACKAGES[@]}"
start_group openkeys 1 "${OPENKEYS_PACKAGES[@]}"

failures=()
for index in "${!GROUP_PIDS[@]}"; do
  if wait "${GROUP_PIDS[$index]}"; then
    log "${GROUP_LABELS[$index]} lane passed"
  else
    failures+=("${GROUP_LABELS[$index]}")
  fi
done
trap - HUP INT TERM

(( ${#failures[@]} == 0 )) \
  || die "test lane(s) failed: ${failures[*]}"
log 'all selected TypeScript test lanes passed'
