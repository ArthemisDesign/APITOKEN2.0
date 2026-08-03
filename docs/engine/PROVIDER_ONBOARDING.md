# Онбординг нового subscription-провайдера до GA

Это канонический playbook для добавления в Claude_API нового AI-провайдера, чья ёмкость берётся
из пользовательских/корпоративных подписок, OAuth-профилей, service account или близкой модели.
Цель — не «научиться отправлять один запрос», а довести отдельную provider-plane до уровня
Claude/Codex/Gemini: безопасное пополнение через Auth Bot, липкий параллельный пул, точные деньги,
evidence-based калибровка, полная админка, blue-green, мониторинг и живой production-аудит.

Этот документ отвечает на вопрос **что доказать**. На вопрос **что именно отредактировать** —
точные файлы, символы, порядок коммитов и уже сработавшие ловушки — отвечает
`docs/engine/PROVIDER_WIRING_CHECKLIST.md`. Читать оба: принципы без карты дают верный, но
медленный обход, карта без принципов — быстрый неверный провайдер.

Документ обязателен вместе с корневыми `AGENTS.md`, `CLAUDE.md`, `BRANCHES.md`,
`docs/CHANGE_CHECKLISTS.md` и `docs/DEPENDENCIES.md`. Локальный `CLAUDE.md` каждого затронутого
крейта/приложения читается до правки. Если этот документ расходится с текущим кодом или локальной
инструкцией, авторитетен текущий checkout; расхождение надо исправить в том же изменении.

## 1. Что считать провайдером и что считать GA

Новая модель внутри уже законченной provider-plane проходит чеклист «Новая модель». Новый способ
оплаты проходит отдельный payment-provider чеклист. Этот playbook применяется, когда появляются
новые credential, upstream transport, subscription quota/credits, пул или отдельный runtime.

Состояния интеграции:

- **research** — собираются факты, production-обещаний нет;
- **preview** — additive/runtime-код может быть в production за выключенным флагом, но неизвестные
  планы, модели, деньги или calibration явно видны;
- **GA** — все применимые гейты этого документа доказаны на точном production SHA.

Merge, зелёная сборка, mock 200, строка модели в каталоге или один живой non-stream запрос сами по
себе не означают GA.

### Терминальный критерий GA

Провайдер GA, когда одновременно выполнено следующее:

1. Названия подписок, модели, доступность по тирам, официальный прайсинг, OAuth/API contract и
   ограничения датированы и подтверждены источниками; неразрешённых противоречий нет.
2. Для каждой опубликованной plan × model × tier/capability строки есть применимое живое
   evidence: auth/refresh, non-stream, настоящий incremental stream, authoritative usage, quota и
   sanitised errors.
3. Credential запечатывается, атомарно публикуется, ротируется и переживает restart/blue-green без
   plaintext, duplicate identity или refresh-family race.
4. Auth Bot полностью проводит новичка от покупки/прокси/активации тарифа до atomic publication;
   cancel, retry, batch, crash и restart не платят и не двигают чужую сделку.
5. Каждый запрос стартует сразу: нет process/account semaphore, очереди, ожидания слота или
   искусственного concurrency reject. Sticky affinity, provider quota и честный cooling сохранены.
6. Retry/rotation возможен только до первого публичного байта. Disconnect дренирует upstream до
   usage и settlement; после первого байта account replay запрещён.
7. Money lifecycle durable: reserve → delivering → exact settlement/refund; idempotency переживает
   retry, disconnect, writer failure и restart.
8. Official API replacement cost и native subscription consumption учитываются раздельно.
   Калибровка окон строится из immutable raw evidence без придуманного plan nominal/prior/EMA.
9. Admin UI, каталог, commerce/OpenKeys/web-потребители, monitoring, alerts, runbook, rollback и
   provider docs завершены.
10. Все выбранные/полные gates зелёные, exact master SHA имеет `deploy/watchdog GREEN`, а
    post-deploy smoke через публичный production endpoint прошёл.

## 2. Работа агента и Goal mode

Для длинной интеграции создаётся один goal только по явной просьбе пользователя работать в Goal
mode. Objective должен содержать терминальный критерий: «provider X доведён до verified production
GA по `docs/engine/PROVIDER_ONBOARDING.md`». Goal нельзя закрывать после research, merge или preview.

Агент ведёт план как минимум по фазам:

1. официальный/GitHub/live research и capability manifest;
2. архитектура, dependencies, migration/contract rollout;
3. credential + Auth Bot;
4. runtime/pool/streaming;
5. billing + calibration;
6. admin/product surfaces;
7. observability/blue-green/tests;
8. production deploy + live GA audit.

