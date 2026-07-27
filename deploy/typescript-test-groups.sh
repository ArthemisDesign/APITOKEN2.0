#!/usr/bin/env bash
set -euo pipefail

# Run workspace tests in independent isolation domains. Packages sharing one test database remain
# serial inside their group; groups use separate databases (or no database) and run concurrently.

die() { printf '[typescript-tests] ERROR: %s\n' "$*" >&2; exit 1; }
log() { printf '[typescript-tests] %s\n' "$*"; }

[[ $# -ge 1 ]] || die "usage: $0 <workspace-root> [exact-package-name ...]"
[[ -d $1 && ! -L $1 ]] || die "workspace root must be a real directory: $1"
WORKSPACE=$(cd -- "$1" && pwd)
shift
[[ -f $WORKSPACE/package.json && ! -L $WORKSPACE/package.json ]] \
  || die "workspace package.json is missing"

PURE_PACKAGES=(
  @claude-api/content-studio
  @claude-api/web
  @claude-api/engine-client
  @claude-api/payments
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
  for requested in "${REQUESTED_PACKAGES[@]}"; do
    [[ $requested != "$expected" ]] || return 0
  done
  return 1
}

for requested in "${REQUESTED_PACKAGES[@]}"; do
  [[ $requested =~ ^@[a-z0-9][a-z0-9._~-]*/[a-z0-9][a-z0-9._~-]*$ ]] \
    || die "unsafe package selector: $requested"
  package_is_known "$requested" || die "unclassified workspace package selector: $requested"
done

run_group() {
  local label=$1 concurrency=$2 package
  local filters=()
  shift 2
  for package in "$@"; do
    package_is_requested "$package" || continue
    filters+=("--filter=$package")
  done
  if (( ${#filters[@]} == 0 )); then
    log "$label group skipped by package scope"
    return 0
  fi
  log "starting $label group (${#filters[@]} package(s), workspace concurrency $concurrency)"
  pnpm --dir "$WORKSPACE" "${filters[@]}" -r \
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
    log "${GROUP_LABELS[$index]} group passed"
  else
    failures+=("${GROUP_LABELS[$index]}")
  fi
done
trap - HUP INT TERM

(( ${#failures[@]} == 0 )) \
  || die "test group(s) failed: ${failures[*]}"
log 'all selected TypeScript test groups passed'
