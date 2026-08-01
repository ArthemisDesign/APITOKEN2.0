#!/usr/bin/env bash
set -euo pipefail

# Install the least-privilege sudo policy for the deployment pipeline.
#
# Replacing a working sudoers file is the single most dangerous edit on this host: a malformed or
# over-restrictive policy can lock the watchdog out of its own privileges with no way back except
# console access. This installer therefore:
#
#   1. validates the candidate with `visudo -c` BEFORE it is ever placed in /etc/sudoers.d;
#   2. keeps a timestamped rollback copy of whatever it replaces;
#   3. runs a live verification of every privilege the pipeline actually needs, as the deploy user;
#   4. restores the previous policy automatically if any of those checks fail.
#
# Run with --check to validate and report without changing anything.

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# Prefer the policy shipped alongside this script (repository checkout or the installed pair), so a
# script and its policy are never mismatched.
SOURCE=${APITOKEN_SUDOERS_SOURCE:-$SCRIPT_DIR/sudoers.d/95-apitoken-deploy}
TARGET=/etc/sudoers.d/95-apitoken-deploy
LEGACY=/etc/sudoers.d/90-deploy
BACKUP_ROOT=/root/sudoers-backups
CHECK_ONLY=0

log() { printf '[sudoers] %s\n' "$*"; }
warn() { printf '[sudoers] WARNING: %s\n' "$*" >&2; }
die() { printf '[sudoers] ERROR: %s\n' "$*" >&2; exit 1; }