Недостающая Ultra/Enterprise подписка или vendor approval блокирует только зависимый live gate.
Агент продолжает все безопасные mock/code/docs/test задачи, затем сообщает точный missing evidence
и минимальное действие человека. Нельзя переносить результат другого тира или выдумывать
доступность. Goal `complete` ставится только после терминального критерия; `blocked` — только по
правилам инструмента Goal mode, не потому что задача большая.

## 3. Research: не работать вслепую

### 3.1 Иерархия evidence

Каждое важное утверждение получает одну метку:

- `official` — актуальные provider-owned docs/schema/model card/pricing/plan/OAuth/terms/changelog;
- `live` — очищенное наблюдение на принадлежащей нам подписке с датой, планом, регионом и версией;
- `oss-hypothesis` — гипотеза из pinned стороннего исходника;
- `decision` — наше архитектурное решение, связанное с evidence;
- `unknown` — не установлено;
- `not-applicable` — исключено с причиной.

Порядок доверия: official normative contract → собственный live wire → несколько независимых OSS
реализаций → community issues. GitHub помогает понять реализацию, но не является authority
прайсинга, плана, разрешения или GA.

### 3.2 Как изучать GitHub безопасно

1. Искать не только имя провайдера, но exact endpoint, редкий header, wrapper key, OAuth client id,
   model alias и текст ошибки.
2. Выбирать по возможности минимум две независимые активные реализации; fork/copy считается один
   раз.
3. Зафиксировать repo URL, полный commit SHA, license, last activity, relevant paths и конкретную
   гипотезу, которую код подтверждает.
4. Клонировать read-only в `mktemp -d`, читать через `rg`; не запускать чужой install/postinstall,
   binary, curl или script.
5. Сравнить field-by-field: URL/query/method, headers, OAuth/PKCE/redirect/refresh, body wrapper,
   streaming framing, terminal usage, quota/reset, model translation и errors.
6. Временный clone/capture удалить после исследования. Ничего не копировать в product worktree
   без понимания license и security review.

Нельзя сохранять access/refresh token, cookie, email/account/subject/project, authenticated proxy,
raw prompt или полный private error. Live-тесты — только на принадлежащих/разрешённых аккаунтах;
никакого обхода CAPTCHA, access control или provider limits.

### 3.3 Capability manifest

В начале создаётся `docs/engine/<PROVIDER>_PROVIDER.md` со следующими таблицами.

**Product/plan:** точные маркетинговые имена, регионы, account types, cadence, quotas/credits,
модели по тирам, automation/redistribution/API terms, дата ревью.

**Credential:** grant, issuer, auth/token endpoints, official client, scopes, redirect, PKCE/state,
refresh rotation, duplicate identity, proxy/geography, revocation.

**Model admission:**

| Public model | Native model/control | Official plans | Live-tested plans | Price epoch | Non-stream | Incremental stream | Usage | Quota | Решение |
|---|---|---|---|---|---|---|---|---|---|

**Wire:**

| Operation | URL/query | Headers | Body/wrapper | Framing | Usage | Errors/retry |
|---|---|---|---|---|---|---|

**Money/quota:** disjoint usage legs, overlap rules, official rates, native credits, buckets,
duration/reset, scale/resolution, hard-stop signal, stale behavior.

Критические различия:

- наличие модели и цены в Developer API не доказывает subscription route;
- строка quota/catalogue не доказывает generation;
- non-stream 200 не доказывает streaming;
- `stream=true` с одним буферизованным кадром не является incremental stream;
- успех Pro не доказывает Ultra/Enterprise и наоборот;
- private route, который технически работает, не отменяет terms/compliance review.

Unknown future model/tier всегда fail closed. Блокируется минимальная зависимая поверхность, но
не весь провайдер, если identity, money и безопасность остальных строк уже доказаны.

## 4. Архитектура и границы

Engine слои неизменны:

```text
registry <- pool <- forward <- server
```

- `registry` — единственный владелец engine PostgreSQL и durable authority; HTTP/провайдерной сети
  нет;
- `pool` — selection/affinity/cooling state machine без HTTP/сети;
- `forward` — provider transport, protocol translation, stream lifecycle, billing/calibration
  orchestration; не владеет PostgreSQL;
- `server` — composition, единственное чтение engine env, fixed plane, HTTP/control/readiness;
- `metering` — чистая integer math/JSON, только exact pricing;
- `<provider>-credential` — чистый AEAD envelope, без сети;
- `authbot` — producer credential перед runtime, не импортирует pool/forward/server;
- `router` — stateless HTTP к stable origins, без registry/billing/retry/queue/provider health;
- `apps/admin` — HTTP consumer без своей БД/секретов.

Перед дизайном сравнить актуальные:

