# Выбор подписки на запрос (пул-селектор)

Как из пула аккаунтов выбирается живая подписка на каждый ход, как отслеживаются лимиты
и как крутится ротация. Это логика полноценного оркестратора (в проде — внутри движка
`tg_agent`); минимальная standalone-версия — `bin/claude-api` (берёт активную подписку).

> Ссылки на строки — для того, у кого есть исходник движка. Здесь важен **алгоритм**.

## Модель в памяти

`SubInfo` — рантайм-запись подписки: `token`, `proxy`, `plan`, `util5h`, `util7d`,
`status`, `reset5h/7d`, `cooling_until`. Утилизация окна берётся из заголовков ответа
Claude (`anthropic-ratelimit-unified-5h/7d-*`).

Загрузка: `load_subs_from_registry()` — сначала `subscriptions.db`, при ошибке — JSON-зеркало.
Фильтр `sub_row_to_info()`: берём только `kind=token`, не `paused/disabled`, и **совпадающий
флот** (`my_fleet()`: env `SUBS_FLEET`, дефолт `all`). Обновление пула сохраняет волатильные
`util/cooling` (`refresh_pool_from_registry`).

## Опрос лимитов (poller)

`spawn_subs_poller()` — фоновый тред: для каждой подписки, чей интервал подошёл, шлёт
**минимальный** `POST /v1/messages` (`max_tokens:1`) через её прокси с заголовками
`authorization: Bearer <token>`, `anthropic-beta: oauth-2025-04-20`, читает из ответа
`…-5h/7d-utilization | status | reset`. Интервал адаптивный 12–60с (`poll_interval_for`).
Пишет снапшот пула (`subs_live.json`) и историю (`subs_poll_log`) — **без секретов**
(только email/plan/util/status/resets).

## Выбор на ход

`pick_sub(is_brain)` — селектор на каждый ход:
1. `least_loaded()` — балансировка: в первую очередь по **7-дневному** окну (полоса ±2%),
   затем по 5ч, затем round-robin (`PICK_RR`).
2. Клиентские ходы допускаются при утилизации **< 95%**; «мозговые» (привратник/модерация) —
   до 100%.
3. Все на лимите → `await_capacity()` ставит ход в очередь до ближайшего `reset`.

`mark_current_sub_cooling()` — circuit-breaker: если сам `claude` вернул `429`, подписке
сразу ставится cooling (не ждём поллер). Текущая подписка хода — в thread-local `CURRENT_SUB`.

## Как подписка становится вызовом API

`claude_base_cmd()` — центральная точка: выбирает подписку (`pick_sub`), затем строит
команду `claude` с окружением:

```
CLAUDE_CONFIG_DIR   = profile_dir выбранной подписки   (host_profile)
CLAUDE_CODE_OAUTH_TOKEN = содержимое token_file          (box_oauth_token)
HTTPS_PROXY/HTTP_PROXY  = proxy подписки                 (box_proxy)
```

Есть два пути: локальный (`env … ~/.local/bin/claude`) и в docker-коробке
(`docker exec -e … claude`).

**Липкость сессии:** пул выбирает подписку заново каждый ход, но при `--resume`
`CLAUDE_CONFIG_DIR` пинится к тому профилю, где реально лежит `.jsonl` сессии, а токен и
прокси берутся у выбранной подписки (авторизация — через env-токен, отвязана от config-dir).

Если пул не выбрал ничего (все на лимите) — `resolve_fallback_sub_email()` относит расход
на активный профиль (`active_profile`).

## Ёмкость и пополнение

- `subs_product_schema.py` — таблицы ёмкости/тарифов (`plan_capacity`, `client_plans`).
- `subs_capacity_calc.py` — расчёт «предложение vs законтрактовано», сигнал докупки.
- `subs_reorder_alert.py` — алерт 🟡/🔴 при нехватке ёмкости.
- `token-refresh.timer` (каждые 20 мин), `subs-reorder-alert.timer` (30 мин).
