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

### Operator telemetry подписок

Same-origin админка дополнительно читает `GET /capacity`, `GET /codex-subs` и `GET /gemini-subs`.
Эти маршруты защищены server-side control/panel auth; браузер получает их только через закрытый
`admin.apitoken.sale`, а ключи ему не выдаются.

`GET /capacity` дополнительно публикует Claude `window_totals`, horizon `available_nano` и
`conversion_models`. Денежные поля для расчётов — decimal nanoUSD strings. Каталог берётся из
`metering` и разделяет Standard/Fast input, cache-read, cache-write 5м/1ч и output; Web Search имеет
отдельную per-request ставку. Per-sub `rem5h_nano`/`rem7d_nano` и email mask позволяют панели
рисовать компактные окна без float money и без раскрытия аккаунта.

Claude capacity — exact realized API-dollar equivalent фактически обслуженной смеси, а не цена
Max/Pro и не обещание фиксированного числа токенов. Каждый успешный turn (customer или admin)
сохраняет immutable model/tier/geo/tariff event с отдельными token и API nanoUSD legs; бесплатный
poll сохраняет только quota observation. После authoritative usage backend сам ставит spend event
в durable FIFO и будит бесплатный post-turn count-tokens probe обслужившей подписки; открытие
админки и следующий пользовательский запрос для накопления evidence не нужны. Forced probe
дебаунсится до одного раза за 15 секунд на подписку, а poll observation всегда дренирует pending
turn FIFO раньше чтения cumulative spend. Пустой plan backend probe восстанавливает через официальный
OAuth profile endpoint; inference-only token с 403 может унаследовать только единогласный известный
paid plan (`pro|max5|max20`) текущего fleet. Mixed/unknown fleet остаётся fail-closed. Найденный plan
durable записывается в registry и применяется к live roster до quota observation;
открывать админку или вручную править cohort не требуется. Если response и post-turn poll попали в
одну секунду, изменившаяся quota всё равно принимается в FIFO-порядке; только точный endpoint-дубль
игнорируется. Для каждого exact plan и окна независимо:

```text
capacity_per_subscription_nano =
  100_000_000 × Σobserved_spend_nano / Σobserved_fraction_units
```

5h (`300` минут) и 7d (`10080` минут) не делят anchor/history. `plan_cohorts` объединяет evidence
только одинакового `plan + window_minutes`, поэтому все routable подписки одного плана получают
одну pooled оценку; `window_totals` суммирует её по routable fleet. Другой план без собственного
положительного evidence делает fleet total `null`, а не частичной суммой. Номинал подписки,
configured prior, EMA/WLS и float money в authority не участвуют.

Текущий remaining требует quota snapshot не старше 900 секунд. Историческая full-window capacity
может оставаться известной при stale/missing snapshot, но remaining/horizon тогда `null` с точным
`missing_reason`. То же fail-closed правило действует, пока FIFO-доставка exact turn evidence имеет
pending event или degraded integrity. `calibration_delivery` публикует `pending_events`,
`dropped_events`, `persistence_ok` и `queue_limit`; нормальное состояние — `0/0/true`. Failed head
переживает transient authority outage в памяти и повторяется раньше более поздних events/snapshots;
immutable replay идемпотентен, semantic conflict изолируется и увеличивает dropped diagnostic.
`calibration_evidence` — агрегаты реальных запросов по masked email/model/tier/geo/tariff со всеми
token/cost legs, отсортировать их для UI можно по `api_total_nanousd`.
`calibration_recent_turns` — bounded newest-first окно до 512 отдельных immutable Anthropic events.
Каждая строка содержит opaque внутренний `request_id`, тот же masked email, полную model/tier/geo/
tariff identity и все token/cost legs; prompt, полный email и credential не публикуются.
`calibration_recent_turn_limit=512` фиксирует серверную границу. Это окно предназначено для точной
операторской атрибуции live-теста через разность request-id sets; агрегаты для этого использовать
нельзя, потому что параллельный customer traffic законно меняет ту же строку.

Bounded production-прогон и правила интерпретации model-level quota deltas описаны в
`docs/ops/CLAUDE_CALIBRATION.md`; runner использует только этот backend contract и не зависит от UI.

`GET /overview` сохраняет прежние округлённые `supply.*_usd` display-поля для панели, но их источник
теперь тот же exact report. Канонические значения находятся рядом в `supply.avail_nano`,
`cap_nano`, `consumed_nano`; `supply.legacy_pool_prior_authoritative=false`. При отсутствии exact
evidence capacity-facing поля fail-closed в `null`, а не возвращаются из старого pool prior/EMA.

`GET /codex-subs` разделяет два разных понятия:

- `*_nanocredits` — native расход и capacity ChatGPT-подписки; именно в credits сравниваются
  одинаковые планы;
- `*_nano` / `*_nanousd` — официальный public API replacement cost фактической или выбранной
  нагрузки. Он меняется от модели, Standard/Fast, cache mix, output и long context и не является
  фиксированным номиналом подписки.