- `docs/engine/CODEX_PROVIDER.md`, `docs/engine/GEMINI_PROVIDER.md`;
- `crates/forward/src/{codex,gemini}/**`, `crates/pool/**`, `crates/registry/**`;
- `crates/metering/src/{codex,gemini}.rs`;
- `crates/authbot/src/{codex_login,gemini_oauth,setup_token}.rs`;
- `apps/admin/src/app/subscriptions/**`;
- `docs/engine/UNIFIED_ROUTER.md`, `crates/router/**`;
- `deploy/engine-bluegreen.sh`, `deploy/watchdog*.sh`, `deploy/Caddyfile`, `systemd/**`;
- `observability/**`, `docs/ops/MONITORING.md`.

Новый distinct auth/quota/backend обычно получает отдельную fixed provider-plane, два слота,
stable loopback origin и router namespace. Его outage/readiness не должны останавливать другие
плоскости или превращать router `/health` в конъюнкцию всех providers.

## 5. Порядок доставки и совместимость

Изменения schema и cross-context contracts — expand-only, producer-first. Типичный порядок:

1. research/provider-doc skeleton;
2. additive engine migration + dormant registry API → merge command → exact watchdog GREEN;
3. credential crate + metering;
4. disabled runtime producer/control DTO → watchdog GREEN;
5. dependent contracts/engine-client/commerce consumers → watchdog GREEN;
6. Auth Bot и admin/product surfaces;
7. systemd/Caddy/blue-green/observability behind disabled switch;
8. controlled live preview/calibration;
9. reviewed catalogue/policy/provider activation;
10. public smoke и GA report.

Существующую migration не менять. Новое поле/route добавляется раньше потребителя. Старый runtime
обязан переживать expanded schema. Удаление/rename/semantic replacement — отдельная последняя фаза.

Новый provider default остаётся выключенным до preview gate. Нельзя менять глобальные Claude/Codex/
Gemini semantics ради удобства нового transport. Shared refactor обязан сохранить все их tests и
public bytes.

## 6. Credential и atomic roster

Для OAuth/secret материала создаётся `crates/<provider>-credential` и локальный `CLAUDE.md`.
Минимальный контракт:

- versioned XChaCha20-Poly1305 envelope (или текущий approved AEAD), explicit `kid`;
- profile id + credential kind в AAD;
- keyring читает старые keys, пишет active key, поддерживает online rewrap;
- bounded strict fields, secret types не имеют leaking `Debug`/errors;
- envelope 0600, directory 0700, правильный owner; symlink/alternate path запрещены;
- temp на том же filesystem, fsync + atomic rename;
- roster содержит только opaque id и exact credential file;
- сначала полностью пишется envelope, затем atomic roster;
- bad reload сохраняет last-good pool.

Stable provider subject/account identity — quota/dedup authority. Raw identity запечатана.
Публикуется только opaque id/разрешённая короткая маска. Reject: duplicate identity, duplicate
authenticated proxy (если нужна изоляция), wrong issuer/audience/client/kind/plan, unknown tier,
permissive mode/path escape.

Rotating refresh family требует per-profile single-flight: winner атомарно re-seal token до снятия
lock. При race двух blue-green поколений loser один раз перечитывает envelope и использует winner;
нельзя бесконтрольно reuse старый refresh token.

## 7. Auth Bot: полноценный onboarding

Добавляется отдельный offer/handoff kind, не ослабляя Claude/Codex/Gemini state machines:

- exact plan menu и offer validation;
- single/batch seller lock и item generation;
- выбор/выдача proxy и credential-free preflight;
- новичковая инструкция: proxy до открытия account, не менять profile/IP, активировать exact plan;
- official OAuth/device flow с state + PKCE и isolated staging;
- plan/issuer/audience/identity validation;
- seal + atomic publication до payout completion;
- cancel/retry/expiry/pause/resume/restart/crash semantics;
- admin jobs/status без secret.

Продавец никогда не присылает password, 2FA, cookie, card data, OAuth token или proxy URL оператору.
Bot не печатает secret/private errors. Callback form — one-time/no-store/bounded. Failed, expired,
wrong-plan flow не оставляет credential/roster row и не завершает выплату. Retry получает новую
generation, но сохраняет точного seller/job/item и назначенный egress.

Live acceptance выполняется для каждого supported plan: acquire → publish → runtime refresh →
remove/revoke, включая один restart на незавершённом flow.

## 8. Runtime и липкий пул

### 8.1 Request lifecycle

Один внутренний CSPRNG `request_id` живёт через все pre-byte attempts:

