#!/usr/bin/env bash
# Run Ubuntu host-installer proofs in a disposable privileged container.
# Fail closed when Docker is missing. Never run this on the production host
# as a substitute for the real installer transaction.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)

die() {
  printf 'host-image-gate: %s\n' "$*" >&2
  exit 1
}

command -v docker >/dev/null \
  || die 'docker is required for Ubuntu host-installer proofs'
docker info >/dev/null 2>&1 \
  || die 'docker is installed but not running'

command -v python3 >/dev/null || die 'python3 is required to name the container'
slug=$(python3 -c 'import hashlib, sys; print(hashlib.sha256(sys.argv[1].encode()).hexdigest()[:12])' "$ROOT")
name=apitoken-host-image-$slug
image=apitoken-host-image:$slug

cleanup() {
  docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

[[ -f $ROOT/deploy/host-image/Dockerfile ]] || die 'host-image Dockerfile is missing'
[[ -f $ROOT/deploy/host-image/prove-installers.sh ]] \
  || die 'host-image prove-installers.sh is missing'

docker build --pull=false -t "$image" \
  -f "$ROOT/deploy/host-image/Dockerfile" \
  "$ROOT/deploy/host-image"

docker rm -f "$name" >/dev/null 2>&1 || true
docker run --rm --name "$name" --privileged \
  -v "$ROOT:/src:ro" \
  "$image"
