# Интеграционный гайд движка (для бэкенда сайта + оплаты)

Этот документ — всё, что нужно, чтобы написать **сайт с оплатой** поверх нашего движка, **не трогая Rust**.
Ты пишешь: сайт (регистрация/личный кабинет), приём оплаты, свою БД пользователей. Движок берёт на
себя: раздачу Claude API, ротацию подписок, **точный учёт денег** (резерв/списание до нанодоллара).
Твой бэкенд командует движком по HTTP через **Control API** (`/admin/*`).

---

## 1. Роли: кто что делает

| | **Движок** (готов, не трогаешь) | **Твой сервис** (пишешь ты) |
|---|---|---|
| Раздача `POST /v1/messages` клиентам | ✅ | — |
| Ротация подписок, лимиты, устойчивость | ✅ | — |
| **Авторитетный баланс** аккаунта, резерв/списание | ✅ | — |
| Аккаунты/ключи/журнал в engine-owned PostgreSQL | ✅ | — |
| Сайт, регистрация, личный кабинет | — | ✅ |
| Приём платежей (Stripe/крипта/…) + вебхуки | — | ✅ |
| Своя БД: юзеры, пароли, связь юзер→account_id, история платежей | — | ✅ |
| Вызовы Control API движка | — | ✅ |

**Модель:** движок — источник истины по деньгам. Твой сервис хранит ЛЮДЕЙ и ПЛАТЕЖИ, а деньги
кредитует и читает у движка. Один `account` в движке = один клиент (или команда) у тебя.

---

## 2. Доступ

- **Production-база для API/worker на том же host:** `http://127.0.0.1:8790`. Это явно
  loopback-bound Caddy origin, который health-route-ит активный engine slot 8787/8788. Никогда не
  закрепляй commerce consumer за конкретным slot-портом. Для другого host Control API должен идти
  только по аутентифицированной приватной сети/TLS; публичный engine-домен admin routes не экспонирует.
- **Твой ключ:** `CONTROL_KEY` (выдан отдельно). Шли в заголовке **`x-api-key: <CONTROL_KEY>`** на все
  `/admin/*`. Этим же ключом **нельзя** раздавать `/v1` — он только для управления (компрометация
  бэкенда ≠ бесплатный инференс).
- Все тела — **JSON**. Все суммы — целые **нанодоллары**: `1 USD = 1 000 000 000 nano`. Никаких float
  в деньгах — работай в nano, дели на 1e9 только для показа.

---

## 3. Денежная модель (обязательно понять)

У аккаунта три «корзины», инвариант держит движок:
```
свободный_баланс + зарезервировано + потрачено = внесено   (всегда, до нанодоллара)
```
- **balance_nano** — свободные деньги, доступные тратить прямо сейчас.
- **reserved_nano** — временно удержано под запросы «в полёте» (движок резервирует потолок перед
  запросом и возвращает разницу после). Ты это не трогаешь — просто знай, что «в моменте» баланс
  может быть чуть меньше на сумму летящих запросов.
- **spent_nano** — суммарно потрачено (монотонно растёт).
- **mult_bp** — наценка в basis points: `2000 = ×0.20` (клиент платит 20% от реального API-эквивалента).
  Это твоя маржа/цена. Задаётся на аккаунт.

Клиент физически **не может уйти в минус**: если денег не хватит — движок урежет ответ под баланс или
вернёт `402`. Тебе не нужно следить за «перерасходом» — его не бывает.

---

## 4. Канонические сценарии

### A. Регистрация клиента
1. Юзер регистрируется у тебя на сайте → ты создаёшь запись в СВОЕЙ БД.
2. `POST /admin/account` → движок вернёт `account_id` (`acct_…`). Сохрани его у себя рядом с юзером.
   Повтор с тем же непустым `handle` вернёт тот же аккаунт, поэтому восстановление регистрации
   идемпотентно и не создаёт осиротевшие аккаунты.
3. `POST /admin/key` c этим `account_id` → движок вернёт **`sk-pool-…`**. Покажи юзеру **один раз**
   (это его API-ключ, секрет). У аккаунта может быть много ключей (на проекты/команду).

### B. Оплата → зачисление (ИДЕМПОТЕНТНО!)
1. Юзер платит → твой платёжный провайдер шлёт тебе **вебхук**.
2. Ты валидируешь вебхук и зовёшь `POST /admin/account/{id}/credit` с `ref` = **id транзакции платежа**.
3. Движок зачислит **идемпотентно по `ref`**: если провайдер доставит вебхук дважды — второй раз
   **НЕ задвоит** (вернёт тот же баланс). Всегда передавай `ref` = уникальный id платежа.

### C. Личный кабинет (баланс/ключи/история)
- Баланс/траты: `GET /admin/account/{id}` → `balance_nano`, `spent_nano`, `reserved_nano`.
- Список ключей юзера: `GET /admin/account/{id}/keys` (не-секретный `key_id`, маска,
  label/status/расход).
- История платежей/трат: `GET /admin/account/{id}/ledger?limit=50` (топапы/списания сверху).

### D. Как клиент ПОЛЬЗУЕТСЯ (что показать ему в доке)
Клиент наводит любой Anthropic-совместимый инструмент на нашу базу и свой `sk-pool-` ключ:
```bash
curl https://<база>/v1/messages \
  -H "x-api-key: sk-pool-…" -H "anthropic-version: 2023-06-01" \
  -H "content-type: application/json" \
  -d '{"model":"claude-opus-4-8","max_tokens":1024,"messages":[{"role":"user","content":"hi"}]}'
```
Свой остаток клиент смотрит сам: `GET /v1` → нет; **`GET /balance`** со своим `sk-pool-` ключом
(`x-api-key`) → JSON с балансом/расходом. Всё остальное — чистый Anthropic API (стрим, tools, count_tokens).

