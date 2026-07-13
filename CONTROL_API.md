# CONTROL_API.md — контракт движок ↔ коммерция (Фаза 1)

Движок (`claude-api`) — **авторитет живого баланса** (reserve/settle локально). Коммерция
(будущий отдельный сервис) управляет им через **Control API** `/admin/*`, не имея прав
неметеренного форвардинга. Модель разделения — см. память проекта `claude-api-two-plane`.

## Авторизация — три раздельных ключа

| Ключ (env) | Права | Кто использует |
|---|---|---|
| `CLAUDE_API_KEYS` | forwarding-admin: неметеренный `/v1` + `/pool` + всё `/admin/*` + дашборды | оператор/движок |
| `CLAUDE_API_CONTROL_KEY` | `/admin/*` (аккаунты, ключи, кредит, статус) | **коммерция** |
| `CLAUDE_API_PANEL_KEY` | read-only `/capacity`, `/metrics` | панель/дашборд |

Иерархия: `authed` (admin) ⊂ `control_authed` (admin|control) ⊂ `readonly_authed` (admin|control|panel).
Ключ передаётся в `x-api-key:` (или `Authorization: Bearer`). Control-ключ **НЕ** форвардит `/v1`
(не админ, не метерный ключ → 401) — компрометация коммерц-ключа не даёт бесплатный инференс.

## Эндпоинты

Все тела — JSON. Все ответы — JSON. Суммы в целых **нанодолларах** (`1 USD = 1e9 nano`).

### POST /admin/account — создать аккаунт
```
→ {"handle"?: string, "mult_bp"?: int}          # mult_bp: наценка bp (2000 = ×0.20); деф = CLAUDE_API_MULT_BP
← 200 {"account":"acct_…","mult_bp":2500,"handle":"acme"}
← 409 {"error":"create failed"}
```

### GET /admin/account/{id} — состояние (для сверки)
```
← 200 {"account","balance_nano","spent_nano","reserved_nano","balance":"$X","mult_bp","status","handle"}
← 404 {"error":"unknown account"}
```

### POST /admin/account/{id}/credit — зачислить средства (ИДЕМПОТЕНТНО по ref)
```
→ {"usd"?: number, "amount_nano"?: int, "ref"?: string}   # ref = id платежа; повтор вебхука НЕ задвоит
← 200 {"account","balance_nano","balance":"$X"}
← 400 {"error":"need usd or amount_nano"}   ← 404 {"error":"unknown account"}
```
**Инвариант денег держит движок:** идемпотентность — partial UNIQUE index `ledger_topup_ref`.
Повторный `credit` с тем же `ref` вернёт тот же баланс, не начислив второй раз.

### POST /admin/account/{id}/status — блокировка
```
→ {"status":"active"|"disabled"}
← 200 {"account","status","updated":1}   ← 404 unknown account
```
`disabled` → все ключи аккаунта отбиваются на `/v1` (баланс не тратится).

### POST /admin/key — выпустить ключ доступа
```
→ {"account_id":"acct_…","label"?: string}
← 200 {"key":"sk-pool-…","account":"acct_…","label"}    # ключ показывается ЕДИНСТВЕННЫЙ раз
← 400 {"error":"unknown account"}   ← 409 {"error":"issue failed"}
```

### POST /admin/key/{key}/status — отзыв ключа
```
→ {"status":"active"|"disabled"}
← 200 {"status","updated":1}   ← 404 unknown key
```

## Поток «платёж → кредит» (для коммерции)

1. Клиент оплачивает → платёжный вебхук приходит в **коммерцию**.
2. Коммерция валидирует вебхук и зовёт `POST /admin/account/{id}/credit` с `ref` = id транзакции.
3. Движок атомарно и **идемпотентно** зачисляет (повторная доставка вебхука безопасна).
4. Живой баланс — в движке; коммерция при желании сверяется через `GET /admin/account/{id}`.

## Ещё НЕ реализовано (следующие фазы)

- **Usage-поток** движок→коммерция (списания для дашборда/истории) — Фаза 2.
- **TLS/домен** перед движком — Фаза 3 (сейчас Control API идёт по HTTP; до публичного запуска —
  только за доверенным периметром/VPN).
