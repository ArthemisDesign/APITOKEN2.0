# План реализации Gemini Batch Mode

Статус: проектное предложение, runtime не реализован.

Дата исследования: 2026-08-19.

## 1. Цель и границы

Нужно добавить для опубликованных текстовых Gemini-моделей асинхронный режим, в котором клиент:

1. отправляет набор независимых `GenerateContentRequest`;
2. сразу получает имя batch operation;
3. позднее читает состояние operation;
4. после завершения забирает упорядоченные результаты и ошибки отдельных задач.

Batch исполняется нашей очередью как набор обычных non-stream `generateContent` вызовов через
существующий пул Code Assist подписок. Это не passthrough в платный Google Batch API. Главная
продуктовая ценность режима для нас — управляемо разнести независимые задачи по подпискам и
пережить disconnect/restart клиента, а не получить тарифную скидку Google.

Фиксированные требования:

- дополнительной batch-скидки нет;
- остается обычный тариф модели и обычный account/provider multiplier;
- весь batch финансово допускается атомарно, затем каждая задача списывается отдельно;
- принятые задания, их holds и результаты переживают restart и blue-green overlap;
- обычные синхронные Gemini-запросы не получают очередь, semaphore или новый admission limit;
- prompts, inline media и результаты не хранятся в PostgreSQL открытым текстом;
- клиентские API совместимы с inline-подмножеством Gemini Developer API, а расширения не
  маскируются под подтвержденные Google-возможности.

В первую версию не входят:

- Files API и `inputConfig.fileName`;
- webhook delivery;
- batch embeddings;
- Vertex AI `BatchPredictionJob`, GCS и BigQuery;
- image-output модель `gemini-3.1-flash-image`;
- отдельный universal batch endpoint для OpenAI/Anthropic форматов;
- гарантированный срок выполнения или обещание Google Batch SLO.

Inline media можно разрешать только там, где его уже принимает общий native Gemini validator, при
общем лимите тела. Исключение image-output модели из первой версии ограничивает размер result set и
не меняет доступность ее обычного `generateContent` маршрута.

## 2. Что предоставляет оригинальный Gemini Batch API

Gemini Developer API использует Google long-running operation и следующие native routes:

| Операция | REST route |
|---|---|
| Создать | `POST /v1beta/models/{model}:batchGenerateContent` |
| Список | `GET /v1beta/batches` |
| Состояние и результат | `GET /v1beta/batches/{id}` |
| Отмена | `POST /v1beta/batches/{id}:cancel` |
| Удаление | `DELETE /v1beta/batches/{id}` |

Для inline input клиент передает `batch.inputConfig.requests.requests[]`; каждый элемент содержит
`request` и необязательный `metadata`. В terminal operation inline output находится в
`metadata.output.inlinedResponses`, в порядке входных элементов; один элемент содержит `response`
или `error` и возвращает исходный `metadata`. Отдельного `/results` route у Developer API нет.

Wire-состояния Google включают `BATCH_STATE_PENDING`, `BATCH_STATE_RUNNING`,
`BATCH_STATE_SUCCEEDED`, `BATCH_STATE_FAILED`, `BATCH_STATE_CANCELLED` и
`BATCH_STATE_EXPIRED` (плюс unspecified). List возвращает `operations[]` и `nextPageToken`, а
cancel/delete — пустой `google.protobuf.Empty` response.
Документированный queue/run deadline — 48 часов, target turnaround — 24 часа без SLA, успешные
результаты хранятся шесть недель. Inline request ограничен 20 MB; file mode допускает более крупный
JSONL, но требует отдельного Files API. Создание у Google неидемпотентно: повторный POST создает
новую operation.

Google продает Batch API по отдельному тарифу, обычно на 50% дешевле interactive inference. Эта
скидка неприменима к нашей реализации: под капотом выполняются обычные subscription-backed turns, а
не тарифицируемые Google Batch jobs. В публичной документации нельзя называть локальную очередь
Google Batch со скидкой или обещать его SLO.

Vertex AI batch — другой контракт: OAuth/IAM, project/location resources, GCS/BigQuery input/output
и другой lifecycle. Смешивать его с Developer API или заявлять Vertex compatibility не нужно.

Официальные источники, которые надо повторно зафиксировать golden fixtures перед кодированием:

- <https://ai.google.dev/gemini-api/docs/batch-api>
- <https://ai.google.dev/gemini-api/docs/rate-limits>
- <https://ai.google.dev/gemini-api/docs/pricing>
- <https://github.com/googleapis/python-genai/blob/main/google/genai/batches.py>
- <https://github.com/googleapis/js-genai/blob/main/src/batches.ts>
- <https://github.com/googleapis/go-genai/blob/main/batches.go>
- <https://cloud.google.com/vertex-ai/generative-ai/docs/multimodal/batch-prediction-gemini>

