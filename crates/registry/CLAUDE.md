# crates/registry — CLAUDE.md

**Роль:** engine-owned PostgreSQL authority. SQLite is a one-time migration source and emergency audit snapshot.

PostgreSQL schema DDL is an explicit operation (`claude-api db migrate-engine`), not a service-start
side effect. `serve` may only perform the read-only schema verification before claiming an owner.

**Владелец-ветка:** `comp/registry`.

**Границы (жёстко):**
- Зависит только от sync PostgreSQL client, `rusqlite` (import/fallback), `anyhow` и чистых
  `serde`/`serde_json`/`sha2` для typed persistence и canonical immutable identities.
- НИКАКОЙ сети, HTTP, чтения env, логики выбора подписок. Только персист + CRUD + `load_active`.
- **Биллинг: АККАУНТЫ клиентов (`accounts`) + ключи-доступы (`api_keys`) + журнал (`ledger`)** — здесь,
  но ТОЛЬКО хранение/атомарные движения в целых нанодолларах. Модель: **баланс на АККАУНТЕ** (профиль
  юзера), ключи (`api_keys.account_id`) — доступы к общему балансу (1:N, на проекты/команду); per-key
  `spent_nano` — атрибуция расхода по ключу. Функции: `account_create/get/by_handle/list/set_status/rm`,
  `account_topup` (+ledger), request-keyed `reserve_request`/`settle_request` (атомарно: баланс аккаунта + per-key
  spent + ledger-строка), `key_issue(account_id,label)/get/list/set_status/set_status_by_id/remove/clear`;
  `api_keys.key_id` — стабильный не-секретный control-plane ID для отзыва без хранения полного ключа,
  `key_account` (JOIN ключ→аккаунт для авторизации), обёртка `Billing` (Mutex<Conn>). Подсчёт стоимости
  (токены→нано) сюда НЕ лезет — это `metering`; registry принимает готовую сумму. **Инвариант денег
  (с овердрафт-буфером):** резерв держит ПОЛ баланса на −$1 (`OVERDRAFT_NANO`, синхронно с
  `metering::OVERDRAFT_NANO`) — funded-запрос НЕ роняется 402 из-за гонки конкурентных резервов; за полом
  любой положительный hold отбит (макс $1 в долг per-account). Settle списывает РЕАЛЬНОЕ (`actual`
  может быть > hold — из остатка баланса), forward-кап держит списание в пределах hold+$1. То есть
  `hold ≤ balance+$1` и `charge ≤ hold+$1` на уровне АККАУНТА. `reservations.request_id` is the ownership key;
  `settlement_outbox` retries until the same transaction updates balances and inserts the unique
  `(kind,request_id)` charge. Upstream request IDs are audit metadata, never the money identity.
  Cursor consumers use `ledger_after(account, after_id, limit)` (oldest-first); account pricing uses
  `account_set_mult_bp`. Control reads use `account_funding_snapshot` so scalar totals and grouped
  paid/welcome/other buckets are one snapshot; the residual remains explicit `unattributed`.
  `ledger_recent`/`ledger_after` read each immutable ledger page and its normalized funding
  allocations in one backend snapshot and expose stored policy/runtime lineage without historical
  inference. SQLite and PostgreSQL semantics must stay identical.
  Optional per-key `spend_limit_nano` and `expires_ts` are engine-authoritative. Reservation updates
  key `reserved_nano` in the same transaction and enforces
  `spent_nano + reserved_nano + hold <= spend_limit_nano`; settlement atomically converts the hold
  into actual spend or releases it. Mutable policy replacement is account-scoped and atomic with
  reservations; a non-null new limit must cover both settled and reserved usage.
  Мягкая миграция старой модели «key=кошелёк» → аккаунт per-key (`migrate_legacy_keys`).
- Публичный тип [`Sub`] (email/token/proxy/fleet) — контракт для `pool`/`forward`. Меняешь его —
  проверь оба потребителя.
