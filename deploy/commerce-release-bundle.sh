#!/usr/bin/env bash
set -eEuo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

fail() {
  printf 'commerce-release-bundle: %s\n' "$*" >&2
  exit 1
}

[[ $# -eq 1 ]] || fail "usage: commerce-release-bundle.sh <tested-candidate>"
INPUT=$1
[[ -d $INPUT && ! -L $INPUT ]] || fail "candidate must be a real directory: $INPUT"
CANDIDATE=$(cd -- "$INPUT" && pwd -P)
ARTIFACT_ROOT=$CANDIDATE/.deploy-artifacts
BUNDLE=$ARTIFACT_ROOT/commerce-release
STAGE=$ARTIFACT_ROOT/.commerce-release.tmp.${BASHPID:-$$}

cleanup() {
  [[ -n ${STAGE:-} && $STAGE == "$ARTIFACT_ROOT"/.commerce-release.tmp.* ]] || return 0
  if [[ -e $STAGE || -L $STAGE ]]; then
    chmod -R u+w "$STAGE" 2>/dev/null || true
    rm -rf -- "$STAGE"
  fi
}
trap cleanup EXIT

[[ ! -e $BUNDLE && ! -L $BUNDLE ]] \
  || fail "commerce release artifact already exists: $BUNDLE"
[[ ! -e $STAGE && ! -L $STAGE ]] || fail "temporary artifact path already exists: $STAGE"

required_files=(
  deploy/engine-commerce-compatibility.contract
  package.json
  pnpm-lock.yaml
  pnpm-workspace.yaml
  apps/api/package.json
  apps/api/dist/main.js
  apps/worker/package.json
  apps/worker/dist/main.js
  apps/content-studio/package.json
  apps/content-studio/.next/BUILD_ID
  apps/content-studio/.next/standalone/apps/content-studio/server.js
  packages/contracts/package.json
  packages/contracts/dist/index.js
  packages/db/package.json
  packages/db/dist/migrate.js
  packages/engine-client/package.json
  packages/engine-client/dist/index.js
  packages/payments/package.json
  packages/payments/dist/index.js
)
for relative_file in "${required_files[@]}"; do
  [[ -f $CANDIDATE/$relative_file && ! -L $CANDIDATE/$relative_file ]] \
    || fail "required tested artifact is missing or unsafe: $relative_file"
done
[[ -d $CANDIDATE/apps/content-studio/.next/static \
    && ! -L $CANDIDATE/apps/content-studio/.next/static ]] \
  || fail "Content Studio static artifact is missing or unsafe"
[[ -d $CANDIDATE/packages/db/migrations && ! -L $CANDIDATE/packages/db/migrations ]] \
  || fail "database migrations are missing or unsafe"

mkdir -p -- "$STAGE"
cp -- "$CANDIDATE/package.json" "$CANDIDATE/pnpm-lock.yaml" \
  "$CANDIDATE/pnpm-workspace.yaml" "$STAGE/"
cp -- "$CANDIDATE/deploy/engine-commerce-compatibility.contract" \
  "$STAGE/.engine-commerce-compatibility-v1"

runtime_projects=(
  apps/api
  apps/worker
  packages/contracts
  packages/db
  packages/engine-client
  packages/payments
)
for relative_dir in "${runtime_projects[@]}"; do
  mkdir -p -- "$STAGE/$relative_dir"
  cp -- "$CANDIDATE/$relative_dir/package.json" "$STAGE/$relative_dir/package.json"
done

# Populate one shared production virtual store. `pnpm deploy` creates a private store per selected
# project and made the same release more than four times larger.
pnpm --dir "$STAGE" install \
  --prod \
  --offline \
  --ignore-scripts \
  --frozen-lockfile \
  --filter @claude-api/commercial-api... \
  --filter @claude-api/payment-worker... \
  --filter @claude-api/db...

for relative_dir in "${runtime_projects[@]}"; do
  [[ -d $CANDIDATE/$relative_dir/dist ]] || continue
  cp -a -- "$CANDIDATE/$relative_dir/dist" "$STAGE/$relative_dir/dist"
done
cp -a -- "$CANDIDATE/packages/db/migrations" "$STAGE/packages/db/migrations"

# Next's standalone trace contains only production files. Keep it isolated from the shared pnpm
# virtual store so identically named packages cannot overwrite one another.
mkdir -p -- "$STAGE/apps/content-studio/.next"
cp -- "$CANDIDATE/apps/content-studio/package.json" \
  "$STAGE/apps/content-studio/package.json"
cp -- "$CANDIDATE/apps/content-studio/.next/BUILD_ID" \
  "$STAGE/apps/content-studio/.next/BUILD_ID"
cp -a -- "$CANDIDATE/apps/content-studio/.next/standalone" \
  "$STAGE/apps/content-studio/.next/standalone"
STANDALONE_APP=$STAGE/apps/content-studio/.next/standalone/apps/content-studio
mkdir -p -- "$STANDALONE_APP/.next"
cp -a -- "$CANDIDATE/apps/content-studio/.next/static" "$STANDALONE_APP/.next/static"
if [[ -d $CANDIDATE/apps/content-studio/public \
      && ! -L $CANDIDATE/apps/content-studio/public ]]; then
  cp -a -- "$CANDIDATE/apps/content-studio/public" "$STANDALONE_APP/public"
fi
printf '1\n' >"$STAGE/.release-bundle-format"

# Reject escaping links and special files before this tree crosses the trusted-candidate boundary.
# Freezing here lets production reflink/copy it without another recursive chmod traversal.
node "$SCRIPT_DIR/release-tree-digest.mjs" "$STAGE" >/dev/null
chmod -R a-w "$STAGE"
mv -- "$STAGE" "$BUNDLE"
STAGE=
printf '%s\n' "$BUNDLE"
