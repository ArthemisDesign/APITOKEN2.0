# crates/registry — CLAUDE.md

**Роль:** реестр подписок (пункт 1). Хранение/чтение таблицы `subs` в SQLite. Источник истины пула.

**Владелец-ветка:** `comp/registry`.

**Границы (жёстко):**
- Зависит только от `rusqlite`, `anyhow`. Больше ни от чего.
- НИКАКОЙ сети, HTTP, чтения env, логики выбора подписок. Только персист + CRUD + `load_active`.
- **Биллинг: АККАУНТЫ клиентов (`accounts`) + ключи-доступы (`api_keys`) + журнал (`ledger`)** — здесь,
  но ТОЛЬКО хранение/атомарные движения в целых нанодолларах. Модель: **баланс на АККАУНТЕ** (профиль
  юзера), ключи (`api_keys.account_id`) — доступы к общему балансу (1:N, на проекты/команду); per-key
  `spent_nano` — атрибуция расхода по ключу. Функции: `account_create/get/by_handle/list/set_status/rm`,
  `account_topup` (+ledger), `account_reserve`/`account_settle` (атомарно: баланс аккаунта + per-key
  spent + ledger-строка), `key_issue(account_id,label)/get/list/set_status/set_status_by_id/remove/clear`;
  `api_keys.key_id` — стабильный не-секретный control-plane ID для отзыва без хранения полного ключа,
  `key_account` (JOIN ключ→аккаунт для авторизации), обёртка `Billing` (Mutex<Conn>). Подсчёт стоимости
  (токены→нано) сюда НЕ лезет — это `metering`; registry принимает готовую сумму. **Инвариант денег:**
  `charge≤hold≤balance` держится на уровне АККАУНТА (reserve атомарен `WHERE balance>=hold`, settle
  сводит пару к −actual). `ledger` — append-only история (topup/charge/adjust, ref=request-id).
  Cursor consumers use `ledger_after(account, after_id, limit)` (oldest-first); account pricing uses
  `account_set_mult_bp`.
  Мягкая миграция старой модели «key=кошелёк» → аккаунт per-key (`migrate_legacy_keys`).
- Публичный тип [`Sub`] (email/token/proxy/fleet) — контракт для `pool`/`forward`. Меняешь его —
  проверь оба потребителя.
- **Персист состояния пула (таблица `pool_state`)** — тоже здесь: `PoolStateRow` (примитивы, registry
  не знает типов `pool`) + `save_pool_state`/`load_pool_state`. Хранит durable-состояние (cooling/
  калибровка/spent/util/reset) для переживания рестарта. Логику решает `pool` (export/import), registry
  лишь пишет/читает готовые строки.

**Инварианты:**
- Токен разрешается из колонки `token` (inline) ИЛИ файла `token_file`. Совместимость с
  исторической `subscriptions.db` держим мягкой миграцией колонок (`ALTER TABLE … ADD COLUMN`).
- Токены/прокси — секреты: не логировать, `list()` отдаёт лишь флаг наличия токена.
- Тариф (`plan`: pro|max5|max20) ХРАНИМ здесь (`set_plan`, колонка `plan`), но НЕ детектим —
  детект сетевой, живёт в `forward::detect_plan`, вызывается из `server`. `get_creds` отдаёт
  (token, proxy) для этого детекта.

**Проверка:** `cargo build -p registry`. Ручной прогон CRUD — через бинарь: `claude-api sub add/list/...`.