- **Durable auth-health подписки** (детект забаненного токена): additive-колонки на `subs`
  (`auth_state`/`auth_fail_streak`/`dead_since_ts`/`dead_reason`/`auth_token_fp`, migration `0003`).
  `SubHealth` + `load_sub_health(fleet)` / `save_sub_health(owner,&h)` — вердикт пишет ТОЛЬКО поллер
  (owner-fenced в PostgreSQL, как деньги/pool_state); машина состояний живёт в `pool`, registry лишь
  персистит готовую строку. `subs_admin` отдаёт вердикт панели. `add`/`add_file` сбрасывают health в
  healthy при (пере)выпуске токена (авто-ревайв). PostgreSQL — authority; SQLite-зеркало лишь для сборки.
- **Персист состояния пула (таблица `pool_state`)** — CAS-versioned and owner-epoch fenced. Atomic
  `capacity_leases` validate cooldown/utilization and atomically track inflight without rejecting
  concurrent work; release/renew/reconciliation remain durable and owner-fenced.
  `leader_leases` elect exactly one poller. `PoolStateRow` (примитивы, registry
  не знает типов `pool`) + `save_pool_state`/`load_pool_state`. Хранит durable-состояние (cooling/
  калибровка/spent/util/reset) для переживания рестарта. Логику решает `pool` (export/import), registry
  лишь пишет/читает готовые строки. No Redis/Redlock participates in correctness.
- Provider attribution — стабильная registry-константа: Anthropic/Codex сохраняют существующие
  значения, native Gemini usage пишет `PROVIDER_GOOGLE = "google"`. Нельзя подменять provider доменом,
  profile/project ID или клиентским значением; Google project IDs в registry не хранятся.
- Gemini quota calibration persistence хранит immutable raw observations, exact cumulative profile
  spend и CAS-derived state для двух fixed buckets. `observed_spend_nano` добавлен expand-only
  migration 0014 и хранит точный `ΣΔspend` workload estimator v2; смысл blend/envelope/confidence
  остаётся в `forward`, registry только валидирует и атомарно сохраняет примитивы.
- Codex calibration migrations 0015/0018 expand-only добавляют fixed-point quota evidence и
  независимый native ChatGPT-credit ledger. Каждый успешный turn записывается как immutable
  `CodexTurnCalibrationEvent`: opaque внутренний `request_id`, home/model, effective Standard/Fast,
  provider-reported tier, обе schedule identity, disjoint token classes, четыре API nanoUSD legs и
  три ChatGPT nanocredit legs. Insert события и продвижение обоих cumulative ledgers выполняются
  одной транзакцией. Exact replay возвращает уже сохранённые totals без второго списания; другой
  semantic payload под тем же request ID даёт typed permanent conflict и не меняет ни один ledger.
  Aggregate report группирует только immutable rows по home/model/tier/schedules.
  Nullable credit-колонки намеренны: migration-first rollout переживает старый runtime, а estimator
  v9 при первом credit-bearing observation сохраняет legacy API estimate в `last_*`, сбрасывает оба
  current estimate и начинает единый новый anchor. Исторический API spend никогда не становится
  ложным zero-credit evidence. Raw observations остаются replay authority, включая rolling reset;
  workload/envelope/confidence и possibly-unattributed semantics принадлежат `forward`, registry
  только валидирует и CAS-сохраняет exact integer primitives.
- Provider calibration migration 0019 expand-only создаёт общий immutable event ledger для
  Claude/Gemini и отдельный exact cumulative nanoUSD ledger по provider subject. Event сохраняет
  provider-specific token/tool/search classes, tariff schedule, speed/geography и disjoint API-cost
  legs; native quota snapshots остаются в своих существующих Claude/Gemini authorities. Старый
  runtime таблицы не читает и не пишет, поэтому migration SHA безопасно выкатывается до dependent
  application commit. Операторский read отдельных событий всегда newest-first и bounded максимум
  512 строками; безлимитный ledger scan в control-room запрещён.