1. auth customer, canonical model/tier, exact conservative hold;
2. durable reserve;
3. sticky affinity или selection eligible profile;
4. single-flight credential refresh;
5. native request и startup classification;
6. retry/rotation, пока клиент не получил public byte;
7. atomic mark `delivering` перед первым public byte;
8. incremental translation;
9. client disconnect прекращает delivery, но bounded task дренирует upstream;
10. terminal authoritative usage → immutable evidence → exact settlement;
11. release guards и health/quota update.

Shutdown сначала закрывает admission, затем ждёт detached drains. На deadline abort read,
консервативно settle last documented state, пересекает task barrier, затем flush billing writer.

### 8.2 Никаких локальных concurrency limits

Запрещены process/per-profile/per-account semaphore, admission queue, wait-for-slot и synthetic 429.
Каждый admitted запрос немедленно начинает upstream attempt. `inflight` — только placement signal.
Реальными ограничениями остаются provider `allowed/limit_reached`, quota reset, auth/transport/model
health и per-request memory/body/time bounds.

Concurrency test использует barriered mock: N независимых upstream requests должны стартовать до
того, как mock отпустит хотя бы один ответ. Последовательное успешное завершение не доказывает
параллельность.

### 8.3 Selection и affinity

Порядок предпочтений:

1. здоровая tenant-scoped conversation affinity;
2. account/model eligibility и explicit provider wall;
3. fresh quota выше stale, never-seen neutral;
4. account/transport/model health;
5. inflight;
6. coarse quota steering только возле wall;
7. atomic rotation cursor.

Новые сессии распределяются; sticky сохраняет cache/thread continuity. Soft reserve jitter-ится
детерминированно по profile, но если все working profiles пересекли reserve и provider ещё разрешает
работу, пул fail-open до реального wall.

### 8.4 Health и retry

Разделять axes:

- durable account/auth `healthy → suspect → dead`;
- in-memory transport `responsive → degraded → wedged`;
- model generation streak/cooldown, если одна модель может ломаться независимо;
- provider quota bucket + reset, не generic health.

Типовой policy, уточняемый live evidence:

- первый 401 → forced refresh + same-profile retry; повторный 401/403 → auth quarantine + rotate;
- 429 quota → cool exact model/account scope до parsed reset, rotate без transport budget;
- timeout/network/408/409/425/5xx до bytes → bounded transport/model rotation;
- deterministic context/schema/safety 4xx → client/provider semantic error, без rotate/blame;
- malformed stream/wrapper → fault до bytes или sanitised terminal error после bytes.

Только успешная generation или equivalent provider probe очищает соответствующий fault. `countTokens`
не реабилитирует generation route, если это разные backend paths.

### 8.5 Streaming

Нужно отдельно доказать endpoint/query/Accept/body variant, multiline/partial framing, incremental
arrival, terminal usage и mid-stream error. Bound: startup time/bytes/chunks и accounting-only
silence после первого event. После первого public byte replay/account switch запрещён. Truncation не
может стать synthetic clean completion. Buffered fallback маркируется как non-stream/buffered, а не
как настоящий stream.

## 9. Точные деньги и settlement

`crates/metering/src/<provider>.rs` — единственный authority official rate card:

- effective-dated model/tier/geography schedule;
- integer nanoUSD (`1 USD = 1_000_000_000`), checked rational/rounding при необходимости;
- disjoint fresh/cached/cache-write/output/reasoning/audio/image/search/tool/long/speed legs;
- explicit subset rules, чтобы cached/reasoning не считались дважды;
- official URL, review date, canonical aliases и exact vector tests;
- unknown price/model/tier fail closed до reserve.

Money lifecycle: reserve conservative hold → same reservation через pre-byte rotation → mark
delivering → terminal exact cost → durable settlement outbox exactly once. До delivery failure
refund; после delivery missing usage использует документированный conservative hold/last-snapshot
policy и operational counter. RAII и restart не оставляют деньги в неопределённом состоянии.

Upstream/client request ids — audit metadata, а не money identity. Exact replay одного semantic
event idempotent; другой payload под тем же внутренним id — typed conflict.

## 10. Калибровка — Claude/GPT-уровень

Калибровка — backend evidence system, а не frontend-формула и не разовый benchmark. Её задача —
ответить, сколько official API replacement cost реально помещается в provider window для
наблюдённой нагрузки, а при наличии native consumption ещё и сколько native units составляет окно.
Покупная цена подписки на этот расчёт не влияет.

Перед реализацией прочитать актуальные:

- `crates/forward/src/anthropic_calibration.rs` — Claude estimator без выдуманных native credits;
- `crates/forward/src/codex/calibration.rs` — GPT dual-ledger estimator и quantisation envelope;
- `crates/registry/src/provider_calibration.rs` и PostgreSQL parity — immutable turn ledger,
  cumulative subject spend, observations и CAS;
