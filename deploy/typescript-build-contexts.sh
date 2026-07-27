#!/usr/bin/env bash
set -euo pipefail

# Build shared workspace libraries once, then build independent runtime contexts concurrently.
# Every selected context is complete enough to become an immutable release on its own.

die() { printf '[typescript-build] ERROR: %s\n' "$*" >&2; exit 1; }
log() { printf '[typescript-build] %s\n' "$*"; }

[[ $# -ge 2 ]] || die "usage: $0 <workspace-root> <context ...>"
[[ -d $1 && ! -L $1 ]] || die "workspace root must be a real directory: $1"
WORKSPACE=$(cd -- "$1" && pwd)
shift
[[ -f $WORKSPACE/package.json && ! -L $WORKSPACE/package.json ]] \
  || die "workspace package.json is missing"

CONTEXTS=()
for requested in "$@"; do
  case "$requested" in
    commerce|sales|openkeys|web) ;;
    *) die "unknown build context: $requested" ;;
  esac
  for existing in ${CONTEXTS[@]+"${CONTEXTS[@]}"}; do
    [[ $existing != "$requested" ]] || die "duplicate build context: $requested"
  done
  CONTEXTS+=("$requested")
done

# Keep the build policy fail-closed. A new workspace must be assigned to a context before a clean
# candidate build can silently omit it.
node - "$WORKSPACE" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");

const root = process.argv[2];
const expected = new Set([
  "@claude-api/commercial-api",
  "@claude-api/content-studio",
  "@claude-api/openkeys",
  "@claude-api/payment-worker",
  "@claude-api/sales-api",
  "@claude-api/sales-web",
  "@claude-api/web",
  "@claude-api/contracts",
  "@claude-api/db",
  "@claude-api/engine-client",
  "@claude-api/openkeys-db",
  "@claude-api/payments",
  "@claude-api/sales-db",
]);
const discovered = new Set();
for (const parent of ["apps", "packages"]) {
  const directory = path.join(root, parent);
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifestPath = path.join(directory, entry.name, "package.json");
    if (!fs.existsSync(manifestPath)) continue;
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (typeof manifest.name !== "string" || discovered.has(manifest.name)) {
      throw new Error(`workspace package has an unsafe or duplicate name: ${manifestPath}`);
    }
    if (typeof manifest.scripts?.build !== "string" || manifest.scripts.build.trim() === "") {
      throw new Error(`${manifest.name} has no build command`);
    }
    discovered.add(manifest.name);
  }
}
for (const name of discovered) {
  if (!expected.has(name)) throw new Error(`${name} has no build context`);
}
for (const name of expected) {
  if (!discovered.has(name)) throw new Error(`declared build package is missing: ${name}`);
}
NODE

SHARED_PACKAGES=()
add_shared() {
  local package=$1 existing
  for existing in ${SHARED_PACKAGES[@]+"${SHARED_PACKAGES[@]}"}; do
    [[ $existing != "$package" ]] || return 0
  done
  SHARED_PACKAGES+=("$package")
}

for context in "${CONTEXTS[@]}"; do
  case "$context" in
    commerce)
      add_shared @claude-api/contracts
      add_shared @claude-api/db
      add_shared @claude-api/engine-client
      add_shared @claude-api/payments
      ;;
    sales)
      add_shared @claude-api/sales-db
      ;;
    openkeys)
      add_shared @claude-api/contracts
      add_shared @claude-api/engine-client
      add_shared @claude-api/openkeys-db
      ;;
  esac
done

run_build() {
  local concurrency=$1 package
  shift
  local filters=()
  for package in "$@"; do filters+=("--filter=$package"); done
  pnpm --dir "$WORKSPACE" "${filters[@]}" -r \
    --workspace-concurrency="$concurrency" --if-present --fail-if-no-match build
}

if (( ${#SHARED_PACKAGES[@]} > 0 )); then
  log "building ${#SHARED_PACKAGES[@]} shared package(s) once"
  run_build 4 "${SHARED_PACKAGES[@]}"
fi

CONTEXT_PIDS=()
CONTEXT_LABELS=()
start_context() {
  local label=$1
  shift
  log "starting $label context build"
  run_build 3 "$@" &
  CONTEXT_PIDS+=("$!")
  CONTEXT_LABELS+=("$label")
}

terminate_contexts() {
  local pid
  trap - HUP INT TERM
  for pid in "${CONTEXT_PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  for pid in "${CONTEXT_PIDS[@]}"; do wait "$pid" 2>/dev/null || true; done
  exit 130
}
trap terminate_contexts HUP INT TERM

for context in "${CONTEXTS[@]}"; do
  case "$context" in
    commerce)
      start_context commerce @claude-api/commercial-api @claude-api/payment-worker \
        @claude-api/content-studio
      ;;
    sales)
      start_context sales @claude-api/sales-api @claude-api/sales-web
      ;;
    openkeys)
      start_context openkeys @claude-api/openkeys
      ;;
    web)
      start_context web @claude-api/web
      ;;
  esac
done

failures=()
for index in "${!CONTEXT_PIDS[@]}"; do
  if wait "${CONTEXT_PIDS[$index]}"; then
    log "${CONTEXT_LABELS[$index]} context passed"
  else
    failures+=("${CONTEXT_LABELS[$index]}")
  fi
done
trap - HUP INT TERM

(( ${#failures[@]} == 0 )) || die "context build(s) failed: ${failures[*]}"
log 'all selected TypeScript contexts built'