- Gemini exact-calibration migration 0022 expand-only создаёт новую plan-scoped authority для
  `gemini-5h`/`gemini-weekly`, не меняя legacy-таблицы, в которые продолжает писать старый runtime.
  Новая raw observation сохраняет реальное decimal resolution, source/request attribution и exact
  cumulative spend из общего migration-0019 ledger; CAS-state добавляет unattributed movement и
  честные nullable bounds. Dependent runtime переключается на эти таблицы только после отдельного
  migration SHA с зелёным deploy, а старое derived evidence не переносится без недостающих
  plan/resolution/source фактов.
- Claude calibration migration 0020 expand-only создаёт отдельные fixed-point authority для 5h/7d
  окон: plan входит в identity, raw observation хранит реальное разрешение quota fraction, источник
  и optional request ID, а CAS-state — exact observed spend/fraction, low/high/confidence и
  unattributed movement. Номинала подписки, prior/EMA и float money в таблицах нет. Старый runtime
  их не читает и не пишет; dependent release должен сначала durable записать turn из migration 0019,
  затем связать quota snapshot с полученным cumulative subject spend. Event insert + subject spend
  advance — одна транзакция: exact request replay возвращает существующий total и не списывает
  повторно, отличный semantic payload даёт typed permanent conflict без изменения ledger.
  Observation immutable-keyed по subject/plan/window/reset/time/source; 5h и 7d никогда не делят
  anchor/history. Registry хранит и валидирует только integer evidence/CAS primitives; estimator
  replay, reset jitter, one-snapshot lag, plan pooling и delivery retry принадлежат `forward`/`server`.
  PostgreSQL initial CAS insert обязан явно типизировать version placeholder как `bigint`: выражение
  с untyped integer literal иначе выводит параметр как `int4` и блокирует durable FIFO до recovery.
- KIMI calibration migration 0027 хранит отдельную plan+duration authority с requested/served model
  и raw `used/limit` quota evidence. `record_kimi_turn` делает immutable insert и cumulative
  subject spend одной transaction: concurrent exact replay сериализуется через
  `ON CONFLICT(request_id)`, другой payload конфликтует, а tracking/update timestamps сохраняют
  earliest/latest даже при out-of-order finalizers. `save_kimi_calibration` вставляет observation и
  двигает derived state одним version CAS; проигравший CAS rollback'ит observation, history читается
  oldest-first. Real-PostgreSQL gate — `pg::tests::kimi_calibration_postgres_matrix`.
- **Execution-group fencing runtime (phase 6.3):** migration 0021 expand-only добавила к
  `reservations` nullable `group_id`, positive one-based `attempt` и insert-first-wins
  `execution_group_winner`. `group_id IS NULL` означает effective group `request_id`; все reserve
  API сохраняют direct wrappers и group-aware варианты, а exact replay сравнивает оба поля.
  Nonzero settlement в той же транзакции захватывает winner slot; loser получает effective actual
  0/full refund, strict funding terminalизируется как cancel без usage/charge, исходный outbox
  payload не переписывается. Zero/cancel winner не назначают. Winner pruning разрешён только после
  удаления последней reservation с тем же `COALESCE(group_id,request_id)`. SQLite и PostgreSQL
  обязаны оставаться семантически идентичны; процессный loser counter/log — только incident-tripwire,
  correctness принадлежит PK таблицы.
