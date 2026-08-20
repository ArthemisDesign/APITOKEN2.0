# План реализации Gemini Batch Mode

Статус: план утвержден (Этап 0 выполнен 2026-08-21), runtime не реализован.

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
  маскируются под подтвержденные Google-возможности;
- batch — lower-priority workload с отдельным квота-потолком: batch-задачи высаживают 5-часовые
  окна (`gemini-5h`) подписок не глубже 15% остатка (потолок использования 85%); за этим порогом
  диспетчер приостанавливает batch-dispatch до provider reset или до восстановления headroom.
  Interactive-трафик этот потолок не имеет и может дожимать остаток до обычных hard-правил.

Продуктовая позиция — «обрезанная» версия Gemini Batch Mode: локальная очередь поверх пула
подписок без скидки и без Google Batch SLO, но с максимально полной поддержкой того, что умеет
настоящая реализация Google Batch API в тех частях, которые реально совместимы с нашим
subscription-backed transport (см. §2.1). Файловый ввод поддерживается собственным client-side
Files API (§4.1) — включая возможность, которой нет у официального Google Batch API (там batch
принимает file только как JSONL ввода, а не как медиа для отдельных запросов).

В первую версию не входят:

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

Этот раздел — зафиксированный контракт. Он собран из публичных источников без живого доступа к
официальному API: актуальный v1beta Discovery document
(`https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta`, снят 2026-08-21),
официальная документация batch-api/files и исходники Python SDK (`google/genai/batches.py`,
`types.py`). Пометкой **[не подтверждено]** отмечены места, где публичных данных нет и решение —
наш самостоятельный дизайн, допустимый по правилу «не маскировать догадки под Google-факт».

### 2.1 Routes и long-running operation

| Операция | REST route | Ответ |
|---|---|---|
| Создать | `POST /v1beta/models/{model}:batchGenerateContent` | `Operation` |
| Список | `GET /v1beta/batches` | `ListOperationsResponse` |
| Состояние и результат | `GET /v1beta/batches/{id}` | `Operation` |
| Отмена | `POST /v1beta/batches/{id}:cancel` | `Empty` |
| Удаление | `DELETE /v1beta/batches/{id}` | `Empty` |

List — это стандартный метод operations (`generativelanguage.batches.list`): query-параметры
`pageSize` (int32), `pageToken`, `filter`, `returnPartialSuccess`; ответ содержит `operations[]`
и `nextPageToken`. Мы поддерживаем `pageSize`/`pageToken`; `filter` и `returnPartialSuccess`
отклоняем `INVALID_ARGUMENT`/`UNIMPLEMENTED` **[не подтверждено поведение Google по умолчанию —
наш осознанный subset]**.

Create body — `BatchGenerateContentRequest` с единственным полем `batch`
(`GenerateContentBatch`). Клиент заполняет `displayName` (required у Google), `model` (required,
`models/{model}`; наш SDK-совместимый fallback — наследование из path, §4.1), `inputConfig`
(required) и опциональный `priority`. Остальные поля output-only.

### 2.2 Operation envelope и `GenerateContentBatch` metadata

Сырой REST-ответ Google (подтверждено REST-курлом в официальной документации): `done` (bool),
`metadata` — это сериализованный `GenerateContentBatch` (SDK читает `metadata.state`,
`metadata.model`, `metadata.output`, …; `t_job_state` конвертирует wire `BATCH_STATE_*` в
SDK-имена `JOB_STATE_*`), `response` на success, `error` (`google.rpc.Status`) на
FAILED/CANCELLED. Python SDK поверх не показывает `batchStats` клиенту: его `BatchJob` имеет
`completion_stats` с пометкой «not supported in Gemini API». То есть `batchStats` существует
только на сыром wire; наш публичный контракт его реализует (§4.2), но SDK-совместимость от него
не зависит.

Поля `GenerateContentBatch` (Discovery): `name` (`batches/{batch_id}`, output-only), `model`,
`displayName`, `inputConfig`, `state` (output-only), `batchStats` (output-only), `output`
(output-only), `createTime`, `updateTime`, `endTime` (google-datetime), `priority`
(optional, string-int64, default 0, отрицательные разрешены; мы принимаем и echo'им, но
игнорируем — §4.1). Отдельного `startTime` в Developer API нет (это Vertex-поле SDK).

`batchStats` (Discovery, все поля string-int64, output-only): `requestCount`,
`successfulRequestCount`, `failedRequestCount`, `pendingRequestCount`.

Состояния (Discovery enum): `BATCH_STATE_UNSPECIFIED`, `BATCH_STATE_PENDING`, `BATCH_STATE_RUNNING`,
`BATCH_STATE_SUCCEEDED`, `BATCH_STATE_FAILED`, `BATCH_STATE_CANCELLED`, `BATCH_STATE_EXPIRED`.
Документация описывает EXPIRED как «running или pending дольше 48 часов, результатов нет».

### 2.3 Ввод: inline и файл

`InputConfig` содержит ровно одно из: `requests` (`InlinedRequests{requests: InlinedRequest[]}`)
или `fileName` (`files/...`). `InlinedRequest` = `request` (`GenerateContentRequest`, required) +
`metadata` (optional object, произвольные ключи; официальный пример кладет туда клиентский ключ:
`"metadata": {"key": "request-1"}`).

Файл — JSONL, одна строка = `{"key": ..., "request": {...}}`; ключ — user-defined, возвращается в
выводе для корреляции (официальная документация). Лимиты: inline create body ≤ 20 MB; input-файл
≤ 2 GB. Документированного жесткого максимума числа запросов на batch в публичных источниках нет
**[не подтверждено]**: наш предел — config-driven, цель «не ниже практических Google-объемов»,
финальное значение фиксируется измерением в Этапе 5 (§4.6).

### 2.4 Вывод: симметрия вводу

`GenerateContentBatchOutput` содержит ровно одно из:

- `inlinedResponses` (`InlinedResponses{inlinedResponses: InlinedResponse[]}`) для inline ввода;
  элемент — `response` (`GenerateContentResponse`) или `error` (`google.rpc.Status`) + echo
  `metadata`; порядок совпадает с порядком входных requests;
- `responsesFile` (имя файла) для файлового ввода; JSONL-строки в порядке входных requests
  **[не подтверждено, несут ли строки файл-вывода клиентский `key`; документация гарантирует
  корреляцию через «user-defined key … will have its response annotated with the same key name» —
  мы реализуем `{"key": ..., "response"|"error": ...}`]**.

Mixed-result семантика: batch с частичными per-item ошибками завершается
`BATCH_STATE_SUCCEEDED` (не FAILED): официальная документация предписывает после успеха смотреть
`batchStats.failedRequestCount` и разбирать строки файла/элементы inline output на предмет
per-item `Status`-ошибок. Job-level `error` — только для FAILED/CANCELLED. Точное соответствие
`done/error/state` при mixed result этому правилу **[не подтверждено wire-фактурой; принято по
тексту документации]**.

### 2.5 Жизненный цикл и лимиты Google

Queue/run deadline — 48 часов (EXPIRED), target turnaround — 24 часа без SLA, результаты хранятся
6 недель. Создание неидемпотентно: повторный POST создает новую operation. Files API: до 2 GB на
файл, 20 GB на проект, TTL 48 часов, state-машина `PROCESSING → ACTIVE | FAILED` (enum включает
`STATE_UNSPECIFIED`), `File` несет `sha256Hash`, `uri`, `downloadUri`, `expirationTime`,
`source` (`UPLOADED|GENERATED|REGISTERED`), а `error` — `Status` при FAILED. У Google Files API
скачивания клиентских файлов нет (download доступен только для сгенерированных файлов, в т.ч.
batch output); мы добавляем `:download` как осознанное расширение, потому что содержимое
принадлежит нам. Discovery также объявляет `PATCH /v1beta/batches/{name}:updateGenerateContentBatch`
и `updateEmbedContentBatch` — вне MVP, явный unsupported (§4.1).

