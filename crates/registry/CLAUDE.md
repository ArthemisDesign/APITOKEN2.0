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
  `account_set_mult_bp`.
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
  `capacity_leases` validate cooldown/utilization/inflight and increment inflight in one transaction.
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
  (`anthropic|openai`), requested/canonical model, alias/tariff identities, timestamps, scalar,
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
  обслуживают Anthropic/OpenAI; только snapshot-bearing success может передать работу bounded
  shadow producer. Production config остаётся выключенным.
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
- **Stage 8 engine evidence:** PostgreSQL-only read report материализуется в одной
  `REPEATABLE READ READ ONLY` транзакции. Он проверяет active main/openkeys graph, runtime
  capability, classifications/funding parity, frozen policy lineage, полное actual→shadow покрытие,
  exact integer nanoUSD и canonical typed sample; внешний Gemini admission aggregate сверяется с
  durable provider=`google` usage/outbox. Subject identities выходят только как SHA-256 digests.
  Report ничего не активирует и не исправляет; любой blocker должен остановить Stage 9.

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