- `tools/claude_calibration/{run_live.py,test_run_live.py}` и
  `docs/ops/CLAUDE_CALIBRATION.md` — безопасный live calibration runner;
- `docs/engine/CODEX_PROVIDER.md` — native-credit cohorts и разделение API/native economics.

### 10.1 Выбор ledger-модели

Official API replacement cost и native subscription consumption — разные величины:

- **Claude-подобный provider** публикует quota fraction, но не native списание. Хранится exact API
  nanoUSD ledger, и estimator публикует только realized workload blend. Native credits отсутствуют,
  а не вычисляются из долларов.
- **GPT-подобный provider** публикует и fraction, и authoritative native consumption. API nanoUSD и
  native units идут двумя независимыми cumulative ledgers; один никогда не восстанавливается из
  другого.
- **Неизвестная provider unit** сохраняется как отдельное raw evidence до live-доказательства её
  семантики. `remaining_amount`, provider credits и API tokens нельзя считать взаимозаменяемыми.

API-dollar capacity не равна цене подписки и не обязана совпадать у одинаковых plans, если у них
разная model/token/tool mix. Сравнивать одинаковые подписки надо по native capacity, если provider
действительно публикует native consumption; иначе сравнивается только like-for-like observed
workload с честно указанным blend.

### 10.2 Exact turn evidence и durability

Каждый successful billable turn с authoritative terminal usage создаёт один immutable event:

- provider и opaque subject/profile;
- внутренний CSPRNG request id, неизменный через все pre-byte retries;
- canonical requested/served model, accepted effective tier и provider-reported tier отдельно;
- inference geography/capability modifiers;
- effective-dated tariff schedule id и priced/completed timestamps;
- все непересекающиеся usage legs: fresh/audio/cache read/cache write TTL/output/image/search/tool;
- subset counters (reasoning/thinking/tool prompt) с explicit invariants;
- exact official API nanoUSD legs и total; authoritative native legs, если они существуют.

Сначала metering проверяет overlap и integer bounds. Затем одна authority transaction вставляет
event и продвигает cumulative subject ledgers. Exact replay того же payload идемпотентен; другой
payload под тем же request id — typed conflict. Aggregate/report строится из immutable rows, но
никогда не заменяет их. Customer discount/multiplier в calibration event не входит.

Между stream finalizer и authority обязателен bounded FIFO:

1. event ставится в очередь до запуска post-turn quota probe;
2. transient writer failure оставляет head pending, поэтому более поздний turn или free poll не
   увидит quota относительно устаревшего cumulative spend;
3. exact ambiguous database reply безопасно переигрывается через immutable idempotency;
4. semantic replay conflict карантинится, увеличивает dropped и не блокирует весь хвост;
5. health sweep повторяет flush даже без нового customer traffic; retire и graceful shutdown делают
   финальный flush;
6. projection/metrics публикуют `pending_events`, `dropped_events`, `persistence_ok`, authority
   availability и queue limit. Пока delivery degraded, продаваемая capacity не считается свежей.

### 10.3 Exact quota observations

Provider utilization парсится из decimal string/header в fixed point, не через binary float:

```text
FRACTION_SCALE = 100_000_000
0%   = 0
100% = 100_000_000
```

Вместе со значением хранится реальная resolution endpoint. Например, `40%` имеет resolution
`1_000_000` fraction units, `12.5%` — `100_000`, `12.125%` — `1_000`. Точность PostgreSQL bigint
не делает грубый whole-percent snapshot точным.

Каждая immutable observation содержит exact subject, authoritative paid plan, provider bucket,
window kind/duration, reset evidence, used fraction, measurement resolution, observed timestamp,
cumulative API/native ledgers, source (`response` или free `poll`), source request id и estimator
version. 5ч, 7д и любые provider-native durations живут независимо. Reads никогда не создают
observations.

### 10.4 Interval state machine

- Первый snapshot — anchor, не sample.
- Первый последующий positive fraction movement с positive settled delta уже публикует estimate;
  не надо ждать произвольное число samples.
- Response quota, пришедшая раньше settlement, удерживает anchor до ledger catch-up.
- Повторённое quota-only movement становится `unattributed_fraction_units` и не приписывается
  gateway расходу.
- Rollback с настоящим reset начинает новый interval, но не стирает complete history. Rolling reset
  определяется совместным utilisation rollback и material reset advance; bounded timestamp jitter
  сам по себе окно не форкает.
- Возврат к старому high-water после rollback не является новым расходом.
- Cutover нового native ledger ставит общий новый anchor обоим текущим estimators. Старый API
  evidence остаётся историей и не трактуется как zero-native spend.
- Stale/duplicate observations не меняют state. Invalid regression/identity/duration/resolution,
  negative delta и integer overflow fail closed.
