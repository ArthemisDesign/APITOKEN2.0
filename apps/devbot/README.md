# @claude-api/devbot

Telegram dev bot: delivers project lifecycle events (Alertmanager alerts,
deploy pipeline milestones from GitHub, manual interventions from journald) and
incoming Chatwoot client messages into the topics of a forum group and answers
commands about system state. Design and source map —
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

The script creates the forum topics and prints ready-made `DEVBOT_TOPIC_*` lines for the env file.
A group that already has the original five topics can add only Support:

```bash
DEVBOT_PROVISION_ONLY=SUPPORT DEVBOT_TELEGRAM_TOKEN=... DEVBOT_CHAT_ID=-100... \
  node scripts/provision-topics.mjs
```

Chatwoot intake stays off until both `DEVBOT_TOPIC_SUPPORT` and `DEVBOT_CHATWOOT_SECRET` are set.
Webhook URL in Chatwoot: `https://support.apitoken.sale/hooks/devbot/{DEVBOT_CHATWOOT_SECRET}`,
event `message_created`. Paste Chatwoot's HMAC secret into `DEVBOT_CHATWOOT_HMAC_SECRET`.

## Checks

```bash
pnpm --filter @claude-api/devbot build
pnpm --filter @claude-api/devbot typecheck
pnpm --filter @claude-api/devbot test
```

## Endpoints (bind 127.0.0.1:DEVBOT_PORT)

- `POST /alerts/{DEVBOT_AM_SECRET}` — Alertmanager v4 webhook (grouped notifications).
- `POST /hooks/devbot/{DEVBOT_CHATWOOT_SECRET}` — Chatwoot `message_created` intake (incoming
  client messages only). Public Caddy path is the same on `support.apitoken.sale`.
- `GET /health` — deploy health gate (`{"ok":true}`).
- `GET /metrics` — `devbot_heartbeat_timestamp_seconds`, `devbot_events_total{topic,kind}`,
  `devbot_telegram_send_failures_total`, `devbot_last_webhook_seconds`,
  `devbot_last_chatwoot_seconds`.
