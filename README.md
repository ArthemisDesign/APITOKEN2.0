# Claude Subscriptions → API

Система, которая берёт **обычные подписки Claude** (аккаунты Max/Pro) и превращает их в
**программный доступ (API)** — через OAuth-профили и CLI `claude`, **без платного Anthropic API**.
Пул аккаунтов, ротация по лимитам, разделение по флотам, поддержание токенов живыми и
пополнение пула ботом-покупателем.

> Этот репозиторий — извлечённая подсистема «подписки → API»: как подписки **записываются**,
> как **превращаются в API** и как ими **управляют**. Без живых секретов (токены/email/прокси
> в репозиторий не попадают — см. `.gitignore`).

---

## 1. Идея

Подписка Claude (Max/Pro) даёт доступ к моделям через приложение/CLI, но **не** через
платный API. Мы логинимся в аккаунт по **OAuth** (`claude auth login`), сохраняем токен в
отдельный **профиль-каталог**, и затем любой вызов CLI `claude` с этим профилем работает
«как API» — на квоте подписки, а не на API-биллинге.

Множество таких аккаунтов складывается в **пул**. Оркестратор на каждый запрос выбирает
живую подписку, подставляет её профиль и прокси, и выполняет ход. При упоре в лимит —
переключается на другую. Так подписки становятся масштабируемым бэкендом.

```
   Claude-аккаунт (email, план Max/Pro)
        │  OAuth-логин под своим прокси (claude auth login → code#state)
        ▼
   Профиль-каталог  ~/.claude-<slug>/.credentials.json      ← это и есть "ключ"
        │  (CLAUDE_CONFIG_DIR = этот каталог)
        ▼
   Реестр пула:  subscriptions.db (истина) + subscriptions.json (зеркало)
        │  поля: email, profile_dir, token_file, proxy, plan, status, fleet, refresher…
        ▼
   Оркестратор на ход:  pick_sub() → CLAUDE_CONFIG_DIR + HTTPS_PROXY → `claude -p …`
        │  выбор по флоту(dev/prod) + плану + ёмкости; при лимите — ротация/cooling
        ▼
   Ответ модели  ←  подписка отработала как API
```

---

## 2. Как подписки записываются (реестр)

Идентификатор подписки = **email аккаунта**. Каталог профиля выводится из email:
`me@x.com → ~/.claude-me-x-com`.

Источник истины — **`subscriptions.db`** (общий SQLite), JSON — зеркало/фолбэк.
Пример структуры: [`schema/subscriptions.example.json`](schema/subscriptions.example.json).

| Поле | Смысл |
|---|---|
| `email` | идентификатор аккаунта-подписки |
| `kind` | `token` (OAuth-токен) |
| `profile_dir` | каталог профиля = **`CLAUDE_CONFIG_DIR`** |
| `token_file` | `…/.credentials.json` (OAuth-токен, **секрет**) |
| `proxy` | `http://user:pass@ip:port` — аккаунт логинится и работает с одного IP |
| `plan` | `pro` \| `max20` (влияет на ёмкость/лимиты) |
| `status` | `active` \| `disabled` |
| `source` | `auth_bot` (куплен ботом) \| ручной OAuth |
| `fleet` | `dev` \| `prod` — пул берёт только подписки своего флота |
| `refresher` | держать токен живым фоновым прогревом |
| `added` / `added_ts` | когда добавлена |

Проекции активной подписки (плоские файлы, читают движок и коробки):
- `active_profile` — каталог профиля;
- `active_proxy` — `http://user:pass@host:port` (уходит в коробку как `HTTPS_PROXY`).

---

## 3. Как подписки превращаются в API

Механизм — **`CLAUDE_CONFIG_DIR`**: у каждой подписки свой профиль-каталог, и вызов `claude`
с этой переменной использует именно её токен.

```bash
# любой вызов CLI на квоте конкретной подписки:
CLAUDE_CONFIG_DIR="$HOME/.claude-<slug>" HTTPS_PROXY="$proxy" \
  ~/.local/bin/claude -p "Reply with exactly: ok" --model claude-haiku-4-5-20251001
# → ok        (это и есть "подписка как API")
```

Подробности OAuth-флоу (claude CLI v2.1.x: URL → браузер → `code#state` в приглашение),
перелогин протухшего токена, маршрутизация — в [`docs/CLAUDE_AUTH_PROFILES.md`](docs/CLAUDE_AUTH_PROFILES.md).

**Выбор подписки на ход (пул-селектор оркестратора)** — см. [`docs/POOL_SELECTION.md`](docs/POOL_SELECTION.md):
разделение по флотам `dev/prod`, выбор по плану и ёмкости (`plan_capacity`), резервный аккаунт
при пустом пуле, cooling при упоре в лимит.

---

## 4. Управление: `scripts/subscription.sh`

Реестр + авторизация через прокси. Идентификатор везде — `<email>`.

```bash
subscription.sh add        <email> <proxy>          # OAuth-логин + ввод code#state + запись
subscription.sh auth-start <email> [proxy]          # начать логин, напечатать OAuth-URL
subscription.sh auth-finish <email> "<code#state>"  # докормить код, проверить, записать
subscription.sh relogin    <email>                  # перелогин с сохранённым proxy
subscription.sh activate   <email>                  # сделать активной (пишет проекции)
subscription.sh set-proxy  <email> <proxy>          # сменить прокси
subscription.sh refresh    [email]                  # прогрев профиля (держит токен живым)
subscription.sh ping       <email>                  # проверить живость (Haiku-пинг через прокси)
subscription.sh list                                # список подписок
```

