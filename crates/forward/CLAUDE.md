# crates/forward — CLAUDE.md

**Роль:** прозрачный форвардинг `/v1/*` на api.anthropic.com (Шаг B) + поллер лимитов.
Сердце «неотличимости от оригинального API».

**Владелец-ветка:** `comp/forward`.

**Границы (жёстко):**
- Зависит от `pool`, `registry`, `metering`, `axum`, `wreq`, `redis`, `serde_json`, `futures-util`,
  `bytes`, `tokio`[sync,rt] + `anyhow` (для DB-актора биллинга).
- НЕ читает env и НЕ содержит CLI/управляющих роутов (`/health`, `/pool`, `/balance`) — это `server`.
- Конфиг получает готовым: [`ProxyConfig`] наполняет `server::config`; биллинг — async DB-актор `Option<Arc<AsyncBilling>>` в `AppState` (1 writer + N readers).

**Три класса авторизации (разделение секретов, `proxy.rs`):** `authed` (forwarding-admin: `api_keys`
/loopback) ⊂ `control_authed` (+`control_keys` — для `/admin/*` коммерции) ⊂ `readonly_authed`
(+`panel_keys` — read-only дашборды). Все — constant-time (`ct_eq`, fold без short-circuit). Control-
ключ НЕ форвардит `/v1` (не админ, не метерный → 401). `AsyncBilling` расширен control-командами
(`create_account`/`issue_key`/`account_status`/`key_status_by_id`) через ТОТ ЖЕ single-writer (без гонок).
Pricing sync uses the same actors: multiplier writes go through the writer and cursor ledger reads
through a reader; HTTP code never opens the authority directly.

**Биллинг (async, `billing.rs` + tee-метеринг `meter.rs`):** авторизация (`authorize`, async):
env-админ проверяется ПЕРВЫМ в памяти; иначе клиентский ключ → `key_account` (JOIN ключ→аккаунт)
→ баланс АККАУНТА (≤0 → 402). Баланс/резерв/наценка — на аккаунте (общий на все ключи юзера).
Все DB-операции идут через `AsyncBilling` (DB-акторы: 1 writer + N readers; sync PostgreSQL/legacy
SQLite живут на выделенных потоках, НЕ на async-воркерах). Generated request ID создаётся до reserve;
успешная доставка помечается durable до передачи стрима; finalize кладёт idempotent settlement в
outbox, а writer retry-ит до commit. RAII cancel закрывает именно этот request ID. Резерв под баланс
с урезанием `max_tokens` (`cap_to_balance`)
→ клиент не получит ни токена/цента сверх баланса. 4xx/ошибки/ротация НЕ тарифицируются.
Для policy-ключей cap берёт минимум из баланса аккаунта и оставшегося lifetime-лимита. Такие ключи
обходят auth TTL cache; срок и лимит повторно проверяются в атомарной транзакции reserve.

**Что внутри:** `ProxyConfig`, `AppState`, `Clients` (кэш http-клиентов по прокси),
`limits_from_headers`/`Limits` (unified-ratelimit из ответа), `poll_sub` (активный опрос idle),
`detect_plan` (тариф из /api/oauth/profile), `forward` (axum-хендлер), `authed`.

**Cache-first роутинг без client opt-in (`affinity.rs`):** tenant = metered `account_id` (все ключи
аккаунта разделяют кэш) или отдельный admin scope. `AffinityStore::infer` считается ДО identity-инжекта:
native `x-claude-code-session-id`/conversation header имеет приоритет; для любого обычного API-клиента
строятся rolling keyed-хэши каждого канонического message-prefix вместе с cache-shape
(`model/system/tools/thinking/context_management`). Запрос сохраняет все свои prefix aliases, поэтому
следующий растущий turn находится по самому глубокому уже известному префиксу без разбора ответа.
Большой/явно cache-controlled общий `system+tools` даёт низкоприоритетный cache-root hint для новой
conversation; она сразу fork-ается в отдельный session lineage и проходит мягкий placement cap.
Local L1 всегда включён; optional Redis L2 делит TTL-привязки между slots. Redis хранит только keyed
digests tenant/native/transcript/subscription и fail-open: сеть/timeout/eviction не участвуют в auth,
money или capacity. Первая попытка = `pool.route_affinity` (place/pin/brief wait/spill/rebind), ретраи =
`pool.pick`. PostgreSQL capacity lease ниже остаётся авторитетным. SSE по-прежнему byte-for-byte.
In-flight держится всю жизнь стрима: успех → `mark_healthy`, `end_stream` из tee-метеринга (`meter.rs`)
снимает слот на завершении/обрыве; 4xx → `mark_ok`.

