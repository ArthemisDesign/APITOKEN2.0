# Онбординг нового subscription-провайдера до GA

Это канонический playbook для добавления в Claude_API нового AI-провайдера, чья ёмкость берётся
из пользовательских/корпоративных подписок, OAuth-профилей, service account или близкой модели.
Цель — не «научиться отправлять один запрос», а довести отдельную provider-plane до уровня
Claude/Codex/Gemini: безопасное пополнение через Auth Bot, липкий параллельный пул, точные деньги,
evidence-based калибровка, полная админка, blue-green, мониторинг и живой production-аудит.

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

## 10. Калибровка — ровно Codex/GPT-уровень

API replacement cost и native subscription consumption — два независимых ledger. Нельзя выводить
credits из API USD или объявлять подписку фиксированным количеством долларов.

Каждый billable turn сохраняет immutable event: opaque home, internal request id, canonical model,
effective tier, provider-reported tier отдельно, schedule epoch, disjoint usage, exact API nanoUSD
legs и exact native credit legs (если provider их публикует). Insert event и продвижение обоих
cumulative ledgers — одна transaction. Raw rows никогда не переписываются estimator upgrade.

Provider utilization парсится decimal fixed-point, не float:

```text
SCALE = 100_000_000
0% = 0
100% = 100_000_000
```

Хранятся immutable observation: exact provider subject/bucket/duration, used fraction, реальная
measurement resolution, reset evidence, cumulative API spend/native credits, timestamps и version.
Storage precision не равна provider resolution: целый процент остаётся грубым измерением.

Для каждого duration/bucket независимо:

```text
capacity_nanoUSD = SCALE * Σ(delta_api_spend_nanoUSD) / Σ(delta_used_fraction)
native_capacity = SCALE * Σ(delta_native_credits) / Σ(delta_used_fraction)
```

Только checked integer/rational math. Запрещены subscription price prior, nominal plan capacity,
EMA/WLS, float money и скрытый fallback.

Interval rules:

- первый snapshot — anchor, не sample;
- учитывается только positive quota movement с settled positive spend/credits;
- quota, опередившая settlement, ждёт ledger catch-up;
- reset/rollback ставит новый anchor, но не стирает complete history;
- rolling reset требует совместного rollback + materially moved reset evidence; timestamp jitter не
  форкает окно;
- credit cutover создаёт общий новый anchor; старый API evidence не становится zero-credit;
- unattributed provider movement публикуется отдельно и не насильно приписывается gateway;
- estimator version rebuild-ится из raw evidence и сравнивается детерминированно.

Публикуются blend/capacity, conservative low/high с quantisation uncertainty, samples, observed
fraction, confidence/maturity, reset, persistence pending/dropped/conflict/unattributed. Если upper
bound математически неограничен, он `null`. Cold/unknown — `null`, не ноль.

### Cohorts

Like-for-like aggregate только exact paid plan + exact native duration/schedule:

```text
pooled_native_capacity = SCALE * Σ(native_spend) / Σ(used_fraction)
```

Равные plans получают общую cohort capacity, разные — никогда. Per-home raw evidence сохраняется.
API-dollar capacity остаётся workload-dependent и не pooled как обещанный номинал подписки.

## 11. Актуальный GPT capacity board — UI-эталон

Production commit `ae054c4` заменил старую GPT calibration laboratory компактной операторской
панелью. Перед новой реализацией обязательно перечитать текущий `origin/master`:

- `apps/admin/src/app/subscriptions/codex-capacity-board.tsx`;
- `apps/admin/src/app/subscriptions/codex-calibration.ts` и tests;
- `apps/admin/src/app/subscriptions/types.ts`, `page.tsx`, `page.test.tsx`;
- `docs/product/ADMIN_PANEL.md`.

Эталонная семантика:

1. Верхний strip берёт самое длинное положительное native window и суммирует exact plan cohorts.
   Если хотя бы одна нужная cohort ещё unknown, общий fleet nominal остаётся unknown, а не
   заниженной частичной суммой.
2. Показываются remaining native credits, full fleet capacity, used share и два API-$ сценария:
   representative Standard и максимально выгодный; оба подписаны model/tier/context/token kind.
3. Token-capacity table показывает для каждой модели и Standard/Fast, сколько current remaining
   credits дают fresh, cache read, cache write и output/reasoning tokens.
4. Profitability сравнивает exact `$ official API-equivalent / native credit` по short/long и всем
   token kinds, сортирует убыванием через integer cross multiplication.
5. Home table показывает bounded masked email, plan/runtime state, quota, plan-cohort remaining
   credits и два API-$ сценария.
6. Заполнение quota progress bar означает **уже потраченный quota**; рядом exact percent, ниже reset.
7. UUID, raw ledger, schedules и noisy individual capacity не выводятся в primary UI. Они остаются
   backend audit/replay evidence.
8. Placeholder/non-positive windows игнорируются. Cold state говорит `ждём Δquota`/unknown, не
   подставляет zero/prior.

Точные конверсии:

```text
token_capacity = remaining_native_nanocredits / native_nanocredits_per_token
api_value_nanoUSD = round_half_up(
  remaining_native_nanocredits * api_nanoUSD_per_token
  / native_nanocredits_per_token
)
```

Context/speed multipliers применяются на тех же integer half-up boundaries, что server metering.
Cache write может использовать native fresh-input credits при отдельной API cache-write цене;
reasoning может быть subset output. Front обязан зеркалить exact overlap rules.

Для нового провайдера сохраняется этот information hierarchy и визуальный язык, но Standard/Fast,
token classes, окно и сценарии заменяются реальными provider-native понятиями. Нельзя натянуть
5h/7d или credits на провайдера, у которого другие units/durations.

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