SDK-код полезен для точных route и field mappings, но не заменяет wire capture официального API.
Перед публикацией нужны sanitized request/response fixtures create/list/get/cancel/delete и
terminal inline output из актуальных Python и JavaScript SDK.

## 3. Текущее состояние проекта

### 3.1 Native Gemini plane

`crates/server/src/http.rs` монтирует native Gemini fallback только на фиксированной Gemini-плоскости.
`crates/forward/src/gemini/api.rs` сейчас распознает пять операций: list/get model,
`generateContent`, `streamGenerateContent` и `countTokens`. Model discovery честно публикует только
эти три generation methods.

Обычный generation flow уже содержит нужные низкоуровневые части:

1. account/key authorization;
2. canonicalization и fail-closed validation native body;
3. public model -> private wire/quota model resolution;
4. conservative hold и pin точного tariff version/multiplier;
5. выбор профиля по hard cooling, freshness quota, inflight, coarse quota rank и round-robin;
6. rotation до public delivery boundary;
7. Code Assist wrapping, OAuth helper и transport;
8. authoritative `usageMetadata` parsing;
9. durable settlement и immutable Gemini calibration event.

Синхронный path создает один billing request до первого upstream attempt и сохраняет его через
внутреннюю profile rotation. Успешный non-stream response не отдается без требуемого terminal usage.
У обычного пула нет concurrency semaphore: inflight — сигнал ранжирования, а не отказ.

### 3.2 Billing authority

`crates/registry` владеет PostgreSQL, customer balance, key limits, reservations, settlement outbox,
ledger, usage и owner epochs. Обычная reservation короткоживущая и привязана к конкретным
`owner_instance + owner_epoch + lease_until`. Reconciler закрывает holds умершего owner.

Поэтому нельзя создать сотни обычных `reservations` при приеме batch и оставить их в очереди на
часы: после blue-green они либо будут ошибочно reconciled, либо останутся недоступны новому owner.
Batch требует отдельного durable hold lifecycle, а не увеличенного lease timeout.

Существующий settlement остается эталоном математики:

- account balance и `reserved_nano` изменяются под одной account-row блокировкой;
- key spend limit учитывает settled + reserved;
- полный actual сохраняется даже при превышении hold;
- collection ограничена общим account floor, shortfall остается явным `uncollected_nano`;
- ledger и usage уникальны по request identity;
- exact replay не списывает повторно;
- zero multiplier сохраняет usage без charge row.

### 3.3 Router и perimeter

`gemini.api.apitoken.sale` и `router.apitoken.sale` уже пропускают `/v1beta/*`. Stateless router
делает для native Gemini один byte-faithful attempt и не знает billing/job state. После добавления
routes на Gemini plane batch автоматически станет доступен через оба hostname.

Router не должен получать batch queue, PostgreSQL, worker, result storage или retry logic. Нужны
только regression tests passthrough и обновление native contract docs. Caddy меняется лишь если
точный path выйдет за существующий `/v1beta/*`; для предлагаемого контракта это не требуется.

## 4. Рекомендуемое решение

```text
Client
  |
  | POST :batchGenerateContent
  v
Gemini plane HTTP adapter
  | auth + full validation + canonical request digest
  | encrypt item payloads
  v
PostgreSQL batch authority
  | atomic job + all item holds
  v
batch scheduler leader
  | fair job selection
  | one active batch item per Gemini profile
  v
existing Gemini pool/transport execution primitive
  | ordinary non-stream Code Assist generateContent
  | authoritative usage or typed error
  v
batch settlement outbox
  | release hold + charge/usage/ledger + terminal result
  v
GET /v1beta/batches/{id}
```

Это один новый bounded subsystem внутри существующей Gemini plane, а не новый deployable service.
Persistence и транзакции живут в `registry`; validation, selection, encryption, transport и
provider semantics — в `forward`; env, worker composition и HTTP routes — в `server`.

### 4.1 Публичный контракт MVP

Рекомендуется реализовать Google-shaped inline subset на native Gemini plane:

- `POST /v1beta/models/{model}:batchGenerateContent`;
- `GET /v1beta/batches` с bounded `pageSize` и opaque stable `pageToken`;
- `GET /v1beta/batches/{id}`;
- `POST /v1beta/batches/{id}:cancel`;
- `DELETE /v1beta/batches/{id}` только для terminal operation;
- `OPTIONS` и CORS, включая `DELETE`.