- При смене estimator version state детерминированно rebuild-ится из immutable observation history;
  сохранённое старое derived значение не считается authority.

### 10.5 Capacity, uncertainty и maturity

Для каждого subject + plan + bucket + duration отдельно:

```text
capacity_nanoUSD = round_half_up(
  FRACTION_SCALE * Σ(delta_api_spend_nanoUSD) / Σ(delta_used_fraction_units)
)

native_capacity = round_half_up(
  FRACTION_SCALE * Σ(delta_native_consumption) / Σ(delta_used_fraction_units)
)
```

Native формула существует только при authoritative native ledger. Все операции — checked
integer/i128/rational math. Запрещены subscription-price prior, plan nominal, EMA/WLS, float money и
скрытый fallback.

На каждом interval denominator расширяется на половину resolution обоих endpoints. Low использует
`delta + uncertainty`, high — `delta - uncertainty`. Если movement не превосходит uncertainty,
finite high математически не доказан и публикуется `null`, не guessed ceiling. Общий estimate
использует все complete intervals; envelope консервативно покрывает contributing samples.
`confidence` — deterministic maturity × envelope stability × quantisation quality, не вероятность.

Projection обязана публиковать decimal integer strings: current capacity/remaining, low/high,
samples, observed fraction/spend/native consumption, resolution, confidence/maturity, last measured,
reset, source/version, unattributed и persistence state. Cold/unknown — `null`, не `$0`.
Fresh exact fraction без reset может обновить **current remaining**, но не доказывает следующий
horizon/reset; stale fraction не должна выглядеть продаваемой.

### 10.6 Cohorts и одинаковые подписки

Like-for-like aggregate допустим только для exact paid plan + native bucket/duration/schedule:

```text
pooled_native_capacity = FRACTION_SCALE
  * Σ(native_consumption)
  / Σ(used_fraction_units)
```

Равные plans получают одну shared cohort capacity, применённую к их current unused fraction. Это
убирает ложный разброс из-за whole-percent rounding и разного числа samples. Разные plans никогда не
смешиваются; missing plan блокирует cohort aggregation. Per-home raw evidence и bounds сохраняются.
Workload-dependent API-dollar capacity не превращается в обещанный plan nominal.

### 10.7 Deterministic test gate

Estimator/authority tests обязаны покрывать:

- cold anchor и первый complete interval;
- exact fractional evidence и whole-percent unbounded high;
- каждый допустимый истинный boundary внутри quantisation envelope;
- mixed model/tier/token/tool workload и disjoint leg totals;
- quota-before-settlement и repeated unattributed movement;
- reset, rolling rollover, reset jitter, rollback/high-water и independent durations;
- native-ledger cutover и legacy incomplete-history rebuild;
- estimator-version replay from immutable history;
- exact event replay, changed-payload conflict, CAS/idempotency и SQLite/PostgreSQL parity;
- transient FIFO failure/recovery, conflict quarantine, overflow/drop health и shutdown flush;
- remaining/bounds от exact current fraction;
- invalid identity/window/resolution, negative/regressing cumulative values и overflow fail closed.

### 10.8 Safe live calibration runner

Для нового provider создаётся `tools/<provider>_calibration/run_live.py`, offline tests и ops runbook
по образцу `tools/claude_calibration`. Runner — часть calibration acceptance, не одноразовый script:

- default dry-run; платный трафик только с `--execute`;
- integer `--budget-usd`, hard maximum не выше явно разрешённого пользователем, worst-case bound и
  budget guard для каждого возможного serving profile;
- exact admin-only target/session без spill/rebind; hard provider wall/cooling/dead остаются
  непреодолимыми;
- baseline `pending=0`, `dropped=0`, persistence/authority healthy и authoritative plan;
- free count/preflight, если provider его имеет; bound включает полный cache miss, max output,
  server-side tool/search payload и все per-call units;
- полный model × supported tier × context × token/cache/media/tool matrix; проверенная недоступность
  записывается, а не скрывается;
- unique run id и cache salt; только ожидаемые write/read делят cache key;
- после платного ответа attribution ждёт ровно один новый immutable event с exact request id,
  profile/model/tier и полным usage/cost vector. Concurrent traffic игнорируется по id, а
  неоднозначность fail closed;
- retry только read-only discovery/count/capacity. Платный request после transport ambiguity не
  повторяется автоматически;
- report содержит exact spend per profile, before/after fraction по каждому window, records,
  coverage, unavailable capabilities, profile stops, final capacity и profitability только для
  positive observed delta.

Runner tests покрывают budget/rebind, exact attribution на фоне чужого traffic, ambiguity,
cost-vector integrity, capability coverage, alias/global ceiling, cache isolation, safe retry,
secret containment, incomplete report и profitability ordering. Mock tests доказывают guards;
реальный provider contract доказывается только controlled run на owned subscriptions.