[[ ${EUID:-$(id -u)} -eq 0 ]] || die "sudoers installation must run as root"
if [[ $# -gt 0 ]]; then
  [[ $# -eq 1 && $1 == --check ]] || die "usage: $0 [--check]"
  CHECK_ONLY=1
fi

command -v visudo >/dev/null || die "visudo is required"
[[ -f $SOURCE && ! -L $SOURCE ]] || die "sudoers source is missing: $SOURCE"
id deploy >/dev/null 2>&1 || die "deploy user is required"
id apitoken-ci >/dev/null 2>&1 || die "apitoken-ci user is required"

# Syntax-validate the candidate in a private temporary file first. visudo -c on a standalone file
# checks it in isolation, which is exactly what /etc/sudoers.d inclusion will do.
staging=$(mktemp /tmp/apitoken-sudoers.XXXXXX)
cleanup() { rm -f -- "$staging"; }
trap cleanup EXIT
install -o root -g root -m 0440 "$SOURCE" "$staging"
# `visudo -c` exits 0 on warnings. An unused Cmnd_Alias is one of them, and it means a privilege the
# policy intends to grant is silently not granted — exactly the failure that locks the pipeline out.
# Treat any warning as fatal.
validation=$(visudo -c -f "$staging" 2>&1) || die "candidate sudo policy failed syntax validation"
if grep -qi 'warning' <<<"$validation"; then
  printf '%s\n' "$validation" >&2
  die "candidate sudo policy produced validation warnings; refusing to install"
fi
log "candidate policy passed visudo syntax validation with no warnings"

if (( CHECK_ONLY == 1 )); then
  if [[ -f $TARGET ]] && cmp -s "$staging" "$TARGET"; then
    log "installed policy already matches the candidate"
  else
    log "installed policy DIFFERS from the candidate (run without --check to apply)"
  fi
  if [[ -f $LEGACY ]]; then
    warn "legacy unrestricted policy is still present: $LEGACY"
  fi
  exit 0
fi

timestamp=$(date -u +%Y%m%dT%H%M%SZ)
backup_dir=$BACKUP_ROOT/$timestamp
install -d -o root -g root -m 0700 "$BACKUP_ROOT" "$backup_dir"
for existing in "$TARGET" "$LEGACY"; do
  [[ -f $existing ]] || continue
  install -o root -g root -m 0440 "$existing" "$backup_dir/${existing##*/}"
done
log "saved rollback copies under $backup_dir"

restore() {
  warn "restoring the previous sudo policy from $backup_dir"
  rm -f -- "$TARGET"
  for saved in "$backup_dir"/*; do
    [[ -f $saved ]] || continue
    install -o root -g root -m 0440 "$saved" "/etc/sudoers.d/${saved##*/}"
  done
  # A restored policy that does not parse is worse than none; verify before leaving it in place.
  visudo -c >/dev/null || warn "RESTORED POLICY DOES NOT VALIDATE; fix /etc/sudoers.d from console"
}

install -o root -g root -m 0440 "$staging" "$TARGET"
if ! visudo -c >/dev/null; then
  restore
  die "combined sudo policy failed validation after install; previous policy restored"
fi

# The legacy unrestricted grant must be removed, or last-match ordering is irrelevant and the whole
# change is cosmetic. It is already backed up above.
if [[ -f $LEGACY ]]; then
  rm -f -- "$LEGACY"
  log "removed legacy unrestricted policy $LEGACY"
  if ! visudo -c >/dev/null; then
    restore
    die "sudo policy failed validation after removing the legacy grant; previous policy restored"
  fi
fi

# Live verification. Each of these is an operation the pipeline genuinely performs; if any is no
# longer permitted, the deployment system is broken and the change must be reverted immediately.
verify_failures=0
run_as_deploy() { sudo -n -u deploy "$@"; }

# Privilege resolution must be evaluated as the deploy user, not root. `sudo -l -- <cmd>` reports
# whether the command would be allowed without running it.
require_permitted() {
  local description=$1
  shift
  # sudo can only match a command spec against a path that exists. A helper shipped by a candidate
  # that has not been installed on this host yet would therefore read as "denied" even though the
  # policy grants it. Skip those rather than failing the install, and say so explicitly.
  if [[ $1 == /* && ! -e $1 ]]; then
    log "skipped (not installed on this host yet): $description"
    return 0
  fi
  if run_as_deploy sudo -n -l -- "$@" >/dev/null 2>&1; then
    log "permitted: $description"
  else
    warn "DENIED but required: $description ($*)"
    verify_failures=$((verify_failures + 1))
  fi
}
require_denied() {
  local description=$1
  shift
  if run_as_deploy sudo -n -l -- "$@" >/dev/null 2>&1; then
    warn "PERMITTED but must be denied: $description ($*)"
    verify_failures=$((verify_failures + 1))
  else
    log "correctly denied: $description"
  fi
}

sample_sha=0000000000000000000000000000000000000000
require_permitted 'daemon-reload' /usr/bin/systemctl daemon-reload
require_permitted 'Anthropic slot start' /usr/bin/systemctl start claude-api-anthropic@8787.service
require_permitted 'Anthropic slot stop' /usr/bin/systemctl stop claude-api-anthropic@8788.service
require_permitted 'combined bridge slot stop' /usr/bin/systemctl stop claude-api@8788.service
require_permitted 'combined bridge restart' /usr/bin/systemctl restart claude-api.service
require_permitted 'combined bridge drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api.service
require_permitted 'engine drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-anthropic@8787.service
require_permitted 'OpenAI provider restart' /usr/bin/systemctl restart claude-api-openai.service
require_permitted 'OpenAI provider stop' /usr/bin/systemctl stop claude-api-openai.service
require_permitted 'OpenAI provider enable' /usr/bin/systemctl enable claude-api-openai.service
require_permitted 'OpenAI provider drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-openai.service
require_permitted 'OpenAI target start' /usr/bin/systemctl start claude-api-openai@8797.service
require_permitted 'OpenAI old-slot stop' /usr/bin/systemctl stop claude-api-openai@8793.service
require_permitted 'OpenAI old-slot async stop' \
  /usr/bin/systemctl --no-block stop claude-api-openai@8793.service
require_permitted 'OpenAI target enable' /usr/bin/systemctl enable claude-api-openai@8797.service
require_permitted 'OpenAI reverse target start' /usr/bin/systemctl start claude-api-openai@8793.service
require_permitted 'OpenAI reverse old-slot stop' /usr/bin/systemctl stop claude-api-openai@8797.service
require_permitted 'OpenAI reverse old-slot async stop' \
  /usr/bin/systemctl --no-block stop claude-api-openai@8797.service
require_permitted 'OpenAI slot drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-openai@8793.service
require_permitted 'legacy Codex home migration' \
  /usr/local/lib/apitoken-watchdog/controller/codex-homes-migrate.sh --apply
require_permitted 'Gemini provider restart' /usr/bin/systemctl restart claude-api-gemini.service
require_permitted 'Gemini provider stop' /usr/bin/systemctl stop claude-api-gemini.service
require_permitted 'Gemini provider enable' /usr/bin/systemctl enable claude-api-gemini.service
require_permitted 'Gemini provider drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-gemini.service
require_permitted 'Gemini target start' /usr/bin/systemctl start claude-api-gemini@8799.service
require_permitted 'Gemini old-slot stop' /usr/bin/systemctl stop claude-api-gemini@8795.service
require_permitted 'Gemini old-slot async stop' \
  /usr/bin/systemctl --no-block stop claude-api-gemini@8795.service
require_permitted 'Gemini target enable' /usr/bin/systemctl enable claude-api-gemini@8799.service
require_permitted 'Gemini reverse target start' /usr/bin/systemctl start claude-api-gemini@8795.service
require_permitted 'Gemini reverse old-slot stop' /usr/bin/systemctl stop claude-api-gemini@8799.service
require_permitted 'Gemini reverse old-slot async stop' \
  /usr/bin/systemctl --no-block stop claude-api-gemini@8799.service
require_permitted 'Gemini slot drain signal' \
  /usr/bin/systemctl kill --kill-whom=main -s SIGUSR1 claude-api-gemini@8795.service
require_permitted 'commerce slot start' /usr/bin/systemctl start apitoken-api@3000.service
require_permitted 'worker restart' /usr/bin/systemctl restart apitoken-worker.service
require_permitted 'content studio restart' /usr/bin/systemctl restart apitoken-content-studio.service
require_permitted 'content studio enable' /usr/bin/systemctl enable apitoken-content-studio.service
require_permitted 'unit introspection' /usr/bin/systemctl show apitoken-api@3000.service
require_permitted 'backup runner' /usr/local/lib/apitoken-watchdog/watchdog-backup.sh "$sample_sha"
require_permitted 'migration runner' /usr/local/lib/apitoken-watchdog/watchdog-migrate.sh "$sample_sha"
require_permitted 'engine schema migration runner' \
  /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh "$sample_sha"
require_permitted 'engine schema migration helper probe' \
  /usr/bin/test -x /usr/local/lib/apitoken-watchdog/controller/engine-migrate.sh
require_permitted 'retention helper' /usr/local/lib/apitoken-watchdog/watchdog-retention.sh 10
require_permitted 'infrastructure runner' /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh "$sample_sha"
require_permitted 'controller-only infrastructure runner' \
  /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh "$sample_sha" --controller-only
require_permitted 'Caddy-only infrastructure runner' \
  /usr/local/lib/apitoken-watchdog/watchdog-infrastructure.sh "$sample_sha" --caddy-only
require_permitted 'GitHub reporting bridge' /usr/local/lib/apitoken-watchdog/watchdog-github commit-status
require_permitted 'GitHub candidate queue bridge' /usr/local/lib/apitoken-watchdog/watchdog-github validation-next 2
require_permitted 'test database helper' /usr/local/lib/apitoken-watchdog/watchdog-test-db start 0
require_permitted 'parallel test database slot' /usr/local/lib/apitoken-watchdog/watchdog-test-db start 1
require_permitted 'candidate ownership' /usr/bin/chown -R root:root -- "/var/lib/apitoken/watchdog/candidates/$sample_sha"
require_permitted 'candidate removal' /usr/bin/rm -rf --one-file-system -- "/var/lib/apitoken/watchdog/candidates/$sample_sha"
require_permitted 'engine release removal' /usr/bin/rm -rf --one-file-system -- "/srv/claude-api/releases/$sample_sha"
require_permitted 'commerce release removal' /usr/bin/rm -rf --one-file-system -- "/opt/apitoken/releases/$sample_sha"
require_permitted 'caddy validation' /usr/bin/caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile
require_permitted 'OpenAI unit probe' /usr/bin/test -f /etc/systemd/system/claude-api-openai.service
require_permitted 'OpenAI slot-template probe' \
  /usr/bin/test -f /etc/systemd/system/claude-api-openai@.service
require_permitted 'Gemini unit probe' /usr/bin/test -f /etc/systemd/system/claude-api-gemini.service
require_permitted 'Gemini slot-template probe' \
  /usr/bin/test -f /etc/systemd/system/claude-api-gemini@.service
require_permitted 'Anthropic unit probe' \
  /usr/bin/test -f /etc/systemd/system/claude-api-anthropic@.service
# Operator tooling must keep working: `apitoken-watchdog status|run|retry|logs`.
require_permitted 'operator status command' /usr/local/bin/apitoken-watchdog status
require_permitted 'operator run command' /usr/local/bin/apitoken-watchdog run
require_permitted 'operator logs command' /usr/local/bin/apitoken-watchdog logs
require_permitted 'operator retry command' /usr/local/bin/apitoken-watchdog retry "$sample_sha"
require_permitted 'operator poll trigger' /usr/bin/systemctl start apitoken-deploy-watchdog.service
require_permitted 'deployment journal' /usr/bin/journalctl -u apitoken-deploy-watchdog.service -n 250 --no-pager

# The privileges that must NOT be reachable. These are the reason the policy exists.
require_denied 'reading the GitHub reporting credential' /usr/bin/cat /etc/apitoken/github-watchdog.env
require_denied 'reading the commerce application secrets' /usr/bin/cat /etc/apitoken/api.env
require_denied 'arbitrary root shell' /bin/bash
require_denied 'switching to root' /usr/bin/su -
require_denied 'replacing a fixed controller' /usr/bin/install -m 0755 /tmp/x /usr/local/lib/apitoken-watchdog/watchdog.sh
require_denied 'stopping PostgreSQL' /usr/bin/systemctl stop apitoken-postgres.service
require_denied 'arbitrary file removal' /usr/bin/rm -rf /etc

if (( verify_failures > 0 )); then
  restore
  die "$verify_failures privilege check(s) failed; previous sudo policy restored"
fi

# The candidate test account is a member of the deploy group only for historical reasons. That
# membership lets candidate-derived test code write deploy-group-writable files in the source
# repository, which undermines the isolation the pipeline depends on. Remove it and tighten the
# repository's group-write bits. The CI account keeps its own home and the traverse-only access it
# actually needs; nothing in the test gate requires deploy group membership.
if id -Gn apitoken-ci | tr ' ' '\n' | grep -Fxq deploy; then
  gpasswd -d apitoken-ci deploy >/dev/null
  log 'removed apitoken-ci from the deploy group'
fi
if [[ -d /opt/apitoken/repo ]]; then
  # Tracked files must not be group-writable: the repository is a fetch-only controller source.
  find /opt/apitoken/repo -maxdepth 1 -group deploy -perm -g+w \
    -exec chmod g-w -- {} + 2>/dev/null || true
  log 'removed group-write bits from the top level of the deployment checkout'
fi

# Install this installer at its fixed root-owned path. The policy permits `deploy` to run exactly
# that path, which is what keeps the change reversible: without it, removing the unrestricted grant
# would also remove the ability to repair or update the policy without console access.
#
# Skip each copy when the source already IS the destination. Running the installed copy is the
# documented recovery path, and `install` treats same-file as an error — which would make the very
# command an operator reaches for during recovery exit non-zero.
INSTALLED_SELF=/usr/local/lib/apitoken-watchdog/install-sudoers.sh
INSTALLED_POLICY=/usr/local/lib/apitoken-watchdog/sudoers.d/95-apitoken-deploy
if [[ -d /usr/local/lib/apitoken-watchdog ]]; then
  self=$(readlink -f -- "${BASH_SOURCE[0]}")
  if [[ $self != "$(readlink -f -- "$INSTALLED_SELF" 2>/dev/null)" ]]; then
    install -o root -g root -m 0755 "$self" "$INSTALLED_SELF"
  fi
  install -d -o root -g root -m 0755 /usr/local/lib/apitoken-watchdog/sudoers.d
  if [[ $(readlink -f -- "$SOURCE") != "$(readlink -f -- "$INSTALLED_POLICY" 2>/dev/null)" ]]; then
    install -o root -g root -m 0644 "$SOURCE" "$INSTALLED_POLICY"
  fi
  log "policy installer and its source are current at their fixed root-owned paths"
fi

log "least-privilege sudo policy installed and verified"
