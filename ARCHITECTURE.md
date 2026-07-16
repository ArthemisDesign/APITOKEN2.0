# Архитектура claude-api

Пул Claude-подписок как **прозрачный `/v1` API**. Один Cargo workspace, слоёная структура —
каждый слой знает только о нижних. Правила для агентов — `CLAUDE.md` (корневой и в каждом крейте).

## Поток запроса

```
Клиент (Anthropic SDK / curl)  ──POST /v1/messages (наш api-key)──►  claude-api
                                                                        │
  server::http (роутер) ── авторизует клиента, отдаёт fallback ──►  forward::forward
                                                                        │
  forward: инжект Claude Code identity + oauth-заголовки,               │
          pool::route → cache-first: пин сессии на «домашнюю» персону   │
          (cache-hit) / capacity-weighted placement / спилл; ретраи →   │
          pool::pick (наименее загруженная),                            │
          Bearer подписки + её прокси                                   ▼
                                                            api.anthropic.com
                                                                        │
  ответ (в т.ч. SSE) ◄──────────── байт-в-байт ──────────────────────  │
  при 429/5xx: pool::mark_cooling + следующая подписка (до начала стрима)
```

## Слои (направление зависимостей — только вниз)

```
┌────────────────────────────────────────────────────────────┐
│ server (bin claude-api)  — КОМПОЗИЦИЯ                       │
│   config(env→ProxyConfig) · http(роутер) · poller · main    │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ forward  — прозрачный форвардинг /v1/* + поллер лимитов     │
│   ProxyConfig · AppState · Clients · poll_sub · forward     │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ pool  — пул + ротация (in-memory)                          │
│   Pool · Live · route · pick · place_best · mark_* · …     │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ registry  — engine-owned PostgreSQL authority              │
│   reservations/outbox · capacity leases · epochs · CRUD    │
└────────────────────────────────────────────────────────────┘
```

## Зоны ответственности (куда класть код)

| Меняешь… | Крейт | Ветка-владелец |
|---|---|---|
| хранение/чтение подписок, схему БД | `registry` | `comp/registry` |
| выбор подписки, ротацию, cooling, состояние лимитов | `pool` | `comp/pool` |
| форвардинг, инжект identity, поллер, стрим | `forward` | `comp/forward` |
| env-конфиг, CLI, роутер, фоновые циклы, проводку | `server` | `comp/server` |
| покупку подписок и пополнение пула (Telegram-бот) | `tools/authbot` | `comp/authbot` |

**Пополнение пула (вне слоёв API).** `tools/authbot` — Python Telegram-бот: покупает подписки
(офферы → оплата USDT BEP-20 → выпуск 1-летнего setup-token + прокси) и регистрирует их в
реестр ЭТОГО проекта через CLI `claude-api sub add-file`. Стоит ПЕРЕД `registry` как производитель;
внутренности крейтов не трогает. Работает исключительно на пул этого проекта (свой bot-токен/env).

## Ключевые решения

- **Форвардинг, а не CLI.** Прокси шлёт сырой HTTP на api.anthropic.com на OAuth-токене подписки —
  поэтому для клиента это настоящий Anthropic API (в отличие от обёртки над `claude` CLI).
- **Identity-инжект** — цена работы на подписочном токене; вынесен в конфиг, тюнится без пересборки.
- **Ротация до стрима** — статус ответа проверяется до отдачи тела, поэтому переключение подписок
  при 429/5xx не рвёт клиентский стрим.
- **env только в server** — нижние слои чисто-функциональны и тестируемы без окружения.
- **PostgreSQL — durable authority.** Generated request IDs own exact reservation rows. Settlement
  first lands in a durable outbox, then atomically closes that exact reservation, updates the account,
  and inserts a charge unique on `(kind, request_id)`. SQLite is retained only as the guarded import
  source and rollback-era audit snapshot.
- **Fencing, not distributed hope.** Every engine process holds a monotonic PostgreSQL owner epoch;
  stale epochs cannot reserve money, persist pool state, or acquire capacity. Subscription admission
  is one transaction (cooldown/utilization/inflight validation + lease + increment). Polling uses one
  PostgreSQL lease-epoch leader; there is no Redlock path.
- **Proven overlap gate.** Real-PostgreSQL fault injection and a two-owner end-to-end test gate the
  blue/green path. PostgreSQL mode may overlap two engine slots because money, delivery, capacity,
  pool writes, and poller leadership are fenced. SQLite fallback still takes the OS singleton lock.

Детали конфигурации — `config.env.example` / `server.env.example`. Деплой —
`systemd/claude-api@.service` + `deploy/engine-bluegreen.sh` (legacy cutover unit remains one-time only).

## Коммерческий контур (отдельно от движка)

```text
future Next.js web → apps/api → whole-USD checkout_sessions → commerce PostgreSQL
                           └── Control API → Rust claude-api
payment provider → apps/api (verified webhook) → engine_credits outbox → apps/worker → Control API
engine charge ledger → apps/worker cursor → monthly B2C tier/job ────────────────────┘
```

`apps/api` владеет будущей browser-facing API-границей и приёмом подписанных вебхуков.
Пользователь вводит произвольное целое число USD строкой; каталог продуктов отсутствует.
Browser identity определяется только opaque server-side сессией; email/Google identities и
сессии живут в commerce PostgreSQL, подробности — `AUTHENTICATION.md`.
`apps/worker` забирает durable credit jobs из PostgreSQL через `FOR UPDATE SKIP LOCKED` и
идемпотентно вызывает `/admin/account/{id}/credit`. Общие схемы/репозитории/клиент движка находятся
в `packages/contracts`, `packages/db`, `packages/engine-client`. Коммерческие приложения не получают
engine DSN и не имеют прямого DB-кода; они общаются с движком только через Control API. Полная карта —
`COMMERCIAL_BACKEND.md`.
Dashboard routes read authoritative balances, ledger rows and per-key spend through the Control API.
Key creation returns the usable secret once; later revocation uses a stable non-secret engine `key_id`.
B2C/B2B pricing state lives in commerce PostgreSQL; the worker synchronizes its multiplier to the
engine through durable jobs. Full rules and tier thresholds are in `PRICING.md`.
