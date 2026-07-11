# HTTP-сервер: пул подписок как сетевой API

`server/app.py` (запуск — `bin/claude-api-server`) отдаёт пул подписок по HTTP. На каждый
запрос пул-селектор (`lib/pool.py`) выбирает наименее загруженную живую подписку, ход
исполняется через её OAuth-токен + прокси (`claude` CLI, как Claude Code); при 429/лимите —
**авто-ротация** на следующую подписку. Фоновый поллер держит утилизацию окон свежей для
балансировки. Учёт токенов + USD-эквивалент — в `usage.log` (см. `docs/COST.md`).

Без внешних зависимостей (stdlib `http.server`) — работает под systemd на сервере как есть.

## Запуск

```bash
export SUB_CFG_DIR=/srv/claude-api/data       # свой реестр пула
export CLAUDE_BIN=/srv/agents/.local/bin/claude
export CLAUDE_API_KEYS="длинный-случайный-ключ"   # ПУСТО = только с 127.0.0.1
bin/claude-api-server                          # слушает 0.0.0.0:8787
```

Под systemd — `systemd/claude-api-server.service` (ключи из `server.env`, вне репо).

## Переменные окружения

| Env | Деф. | Смысл |
|---|---|---|
| `CLAUDE_API_HOST` / `CLAUDE_API_PORT` | `0.0.0.0` / `8787` | адрес прослушки |
| `CLAUDE_API_KEYS` | — | ключи API через запятую; пусто → только localhost |
| `CLAUDE_API_MODEL` | `claude-opus-4-8` | дефолт-модель |
| `CLAUDE_API_MAX_TRIES` | `3` | попыток ротации при 429/лимите |
| `CLAUDE_API_TIMEOUT` | `600` | таймаут одного хода (сек) |
| `CLAUDE_API_POLL` | `1` | фоновый поллер лимитов (0 = выкл) |
| `CLAUDE_API_UTIL_CAP` | `0.95` | клиентский потолок утилизации окна |
| `CLAUDE_API_COOL_SECS` | `300` | cooling при 429 без известного reset |
| `SUBS_FLEET` | — (все) | брать подписки только этого флота |

## Эндпоинты

### `GET /health` — без авторизации
```json
{ "ok": true, "subs": 3, "model": "claude-opus-4-8", "auth": true }
```

### `GET /pool` — статус пула (без секретов)
```json
{ "pool": [ { "email": "...", "plan": "max20", "util5h": 0.12, "util7d": 0.4,
             "cooling": false, "last_used": 1783… } ], "cap": 0.95, "poller": true }
```

### `POST /run` — простой вызов
Тело: `{ "prompt": "...", "model"?: "...", "sub"?: "email", "allow_full"?: false }`
```bash
curl -s -X POST http://HOST:8787/run \
  -H "Authorization: Bearer $KEY" \
  -d '{"prompt":"2+2?","model":"claude-opus-4-8"}'
```
Ответ: `{ "result": "...", "sub": "who-served", "usd": 0.0012, "usage": {...},
          "tokens": {...}, "attempts": 1 }`. При исчерпании пула — `502` с `error`.

- `sub` — попросить конкретную подписку (иначе выбирает пул).
- `allow_full` — пускать до 100% утилизации (приоритетные ходы), иначе потолок 0.95.

### `POST /v1/messages` — минимальный Anthropic-совместимый
Тело: `{ "model", "max_tokens"?, "system"?, "messages": [{"role","content"}] }`.
`messages[]` схлопываются в плоский prompt → пул → ответ в форме Anthropic:
```json
{ "type":"message", "role":"assistant", "content":[{"type":"text","text":"..."}],
  "usage": {"input_tokens":…,"output_tokens":…},
  "_pool": {"sub":"...","usd":…,"attempts":1} }
```
> Это НЕ полный порт Messages API (нет tool-use/стриминга/мультимодальности) — тонкий
> шим, чтобы клиенты, умеющие в `/v1/messages`, ходили в пул. Расширяем по мере надобности.

## Авторизация

`Authorization: Bearer <key>` **или** `x-api-key: <key>`. Ключи — `CLAUDE_API_KEYS`.
Если ключи не заданы — сервер отвечает только на запросы с `127.0.0.1` (для локальной отладки).
`/health` открыт всегда (для healthcheck'ов).

## Распределение и лимиты

- **Поллер** (фон, адаптивно 12–60с по загрузке): минимальный запрос через прокси каждой
  подписки, читает `anthropic-ratelimit-unified-5h/7d-*` из заголовков → `subs_live.json`.
- **Выбор**: наименее загруженная не-остывающая подписка под потолком `CLAUDE_API_UTIL_CAP`.
- **Ротация**: если `claude` вернул 429/лимит — подписка идёт в cooling (`CLAUDE_API_COOL_SECS`
  или до `reset`), ход повторяется на следующей (до `CLAUDE_API_MAX_TRIES`).
- Алгоритм — `docs/POOL_SELECTION.md`.