**Ротация/лимиты (устойчивость пула):**
- **Пассивный сбор:** на КАЖДОМ ответе апстрима вытаскиваем unified-ratelimit (`limits_from_headers`)
  → `pool.set_util`. Так util/reset всегда свежи из боевого трафика; активный `poll_sub` (server)
  добивает лишь простаивающие подписки (обновлённый `polled_ts` сам это гейтит). Экономит квоту.
- **Классификация вины (не студить подписку за чужое):**
  - `429` → квота подписки → `mark_cooling(cool_secs_429)`: `Retry-After` → окно-виновник
    (`util7d≥0.95` → `reset7d`, иначе `reset5h`) → burst-дефолт. Не студим на 5h, если выбит 7d.
  - `401/403` → мёртвый/битый токен, НЕ транзиент → `mark_cooling(AUTH_QUARANTINE=900s)` + лог
    «нужен refresh». Иначе долбили бы забаненный аккаунт раз в 10с (ban-signal) и жгли слот попытки.
  - `5xx/408/409/425/сетевой` → вина АПСТРИМА → `mark_done` (слот −1 БЕЗ cooling: подписка здорова)
    + `breaker.record_fail`. Битый прокси (build err) → короткий `mark_cooling(10)` (локально).
  - `2xx/4xx` → апстрим здоров → `breaker.record_ok` (сброс окна фейлов).
- **Circuit breaker (`breaker.rs`):** размыкается, когда в окне зафейлило ≥ N РАЗНЫХ подписок
  (distinct email, а не сырой счёт) — это признак аутейджа api.anthropic.com, тогда как одна
  флаки-прокси/poison даёт фейлы одного email и breaker НЕ трогает. Пока разомкнут — вход отбивает
  `503 + Retry-After` (не веерим по пулу). `record_fail(now, email)`; на 2xx/4xx `record_ok` сбрасывает.
- **Бюджет ротации (ошибка подписки клиенту НЕ идёт):** 429/401/403 — вина конкретного аккаунта
  (бан/лимит), бюджет `max_tries` НЕ тратят → крутимся по всему флоту (пул сам исключает cooling), пока
  не найдём здоровую. Бюджет тратят только BACKEND-фейлы (5xx/сеть — аутейдж). Верхний предел итераций
  = «весь флот + запас».
- **Исход при провале всех попыток:** упёрлись в backend-бюджет → отдаём последнюю upstream-ошибку
  (аутейдж; breaker вот-вот разомкнётся); все подписки за лимитом → `429 + Retry-After = soonest_ready`
  (клиент откатится сам — именно это, а не ошибка отдельной забаненной подписки); пул пуст → `503`.
  Каждый `mark_used` парен с `InflightGuard`/`end_stream`/`mark_done`.

**Антифингерпринт флота (`persona_ua`):** флот из 100 байт-в-байт одинаковых UA — сам по себе
отпечаток. `persona_ua(cfg, email)` даёт **стабильный во времени** для подписки, но **различный между
подписками** UA: пул задан списком (`user_agents` len>1) → пиним по hash(email); иначе варьируем
patch-версию базового UA на `ua_spread`. Клиентский `user-agent` НЕ пробрасываем (в `skip_req_header`)
— отпечаток наш. Тот же UA идёт и в `poll_sub`/`detect_plan` (здоровье персоны = тот же отпечаток,
что и бой). Identity/beta/anthropic-version НЕ варьируем — они корректностные (нет ground-truth на
правдоподобные альтернативы). Env: `CLAUDE_API_UA` (один или список), `CLAUDE_API_UA_SPREAD`.

