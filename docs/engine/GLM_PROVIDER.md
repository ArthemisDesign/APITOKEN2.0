# GLM (Zhipu AI / Z.ai) — provider capability manifest

Статус интеграции: **default-off backend preview, research завершён, runtime в `forward`
реализован (gateway + диспетч + billing writer), server-композиция и live-гейты впереди**.
Дата ревью источников — **2026-08-03**.

Документ создан по `docs/engine/PROVIDER_ONBOARDING.md` §3.3 и является capability manifest
плоскости GLM. Каждое утверждение помечено по иерархии evidence из §3.1: `official`, `live`,
`oss-hypothesis`, `decision`, `unknown`, `not-applicable`. Механическая карта правок —
`docs/engine/PROVIDER_WIRING_CHECKLIST.md`; образец реализации — `docs/engine/KIMI_PROVIDER.md`
(KIMI — ближайший аналог: китайский subscription-провайдер с Anthropic-совместимым транспортом).

## 0. Область и намеренные ограничения

Плоскость GLM строится **только как backend**: engine runtime, метеринг, калибровка, Auth Bot
и внутренний live-runner. Провайдер **не публикуется** в публичный каталог, `/v1/models` роутера,
commerce/OpenKeys прайсинг, сайт и клиентскую документацию — решение зеркалит KIMI §0.

- `official` `docs.z.ai/legal-agreement/subscription-terms` **прямо запрещает** перепродажу:
  «you may not resell, sub-resell, repackage, aggregate, proxy or otherwise provide the GLM
  Coding Plan to any third party», запрещён «general-purpose API access» из собственных
  приложений/SaaS без отдельного письменного договора, подписка привязана к одному физлицу.
  Санкции — урезание квоты, suspension/termination, бан после >3 нарушений
  (`docs.z.ai/devpack/usage-policy`).
- `decision` Именно этот запрет, как и у KIMI («personal interactive use only»), окончательно
  фиксирует backend-only режим: интеграция нужна для внутренней ёмкости и калибровки; любая
  публикация требует отдельного юридического ревью и в этой работе не выполняется.
