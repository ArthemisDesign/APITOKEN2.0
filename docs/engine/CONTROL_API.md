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

### Pricing release v2: producer prepare/read surface

Engine PostgreSQL exposes an additive producer-first surface for the immutable release/funding-v2
authority. It is intentionally limited to prepare and read operations:

```text
POST /admin/pricing/v2/policy/prepare
GET  /admin/pricing/v2/policy/{policy_id}/version/{policy_version}
POST /admin/pricing/v2/release/prepare
GET  /admin/pricing/v2/release/{generation}
POST /admin/pricing/v2/recovery-link/prepare
GET  /admin/pricing/v2/recovery-link/{target_generation}/{recovery_generation}
GET  /admin/pricing/v2/head
GET  /admin/pricing/v2/inventory?after_account_id=<id>&limit=500
GET  /admin/pricing/v2/funding/{account_id}/normalization
POST /admin/pricing/v2/funding/{account_id}/normalization
```

There is no activation route in this producer checkpoint. Policy/release/link rows are append-only
and monotonic by policy version or release generation. Prepare returns the same typed
`stored|unchanged|stale|version_conflict|missing_dependency|invalid` result envelope as Stage 3C.
`GET .../head` returns `{ "head": null }` until the separately reviewed
Stage 9 producer exists and a fresh full-inventory Stage 8 proof authorizes one global CAS. Therefore
deploying or calling the routes above cannot change live traffic, admission, prices or balances.

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
assignments do not equal the exact set of currently active engine accounts. Every balance assignment
must reference an existing funding generation; every service assignment is `meter_only`, has no
funding generation and includes non-empty `purpose`/`responsible`. Main/OpenKeys catalogs, switches,
policies and all digests must already exist with matching capability lineage. A recovery link binds a
prepared `target` generation to a strictly newer prepared `recovery` generation.

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

Поздние checkpoints подключают typed TS consumer, runtime release snapshots, Stage 8 evidence и
наконец один activation CAS. Account creation/activation и тот future CAS share the same
control-plane lock; data-plane reserve/settlement never takes it.
After each producer SHA reached a green exact-SHA `deploy/watchdog`, `packages/contracts` gained the
strict release and funding-normalization wire schemas and `packages/engine-client` gained typed
prepare/read plus account-local normalization plan/apply methods. The client surface still has no
activation method. The bounded full-inventory application job is intentionally a separate consumer
checkpoint; merely having a typed client does not materialize any account.

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