У каждого home `calibration_evidence` содержит immutable aggregates по model/effective tier/
provider-reported tier/tariff schedules: turns, fresh-derived total input, cached input,
cache-write, output/reasoning, все API legs и все ChatGPT-credit legs. Эта evidence появляется
после первого успешного turn и не ждёт движения quota. `capacity_nanocredits` остаётся `null`, пока
не появится подтверждённое положительное `Δquota`; `null` не означает ноль. Integrity поля
`calibration_pending_events`/`calibration_dropped_events` должны быть `0/0`.

`measurement_resolution_fraction_units` у окна сообщает реальную числовую разрешающую способность
quota snapshot: для типичного целого `40%` это `1_000_000`, а не `1`. Low/high estimator v10
учитывают половину разрешения обоих endpoints; если движение quota не больше этой погрешности,
верхняя граница честно остаётся `null`.

Для коммерческого ответа по одинаковым подпискам используй корневой `plan_cohorts`, сгруппированный
по exact `plan + window_minutes`. `capacity_per_home_nanocredits` — одна общая pooled-оценка для
каждого home этой когорты, а `fleet_capacity_*`/`fleet_remaining_*` — её размер и текущий остаток
по всему cohort. Формула point estimate:

```text
capacity_per_home_nanocredits =
  100_000_000 × Σobserved_spend_nanocredits / Σobserved_fraction_units
```

`measured_homes` показывает число contributors, `homes_total` — размер когорты. Low/high —
консервативный общий envelope; если хотя бы один contributor не даёт конечную верхнюю границу,
cohort high тоже `null`. Per-home `capacity_nanocredits` не перезаписывается и остаётся raw audit
evidence, поэтому его разброс при whole-percent quota ожидаем. `window_totals` также остаётся суммой
individual estimates. API USD нельзя брать из `plan_cohorts`: он зависит от workload и считается
через conversion formula ниже.

Корень ответа также публикует `conversion_models`: versioned API/credit rates, независимые Fast
multipliers и long-context modifiers. Все деньги и credits сериализуются decimal strings; токены,
проценты, timestamps и counters — числа. Email — только bounded mask без домена. UI обязан считать
workload conversion через BigInt:

```text
API equivalent nanoUSD = capacity_nanocredits × workload_api_nanousd / workload_nanocredits
```

Reasoning — diagnostic subset output и не прибавляется второй раз. Cache-write имеет отдельную API
ставку, но в credit card входит во fresh input.

`GET /gemini-subs` публикует canonical `capacity_nano`/`remaining_nano` fleet totals,
`conversion_models` из `metering::gemini` и `quota_model_ids` для join публичной модели с её
Antigravity effort buckets. `remaining_amount` сериализуется decimal string; если Google отдаёт
только `remaining_fraction`, количество токенов/units остаётся неизвестным и не выводится из
workload-dollar blend. Профиль содержит только bounded email hint (четыре символа local-part без
домена); full email, subject, project, private tier, proxy и OAuth не сериализуются.

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
- **mult_bp** — текущий legacy scalar в basis points: `2000 = ×0.20`. После Stage 9 клиентская
  цена берётся из immutable release/policy rule; scalar остаётся migration/audit source и не
  является fallback. Service использует отдельный `meter_only`, а не `mult_bp=0`.

B2C/B2B/OpenKeys физически не могут уйти в минус: если денег не хватит, движок урежет ответ под
баланс или вернёт `402`. Service — явное исключение: official usage учитывается durable, но баланс
не резервируется и не дебетуется.

---

## 4. Канонические сценарии

### A. Регистрация клиента
1. Юзер регистрируется у тебя на сайте → ты создаёшь запись в СВОЕЙ БД.
2. `POST /admin/account` → движок вернёт `account_id` (`acct_…`). Сохрани его у себя рядом с юзером.
   Повтор с тем же непустым `handle` вернёт тот же аккаунт, поэтому восстановление регистрации
   идемпотентно и не создаёт осиротевшие аккаунты.
3. `POST /admin/key` c этим `account_id` → движок вернёт **`sk-pool-…`**. Для strict-аккаунта
   запрос обязан включать exact `activation_policy_ack` из применённой active policy. Покажи ключ
   юзеру **один раз** (это его API-ключ, секрет). У аккаунта может быть много ключей.

### B. Оплата → зачисление (ИДЕМПОТЕНТНО!)
1. Юзер платит → твой платёжный провайдер шлёт тебе **вебхук**.
2. Ты валидируешь вебхук и зовёшь `POST /admin/account/{id}/credit` с `amount_nano` (строка) и
   `ref` = **provider-qualified id транзакции** вида `<provider>:<transaction-id>`
   (например, `stripe:pi_123`).
3. Движок зачислит **идемпотентно по `ref`**: если провайдер доставит вебхук дважды — второй раз
   **НЕ задвоит** (вернёт тот же баланс). Положительное зачисление БЕЗ provider-qualified `ref`
   отклоняется с `400` — это гарантия, что одинаковые transaction id разных провайдеров не
   столкнутся в глобальном UNIQUE-индексе.

### C. Личный кабинет (баланс/ключи/история)
- Баланс/траты: `GET /admin/account/{id}` → `balance_nano`, `spent_nano`, `reserved_nano`.
- Список ключей юзера: `GET /admin/account/{id}/keys` (не-секретный `key_id`, маска,
  label/status/расход).