Create принимает только `batch.inputConfig.requests`; `fileName`, `webhookConfig`, `priority` и
embedding forms возвращают явный Google-shaped `400 INVALID_ARGUMENT`, а не silently ignored
fields. Официальные Python SDK fixtures задают модель только create path и опускают
`request.model`, поэтому вложенный model необязателен: отсутствующий наследует path model, а
присутствующий обязан совпадать с ним после canonicalization. Другой model отклоняется. Это
учитывает актуальную Google schema, в которой model формально присутствует и на batch, и на каждом
`GenerateContentRequest`, не ломая стандартный SDK shape. Discovery-visible `updateGenerateContentBatch` и
`updateEmbedContentBatch` также не входят в MVP и получают явный unsupported ответ, а не
непреднамеренный fallback.

Discovery подтверждает operation envelope, `metadata` типа `GenerateContentBatch`,
`batchStats`, `operations[]`, `nextPageToken` и пустые cancel/delete responses. До реализации их
конкретную JSON-сериализацию, timestamps, mixed-result semantics и pagination token нужно
закрепить официальными golden fixtures. Нельзя придумывать Google-compatible поля по памяти.

Допустимое локальное расширение — необязательный `Idempotency-Key` header:

- без header поведение как у Google: каждый POST создает новый batch;
- с header exact replay того же canonical body возвращает существующую operation;
- тот же header с другим body возвращает `409 ABORTED`;
- header никогда не отправляется upstream и хранится только как keyed digest.

Operation owner — `account_id`; создающий `key_id` остается attribution. Любой активный ключ того
же account может list/get/cancel/delete operation, что позволяет получить результат после key
rotation. Все SQL reads фильтруются по account до materialization. Malformed, unknown, deleted и
foreign-account IDs возвращают неразличимый native 404.

Key disable/revocation останавливает запуск еще не начатых item этого key и освобождает их holds;
уже dispatching item дочитывается и settles. Другой активный account key по-прежнему может читать
operation. Это должно быть одной transactionally tested policy, а не периодической догадкой HTTP
слоя.

### 4.2 Внешнее и внутреннее состояние

Внутренний item state должен быть богаче публичного Google state:

```text
queued -> claimed -> dispatching -> settlement_pending -> succeeded
   |         |             |                 |             failed
   |         |             |                 |             indeterminate
   +---------+-------------+-----------------+-----------> canceled
```

`claimed` с истекшей lease и без durable dispatch intent можно вернуть в `queued`. После перехода
в `dispatching` автоматический replay после crash запрещен: обычный Code Assist generation не имеет
проверенного idempotency key, поэтому transport мог выполнить модель до потери ответа. Такой item
становится `indeterminate`, hold закрывается по общей unknown-usage policy, а клиент получает
per-item error. Это честнее, чем тихо выполнить prompt второй раз.

Job state выводится из item rows, а не хранится как независимо изменяемый источник истины:

- все queued/claimed -> `PENDING`;
- есть dispatching/settlement_pending -> `RUNNING`;
- все terminal и все успешны -> `SUCCEEDED`;
- cancel requested, новых запусков нет, все terminal -> `CANCELLED`;
- job-level corruption/admission failure -> `FAILED`;
- deadline достигнут до terminal -> `EXPIRED`;
- смешанные item success/error после обычного выполнения остаются terminal operation с
  `inlinedResponses[]`; per-item error не превращает весь корректно обработанный batch в job-level
  failure.

Точное отображение mixed result на Google operation `done/error/state` фиксируется golden fixture.
Внутренний `indeterminate` наружу идет только как bounded per-item Google Status без profile,
request body или provider trace.

### 4.3 PostgreSQL schema

Нужна отдельная expand-only engine migration (следующий номер определяется непосредственно перед
реализацией; на дату исследования current version — 54). Минимальная схема:

`gemini_batch_jobs`:

- CSPRNG public `job_id` и resource name;
- `account_id`, creator `key_id`, public model и bounded display name;
- canonical request digest;
- nullable idempotency digest с unique `(account_id, digest)`;
- create/update/cancel/deadline/completed/delete/result-expiry timestamps;
- schema version и encryption policy version;
- никаких plaintext prompts, API keys или correctness-critical mutable counters.

`gemini_batch_items`:

- `(job_id, item_index)` и stable item request UUID;
- request digest и bounded metadata ciphertext reference;
- `hold_nano`, provider=`google`, payable multiplier, priced timestamp и exact tariff pin;
- state, next attempt time, terminal class;
- worker instance/epoch, claim generation и lease;
- selected opaque profile id только для internal reconciliation;
- отдельная logical request UUID и per-item execution group UUID; HTTP create operation не является
  execution identity дочерних turns;
- dispatch intent/actual-send evidence и bounded attempt count;
- typed authoritative Gemini usage и settlement identity;
- terminal result/error ciphertext reference и timestamps.

`gemini_batch_blobs`:

