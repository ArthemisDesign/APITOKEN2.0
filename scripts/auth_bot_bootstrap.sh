#!/usr/bin/env bash
# auth_bot_bootstrap.sh — подготовить окружение auth_token_bot (Rust) на ГОЛОМ сервере.
# Нейросеть НЕ нужна. Сам бот — Rust-бинарь (repo CLAUDE_SETUP_TOKEN_BOT); выплаты USDT
# (BEP-20) встроены в бинарь (alloy), python/venv/web3 больше НЕ требуются.
#
# Скрипт делает ровно две вещи:
#   • ставит claude CLI — OAuth-утилита для `claude setup-token` (НЕ модель, без GPU/квоты),
#     нужна только для финального шага «передача доступа» (email→токен);
#   • расшифровывает env (auth_bot.env.gpg) для сервиса.
#
# Идемпотентно. Запускать от пользователя, под которым будет крутиться бот.
#   scripts/auth_bot_bootstrap.sh
# Перешифрованный ключ подтянется, если задан AGENTS_SECRETS_PASSPHRASE.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BOT="$ROOT/tools/auth_token_bot"
VENV="${AUTH_BOT_VENV:-$BOT/venv}"
CLAUDE_BIN="${CLAUDE_BIN:-$HOME/.local/bin/claude}"

say(){ printf '\n=== %s ===\n' "$*"; }

say "python3"
command -v python3 >/dev/null || { echo "нет python3 — поставь его (apt install python3 python3-venv)"; exit 1; }
python3 --version

say "claude CLI (OAuth-утилита для setup-token; это НЕ нейросеть)"
if [ -x "$CLAUDE_BIN" ] || command -v claude >/dev/null 2>&1; then
  echo "уже установлен: $(command -v claude 2>/dev/null || echo "$CLAUDE_BIN")"
else
  echo "ставлю claude CLI официальным скриптом…"
  curl -fsSL https://claude.ai/install.sh | bash
fi
( "$CLAUDE_BIN" --version 2>/dev/null || claude --version 2>/dev/null ) \
  || echo "⚠️ claude не в PATH — пропиши CLAUDE_BIN=$CLAUDE_BIN в env бота"

say "секреты (ВЕСЬ env зашифрован в auth_bot.env.gpg)"
ENC="$BOT/auth_bot.env.gpg"
if [ -f "$ENC" ] && [ -n "${AGENTS_SECRETS_PASSPHRASE:-}" ]; then
  umask 077
  OUT="${AUTH_BOT_ENV_OUT:-$BOT/auth_bot.env.decrypted}"
  "$ROOT/scripts/secrets.sh" decrypt "$OUT"
  echo "ВЕСЬ env расшифрован в $OUT (токен бота + admin + ключ кошелька)"
  echo "→ поставь его как env бота:"
  echo "   sudo cp $OUT /etc/agents/auth_bot.env && sudo chmod 600 /etc/agents/auth_bot.env && shred -u $OUT"
else
  echo "пропуск (нет $ENC или не задан AGENTS_SECRETS_PASSPHRASE)"
fi

say "ГОТОВО — нейросеть не использовалась и не нужна"
echo "Бот:           Rust-бинарь auth_token_bot_rs (собери из repo CLAUDE_SETUP_TOKEN_BOT:"
echo "               cd rust && cargo build --release → target/release/auth_token_bot)"
echo "systemd:       ExecStart=<путь>/auth_token_bot_rs (см. claude-auth-bot.service)"
echo "env бота:      AUTH_BOT_TOKEN, AUTH_BOT_ADMIN(=@username,<id>), AUTH_BOT_BSC_PKEY, CLAUDE_BIN=$CLAUDE_BIN"
echo "авто-установка CLI на старте бота (опц.): AUTH_BOT_AUTO_INSTALL_CLI=1"