- История платежей/трат: `GET /admin/account/{id}/ledger?limit=50` (топапы/списания сверху).
- Разбивка расхода по моделям/токенам: `GET /admin/account/{id}/usage?window=30d` (для дашборда).

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
POST /admin/accounts/query              {"account_ids":["acct_…"]}   → 200 {accounts:[{account,
                                                                            balance_nano,spent_nano,
                                                                            reserved_nano,balance,mult_bp,
                                                                            status,handle}]} (1..500 id;
                                                                            400 при пустом/невалидном списке)
GET  /admin/account/{id}                                             → 200 {account, balance_nano, spent_nano,
                                                                            reserved_nano, balance, mult_bp, status, handle,
                                                                            funding:{...}} | 404
POST /admin/account/{id}/credit         {"amount_nano": "25000000000",
                                         "ref": "<provider>:<tx>"}   → 200 {account, balance_nano, balance} | 400 | 404 | 409
                                        (только amount_nano — десятичная строка i64 в nano;
                                         неизвестные поля отклоняются 422. Идемпотентно по ref;
                                         для положительной суммы ref ОБЯЗАТЕЛЕН в формате
                                         <provider>:<transaction-id>; сумма < 0 = debit/коррекция,
                                         ref для неё необязателен; 409 — ref уже использован
                                         другим платежом)
POST /admin/account/{id}/status         {"status":"active"|"disabled"}  → 200 {account,status,updated} | 404
POST /admin/account/{id}/pricing        {"mult_bp":0..10000}             → 200 {account,mult_bp,updated} | 404
GET  /admin/account/{id}/keys                                        → 200 {keys:[{key_id,key_masked,label,status,
                                                                            spent_nano,spent,reserved_nano,
                                                                            spend_limit_nano,expires_ts,
                                                                            created_ts,last_used_ts}]}
GET  /admin/account/{id}/ledger?limit=N[&after_id=ID]                 → 200 {entries:[{id,kind,request_id,
                                                                            amount_nano,ref,ts,provider,
                                                                            official_nano,attribution,
                                                                            funding_allocations,...}]}