Формат прокси на входе: `ip:port:user:pass` | `ip:port` | `http(s)://…`.

---

## 5. Пополнение пула: `tools/auth_token_bot`

Основной канал добычи подписок — покупка у продавцов через бота: продавец логинит свой
аккаунт, бот вытаскивает 1-летний токен (`claude setup-token` в PTY) и добавляет в пул со
статусом/планом/флотом. Конфиг-пример — [`tools/auth_token_bot/auth_bot.env.example`](tools/auth_token_bot/auth_bot.env.example),
сервис — [`systemd/claude-auth-bot.service`](systemd/claude-auth-bot.service). (Сам бинарь бота —
вне этого репозитория; здесь — интерфейс и деплой.)

---

## 6. Структура репозитория

```
bin/claude-api                      — CLI-РАННЕР: запрос на квоте активной подписки (Opus 4.8, выбор модели, учёт токенов)
bin/claude-api-server               — ЗАПУСК HTTP-сервера (пул как сетевой API)
server/app.py                       — HTTP-СЕРВЕР: /run, /v1/messages, /pool, /health; авто-ротация при 429
lib/pool.py                         — ПУЛ-СЕЛЕКТОР: выбор наименее загруженной подписки + поллер лимитов + cooling
lib/cost.py                         — учёт токенов + USD-эквивалент (по-модельные цены, порт из движка)
scripts/subscription.sh            — CLI реестра + OAuth-логин под прокси
docs/HTTP_API.md                    — сетевой API: эндпоинты, авторизация, распределение
docs/CLAUDE_AUTH_PROFILES.md        — механизм авторизации (OAuth, CLAUDE_CONFIG_DIR, релогин)
docs/POOL_SELECTION.md              — как пул выбирает подписку на ход + ротация (lib/pool.py)
docs/COST.md                        — цены $/Mtoken + формула USD-эквивалента
schema/subscriptions.example.json   — пример структуры реестра (без секретов)
schema/subscriptions.schema.sql     — схема таблицы пула (SQLite)
config.env.example                  — конфиг по умолчанию (модель, пути, сервер)
server.env.example                  — секреты сервера (ключи API), вне репо
tools/auth_token_bot/               — конфиг-пример бота пополнения пула
systemd/claude-api-server.service   — сервис-юнит HTTP-сервера
systemd/claude-auth-bot.service     — сервис-юнит бота
```

---

## 7. Запуск (standalone, тот же пул подписок)

Проект самостоятельный: свой каталог, свой реестр (`SUB_CFG_DIR`), но работает с **тем же
пулом аккаунтов**, что и основной. Импорт текущего пула = копия `subscriptions.db` в свой
data-каталог (профили `~/.claude-<slug>` — общие, на них ссылается реестр).

```bash
export SUB_CFG_DIR=/srv/claude-api/data          # СВОЙ реестр (не /srv/agents/…)
mkdir -p "$SUB_CFG_DIR"

# 1) импортировать текущий пул подписок в свой проект:
cp /srv/agents/.config/personal-agents/subscriptions.db "$SUB_CFG_DIR/"

# 2) посмотреть пул:
scripts/subscription.sh list

# 3) выполнить запрос через пул (подписка → API):
bin/claude-api "Reply with exactly: ok"
bin/claude-api --sub account-a@example.com --model claude-haiku-4-5-20251001 "2+2?"
```

Добавить новую подписку в СВОЙ реестр — тот же `subscription.sh add <email> <proxy>`
(при `SUB_CFG_DIR`, указывающем на проект). Дальше `activate`/`refresh`/`ping` — как обычно.

## 8. Сетевой API (HTTP-сервер поверх пула)

CLI-раннер берёт **одну активную** подписку. HTTP-сервер (`server/app.py`) отдаёт **весь пул**
по сети: на каждый запрос выбирает наименее загруженную живую подписку, при 429 — авто-ротация
на следующую; фоновый поллер держит утилизацию окон свежей. Полный справочник — [`docs/HTTP_API.md`](docs/HTTP_API.md).

```bash
export SUB_CFG_DIR=/srv/claude-api/data
export CLAUDE_API_KEYS="длинный-случайный-ключ"     # ПУСТО = только с 127.0.0.1
bin/claude-api-server                                # http://0.0.0.0:8787

curl -s -X POST http://127.0.0.1:8787/run \
  -H "Authorization: Bearer $CLAUDE_API_KEYS" \
  -d '{"prompt":"2+2?","model":"claude-opus-4-8"}'
# → {"result":"4","sub":"account-a@example.com","usd":0.0012,"attempts":1,...}
```

Эндпоинты: `POST /run`, `POST /v1/messages` (Anthropic-совместимый шим), `GET /pool`, `GET /health`.
Под systemd — [`systemd/claude-api-server.service`](systemd/claude-api-server.service).

## 9. Безопасность

**В репозиторий не попадают:** токены (`.credentials.json`), реальные `subscriptions.json`/`.db`,
прокси с паролями, email-аккаунты, `*.env`, `*.gpg`. Всё это в `.gitignore`. Здесь только **код,
схемы и механизм**. Каждый аккаунт логинится и работает **с одного IP** (свой прокси) —
чтобы не триггерить анти-абьюз Claude.
