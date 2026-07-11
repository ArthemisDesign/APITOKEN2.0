# claude-api — подписки Claude как прозрачный `/v1` API

Пул обычных подписок Claude (Max/Pro) отдаётся по сети как **API, неотличимый от
`api.anthropic.com`**. Наводишь любой Anthropic-клиент (SDK, `curl`, стороннее приложение) на
этот сервер — а под капотом запрос идёт **на квоте подписки из пула**, с ротацией по лимитам.
Никакого платного Anthropic API. Один бинарь на Rust, без внешних сервисов.

```
   Клиент (Anthropic SDK / curl)                POST /v1/messages  (наш api-key)
        │  base_url = наш сервер
        ▼
   claude-api (этот проект)
        │  1. авторизует клиента по нашему ключу (x-api-key)
        │  2. выбирает НАИМЕНЕЕ загруженную живую подписку из пула
        │  3. под капотом: Bearer подписки + Claude Code identity + oauth-beta + её прокси
        │  4. при 429/5xx/протухшем токене — cooling и ротация на следующую
        ▼
   api.anthropic.com   →   ответ (в т.ч. SSE-стрим) отдаётся клиенту БАЙТ-В-БАЙТ
```

Для клиента протокол ровно такой же, как у настоящего API (запрос/ответ/стриминг/ошибки).
Инжект «Claude Code identity» в `system` — единственное, что делается под капотом: без него
Anthropic не пускает OAuth-токены подписок на `/v1/messages`. Это невидимо для клиента.

---

## Из чего состоит (Cargo workspace)

Слои — только вниз: `registry ← pool ← forward ← server`. Карта — [`ARCHITECTURE.md`](ARCHITECTURE.md),
правила для агентов — [`CLAUDE.md`](CLAUDE.md), модель веток — [`BRANCHES.md`](BRANCHES.md).

| Крейт | Роль | Ветка-владелец |
|---|---|---|
| `crates/registry` | **Реестр подписок** (SQLite `subs`): email, OAuth-токен, прокси, статус, флот | `comp/registry` |
| `crates/pool` | **Пул + ротация**: выбор наименее загруженной, cooling при 429, состояние лимитов | `comp/pool` |
| `crates/forward` | **Прозрачный форвардинг** `/v1/*`: инжект identity, поллер лимитов, ротация, стрим | `comp/forward` |
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
export SUB_CFG_DIR=/srv/claude-api/data      # где лежит subscriptions.db

# добавить подписку (inline-токен или файл с токеном):
claude-api sub add account-a@example.com --token 'sk-ant-oat01-…' --proxy http://user:pass@1.2.3.4:8080 --fleet prod
claude-api sub add-file account-b@example.com --token-file ~/.claude-b/oauth_token --proxy 5.6.7.8:8080

claude-api sub list                          # список (без утечки токена)
claude-api sub status account-a@example.com paused   # active|paused|disabled
claude-api sub proxy  account-a@example.com http://…  # сменить прокси
claude-api sub fleet  account-a@example.com dev       # сменить флот
claude-api sub rm     account-a@example.com
```

БД совместима с исторической `subscriptions.db` (недостающие колонки доливаются мягкой миграцией).

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

Служебные эндпоинты: `GET /health` (без авторизации), `GET /pool` (статус пула, util/cooling).

## Конфигурация

Все переменные — в [`config.env.example`](config.env.example) (пул/порт/апстрим) и секреты в
[`server.env.example`](server.env.example) (ключи API). Под systemd —
[`systemd/claude-api.service`](systemd/claude-api.service) (грузит оба env-файла).

## Безопасность

В репозиторий **не попадают**: `subscriptions.db`, токены, прокси с паролями, `*.env`, `target/`
(см. `.gitignore`). Каждый аккаунт логинится и работает **с одного IP** (свой прокси) — чтобы не
триггерить анти-абьюз Claude. Ключи нашего API держим в `server.env` (вне репо); без ключей
сервер отвечает только на `127.0.0.1`.