- **Multi-provider pricing Stage 3A** (`pricing`) — dormant persistence contract: immutable
  catalog/switch/account-policy versions сначала `prepare`, затем отдельные heads/binding двигаются
  только явным monotonic CAS `activate`. SQLite и PostgreSQL обязаны возвращать одинаковые typed
  outcomes и атомарно сохранять parent со всеми children. Registry-owned timestamps не входят в
  identity; policy хранит полные catalog/switch/capability/source pins. `Strict` здесь fail-closed,
  legacy OpenKeys replacement-locked и сверяется с live `accounts.mult_bp`, current OpenKeys только
  1:1 без model-rules. До Stage 3B у API нет runtime/HTTP writer и production activation. Будущий
  resolver обязан на каждом чтении проверить exact immutable policy dependencies и независимо
  применить текущие admission heads. Current heads не обязаны равняться policy pins: отдельные
  catalog → switches → policy activations не являются общей транзакцией.
- **Stage 3C Control API persistence surface:** server may expose the existing typed
  prepare/read/activate contract through its authenticated `/admin/pricing/*` routes, but registry
  remains HTTP-free. Every write still runs on the billing single writer; reads use one backend
  transaction. JSON serde on pricing DTOs is strict about unknown struct fields and uses stable
  snake_case enums. Exact replay returns `Unchanged`; invalid, missing dependency, stale,
  version-conflict, catalog/switch CAS, policy-binding CAS and immutable-lock remain distinct typed
  outcomes. This surface does not seed/backfill data, issue keys or enable strict enforcement.
- **Stage 3B0/3B1b snapshot read — dormant:** `pricing_read_bundle(account_id)` за одну read-only
  транзакцию возвращает live `accounts.mult_bp`, binding/active policy, exact immutable
  `policy_catalog/policy_switches` и текущие `admission_catalog/admission_switches`: SQLite через
  deferred snapshot, PostgreSQL через `REPEATABLE READ READ ONLY`. Active policy обязана найти обе
  pinned dependencies или read падает как integrity error; inactive binding получает только
  admission heads, unbound account — ни одной пары. Scalar входит в тот же snapshot, иначе legacy
  OpenKeys validation гоняется с multiplier writer. Registry только материализует данные;
  независимые policy/admission gates выполняет чистый resolver в `forward`. Нельзя собирать bundle
  последовательностью отдельных `active_*`/`*_by_generation` reads — это смешивает поколения.
  Единственный runtime caller — default-off bounded Stage 3B1c worker: он читает bundle только
  через отдельный PostgreSQL shadow-reader actor и не участвует в readiness/admission/money.
- **Stage 3B1a shadow schema:** PostgreSQL migration `0009` и SQLite parity создают
  отдельную immutable `pricing_shadow_admission_evaluations`. Она не подменяет actual
  `pricing_admission_snapshots`: shadow-строка ссылается на уже зафиксированный actual snapshot и
  хранит обе lineage-пары (`policy_*` и `admission_*`), runtime manifest, scalar comparison и typed
  outcome. Dependency capability pins exact-связаны с immutable catalog/switch versions. Dormant
  typed SQLite/PostgreSQL insert/read API уже вычисляет canonical manifest digest из полного
  отсортированного member-set, проверяет membership всех четырёх pins до записи и при чтении заново
  вычисляет `evaluation_digest`. Exact replay с другими timestamps/diagnostics возвращает первую
  строку; отличный semantic digest — typed conflict, а не update. Manifest members служат
  insert-time evidence и в строке не дублируются; standalone read подтверждает manifest identity,
  но без исходного manifest не перечисляет members заново. Default-off Stage 3B1c worker теперь
  может писать эти строки только после атомарного actual snapshot; migration сама по-прежнему не
  создаёт heads, policies или seed data.
