# Контракт мультипровайдерных скидок и online-cutover

Статус документа: утверждённый целевой контракт от 2026-08-02. Код и production-данные считаются
переведёнными на него только после прохождения Definition of Done ниже. Этот документ заменяет
предыдущий дизайн прогрессивных тарифов: активного режима `track`, tier ladder и 30-дневного
retention в целевой системе нет.

## 1. Принятые продуктовые решения

1. Gemini — полноценный продуктовый провайдер вместе с Anthropic и OpenAI.
2. Обычный B2C-клиент получает глобальную скидку `50%` на каждую модель, включённую в основной
   продуктовый каталог.
3. Для B2C можно задать отдельную процентную скидку на провайдера и более точную скидку на модель.
4. B2B не наследует глобальную B2C-скидку. У каждого B2B-клиента собственная policy. Текущий
   скалярный процент при миграции становится provider-rule для внутреннего provider ID
   `anthropic`; OpenAI/Gemini не добавляются существующему B2B автоматически.
5. Все существующие и новые OpenKeys списываются `1:1` по официальной цене модели
   (`discount_bps=0`, `payable_multiplier_bp=10000`).
6. Service-аккаунты имеют доступ ко всем моделям, поддержанным runtime, независимо от продуктовых
   B2C/B2B/OpenKeys-каталогов. Код домена решает, какие модели фактически использовать. Аварийный
   master-switch и техническая недоступность провайдера по-прежнему действуют.
7. Потребление service-аккаунтов полностью измеряется и сохраняется, но запросы не резервируют и
   не списывают клиентский баланс. Для этого используется отдельный billing mode `meter_only`, а
   не нулевой multiplier и не бесконечный искусственный баланс.
8. Welcome bonus сохраняется. Новая выдача — ровно `$5.000000000`; ранее выданные `$4` не
   увеличиваются задним числом. Бонус доступен для любой разрешённой B2C-модели и провайдера.
9. Реферальные комиссии сохраняются. Их eligibility определяется B2C-атрибуцией и реферальной
   связью, а не режимом цены. Комиссия начисляется только на фактически списанную paid-funded
   часть; расход welcome bonus не комиссионируется.
10. Перевод production выполняется для всего инвентаря одним глобальным переключением. Canary и
    поаккаунтное включение клиентов запрещены.
11. Production нельзя останавливать ради pricing/funding cutover. Не допускаются глобальный drain,
    maintenance mode, остановка money writers или ожидание нуля всех активных reservations.
12. Ручная финансовая классификация Stage 6 не требуется. Известный неиспользованный welcome
    credit сохраняется как bonus; весь остальной существующий остаток по явному решению владельца
    считается paid. Суммы всё равно проходят автоматические структурные инварианты.

`fixed discount` в этом контракте означает статический процент в basis points. Literal fixed
tariff вроде «эта модель всегда стоит $0.01» не входит в контракт.

## 2. Экономика по классам аккаунтов

| Класс | Цена | Доступ | Зависимость от баланса | Referral |
|---|---|---|---|---|
| B2C | global `5000 bps`, затем provider/model override | основной product catalog | да | paid-funded usage |
| B2B | индивидуальные provider/model rules | только явно разрешённые policy модели | да | нет, пока отдельный B2B-контракт не утверждён |
| OpenKeys | `0 bps` скидки, строго 1:1 | OpenKeys product catalog | да | нет |
| Service | customer charge не вычисляется; official cost сохраняется | все runtime-capable модели | нет (`meter_only`) | нет |

Pricing и admission — разные решения. Скидка не включает модель, а наличие модели в каталоге не
задаёт её цену. Исключение service относится только к продуктовым gates; capability manifest,
безопасность транспорта и master-switch продолжают закрывать реально недоступный provider.

## 3. Разрешение B2C-скидки

Все проценты хранятся целым `discount_bps`:

```text
10000 bps = 100.00%
payable_multiplier_bp = 10000 - discount_bps
charged_nano = floor(official_nano * payable_multiplier_bp / 10000)
```

Разрешение выполняется в строгом порядке:

1. exact `(provider_id, canonical_model_id)` model-rule;
2. provider-rule;
3. global B2C default `discount_bps=5000`;
4. отсутствие любого применимого правила — fail closed, а не legacy scalar fallback.

Пример:

```text
global B2C       = 50%
provider Gemini = 60%
Gemini image     = 55%
```

Обычная Gemini-модель получает скидку 60%, а image-модель — 55%. Model-rule заменяет provider-rule
целиком; проценты не складываются.

Официальная стоимость сначала вычисляется `crates/metering` в integer nanoUSD с immutable tariff
identity. Policy применяется к готовой official-стоимости. Float/JavaScript `number` для денег
запрещены.

## 4. B2B, OpenKeys и service

### B2B

Policy принадлежит конкретному B2B owner и содержит provider/model rules с тем же приоритетом
model → provider. Global B2C policy на B2B никогда не распространяется.

При backfill существующий `mult_bp` преобразуется так:

```text
discount_bps = 10000 - mult_bp
scope = provider:anthropic
```

Миграция не выдаёт B2B доступ к OpenAI или Gemini. После cutover оператор может явно добавить их
provider/model rules через полную CAS-замену policy.

### OpenKeys

Все OpenKeys, включая ранее считавшиеся legacy, получают canonical immutable 1:1 policy. История
прошлых списаний не переписывается, но после глобального cutover любой новый reserve использует
`payable_multiplier_bp=10000`. В API выпуска нет multiplier/discount override.

Новая модель становится доступной OpenKeys только через явную новую generation OpenKeys product
catalog. Это не меняет правило 1:1.

### Service

Service policy содержит purpose/responsible metadata и `billing_mode=meter_only`; она не содержит
продуктовую скидку как способ обойти balance gate. Каждый завершённый запрос сохраняет:

- account/key/request identity;
- provider и canonical model;
- tariff identity и official cost components;
- фактический upstream usage;
- runtime/release lineage.

Customer debit, balance reserve и `402 insufficient balance` для service не выполняются. Ошибка
записи usage/settlement остаётся fail-closed по обычному durable outbox-контракту: «не зависит от
баланса» не означает «можно потерять учёт».

## 5. Welcome bonus и funding

Новая eligible Google/GitHub B2C-регистрация получает идемпотентный credit:

```text
amount_nano = 5000000000
ref = signup-bonus:<commercial-user-id>
source_type = welcome_bonus
eligibility = any_b2c_model
```

Password, B2B, OpenKeys и service аккаунты бонус не получают. Смена цены не меняет номинал уже
зачисленного бонуса.

Funding lots нужны для честного paid/bonus attribution, но не ограничивают bonus конкретным
pricing-mode. Reserve расходует welcome bonus первым, затем paid; settlement и refund используют
точную allocation, сохранённую при reserve. Поэтому referral получает только paid-funded часть.

Для существующего аккаунта online-backfill выполняется под тем же account row/advisory lock, что и
money writers:

1. прочитать актуальный aggregate balance, ledger и уже нормализованные lots;
2. восстановить остаток точных `signup-bonus:*` credits;
3. записать его как `welcome_bonus`;
4. классифицировать весь прочий остаток как `paid`;
5. проверить `sum(bucket balance/reserved/spent) == account balance/reserved/spent`;
6. атомарно отметить funding generation готовой.

Ручного reviewer artifact и ручного разбора отдельных аккаунтов нет. Несходящаяся арифметика,
negative overflow, конфликт replay или неизвестная незавершённая legacy reservation — технический
blocker, а не повод угадать значение.

## 6. Удаление прогрессивной модели

В целевом runtime, API, UI и новых durable records отсутствуют:

