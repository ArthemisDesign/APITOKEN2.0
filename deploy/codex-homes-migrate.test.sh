#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d "${TMPDIR:-/tmp}/apitoken-codex-homes-migrate-test.XXXXXXXX")
trap 'rm -rf -- "$TEMP"' EXIT

fail() {
  printf '[codex-homes-migrate-test] ERROR: %s\n' "$*" >&2
  exit 1
}

make_home() {
  local home=$1
  mkdir -p -- "$home"
  printf '{}' > "$home/auth.json"
}

# --check / --apply require a discoverable engine binary and seal env.
export CODEX_LEGACY_SINGLE_HOME="$TEMP/legacy/home"
export CODEX_LEGACY_HOMES_DIR="$TEMP/legacy-homes"
export CODEX_ROSTER_DIR="$TEMP/roster"
export CODEX_SEAL_ENV="$TEMP/seal.env"
export ENGINE_BIN="$TEMP/engine/claude-api"

# Unknown arguments are rejected before any filesystem work.
if bash "$ROOT/deploy/codex-homes-migrate.sh" >/dev/null 2>&1; then
  fail 'missing mode argument was accepted'
fi
if bash "$ROOT/deploy/codex-homes-migrate.sh" --maybe >/dev/null 2>&1; then
  fail 'unknown mode argument was accepted'
fi

# --check fails closed without the engine binary and the seal env.
if bash "$ROOT/deploy/codex-homes-migrate.sh" --check >/dev/null 2>&1; then
  fail '--check passed without an engine binary'
fi
mkdir -p -- "$TEMP/engine"
printf '#!/usr/bin/env bash\n' > "$ENGINE_BIN"
chmod 0755 "$ENGINE_BIN"
if bash "$ROOT/deploy/codex-homes-migrate.sh" --check >/dev/null 2>&1; then
  fail '--check passed without a seal env'
fi
printf 'CODEX_SEAL_KEYS=test:0000000000000000000000000000000000000000000000000000000000000000\nCODEX_SEAL_ACTIVE_KID=test\n' \
  > "$CODEX_SEAL_ENV"

# Discovery: regular homes only; dot-staging and symlinks are skipped.
make_home "$CODEX_LEGACY_SINGLE_HOME"
make_home "$CODEX_LEGACY_HOMES_DIR/account-one"
make_home "$CODEX_LEGACY_HOMES_DIR/account-two"
mkdir -p -- "$CODEX_LEGACY_HOMES_DIR/.staging-hidden"
ln -s "$CODEX_LEGACY_HOMES_DIR/account-one" "$CODEX_LEGACY_HOMES_DIR/account-link"
out=$(bash "$ROOT/deploy/codex-homes-migrate.sh" --check) \
  || fail '--check failed on a valid fixture'
[[ $out == *'3 legacy home(s)'* ]] || fail "--check counted the wrong homes: $out"

# A home without auth.json is not migratable.
make_home "$CODEX_LEGACY_HOMES_DIR/broken" && rm -- "$CODEX_LEGACY_HOMES_DIR/broken/auth.json"
if bash "$ROOT/deploy/codex-homes-migrate.sh" --check >/dev/null 2>&1; then
  fail '--check accepted a home without auth.json'
fi
rmdir -- "$CODEX_LEGACY_HOMES_DIR/broken"

# --apply seals every home through the engine migrator and deletes it afterwards.
cat > "$ENGINE_BIN" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
home= roster= keys= kid= delete=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    codex-seal) ;;
    --home) home=$2; shift ;;
    --roster) roster=$2; shift ;;
    --keys) keys=$2; shift ;;
    --active-kid) kid=$2; shift ;;
    --delete-home) delete=1 ;;
    *) exit 2 ;;
  esac
  shift
done
[[ -n $home && -n $roster && -n $keys && -n $kid && $delete == 1 ]] || exit 2
id=$(basename -- "$home")
mkdir -p -- "$roster/credentials"
printf '{}' > "$roster/credentials/$id.json"
rm -rf -- "$home"
printf '%s\n' "$id"
EOF
chmod 0755 "$ENGINE_BIN"

bash "$ROOT/deploy/codex-homes-migrate.sh" --apply >/dev/null \
  || fail '--apply failed on a valid fixture'
[[ ! -e $CODEX_LEGACY_SINGLE_HOME ]] || fail '--apply kept the legacy single home'
[[ ! -e $CODEX_LEGACY_HOMES_DIR/account-one ]] || fail '--apply kept a sealed pool home'
[[ -f $CODEX_ROSTER_DIR/credentials/home.json ]] || fail '--apply produced no single-home profile'
[[ -f $CODEX_ROSTER_DIR/credentials/account-one.json ]] || fail '--apply produced no pool profile'
[[ -d $CODEX_LEGACY_HOMES_DIR/.staging-hidden ]] || fail '--apply touched a hidden staging dir'

printf '[codex-homes-migrate-test] ok\n'
