#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ignore_script="$script_dir/vercel-ignore-build.sh"
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

export GIT_AUTHOR_NAME='Vercel ignore test'
export GIT_AUTHOR_EMAIL='vercel-ignore@example.invalid'
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

assert_status() {
  local expected=$1
  local sha=$2
  local cwd=$3
  local output_file="$tmp_dir/output"
  local actual

  set +e
  (cd "$cwd" && VERCEL_GIT_PREVIOUS_SHA="$sha" bash "$ignore_script") >"$output_file" 2>&1
  actual=$?
  set -e

  if [[ "$actual" -ne "$expected" ]]; then
    printf 'expected exit %s, got %s for SHA %s\n' "$expected" "$actual" "$sha" >&2
    cat "$output_file" >&2
    exit 1
  fi
}

remote="$tmp_dir/origin.git"
seed="$tmp_dir/seed"
git init --bare --initial-branch=master "$remote" >/dev/null
git init --initial-branch=master "$seed" >/dev/null
mkdir -p "$seed/apps/web" "$seed/docs"
printf '{"name":"fixture"}\n' >"$seed/package.json"
printf 'lockfileVersion: 9\n' >"$seed/pnpm-lock.yaml"
printf 'packages: [apps/*]\n' >"$seed/pnpm-workspace.yaml"
printf '24\n' >"$seed/.node-version"
printf 'frontend\n' >"$seed/apps/web/page.txt"
printf 'docs v1\n' >"$seed/docs/readme.md"
git -C "$seed" add package.json pnpm-lock.yaml pnpm-workspace.yaml .node-version apps/web/page.txt docs/readme.md
git -C "$seed" commit -m 'base' >/dev/null
base_sha=$(git -C "$seed" rev-parse HEAD)
git -C "$seed" remote add origin "$remote"
git -C "$seed" push -u origin master >/dev/null 2>&1

printf 'docs v2\n' >"$seed/docs/readme.md"
git -C "$seed" add docs/readme.md
git -C "$seed" commit -m 'docs only' >/dev/null
git -C "$seed" push >/dev/null 2>&1

# A shallow Vercel-style clone does not contain the previous deployment commit.
# The script must fetch it and prove that an unrelated change can be skipped.
shallow_unchanged="$tmp_dir/shallow-unchanged"
git clone --quiet --depth=1 "file://$remote" "$shallow_unchanged"
if git -C "$shallow_unchanged" cat-file -e "${base_sha}^{commit}" 2>/dev/null; then
  echo 'fixture error: base commit unexpectedly exists in shallow clone' >&2
  exit 1
fi
assert_status 0 "$base_sha" "$shallow_unchanged/apps/web"
git -C "$shallow_unchanged" cat-file -e "${base_sha}^{commit}"

# A watched frontend change must request a build even when the base must be fetched.
printf 'frontend v2\n' >"$seed/apps/web/page.txt"
git -C "$seed" add apps/web/page.txt
git -C "$seed" commit -m 'frontend change' >/dev/null
git -C "$seed" push >/dev/null 2>&1
shallow_changed="$tmp_dir/shallow-changed"
git clone --quiet --depth=1 "file://$remote" "$shallow_changed"
assert_status 1 "$base_sha" "$shallow_changed/apps/web"

# Missing, malformed, and unfetchable bases all fail closed to a build request;
# none may leak Git's exit 128 and turn the ignored step into a deployment error.
assert_status 1 '' "$shallow_changed/apps/web"
assert_status 1 'not-a-commit' "$shallow_changed/apps/web"
assert_status 1 'ffffffffffffffffffffffffffffffffffffffff' "$shallow_changed/apps/web"

printf 'vercel ignore-build tests passed\n'
