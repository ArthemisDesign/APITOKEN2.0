# CLAUDE.md — системные правила проекта claude-api

> Это обязательные инструкции для ЛЮБОГО агента, работающего в этом репозитории.
> Они важнее удобства и привычек: **соблюдай архитектуру и модель веток ниже всегда.**
> В каждом крейте есть свой вложенный `crates/<name>/CLAUDE.md` с локальными границами —
> Claude Code читает их автоматически, когда работает в подкаталоге.

## Что это

Пул обычных Claude-подписок (Max/Pro) отдаётся по сети как **API, неотличимый от
`api.anthropic.com`**. Клиент наводит любой Anthropic-совместимый инструмент на наш сервер —
запрос уходит на квоте подписки из пула, с ротацией по лимитам. Полное описание — `README.md`,
карта модулей — `ARCHITECTURE.md`, модель веток — `BRANCHES.md`.

## Commercial workspace (TypeScript, отдельный bounded context)

В этом же репозитории находится коммерческий pnpm-workspace: `apps/api`, `apps/worker` и общие
`packages/*`. Он отвечает за будущих пользователей, платежи, вебхуки и связь user→engine account.
Он **не входит** в Rust-цепочку `registry ← pool ← forward ← server` и не импортирует Rust-крейты.

- Коммерческий код не открывает engine PostgreSQL/SQLite и не пишет баланс напрямую.
- Единственная граница коммерция→движок — HTTP Control API из `CONTROL_API.md`.
- `apps/api` и `apps/worker` независимо деплоятся; общую логику кладём в `packages/*`.
- Коммерческая PostgreSQL хранит людей/платежи/доставку событий, но НЕ авторитетный live-баланс.
- CONTROL_KEY существует только в server-side env; браузеру, ответам и логам его не отдавать.
- Суммы провайдера и движка — только integer (`bigint`/decimal string), без JavaScript `number`.
- Пополнение — произвольное целое число USD, введённое пользователем строкой цифр. Каталога
  фиксированных продуктов нет; точки, дроби, float, знаки и ведущие нули запрещены.
- Browser API никогда не принимает `user_id` как доказательство личности. Владельца берём только
  из проверенной server-side сессии; приватные SQL-запросы дополнительно фильтруют по `user_id`.
- Полный клиентский `sk-pool-…` возвращается браузеру только при выпуске и не хранится в commerce
  PostgreSQL; листинг/отзыв используют маску и не-секретный engine `key_id`.
- Password users never receive a session or engine account before email verification. Google/GitHub
  may bypass local verification only with a provider-verified email; identities key on provider subject.
- Auth tokens are stored hashed. The email outbox may contain only AES-GCM-encrypted raw tokens;
  neither tokens nor verification/reset URLs may be logged.
- Публичный production API коммерческого слоя: `https://backend.apitoken.sale`; клиентский домен:
  `https://apitoken.sale`.
- B2C pricing derives only from idempotently consumed engine charge-ledger rows. Tier/month state and
  B2B invite/manual pricing live in commerce PostgreSQL; engine multiplier changes use durable jobs.

Локальная карта и запуск — `COMMERCIAL_BACKEND.md`. Проверка: `pnpm build && pnpm typecheck && pnpm test`.

## Архитектура — слои (НЕ нарушать направление зависимостей)

```
registry  ←  pool  ←  forward  ←  server(bin)
```

| Крейт | Отвечает за | МОЖЕТ зависеть от | НЕ ДЕЛАЕТ |
|---|---|---|---|
| `crates/registry` | engine PostgreSQL authority + SQLite importer | postgres, rusqlite, anyhow | HTTP, env, логика пула |
| `crates/pool` | пул + ротация (in-memory) | registry | сеть, HTTP, БД, env |
| `crates/forward` | прозрачный форвардинг /v1/*, поллер лимитов | pool, registry, axum, reqwest | чтение env, CLI, управляющие роуты |
| `crates/server` | КОМПОЗИЦИЯ: env-конфиг, CLI, роутер, фоновые циклы | forward, pool, registry | бизнес-логику форвардинга (она в forward) |

**Пополнение пула — `tools/authbot` (ВНЕ слоёв API).** Отдельный компонент-ПРОИЗВОДИТЕЛЬ
подписок: Telegram-бот (Python) покупает подписки и кладёт их в реестр. Он не участвует в
слоях `registry←…←server`, а стоит ПЕРЕД реестром: пишет в него ТОЛЬКО через CLI
(`claude-api sub add-file …`), не трогая внутренности крейтов. Владелец-ветка `comp/authbot`,
локальные правила — `tools/authbot/CLAUDE.md`.

**Инварианты (проверяй перед коммитом):**
1. **Прозрачность.** Для клиента протокол = чистый Anthropic API (тело/ответ/стрим/ошибки).
   Единственное, что делаем под капотом — инжект Claude Code identity + oauth-заголовки
   (иначе токен подписки не пускают). Не ломать эту прозрачность.
2. **Направление зависимостей** строго по таблице. registry/pool — без сети и без HTTP.
3. **env читается ТОЛЬКО** в `crates/server/src/config.rs`. Ниже по стеку — принимают готовый
   конфиг (`forward::ProxyConfig`), а не лезут в окружение.
4. **Секреты не коммитим:** токены, `subscriptions.db`, прокси с паролями, `*.env`, `target/`
   (см. `.gitignore`). В коде и логах не печатать токены.
5. **Куда класть новый код** — по зоне ответственности из таблицы. Меняешь работу с БД →
   `registry`; выбор/ротацию → `pool`; транспорт форвардинга → `forward`; проводку/CLI/env →
   `server`. Если тянет добавить сеть в pool или env в forward — это сигнал, что слой выбран не тот.

## Модель веток (trunk + ветка-владелец на компонент)

Подробно — `BRANCHES.md`. Кратко:

- **`master`** — интеграция, ВСЕГДА собирается (`cargo build` зелёный). Прямые коммиты — только
  для кросс-компонентной проводки/доков/релиза.
- **`comp/registry`, `comp/pool`, `comp/forward`, `comp/server`** — долгоживущие ветки-владельцы.
  Работа над компонентом идёт в его ветке и трогает преимущественно свой крейт. На каждой — свой
  `BRANCH.md` (назначение, границы, как тестировать).
- Правило: **изменение компонента делай на его `comp/*`-ветке**, затем merge в `master`.
  Кросс-компонентную задачу разбивай по владельцам или веди на `master` с явным обоснованием.
- Перед началом: `git branch` — пойми, где ты. Ветка сама себя объясняет через `BRANCH.md`.

## Как собрать/проверить

```bash
cargo build                      # весь workspace (должен быть зелёным до коммита)
cargo build -p forward           # отдельный крейт
cargo run -p claude-api -- serve # поднять сервер (env см. crates/server/src/config.rs)
cargo run -p claude-api -- sub list
```

Smoke без живых подписок — мок-апстрим (`CLAUDE_API_UPSTREAM=http://127.0.0.1:PORT`): проверяет
форвардинг, инжект identity, ротацию при 429, стрим. Пример — в истории/`README.md`.

## Рабочий цикл коммита

1. Определи компонент → перейди на его `comp/*`-ветку (или заведи feature-ветку от неё).
2. Меняй код в границах крейта; держи `cargo build` зелёным.
3. Обнови документацию, если поменялись границы/поведение (`crates/<x>/CLAUDE.md`, `ARCHITECTURE.md`).
4. Коммить по одному логическому изменению; секреты не стейджить (`git add <пути>`, не `git add -A`).
5. Влей в `master` по готовности. Trailer: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