- **Stage 3B1c.1/3B1c.2 actual legacy snapshot foundation:** typed
  `LegacyScalarAdmissionSnapshot` фиксирует exact request/account, fixed-plane provider
  (`anthropic|openai|google`), requested/canonical model, alias/tariff identities, timestamps, scalar,
  official/charged hold и provider-typed premium modifiers. Registry сам строит и при каждом чтении
  перепроверяет `sha256:v1:<hex>` по versioned binary TLV с отдельным domain separator; JSON premium
  modifiers — только строгая storage-проекция, не digest source. Новые
  `sqlite_reserve_request_with_legacy_snapshot` и
  `PgStore::reserve_request_with_legacy_snapshot` используют `snapshot.charged_hold_nano` как
  единственный hold source и атомарно сохраняют деньги, reservation и snapshot. Exact retry активной
  `reserved|delivering` reservation возвращает сохранённый typed snapshot без продления lease и без
  второй money mutation; mismatch, terminal state, non-legacy snapshot или старая reservation без
  snapshot дают typed conflict. PostgreSQL сохраняет owner fence и request advisory lock.
  Guarded-варианты обоих API вызывают caller-owned commit gate только для insert/exact replay после
  всех fallible writes и финального owner fence, непосредственно перед commit; закрытый gate
  полностью откатывает попытку как `AbortedBeforeCommit`. `NotReserved`, conflict и более ранняя
  ошибка gate не вызывают. Старые reserve API не изменены и snapshot не создают. Миграции не
  добавлялись: используется actual schema `0006`. Default-off live sampler и atomic caller
  обслуживают Anthropic/OpenAI/Google; Google хранит typed `gemini_v1` reserve modifiers и durable
  provider ID `google`, а не deprecated `gemini`. Только snapshot-bearing success может передать
  работу bounded shadow producer. Production config остаётся выключенным.
  Новый PostgreSQL writer после потенциального ожидания request-lock повторно проверяет owner через
  `FOR UPDATE`, удерживает epoch-row до commit и использует свежий reservation timestamp; real-PG
  race test доказывает rollback старого epoch без money/orphan writes. Snapshot constructor
  гарантирует storage shape, но не model/tariff provenance: live caller обязан строить input только
  из `metering` canonicalizer. Для live atomic API принят bounded idempotency contract: immutable
  `admission_ts` допускает replay только при возрасте `<24h`; future/expired timestamp возвращает
  typed conflict до money mutation, включая повторную проверку после потенциального DB lock wait.
  Terminal reservation и actual/shadow children хранятся отдельно от ledger/usage 30 дней, а
  registry отвергает любой более свежий prune cutoff до открытия транзакции; maintenance сообщает
  точные cascade counts. Это не permanent tombstone и не infinite dedupe:
  bridge обязан использовать только внутренний CSPRNG UUIDv4, сохранять первый timestamp и иметь
  queue `max_age <24h`. SQLite и PostgreSQL сохраняют унаследованные разные balance gates
  (full-cover против overdraft floor);
  parity этого checkpoint относится к atomic snapshot/replay/conflict, а не к `NotReserved`.
- **Stage 3B1c shadow evaluation persistence:** `ShadowActualSnapshotRef` строится
  только из validated actual snapshot; fixed-plane identity, scalar и holds нельзя независимо
  подменить caller-ом. Registry вычисляет policy hold checked integer half-up, сам выводит
  `equal|different`. Actual ниже checked scalar quote считается exact funding ceiling, который
  одинаково ограничивает policy candidate; actual выше scalar quote fail closed. Compatibility
  enum старого balance-cap drop больше не эмитится. Resolved outcome хранит exact immutable
  policy/rule и обе lineage-пары; rejected требует observed scalar, read-error его не допускает.
  Diagnostic JSON
  неавторитетен, исключён из digest и ограничен одинаковым для SQLite/JSONB контрактом по compact
  bytes, NUL, depth и items. PostgreSQL сериализует request через отдельный advisory namespace и
  держит parent actual `FOR KEY SHARE` до immutable insert; SQLite использует `BEGIN IMMEDIATE`.
  API не читает current heads и не re-resolve-ит historical evidence. Pure forward work-item/builder
  использует registry-owned typed eligibility gate до enqueue, выводит resolver manifest
  только из canonical evidence и сверяет identity до формирования input. Read-only outcome getter
  не выполняет persistence. Timed PostgreSQL wrappers set transaction-local statement/lock timeout;
  live reads use a separate bounded actor budget, while inserts pass through the existing billing
  writer without transient retry. SQLite APIs remain for parity/tests and have no live producer.