- `decision` Модель бизнеса реселлеров (аудит запроса): китайские 中转站 (new-api/one-api relay)
  покупают Coding Plan, берут ключ из консоли, заводят его как channel и продают
  Anthropic/OpenAI-совместимый доступ поверх квоты — ровно модель нашего Claude-пула
  (`oss-hypothesis`: new-api#2051 добавил `open.bigmodel.cn/api/coding/paas/v4` именно для этого).

Пока публикации нет, GA-критерий §1 `PROVIDER_ONBOARDING.md` **не заявляется**. Терминальное
состояние — verified **preview**: всё, что не требует живой подписки, доказано на mock-гейтах;
живые гейты перечислены в §6 и ждут собственную подписку.

## 1. Product / plan

`official` Zhipu AI (Z.ai / open.bigmodel.cn) разводит независимые системы доступа:

| Плоскость | Назначение | Base URL | Биллинг |
|---|---|---|---|
| Z.ai Open Platform | pay-per-token developer API | `https://api.z.ai/api/paas/v4` (int.), `https://open.bigmodel.cn/api/paas/v4` (CN) | по токенам |
| GLM Coding Plan (подписка) | subscription coding plan | `https://api.z.ai/api/anthropic` + `https://api.z.ai/api/coding/paas/v4` (int.), `https://open.bigmodel.cn/api/anthropic` + `…/api/coding/paas/v4` (CN) | из квоты подписки |
| Z.ai web/app chat | потребительский чат | — | подписка, **API не даёт** |

`official` Неправильный endpoint = квота плана не расходуется и вызов не обслуживается
(devpack/faq, ошибка «1113 Insufficient Balance» при вызове вне плановых endpoint'ов).
`official` Ключ Coding Plan привязан к типу продукта: Team Plan Key «not interchangeable with
other Z.AI's API Keys», ошибка `1315` — «API Key is limited to enterprise coding package
scenarios».

`decision` Наш провайдер — **только GLM Coding Plan, только individual credits-планы**
(Lite/Pro/Max новой системы, см. §1.1). Open Platform — исключительно authority официального
прайсинга (replacement cost). Team Plan (другая единица квоты — токены, не кредиты) и legacy
prompts-планы (V1/V2, сняты с продажи 2026-07-30) в первой версии плоскости **не поддерживаются**
и fail closed.

### 1.1 Тарифные планы (credits-система, с 2026-07-30)

`official` (`docs.z.ai/devpack/overview`, `docs.z.ai/devpack/notice/usage-revision`; китайская
версия `docs.bigmodel.cn/cn/coding-plan/overview` согласуется):

| План | Кредиты / 5ч | Кредиты / неделю |
|---|---|---|
| Lite | 2 000 | 10 000 |
| Pro | 12 000 | 60 000 |
| Max | 28 000 | 140 000 |

- Формула списания: `credits = (input_tokens × in_mult + cached_input_tokens × cache_mult +
  output_tokens × out_mult) / 10 000`. Мультипликаторы — §5.2.
- **Off-peak −50 %**: peak = пн–пт 14:00–18:00 SGT (UTC+8), вне пика списание вдвое меньше.
- Reset: 5-часовые кредиты — динамически («через 5 часов после потребления», rolling);
  недельные — каждые 7 дней с момента заказа.
- При исчерпании квоты баланс аккаунта **не списывается**; вызовы вне плана невозможны.
- `unknown` Текущие USD-цены планов не зафиксированы provider-owned страницей (z.ai/subscribe —
  JS-рендер). Community: Lite ~$18/мес, Pro ~$72–80/мес, Max ~$160–168/мес. Цена подписки в
  расчётах не участвует (калибровка считает API replacement cost, а не окупаемость).

`official` Легаси prompts-планы (V1/V2) живут у старых подписчиков до конца billing cycle:
Lite ~80 prompts/5ч, Pro ~400/5ч, Max ~1 600/5ч, GLM-5.2/5-Turbo жгут 3× в peak.
`decision` Легаси не онбордим: Auth Bot принимает только credits-планы; quota-наблюдение,
несовместимое с credits-семантикой, fail closed (см. §5.3).

`official` Team Plan: Standard 60M tokens/5ч + 300M/нед, Premium 160M/5ч + 800M/нед — **другая
единица** (токены, не кредиты). `decision` Не поддерживается в v1; Team-ключ распознаётся по
несоответствию ожидаемой credits-форме и отклоняется при onboarding.

## 2. Credential

`official` В отличие от KIMI (OAuth device flow), GLM Coding Plan работает по **статическому
API-ключу** из консоли: Individual Coding Plan → Plan Overview → создать API Key
(`docs.z.ai/devpack/quick-start`). Refresh-семьи нет; ротация = перевыпуск ключа в консоли.

`decision` Модель acquisition — по образцу Claude setup-token ветки Auth Bot: продавец проходит
прокси/аккаунт-гайд, покупает exact plan на своём аккаунте Z.ai/bigmodel.cn, создаёт ключ в
консоли и присылает ключ боту. Бот валидирует (§7), запечатывает AEAD-конверт и атомарно
публикует roster до завершения выплаты. Продавец никогда не присылает пароль, 2FA, cookie или
карту; API-ключ — единственный credential-артефакт, как `sk-ant-oat01-…` у Claude.

`decision` Base URL хранится в запечатанном credential **на профиль**: int-продавец →
`https://api.z.ai`, CN-продавец → `https://open.bigmodel.cn`; allowlist ровно из двух хостов.
Ключи int/CN несовместимы между площадками (decision из привязки ключа к консоли-издателю).

### 2.1 Identity и валидация ключа

`oss-hypothesis` Machine-readable identity endpoint (аналог KIMI `/me`) **не найден** ни в
документации, ни в OSS. Роль проверки валидности выполняет quota endpoint (§5.2):
`GET {base}/api/monitor/usage/quota/limit`, заголовок `Authorization: <key>` **без
Bearer-префикса**; невалидный ключ возвращает **HTTP 200 с `code: 401` в теле** (onWatch
`zai_client.go`/`zai_types.go`, pinned SHA — §8).

`decision` Stable provider subject: квота привязана к аккаунту/ключу; в отсутствие `/me`
subject-идентичностью выступает сам ключ, точнее его keyed-BLAKE3 digest (raw ключ не покидает
конверт). Dedup правило: один и тот же ключ не может занимать два профиля; повторная публикация
того же digest заменяет профиль на месте (как у KIMI по subject).

`decision` Authoritative paid plan identity: machine-readable `user_level_name` не существует
(`unknown`). План фиксируется **декларативно из продукта оффера** (оператор создаёт оффер
«GLM Coding Plan Pro», продавец обязан активировать именно его) и **корроборируется** quota
endpoint: опубликованные window limits (2 000/12 000/28 000 кредитов за 5ч) однозначно
отображаются в Lite/Pro/Max. Наблюдённый лимит, противоречащий заявленному плану, — fail
closed, профиль вне ротации до операторского review.

## 3. Model admission

`official` На Coding Plan вызываются **только три модели** (devpack/faq: «Only the following
three models can be called»; devpack/overview):

| Подписочная модель | Официальная модель (rate card) | Контекст | Max output | Тир | Non-stream | Incremental stream | Usage | Quota | Решение |
|---|---|---|---|---|---|---|---|---|---|
| `glm-5.2` | `glm-5.2` | 1 000 000 | 131 072 | все планы | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |
| `glm-5-turbo` | `glm-5-turbo` | 200 000 | 131 072 | все планы | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |
| `glm-4.7` | `glm-4.7` | 200 000 | 131 072 | все планы | `unknown` | `unknown` | `unknown` | `official` | preview, за switch |

`official` **Served ≠ requested**: «调用历史模型 GLM-5.1/GLM-5 都将自动切换至 GLM-5.2» — запросы
к glm-5.1/glm-5 молча роутятся на glm-5.2 (кит. overview). `decision` Тарификация идёт по
**served** модели из ответа (та же ловушка, что у KIMI §3): immutable turn event хранит
requested и served раздельно; отсутствие served model в ответе — биллинг fail closed до reserve.

`official` Специальный синтаксис `glm-5.2[1m]` в официальном маппинге Claude Code
(`ANTHROPIC_DEFAULT_SONNET_MODEL: glm-5.2[1m]`) — селектор 1M-окна, а не отдельная модель.
`decision` Канонический id — `glm-5.2`; скобочная форма принимается как алиас (как `k3[1m]`).

`official` Thinking: `thinking.type` enabled/disabled; `reasoning_effort` (только GLM-5.2:
max/xhigh/high/medium/low/minimal/none; low/medium→high, xhigh→max; none/minimal — thinking off).
`unknown` Меняет ли отключение thinking served-модель/тариф (у KIMI менял) — до live fail
closed: billing по served model из ответа, при расхождении с допустимым множеством — ошибка.

`oss-hypothesis` Vision (`glm-4.6v`) и highspeed-вариант `glm-5.2-highspeed` фигурируют в
models.dev для `zai-coding-plan`, но не входят в официальную тройку текстовых моделей.
`decision` В v1 не публикуются: capability записывается unavailable, бюджет не тратится.

## 4. Wire

| Операция | URL | Заголовки | Тело | Framing | Usage | Ошибки |
|---|---|---|---|---|---|---|
| Generation (Anthropic) | `POST {base}/api/anthropic/v1/messages` | `Authorization: Bearer` (official, Claude Code через `ANTHROPIC_AUTH_TOKEN`; wire-подтверждение `oss-hypothesis` gsd-2#3874) | Anthropic Messages | SSE | `unknown` (cache-поля) | §4.2 |
| Generation (OpenAI) | `POST {base}/api/coding/paas/v4/chat/completions` | `Authorization: Bearer` (official) | OpenAI Chat | SSE | `official` `prompt_tokens/completion_tokens/prompt_tokens_details.cached_tokens/total_tokens` | §4.2 |
| Quota | `GET {base}/api/monitor/usage/quota/limit` | `Authorization: <key>` **без Bearer** (`oss-hypothesis`) | — | JSON | — | HTTP 200 + `code:401` в теле |
| Catalogue | `GET {base}/api/anthropic/v1/models` | `oss-hypothesis` | — | JSON | — | `unknown` |

`decision` **Плоскость обслуживает только Anthropic-маршрут** — как и KIMI, это позволяет
переиспользовать нативный Anthropic-путь движка без трансляционного слоя масштаба `gemini/`.
OpenAI-маршрут задокументирован, но в v1 не поднимается.

`oss-hypothesis` Risk-control Z.ai детектит «SDK-based access» — запросы без идентифицирующих
tool-заголовков — и применяет throttling/бан (pi#4187). `decision` Gateway обязан отправлять
Claude-Code-совместимый identity-набор (User-Agent CLI, anthropic-beta и т.п.) по образцу
инжекта identity в Claude-плоскости; «голый SDK» трафик — прямой путь к бану подписки.

`unknown` Принимает ли Anthropic-endpoint `x-api-key` (документирован только Bearer).
`decision` Реализуем Bearer; альтернатива не нужна до live-доказательства обратного.

`unknown` Точная форма `usage` на Anthropic-маршруте (имена cache-полей, счётчик
thinking-токенов). Без authoritative usage биллинг fail closed — settlement по
консервативному hold.

### 4.1 Реализованный backend gateway

`decision` Точные reviewed GLM aliases (`glm-5.2`, `glm-5-turbo`, `glm-4.7`, `glm-5.2[1m]`)
диспетчеризуются внутри Anthropic `POST /v1/messages` после общей авторизации и
bounded-чтения тела, но до Claude-specific identity, pricing и pool mutation — по образцу
KIMI §4.1. Alias никогда не уходит в Claude upstream: выключенная плоскость, повреждённый
initial roster и cold roster дают fail-closed ответ GLM-пути, не fallback.

### 4.2 Классы ошибок

`official` (`docs.z.ai/api-reference/api-code`, ревизия 2026-07-30): двухслойная схема —
HTTP status + business code в теле `{"error": {"code": "1308", "message": "…"}}`:

| Код | HTTP | Смысл | Наша реакция |
|---|---|---|---|
| 1000–1005 | 401 | auth failure | auth quarantine (refresh нет — ключ статический), rotate |
| 1113 | 429 | insufficient balance (внеплановый вызов) | account anomaly → suspect, не quota wall |
| 1210–1215 | 400 | request validation | client semantic error, без rotate/blame |
| 1220 | 403 | доступ запрещён | capability/план → fail closed scope |
| 1301 | 400 | контент-фильтр | client semantic error |
| 1302 | 429 | rate limit | bounded transport rotation |
| 1305 | 429 | overload | bounded transport rotation |
| 1308 | 429 | **5-часовая квота исчерпана**, «reset at {next_flush_time}» | quota wall → cooling до parsed reset, без transport-бюджета |
| 1309 | 429 | план истёк | account dead (вне ротации до замены ключа) |
| 1310 | 429 | **недельная/месячная квота исчерпана** | quota wall → cooling до weekly reset |
| 1311 | 429 | модель не входит в план | model-scope ineligible, не account |
| 1313 | 429 | fair-use нарушение | account suspect (risk-control), out of rotation |
| 1315 | 429 | ключ ограничен enterprise-сценариями | wrong key kind → suspect (out of rotation, operator review) |
| 1316–1321 | 429 | extra usage / monthly spend limit (Team-механика) | account anomaly → suspect |

`official` При аномальном обрыве SSE коды ошибок не возвращаются — причина приходит в
`finish_reason` чанка. `decision` Mid-stream классификация обязана читать `finish_reason`
(`sensitive`, `model_context_window_exceeded`, `network_error`).

`oss-hypothesis` Quota endpoint отвечает HTTP 200 даже при `code: 401` в теле.
`decision` Обработчик quota обязан проверять business code, а не только HTTP status.

## 5. Money / quota

### 5.1 Официальный rate card (replacement cost)

`official` `docs.z.ai/guides/overview/pricing`, ревью 2026-08-03, USD, за 1M токенов:

| Модель | Input | Cached input | Cached storage | Output |
|---|---|---|---|---|
| `glm-5.2` (= glm-5.1) | $1.40 | $0.26 | limited-time free | $4.40 |
| `glm-5-turbo` | $1.20 | $0.24 | limited-time free | $4.00 |
| `glm-5` | $1.00 | $0.20 | limited-time free | $3.20 |
| `glm-4.7` (= 4.6, 4.5) | $0.60 | $0.11 | limited-time free | $2.20 |
| `glm-4.5-air` | $0.20 | $0.03 | limited-time free | $1.10 |

`official` Cache storage — «Limited-time Free». `decision` Как у KIMI: платный cache-write leg
**отсутствует** задокументированно, а не считается нулём молча; при снятии «limited-time»
пометки расписание обновляется новой epoch.

`official` Web Search tool на Open Platform — $0.01/use; Coding-Plan MCP (Web Search / Web
Reader / Zread / Vision) списывается кредитами (×1.2 за вызов). `decision` Tool/search
capability на нашем маршруте записывается **unavailable** до доказательства конечного
per-request ceiling (`SKILL.md`); бюджет не тратится.

`official` Reasoning: `reasoning_content` возвращается отдельным полем; счётчик thinking-токенов
в `usage` не документирован. `decision` Reasoning — **subset output** (консервативно:
completion_tokens включает reasoning), отдельным leg'ом не тарифицируется; инвариант
reasoning ≤ output проверяется там, где провайдер возвращает разбивку.

### 5.2 Нативная квота — credits и quota endpoint

`official` Формула кредитов (devpack/overview):

```text
credits = (input_tokens × in_mult + cached_input_tokens × cache_mult
           + output_tokens × out_mult) / 10 000
```

| Продукт | in_mult | cache_mult | out_mult |
|---|---|---|---|
| GLM-5.2 | 6.9 | 1.7 | 24 |
| GLM-5-Turbo | 5.7 | 1.5 | 21 |
| GLM-4.7 | 4.6 | 1.2 | 16 |
| GLM-4.6V (Vision MCP) | 1.2 | 0.3 | 2.7 |
| MCP-инструменты (за вызов) | — | — | 1.2 |

`official` Off-peak: списание ×0.5 вне пика (пн–пт 14:00–18:00 SGT = UTC+8).

`oss-hypothesis` Quota endpoint `GET {base}/api/monitor/usage/quota/limit`
(onWatch `zai_types.go`, pinned SHA — §8): обёртка `{code, msg, success, data}`,
`data.limits[]` с `type: "TIME_LIMIT"|"TOKENS_LIMIT"`, поля
`unit, number, usage, currentValue, remaining, percentage, nextResetTime` (epoch ms),
`usageDetails[].modelCode` — per-model разбивка. Невалидный ключ: HTTP 200 + `code: 401`.

`unknown` Семантика единиц `currentValue/remaining/number` (кредиты? токены? что означают
`TIME_LIMIT` против `TOKENS_LIMIT`) не доказана. По `SKILL.md` величины сохраняются как **raw
quota evidence** до live-доказательства и не делятся на токеновую цену.

### 5.3 Выбор ledger-модели

`decision` GLM — **GPT-подобный dual-ledger провайдер с опубликованными native limits**:

1. **API-nanoUSD ledger** — точный, per-turn, из rate card §5.1 по **served** модели.
2. **Native credits ledger** — per-turn, вычисляемый из официальной формулы §5.2 (включая
   off-peak ×0.5 по расписанию UTC+8). Это не «выведенная» величина: формула и мультипликаторы
   опубликованы провайдером; ledger независим от API-долларов и никогда из них не
   восстанавливается.
3. **Window observations** — quota endpoint (§5.2) даёт provider-side состояние окна
   (`remaining`, `nextResetTime`); сохраняется как immutable raw evidence со своей resolution.

Нативную ёмкость окна оценивать не нужно — она опубликована (2 000/12 000/28 000 кредитов за
5ч; 10 000/60 000/140 000 за неделю по плану). Оценке подлежит только official API replacement
cost, помещающийся в окно при наблюдённой нагрузке: `capacity_nanoUSD = native_limit ×
ΣΔapi_nano / ΣΔnative_credits` на complete intervals (форма §10.5 onboarding, checked integer
math, round-half-up).

`decision` Расхождение нашего computed credits ledger с provider-side quota endpoint —
ожидаемый класс evidence: off-peak округления, MCP-вызовы, легаси-планы и **чужое потребление**
(все поддержанные инструменты делят одну квоту аккаунта) видны как unattributed movement и не
приписываются нашему gateway. Легаси prompts-форма наблюдений несовместима с credits-моделью и
fail closed (профиль quarantine, операторский review), а не молча интерпретируется.

`decision` Когорты (§10.6) объединяются только по точному declared plan + точной длительности
окна, с корроборацией наблюдённым native limit. Unknown/legacy plan блокирует агрегацию.

### 5.4 Runtime ordering

`decision` Зеркалит KIMI §5.4: первый бесплатный quota-anchor после валидации ключа, затем
периодический poll (`CLAUDE_API_GLM_QUOTA_POLL_SECS`); roster discovery — независимый
15-секундный tick. Turn-before-quota ordering обязателен: poll не выполняется при pending head
в bounded turn FIFO, после HTTP-снимка writer повторно дренирует FIFO, читает durable
cumulative ledgers и завершает immutable observation/CAS до публикации quota steering.

## 6. Что остаётся недоказанным

Каждый `unknown` fail closed и снимается только контролируемым live-прогоном на собственной
подписке:

1. Точная форма `usage` на Anthropic-маршруте (cache-поля, thinking-счётчик).
2. Реальная инкрементальность SSE (буферизованный кадр ≠ stream).
3. Семантика единиц quota endpoint (`currentValue/remaining/number`, TIME_LIMIT vs TOKENS_LIMIT).
4. Различение legacy prompts-плана от credits-плана по quota endpoint на живом аккаунте.
5. Меняет ли отключение thinking served-модель/тариф.
6. Поведение quota wall на Anthropic-маршруте: точный business code и тело (1308/1310).
7. Принимает ли Anthropic-endpoint `x-api-key`; полный набор обязательных identity-заголовков,
   проходящих risk-control без троттлинга.
8. Наличие и точность per-model `usageDetails` для unattributed-атрибуции.
9. Текущие USD/CNY цены планов (не блокирует: цена в расчётах не участвует).

## 7. Auth Bot: acquisition flow (решение)

`decision` Отдельный `HandoffKind::Glm`, шаги `glm_proxy → glm_ready → glm_wait`, кнопка
`glm:ready`. Отличие от KIMI — нет device flow; вместо него ввод ключа текстом:

1. `glm_proxy`: прокси текстом (обратимый разбор, канонизация `glm_credential::normalize_proxy_url`),
   выбор int (`api.z.ai`) или CN (`open.bigmodel.cn`) площадки кнопкой.
2. Продавец получает новичковый гайд: не открывать аккаунт без прокси, не менять профиль/IP,
   активировать exact plan (Lite/Pro/Max по продукту оффера), создать ключ в консоли
   Plan Overview.
3. `glm_ready`: продавец подтверждает готовность аккаунта; бот просит прислать API-ключ.
4. `glm_wait`: бот валидирует ключ: бесплатный quota-probe (§2.1) → одна минимальная платная
   generation на Anthropic-маршруте (`glm-4.7`, `max_tokens=1`, aggregate cap $0.0001 по
   AGENTS.md admission micro-smoke) → seal envelope → atomic roster publish → завершение
   выплаты.
5. Cancel/retry/expiry/wrong-plan не оставляют ни файла credential, ни строки roster и не
   завершают выплату. Невалидный ключ (HTTP 200 + `code:401`), Team/legacy форма quota,
   расхождение plan↔limit — возврат на `glm_ready` с безопасной подсказкой.

## 8. Состояние доставки

| Этап | Артефакт | Состояние |
|---|---|---|
| research / capability manifest | этот файл | готово |
| официальный rate card + credit multipliers | `crates/metering/src/glm.rs` | готово, 24 теста |
| calibration authority (schema 0029) | `crates/registry/migrations_pg/0029_glm_window_calibration.sql` | готово, expand-only, real-PG matrix зелёная |
| типы наблюдений | `crates/registry/src/glm_calibration.rs` | готово, 20 тестов |
| credential | `crates/glm-credential` | готово, 18 тестов |
| calibration estimator | `crates/forward/src/glm_calibration.rs` | готово, 27 тестов |
| Auth Bot: протокол валидации + roster | `crates/authbot/src/{glm_key,glm_roster}.rs` | готово, 26 тестов |
| Auth Bot: мастер продавца | `crates/authbot/src/bot.rs` (+`db.rs` `hregion`/recovery, `main.rs`) | готово, 21 тест (мастер, меню, регион, restart-восстановление) |
| runtime-примитивы: config / transport / roster / client / selection / pool / queue | `crates/forward/src/glm/` | готово, 71 тест |
| gateway (+ диспетч `proxy.rs`, `AppState.glm`, billing writer, test-loopback фича credential) | `crates/forward/src/glm/gateway.rs` | готово, 35 mock-тестов + real-PG гейт в `billing.rs` |
| server: env/config + композиция | `crates/server/src/{config,main,poller}.rs` | готово (env/config, композиция, maintenance loop, shutdown flush) |
| observability, admin projection | `observability/**`, `apps/admin` | не начато — отложено по рамке владельца «только backend, тестовый режим» (2026-08-04) |
| безопасный live-runner | `tools/glm_calibration/` | не начато — ждёт первую живую подписку (runner без неё не проверить) |
| live-матрица на нашей подписке | — | **нужна подписка (блокирует человек)** |

Очередь и SHA-трекинг — `research/GLM_PLANE_PROGRESS.md`.

## 9. Источники

Все ссылки просмотрены 2026-08-03.

- `https://docs.z.ai/devpack/overview` — планы, кредиты, формула, off-peak, reset
- `https://docs.z.ai/devpack/faq` — endpoint'ы, тройка моделей, поведение при исчерпании
- `https://docs.z.ai/devpack/quick-start` — получение ключа, endpoint guide
- `https://docs.z.ai/devpack/usage-policy` — rate limits, запрет sharing, санкции
- `https://docs.z.ai/devpack/notice/usage-revision` — переход prompts→credits 2026-07-30, legacy, Team
- `https://docs.z.ai/guides/overview/pricing` — официальный rate card
- `https://docs.z.ai/guides/llm/{glm-5.2,glm-5-turbo,glm-4.7}` — контексты, max output
- `https://docs.z.ai/api-reference/llm/chat-completion` — OpenAI wire, usage, thinking
- `https://docs.z.ai/api-reference/api-code` — коды ошибок
- `https://docs.z.ai/legal-agreement/subscription-terms` — запрет resale/proxy/multi-user
- `github.com/onllm-dev/onwatch` @ main (2026-08-03), `internal/api/zai_client.go`,
  `internal/api/zai_types.go` — quota endpoint (read-only исследование, MIT-подобная лицензия
  проверена при чтении; временный клон не создавался, чтение через web)
- `github.com/gsd-build/gsd-2#3874` — Bearer на `/api/anthropic` (wire-дамп)
- `github.com/QuantumNous/new-api#2051` — реселлерская схема coding-plan channel
- `github.com/earendil-works/pi#4187` — risk-control «SDK-based access»