## 11. Текущий subscriptions control-room — UI-эталон

Основная админка после `ea5a07a` намеренно не является calibration laboratory. Перед добавлением
провайдера перечитать текущий `origin/master`:

- `apps/admin/src/app/subscriptions/fleet-capacity-overview.tsx`;
- `apps/admin/src/app/subscriptions/provider-board-ui.tsx`;
- `apps/admin/src/app/subscriptions/{claude,codex,gemini}-capacity-board.tsx`;
- `apps/admin/src/app/subscriptions/{provider,codex}-calibration.ts` и tests;
- `apps/admin/src/app/subscriptions/types.ts`, `page.tsx`, `page.test.tsx`;
- `apps/admin/src/app/globals.css` и `docs/product/ADMIN_PANEL.md`.

Эталонная information hierarchy:

1. Сверху единый control-room из provider cards. В каждой только два реально сопоставимых rail
   (сейчас 5ч/7д): current API-$ remaining / full calibrated window, used share, ready/total
   identities и measured coverage. У provider с другими durations показываются его настоящие окна,
   а не искусственные 5ч/7д.
2. Ниже у каждого provider одна компактная account/profile table. Слева sticky bounded email hint и
   plan/state; дальше quota/reset и exact remaining/full API-$ по реальным окнам. GPT дополнительно
   может показывать remaining native credits и два кратких API-$ сценария, рассчитанных через
   authoritative native/API rate cards.
3. Filled quota bar означает **уже использованную** долю. Рядом exact display percent, ниже reset.
4. Dead/non-routable строка говорит `вне ротации` и не входит в capacity. Pending/stale evidence
   говорит `сохраняется`, `обновляем` или `ждём данные`; `null` не превращается в zero/prior.
5. UUID, full email, raw ledgers, schedules, transport/proxy, private quota buckets, token-capacity и
   profitability matrices не выводятся в primary UI. Backend сохраняет их для audit/replay.
6. Одна identity даёт одну строку независимо от количества models/windows. Money приходит decimal
   strings и обрабатывается BigInt; model availability остаётся компактным count, если нужна
   оператору.
7. Визуальный язык переиспользует `FleetCapacityOverview`, `ProviderSection`,
   `ProviderQuotaMeter`, `TableCard`, существующие colors/spacing/sticky column и responsive
   horizontal overflow. Новый provider не создаёт отдельную design system.

Для GPT остаются точные конверсии, когда они нужны краткому summary/home:

```text
token_capacity = remaining_native_nanocredits / native_nanocredits_per_token
api_value_nanoUSD = round_half_up(
  remaining_native_nanocredits * api_nanoUSD_per_token
  / native_nanocredits_per_token
)
```

Context/speed multipliers применяются на тех же integer half-up boundaries, что server metering.
Cache write может использовать native fresh-input credits при отдельной API cache-write цене;
reasoning может быть subset output. Front не изобретает rates и зеркалит exact overlap rules только
для компактных денежных значений, а не разворачивает аналитические matrices.

Обязательны SSR/component tests для exact значений, privacy mask, null/stale/dead/pending, coverage,
ordering, duplicate prevention и explicit absence удалённых analytics. После build выполняется
visual review desktop/mobile; длинные таблицы не должны ломать page width или терять левую identity.

## 12. Admin, catalogue и продуктовые поверхности

Provider read/control projection содержит только privacy-safe:

- opaque id и разрешённую bounded mask;
- exact paid plan;
- auth/live/readiness, account/transport/model health;
- quota/cooling/reset и inflight как activity;
- calibration windows, resolution, native/API capacities, bounds/confidence/samples;
- exact plan+duration cohorts;
- bounded transport/runtime attestations.

Запрещены full email, external account/subject/project/org, token/cookie/proxy, credential path,
private error/trace.

UI должен корректно показывать null, zero, 100-but-allowed, stale, pending, reset, unbounded high,
model failure, cohort coverage и duplicate input. Одинаковый profile не может размножаться из-за
join по windows/models. Обязательны component/calculation/page tests, responsive/mobile,
accessibility и `docs/ops/FRONTEND_VISUAL_QA.md`.

Проверить все mirrors из `docs/CHANGE_CHECKLISTS.md`:

- metering/canonical models;
- `packages/contracts`, versioned product catalog и provider switches/policies;
- router namespace/aliases/`/v1/models`;
- OpenKeys fail-closed catalog;
- commerce worker/pricing policies;
- customer web/docs/SEO/integration builder;
- admin subscriptions и sales calculator.