- `(job_id, item_index, kind=request|result)`;
- key id, nonce/ciphertext, plaintext length и digest;
- retention timestamp;
- PostgreSQL видит только opaque bytes.

`gemini_batch_settlement_outbox`:

- одна immutable settlement intent на item request UUID;
- disposition, actual/charge basis, typed usage и terminal result transition;
- pending/done/failed, attempts, next retry и bounded error;
- exact replay validation и unique item identity.

`gemini_batch_profile_leases`:

- opaque profile id как primary key;
- owning job/item, worker instance/epoch, claim generation и lease deadline;
- acquire/renew/release только вместе с matching item fence;
- lease нельзя украсть, пока прежний owner heartbeat жив, даже если он потерял scheduler leadership.

Job/item rows не ссылаются на raw API key. Stable `key_id` нужен для attribution и revocation
policy; денежный account остается authority. Пока key существует, batch settlement обновляет его
aggregates и получает raw key только внутри registry transaction для существующих nullable
`ledger.key`/`usage_events.key` полей. Для нового кода предпочтительно expand-only добавить nullable
non-secret `key_id` в ledger/usage и постепенно перевести read attribution на него. Физическое
удаление key row не должно блокировать release account hold или terminal settlement: key aggregates
уже исчезли, а ledger/usage сохраняют nullable raw-key attribution и batch `key_id`. Account removal
в текущем PostgreSQL authority является soft-delete; queued work такого account отменяется, а уже
dispatching work terminalizes и settles, чтобы не оставить holds.

### 4.4 Whole-batch admission и per-item money

Create выполняет одну PostgreSQL transaction:

1. авторизует активные account/key и берет idempotency advisory lock;
2. exact-replay validates canonical digest;
3. валидирует все items до money mutation;
4. вычисляет hold каждого item по обычному Gemini rate card, тому же provider multiplier и одному
   priced timestamp;
5. запрещает silent output cap: при недостатке денег отклоняется весь batch с 402;
6. блокирует account, затем key в существующем money lock order;
7. проверяет aggregate hold против account floor и key spend limit;
8. одним движением уменьшает balance, увеличивает account/key reserved totals;
9. вставляет job, items и encrypted payloads;
10. commit предшествует успешному operation response.

Если `maxOutputTokens` отсутствует, hold использует native model output ceiling. API должен явно
документировать, что bounded value уменьшает крупный hold; сервер не подменяет клиентский request.

Один aggregate reservation запрещен: partial success/cancel/error требуют независимо release/settle
каждого item. Но и admission-on-dispatch не подходит: он принимал бы в очередь financially unfunded
работу. Рекомендуемая комбинация — atomic aggregate admission через сумму per-item holds.

Обычные owner-bound `reservations` для ожидающих item не используются. Batch outbox должен вызывать
тот же внутренний settlement math, что обычный outbox, после refactor общей transaction helper:

- release exact item hold;
- collect actual до account floor;
- update account/key spend/reserved;
- insert unique ledger/usage;
- persist explicit uncollected amount;
- insert/validate immutable Gemini provider-turn event and cumulative profile spend;
- terminalize item/result и outbox в той же transaction.

Нельзя копировать settlement SQL во второй, постепенно расходящийся алгоритм. Common helper остается
в `registry` и принимает типизированный source (`interactive reservation` или `Gemini batch item`).
Worker сначала шифрует terminal response, затем одной registry command atomically сохраняет
ciphertext и immutable settlement/calibration intent. Публичный GET не считает item terminal и не
отдает result, пока outbox APPLY не завершил money, usage, ledger, calibration и terminal state. Так
crash между provider response и settlement не создает бесплатный видимый result, а quota poll не
может обогнать потерянный spend event.

Tariff pin обязателен на item: изменение compiled/hot tariff или account multiplier после create
влияет только на следующий batch. Customer charge и provider replacement cost остаются отдельными
integer nanoUSD facts. В `crates/metering` не добавляется batch price и не применяется множитель 50%.

Каждый item имеет собственную money/request identity. Batch ID нельзя использовать как общий
execution group: иначе insert-first-wins fencing разрешит списаться только первому item. Если позже
появятся несколько billable attempts одного item, group identity создается per item, а attempts
нумеруются внутри него.

### 4.5 Шифрование и retention

Добавляется отдельный keyring `CLAUDE_API_GEMINI_BATCH_DATA_KEYS` и active kid. Env читает только
`crates/server/src/config.rs`; готовый keyring передается в `forward`. Не следует переиспользовать
OAuth credential keyring: разные rotation и retention domains не должны связывать доступность
подписок с возможностью прочитать customer results.

