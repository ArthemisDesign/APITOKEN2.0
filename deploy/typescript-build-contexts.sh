#!/usr/bin/env bash
set -euo pipefail

# Build shared workspace libraries once, then build independent runtime contexts concurrently.
# Every selected context is complete enough to become an immutable release on its own.

die() { printf '[typescript-build] ERROR: %s\n' "$*" >&2; exit 1; }
log() { printf '[typescript-build] %s\n' "$*"; }

[[ $# -ge 2 ]] || die "usage: $0 <workspace-root> <context ...>"
[[ -d $1 && ! -L $1 ]] || die "workspace root must be a real directory: $1"
WORKSPACE=$(cd -- "$1" && pwd -P)
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

# A build output cache complements pnpm's and Next's compiler caches: an exact content key can
# restore the complete runtime outputs and skip the build itself. It is deliberately optional.
# A missing, corrupt, or contended entry is only a cache miss; the normal clean build remains the
# source of truth.
CACHE_ROOT=${TYPESCRIPT_ARTIFACT_CACHE_ROOT:-}
CACHE_KEEP=${TYPESCRIPT_ARTIFACT_CACHE_KEEP:-6}
CACHE_ENABLED=0
case "$CACHE_KEEP" in
  ''|*[!0-9]*) CACHE_KEEP=6 ;;
esac
(( CACHE_KEEP >= 1 && CACHE_KEEP <= 20 )) || CACHE_KEEP=6
if [[ -n $CACHE_ROOT ]]; then
  if [[ $CACHE_ROOT != /* ]]; then
    log "artifact cache disabled: root is not absolute"
  elif [[ -L $CACHE_ROOT ]]; then
    log "artifact cache disabled: root is a symlink"
  elif mkdir -p -- "$CACHE_ROOT" 2>/dev/null \
    && [[ -d $CACHE_ROOT && ! -L $CACHE_ROOT && -w $CACHE_ROOT ]]; then
    CACHE_ROOT=$(cd -- "$CACHE_ROOT" && pwd -P)
    CACHE_ENABLED=1
  else
    log "artifact cache disabled: root is unavailable"
  fi
fi

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  else
    shasum -a 256 | awk '{print $1}'
  fi
}

cache_input_paths() {
  local context=$1
  CACHE_INPUT_PATHS=(
    package.json
    pnpm-lock.yaml
    pnpm-workspace.yaml
    .node-version
    tsconfig.base.json
    deploy/typescript-build-contexts.sh
  )
  case "$context" in
    commerce)
      CACHE_INPUT_PATHS+=(
        apps/api apps/worker apps/content-studio
        packages/contracts packages/db packages/engine-client packages/payments
      )
      ;;
    sales)
      CACHE_INPUT_PATHS+=(apps/sales-api apps/sales-web packages/sales-db)
      ;;
    openkeys)
      CACHE_INPUT_PATHS+=(
        apps/openkeys packages/contracts packages/engine-client packages/openkeys-db
      )
      ;;
    web)
      CACHE_INPUT_PATHS+=(apps/web)
      ;;
  esac
}

artifact_cache_key() {
  local context=$1
  cache_input_paths "$context"
  {
    printf 'typescript-artifact-cache-v1\ncontext=%s\n' "$context"
    printf 'platform=%s/%s/%s\n' "$(uname -s)" "$(uname -m)" "$(uname -r)"
    printf 'node=%s\n' "$(node --version)"
    printf 'pnpm=%s\n' "$(pnpm --version)"
    node - "$context" <<'NODE'
const crypto = require("node:crypto");

const context = process.argv[2];
const common = ["NODE_ENV", "NEXT_TELEMETRY_DISABLED"];
const names = {
  commerce: [...common, "DATABASE_URL", "TEST_DATABASE_URL"],
  sales: [
    ...common,
    "NEXT_PUBLIC_SALES_API_URL",
    "SALES_DATABASE_URL",
    "TEST_SALES_DATABASE_URL",
  ],
  openkeys: [
    ...common,
    "ENGINE_BASE_URL",
    "ENGINE_PUBLIC_BASE_URL",
    "OPENKEYS_ADMIN_ACCOUNTS",
    "OPENKEYS_ADMIN_PASSWORD",
    "OPENKEYS_ADMIN_USER",
    "OPENKEYS_DATABASE_URL",
    "OPENKEYS_PUBLIC_BASE_URL",
    "OPENKEYS_SECRET_KEY",
  ],
  web: [
    ...common,
    "BING_SITE_VERIFICATION",
    "AUDIT_FILTER",
    "AUDIT_SCOPE",
    "AUDIT_START_AT",
    "AUDIT_VERIFY_COMPLIANCE",
    "AUDIT_VERIFY_CREDITS",
    "AUDIT_VERIFY_DOCS_THEME",
    "AUDIT_VERIFY_HERO",
    "AUDIT_VERIFY_KEYS",
    "AUDIT_VERIFY_PRICING",
    "AUDIT_VERIFY_PROFILE",
    "AUDIT_VERIFY_ROUTING",
    "AUDIT_VERIFY_SITE_ROUTING",
    "AUDIT_VERIFY_USAGE",
    "CHROME_PATH",
    "GOOGLE_SERVICE_ACCOUNT",
    "GOOGLE_SITE_VERIFICATION",
    "GSC_SITE",
    "YANDEX_SITE_VERIFICATION",
    "NEXT_PUBLIC_BACKEND_URL",
    "NEXT_PUBLIC_DOCS_URL",
    "SCREENSHOT_DIR",
    "SITE_URL",
  ],
}[context];
for (const name of [...new Set(names)].sort()) {
  const value = Object.hasOwn(process.env, name) ? process.env[name] : "<unset>";
  const digest = crypto.createHash("sha256").update(value).digest("hex");
  process.stdout.write(`env:${name}=${digest}\n`);
}
NODE
    git -C "$WORKSPACE" ls-tree -r --full-tree HEAD -- "${CACHE_INPUT_PATHS[@]}" \
      | LC_ALL=C sort
  } | sha256_stream
}

# Node performs the filesystem walk because lstat/copy semantics are consistent on both macOS and
# Linux. Only fixed generated-output roots are touched, and the cache manifest covers every
# restored file and its executable bits. Cache trees reject links; a generated workspace-relative
# link is stored only as validated metadata and recreated inside the installed candidate. Next's
# incremental `.next/cache` stays in its dedicated cache while complete runtime outputs are swapped.
artifact_cache_files() {
  local operation=$1 entry=$2 context=$3 key=$4
  node - "$operation" "$WORKSPACE" "$entry" "$context" "$key" <<'NODE'
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
process.on("uncaughtException", (error) => {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
});

const [operation, workspace, entry, context, key] = process.argv.slice(2);
const rootsByContext = {
  commerce: [
    "packages/contracts/dist",
    "packages/db/dist",
    "packages/engine-client/dist",
    "packages/payments/dist",
    "apps/api/dist",
    "apps/worker/dist",
    "apps/content-studio/.next",
  ],
  sales: [
    "packages/sales-db/dist",
    "apps/sales-api/dist",
    "apps/sales-web/.next",
  ],
  openkeys: [
    "packages/contracts/dist",
    "packages/engine-client/dist",
    "packages/openkeys-db/dist",
    "apps/openkeys/.next",
  ],
  web: ["apps/web/.next"],
};
const requiredByContext = {
  commerce: [
    "packages/db/dist/migrate.js",
    "apps/api/dist/main.js",
    "apps/worker/dist/main.js",
    "apps/content-studio/.next/BUILD_ID",
  ],
  sales: [
    "packages/sales-db/dist/migrate.js",
    "apps/sales-api/dist/main.js",
    "apps/sales-web/.next/BUILD_ID",
  ],
  openkeys: [
    "packages/openkeys-db/dist/migrate.js",
    "apps/openkeys/.next/BUILD_ID",
  ],
  web: ["apps/web/.next/BUILD_ID"],
};
const roots = rootsByContext[context];
const required = requiredByContext[context];
if (!roots || !required || !/^[0-9a-f]{64}$/.test(key)) {
  throw new Error("invalid cache context or key");
}

const manifestPath = path.join(entry, ".manifest.json");
const digestFile = (file) =>
  crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const modeOf = (stat) => stat.mode & 0o777;
const isNextCache = (relative) =>
  relative.endsWith("/.next/cache") || relative.includes("/.next/cache/");
const checkedStat = (target) => {
  const stat = fs.lstatSync(target);
  if (stat.isSymbolicLink()) throw new Error(`symbolic link is forbidden: ${target}`);
  if (!stat.isDirectory() && !stat.isFile()) {
    throw new Error(`special file is forbidden: ${target}`);
  }
  return stat;
};
const ensureInside = (base, target) => {
  const relative = path.relative(base, target);
  if (relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative))) return;
  throw new Error(`path escaped cache root: ${target}`);
};
const mkdirChecked = (directory, mode = 0o755) => {
  fs.mkdirSync(directory, { recursive: true, mode });
  const stat = fs.lstatSync(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new Error(`cache parent is not a real directory: ${directory}`);
  }
};

const ensureLinkResolutionInsideWorkspace = (resolved) => {
  let pending = resolved;
  let followedLinks = 0;
  while (true) {
    ensureInside(workspace, pending);
    const relative = path.relative(workspace, pending);
    const components = relative === "" ? [] : relative.split(path.sep);
    let current = workspace;
    let followed = false;
    for (let index = 0; index < components.length; index += 1) {
      const candidate = path.join(current, components[index]);
      let stat;
      try {
        stat = fs.lstatSync(candidate);
      } catch (error) {
        if (error?.code === "ENOENT" || error?.code === "ENOTDIR") return;
        throw error;
      }
      if (stat.isSymbolicLink()) {
        followedLinks += 1;
        if (followedLinks > 40) throw new Error(`symbolic-link cycle: ${resolved}`);
        pending = path.resolve(
          path.dirname(candidate),
          fs.readlinkSync(candidate),
          ...components.slice(index + 1),
        );
        ensureInside(workspace, pending);
        followed = true;
        break;
      }
      current = candidate;
    }
    if (!followed) return;
  }
};

const validateWorkspaceLink = (relative, target) => {
  if (typeof target !== "string" || target.length === 0 || target.includes("\0")
      || path.isAbsolute(target)) {
    throw new Error(`unsafe symbolic-link target: ${relative}`);
  }
  if (!roots.some((root) => relative.startsWith(`${root}/`))) {
    throw new Error(`symbolic link is outside an output root: ${relative}`);
  }
  const destination = path.join(workspace, ...relative.split("/"));
  ensureInside(workspace, destination);
  const resolved = path.resolve(path.dirname(destination), target);
  ensureInside(workspace, resolved);
  // Next standalone traces may contain a relative link to an intentionally omitted optional
  // package. Preserve that dangling link, but follow every existing path component so a dangling
  // intermediary cannot disguise an escape from the workspace.
  ensureLinkResolutionInsideWorkspace(resolved);
  return { path: relative, target };
};

function copyTree(source, destination, relativeRoot, records) {
  const sourceStat = checkedStat(source);
  if (!sourceStat.isDirectory()) throw new Error(`output root is not a directory: ${source}`);
  mkdirChecked(destination, modeOf(sourceStat));
  fs.chmodSync(destination, modeOf(sourceStat));
  const children = fs.readdirSync(source, { withFileTypes: true })
    .map((child) => child.name)
    .sort();
  for (const name of children) {
    const sourceChild = path.join(source, name);
    const destinationChild = path.join(destination, name);
    const relative = path.posix.join(relativeRoot, name);
    if (isNextCache(relative)) continue;
    const rawStat = fs.lstatSync(sourceChild);
    if (rawStat.isSymbolicLink()) {
      if (!records) throw new Error(`symbolic link leaked into cache entry: ${sourceChild}`);
      records.links.push(validateWorkspaceLink(relative, fs.readlinkSync(sourceChild)));
      continue;
    }
    const stat = checkedStat(sourceChild);
    if (stat.isDirectory()) {
      copyTree(sourceChild, destinationChild, relative, records);
      continue;
    }
    mkdirChecked(path.dirname(destinationChild));
    fs.copyFileSync(sourceChild, destinationChild, fs.constants.COPYFILE_EXCL);
    fs.chmodSync(destinationChild, modeOf(stat));
    if (records) {
      records.files.push({
        path: relative,
        mode: modeOf(stat),
        sha256: digestFile(destinationChild),
      });
    }
  }
}

function walkFiles(base, current, relativeRoot, entries) {
  ensureInside(base, current);
  const stat = checkedStat(current);
  if (!stat.isDirectory()) throw new Error(`cached output root is not a directory: ${current}`);
  const children = fs.readdirSync(current, { withFileTypes: true })
    .map((child) => child.name)
    .sort();
  for (const name of children) {
    const child = path.join(current, name);
    const relative = path.posix.join(relativeRoot, name);
    if (isNextCache(relative)) throw new Error("Next compiler cache leaked into artifact cache");
    const childStat = checkedStat(child);
    if (childStat.isDirectory()) {
      walkFiles(base, child, relative, entries);
    } else {
      entries.push({ path: relative, mode: modeOf(childStat), sha256: digestFile(child) });
    }
  }
}

function validateRequired(entries) {
  const files = new Set(entries.map((item) => item.path));
  for (const relative of required) {
    if (!files.has(relative)) throw new Error(`required runtime artifact is missing: ${relative}`);
  }
}

function verify() {
  const entryStat = checkedStat(entry);
  if (!entryStat.isDirectory()) throw new Error("cache entry is not a directory");
  const manifestStat = checkedStat(manifestPath);
  if (!manifestStat.isFile()) throw new Error("cache manifest is not a file");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.format !== 1 || manifest.context !== context || manifest.key !== key
      || !Array.isArray(manifest.files) || !Array.isArray(manifest.links)) {
    throw new Error("cache manifest identity is invalid");
  }
  const actual = [];
  for (const root of roots) {
    const cachedRoot = path.join(entry, ...root.split("/"));
    walkFiles(entry, cachedRoot, root, actual);
  }
  actual.sort((left, right) => left.path.localeCompare(right.path));
  const expected = [...manifest.files].sort((left, right) => left.path.localeCompare(right.path));
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error("cache contents do not match the manifest");
  }
  const links = manifest.links.map((link) => validateWorkspaceLink(link.path, link.target))
    .sort((left, right) => left.path.localeCompare(right.path));
  const linkPaths = new Set();
  const filePaths = new Set(actual.map((item) => item.path));
  for (const link of links) {
    if (linkPaths.has(link.path) || filePaths.has(link.path)) {
      throw new Error(`duplicate cache manifest path: ${link.path}`);
    }
    linkPaths.add(link.path);
    const cachedLink = path.join(entry, ...link.path.split("/"));
    if (fs.existsSync(cachedLink) || fs.lstatSync(path.dirname(cachedLink)).isSymbolicLink()) {
      throw new Error(`symbolic-link metadata leaked into cache tree: ${link.path}`);
    }
  }
  validateRequired(actual);
  return { files: actual, links };
}

function clearRoot(relative) {
  const target = path.join(workspace, ...relative.split("/"));
  ensureInside(workspace, target);
  if (!relative.endsWith("/.next")) {
    fs.rmSync(target, { recursive: true, force: true });
    return;
  }
  if (!fs.existsSync(target)) return;
  const stat = checkedStat(target);
  if (!stat.isDirectory()) {
    fs.rmSync(target, { force: true });
    return;
  }
  for (const name of fs.readdirSync(target)) {
    const child = path.join(target, name);
    if (name === "cache") {
      const cacheStat = fs.lstatSync(child);
      if (cacheStat.isDirectory() && !cacheStat.isSymbolicLink()) continue;
    }
    fs.rmSync(child, { recursive: true, force: true });
  }
}

if (operation === "snapshot") {
  mkdirChecked(entry);
  const records = { files: [], links: [] };
  for (const root of roots) {
    const source = path.join(workspace, ...root.split("/"));
    const destination = path.join(entry, ...root.split("/"));
    copyTree(source, destination, root, records);
  }
  records.files.sort((left, right) => left.path.localeCompare(right.path));
  records.links.sort((left, right) => left.path.localeCompare(right.path));
  validateRequired(records.files);
  fs.writeFileSync(manifestPath, `${JSON.stringify({
    format: 1,
    context,
    key,
    files: records.files,
    links: records.links,
  })}\n`, { flag: "wx", mode: 0o444 });
} else if (operation === "verify") {
  verify();
} else if (operation === "restore") {
  const manifest = verify();
  for (const root of roots) clearRoot(root);
  for (const root of roots) {
    const source = path.join(entry, ...root.split("/"));
    const destination = path.join(workspace, ...root.split("/"));
    copyTree(source, destination, root, null);
  }
  for (const link of manifest.links) {
    const safe = validateWorkspaceLink(link.path, link.target);
    const destination = path.join(workspace, ...safe.path.split("/"));
    mkdirChecked(path.dirname(destination));
    fs.symlinkSync(safe.target, destination);
  }
} else if (operation === "clear") {
  for (const root of roots) clearRoot(root);
} else {
  throw new Error(`unknown cache operation: ${operation}`);
}
NODE
}

artifact_cache_clear() {
  local context=$1
  artifact_cache_files clear "$CACHE_ROOT" "$context" "$(printf '%064d' 0)"
}

artifact_cache_restore() {
  local context=$1 key component_root entry restore_error
  (( CACHE_ENABLED == 1 )) || return 1
  key=$(artifact_cache_key "$context") || return 1
  [[ $key =~ ^[0-9a-f]{64}$ ]] || return 1
  component_root=$CACHE_ROOT/$context
  entry=$component_root/$key
  [[ -d $component_root && ! -L $component_root && -d $entry && ! -L $entry ]] || return 1
  if restore_error=$(artifact_cache_files restore "$entry" "$context" "$key" 2>&1); then
    log "$context artifact cache hit ($key)"
    return 0
  fi
  restore_error=${restore_error%%$'\n'*}
  log "$context artifact cache entry is corrupt (${restore_error:-verification failed}); rebuilding"
  artifact_cache_files clear "$CACHE_ROOT" "$context" "$key" >/dev/null 2>&1 || true
  return 1
}

artifact_cache_prune() {
  local component_root=$1 protected_entry=$2 kept=0 entry name
  local ordered=()
  if [[ -d $protected_entry && ! -L $protected_entry \
        && ${protected_entry%/*} == "$component_root" ]]; then
    ordered+=("$protected_entry")
  fi
  while IFS= read -r entry; do ordered+=("$entry"); done < <(
    for entry in "$component_root"/*; do
      [[ -d $entry && ! -L $entry ]] || continue
      [[ $entry != "$protected_entry" ]] || continue
      name=${entry##*/}
      [[ $name =~ ^[0-9a-f]{64}$ ]] || continue
      if stat -c %Y -- "$entry" >/dev/null 2>&1; then
        printf '%s %s\n' "$(stat -c %Y -- "$entry")" "$entry"
      else
        printf '%s %s\n' "$(stat -f %m -- "$entry")" "$entry"
      fi
    done | LC_ALL=C sort -rn | sed 's/^[0-9][0-9]* //'
  )
  for entry in ${ordered[@]+"${ordered[@]}"}; do
    kept=$((kept + 1))
    (( kept <= CACHE_KEEP )) && continue
    name=${entry##*/}
    [[ ${entry%/*} == "$component_root" && $name =~ ^[0-9a-f]{64}$ ]] || continue
    rm -rf -- "$entry" 2>/dev/null \
      || log "could not prune old $name artifact cache entry"
  done
}

artifact_cache_save() {
  local context=$1 key component_root entry lock temp snapshot_error
  (( CACHE_ENABLED == 1 )) || return 0
  key=$(artifact_cache_key "$context") || {
    log "$context artifact cache key failed; continuing without save"
    return 0
  }
  [[ $key =~ ^[0-9a-f]{64}$ ]] || return 0
  component_root=$CACHE_ROOT/$context
  entry=$component_root/$key
  lock=$component_root/.$key.lock
  if [[ -L $component_root ]] \
    || ! mkdir -p -- "$component_root" 2>/dev/null \
    || [[ ! -d $component_root || -L $component_root ]]; then
    log "$context artifact cache directory is unavailable; continuing without save"
    return 0
  fi
  if [[ -d $entry && ! -L $entry ]] \
    && artifact_cache_files verify "$entry" "$context" "$key" >/dev/null 2>&1; then
    return 0
  fi
  if ! mkdir -- "$lock" 2>/dev/null; then
    log "$context artifact cache save is already in progress"
    return 0
  fi
  temp=$(mktemp -d "$component_root/.tmp.$key.XXXXXX") || {
    rmdir -- "$lock" 2>/dev/null || true
    return 0
  }
  if ! snapshot_error=$(artifact_cache_files snapshot "$temp" "$context" "$key" 2>&1); then
    rm -rf -- "$temp" 2>/dev/null || true
    rmdir -- "$lock" 2>/dev/null || true
    snapshot_error=${snapshot_error%%$'\n'*}
    log "$context artifacts were incomplete (${snapshot_error:-unknown error}); continuing without cache save"
    return 0
  fi
  if [[ -e $entry || -L $entry ]]; then
    if ! rm -rf -- "$entry" 2>/dev/null; then
      rm -rf -- "$temp" 2>/dev/null || true
      rmdir -- "$lock" 2>/dev/null || true
      log "$context corrupt cache entry could not be replaced; continuing without save"
      return 0
    fi
  fi
  if mv -- "$temp" "$entry"; then
    log "$context artifact cache saved ($key)"
  else
    rm -rf -- "$temp" 2>/dev/null || true
  fi
  rmdir -- "$lock" 2>/dev/null || true
  # Multiple entries can be published within one filesystem timestamp tick. Always retain the
  # entry completed by this invocation instead of relying on an arbitrary lexical tie-break.
  artifact_cache_prune "$component_root" "$entry"
  return 0
}

BUILD_CONTEXTS=()
for context in "${CONTEXTS[@]}"; do
  if artifact_cache_restore "$context"; then
    continue
  fi
  if (( CACHE_ENABLED == 1 )); then
    # Remove any partial or stale complete artifacts while retaining the independent Next compiler
    # cache. A normal clean candidate has nothing to remove.
    artifact_cache_clear "$context" \
      || die "could not clear generated artifacts for $context cache miss"
    log "$context artifact cache miss"
  fi
  BUILD_CONTEXTS+=("$context")
done

SHARED_PACKAGES=()
add_shared() {
  local package=$1 existing
  for existing in ${SHARED_PACKAGES[@]+"${SHARED_PACKAGES[@]}"}; do
    [[ $existing != "$package" ]] || return 0
  done
  SHARED_PACKAGES+=("$package")
}

for context in ${BUILD_CONTEXTS[@]+"${BUILD_CONTEXTS[@]}"}; do
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

if (( ${#BUILD_CONTEXTS[@]} == 0 )); then
  log 'all selected TypeScript contexts restored from complete artifacts'
  exit 0
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

for context in "${BUILD_CONTEXTS[@]}"; do
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
for context in "${BUILD_CONTEXTS[@]}"; do
  artifact_cache_save "$context"
done
log 'all selected TypeScript contexts built'