**Fair-share (`keylimiter.rs`):** `KeyLimiter` кап-ирует ОДНОВРЕМЕННЫЕ запросы на метерный ключ
(`max_inflight_per_key`, деф 20; 0=выкл) — баланс ограничивает суммарный расход, но не одновременность,
и без этого один «кит» бёрстом залил бы флот, оставив остальных на 429. Слот держит `KeyGuard` всю
обработку запроса (освобождает на любом исходе/отмене). Превышение → `429 slow down` + метрика
`key_throttled`. Админ (env-ключ/localhost) — без лимита.

**RAII-гарды на отмену запроса (критично):** клиент рвёт соединение → future хендлера дропается на
`await`; без гардов `mark_used(+1)` и `reserve(hold)` НЕ откатывались бы (утечка ёмкости персоны +
денег клиента навсегда). `InflightGuard` (Drop → `mark_done`) закрывает слот на любом не-стриминговом
исходе И при отмене; разоружается на успехе (слот держит стрим → `end_stream`). `HoldGuard` (Drop →
`settle(hold,0)`) возвращает резерв на любом не-успешном исходе И при отмене; разоружается на успехе
(hold закрывает tee-метеринг фактикой). Поэтому `mark_cooling`/`mark_healthy` in-flight НЕ трогают —
владелец слота один. Breaker кормится максимум раз на запрос (анти-DoS от poison-запроса).

**Инварианты прозрачности (критично — не ломать):**
1. Ответ апстрима отдаётся клиенту **байт-в-байт** (включая SSE-стрим). Не буферизировать,
   не переписывать тело.
2. Под капотом: инжект Claude Code identity ПЕРВЫМ system-блоком + `anthropic-beta: oauth-…` +
   `Bearer` подписки. Клиентский `system` сохраняется вторым блоком. Без identity Anthropic не
   пускает OAuth-токены подписок — но клиент об этом знать не должен.
3. **Ротация только ДО начала стрима:** решение по статусу (429/401/403/5xx → cooling + следующая
   подписка) принимается до отдачи тела. Как только начали стримить — не переключаемся.
4. Клиентские ошибки запроса (400/404/422 …) пробрасываются как есть, БЕЗ ротации.
5. Заголовки авторизации клиента (`x-api-key`/`authorization`) НЕ уходят апстриму — заменяются
   на Bearer подписки. Токены не логировать.
6. **Санитайзер синтетических ошибок (`LocalErr`/`local_err` в `proxy.rs`) — ЕДИНСТВЕННАЯ точка,
   где рождаются НАШИ ответы клиенту.** Клиент считает, что говорит с api.anthropic.com, поэтому в
   `error.type`/`message` НЕ должно быть наших внутренностей (`subscription/pool/upstream/authority/
   cooling/persona/fleet/oauth`). Внутренняя причина живёт только в метриках и `eprintln`-логе, НЕ в
   теле. Публичные триплеты — аутентичные Anthropic: `overloaded_error`=**529**, `api_error`=500,
   `rate_limit_error`=429, `authentication_error`=401, `not_found_error`=404, `request_too_large`=413.
   Нехватка ёмкости/пул пуст/breaker/authority/сбой апстрим-соединения → обезличенный retryable
   `Overloaded`/`RateLimited`. Законные ошибки состояния аккаунта клиент ЗНАТЬ должен и они остаются:
   `InvalidKey` (401), `LowBalance` (**402**, контракт docs-portal). Новую ошибку добавляй ТОЛЬКО как
   вариант `LocalErr` (не сырой `err_response`); regression-тест `local_err_never_leaks_*` это гейтит.


**Тюнинг под живой Anthropic** (identity/beta/UA/version) — через поля `ProxyConfig`, которые
`server` берёт из env. Значения по умолчанию — в `config.rs`.

**Проверка:** `cargo build -p forward`; полный smoke — через бинарь против мок-апстрима.