- pricing mode `track`;
- tier ladder и Starter/следующие tiers;
- 30-day retention spend/retention eligibility;
- track eligibility и track-only funding;
- зависимость commission eligibility от pricing mode;
- фоновые tier reconciliation/month-close jobs;
- публичные обещания прогрессивной скидки.

Существующие append-only миграции и immutable исторические ledger/snapshot rows не переписываются:
они могут содержать старые строки для аудита. Новый код не создаёт их и не использует для текущего
admission, цены, funding или комиссии. Это сохранение истории, а не compatibility path.

Физическое удаление старых mutable commerce columns/tables возможно только отдельным поздним
изменением после доказанного отсутствия readers/writers. Продуктовая семантика удаляется до этого и
не ждёт schema cleanup.

## 7. Zero-downtime rollout

### 7.1. Почему нельзя переключать аккаунты по одному

Последовательный перевод bindings создаёт смешанное production-состояние и требует canary/ручного
учёта. Вместо этого вводится immutable pricing release:

- release содержит exact capability/catalog/switch identities;
- global B2C policy identity;
- все B2B, OpenKeys и service assignments;
- funding generation;
- minimum runtime capability;
- canonical digest всего manifest.

Engine хранит prepared releases и один active release head. Подготовка release не меняет traffic.
Все запросы читают active head и связанные immutable данные в одном PostgreSQL snapshot.

### 7.2. Expand и dual-compatible runtime

Сначала отдельными migration-first коммитами добавляются только новые структуры. Затем blue-green
выкатывается runtime, который:

- engine migration `crates/registry/migrations_pg/0023_pricing_release_funding_v2.sql` добавляет
  release/funding authority, request snapshots, deferred aggregate/allocation invariants и nullable
  v2 lineage старых writer surfaces;
- engine migration `crates/registry/migrations_pg/0024_pre_cutover_funding_snapshots_v2.sql`
  добавляет независимый от prepared release immutable funding snapshot для запросов переходного
  периода. Он разрывает цикл «release assignment требует funding generation, а normalized writer
  требует allocation snapshot» и не создаёт release head, policy либо новый pricing path;
- commerce migration `packages/db/migrations/0026_pricing_release_expand.sql` добавляет policy,
  inventory, target/recovery plan, resumable Stage 6/control job, Stage 8 evidence и activation
  receipt authority;
- sales migration `packages/sales-db/migrations/0015_paid_funded_commission_v2.sql` добавляет
  отдельные immutable usage/commission v2 tables без pricing-mode поля.

Все три migration surfaces пусты и dormant: наличие таблиц не создаёт policy, release head,
funding generation или live consumer. Зависимый producer/runtime допускается только после зелёных
production migration/watchdog exact schema SHA.

Первый зависимый engine producer добавляет PostgreSQL-only `/admin/pricing/v2/*` prepare/read:
immutable policy/release/recovery link, полный cursor inventory и nullable release head. В нём нет
activation route и он не публикует v2 runtime-capability claim, поэтому подготовленные rows остаются
dormant и не могут изменить data-plane. После зелёного `deploy/watchdog` exact producer SHA строгие
wire-схемы и typed prepare/read methods добавляются в `packages/contracts` и
`packages/engine-client`; activation method и вызывающий application job по-прежнему отсутствуют.

- продолжает обслуживать текущий active legacy release;
- умеет читать новый release schema;
- сохраняет immutable pricing/funding snapshot в каждой новой reservation;
- dual-writes новые topup/bonus/reserve/settlement данные в aggregate и новые funding lots;
- поддерживает `meter_only` service settlement;
- не создаёт новых tier/track records.

Новый release остаётся dormant. Поэтому deploy runtime сам по себе не меняет ни цену, ни доступ.

До появления global release head точная pricing identity остаётся в существующем immutable
`pricing_admission_snapshots`, а funding generation/bonus-first allocations — в отдельном
`funding_reservation_{snapshots,allocations}_v2`. После активации release новые requests используют
связанные `pricing_request_{snapshots,funding_allocations}_v2`. Оба формата закрепляют reserve-time
решение и позволяют старому запросу завершиться после cutover; один request не может иметь оба
funding snapshot одновременно.