- **Stage 8 engine evidence v2:** PostgreSQL-only read report материализуется в одной
  `REPEATABLE READ READ ONLY` транзакции и принимает exact target/recovery generations. Помимо
  active main/openkeys graph, classifications и полного actual→shadow покрытия он перечитывает
  prepared target/recovery releases и recovery link, сверяет оба full-inventory assignment set,
  их общий funding identity, live funding heads/lots с aggregate, текущие canonical
  `engine_inventory_digest`/`funding_digest`, target rule precedence и наблюдаемый audit count
  незавершённого legacy-format inflight без требования нуля. Compile-fixed pricing capability и
  отдельный release schema version не
  смешиваются: каждый live `engine_instances` обязан заявить release/funding schema v2 и
  непустой runtime digest; отсутствие хотя бы одного такого claim — blocker до отдельного Stage 9
  runtime checkpoint. `shadow_digest`, `runtime_floor_digest` и весь report получают canonical
  `sha256:v2` identity. Внешний Gemini admission aggregate и durable provider=`google`
  usage/outbox остаются bounded audit counts, но не заменяют обязательные Google actual snapshots
  и shadow evaluations. Subject identities выходят только как SHA-256 digests. Report ничего не
  активирует и не исправляет; любой blocker должен остановить Stage 9.
  При absent release head inventory означает полный base manifest для cutover. При exact target
  head тот же endpoint строит fresh recovery evidence по immutable base inventory и принимает
  post-cutover account только через exact paired target/recovery extension с live funding parity;
  другой active head является blocker.
- **Целевой Stage 5/6/9 контракт:** authoritative inventories полностью заменяют ручную assignment
  matrix. Funding normalizes online account-local transactions: exact historical welcome остаётся
  bonus, residual считается paid; новые grants `$5`, reviewer artifact и global money drain не
  нужны. Prepared pricing release связывает весь inventory, а Stage 9 меняет один global active
  head. Registry обязан атомарно сохранить reserve-time release/funding snapshot, разрешить
  in-flight v2 settlement через cutover и поддержать service `meter_only` без balance debit.
- **Pricing release/funding v2 schema checkpoint:** PostgreSQL migration `0023` создаёт пустые
  immutable policy/release/assignment/evidence authorities, один отсутствующий до activation
  global head, per-account funding generations/lots/heads и request/ledger allocations. Deferred
  constraints держат account↔generation↔lot и reservation↔allocation суммы, включая overrun
  `charged > reserved` только при нулевом release; reserve snapshot закрепляет exact
  release/assignment/policy/rule/tariff и не обновляется. Nullable lineage в
  `settlement_outbox`/`usage_events`/`ledger` сохраняет старых writers валидными и обязана точно
  ссылаться на snapshot для v2 rows. Service policy не имеет product catalog/switch/rules,
  `meter_only` требует нулевой customer charge. Новый runtime не использует эти структуры до
  отдельного producer SHA после зелёного migration/watchdog этого checkpoint.
- **Pre-cutover funding snapshot checkpoint:** PostgreSQL migration `0024` добавляет независимые
  `funding_reservation_snapshots_v2`/`funding_reservation_allocations_v2` для account-local Stage 6.
  Они не выбирают pricing release и не создают head: до Stage 9 существующий immutable pricing
  snapshot остаётся authority цены, а новый snapshot закрепляет только active funding generation,
  bonus-first lot order и paid-only overdraft. Deferred coverage запрещает новую незавершённую
  reservation нормализованного аккаунта без ровно одного compatible funding snapshot. Runtime
  writers подключаются только отдельным SHA после зелёного migration/watchdog; funding head после
  первой normalization не удаляется и двигается только monotonic generation/version step.
