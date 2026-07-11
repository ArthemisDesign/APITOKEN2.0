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
  (живой HTML-дашборд ёмкости, `panel.html` через include_str!) + fallback на `forward::forward`.
- `poller.rs` — `reload_loop` (перечитать реестр), `poll_loop` (опрос лимитов через `forward::poll_sub`).
- `main.rs` — clap CLI: `serve` и `sub add/add-file/list/rm/status/proxy/fleet`.

**Инварианты:**
- Новую env-переменную заводи ТОЛЬКО тут и прокидывай дальше через конфиг-структуры.
- Управляющие эндпоинты (`/health`, `/pool`) — здесь; всё остальное уходит в форвардинг.
- `/health` без авторизации; `/pool` — через `forward::authed`.

**Проверка:** `cargo build -p claude-api`; `cargo run -p claude-api -- serve`.