### 7.3. Online backfill без остановки writers

Backfill идёт аккаунт за аккаунтом короткими транзакциями. Он берёт только account-local lock.
Запрос этого аккаунта может кратко ждать его транзакцию; остальные аккаунты продолжают работать.

Новые writers используют тот же lock и повторно читают funding generation после ожидания. Поэтому
они либо полностью проходят по legacy+dual-write пути до backfill, либо полностью по новому пути
после него; потерянного промежуточного write нет.

Legacy-format reservations и outbox rows естественно завершаются на работающей системе. Новые
reservations уже несут release/funding snapshot. Stage 8 требует ноль незавершённых legacy-format
rows, но не ноль всех активных запросов.

### 7.4. Защита provisioning race

Account create/activation и pricing release activation берут общий control-plane advisory lock.
Пока готовится release, новые аккаунты получают обе совместимые identities до выдачи usable key.
Финальная транзакция повторно инвентаризирует все active accounts и отказывается переключать head,
если хотя бы один аккаунт не классифицирован.

Data-plane reserve/settlement этот глобальный lock не берут. На время CAS может на миллисекунды
подождать только создание/активация аккаунта, но не клиентский traffic.

### 7.5. Full shadow вместо canary

До cutover новый resolver вычисляется в shadow для 100% поддержанных запросов. Shadow ничего не
резервирует и не списывает второй раз. Он сравнивает admission, official cost, resolved discount,
funding availability и release lineage с ожидаемым target.

Canary-account list не создаётся. Stage 8 принимает только полное покрытие инвентаря и отсутствие
необъяснённых расхождений.

### 7.6. Атомарный массовый cutover

Stage 9 выполняет одну короткую `SERIALIZABLE` транзакцию под pricing-release advisory lock:

1. перечитывает exact Stage 8 evidence и prepared release digest;
2. проверяет minimum runtime capability на обоих blue-green слотах и rollback floor;
3. проверяет все active accounts, funding generations и отсутствие legacy-format inflight rows;
4. проверяет отсутствие pending/retry/dead control jobs и policy ACK drift;
5. CAS-переводит единственный active release head на target generation;
6. фиксирует operator/reason/time и evidence digests в activation record.

Транзакция не изменяет балансы и не обновляет N account rows. После commit следующий reserve любого
клиента видит новый release. Уже начатый запрос завершается по immutable snapshot, сохранённому при
его reserve. Поэтому traffic не останавливается и один запрос не смешивает старую цену с новой.

### 7.7. Recovery без остановки

Перед cutover готовится recovery release следующей monotonic generation с v2-compatible
семантикой предыдущей production-цены. Если post-activation automation обнаруживает системную
ошибку, она активирует recovery generation тем же single-head CAS. Возврат старого binary, который
не понимает новый release/funding schema, запрещён.

Rollback не удаляет immutable policies, funding lots или snapshots и не откатывает завершённые
списания. Exact replay activation возвращает `unchanged`.

## 8. Переразмеченные этапы

### Stage 5 — target materialization

Planner строит B2C 50%, B2B Anthropic migration, canonical OpenKeys 1:1 и service `meter_only`
assignments. Отдельной ручной assignment matrix нет: authoritative inventories обязаны полностью и
однозначно покрыть active engine accounts; любой collision/missing owner блокирует apply.

### Stage 6 — online funding normalization

Stage 6 становится resumable online-backfill с owner-approved правилом «точный остаток welcome,
всё остальное paid». Он не требует maintenance window, reviewer artifact или zero reservations.

### Stage 7 — OpenKeys

OpenKeys issuance заранее переходит на canonical 1:1 policy. Existing inventory также готовится к
1:1 target release; live цена меняется только вместе со всеми аккаунтами на Stage 9.

### Stage 8 — full-inventory evidence

