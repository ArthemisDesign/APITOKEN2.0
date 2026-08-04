# @claude-api/devbot

Telegram dev bot: delivers project lifecycle events (Alertmanager alerts,
deploy pipeline milestones from GitHub, manual interventions from journald) into the topics of a
forum group and answers commands about system state. Design and source map —
[docs/ops/DEVBOT.md](../../docs/ops/DEVBOT.md).

Plain TypeScript service (Node 24, no NestJS and no telegram frameworks): a thin
Bot API client on the built-in `fetch`, modeled after `crates/authbot/src/tg.rs`; state is a single
JSON file, no database.

## Local run

```bash
cp .env.example .env          # fill in the DEVBOT_* values
set -a && . ./.env && set +a
pnpm build
node dist/main.js             # or: pnpm start
```

Without `DEVBOT_GITHUB_TOKEN` everything works except the github poller and `/deploys`;
without `DEVBOT_ENGINE_*` the `/pool` and `/settlement` commands reply "not configured".

## Topic provisioning

The group must be a forum group (topics enabled), and the bot must be an admin with the right
to manage topics. One time only:

```bash
DEVBOT_TELEGRAM_TOKEN=... DEVBOT_CHAT_ID=-100... node scripts/provision-topics.mjs
```

The script creates 5 topics and prints ready-made `DEVBOT_TOPIC_*` lines for the env file.

## Checks

```bash
pnpm --filter @claude-api/devbot build
pnpm --filter @claude-api/devbot typecheck
pnpm --filter @claude-api/devbot test
```

## Endpoints (bind 127.0.0.1:DEVBOT_PORT)

- `POST /alerts/{DEVBOT_AM_SECRET}` — Alertmanager v4 webhook (grouped notifications).
- `GET /health` — deploy health gate (`{"ok":true}`).
- `GET /metrics` — `devbot_heartbeat_timestamp_seconds`, `devbot_events_total{topic,kind}`,
  `devbot_telegram_send_failures_total`.