POST /admin/account/{id}/ledger/ack     {"last_id": "12345"}          → 200 {account, consumer:"pricing",
                                                                            last_id} | 400
                                        (durable watermark для consumer="pricing": десятичная строка
                                         неотрицательного integer; retention удаляет старую
                                         charge-детализацию только ниже watermark'а)
GET  /admin/account/{id}/usage?window=30d                            → 200 {account, window, since_ts,
                                                                            until_ts, requests,
                                                                            total_official_nano,
                                                                            total_charged_nano,
                                                                            buckets:{...}, models:[...],
                                                                            daily:[...],
                                                                            daily_providers:[...],
                                                                            keys:[...]} | 404
                                        (window = <n>d | <n>h | all; по умолчанию и при
                                         нераспознанном значении — 30d)
```

Without `after_id`, ledger entries are the newest bounded history. With `after_id`, entries are
returned oldest-first with `id > after_id`; this is the durable worker cursor for usage attribution,
funding validation and referral commission. It is no longer a tier/progressive-pricing authority in
the target contract.

`funding` читается вместе со scalar account aggregates из одного snapshot. Он содержит
`account_class`, `funding_enforcement`, `reconciliation_state`, `bucket_count` и для
`balance/reserved/spent` отдельные `paid_*_nano`, `bonus_*_nano`, `other_*_nano` и
`unattributed_*_nano`. В текущем schema `bonus` может ссылаться на исторический
`welcome_track_bonus`; target writers создают provider-independent `welcome_bonus`, доступный любой
B2C-модели. `paid` означает durable paid funding. Online Stage 6 классифицирует exact welcome
остаток, а весь прочий legacy residual — paid по утверждённому контракту; ручной reviewer artifact
не используется.

Новая ledger row сохраняет expand-совместимые top-level `request_id`, `provider` и `official_nano`.
`attribution` равен `null` для исторической строки без `attribution_schema_version`; иначе он
переносит сохранённые snapshot/policy/rule/catalog/switch/tariff/eligibility/runtime-manifest поля,
`official_cost_json`, категориальные funding totals и исходный `funding_allocation_json` без
повторного resolve. `funding_allocations` всегда является массивом нормализованных durable
allocations (`bucket_id`, `source_type`, `source_ref`, `bucket_version`, `direction`, `amount_nano`,
optional `allocation_order`); старые строки честно возвращают пустой массив. Все `*_nano`, ledger
IDs и generations остаются integer JSON values; `packages/contracts` нормализует их в decimal
strings до попадания в JavaScript business logic.

`GET /admin/account/{id}/usage` агрегирует сохранённые immutable компоненты settlement за
фиксированный полуинтервал `[since_ts, until_ts)` — это НЕ пересчёт по текущему прайсу. Все
`*_nano` в ответе — decimal strings; токены, requests и timestamps — числа. `buckets` раскладывает
official-стоимость на `input`, `output`, `cache_read`, `cache_write`, `web_search`; строки, которые
нельзя честно разнести по компонентам (legacy), попадают в `unattributed_legacy`, и сумма всех
buckets всегда равна `total_official_nano`. `total_charged_nano` — сколько фактически списано с
аккаунта после мультипликатора. `models`, `daily`, `daily_providers` и `keys` дают ту же разбивку
по моделям, дням, провайдерам и маскированным ключам.

### Ключи доступа
```
POST /admin/key                         {"account_id", "label"?,
                                         "spend_limit_nano"?, "expires_ts"?,
                                         "activation_policy_ack"?: {
                                           "effective_policy_version": integer,
                                           "policy_digest": string
                                         }}
                                                                     → 200 {key:"sk-pool-…", key_id:"key_…", account,
                                                                            label,spend_limit_nano,expires_ts}
                                                                       | 400 | 409  (key виден 1 раз!)
POST /admin/key-id/{key_id}/status      {"status":"active"|"disabled",
                                         "activation_policy_ack"?: {
                                           "effective_policy_version": integer,
                                           "policy_digest": string
                                         }} → 200 {key_id,status,updated} | 400 | 404 | 409 (рекомендуется)
POST /admin/key-id/{key_id}/label       {"label":"…"}                → 200 {key_id,label,updated} | 400 | 404
                                        (1..64 символа после trim)
POST /admin/account/{id}/key-id/{key_id}/policy
                                        {"spend_limit_nano":string|null,
                                         "expires_ts":integer|null}
                                                                     → 200 {key_id,spend_limit_nano,
                                                                            expires_ts,updated} | 404 | 409
```

`key_id` не даёт доступа к `/v1` и безопасен для хранения в коммерческой PostgreSQL. Новый backend
должен отзывать ключ по `key_id`, чтобы никогда не сохранять пригодный к использованию `sk-pool-…`.
Полный ключ никогда помещается в URL; legacy endpoint удалён.

Для strict-аккаунта выпуск нового ключа и перевод disabled-ключа обратно в `active` требуют ACK,
который дословно совпадает с `effective_version` и `content_digest` текущей active policy.
Отсутствующий, устаревший или неверный ACK возвращает `409`; синтаксически допустимый, но
невалидный identity (неположительная версия, пустой/необрезанный digest) — `400`. Отключение ключа
не требует ACK. Для legacy/shadow-аккаунта поле необязательно, но если оно передано, движок всё
равно проверяет exact match и не принимает двусмысленное подтверждение.

`spend_limit_nano` is an optional positive decimal string and caps lifetime charged platform spend
for that key. `expires_ts` is an optional future Unix timestamp in seconds. The engine enforces both
again inside the atomic reservation transaction, including in-flight holds, so concurrent requests
cannot cross a key's cap. `NULL` means unlimited/no expiration and preserves legacy behavior.
The policy endpoint is an account-scoped full replacement: both nullable fields are required.
It can increase or clear a limit and extend or clear expiry without changing key status. A new
limit below `spent_nano + reserved_nano` is rejected atomically with `409` and code
`limit_below_committed`, so an edit cannot invalidate an in-flight reservation.

### Versioned multi-provider pricing (Stage 3C)

Pricing control is an explicit `prepare → read → activate` protocol. Preparing an immutable
version never changes traffic. Activation is a monotonic compare-and-set (CAS), and callers must
send the exact expected active target. Catalog, switches, and account policy are separate heads;
the supported order for a new release is catalog first, then switches, then policy.

```
POST /admin/pricing/catalog/prepare
GET  /admin/pricing/catalog/{product_id}/version/{generation}
GET  /admin/pricing/catalog/{product_id}/active
POST /admin/pricing/catalog/{product_id}/activate

POST /admin/pricing/switches/prepare
GET  /admin/pricing/switches/version/{generation}
GET  /admin/pricing/switches/active
POST /admin/pricing/switches/activate

POST /admin/pricing/policy/prepare
GET  /admin/pricing/policy/{account_id}/version/{effective_version}
GET  /admin/pricing/policy/{account_id}/active
GET  /admin/pricing/policy/{account_id}/state
POST /admin/pricing/policy/{account_id}/activate
```

Prepare bodies are the complete immutable `PricingCatalogSpec`, `ProviderSwitchSpec`, or
`AccountPolicySpec`. They include schema/capability generations and digests, content digest, full
entries/rules, and all policy lineage pins. Unknown JSON fields are rejected. A prepare ACK returns
`result=stored|unchanged` and echoes the complete immutable identity; the same version with a
different body is `409 version_conflict`.

Catalog activation repeats the complete prepared immutable spec rather than only its compact
version/digest target:

```json
{
  "catalog": {
    "product_id": "main",
    "generation": 2,
    "schema_version": 1,
    "capability_generation": 4,
    "capability_digest": "sha256:capability...",
    "content_digest": "sha256:catalog...",
    "entries": []
  },
  "expectation": {"exact": {"version": 1, "content_digest": "sha256:..."}}
}
```

Switch activation likewise sends `switches` with the complete prepared `ProviderSwitchSpec` plus
the CAS `expectation`. Use `"expectation":"absent"` only for the first catalog or switch head.
Account-policy activation sends the complete prepared `policy`, the complete target `binding`, and
`expectation="unbound"|{"inactive":...}|{"exact":...}`. Catalog `product_id` and policy
`account_id` must match the URL. Before CAS, the engine reads the named immutable generation and
requires exact spec equality; a missing prepared version is `missing_dependency`, while different
content under the same version is `version_conflict`.

Successful activation returns `result=applied|unchanged`. Exact retry after a lost ACK returns
`unchanged` for the same committed target. Its identity echoes the complete catalog/switch spec, or
the complete policy plus derived binding target, together with the expectation. Rejections are
typed and retain evidence:

- `400 invalid` — malformed schema, rules, identity, binding, or unsupported strict state;
- `409 missing_dependency` — required prepared/active catalog or switches are absent;
- `409 stale` — target is older than durable state;
- `409 version_conflict` — same version has another digest/content;
- `409 cas_mismatch` / `policy_cas_mismatch` — expected head/binding differs; response includes
  the actual durable state;
- `423 locked` — immutable legacy policy cannot be replaced.

`GET .../state` reads the live scalar, policy binding, pinned policy catalog/switches, and current
admission catalog/switches in one database snapshot. Stage 3C does not backfill data, issue keys,
enable strict enforcement, or bypass the catalog → switches → policy order.

### Pricing release v2: producer and activation surface

Engine PostgreSQL exposes an additive producer-first surface for the immutable release/funding-v2
authority. Immutable preparation remains traffic-neutral; activation is a separate evidence-gated
operation:

```text
POST /admin/pricing/v2/policy/prepare
GET  /admin/pricing/v2/policy/{policy_id}/version/{policy_version}
POST /admin/pricing/v2/release/prepare
GET  /admin/pricing/v2/release/{generation}
POST /admin/pricing/v2/recovery-link/prepare
GET  /admin/pricing/v2/recovery-link/{target_generation}/{recovery_generation}
POST /admin/pricing/v2/assignment-extension/prepare
GET  /admin/pricing/v2/assignment-extension/{head_version}/{account_id}
POST /admin/pricing/v2/stage8-evidence/capture
POST /admin/pricing/v2/activate
GET  /admin/pricing/v2/head
GET  /admin/pricing/v2/provisioning-context
GET  /admin/pricing/v2/inventory?after_account_id=<id>&limit=500
GET  /admin/pricing/v2/funding/{account_id}/normalization
POST /admin/pricing/v2/funding/{account_id}/normalization
```

Policy/release/link/assignment-extension rows are append-only; policy and release identities are
monotonic by policy version or release generation. Prepare returns the same typed
`stored|unchanged|stale|version_conflict|missing_dependency|invalid` result envelope as Stage 3C.
`GET .../head` returns `{ "head": null }` until a protected consumer submits a fresh passed Stage 8
identity to the activation route. Prepare routes cannot move the global head, mutate an immutable
release manifest or change balances.
An assignment extension can make one post-cutover account resolvable under an already-active head;
the provisioning consumer must therefore complete its exact readback before issuing or enabling a
usable customer key.

`GET /admin/pricing/v2/provisioning-context` is the post-cutover discovery authority for account
provisioning outside the commerce database. It returns `{ "context": null }` before cutover. After
cutover, `context` is materialized in one PostgreSQL `REPEATABLE READ READ ONLY` snapshot:

```text
head = { active_generation, active_digest, head_version, updated_ts }
activation = { activation_id, activation_kind=cutover|recovery,
               evidence_digest, activated_ts }
active_release = {
  generation, release_kind, schema_version,
  capability_generation, capability_digest,
  main_catalog_generation, main_catalog_digest,
  openkeys_catalog_generation, openkeys_catalog_digest,
  switch_generation, switch_digest,
  inventory_digest, funding_manifest_digest,
  minimum_runtime_schema_version, content_digest
}
paired_recovery? = { release=<same projection>, recovery_link=<exact immutable link> }
```

The producer joins the exact head-version activation audit to its persisted passed Stage 8
evidence, verifies target/recovery identities, immutable runtime/funding lineage, base funding
assignment parity and the evidence-selected recovery link. Any disagreement returns authority
unavailable; it never falls back to an arbitrary prepared link. An active target has exactly one
`paired_recovery`; an active recovery has `paired_recovery=null` because no later pair has been
confirmed by that head transition. The projection deliberately omits full base assignments: the
account-specific extension remains the sole post-cutover write contract.

`PricingReleasePolicyV2` has the following exact shape (all unknown fields are rejected):

```text
policy_id, policy_version, owner_type, owner_id, account_class,
product_id?, billing_mode, schema_version=2,
capability_generation, capability_digest,
catalog_generation?, catalog_digest?, switch_generation?, switch_digest?,
content_digest,
rules[] = { rule_id, rule_digest,
            scope = { scope=global }
                  | { scope=provider, provider_id }
                  | { scope=model, provider_id, canonical_model_id },
            discount_bps, payable_multiplier_bp }
```

Each rule's outer `scope` field contains the strict tagged snake-case object shown above; provider
and model identity are inside that object, not sibling rule fields. Engine validation requires
`payable_multiplier_bp = 10000 - discount_bps`, one global rule for B2C, no global rule for B2B,
and one global zero-discount rule for OpenKeys. Service policy is rule-free, has no product/catalog/
switch pins and uses only `billing_mode=meter_only`.

`PricingReleaseV2` has:

```text
generation, release_kind=target|recovery, schema_version=2,
capability_generation, capability_digest,
main_catalog_generation, main_catalog_digest,
openkeys_catalog_generation, openkeys_catalog_digest,
switch_generation, switch_digest,
inventory_digest, policy_manifest_digest, assignment_manifest_digest,
funding_manifest_digest, minimum_runtime_schema_version, content_digest,
assignments[] = { account_id, account_class, policy_id, policy_version, policy_digest,
                  billing_mode, funding_generation?, purpose?, responsible?,
                  assignment_digest }
```

Prepare runs under the release control-plane advisory lock and rejects any release whose unique
assignments do not equal the exact full engine inventory, including both `active` and `disabled`
accounts. Keeping disabled accounts in the immutable graph guarantees that a later enablement
cannot expose an account without its prepared policy/funding identity. Every balance assignment must
reference an existing funding generation; every service assignment is `meter_only`, has no funding
generation and includes non-empty `purpose`/`responsible`. Main/OpenKeys catalogs, switches,
policies and all digests must already exist with matching capability lineage. A recovery link binds
a prepared `target` generation to a strictly newer prepared `recovery` generation.

`PricingReleaseAssignmentExtensionV2` is the post-cutover provisioning shape (all unknown fields
are rejected):

```text
provisioning_head_generation, provisioning_head_digest, provisioning_head_version,
paired_recovery_generation?, paired_recovery_digest?, extension_group_digest,
members[] = {
  release_generation,
  assignment = { account_id, account_class, policy_id, policy_version, policy_digest,
                 billing_mode, funding_generation?, purpose?, responsible?, assignment_digest },
  extension_digest
}
```

Prepare takes the same pricing-release control-plane advisory lock as activation and accepts
only the exact current head. If that active target's activation evidence selected a recovery link,
`members` must contain that exact atomic active/recovery pair; another prepared link or an omitted
pair returns typed `missing_dependency`. An active recovery contains exactly the active member.
Both members must name
the same account, policy, class, billing mode, funding generation and service metadata while keeping
their own release generation, assignment digest and extension digest. The account must already
exist, must be absent from both immutable base assignment manifests, and every policy/funding
dependency must exist. Balance accounts take the account funding lock and require the assignment
generation to be the exact active funding head; service accounts remain `meter_only` with no
funding generation.

An exact replay returns `unchanged`, a different body for the same
`(provisioning_head_version, account_id)` returns `version_conflict`, and a request for a head that is
no longer current returns typed `stale` without inserting either member. `GET` performs exact
readback by that tuple. Runtime resolution reads one coherent base assignment or append-only
extension for the active release; it never mutates the immutable release manifest. This surface does
not create or activate a head.

`POST /admin/pricing/v2/stage8-evidence/capture` is the producer-first machine transport for the
same schema-v2 report as `claude-api db stage8-evidence`. Its body is strict and contains only
explicit capture inputs; caller-supplied runtime evidence is rejected:

```json
{
  "target_generation": 41,
  "recovery_generation": 42,
  "window_start_ts": 1785700000,
  "window_end_ts": 1785700300,
  "min_samples_per_provider": 100,
  "financial_sample_size": 100,
  "gemini_client_admissions": 27
}
```

Target must be positive and recovery strictly newer. The window is a positive non-empty half-open
past interval; provider minimum is `1..=1000000`, financial sample size `1..=1000`, and Gemini
admissions is a nonnegative external aggregate. The server attaches its compile-fixed
`PricingRuntimeManifestEvidence` from `AppState`; the HTTP caller cannot choose runtime capability
lineage. A bounded `AsyncBilling` reader executes the existing PostgreSQL `REPEATABLE READ READ
ONLY` collector. It never enters the billing writer and cannot update a release head, account,
funding bucket, balance, reservation, ledger, activation evidence or traffic state.

A successfully captured report is the unwrapped schema-v2 JSON object with HTTP `200` regardless
of its `passed` value. In particular, `passed=false` plus `blockers[]` is valid durable evidence and
must be persisted by the consumer rather than translated into a transport failure.
Malformed bounds are `400`, a shape/type/unknown-field error is `422`, missing control auth is
`401`, and non-PostgreSQL or unavailable authority is `503`. The report contains signed-i64
nanoUSD JSON numbers. TypeScript consumers must read the response as raw text and parse it with
`json-bigint`; `response.json()` is forbidden because it can round those integers before evidence
digest verification.

After the exact producer SHA reached green `deploy/watchdog`, the commerce consumer was connected
through the strict `packages/contracts` schema and the sole
`EngineClient.capturePricingStage8EvidenceV2` transport. The client bounds the response to 16 MiB,
verifies the canonical integer-preserving shape and explicit request identity, and returns both the
parsed report and its exact raw bytes. `apps/worker` may call the producer only for a durable capture
job explicitly staged through the AdminGuard-protected commerce route
`POST /v1/admin/pricing-stage8-capture-v2/stage`; the paired GET is read-only status with bounded
freshness and sanitized blocker metadata, never raw subject identities. The worker
persists the untouched engine bytes before running the combined commerce/OpenKeys/service
collector, then atomically stores the combined bytes and terminal `passed|blocked` result. An
engine `passed=false` report is therefore a successful capture input. Retry/dead transitions are
bounded, stale leases are recovered, and at most one capture job is processing globally. Migration,
startup, polling and activation staging cannot infer or create a capture job; capture completion
cannot create an activation job or move the release head.

`POST /admin/pricing/v2/activate` is the only global live mutation. All unknown fields are rejected.
The initial cutover request has this exact shape:

```json
{
  "activation_kind": "cutover",
  "expectation": "absent",
  "evidence": {
    "evidence_digest": "sha256:v2:<combined-commerce-evidence>",
    "target_generation": 41,
    "target_digest": "sha256:v2:<engine-target-release>",
    "recovery_generation": 42,
    "recovery_digest": "sha256:v2:<engine-recovery-release>",
    "engine_inventory_digest": "sha256:v2:<engine-inventory>",
    "funding_digest": "sha256:v2:<funding-manifest>",
    "shadow_digest": "sha256:v2:<shadow-window>",
    "runtime_floor_digest": "sha256:v2:<runtime-floor>",
    "legacy_inflight_count": 7,
    "engine_captured_ts": 1785700000,
    "observed_ts": 1785700010,
    "valid_until_ts": 1785700310
  },
  "operator_id": "pricing-control-worker:<instance>",
  "reason": "activate exact prepared Stage 9 target"
}
```

The combined evidence TTL is at most 300 seconds; its source engine capture may be at most 120
seconds older than `observed_ts` (with at most five seconds of source clock skew). The protected
commerce consumer must first verify the canonical source engine digest, exact persisted
`passed=true` combined row, commerce/service/OpenKeys authority, job backlog and sales runtime.
`evidence_digest` is that combined-row audit identity, while target/recovery digests are the engine
release identities, not commerce plan digests.

For forward recovery, `activation_kind` is `recovery` and `expectation` is the complete exact target
head returned by the cutover:

```json
{
  "exact": {
    "active_generation": 41,
    "active_digest": "sha256:v2:<engine-target-release>",
    "head_version": 1,
    "updated_ts": 1785700012
  }
}
```

Cutover is accepted only from an absent head and only to the prepared target. Recovery is accepted
only as a monotonic CAS from that exact target to its linked newer recovery. Engine runs one
`SERIALIZABLE` transaction under `pricing-release-v2:control-plane`, locks the singleton head,
re-reads the immutable pair/link and active catalog/switch heads, and independently recomputes the
base inventory, funding manifest/parity and live runtime-floor digest. Every live instance must
claim release/funding schema v2, the exact compile-fixed runtime digest and its own current owner
epoch. A recovery also proves that any account created after cutover has the exact atomic
target/recovery assignment extension. With the exact target head active, the Stage 8 engine report
keeps the immutable base inventory identity and validates every later account through that paired
extension and its live funding head, so fresh recovery evidence remains obtainable after the
original 300-second cutover proof expires. Only then does the transaction append the evidence row
and activation audit and insert/update one head row. It does not write accounts, balances, funding
lots, reservations, ledger or usage rows, and it does not take data-plane request locks.

Success is `200` with `result=applied|unchanged` and an `activation` receipt containing the durable
activation id/kind, from identity, expected head version, resulting complete head, evidence digest,
operator/reason and timestamp. Exact replay of the same committed request returns `unchanged` from
the durable audit, including after its original evidence TTL elapsed. Rejections roll back the
whole transaction and return `result=rejected` with one typed code:

- `400 invalid`;
- `409 missing_dependency`, `cas_mismatch`, `evidence_stale`, `evidence_conflict`;
- `409 release_lineage_drift`, `authority_drift`, `inventory_drift`, `funding_drift`,
  `funding_invariant_drift`, `runtime_floor_drift` or `runtime_incompatible`.

After the producer SHA reached green `deploy/watchdog`, `packages/contracts` added the strict
request/receipt/rejection schemas, `packages/engine-client` added the sole typed transport, and
`packages/db/src/pricing-release-activation-jobs.ts` plus `apps/worker` added a durable consumer.
The worker can call this route only after an explicit immutable activation job exists. No API,
startup hook, migration or Stage 8 collection automatically stages that job; a deployed consumer
with an empty queue cannot create a head.

Inventory is ordered by `account_id`, returns at most 500 rows plus `next_after_account_id`, and
contains status, legacy scalar, integer balance/reserved/spent and nullable funding-v2 head identity.
It contains no key secret. Consumers must exhaust the cursor and join this engine inventory with the
authoritative commerce/OpenKeys inventories; a partial page is never release evidence.

Funding normalization is an account-local content-addressed producer and cannot activate pricing.
`GET .../normalization` returns:

```text
normalization = {
  account_id, account_status,
  status = ready|blocked|normalized,
  source = aggregate_paid_only|ledger_replay|legacy_buckets|stored_generation,
  source_state_digest = sha256:v2:...,
  normalization_digest?, funding_generation?, funding_head_version?,
  balance_nano, reserved_nano, spent_nano,
  lots[] = {lot_id, source_type=paid|welcome_bonus, source_ref,
            balance_nano, reserved_nano, spent_nano, version, status},
  blockers[] = {code, detail}
}
```

`POST` принимает strict body
`{expected_source_state_digest, expected_normalization_digest}`. Успешный ответ имеет
`result.status=stored|unchanged`; `stale|blocked|conflict` возвращают HTTP 409 вместе с заново
построенным `result.normalization`, неизвестный account — 404, malformed digest/body — 400/422.
Apply берёт тот же account funding lock, что reserve/settlement/top-up, и атомарно пишет generation,
lots и initial head. Legacy in-flight блокирует только свой account; writer, ожидавший lock,
перечитывает новый head и dual-write'ит уже в funding v2. Глобального drain нет.

Assignment-extension typed TS consumers подключены только после зелёного exact producer SHA.
`packages/contracts` strict-валидирует nullable provisioning context и exact active/recovery pair;
`packages/engine-client` является единственным typed transport и владельцем canonical Stage 5 v2
policy/assignment digest builder. При ненулевом context commerce, OpenKeys и service writers
завершают нужную account-local цепочку, exact policy/extension prepare+GET readback и свежую
финальную context-проверку до выдачи usable key или объявления service account доступным.
OpenKeys сначала кредитует номинал, затем normalizes funding и сохраняет глобальную 1:1 policy;
service policy rule-free, `meter_only`, без funding/catalog/switch pins и с обязательными
purpose/responsible. Активный target получает только evidence-selected recovery member, активный
recovery — один member. `apps/api` дополнительно повторяет commerce key-проверку после remote issue;
если head или authority изменились, ключ отключается до возврата raw secret. При `context=null`
consumer сохраняет pre-cutover путь и ничего release-v2 не materialize'ит, поэтому deploy сам не
запускает cutover.
Stage 8 evidence уже поддерживает zero-drain audit counts, а activation producer выполняет один
CAS. Strict contracts/client/durable worker consumer подключены producer-first: request хранится до
сети, complete ACK сохраняется до `confirmed`, timeout повторяет только exact body. Consumer не
создаёт job сам; до отдельного Stage 8 source-capture/control-plane checkpoint staging fail-closed
на nullable source fields. Data-plane reserve/settlement release control-plane lock не берут.
After each producer SHA reached a green exact-SHA `deploy/watchdog`, `packages/contracts` gained the
strict release, funding-normalization, assignment-extension and activation wire schemas, while
`packages/engine-client` gained typed prepare/read, account-local normalization/extension and the
single activation method. The bounded application jobs are separate `apps/worker` consumers:
it runs only for an explicitly staged target-release job, re-GETs exact plan digests before every
POST, excludes service `meter_only` accounts and confirms only complete funding-manifest coverage.
The activation lane likewise runs only for an explicitly staged immutable request and persists its
full receipt. Merely having a typed client or a deployed worker does not materialize an account or
move the release head.

### Коды ошибок
`400` неверное тело (явная валидация handler'а) · `401` нет/неверный control-ключ · `404`
аккаунт/ключ/версия не найдены · `409` CAS/версионный конфликт или лимит ниже уже
списанного+зарезервированного · `422` JSON синтаксически валиден, но не соответствует схеме тела
(неизвестное поле под `deny_unknown_fields`, неверный тип) · `423` immutable
pricing policy locked · `503` биллинг выключен или billing authority недоступен.
На клиентском `/v1`: `402` баланс ≤ 0.

### Пример: полный цикл (bash)
```bash
CTL=<CONTROL_KEY>; B=http://127.0.0.1:8790
AID=$(curl -s -XPOST $B/admin/account -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d '{"handle":"acme","mult_bp":2000}' | jq -r .account)
curl -s -XPOST $B/admin/account/$AID/credit -H "x-api-key: $CTL" -H 'content-type: application/json' \
     -d '{"amount_nano":"25000000000","ref":"stripe:pi_123"}'        # зачислить $25 (идемпотентно)
KEY=$(curl -s -XPOST $B/admin/key -H "x-api-key: $CTL" -H 'content-type: application/json' \
      -d "{\"account_id\":\"$AID\",\"label\":\"prod\"}" | jq -r .key)   # выдать клиенту
curl -s $B/admin/account/$AID -H "x-api-key: $CTL"               # баланс для кабинета
curl -s $B/admin/account/$AID/ledger -H "x-api-key: $CTL"        # история
```

---

## 6. Чего пока НЕТ (не блокирует старт)

- **Push-поток usage** движок→твой сервис отсутствует. Коммерческий worker опрашивает cursor-based
  `GET /admin/account/{id}/ledger?after_id=...` и подтверждает обработанный курсор через
  `POST /admin/account/{id}/ledger/ack`; доставка при этом идемпотентна.
- **Cross-host TLS/private networking** остаётся частью Фазы 3. Текущий HTTP Control origin доступен
  только через loopback на том же host и не публикуется Caddy наружу.
- Ротация `CONTROL_KEY` требует согласованно обновить engine, API и worker env, затем провести
  обычный engine/API blue-green и stop/start worker по `docs/ops/DEPLOYMENT.md`; одиночный ручной restart
  создаст окно с несовпадающими ключами.

Вопросы по контракту — они все закрываются этим движком; если чего-то не хватает для сайта,
допилим на нашей стороне.