- **Pre-cutover funding dual writers:** все три PostgreSQL reserve-пути (scalar,
  legacy-snapshot и strict-policy), settlement outbox и `account_topup` сериализуются в порядке
  `request advisory → funding account advisory → reread head → row locks/money writes` (у top-up
  нет request lock). Пока funding head отсутствует, transaction сохраняет прежнюю legacy-семантику;
  после появления head та же transaction обязана обновить account aggregate, active generation,
  lots и immutable reservation allocations вместе. Reserve распределяет `welcome_bonus` первым,
  затем `paid`; единственный разрешённый overrun не превышает `$1` и относится к последней paid
  allocation, включая zero paid anchor для bonus-only/zero hold. Нормализованная
  balance-generation обязана поэтому содержать paid lot даже при нулевом residual; его отсутствие
  fail closed. Settlement использует только
  reserve-time allocation, пишет `funding_ledger_allocations_v2`, а terminal replay не повторяет
  money mutation и остаётся валиден после monotonic advance funding head. `signup-bonus:*`
  top-up создаёт welcome lot, остальные credits и negative adjustments — paid lot. Реальный
  PostgreSQL gate: `pg::tests::pre_cutover_funding_v2_writer_postgres_matrix`; он доказывает
  replay/cancel/settle/overrun/outbox recovery и обе lock-order гонки.
- **Online account-local funding normalization:** `funding_normalization_v2` строит read-only
  content-addressed `sha256:v2` plan и применяет только exact source/target identity в одной
  `SERIALIZABLE` PostgreSQL transaction под тем же funding-account advisory lock. Legacy active
  reservation с неоднозначной bonus/paid attribution блокирует только свой account. Если exact
  buckets или ledger доказывают, что весь active reserve принадлежит `paid` (включая полностью
  исчерпанный welcome), apply в той же transaction создаёт generation/lots/head и immutable
  paid-only funding snapshots/allocations для каждого такого запроса; pricing snapshot не
  переписывается. Stale state перепланируется, exact replay возвращает `unchanged`. Exact старый
  welcome bucket переносится в `welcome_bonus`, иначе остаток
  восстанавливается по `signup-bonus:*` и immutable balance gaps; exact same-subject/full-amount
  `bonus-revoke:*` удаляет entitlement и делает весь current aggregate `paid`, а partial/mismatched/
  duplicate/mixed evidence блокируется. Весь прочий residual — `paid`, включая обязательный zero
  paid anchor. Apply не двигает pricing release. Real-PG gate:
  `funding_normalization_v2::tests::postgres_online_funding_normalization_v2_matrix`.
- **Stage 9 runtime-claim fence:** migration `0025` expand-only добавляет nullable
  `engine_instances.pricing_release_claim_epoch`. Пока global release head отсутствует, старый
  runtime с nullable v2 claim остаётся совместимым. После первого head любой insert, heartbeat или
  owner takeover обязан нести release/funding schema v2, непустой runtime digest и claim epoch,
  равный текущему owner epoch. Это не даёт старому binary унаследовать v2 identity предыдущего
  процесса через `ON CONFLICT`. Dependent claim writer доставляется только после GREEN migration
  SHA.
- **Stage 9 zero-drain/provisioning schema checkpoint:** migration `0026` ослабляет только
  DB-ограничение Stage 8 evidence: `legacy_inflight_count` остаётся обязательным audit count, а
  engine report теперь выдаёт `passed=true` при нуле реальных blockers, пока запросы старого
  формата штатно завершаются по своим immutable snapshots. Та же migration создаёт пустую
  append-only `pricing_release_assignment_extensions_v2` для аккаунтов, появившихся после cutover.
  Каждая extension привязана к exact текущему head activation и атомарной active/recovery pair;
  manifest assignments не мутируются. Dependent PostgreSQL producer теперь под общим pricing
  control lock валидирует exact head/recovery link, отсутствие base assignment и policy; balance
  assignment дополнительно берёт account funding lock и требует exact active funding head. Writer
  атомарно пишет пару, возвращает `unchanged` на exact replay и typed
  `stale|version_conflict` без частичной записи. Exact readback ключуется
  `(provisioning_head_version, account_id)`, а runtime resolver читает base либо extension в одном
  snapshot. SQLite остаётся unavailable; route не создаёт и не двигает head. Real-PG coverage —
  `pg::tests::pricing_release_runtime_v2_postgres_matrix`.