`forward` уже зависит от `chacha20poly1305`. Используется XChaCha20-Poly1305 с новым random nonce для
каждого blob. AAD включает contract version, account id, job id, item index и kind, поэтому ciphertext
нельзя переставить между tenants/items/request/result. Logs, metrics и registry Debug не содержат
plaintext, metadata, ciphertext, nonce, customer/job/profile IDs или encryption key id.

Retention policy:

- nonterminal jobs/items и pending/failed settlement intents не удаляются;
- input payload хранится до terminal state плюс короткий recovery grace period;
- terminal inline results доступны 42 дня, как документированные шесть недель Google;
- после result expiry GET возвращает явный expired operation без payload;
- idempotency tombstone и canonical digest хранятся минимум 60 дней;
- ledger, usage, tariff/settlement evidence следуют существующей financial retention;
- DELETE удаляет customer payload/result и скрывает resource, но оставляет минимальный tombstone и
  financial evidence;
- pruning bounded, сначала blobs, затем terminal item/job metadata; active lease/outbox не pruned.

### 4.6 Scheduler и распределение по подпискам

Scheduler запускается только в Gemini provider mode и требует PostgreSQL authority. SQLite не
получает batch fallback. Один named PostgreSQL leader lease (`gemini_batch_dispatch`) не дает двум
blue-green поколениям одновременно подбирать новые items. Потеря leadership закрывает новые claims;
уже claimed work завершает bounded drain или оставляет durable state для reconciliation.

Выбор работы:

- `FOR UPDATE SKIP LOCKED` и fenced `owner_instance + owner_epoch + claim_generation`;
- fair order между account/job, а не полное выжигание первого большого batch;
- внутри job сохраняется item order только для результата, выполнение может быть out-of-order;
- bounded global batch concurrency;
- не более одного активного batch item на один profile;
- отдельные per-account active-job/item limits защищают fleet от одного клиента;
- queued item с будущим `next_attempt_at` не блокирует другие jobs.

Claim item и acquire `gemini_batch_profile_leases` происходят в одной transaction. Profile lease
renewal сверяет owner epoch, item и claim generation. Новый scheduler leader не может занять профиль,
пока старый live owner заканчивает actual dispatch; после смерти owner reconciler сначала
классифицирует item по durable actual-send boundary и только затем release profile lease.

Выбор подписки переиспользует `GeminiGateway::select_routed` без affinity hints: batch items
независимы, а цель режима — распределение. Hard quota/model cooling, authenticated status, fresh
quota preference, inflight rank, soft quota reserve и round-robin сохраняются. Batch-specific
per-profile active fence заставляет burst покрыть разные подписки; обычные interactive requests
по-прежнему могут идти на эти профили и не ждут batch semaphore.

Batch — lower-priority workload:

- interactive path не проверяет queue depth;
- scheduler предпочитает профили с минимальным текущим inflight;
- при hard cooling/нулевой quota item остается queued с provider-derived retry time;
- queue deadline ограничивает бесконечное ожидание;
- никакой номинальный subscription RPM/RPD не придумывается локально.

Нужны начальные compile/config bounds, окончательно выбранные после load test. Рекомендуемый стартовый
envelope: 20 MiB inline create body, до 1,000 items, до 1,000,000 суммарно запрошенных output tokens,
до 64 MiB сериализованного result set, до 100 nonterminal jobs на account. Лимиты должны быть меньше
реально измеренной безопасной PostgreSQL/worker границы и возвращать `RESOURCE_EXHAUSTED`, а не
частично принимать batch.

### 4.7 Execution primitive

Нельзя запускать item рекурсивным вызовом Axum `gemini_api`: текущая функция смешивает HTTP response
lifecycle, CORS, stream framing, affinity, public first-byte boundary и execution.

Из `crates/forward/src/gemini/api.rs` надо выделить внутренний non-stream execution primitive:

- вход: validated canonical request, public/wire model, auth-independent item context и scheduler
  policy;
- использует существующие pool/token/wrapper/transport/response/usage helpers;
- выход: sanitized native response + authoritative usage, typed retryable refusal или terminal error;
- сообщает durable actual-send boundary;
- не создает HTTP response и не владеет customer money;
- обычный `generateContent` adapter и batch worker становятся двумя callers одного primitive.

Существующий interactive behavior должен остаться byte-for-byte и test-for-test прежним. Batch
всегда non-stream upstream: клиент все равно не наблюдает incremental bytes, а terminal JSON проще
durably encrypt и settle. Per-item provider retries допустимы только по текущей доказанной
pre-result classification и внутри одного live worker lifecycle. После crash dispatching item не
replayed.

### 4.8 Cancellation и deletion

Cancel — idempotent intent:

- queued/claimed без dispatch intent atomically становятся canceled и release hold;
- scheduler больше не берет новые items этого job;
- ordinary Code Assist `generateContent` не имеет batch cancellation handle, поэтому dispatching
  item не объявляется остановленным; он дочитывается, сохраняет результат и settles;
