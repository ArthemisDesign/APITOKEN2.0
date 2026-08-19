#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT

fixture=$TEMP/fixture
stubs=$TEMP/stubs
mkdir -p "$fixture/deploy" "$stubs"
cp "$ROOT/deploy/watchdog-lib.sh" "$fixture/deploy/watchdog-lib.sh"
sed \
  -e 's#^BACKUP_ROOT=.*#BACKUP_ROOT=${WATCHDOG_BACKUP_TEST_ROOT:?}#' \
  -e 's#^\[\[ ${EUID:.*validated deployment backup must run as root"#:#' \
  "$ROOT/deploy/watchdog-backup.sh" >"$fixture/deploy/watchdog-backup.sh"
chmod +x "$fixture/deploy/watchdog-backup.sh"

cat >"$stubs/install" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
target=
while (($#)); do
  case "$1" in
    -d) ;;
    -o|-g|-m) shift ;;
    *) target=$1 ;;
  esac
  shift
done
[[ -n $target ]]
mkdir -p "$target"
STUB

cat >"$stubs/cp" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
args=()
for arg in "$@"; do
  case "$arg" in --reflink=auto|--) ;; *) args+=("$arg") ;; esac
done
/bin/cp "${args[@]}"
STUB

cat >"$stubs/docker" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
case " $* " in
  *' psql '*)
    if [[ $WATCHDOG_BACKUP_TEST_MODE == joined-stale && $* == *"datname='claude_engine'"* ]]; then
      printf '1\n'
    fi
    ;;
  *' pg_restore '*)
    cat >/dev/null
    count=$(cat "$WATCHDOG_BACKUP_TEST_RESTORE_COUNT" 2>/dev/null || printf 0)
    printf '%s\n' "$((count + 1))" >"$WATCHDOG_BACKUP_TEST_RESTORE_COUNT"
    ;;
  *) printf 'unexpected docker command: %s\n' "$*" >&2; exit 2 ;;
esac
STUB

cat >"$stubs/stat" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
file=${@: -1}
perl -e 'print((stat shift)[9], "\n")' "$file"
STUB

cat >"$stubs/systemctl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  reset-failed) ;;
  show)
    case " $* " in
      *' ActiveState '*)
        count=$(cat "$WATCHDOG_BACKUP_TEST_START_COUNT" 2>/dev/null || printf 0)
        if [[ $WATCHDOG_BACKUP_TEST_MODE == joined-stale && $count == 0 ]]; then
          printf 'activating\n'
        else
          printf 'inactive\n'
        fi
        ;;
      *' Result '*) printf 'success\n' ;;
      *) printf 'unexpected systemctl show: %s\n' "$*" >&2; exit 2 ;;
    esac
    ;;
  start)
    count=$(cat "$WATCHDOG_BACKUP_TEST_START_COUNT" 2>/dev/null || printf 0)
    count=$((count + 1))
    printf '%s\n' "$count" >"$WATCHDOG_BACKUP_TEST_START_COUNT"
    case "$WATCHDOG_BACKUP_TEST_MODE:$count" in
      fresh:1) touch "$WATCHDOG_BACKUP_TEST_ROOT/commerce.dump" ;;
      joined-stale:1|joined-stale:2)
        touch "$WATCHDOG_BACKUP_TEST_ROOT/commerce.dump" \
          "$WATCHDOG_BACKUP_TEST_ROOT/claude_engine.dump"
        ;;
    esac
    ;;
  *) printf 'unexpected systemctl command: %s\n' "$*" >&2; exit 2 ;;
esac
STUB
chmod +x "$stubs/cp" "$stubs/install" "$stubs/docker" "$stubs/stat" "$stubs/systemctl"

run_case() {
  local mode=$1 sha=$2 expected_starts=$3 expected_restores=$4 backup_root
  backup_root=$TEMP/$mode
  mkdir -p "$backup_root"
  printf 'archive\n' >"$backup_root/commerce.dump"
  touch -t 200001010000 "$backup_root/commerce.dump"
  if [[ $mode == joined-stale ]]; then
    printf 'archive\n' >"$backup_root/claude_engine.dump"
    touch -t 200001010000 "$backup_root/claude_engine.dump"
  fi
  export WATCHDOG_BACKUP_TEST_MODE=$mode
  export WATCHDOG_BACKUP_TEST_ROOT=$backup_root
  export WATCHDOG_BACKUP_TEST_START_COUNT=$backup_root/start-count
  export WATCHDOG_BACKUP_TEST_RESTORE_COUNT=$backup_root/restore-count
  PATH="$stubs:$PATH" "$fixture/deploy/watchdog-backup.sh" "$sha" \
    >"$backup_root/stdout" 2>"$backup_root/stderr"
  [[ $(<"$backup_root/start-count") == "$expected_starts" ]]
  [[ $(<"$backup_root/restore-count") == "$expected_restores" ]]
  [[ -f $backup_root/commerce.pre-deploy-$sha.dump ]]
  [[ -f $backup_root/.pre-deploy-$sha.complete ]]
}

run_case fresh 1111111111111111111111111111111111111111 1 1
run_case joined-stale 2222222222222222222222222222222222222222 2 2
grep -Fq 'backup invocation predates the deployment boundary' "$TEMP/joined-stale/stdout"

failed_root=$TEMP/always-stale
mkdir -p "$failed_root"
printf 'archive\n' >"$failed_root/commerce.dump"
touch -t 200001010000 "$failed_root/commerce.dump"
export WATCHDOG_BACKUP_TEST_MODE=always-stale
export WATCHDOG_BACKUP_TEST_ROOT=$failed_root
export WATCHDOG_BACKUP_TEST_START_COUNT=$failed_root/start-count
export WATCHDOG_BACKUP_TEST_RESTORE_COUNT=$failed_root/restore-count
if PATH="$stubs:$PATH" "$fixture/deploy/watchdog-backup.sh" \
    3333333333333333333333333333333333333333 >"$failed_root/stdout" 2>"$failed_root/stderr"; then
  printf 'permanently stale backup set passed\n' >&2
  exit 1
fi
[[ $(<"$failed_root/start-count") == 2 ]]
[[ ! -e $failed_root/restore-count ]]
[[ ! -e $failed_root/commerce.pre-deploy-3333333333333333333333333333333333333333.dump ]]
[[ ! -e $failed_root/.pre-deploy-3333333333333333333333333333333333333333.complete ]]
grep -Fq 'did not produce a complete fresh database set after two runs' "$failed_root/stderr"

printf 'watchdog backup regression suite passed\n'