---

## 5. Справочник Control API (`x-api-key: <CONTROL_KEY>`)

### Аккаунты
```
POST /admin/account                     {"handle"?, "mult_bp"?}      → 200 {account, mult_bp, handle}
GET  /admin/account/{id}                                             → 200 {balance_nano, spent_nano,
                                                                            reserved_nano, balance, mult_bp, status, handle} | 404
POST /admin/account/{id}/credit         {"usd"?|"amount_nano"?, "ref"?} → 200 {balance_nano, balance} | 404
                                        (идемпотентно по ref; usd отрицательный = коррекция)
POST /admin/account/{id}/status         {"status":"active"|"disabled"}  → 200 {updated} | 404
POST /admin/account/{id}/pricing        {"mult_bp":0..10000}             → 200 {account,mult_bp,updated} | 404
GET  /admin/account/{id}/keys                                        → 200 {keys:[{key_id,key_masked,label,status,
                                                                            spent_nano,reserved_nano,
                                                                            spend_limit_nano,expires_ts,
                                                                            created_ts,last_used_ts}]}
GET  /admin/account/{id}/ledger?limit=N[&after_id=ID]                 → 200 {entries:[{id,kind,amount_nano,ref,ts,...}]}
```

Without `after_id`, ledger entries are the newest bounded history. With `after_id`, entries are
returned oldest-first with `id > after_id`; this is the durable worker cursor for progressive pricing.

### Ключи доступа
```
POST /admin/key                         {"account_id", "label"?,
                                         "spend_limit_nano"?, "expires_ts"?}
                                                                     → 200 {key:"sk-pool-…", key_id:"key_…", account,
                                                                            spend_limit_nano,expires_ts}  (key виден 1 раз!)
POST /admin/key-id/{key_id}/status      {"status":"active"|"disabled"} → 200 {updated} | 404 (рекомендуется)
POST /admin/account/{id}/key-id/{key_id}/policy
                                        {"spend_limit_nano":string|null,
                                         "expires_ts":integer|null}
                                                                     → 200 {key_id,spend_limit_nano,
                                                                            expires_ts,updated} | 404 | 409
```

`key_id` не даёт доступа к `/v1` и безопасен для хранения в коммерческой PostgreSQL. Новый backend
должен отзывать ключ по `key_id`, чтобы никогда не сохранять пригодный к использованию `sk-pool-…`.
Полный ключ никогда помещается в URL; legacy endpoint удалён.

`spend_limit_nano` is an optional positive decimal string and caps lifetime charged platform spend
for that key. `expires_ts` is an optional future Unix timestamp in seconds. The engine enforces both
again inside the atomic reservation transaction, including in-flight holds, so concurrent requests
cannot cross a key's cap. `NULL` means unlimited/no expiration and preserves legacy behavior.
The policy endpoint is an account-scoped full replacement: both nullable fields are required.
It can increase or clear a limit and extend or clear expiry without changing key status. A new
limit below `spent_nano + reserved_nano` is rejected atomically with `409` and code
`limit_below_committed`, so an edit cannot invalidate an in-flight reservation.

### Коды ошибок
`400` неверное тело · `401` нет/неверный control-ключ · `404` аккаунт/ключ не найден ·
`409` конфликт создания или лимит ниже уже списанного+зарезервированного · `503` биллинг выключен.
На клиентском `/v1`: `402` баланс ≤ 0.

### Пример: полный цикл (bash)
```bash
CTL=<CONTROL_KEY>; B=http://127.0.0.1:8790
AID=$(curl -s -XPOST $B/admin/account -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d '{"handle":"acme","mult_bp":2000}' | jq -r .account)
curl -s -XPOST $B/admin/account/$AID/credit -H "x-api-key: $CTL" -H 'content-type: application/json' \
     -d '{"usd":25,"ref":"stripe_pi_123"}'                       # зачислить $25 (идемпотентно)
KEY=$(curl -s -XPOST $B/admin/key -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d "{\"account_id\":\"$AID\",\"label\":\"prod\"}" | jq -r .key)   # выдать клиенту
curl -s $B/admin/account/$AID -H "x-api-key: $CTL"               # баланс для кабинета
curl -s $B/admin/account/$AID/ledger -H "x-api-key: $CTL"        # история
```

---

## 6. Чего пока НЕТ (не блокирует старт)

- **Push-поток usage** движок→твой сервис отсутствует. Коммерческий worker опрашивает cursor-based
  `GET /admin/account/{id}/ledger?after_id=...`; доставка при этом идемпотентна.
- **Cross-host TLS/private networking** остаётся частью Фазы 3. Текущий HTTP Control origin доступен
  только через loopback на том же host и не публикуется Caddy наружу.
- Ротация `CONTROL_KEY` требует согласованно обновить engine, API и worker env, затем провести
  обычный engine/API blue-green и stop/start worker по `DEPLOYMENT.md`; одиночный ручной restart
  создаст окно с несовпадающими ключами.

Вопросы по контракту — они все закрываются этим движком; если чего-то не хватает для сайта,
допилим на нашей стороне.