- **Pricing release v2 producer checkpoint:** `pricing::release_v2` и PostgreSQL persistence
  добавляют append-only policy/release/recovery prepare и read-only inventory/head. Release
  prepare проверяет exact full-account coverage (`active` + `disabled`) и готовые funding
  dependencies. Disabled account намеренно остаётся в immutable release, чтобы последующее
  включение не создавало дыру в policy/funding authority. SQLite возвращает unavailable вместо
  локальной authority.
- **Stage 9 activation producer:** `postgres_activate_pricing_release_v2` — единственный writer
  global head. В одной `SERIALIZABLE` transaction под `PRICING_RELEASE_CONTROL_LOCK_V2` он требует
  fresh combined evidence, exact absent/target CAS, immutable target/recovery link, current
  catalog/switch lineage, full base inventory (или exact paired extensions при recovery), funding
  parity и compile-fixed runtime floor с exact owner-epoch claims. Evidence, activation audit и одна
  head row commit'ятся вместе; rejection целиком rollback'ится. Exact audit replay возвращает
  `unchanged`, recovery идёт только вперёд из target. Accounts, balances, lots, reservations,
  ledger/usage не пишутся. Real-PG coverage —
  `pg::tests::postgres_stage8_engine_evidence_contract`.
- **Pricing release v2 runtime foundation:** PostgreSQL resolver читает head, assignment, policy,
  catalog/switch gates и rule precedence `model → provider → global` одним snapshot; service
  `meter_only` обходит product catalog, но сохраняет provider master-switch. Reserve повторно
  разрешает exact head под `request → funding-account → owner` locks, атомарно пишет reservation,
  immutable pricing snapshot и bonus-first pricing funding allocations. После появления head новые
  legacy-format reserves fail closed, но exact старые request IDs replay'ятся своим прежним writer.
  Outbox/settlement выбирает только один funding format по snapshot: release-v2 не требует
  pre-cutover funding snapshot и пишет exact paid/bonus ledger allocations. Незавершённый release
  settlement требует, чтобы pinned generation всё ещё была active; после monotonic advance
  разрешён только exact terminal replay без повторной money mutation. Provider adapter передаёт уже
  рассчитанный customer debit; registry проверяет provider, non-negative usage и потолок
  `hold+$1`, но не пересчитывает debit из full official usage (Codex может честно ограничить billed
  output). Runtime сам не вызывает activation producer: до отдельного защищённого commerce consumer
  head остаётся absent.

**Инварианты:**
- Токен разрешается из колонки `token` (inline) ИЛИ файла `token_file`. `import_sqlite` refuses a
  cutover while anonymous aggregate reservations remain and reconciles account totals before commit.
- Токены/прокси — секреты: не логировать, `list()` отдаёт лишь флаг наличия токена.
- Тариф (`plan`: pro|max5|max20) ХРАНИМ здесь (`set_plan`, колонка `plan`), но НЕ детектим —
  детект сетевой, живёт в `forward::detect_plan`, вызывается из `server`. `get_creds` отдаёт
  (token, proxy) для этого детекта.

**Проверка:** `cargo test -p registry`; real PostgreSQL matrices use
`CLAUDE_API_TEST_DATABASE_URL=... cargo test -p registry pg::tests::stage2_fault_matrix` and
`CLAUDE_API_TEST_DATABASE_URL=... cargo test -p registry pricing::postgres::tests::postgres_pricing_contract_matrix`.