- operation остается cancelling/running, пока каждый started item не terminal;
- уже полученные results сохраняются;
- повторный cancel возвращает то же состояние.

Delete не является cancel. Она разрешена только после terminal state, скрывает operation и запускает
payload cleanup. Для active job клиент сначала вызывает cancel. Это устраняет неоднозначность
официальных описаний delete и не позволяет потерять holds/outbox.

### 4.9 Ошибки и retry policy

| Ситуация | Item action | Money |
|---|---|---|
| Local validation | весь create rejected | mutation отсутствует |
| 401/token до actual send | выбрать другой профиль в bounded attempt | hold остается |
| 403 request rejection после bounded fleet classification | terminal item error | hold released, charge 0 |
| 429 с retry/reset | вернуть item в retry scheduling, пока live owner может безопасно доказать no output | hold остается |
| 5xx/transport до actual send | bounded rotate/retry | hold остается |
| Ошибка после actual send без terminal usage | `indeterminate`, без replay | общая unknown-usage policy, explicit pool loss if zero |
| 2xx без требуемого usage | `indeterminate` protocol error, без replay | unknown-usage policy; usage/charge не выдумывать |
| 2xx с usage | result + settlement outbox | exact per-item charge |
| Settlement DB transient error | result не публикуется terminal до outbox apply | outbox retries |
| Permanent settlement conflict | item/outbox quarantined, alert | hold не silently released |

Политика unknown usage должна совпадать с фактическим default PostgreSQL reconciler и быть явно
описана в `GEMINI_PROVIDER.md`; сейчас текст provider doc о full-hold stream fallback требует
сверки с default-off operator switch до начала реализации batch.

## 5. Наблюдаемость и операции

На `/metrics` нужны только fixed-cardinality series:

- queue jobs/items по closed state;
- oldest queued age;
- claimed/dispatching workers;
- completion/error/cancel/expired totals по bounded reason class;
- indeterminate-after-send total;
- batch settlement outbox pending/failed и oldest age;
- reserved batch nanoUSD aggregate как decimal-safe internal gauge or bounded integer series;
- encrypted payload/result bytes and prune failures;
- scheduler leader/fencing failures;
- per-profile batch activity только aggregate histogram/gauge без profile label.

`/gemini-subs` можно additive расширить fleet-level batch summary, но нельзя отдавать account, job,
item, prompt или result. Новые alerts обязательны для stale queue, failed settlement outbox,
indeterminate spike, missing scheduler leader при backlog и retention failure; каждому alert нужен
runbook section в `docs/ops/MONITORING.md` в том же commit.

Interactive Gemini readiness не зависит от batch backlog, failed customer item или отсутствующего
старого batch data key. Batch subsystem имеет отдельный health/admission gate: при недоступной
authority, неполном keyring или failed terminal pipeline новые batch calls и claims отвечают 503,
существующие holds остаются durable, метрики/alerts горят, но `/ready` не depool-ит обычный Gemini
`generateContent`. Discovery рекламирует `batchGenerateContent` только когда эта отдельная gate
готова; после публикации временная деградация отражается честным 503, а не удалением метода из
кэширующегося каталога.

Graceful shutdown:

1. прекратить новые batch claims;
2. дать dispatching items bounded drain;
3. checkpoint terminal results/outbox;
4. истекший drain не replay и не invent settlement;
5. flush Gemini calibration FIFO и money outboxes;
6. завершить process.

## 6. Этапы реализации

### Этап 0. Contract fixtures и решения

- Получить официальные sanitized wire fixtures из Python и JavaScript SDK; сверить их с актуальным
  v1beta Discovery document.
- Зафиксировать exact create/list/get/cancel/delete envelopes, mixed item errors и pagination.
- Проверить модельный allowlist Batch API, но не переносить Google tariff/SLO.
- Утвердить MVP limits, 48-hour execution deadline, 42-day result retention и 60-day tombstone.
- Исправить расхождение unknown-usage текста и runtime policy в `GEMINI_PROVIDER.md`.

Exit gate: contract tests могут быть написаны без догадок; open questions из раздела 8 закрыты.

### Этап 1. Migration-only expansion

- Добавить пустые batch job/item/blob/outbox tables, constraints и indexes.
- Зарегистрировать migration/current schema version и schema tests.
- Migration не создает jobs, не меняет balances и не имеет runtime reader/writer.
- Отдельно merge/deploy migration и дождаться GREEN `deploy/migration` + `deploy/watchdog`.

Exit gate: old runtime работает с расширенной schema; rollback binary не затронут.

### Этап 2. Registry authority, без публичных routes

