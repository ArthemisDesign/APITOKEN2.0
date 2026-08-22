#!/usr/bin/env bash
set -euo pipefail

# Create the log-only observe SSH account. Must run from apitoken-observe-install.service, not from
# the watchdog mount namespace: ProtectSystem=full makes /etc/shells and /etc/passwd read-only even
# after sudo, and ProtectHome=read-only cannot create /home/observe.

WRAPPER_SRC=/usr/local/lib/apitoken-watchdog/apitoken-observe.sh
WRAPPER=/usr/local/bin/apitoken-observe
HOME_DIR=/home/observe
SSH_DIR=$HOME_DIR/.ssh
KEYS=$SSH_DIR/authorized_keys
DEPLOY_KEYS=/home/deploy/.ssh/authorized_keys
SHELLS=/etc/shells

die() { printf 'observe-install: %s\n' "$*" >&2; exit 1; }

[[ ${EUID:-$(id -u)} -eq 0 ]] || die 'must run as root'
[[ -f $WRAPPER_SRC && ! -L $WRAPPER_SRC ]] || die 'wrapper source is missing'
[[ $(stat -c '%u:%a' -- "$WRAPPER_SRC") == 0:755 ]] \
  || die 'wrapper source must be root-owned mode 0755'
install -o root -g root -m 0755 "$WRAPPER_SRC" "$WRAPPER"

if [[ -e $SHELLS || -L $SHELLS ]]; then
  [[ -f $SHELLS && ! -L $SHELLS ]] || die "$SHELLS must be a regular file"
else
  install -o root -g root -m 0644 /dev/null "$SHELLS"
fi
if command -v add-shell >/dev/null; then
  add-shell "$WRAPPER"
else
  grep -qxF "$WRAPPER" "$SHELLS" || printf '%s\n' "$WRAPPER" >>"$SHELLS"
fi

# Ubuntu useradd --system does not create a user-private group.
if ! getent group observe >/dev/null; then
  groupadd --system observe
fi
if ! id observe >/dev/null 2>&1; then
  useradd --system --gid observe --create-home --home-dir "$HOME_DIR" --shell "$WRAPPER" \
    --comment 'apitoken log-only SSH' observe
else
  usermod -g observe observe
fi
usermod --shell "$WRAPPER" observe
if getent group systemd-journal >/dev/null; then
  usermod -a -G systemd-journal observe
fi
if getent group adm >/dev/null; then
  usermod -a -G adm observe
fi
if id -Gn observe | tr ' ' '\n' | grep -Fxq deploy; then
  gpasswd -d observe deploy >/dev/null \
    || printf 'observe-install: warning: could not remove observe from the deploy group\n' >&2
fi

install -d -o observe -g observe -m 0750 "$HOME_DIR"
install -d -o observe -g observe -m 0700 "$SSH_DIR"
[[ -d $HOME_DIR && ! -L $HOME_DIR ]] || die "$HOME_DIR must be a real directory"
[[ -d $SSH_DIR && ! -L $SSH_DIR ]] || die "$SSH_DIR must be a real directory"
tmp=$(mktemp)
{
  printf '%s\n' '# managed by install-observe.sh; ForceCommand is the observe wrapper'
  if [[ -f $DEPLOY_KEYS && ! -L $DEPLOY_KEYS ]]; then
    awk '
      match($0, /(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256|sk-ssh-ed25519@openssh.com|sk-ecdsa-sha2-nistp256@openssh.com) [A-Za-z0-9+\/=]+/) {
        printf "restrict,command=\"/usr/local/bin/apitoken-observe\" %s\n", substr($0, RSTART, RLENGTH)
      }
    ' "$DEPLOY_KEYS"
  fi
} >"$tmp"
install -o observe -g observe -m 0600 "$tmp" "$KEYS"
rm -f -- "$tmp"
printf 'observe-install: observe account is ready\n'
