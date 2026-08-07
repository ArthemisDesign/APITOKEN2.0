#!/usr/bin/env bash
# Vercel clones only a short history. Fetch the last deployed commit when it is
# outside that window, and skip only after proving the watched inputs unchanged.
set -u

previous_sha="${VERCEL_GIT_PREVIOUS_SHA:-}"

build() {
  printf 'Vercel build required: %s\n' "$1" >&2
  exit 1
}

if [[ -z "$previous_sha" ]]; then
  build "no previous deployment SHA is available"
fi

if [[ ! "$previous_sha" =~ ^[0-9a-fA-F]{40}$ ]]; then
  build "the previous deployment SHA is invalid"
fi

if ! git cat-file -e "${previous_sha}^{commit}" 2>/dev/null; then
  printf 'Previous deployment commit is outside the clone; fetching %s\n' "$previous_sha" >&2
  if ! git fetch --quiet --no-tags --depth=1 origin "$previous_sha"; then
    build "the previous deployment commit could not be fetched"
  fi
fi

if ! git cat-file -e "${previous_sha}^{commit}" 2>/dev/null; then
  build "the fetched previous deployment object is not a commit"
fi

git diff --quiet "$previous_sha" HEAD -- \
  . \
  ../../pnpm-lock.yaml \
  ../../pnpm-workspace.yaml \
  ../../.node-version \
  ../../package.json
diff_status=$?

case "$diff_status" in
  0)
    printf 'Skipping Vercel build: frontend inputs are unchanged\n'
    exit 0
    ;;
  1)
    build "frontend inputs changed"
    ;;
  *)
    build "Git could not compare frontend inputs (exit ${diff_status})"
    ;;
esac