Google продает Batch API по отдельному тарифу, обычно на 50% дешевле interactive inference. Эта
скидка неприменима к нашей реализации: под капотом выполняются обычные subscription-backed turns, а
не тарифицируемые Google Batch jobs. В публичной документации нельзя называть локальную очередь
Google Batch со скидкой или обещать его SLO. Модельный охват: Google поддерживает «range of Gemini
models» с модальностями интерактивного API; мы не делаем allowlist и пускаем все опубликованные
текстовые Gemini-модели (image-output исключен из MVP, §1).

Vertex AI batch — другой контракт: OAuth/IAM, project/location resources, GCS/BigQuery input/output
и другой lifecycle. Смешивать его с Developer API или заявлять Vertex compatibility не нужно.

Официальные источники, на которых зафиксирован контракт:

- <https://ai.google.dev/gemini-api/docs/batch-api>
- <https://ai.google.dev/gemini-api/docs/files>
- `https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta` (snapshot 2026-08-21)
- <https://github.com/googleapis/python-genai/blob/main/google/genai/batches.py> и `types.py`
- <https://github.com/googleapis/js-genai/blob/main/src/batches.ts>
- <https://cloud.google.com/vertex-ai/generative-ai/docs/multimodal/batch-prediction-gemini>

Правило на будущее: любое место, где публичные источники молчат, реализуется самостоятельно и
помечается **[не подтверждено]**; клиентская документация не выдает такие места за
Google-совместимость.

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

### 3.2 Квотная картина: 5-часовые и недельные окна

Профили Gemini опрашивают два независимых провайдерских сигнала:

- per-model quota catalogue (`v1internal:fetchAvailableModels`) с `remainingFraction`/`resetTime` —
  управляет model/profile cooling и soft steering в `select_routed_inner`;
- quota summary (`v1internal:retrieveUserQuotaSummary`) — принимаются только точные buckets
  `gemini-5h` и `gemini-weekly`; по ним строится exact window calibration
  (`WindowCalibration`, `capacity_reports`, `GeminiWindowCapacityReport` с
  `remaining_fraction_units`), персистится в plan-scoped authority и отражается в admin-статусе.

Эти два сигнала сегодня используются по-разному: catalogue влияет на ранжирование интерактивных
запросов, а 5h/weekly summary — на калибровку емкости и отчетность. Batch-политика 85%-потолка
(§4.6) опирается именно на `gemini-5h` bucket summary как на окно, у которого есть известный
provider reset. Если в ответе `retrieveUserQuotaSummary` окажутся дополнительные buckets
(например, per-model), контракт фильтра остается fail-closed: чужие buckets не трактуются как
5h-окно.

### 3.3 Files API сегодня

Клиентские Files API ссылки (`fileData.fileUri`) сейчас fail-closed отклоняются в
`validate_no_file_data` до dispatch: файл живет в Google-проекте клиента, а наш pooled identity его
не видит, поэтому каждый профиль отвечает `PERMISSION_DENIED`, что выглядело как флот-аут.
Собственного upload-хранилища у Gemini plane нет — клиенту предлагается inlineData. Batch-режим
вводит первое собственное зашифрованное blob-хранилище файлов (§4.1), которое снимает это
ограничение для batch-items, не меняя поведение обычного `generateContent`.

### 3.4 Billing authority

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

### 3.5 Router и perimeter

`gemini.api.apitoken.sale` и `router.apitoken.sale` уже пропускают `/v1beta/*`. Stateless router
делает для native Gemini один byte-faithful attempt и не знает billing/job state. После добавления
routes на Gemini plane batch автоматически станет доступен через оба hostname. Маршрут
`/upload/v1beta/*` (multipart resumable upload) сегодня в perimeter не объявлен: его надо добавить
в Caddy/router config одним commit'ом с Files API routes (§4.1), с отдельным body-limit для upload
и regression tests passthrough.

Router не должен получать batch queue, PostgreSQL, worker, result storage или retry logic. Нужны
только regression tests passthrough и обновление native contract docs. Caddy меняется лишь для
нового `/upload/v1beta/*` префикса; для batch routes внутри существующего `/v1beta/*` это не
требуется.

## 4. Рекомендуемое решение