- Реализовать atomic create + all-item holds + idempotency.
- Реализовать scoped list/get/cancel/delete/prune.
- Реализовать leader/item leases и owner/claim fencing.
- Реализовать profile leases и атомарный item+profile claim.
- Вынести общий settlement transaction helper и подключить batch outbox с atomic result,
  ledger/usage и calibration apply.
- Добавить real-PostgreSQL fault matrix, concurrency и crash/replay tests.
- SQLite отвечает typed unsupported и не имитирует durability.

Exit gate: все money equations и restart cases доказаны на real PostgreSQL.

### Этап 3. Encryption и execution core

- Добавить dedicated batch data keyring config и secret delivery.
- Реализовать AEAD blob codec и rotation/read-old-write-active policy.
- Выделить общий non-stream Gemini execution primitive.
- Подключить scheduler leader, fair claims, one-batch-item-per-profile и shutdown drain.
- Оставить feature default-off; public parser продолжает возвращать 404.

Exit gate: mock upstream выполняет jobs через несколько profiles, restart продолжает queue, plaintext
не появляется в DB/logs.

### Этап 4. Producer-first public API, default-off

- Добавить exact Google-shaped inline routes в Gemini plane.
- Добавить auth-before-body, account isolation, CORS DELETE и explicit unsupported fields.
- Добавить optional idempotency extension.
- Обновить `GEMINI_PROVIDER.md`, `UNIFIED_ROUTER.md`, crate contracts и public examples в том же
  producer commit.
- Router получает только passthrough/identity regression tests, не job code.
- Model discovery добавляет `batchGenerateContent` только когда endpoint включен и проверен.

Exit gate: official SDK compatibility tests проходят на direct Gemini host и unified router.

### Этап 5. Resilience, observability и controlled canary

- Добавить metrics, alerts, Grafana/admin fleet summary и runbooks.
- Прогнать multi-owner fault injection, kill-at-every-boundary matrix и load/fairness tests.
- Выполнить controlled internal batch с explicit aggregate budget: несколько items, несколько
  профилей, partial error, cancel, restart и exact settlement.
- Проверить no-discount charge parity с теми же requests через ordinary generateContent.
- Сохранить sanitized exact-SHA evidence; failed paid run не replay.

Exit gate: provider output/usage, profile distribution, customer ledger, quota calibration и restart
recovery сходятся на production-GREEN implementation SHA.

### Этап 6. Публикация

- Отдельным commit включить reviewed systemd flag и discovery method.
- Обновить customer docs без заявления Google discount/SLA/file support.
- После deploy выполнить public create -> poll -> terminal result smoke через оба hostname.
- Наблюдать queue age, settlements, indeterminate и interactive latency в soak window.
- Rollback выключает admission новых jobs, но совместимый runtime обязан дочитать уже принятые jobs;
  нельзя откатываться на binary, который не понимает их holds/outbox.

Exit gate: точный published SHA GREEN, queue drains, нет balance divergence и stale holds.

### Этап 7. Отдельные будущие расширения

- product-owned encrypted Files API/JSONL input и streamed result download;
- signed webhooks с SSRF defense, retry budget и secret rotation;
- image-output batch с отдельными storage/egress limits;
- universal provider-neutral batch contract, только если появится второй реальный provider;
- direct Google Batch adapter, только если появится отдельная metered credential/tariff authority.

Каждое расширение меняет threat model и не должно попадать в MVP «заодно».

## 7. Обязательная тестовая матрица

### Contract и auth

- Python/JavaScript SDK create/list/get/cancel/delete direct + router, включая пустые
  cancel/delete responses;
- exact golden JSON и unknown-field behavior;
- auth до buffering oversized/chunked body;
- query credential rejected;
- same-account/different-key access;
- foreign/unknown/deleted IDs имеют одинаковый 404;
- stable cursor pagination без cross-account rows;
- explicit rejection file/webhook/priority/update/embedding/cross-model/image-output.

### Money и idempotency

- all-or-nothing create при insufficient balance или key limit;
- concurrent creates одного account не проходят ниже floor;
- exact idempotency replay и conflicting digest;
- сумма item holds равна изменению account/key reserved;
- tariff/multiplier edit после create не reprices items;
- success/error/cancel release каждый hold ровно один раз;
- actual > hold сохраняет full usage и explicit uncollected;
- zero multiplier: usage есть, charge row нет;
- два item одного batch оба списываются; batch id не становится winner group;
- duplicate/conflicting settlement outbox replay;
- key disable во время queued/dispatching;
- account soft-delete во время queued/dispatching;
- settlement после удаления key row не теряет account conservation.

### Worker и crash recovery

