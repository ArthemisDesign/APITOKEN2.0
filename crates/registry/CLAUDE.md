# crates/registry — CLAUDE.md

**Роль:** engine-owned PostgreSQL authority. SQLite is a one-time migration source and emergency audit snapshot.

**Владелец-ветка:** `comp/registry`.

**Границы (жёстко):**
- Зависит только от sync PostgreSQL client, `rusqlite` (import/fallback), `anyhow`.
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
- **Stage 3B0/3B1b snapshot read — dormant:** `pricing_read_bundle(account_id)` за одну read-only
  транзакцию возвращает live `accounts.mult_bp`, binding/active policy, exact immutable
  `policy_catalog/policy_switches` и текущие `admission_catalog/admission_switches`: SQLite через
  deferred snapshot, PostgreSQL через `REPEATABLE READ READ ONLY`. Active policy обязана найти обе
  pinned dependencies или read падает как integrity error; inactive binding получает только
  admission heads, unbound account — ни одной пары. Scalar входит в тот же snapshot, иначе legacy
  OpenKeys validation гоняется с multiplier writer. Registry только материализует данные;
  независимые policy/admission gates выполняет чистый resolver в `forward`. Нельзя собирать bundle
  последовательностью отдельных `active_*`/`*_by_generation` reads — это смешивает поколения.
  Runtime callers, billing actor command, telemetry, readiness и writes пока запрещены.
- **Stage 3B1a shadow schema — dormant:** PostgreSQL migration `0009` и SQLite parity создают
  отдельную immutable `pricing_shadow_admission_evaluations`. Она не подменяет actual
  `pricing_admission_snapshots`: shadow-строка ссылается на уже зафиксированный actual snapshot и
  хранит обе lineage-пары (`policy_*` и `admission_*`), runtime manifest, scalar comparison и typed
  outcome. Dependency capability pins exact-связаны с immutable catalog/switch versions; runtime
  manifest обозначает поддерживаемый набор, поэтому будущий insert API обязан проверить membership
  всех pins до записи. Миграция не устанавливает writer/caller, heads, policies или данные. Будущий
  insert API обязан быть exact-idempotent по `request_id + evaluation_digest`; отличный digest —
  conflict, а не update. Live shadow запрещён до отдельного bounded worker и атомарного actual
  snapshot + reserve.

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
