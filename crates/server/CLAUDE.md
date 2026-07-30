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
- `config.rs` — `Settings` (db_path/bind/fleet/Redis affinity + `ProxyConfig`) из env.
- `http.rs` — роутер: `/health`, `/pool`, `/balance`, `/capacity` (управляющие) + startup-fixed
  Claude/OpenAI/Gemini router. Production provider выбирает systemd unit, не request; Caddy marker остаётся
  только в одноразовом `Combined` migration bridge и никогда не принимается от клиента. + `/admin-panel`
  (единый `admin-panel.html` для admin.apitoken.sale; архитектура — корневой `PANEL.md`) +
  `/admin/*` (control-плоскость,
  см. `admin.rs`) + fallback на `forward::forward`. Выпуск ключа возвращает не-секретный `key_id`,
  а `/admin/key-id/{key_id}/status` позволяет отзывать ключ без повторной передачи полного секрета.
- `admin.rs` — **Control API** (`/admin/account`, `/admin/key`, `/admin/*/credit|status`): контракт,
  которым БУДУЩАЯ КОММЕРЦИЯ (отдельный сервис) управляет движком. Гейт — `forward::control_authed`
  (control-ключ, ОТДЕЛЬНО от forwarding-admin). Все записи — через single-writer актор `AsyncBilling`
  (та же дисциплина, что reserve/settle). Движок остаётся авторитетом ЖИВОГО баланса; коммерция лишь
  создаёт аккаунты/ключи и кредитует (идемпотентно по `ref`). Полный контракт — `CONTROL_API.md`.
  Account pricing is updated by `/admin/account/{id}/pricing`; cursor ledger reads use `after_id` for
  the commercial pricing worker.
  Key issue/list also carries optional `spend_limit_nano`/`expires_ts` policy metadata. The
  account-scoped `/admin/account/{id}/key-id/{key_id}/policy` endpoint replaces both nullable
  guardrails; validation is at this HTTP boundary while enforcement remains in registry reservation
  transactions.
- `poller.rs` — СОБЫТИЙНЫЕ циклы: `reload_loop` (перечитать реестр; будит поллер `Notify` при
  изменении флота) + `poll_loop` (liveness-only probe созревших подписок конкурентно, затем сон
  РОВНО до ближайшего due-времени или до `poke`). Фиксированного тика нет: reset вычисляется
  локально, а не поллится; под боевым трафиком `polled_ts` свеж (пассивный сбор в forward) → активный
  probe не срабатывает. `LIVENESS_INTERVAL` — как редко пинговать простаивающую (проверка живости токена).
  `persist_loop` — write-through персист состояния пула по событию cooling (`pool.on_change` → `Notify`)
  + редкий safety-flush; на старте `serve` восстанавливает состояние через `pool.import_state`.
  Если import карантинил implausible legacy calibration, `serve` сразу будит persist-loop, чтобы
  repaired prior-fallback не остался лишь in-memory до safety-flush.
  `poll_loop` также ведёт **durable auth-health**: probe кормит `pool.record_probe` (машина dead-детекта),
  изменившийся вердикт персистится owner-fenced (`save_sub_health`); suspect/dead probe-ятся НЕЗАВИСИМО
  от cooling (`SUSPECT_INTERVAL`/`DEAD_RESURRECT_INTERVAL`), чтобы добрать корроборацию/ресуррекцию. На
  старте `serve` сеет вердикт через `pool.import_health` (мёртвые сразу вне ротации, переживают рестарт).
- `main.rs` — clap CLI: `serve` и `sub add/add-file/list/rm/status/proxy/fleet/set-plan/detect-plan/health`.

**Инварианты:**
- Новую env-переменную заводи ТОЛЬКО тут и прокидывай дальше через конфиг-структуры.
- Redis здесь только конфигурируется; `AffinityStore` живёт в `forward`, а pool остаётся без сети.
- Управляющие эндпоинты (`/health`, `/pool`, `/capacity`, `/gemini-subs`, `/admin/*`) — здесь;
  остальное → форвардинг. `/gemini-subs` существует только в fixed Gemini runtime, гейтится
  `readonly_authed` и сериализует opaque ids/quota/cooling плюс отдельные gaxios и Undici transport
  attestations без Google identity, project/proxy/OAuth.
- **Три класса ключей (разделение секретов):** `CLAUDE_API_KEYS` (forwarding-admin: неметеренный /v1
  + всё), `CLAUDE_API_CONTROL_KEY` (control-плоскость `/admin/*`: аккаунты/деньги, для коммерции),
  `CLAUDE_API_PANEL_KEY` (read-only дашборды `/capacity`,`/metrics`). Гейты: `authed` (admin) ⊂
  `control_authed` (admin|control) ⊂ `readonly_authed` (admin|control|panel).
- `/health` без авторизации (голый liveness); `/pool` — `authed`; `/capacity`,`/metrics` —
  `readonly_authed`; `/admin/*` — `control_authed`.
- Fixed OpenAI `/ready` дополнительно проверяет provider snapshot: legacy owned-child требует один
  live+authenticated home, shared-daemon — минимум два; деградация ниже порога снимает слот из Caddy.
- `/metrics` публикует privacy-safe affinity counters, включая soft cache-root hits/writes; raw client
  IDs, prompt content, account IDs и subscription IDs в Redis/метрики не попадают.
- **loopback-доверие — только явный opt-in** `CLAUDE_API_TRUST_LOOPBACK=1` + реальный loopback-bind
  (иначе за реверс-прокси аноним получил бы админ-доступ).
- Shutdown OpenAI сначала ждёт detached Codex stream/history/settlement tasks, затем reaps children;
  только после этого billing FIFO-flush может завершить процесс и отпустить общий home lock.
- Shutdown Gemini сначала закрывает admission и ждёт detached SSE drain; на deadline abort-сигнал
  прерывает upstream read, task settle-ит последний usage snapshot и пересекает semaphore barrier.
  Billing FIFO-flush разрешён только после этого. Gemini health/preflight/network живут в `forward`,
  а env/upstream pin и startup-fixed service composition — только здесь. Production unit обязан
  argv-level pin-ить CLI version + Node binary/version/SHA после shared EnvironmentFile.

**Проверка:** `cargo build -p claude-api`; `cargo run -p claude-api -- serve`.