Read-only evidence связывает exact commerce/engine/OpenKeys inventories, все policy ACK, funding
generation, 100% shadow, runtime capability и prepared/recovery release digests. Evidence имеет TTL
и становится stale при любом control-plane изменении.

### Stage 9 — one-head activation

Canary planner удаляется. Единственное apply-действие — CAS active release head для всего
production inventory. Stage 9 не останавливает сервис и не требует ручной финансовой подписи.

## 9. Контракты UI и API

Клиентский B2C pricing view показывает effective discount и источник правила:

```text
global_default | provider_override | model_override
```

Он не показывает tier, retention progress или внутренние release digests. B2B видит только свою
policy. OpenKeys показывает 1:1. Service usage доступен операторам как official cost без balance.

Admin pricing editor обязан:

- редактировать global B2C default, provider и model rules;
- показывать effective preview и приоритет exact model;
- не предлагать `track`/tier controls;
- не применять B2C rules к B2B/OpenKeys/service;
- управлять B2B полной policy;
- показывать service как all-model `meter_only`;
- показывать prepared/active/recovery release и Stage 8 freshness.

Sales feed после expand-only producer-first перехода получает `commission_eligible` независимо от
pricing mode и exact `paid_funded_nano`. Старые поля могут временно присутствовать для deployed
consumer, но новый consumer их не использует; удаление — последним контрактным шагом.

## 10. Обязательные тесты

- B2C: global 50%, provider override, model override, model-over-provider precedence.
- B2B: scalar migration только в `anthropic`; отсутствие B2C inheritance.
- OpenKeys: existing/new 1:1; любой discount override отклоняется.
- Service: все runtime models доступны; нулевой баланс не даёт 402; official usage durable.
- Welcome: новая выдача $5, exact idempotency, старые $4 не увеличиваются, все B2C providers
  разрешены, bonus-funded usage не комиссионируется.
- Referral: paid-funded commission сохраняется без pricing-mode eligibility.
- Funding backfill: concurrent topup/reserve/settle, account-local lock, exact replay, bucket sums.
- In-flight cutover: reserve до activation и settlement после неё используют один snapshot.
- Provisioning race: новый active account не может отсутствовать в target release.
- Stage 8: 100% inventory/shadow, Gemini, no legacy-format inflight, exact runtime floor.
- Stage 9: single-head atomicity, no N-account writes, exact replay, stale evidence rejection.
- Recovery: forward activation recovery generation без старого binary и без traffic stop.
- Cleanup: active code/API/UI не создают и не читают tier/retention/track semantics.

## 11. Definition of Done

Работа завершена только когда одновременно выполнено следующее:

1. Все expand migrations доставлены раньше зависимого кода.
2. Ни один новый writer не создаёт прогрессивные pricing records.
3. Все active accounts однозначно классифицированы B2C/B2B/OpenKeys/service.
4. Gemini присутствует в main catalog; product-specific OpenKeys enablement задан явно.
5. B2C global/provider/model resolution соответствует контракту.
6. B2B мигрирован только в Anthropic rule; OpenKeys target строго 1:1.
7. Service работает при нулевом балансе и полностью учитывает official usage.
8. Новый welcome bonus равен $5; referral paid-funded math зелёная.
9. Online funding backfill завершён без глобальной остановки.
10. Stage 8 green на полном инвентаре и 100% shadow.
11. Prepared target и recovery releases exact и поддерживаются текущим runtime floor.
12. Stage 9 одним CAS переключил весь production inventory; canary не использовался.
13. Post-activation smoke и monetary invariants зелёные на exact deployed SHA.
14. Public/admin/customer/sales документы и UI больше не обещают tiers или track-only bonus.
15. `deploy/watchdog` зелёный на финальном SHA.

Production mutation не должна выполняться из исследовательской или документационной задачи.
Применение Stage 6/8/9 выполняется только после реализации, тестов и штатной доставки каждого
expand-only producer step.
