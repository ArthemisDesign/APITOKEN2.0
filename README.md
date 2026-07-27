# claude-api — подписки Claude как прозрачный `/v1` API

Пул обычных подписок Claude (Max/Pro) отдаётся по сети как **API, неотличимый от
`api.anthropic.com`**. Наводишь любой Anthropic-клиент (SDK, `curl`, стороннее приложение) на
этот сервер — а под капотом запрос идёт **на квоте подписки из пула**, с ротацией по лимитам.
Никакого платного Anthropic API. Один бинарь, engine-owned PostgreSQL authority и необязательный
Redis для общей эфемерной cache-affinity.

```
   Клиент (Anthropic SDK / curl)                POST /v1/messages  (наш api-key)
        │  base_url = наш сервер
        ▼
   claude-api (этот проект)
        │  1. авторизует клиента по нашему ключу (x-api-key)
        │  2. автоматически держит продолжение диалога на тёплой подписке; новые балансирует
        │  3. под капотом: Bearer подписки + Claude Code identity + oauth-beta + её прокси
        │  4. при 429/5xx/протухшем токене — cooling и ротация на следующую
        ▼
   api.anthropic.com   →   ответ (в т.ч. SSE-стрим) отдаётся клиенту БАЙТ-В-БАЙТ
```

Для клиента протокол ровно такой же, как у настоящего API (запрос/ответ/стриминг/ошибки).
Инжект «Claude Code identity» в `system` — единственное, что делается под капотом: без него
Anthropic не пускает OAuth-токены подписок на `/v1/messages`. Это невидимо для клиента.

Session ID передавать не нужно. Claude Code/harness распознаётся по уже существующему native header;
обычный SDK/curl/собственный продукт — по keyed-хэшам канонических префиксов растущей истории.
Привязка namespace-ится по аккаунту клиента, поэтому несколько его API-ключей разделяют тёплый кэш.
Local L1 работает без зависимостей, Redis делится привязками между engine slots и fail-open: его
отказ снижает только cache-hit, а деньги и capacity по-прежнему решает PostgreSQL.

---

## Из чего состоит (Cargo workspace)

Слои — только вниз: `registry ← pool ← forward ← server`. Карта — [`ARCHITECTURE.md`](ARCHITECTURE.md),
правила для агентов — [`CLAUDE.md`](CLAUDE.md), модель веток — [`BRANCHES.md`](BRANCHES.md),
production-хосты и эксплуатация — [`INFRASTRUCTURE.md`](INFRASTRUCTURE.md).
Операторский deploy/rollback — [`DEPLOYMENT.md`](DEPLOYMENT.md), модель PostgreSQL authority и
fencing Stage 2 — [`docs/STAGE2_POSTGRES_AUTHORITY.md`](docs/STAGE2_POSTGRES_AUTHORITY.md).
Contributor/AI workflow и автоматическая доставка `master` — [`CONTRIBUTING.md`](CONTRIBUTING.md).

| Крейт | Роль | Ветка-владелец |
|---|---|---|
| `crates/registry` | **PostgreSQL authority**: subscriptions, money reservations/outbox, capacity leases, epochs | `comp/registry` |
| `crates/pool` | **Пул + ротация**: выбор наименее загруженной, cooling при 429, состояние лимитов | `comp/pool` |
| `crates/forward` | **Прозрачный форвардинг** `/v1/*`: auto-affinity L1/Redis, identity, ротация, стрим | `comp/forward` |
| `crates/server` | **Композиция** (bin `claude-api`): env-конфиг, CLI, роутер `/health`+`/pool`, фоновые циклы | `comp/server` |

У каждого крейта — свой `CLAUDE.md` с локальными границами (Claude Code читает их автоматически).

---

## Сборка

```bash
cargo build --release          # → target/release/claude-api
```

## Реестр подписок (пункт 1)

Идентификатор — email. Подписке нужны только **OAuth-токен + прокси** (свой IP на аккаунт).

