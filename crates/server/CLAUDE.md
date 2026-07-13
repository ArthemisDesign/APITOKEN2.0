# crates/server — CLAUDE.md

**Роль:** КОМПОЗИЦИЯ (bin `claude-api`). Читает env, поднимает пул из реестра, стартует фоновые
циклы и HTTP-роутер. Здесь — и только здесь — всё связывается вместе.

**Владелец-ветка:** `comp/server`.

**Границы (жёстко):**
- Зависит от `forward`, `pool`, `registry`, `tokio`, `axum`, `clap`.
- **ЕДИНСТВЕННОЕ место, где читается окружение** — `src/config.rs` (`Settings::from_env`).
  Ниже по стеку env не трогают.
- Не содержит бизнес-логики форвардинга (она в `forward`) и логики выбора (она в `pool`).
  Здесь — проводка: env → `ProxyConfig`/`Pool`/`Clients` → `AppState` → роутер + циклы.

**Что внутри:**
- `config.rs` — `Settings` (db_path/bind/fleet + `ProxyConfig`) из env.
- `http.rs` — роутер: `/health`, `/pool`, `/balance`, `/capacity` (управляющие) + `/panel`
  (живой HTML-дашборд ёмкости, `panel.html` через include_str!) + `/admin/*` (control-плоскость,
  см. `admin.rs`) + fallback на `forward::forward`. Выпуск ключа возвращает не-секретный `key_id`,
  а `/admin/key-id/{key_id}/status` позволяет отзывать ключ без повторной передачи полного секрета.
- `admin.rs` — **Control API** (`/admin/account`, `/admin/key`, `/admin/*/credit|status`): контракт,
  которым БУДУЩАЯ КОММЕРЦИЯ (отдельный сервис) управляет движком. Гейт — `forward::control_authed`
  (control-ключ, ОТДЕЛЬНО от forwarding-admin). Все записи — через single-writer актор `AsyncBilling`
  (та же дисциплина, что reserve/settle). Движок остаётся авторитетом ЖИВОГО баланса; коммерция лишь
  создаёт аккаунты/ключи и кредитует (идемпотентно по `ref`). Полный контракт — `CONTROL_API.md`.
- `poller.rs` — СОБЫТИЙНЫЕ циклы: `reload_loop` (перечитать реестр; будит поллер `Notify` при
  изменении флота) + `poll_loop` (liveness-only probe созревших подписок конкурентно, затем сон
  РОВНО до ближайшего due-времени или до `poke`). Фиксированного тика нет: reset вычисляется
  локально, а не поллится; под боевым трафиком `polled_ts` свеж (пассивный сбор в forward) → активный
  probe не срабатывает. `LIVENESS_INTERVAL` — как редко пинговать простаивающую (проверка живости токена).
  `persist_loop` — write-through персист состояния пула по событию cooling (`pool.on_change` → `Notify`)
  + редкий safety-flush; на старте `serve` восстанавливает состояние через `pool.import_state`.
- `main.rs` — clap CLI: `serve` и `sub add/add-file/list/rm/status/proxy/fleet`.

**Инварианты:**
- Новую env-переменную заводи ТОЛЬКО тут и прокидывай дальше через конфиг-структуры.
- Управляющие эндпоинты (`/health`, `/pool`, `/capacity`, `/admin/*`) — здесь; остальное → форвардинг.
- **Три класса ключей (разделение секретов):** `CLAUDE_API_KEYS` (forwarding-admin: неметеренный /v1
  + всё), `CLAUDE_API_CONTROL_KEY` (control-плоскость `/admin/*`: аккаунты/деньги, для коммерции),
  `CLAUDE_API_PANEL_KEY` (read-only дашборды `/capacity`,`/metrics`). Гейты: `authed` (admin) ⊂
  `control_authed` (admin|control) ⊂ `readonly_authed` (admin|control|panel).
- `/health` без авторизации (голый liveness); `/pool` — `authed`; `/capacity`,`/metrics` —
  `readonly_authed`; `/admin/*` — `control_authed`.
- **loopback-доверие — только явный opt-in** `CLAUDE_API_TRUST_LOOPBACK=1` + реальный loopback-bind
  (иначе за реверс-прокси аноним получил бы админ-доступ).

**Проверка:** `cargo build -p claude-api`; `cargo run -p claude-api -- serve`.
