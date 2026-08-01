# @claude-api/devbot

Dev-бот Telegram: доставляет события жизненного цикла проекта (алерты Alertmanager,
вехи деплой-пайплайна из GitHub, ручные вмешательства из journald) в топики
forum-группы и отвечает на команды о состоянии систем. Дизайн и карта источников —
[docs/ops/DEVBOT.md](../../docs/ops/DEVBOT.md).

Plain TypeScript-сервис (Node 24, без NestJS и telegram-фреймворков): тонкий клиент
Bot API на встроенном `fetch` по образцу `crates/authbot/src/tg.rs`, state — один
JSON-файл, БД нет.

## Локальный запуск

```bash
cp .env.example .env          # заполнить DEVBOT_* значениями
set -a && . ./.env && set +a
pnpm build
node dist/main.js             # или: pnpm start
```

Без `DEVBOT_GITHUB_TOKEN` работает всё, кроме github-поллера и `/deploys`;
без `DEVBOT_ENGINE_*` команды `/pool` и `/settlement` отвечают «не настроено».

## Провижининг топиков

Группа должна быть forum-группой (topics включены), бот — админом с правом
управления топиками. Один раз:

```bash
DEVBOT_TELEGRAM_TOKEN=... DEVBOT_CHAT_ID=-100... node scripts/provision-topics.mjs
```

Скрипт создаёт 5 топиков и печатает готовые строки `DEVBOT_TOPIC_*` для env-файла.

## Проверки

```bash
pnpm --filter @claude-api/devbot build
pnpm --filter @claude-api/devbot typecheck
pnpm --filter @claude-api/devbot test
```

## Эндпоинты (bind 127.0.0.1:DEVBOT_PORT)

- `POST /alerts/{DEVBOT_AM_SECRET}` — webhook Alertmanager v4 (grouped notifications).
- `GET /health` — health-gate деплоя (`{"ok":true}`).
- `GET /metrics` — `devbot_heartbeat_timestamp_seconds`, `devbot_events_total{topic,kind}`,
  `devbot_telegram_send_failures_total`.