```bash
export SUB_CFG_DIR=/srv/claude-api/data      # local config/SQLite migration snapshot
export CLAUDE_API_DATABASE_URL=postgresql://.../claude_engine

# секреты читаются только из mode-0600 файлов, не из argv:
claude-api sub add-file account-a@example.com --token-file ~/.claude-b/oauth_token --proxy-file ~/.claude-b/proxy_url --fleet prod

claude-api sub list                          # список (тариф в колонке plan, без утечки токена)
claude-api sub status account-a@example.com paused   # active|paused|disabled
claude-api sub proxy  account-a@example.com --proxy-file ~/.claude-b/new_proxy_url
claude-api sub fleet  account-a@example.com dev       # сменить флот
claude-api sub rm     account-a@example.com
```

**Тариф подписки (pro / max5 / max20).** Определяется автоматически при `add`/`add-file` —
запросом `GET /api/oauth/profile` токеном подписки (как это делает Claude Code). Команды:

```bash
claude-api sub detect-plan [account-a@example.com]   # определить тариф (без email — все без тарифа)
claude-api sub set-plan account-a@example.com max20  # задать вручную (фолбэк)
```

> ⚠️ Токены от `claude setup-token` (их выпускает бот покупки) бывают со scope только
> `user:inference` — тогда профиль отвечает `403` и авто-детект даёт `noscope`. Тариф в этом
> случае ставится вручную (`set-plan`) или подтянется после перелогина токена (scope `user:profile`).

Историческая `subscriptions.db` импортируется guarded-командой Stage 2; active money authority —
role-isolated PostgreSQL. Import refuses anonymous in-flight holds and verifies balance aggregates.

## Запуск сервера

```bash
export SUB_CFG_DIR=/srv/claude-api/data
export CLAUDE_API_KEYS="длинный-случайный-ключ"   # ПУСТО = принимать только с 127.0.0.1
claude-api serve                                   # http://0.0.0.0:8787
```

Использование клиентом — как обычный Anthropic API, только `base_url` и ключ свои:

```bash
curl -s http://SERVER:8787/v1/messages \
  -H "x-api-key: $CLAUDE_API_KEYS" \
  -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model":"claude-opus-4-8","max_tokens":256,
       "messages":[{"role":"user","content":"2+2?"}]}'
```

```python
# Anthropic SDK — просто переопредели base_url:
from anthropic import Anthropic
client = Anthropic(base_url="http://SERVER:8787", api_key="длинный-случайный-ключ")
client.messages.create(model="claude-opus-4-8", max_tokens=256,
                       messages=[{"role":"user","content":"2+2?"}])
```

Служебные эндпоинты: `GET /live` (процесс жив), `GET /ready` (можно направлять новый трафик),
`GET /health` (совместимый health), `GET /pool` (статус пула, util/cooling). Во время drain
`/ready` возвращает 503 раньше закрытия listener; `/live` и `/health` остаются доступны.

## Конфигурация

Все переменные — в [`config.env.example`](config.env.example) (пул/порт/апстрим) и секреты в
[`server.env.example`](server.env.example) (ключи API). Production PostgreSQL-слоты запускает
[`systemd/claude-api@.service`](systemd/claude-api@.service); untemplated
[`systemd/claude-api.service`](systemd/claude-api.service) оставлен только как one-time bridge.
Watchdog автоматически создаёт Redis/affinity secrets и управляет локальным
[`apitoken-affinity-redis.service`](systemd/apitoken-affinity-redis.service).

Опциональный pinned `codex app-server` транспорт для строгого OpenAI-compatible text subset
доступен только через `https://openai.api.apitoken.sale/v1` и описан в
[`docs/CODEX_APP_SERVER.md`](docs/CODEX_APP_SERVER.md). Он выключен по умолчанию и не изменяет
существующий Claude-маршрут на `https://api.apitoken.sale`.

## Безопасность

В репозиторий **не попадают**: `subscriptions.db`, токены, прокси с паролями, `*.env`, `target/`
(см. `.gitignore`). Каждый аккаунт логинится и работает **с одного IP** (свой прокси) — чтобы не
триггерить анти-абьюз Claude. Ключи нашего API держим в `server.env` (вне репо); без ключей
сервер отвечает только на `127.0.0.1`.