- два owners борются за scheduler leader;
- `SKIP LOCKED` и claim generation исключают двойной claim;
- profile lease не допускает два active batch item на профиль при leader loss/blue-green overlap;
- crash до claim, после claim, до dispatch intent, после actual send, после response, после encrypted
  result, после outbox insert и после money commit;
- stale owner/claim не может terminalize item;
- post-send unknown не replay;
- 401/403/429/5xx/transport классификация;
- partial success, cancel race, expiry и delete;
- shutdown/deploy не ждет весь 48-hour queue и не теряет active state;
- calibration turn FIFO не обгоняется quota poll.

### Privacy, limits и performance

- DB/log/metrics snapshots не содержат prompt, result, API key, metadata или profile id;
- AEAD swap между account/job/item/kind не decrypt;
- active/old key rotation и missing key fail closed;
- body/item/output/result/job/account limits;
- bounded result assembly без одного giant in-memory JSON copy;
- fair progress двух accounts при одном крупном batch;
- batch workload не добавляет wait/reject на interactive path;
- load test PostgreSQL locks, outbox throughput, scheduler wakeup и result reads.

Существующие Gemini API/stream/disconnect/missing-usage tests, registry Stage 2 fault matrix,
router native passthrough tests, `cargo test --locked --workspace`, rotation and universal chat smoke
остаются green.

## 8. Решения, которые надо подтвердить до кода

Рекомендованные defaults перечислены ниже. Изменение любого ответа существенно меняет schema или
public contract, поэтому их надо закрыть в Этапе 0.

| Вопрос | Рекомендация |
|---|---|
| API shape | Gemini Developer API inline-compatible subset |
| Где доступен | direct Gemini host и native `/v1beta/*` unified-router passthrough |
| Execution | локальная очередь обычных non-stream Code Assist turns |
| Pricing | тот же normal tariff + account Google multiplier, без batch discount |
| Admission | весь batch атомарно, per-item holds |
| Affinity | выключена для независимых items; quota/inflight/cursor selection остается |
| Concurrency | bounded global, один batch item на profile, interactive не блокируется |
| Create idempotency | Google-compatible non-idempotent default + optional `Idempotency-Key` |
| Ownership | account-scoped, creator key only attribution/revocation policy |
| Cancel | queued stops immediately; dispatching drains best effort |
| Crash after send | indeterminate, no automatic replay |
| Queue deadline | 48 часов |
| Result retention | 42 дня; financial evidence отдельно |
| MVP media | existing bounded inline inputs; image-output model excluded |
| Large input | Files API отдельным этапом |

## 9. Definition of Done

Batch mode считается готовым, только когда одновременно доказано:

- official SDK create/poll/result/cancel/delete compatibility на опубликованном contract subset;
- один accepted batch распределяет items минимум по двум доступным subscriptions без client hints;
- batch переживает Gemini blue-green replacement и продолжает progress;
- каждый successful item имеет terminal native response, authoritative usage, unique usage/ledger
  identity и exact tariff pin;
- errors/cancel/expiry освобождают holds ровно один раз;
- одинаковый model/usage/tariff/multiplier vector в ordinary и batch settlement дает один pricing
  basis, без дополнительной скидки;
- post-send crash не приводит к автоматическому duplicate generation;
- plaintext customer data отсутствует в DB/logs/metrics;
- queue/outbox/retention alerts и runbooks установлены;
- interactive Gemini latency/admission semantics не изменены;
- exact implementation SHA прошел repository gate, controlled live batch и public smoke;
- документация честно называет inline-only/local execution ограничения и не обещает Google Batch
  tariff или SLA.

## 10. Почему отклонены альтернативы

**Прямой passthrough в Google Batch.** Подписки используют private Code Assist OAuth/project
identity, а не customer Developer API project. Это не дает нужной tenant isolation и меняет
replacement cost/tariff.

**Queue в unified router.** Router stateless, HTTP-only и не имеет billing/registry dependencies.
Job ownership там нарушит его главную границу и усложнит zero-downtime.

**Queue в commerce worker.** Public provider data plane не должен зависеть от private Control API и
commerce database. Engine остается authority денег и исполнения.

**In-memory tasks по примеру текущего media gateway.** Такой read model теряется при deploy и не
может владеть многочасовыми holds/results. Он полезен как transport pattern, но не как authority.

**Одна reservation на batch.** Нельзя корректно settlement partial success/cancel/error и показать
per-item usage.

**Reserve непосредственно перед каждым item.** Принятый job может оказаться полностью unfunded через
час; create response тогда обещает работу, которую authority не допустила.

**Долгий lease обычной reservation.** Он привязан к умершему owner epoch и конфликтует с Stage 2
reconciliation/blue-green fencing.

**Отдельная скидка 50%.** Это тариф Google для их metered Batch API. Наша очередь тратит обычную
subscription quota и должна использовать текущий normal rate card.