```text
Client
  |
  | POST :batchGenerateContent (inline requests или inputConfig.fileName)
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
  | batch 5h headroom gate: remaining > 15% (§4.6)
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

Create принимает `batch.inputConfig.requests` (inline) и `batch.inputConfig.fileName` — ссылку на
файл нашего собственного Files API с JSONL-вводом (одна строка = `{"key": ..., "request": {...}}`,
как у Google; `key` — клиентский opaque идентификатор строки). Расширение сверх Google: внутри
item-запросов (и inline, и из JSONL) разрешается `fileData.fileUri` со ссылкой на наш собственный
файл — сервер резолвит ее в bytes до dispatch, проверяет mime type/size и передает upstream как
`inlineData`. Ссылки на внешние Google `files/…` ресурсы остаются отклоненными тем же
`FILE_URI_UNSUPPORTED` ответом, что сегодня: pooled identity их не видит.

Форма результата симметрична вводу, как у Google:

- inline ввод → terminal operation содержит `metadata.output.inlinedResponses[]` в порядке входных
  элементов, каждый элемент несет `response` или `error` и echo исходного `metadata`;
- файловый ввод → terminal operation содержит `metadata.output.responsesFile` — ссылку на новый
  файл нашего Files API с JSONL-строками `{"key": ..., "response"|"error": ...}`; `key` из входной
  строки возвращается без изменений, `inlinedResponses` в этом случае отсутствует. Output-файл
  создается atomically при terminalization job, принадлежит тому же account, скачивается обычным
  `:download`; его TTL отсчитывается от завершения job и ограничен 42-дневным result retention
  (§4.5), а не стандартными 48 часами upload TTL.

`priority` принимается как у Google: поле валидируется по wire-типу (int), сохраняется и echo'ится
в operation metadata, но функционально игнорируется — наш scheduler имеет собственный детерминизм
(fair order между account/job, 5h headroom gate) и не дает одному клиенту обгонять других. Это
осознанный no-op для wire-совместимости, а не silently dropped field: поведение документируется в
клиентской документации. `webhookConfig` и embedding forms возвращают явный Google-shaped
`400 INVALID_ARGUMENT`, а не silently ignored fields. Официальные Python SDK fixtures задают модель
только create path и опускают `request.model`, поэтому вложенный model необязателен: отсутствующий
наследует path model, а присутствующий обязан совпадать с ним после canonicalization. Другой model
отклоняется. Это учитывает актуальную Google schema, в которой model формально присутствует и на
batch, и на каждом `GenerateContentRequest`, не ломая стандартный SDK shape. Discovery-visible
`updateGenerateContentBatch` и `updateEmbedContentBatch` также не входят в MVP и получают явный
unsupported ответ, а не непреднамеренный fallback.

Собственный Files API (Google-shaped subset, account-scoped, зашифрованное хранение):

- `POST /upload/v1beta/files` — multipart resumable upload; ответ `file` resource с
  `name=files/{id}`, `displayName`, `mimeType`, `sizeBytes`, `createTime`, `updateTime`,
  `expirationTime` (TTL 48 часов как у Google), `state: PROCESSING|ACTIVE|FAILED`;
- `POST /v1beta/files` — metadata-only create (для parity с SDK);
- `GET /v1beta/files`, `GET /v1beta/files/{id}`, `DELETE /v1beta/files/{id}`;
- `GET /v1beta/files/{id}:download` — честный download, т.к. содержимое принадлежит нам.

Ограничения MVP: файл не используется обычным синхронным `generateContent` (там `fileData` по-
прежнему отклоняется `FILE_URI_UNSUPPORTED` с указанием inlineData как альтернативы); ссылка на файл
валидна только внутри batch-ввода; файл привязан к `account_id`, виден любому активному ключу того
же account; при создании batch делается durable reference (`file_id`) в item row. **Решение
(подтверждено владельцем продукта): удаление файла, на который есть живые ссылки незавершенных
jobs, запрещено до terminal/expiry этих jobs** — `DELETE /v1beta/files/{id}` в этом случае
возвращает `FAILED_PRECONDITION` со списком блокирующих jobs в sanitized виде. Тело резолвится
один раз при диспетче item, не кэшируется вне зашифрованного blob.

`batchStats` считается derived-on-read из item rows на каждый GET (подробно — §4.2), никаких
mutable counters. JSON-сериализация полей зафиксирована в §2.2 по Discovery document (string-int64
счетчики, google-datetime timestamps); pagination token — opaque stable cursor нашего дизайна
**[не подтверждено]**, Google его формат публично не раскрывает.

Допустимое локальное расширение — необязательный `Idempotency-Key` header:

- без header поведение как у Google: каждый POST создает новый batch;
- с header exact replay того же canonical body возвращает существующую operation;
- тот же header с другим body возвращает `409 ABORTED`;
- header никогда не отправляется upstream и хранится только как keyed digest.

Operation owner — `account_id`; создающий `key_id` остается attribution. Любой активный ключ того
же account может list/get/cancel/delete operation, что позволяет получить результат после key
rotation. Все SQL reads фильтруются по account до materialization. Malformed, unknown, deleted и
foreign-account IDs возвращают неразличимый native 404. Тот же account-scope применяется к Files
API resources.

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

Отдельный источник возврата в `queued` — квота-стоп: item не dispatch'ится, потому что флот уперся
в 85%-потолок 5h-окна (§4.6). Такой item не считается ни ошибкой, ни retry-attempt: он просто ждет
ближайшего provider reset или освобождения headroom с provider-derived `next_attempt_at`.

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

**`batchStats` — derived-on-read, mutable counters запрещены.** Google-форма объекта —
`requestCount`, `successfulRequestCount`, `failedRequestCount`, `pendingRequestCount`. Все четыре
значения на каждый GET считаются SQL-агрегатом по item rows этого job (grouped by state class),
операция дешевая при индексе `(job_id, state)`. Хранить их как обновляемые счетчики в
`gemini_batch_jobs` нельзя, и это зафиксировано в схеме (§4.3) как инвариант, потому что:

1. **Второй источник истины неизбежно расходится.** Item state меняют несколько независимых
   акторов: scheduler claim, worker dispatch, settlement outbox apply, cancel path, expiry
   reconciler. Каждый из них может упасть между обновлением item row и обновлением счетчика —
   после crash счетчик навсегда отличается от факта, и нет способа доказать, какое значение
   верное, кроме полного пересчета (то есть derived-on-read все равно остается эталоном).
2. **Lost update под конкуренцией.** Worker settlement и cancel одного job идут в разных
   транзакциях; неатомарный `UPDATE counters = counters + 1` или read-modify-write вне одной
   блокировки теряет приращения. Поддерживать это корректно — значит воспроизвести вторую
   locking-матрицу рядом с уже существующей money/state fencing, без новой информации.
3. **Транзакционная простота.** Derived-on-read не требует ни одной дополнительной записи на hot
   path settlement: terminal transition item — это один row update, а stats появляются «бесплатно»
   при следующем чтении. Консистентность stats с видимым результатом гарантируется тем же
   правилом, что и раньше: GET не считает item terminal, пока outbox APPLY не завершен, поэтому
   клиент никогда не видит `successfulRequestCount`, опережающий реально видимые результаты.

Mixed-result отображение на operation зафиксировано по официальной документации (§2.4): частичные
per-item ошибки завершают job как `BATCH_STATE_SUCCEEDED` с per-item `Status` внутри output, а
job-level `error` — только для FAILED/CANCELLED. Внутренний `indeterminate` наружу идет только как
bounded per-item Google Status без profile, request body или provider trace; в `batchStats`
indeterminate учитывается в `failedRequestCount` (для клиента это отказ item) **[не подтверждено
wire-фактурой; принято по тексту документации]**.

### 4.3 PostgreSQL schema

Нужна отдельная expand-only engine migration (следующий номер определяется непосредственно перед
реализацией; на дату исследования current version — 54). Минимальная схема:

`gemini_batch_jobs`:

- CSPRNG public `job_id` и resource name;
- `account_id`, creator `key_id`, public model и bounded display name;
- canonical request digest;
- nullable idempotency digest с unique `(account_id, digest)`;
- create/update/cancel/deadline/completed/delete timestamps; `result_expiry` nullable до terminalization
  и выставляется atomically как `completed + 42 days`, чтобы 48-часовая queue не сокращала клиентский
  срок чтения результатов;
- принятый `priority` (echo-only, см. §4.1) и форма ввода/вывода (`input_kind=inline|file`,
  `output_file_id` для file-ввода);
- schema version и encryption policy version;
- никаких plaintext prompts, API keys или correctness-critical mutable counters (`batchStats`
  считается derived-on-read, см. §4.2).

`gemini_batch_items`:

- `(job_id, item_index)` и stable item request UUID;
- nullable bounded client `key` из входной JSONL-строки (opaque passthrough в output-файл;
  хранится как небольшое текстовое значение, не secret);
- request digest и bounded metadata ciphertext reference;
- nullable `file_id` reference на файл ввода и per-item file references для `fileData`;
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

`gemini_batch_files`:

- CSPRNG public `file_id` и resource name `files/{id}`;
- `account_id`, bounded display name, mime type, size bytes, sha256 digest;
- `source_kind=client_upload|batch_output` — output-файлы создаются только terminalization пути
  job и не могут быть удалены клиентом до result expiry (как у Google);
- blob ciphertext хранится chunked в отдельной `gemini_batch_file_chunks` authority: ordered
  `(file_id, chunk_index)`, per-chunk key id/nonce/ciphertext/plaintext length/digest. Один inline
  PostgreSQL `bytea` запрещен: varlena не покрывает честный 2 GB контракт и не позволяет resumable
  upload без giant in-memory copy;
- state `processing|active|failed`, failure reason class;
- create/update/expiration timestamps (TTL 48 часов как у Google для upload; для batch_output —
  не короче result retention ссылающегося job, см. §4.5);
- никаких plaintext bytes и никаких raw API keys.

`gemini_batch_settlement_outbox`:

- одна immutable settlement intent на item request UUID;
- disposition, actual/charge basis, typed usage и terminal result transition;
- полный immutable `ProviderTurnCalibrationEvent` payload: selected opaque profile/subject, model,
  service tier/geography, completed/priced timestamps, tariff schedule и disjoint API nanoUSD legs;
  без этого money + usage + calibration нельзя применить atomically или exact-replay validate;
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
3. валидирует все items до money mutation (включая резолвинг `fileData` ссылок в существующие
   active файлы того же account и проверку суммарного resolved size против per-item/aggregate
   лимитов);
4. проверяет Google-масштабные лимиты: число items (строк JSONL или inline requests) не превышает
   документированный Google максимум, размер файла ввода — 2 GB, суммарный объем файлов аккаунта —
   20 GB; превышение отклоняет весь batch с `RESOURCE_EXHAUSTED`, а не частично;
5. вычисляет hold каждого item по обычному Gemini rate card, тому же provider multiplier и одному
   priced timestamp;
6. запрещает silent output cap: при недостатке денег отклоняется весь batch с 402;
7. блокирует account, затем key в существующем money lock order;
8. проверяет aggregate hold против account floor и key spend limit;
9. одним движением уменьшает balance, увеличивает account/key reserved totals;
10. вставляет job, items и encrypted payloads; для файлового ввода потоково читает JSONL,
    валидируя каждую строку (`key` + `request`) и не материализуя весь файл в памяти;
11. commit предшествует успешному operation response.

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
- terminalize item/result и outbox в той же transaction;
- при file-вводе после terminalization всех items job той же authority транзакцией собирает
  output JSONL (`key` + `response`/`error` по строкам), шифрует его как `batch_output` файл и
  проставляет `output_file_id` в job row до того, как GET увидит operation terminal.

Нельзя копировать settlement SQL во второй, постепенно расходящийся алгоритм. Common helper остается
в `registry` и принимает типизированный source (`interactive reservation` или `Gemini batch item`).
**Масштаб hold-агрегата при Google-лимитах.** Aggregate hold сотен тысяч items может превышать
любой реальный баланс — и это корректно: create честно отклоняется 402, сервер не подменяет
клиентский request и не принимает частично. Формула hold не меняется (native output ceiling при
отсутствии `maxOutputTokens`), поэтому крупный file-based batch требует от клиента либо
достаточного баланса, либо явного `maxOutputTokens` на item. Это следствие решения «весь batch
финансово допускается атомарно» и осознанно отличается от Google (у Google batch предоплаты нет);
в клиентской документации оно называется прямо.

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
каждого blob. AAD включает contract version, account id, job id, item index и kind (request, result,
file), поэтому ciphertext нельзя переставить между tenants/items/request/result/file. Logs, metrics
и registry Debug не содержат plaintext, metadata, ciphertext, nonce, customer/job/profile IDs или
encryption key id.

Retention policy:

- nonterminal jobs/items и pending/failed settlement intents не удаляются;
- input payload хранится до terminal state плюс короткий recovery grace period;
- загруженные файлы живут 48 часов от `createTime` (как у Google) или до истечения ссылок на них,
  смотря что позже; expired файл удаляется pruner'ом вместе с blob;
- terminal inline results и output-файлы file-based jobs доступны 42 дня, как документированные
  шесть недель Google; expiration output-файла не короче result expiry ссылающегося job, после
  expiry файл удаляется pruner'ом;
- после result expiry GET возвращает явный expired operation без payload;
- idempotency tombstone и canonical digest хранятся минимум 60 дней;
- ledger, usage, tariff/settlement evidence следуют существующей financial retention;
- DELETE удаляет customer payload/result и скрывает resource, но оставляет минимальный tombstone и
  financial evidence;
- pruning bounded, сначала blobs, затем terminal item/job metadata; active lease/outbox не pruned.

### 4.6 Scheduler, распределение по подпискам и batch 5h headroom policy

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

**Batch 5h headroom policy (ключевая бизнес-политика).** Клиенту разрешается высаживать пул
Gemini-подписок, пока на 5-часовых окнах (`gemini-5h` bucket `retrieveUserQuotaSummary`) остается
минимум 15% лимита; batch-потолок использования 5h-окна — 85%:

- Для кандидатного profile scheduler читает последнюю per-profile `gemini-5h` quota summary
  (`remaining_fraction_units`, `resets_at`, `observed_at`). Dispatch разрешен только если
  `remaining_fraction_units > 15%` (строгое неравенство: ровно 15% — уже стоп) и snapshot свежий
  (в пределах существующего `quota_stale_secs` окна, вычисляемого из `health_probe_interval_secs`).
- При отсутствии свежего 5h-snapshot (quota summary еще не приходила или протухла) dispatch этого
  profile запрещен: fail-closed, а не fail-open. Batch — deferrable workload, поэтому ошибка в
  сторону осторожности корректна; interactive-path правила (stale = fail-open) на batch не
  переносятся.
- Недельное окно (`gemini-weekly`) в hard gate не участвует: его исчерпание уже отражено
  provider-side per-model cooling (`fetchAvailableModels` catalogue, explicit zero →
  `quota_blocked_until`), которое batch наследует через обычный профильный eligibility. Отдельный
  weekly headroom gate не добавляется: 5h-окно — самое короткое и самое быстро сгорающее
  ограничение, и бизнес-решение ограничено именно им. Если позже потребуется weekly-потолок, он
  добавляется отдельным config-тунингом без смены schema.
- Per-model catalogue cooling продолжает действовать независимо: batch item на профиле с
  охлажденной моделью не dispatch'ится, даже если 5h headroom есть. Batch gate — дополнительный
  предикат поверх существующих hard правил, а не их замена.
- Решение принимается на claim-границе одним целым числом из authority: профиль либо имеет свежий
  snapshot с `remaining_fraction_units > 15%`, либо нет. Никакой плавающей арифметики на hot path:
  `remaining_fraction_units` хранится в fixed-point `10^-8` единицах, порог выражен в тех же
  единицах (`15_000_000` при `FRACTION_SCALE = 100_000_000`).
- Операционная метка причины недоступности — `batch_5h_headroom_stop`: попадает в bounded metrics
  (counter по причине) и в admin-статус профиля (additive поле, не раскрывающее customer data),
  чтобы оператор видел «флот остановлен для batch по 5h-политике», а не искал фантомную деградацию.

Soft reserve vs batch hard stop. Интерактивный `select_routed_inner` использует мягкий per-profile
`quota_reserve_fraction` (+jitter), который удерживает профили от синхронного выгорания, но fail-open:
если все ниже резерва, последний рабочий профиль продолжает обслуживать до явного provider zero.
Batch-политика — принципиально другой механизм: **hard floor**, ниже которого batch останавливается
безусловно, даже если профиль еще обслуживает интерактивные запросы. Batch не может сжечь последние
15% 5h-окна — это пространство резервируется для interactive-трафика и для живости пула между
reset'ами. Механизм намеренно не переиспользует `quota_reserve_fraction`: soft reserve написан для
ранжирования с fail-open деградацией, а нам нужен gate с fail-closed деградацией на отдельном
окне (`gemini-5h`, а не per-model catalogue). Порог 15% — config-driven (`GeminiConfig` поле,
env в `crates/server/src/config.rs`, по умолчанию 15), а не константа в коде: бизнес-решение должно
быть операционно регулируемым без redeploy-freeze.

Реализация: в `GeminiGateway` появляется batch-специализированный selection path — не новый
вызывающий `select_routed`, а обертка, которая после обычного eligibility (hard cooling,
authenticated, freshness) дополнительно фильтрует кандидатов предикатом `batch_5h_headroom_ok`.
Interactive `select_routed` остается байт-в-байт прежним: batch policy не течет в общий path.
`GeminiGateway::select_routed` без affinity hints переиспользуется как база: batch items
независимы, а цель режима — распределение. Hard quota/model cooling, authenticated status, fresh
quota preference, inflight rank и round-robin сохраняются. Batch-specific per-profile active fence
заставляет burst покрывать разные подписки; обычные interactive requests по-прежнему могут идти на
эти профили и не ждут batch semaphore. Когда все кандидаты отсеяны 5h-gate, scheduler не возвращает
клиентскую ошибку: items остаются `queued`, а `next_attempt_at` выставляется по минимальному
`resets_at` среди свежих 5h-snapshot'ов (или по bounded backoff, если ни один snapshot не свеж),
плюс jitter, чтобы флот не просыпался синхронно.

Batch — lower-priority workload:

- interactive path не проверяет queue depth и не знает о 85%-потолке;
- scheduler предпочитает профили с минимальным текущим inflight;
- при hard cooling/нулевой quota item остается queued с provider-derived retry time;
- при 5h headroom stop item остается queued до reset/headroom recovery;
- queue deadline (48 часов) ограничивает бесконечное ожидание: job, который не успел выполниться из-за
  повторяющихся 5h-stop'ов, честно умирает `EXPIRED`, а не висит вечно;
- никакой номинальный subscription RPM/RPD не придумывается локально.

Лимиты целимся в Google-масштаб, а не в локальный MVP-envelope: inline create body ограничен
общим 20 MiB лимитом тела Gemini plane (у Google — 20 MB, §2.5); файловый ввод — до 2 GB на файл
и 20 GB суммарно на аккаунт (как у Google Files API); публично документированного максимума числа
items на batch у Google нет **[не подтверждено]**. Этап 5 зафиксировал безопасный предел
**100 000 items на batch**: generated 100k JSONL проходит bounded streaming/memory gate; 250k/500k
остаются вне первой версии до отдельного PostgreSQL/WAL capacity expansion. Предел config-driven,
default 100k, превышение атомарно возвращает `RESOURCE_EXHAUSTED`. File-based
result set не ограничен 64 MiB, потому что результат отдается файлом, а не одним JSON;
ограничение 64 MiB остается только для inline-формы. Per-account ограничения: до 100 nonterminal
jobs и 20 GB файлов — единственные флот-защитные лимиты сверх Google. Все bounds — config-driven,
возвращают `RESOURCE_EXHAUSTED`, а не частично принимают batch. Реальная пропускная способность
(PostgreSQL locks, outbox throughput, потоковая сборка output-файла) доказывается load test в
Этапе 5 на Google-масштабных объемах; если измеренная граница ниже целевого лимита, план и
клиентская документация обновляются на измеренное значение в том же commit, что и результаты
измерения (§12), а не остаются на желаемом.

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
| Batch 5h headroom stop (§4.6) | item остается queued до reset/recovery | hold остается |
| 401/token до actual send | выбрать другой профиль в bounded attempt | hold остается |
| 403 request rejection после bounded fleet classification | terminal item error | hold released, charge 0 |
| 429 с retry/reset | вернуть item в retry scheduling, пока live owner может безопасно доказать no output | hold остается |
| 5xx/transport до actual send | bounded rotate/retry | hold остается |
| Ошибка после actual send без terminal usage | `indeterminate`, без replay | общая unknown-usage policy, explicit pool loss if zero |
| 2xx без требуемого usage | `indeterminate` protocol error, без replay | unknown-usage policy; usage/charge не выдумывать |
| 2xx с usage | result + settlement outbox | exact per-item charge |
| Settlement DB transient error | result не публикуется terminal до outbox apply | outbox retries |
| Permanent settlement conflict | item/outbox quarantined, alert | hold не silently released |

Политика unknown usage (сверена с кодом 2026-08-21, `crates/forward/src/settlement_policy.rs` +
`crates/registry/src/pg.rs::reconcile_expired`): флот default — **unmeasured turn стоит клиенту
0**; если owner успел записать measured checkpoint, reconcile списывает ровно его (clamp до
hold). Полный hold списывается только при включенном operator switch
`CLAUDE_API_CHARGE_HOLD_ON_UNKNOWN_USAGE` (default off). Batch-строки выше ссылаются ровно на эту
политику; `GEMINI_PROVIDER.md` описывает ее в тех же терминах.

## 5. Наблюдаемость и операции

На `/metrics` нужны только fixed-cardinality series:

- queue jobs/items по closed state;
- oldest queued age;
- claimed/dispatching workers;
- completion/error/cancel/expired totals по bounded reason class;
- indeterminate-after-send total;
- batch 5h headroom stop counter и fleet-level gauge «профилей, доступных batch сейчас»;
- batch settlement outbox pending/failed и oldest age;
- reserved batch nanoUSD aggregate как decimal-safe internal gauge or bounded integer series;
- encrypted payload/result/file bytes and prune failures;
- scheduler leader/fencing failures;
- per-profile batch activity только aggregate histogram/gauge без profile label.

`/gemini-subs` можно additive расширить fleet-level batch summary, но нельзя отдавать account, job,
item, prompt или result. Новые alerts обязательны для stale queue, failed settlement outbox,
indeterminate spike, missing scheduler leader при backlog, retention failure и sustained
5h-headroom stop (флот не может обслуживать batch дольше N часов подряд — сигнал либо о нехватке
подписок, либо о слишком агрессивном batch-трафике); каждому alert нужен runbook section в
`docs/ops/MONITORING.md` в том же commit.

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

### Этап 0. Контракт и решения — ВЫПОЛНЕН (2026-08-21)

Контракт зафиксирован в §2 из публичных источников (Discovery document v1beta snapshot
2026-08-21, официальная документация batch-api/files, исходники Python SDK); живые wire captures
официального API осознанно не выполняются — нет доступа, а места без публичных данных помечены
**[не подтверждено]** и реализуются как наш дизайн. Решения владельца продукта:

- модели: без allowlist — все опубликованные текстовые Gemini-модели; image-output вне MVP (§1);
- лимиты: Google-масштаб (2 GB файл, 20 GB на аккаунт, 20 MB inline body, 48h TTL upload-файлов,
  42-day result retention включая output-файлы, 60-day tombstone); максимум items на batch —
  config-driven, измеряется в Этапе 5 (§4.6);
- форма результата: inline → `inlinedResponses`, file → `responsesFile` JSONL с passthrough `key`;
  сериализация `batchStats` и echo-семантика `priority` — по §2.2;
- batch 5h headroom policy ПОДТВЕРЖДЕНА: порог 15% (config, default 15), источник — `gemini-5h`
  bucket quota summary, fail-closed при отсутствии свежего snapshot, weekly-окно вне gate;
- удаление файла при живых batch-ссылках ПОДТВЕРЖДЕНО как запрет до terminal/expiry ссылающихся
  jobs (`FAILED_PRECONDITION`, §4.1);
- unknown-usage политика сверена с кодом и описана в `GEMINI_PROVIDER.md` (fleet default:
  unmeasured turn стоит 0; measured checkpoint clamp до hold; full hold только при operator
  switch `CLAUDE_API_CHARGE_HOLD_ON_UNKNOWN_USAGE=on`).

Exit gate: contract tests могут быть написаны без догадок — выполнен; все пункты раздела 8 имеют
подтвержденные ответы.

### Этап 1. Migration-only expansion

- Добавить пустые batch job/item/blob/file/outbox tables, constraints и indexes.
- Зарегистрировать migration/current schema version и schema tests.
- Migration не создает jobs, не меняет balances и не имеет runtime reader/writer.
- Отдельно merge/deploy migration и дождаться GREEN `deploy/migration` + `deploy/watchdog`.

Exit gate: old runtime работает с расширенной schema; rollback binary не затронут.

### Этап 2. Registry authority, без публичных routes

- **Migration correction перед runtime (добавлено после schema review 2026-08-20):** отдельной
  expand-only migration добавить nullable `ledger.key_id`/`usage_events.key_id`, item-level
  creator `key_id`, nullable result expiry, chunked file storage и полный immutable calibration
  payload для batch outbox. После 0056 отдельная 0057 заменяет только legacy anonymous file-shape
  CHECK: active chunked file не обязан хранить фиктивный inline blob, а старый inline shape остается
  валиден. Обе corrections доставить и дождаться GREEN `deploy/engine` + `deploy/watchdog` до любого
  reader/writer Stage 2; migrations 0055/0056 не переписываются.
- Реализовать atomic create + all-item holds + idempotency.
- Реализовать scoped list/get/cancel/delete/prune для jobs и files.
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
- Реализовать batch 5h headroom gate (§4.6) как отдельный, unit-тестируемый предикат поверх
  profile eligibility: fixed-point порог, fail-closed stale/missing snapshot, bounded backoff по
  `resets_at`.
- Подключить scheduler leader, fair claims, one-batch-item-per-profile и shutdown drain.
- Реализовать Files API upload/get/list/delete/download с зашифрованным blob-хранилищем.
- Оставить feature default-off; public parser продолжает возвращать 404.

Exit gate: mock upstream выполняет jobs через несколько profiles, 5h-gate останавливает dispatch при
15%-остатке и возобновляет после reset; restart продолжает queue, plaintext не появляется в DB/logs.

### Этап 4. Producer-first public API, default-off

- Добавить exact Google-shaped inline routes в Gemini plane, включая Files API subset и
  `inputConfig.fileName`.
- Добавить `/upload/v1beta/*` в perimeter (Caddy + router passthrough) одним commit'ом.
- Добавить auth-before-body, account isolation, CORS DELETE и explicit unsupported fields.
- Добавить optional idempotency extension.
- Обновить `GEMINI_PROVIDER.md`, `UNIFIED_ROUTER.md`, crate contracts и public examples в том же
  producer commit.
- Router получает только passthrough/identity regression tests, не job code.
- Model discovery добавляет `batchGenerateContent` только когда endpoint включен и проверен.

Exit gate: official SDK compatibility tests проходят на direct Gemini host и unified router.

### Этап 5. Resilience, observability и controlled canary

- Добавить metrics, alerts, Grafana/admin fleet summary и runbooks.
- Прогнать multi-owner fault injection, kill-at-every-boundary matrix и load/fairness tests на
  Google-масштабных объемах (файл ввода ближе к 2 GB, сотни тысяч items), чтобы доказать потоковую
  валидацию JSONL, сборку output-файла и outbox throughput без giant in-memory copy.
- Выполнить controlled internal batch с подтвержденным владельцем aggregate budget **$10** на весь
  Этап 5 (live-smoke + canary вместе): несколько items, несколько профилей, partial error, cancel,
  restart, 5h-headroom stop и recovery, exact settlement. Бюджет покрывает customer-деньги нашего
  собственного тестового аккаунта; перед каждым paid run агент считает expected worst-case spend
  (сумма per-item holds) и не стартует run, если остаток бюджета его не покрывает. Потраченное
  фиксируется в журнале (§12) после каждого run — с точной суммой settlement и остатком.
- Проверить no-discount charge parity с теми же requests через ordinary generateContent (считается
  в тот же $10 бюджет).
- Сохранить sanitized exact-SHA evidence; failed paid run не replay — сначала root cause, затем
  новый run в пределах остатка бюджета. Канонический credential-safe operator runner и процедура —
  `tools/gemini_batch/run_live.py` и `docs/ops/GEMINI_BATCH_STAGE5_CANARY.md`: dry-run default,
  production SSH remote-only key loading, exact GREEN implementation SHA + original/previous spend
  checkpoint, одна paid-create попытка и fail-closed secret-free projection holds сразу после create.

Exit gate: provider output/usage, profile distribution, 5h headroom gate behavior, customer ledger,
quota calibration и restart recovery сходятся на production-GREEN implementation SHA.

### Этап 6. Публикация

- Отдельным commit включить reviewed systemd flag и discovery method.
- Обновить customer docs без заявления Google discount/SLA; честно описать собственный Files API и
  его TTL, расширение `fileData` внутри batch и 85%-потолок 5h-окна как операционную характеристику
  (не как клиентский лимит).
- После deploy выполнить public create -> poll -> terminal result smoke через оба hostname
  (минимальный paid request; считается в тот же подтвержденный $10 aggregate budget Этапа 5, если
  его остаток покрывает worst-case hold, иначе — отдельное подтверждение владельца).
- Наблюдать queue age, settlements, indeterminate, 5h headroom stops и interactive latency в soak
  window.
- Rollback выключает admission новых jobs, но совместимый runtime обязан дочитать уже принятые jobs;
  нельзя откатываться на binary, который не понимает их holds/outbox.

Exit gate: точный published SHA GREEN, queue drains, нет balance divergence и stale holds.

### Этап 7. Отдельные будущие расширения

- signed webhooks с SSRF defense, retry budget и secret rotation;
- image-output batch с отдельными storage/egress limits;
- universal provider-neutral batch contract, только если появится второй реальный provider;
- direct Google Batch adapter, только если появится отдельная metered credential/tariff authority;
- расширение Files API на синхронный `generateContent` (резолвинг `fileData` в inlineData на лету),
  только после доказанной стабильности batch file pipeline;
- configurable weekly headroom gate, если бизнес решит резервировать и недельное окно.

Каждое расширение меняет threat model и не должно попадать в MVP «заодно».

## 7. Обязательная тестовая матрица

### Contract и auth

- Python/JavaScript SDK create/list/get/cancel/delete direct + router, включая пустые
  cancel/delete responses (ожидания — из зафиксированного контракта §2, не из live captures);
- Files API upload/get/list/delete/download fixtures, state machine `PROCESSING → ACTIVE`;
- batch `inputConfig.fileName` с JSONL-вводом (`key` + `request`), `fileData` ссылками на
  собственные файлы и симметричным `responsesFile` output с passthrough `key`;
- `priority` принимается, валидируется по типу и echo'ится в operation metadata без влияния на
  порядок выполнения;
- `batchStats` на каждый GET совпадает с фактическими item states (derived-on-read), включая
  гонки с cancel/expiry;
- wire-сериализация совпадает с зафиксированным контрактом §2: string-int64 счетчики,
  google-datetime timestamps, `BATCH_STATE_*` в `metadata`, `output` внутри `metadata`;
- exact golden JSON и unknown-field behavior;
- auth до buffering oversized/chunked body;
- query credential rejected;
- same-account/different-key access;
- foreign/unknown/deleted IDs имеют одинаковый 404;
- stable cursor pagination без cross-account rows;
- explicit rejection webhook/update/embedding/cross-model/image-output/external Google
  `files/…` reference.

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

### Batch 5h headroom policy

- профиль с `remaining_fraction_units > 15%` доступен batch, ровно 15% и ниже — нет;
- missing/stale 5h snapshot — dispatch запрещен (fail-closed);
- все профили под gate → items остаются queued, `next_attempt_at` по минимальному `resets_at`;
- восстановление headroom после reset возобновляет dispatch без ручного вмешательства;
- weekly bucket не влияет на gate (ни наличие, ни отсутствие);
- per-model catalogue cooling действует независимо: охлажденная модель не dispatch'ится даже при
  достаточном 5h headroom;
- interactive `select_routed` не меняет поведения ни при какой комбинации 5h-остатков;
- config-порог меняется без смены schema; граничные значения 0% и 100% валидируются;
- jitter в `next_attempt_at` не допускает синхронного fleet wake после общего reset.

### Privacy, limits и performance

- DB/log/metrics snapshots не содержат prompt, result, file bytes, API key, metadata или profile id;
- AEAD swap между account/job/item/kind/file не decrypt;
- active/old key rotation и missing key fail closed;
- body/item/output/result/file/job/account limits;
- file upload streaming без одного giant in-memory copy; потоковая валидация входного JSONL и
  потоковая сборка output-файла при Google-масштабном числе items;
- fair progress двух accounts при одном крупном batch;
- batch workload не добавляет wait/reject на interactive path;
- load test PostgreSQL locks, outbox throughput, scheduler wakeup и result reads.

Существующие Gemini API/stream/disconnect/missing-usage tests, registry Stage 2 fault matrix,
router native passthrough tests, `cargo test --locked --workspace`, rotation and universal chat smoke
остаются green.

## 8. Решения, подтвержденные до кода

Все ответы ниже подтверждены владельцем продукта 2026-08-21 (Этап 0 выполнен, §6). Изменение
любого ответа существенно меняет schema или public contract и требует нового review.

| Вопрос | Решение |
|---|---|
| API shape | Gemini Developer API inline-compatible subset + собственный Files API subset |
| Где доступен | direct Gemini host и native `/v1beta/*` (+`/upload/v1beta/*`) unified-router passthrough |
| Execution | локальная очередь обычных non-stream Code Assist turns |
| Pricing | тот же normal tariff + account Google multiplier, без batch discount |
| Admission | весь batch атомарно, per-item holds |
| Affinity | выключена для независимых items; quota/inflight/cursor selection остается |
| Concurrency | bounded global, один batch item на profile, interactive не блокируется |
| Batch 5h headroom | ПОДТВЕРЖДЕНО: batch dispatch только при `gemini-5h` remaining > 15% (config, default 15); fail-closed при stale/missing snapshot; weekly-окно вне gate |
| Модели | без allowlist: все опубликованные текстовые Gemini-модели; image-output model вне MVP |
| Контрактные фактуры | без live captures: публичные источники (§2); места без данных помечены **[не подтверждено]** и реализуются как наш дизайн |
| File input | собственный encrypted Files API (TTL 48ч), `inputConfig.fileName` JSONL + `fileData` ссылки внутри batch |
| File output | как у Google: file-ввод → `metadata.output.responsesFile` JSONL в нашем Files API, passthrough клиентского `key`; inline-ввод → `inlinedResponses` |
| `priority` | принимаем и echo'им как у Google, функционально игнорируем (scheduler имеет собственный детерминизм) |
| Лимиты | Google-масштаб: 2 GB файл, 20 GB на аккаунт, 20 MB inline body; максимум items — config-driven, измеряется в Этапе 5 и обновляется здесь же (§12); флот-защита — только nonterminal jobs на аккаунт |
| `batchStats` | derived-on-read из item rows на каждый GET; mutable counters в job row запрещены (§4.2) |
| File delete с живыми ссылками | ПОДТВЕРЖДЕНО: запрещено до terminal/expiry ссылающихся jobs (`FAILED_PRECONDITION`) |
| Create idempotency | Google-compatible non-idempotent default + optional `Idempotency-Key` |
| Ownership | account-scoped, creator key only attribution/revocation policy |
| Cancel | queued stops immediately; dispatching drains best effort |
| Crash after send | indeterminate, no automatic replay; unknown-usage policy — fleet default «unmeasured = 0», measured checkpoint clamp до hold, full hold только при operator switch (§4.9) |
| Queue deadline | 48 часов |
| Live-smoke budget | ПОДТВЕРЖДЕНО: $10 aggregate на Этап 5 (canary + parity + public smoke Этапа 6); каждый paid run сначала считает worst-case по holds, потраченное идет в журнал (§12) |
| Result retention | 42 дня (включая output-файлы); financial evidence отдельно |
| MVP media | existing bounded inline inputs + собственные файлы внутри batch; image-output model excluded |
| Large input | собственный Files API (в MVP) |

## 9. Definition of Done

Batch mode считается готовым, только когда одновременно доказано:

- official SDK create/poll/result/cancel/delete compatibility на опубликованном contract subset;
- собственный Files API принимает upload, отдает metadata/download, соблюдает TTL и account isolation;
- batch принимает JSONL-файл и `fileData` ссылки на собственные файлы и резолвит их до dispatch;
- file-based batch возвращает результат output-файлом (`responsesFile`) с passthrough клиентских
  `key`, inline batch — `inlinedResponses`; обе формы совпадают с контрактом §2;
- `batchStats` на GET всегда согласован с фактически видимыми результатами;
- batch dispatch останавливается при 15%-остатке `gemini-5h` и возобновляется после reset без
  ручного вмешательства; interactive-трафик при этом не меняет поведения;
- один accepted batch распределяет items минимум по двум доступным subscriptions без client hints;
- batch переживает Gemini blue-green replacement и продолжает progress;
- каждый successful item имеет terminal native response, authoritative usage, unique usage/ledger
  identity и exact tariff pin;
- errors/cancel/expiry освобождают holds ровно один раз;
- одинаковый model/usage/tariff/multiplier vector в ordinary и batch settlement дает один pricing
  basis, без дополнительной скидки;
- post-send crash не приводит к автоматическому duplicate generation;
- plaintext customer data отсутствует в DB/logs/metrics;
- queue/outbox/retention/5h-headroom alerts и runbooks установлены;
- interactive Gemini latency/admission semantics не изменены;
- exact implementation SHA прошел repository gate, controlled live batch и public smoke;
- документация честно называет local execution ограничения, собственный Files API и 85%-потолок
  5h-окна, и не обещает Google Batch tariff или SLA.

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

**Переиспользование soft `quota_reserve_fraction` для batch-политики.** Soft reserve — механизм
ранжирования с fail-open деградацией (последний профиль продолжает обслуживать до provider zero).
Batch-политика — hard floor с fail-closed деградацией на отдельном 5h-окне: смешивание двух
семантик в одном config-поле сделало бы обе непредсказуемыми.

**Passthrough клиентских Google `files/…` ссылок в batch.** Pooled identity не видит чужой проект;
каждый профиль отвечал бы `PERMISSION_DENIED`, что неотличимо от флот-аута. Собственный Files API
снимает ограничение без ложных provider-ошибок.

## 11. Замечания и предложения по предыдущей редакции плана

Этот раздел фиксирует ревью документа версии от 2026-08-19 (коммиты `f76404a7`, `0da98133`) и
объясняет, что и почему изменено в текущей редакции.

1. **Отсутствовала политика потребления квоты пула.** Предыдущая редакция описывала batch как
   lower-priority workload, но не отвечала на главный бизнес-вопрос: насколько глубоко batch может
   высаживать подписки. Без явного ответа либо batch уперся бы в soft reserve (и был бы
   недогруженным), либо дожимал бы пул до provider zero вместе с interactive-трафиком (и ставил бы
   под угрозу живость пула между 5h-reset'ами). Введен отдельный hard gate: batch останавливается
   при 15%-остатке `gemini-5h` (потолок 85%), interactive — без изменений. Порог config-driven.
2. **Files API был полностью исключен из MVP**, хотя собственное зашифрованное blob-хранилище для
   batch payloads все равно вводится. Добавить Google-shaped Files API subset на том же хранилище
   — умеренный дельта-объем при значительной продуктовой ценности: крупные медиа-вводы не
   приходится base64-инлайнить в каждый item, а `inputConfig.fileName` становится реальным.
   Одновременно появляется возможность, которой нет у Google Batch: `fileData`-ссылки внутри
   batch-items. Это честно объявленное расширение, а не маскировка.
3. **Soft reserve нельзя было переиспользовать как batch-потолок.** `quota_reserve_fraction` —
   fail-open механизм ранжирования на per-model catalogue; бизнес-политика требует fail-closed
   gate на 5h-summary-окне. Это разные источники данных (per-model catalogue vs `gemini-5h`
   bucket) и разная семантика деградации. В план добавлено явное разъяснение, почему механизмы
   не объединяются.
4. **Не был описан источник данных для квота-решений.** В коде есть два независимых провайдерских
   сигнала (per-model catalogue и 5h/weekly summary); план теперь явно называет, какой из них —
   authority для batch gate, и фиксирует поведение при stale/missing данных (fail-closed, т.к.
   batch — deferrable).
5. **Weekly-окно не было упомянуто.** План теперь явно отвечает: отдельного weekly gate нет —
   его исчерпание уже отражается в provider-side per-model cooling, которое batch наследует.
   Расширение оставлено как config-тунинг без смены schema.
6. **`/upload/v1beta/*` не был учтен в периметре.** Существующий perimeter покрывает только
   `/v1beta/*`; multipart upload требует отдельного маршрута и отдельного body-limit в Caddy и
   router passthrough — добавлено в §3.5 и Этап 4.
7. **Файловый lifecycle требовал решений, которых не было:** TTL (48 часов, как у Google),
   семантика удаления при живых batch-ссылках (рекомендация — запрет), state machine файла,
   account-scope и поведение при expiry. Все вопросы вынесены в §4.1, §4.5 и §8.
8. **Наблюдаемость квота-политики отсутствовала.** Без метрики `batch_5h_headroom_stop` и alert'а
   на sustained stop оператор не отличил бы «batch стоит по политике» от «batch сломан».
   Добавлено в §5 и тестовую матрицу.
9. **Смешанные per-item ошибки в JSONL-файле** (битая строка, несовпадающий model в строке)
   требуют того же all-or-nothing admission, что и inline: весь файл валидируется до money
   mutation, битая строка отклоняет весь create с указанием номера строки — добавлено в §4.4.
10. **Рекомендация держать batch-порог в config, а не в константе.** 15% — бизнес-решение, которое
    может меняться по мере наблюдения за живостью пула; env-driven значение с валидацией диапазона
    в `crates/server/src/config.rs` не требует redeploy-freeze для операционной подстройки.

### 11.1 Вторая итерация ревью — закрытие расхождений с Google-контрактом

Сравнение предыдущей редакции с официальным Gemini Batch API выявило пять пробелов; все закрыты
решением «как у Google»:

11. **File output отсутствовал.** У Google файловый ввод порождает файловый результат:
    `metadata.output.responsesFile` — JSONL со строками `{"key": ..., "response"|"error": ...}` в
    том же Files API, а `inlinedResponses` отсутствует. План всегда отдавал inline output, что
    ломало SDK-workflow клиентов Google и противоречило нашему же лимиту на result set. Теперь
    форма результата симметрична вводу (§4.1), output-файл создается atomically при
    terminalization job, его TTL ограничен 42-дневным result retention (§4.3, §4.5), а в схеме
    появился `source_kind=client_upload|batch_output`.
12. **Passthrough `key` из JSONL-строк не был описан.** Официальный файловый формат —
    `{"key": ..., "request": ...}`, и `key` возвращается в результатах для корреляции. Без него
    миграция клиента с Google ломалась бы. В `gemini_batch_items` добавлен nullable bounded client
    `key` (§4.3), echo-правила — в §4.1 и тестовую матрицу.
13. **`priority` отклонялся.** Google принимает это опциональное поле. Решение: принимать и
    echo'ить как у Google, но функционально игнорировать — наш scheduler имеет собственный
    детерминизм (fair order, 5h gate), и давать клиенту ручку обгона других нельзя. Это
    осознанный wire-compatible no-op, документированный клиенту, а не silently dropped field
    (§4.1, §8).
14. **Лимиты были локальным MVP-envelope (1,000 items, 64 MiB result set) без траектории к
    Google.** Решение: целимся в Google-масштаб — 2 GB на файл, 20 GB на аккаунт, Google-максимум
    items, file-based result set не ограничен 64 MiB (результат отдается файлом); из флот-защитных
    лимитов сверх Google остается только 100 nonterminal jobs на аккаунт. Потоковая валидация
    JSONL и потоковая сборка output-файла обязательны; реальная пропускная способность доказывается
    load test в Этапе 5, и если измеренная граница ниже Google-лимита — документируется измеренное
    значение, а не желаемое (§4.4, §4.6, Этап 0/5). Следствие, которое надо честно назвать
    клиенту: aggregate hold Google-масштабного batch может превышать реальный баланс, и create
    тогда отклоняется 402 целиком — это цена атомарного admission (§4.4).
15. **Семантика `batchStats` была отложена на fixtures без инварианта реализации.** Теперь
    зафиксировано (§4.2): все четыре счетчика (`requestCount`, `successfulRequestCount`,
    `failedRequestCount`, `pendingRequestCount`) считаются SQL-агрегатом по item rows на каждый
    GET, а mutable counters в job row запрещены инвариантом схемы — иначе второй источник истины
    расходится с фактом при crash между обновлением item и счетчика, теряет приращения под
    конкуренцией settlement/cancel и требует второй locking-матрицы без новой информации.
    Derived-on-read также бесплатно гарантирует, что stats никогда не опережают реально видимые
    клиенту результаты (GET не считает item terminal до outbox APPLY).

## 12. Журнал исполнения

Агент, работающий над исполнением этого плана (Этапы 1–6), обязан вести журнал результатов в
`docs/engine/GEMINI_BATCH_MODE_JOURNAL.md` (создается первой записью; файл — часть того же
commit, что и соответствующая работа). Этот файл — не инструкция, а append-only протокол
исполнения: новые записи добавляются в конец, существующие не переписываются.

Формат записи:

```text
## YYYY-MM-DD — <этап и краткое название шага>
SHA: <точный commit SHA(ы)>
Результат: <что сделано, чем доказано: команды проверки и их исход>
Отступления от плана: <нет | что и почему; любое отклонение сначала отражается
правкой соответствующего раздела этого плана в том же commit>
Измерения: <load/limits/latency цифры, если шаг их производил>
Следующий шаг: <что блокирует / что дальше>
```

Обязательные правила:

- каждый завершенный шаг этапа (не только этап целиком) — отдельная запись;
- измеренные лимиты Этапа 5 сначала обновляют §4.6/§8 этого плана, затем попадают в запись;
- проваленный шаг фиксируется так же, как успешный: с фактическим выводом проверок и причиной;
- номера этапов и ссылки на разделы этого плана обязательны, чтобы журнал читался как
  машинно-проверяемая трассировка плана;
- `docs/README.md` получает строку на журнал в commit первой записи.
