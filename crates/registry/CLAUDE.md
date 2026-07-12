# crates/registry — CLAUDE.md

**Роль:** реестр подписок (пункт 1). Хранение/чтение таблицы `subs` в SQLite. Источник истины пула.

**Владелец-ветка:** `comp/registry`.

**Границы (жёстко):**
- Зависит только от `rusqlite`, `anyhow`. Больше ни от чего.
- НИКАКОЙ сети, HTTP, чтения env, логики выбора подписок. Только персист + CRUD + `load_active`.
- **Биллинг ключей (таблица `api_keys`)** тоже здесь — но ТОЛЬКО хранение баланса в целых
  нанодолларах: `key_issue/topup/deduct/get/list/set_status` + обёртка `Billing` (Mutex<Conn>
  для запросного пути). Подсчёт стоимости (токены→нано) сюда НЕ лезет — это `metering`; registry
  принимает готовую сумму списания. `deduct` атомарен (balance−charge, spent+charge одной командой).
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