Новая модель не становится saleable автоматически. Catalogue/switch/policy готовятся immutable,
runtime capability pins проверяются, producer deploy идёт первым, activation — после live evidence.

## 13. Blue-green, readiness и rollback

Отдельная provider-plane получает два slot units/ports, stable loopback Caddy origin, общий sealed
roster/keyring + PostgreSQL authority, но не общий process-local state. Promotion запускает candidate
в inactive slot и не останавливает active заранее.

Candidate readiness fail closed при invalid env/base/keyring/permissions, недоступной authority,
нуле live authenticated homes, полном auth/refresh failure, missing model/rate catalog, helper/client
attestation mismatch или невозможности корректного settlement. Quota exhaustion — capacity state,
не process death. Одна usable subscription — реальная ёмкость; arbitrary minimum fleet запрещён.

Rollback — предыдущий tested immutable release без schema rollback. Deployment selector, candidate
validation и exact-SHA verification включают новую plane. Production migration выполняет только
watchdog/blue-green controller.

## 14. Metrics, alerts и runbook

Только fixed/low-cardinality metrics:

- profiles по account/transport/model health;
- attempts/success/bounded failure classes;
- pre-byte rotation и post-byte terminal error;
- inflight/detached drains;
- quota freshness/exhaustion/model cooling;
- refresh/roster reload;
- reserve/settlement/refund/outbox/missing usage;
- calibration observations/samples/pending/dropped/conflict/unattributed/version;
- stream startup/first-event/turn latency.

Никаких raw profile/customer/email/account/project/proxy/prompt/request id/provider error labels.

Alerts и одноимённые anchors в `docs/ops/MONITORING.md` добавляются одним change: zero usable homes,
auth spike, all quota exhausted/stale, settlement backlog/failure, missing usage, roster/refresh/key
rotation failure, calibration loss/conflict/unattributed, stream error/latency, blue-green mismatch.
Runbook: impact, safe diagnosis, provider status, kill switch, replacement, rollback, evidence.

## 15. Полная test matrix

- credential seal/open/AAD/key/version/mode/path/symlink/rewrap/duplicates;
- OAuth state/PKCE/replay/expiry/wrong issuer/audience/client/plan;
- Auth Bot single/batch/locks/cancel/retry/pause/resume/restart/publication faults;
- metering exact rates, aliases, overlap, long/speed/media and overflow;
- registry SQLite/PostgreSQL parity, migration, immutable replay/conflict/CAS/outbox;
- request/response/error/stream fixtures всех operations;
- barriered burst 50+ simultaneous starts, sticky + unbound spread;
- 401/429/5xx/network/malformed pre-byte rotation budget;
- one-byte boundary: после него retry/account switch = 0;
- disconnect drain, missing usage, exactly-once settlement и shutdown deadline;
- roster reload/remove/refresh race/blue-green overlap;
- router catalogue/native/adapted routes и provider outage isolation;
- admin BigInt calculations/null/cohorts/duplicates/visual QA;
- systemd/Caddy/deploy/monitoring regressions;
- полный existing Claude/Codex/Gemini regression gate.

Mocks доказывают deterministic edges, live tests — реальный contract. Нельзя заменять одно другим.

## 16. Production live matrix

Для каждого advertised plan/model/tier/capability на owned subscriptions фиксируется дата, region,
client/runtime version и code SHA:

- auth + refresh;
- catalogue/native route;
- non-stream + canonical model + non-zero authoritative usage;
- incremental stream (несколько public arrivals, если output позволяет);
- token count/estimate, если advertised;
- quota movement/reset/hard stop;
- representative deterministic 4xx и exhaustion classification;
- official-rate settlement;
- proxy/geography behavior.

После landing тест идёт через public production hostname/router с dedicated test key/account:
models list, non-stream, stream, sticky, parallel burst, safe client error, exact charge, admin/metrics,
calibration persistence и exact release origin. Quota burn минимизируется; immutable money/calibration
evidence не удаляется.

## 17. Финальный отчёт

Отчёт содержит:

- exact master SHA и `deploy/watchdog` verdict;
- enabled/disabled plans/models/surfaces с причинами;
- датированную live matrix;
- official schedule ids/epochs и estimator version;
- calibration maturity/coverage/unknowns;
- Auth Bot acceptance;
- concurrency/stream/settlement fault results;
- units/slots/origins/readiness/rollback;
- metrics/alerts/runbook;
- residual risks и intentional unsupported capabilities.

Запрещено объявлять GA при unknown advertised tier/model, отсутствии authoritative usage, fake
streaming, secret leak, local concurrency wait, retry after bytes, lost disconnect settlement,
nominal/EMA calibration, незавершённом Auth Bot rollback, отсутствии blue-green/monitoring или пока
точный production watchdog/public smoke не зелёные.
