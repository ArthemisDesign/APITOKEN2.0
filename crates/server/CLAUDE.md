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
  только в одноразовом `Combined` migration bridge и никогда не принимается от клиента. +
  data routes для admin.apitoken.sale (`/overview`, `/capacity`, `/subs` и др.; UI — standalone
  Next.js `apps/admin`, архитектура — корневой `docs/product/PANEL.md`) +
  `/admin/*` (control-плоскость,
  см. `admin.rs`) + fallback на `forward::forward`. Выпуск ключа возвращает не-секретный `key_id`,
  а `/admin/key-id/{key_id}/status` позволяет отзывать ключ без повторной передачи полного секрета.
  `/metrics` экспортирует registry incident-tripwire
  `claude_api_execution_group_double_winner_total`; метрика должна оставаться нулевой, а
  transactional winner correctness не зависит от процесса или Prometheus.
- `admin.rs` — **Control API** (`/admin/account`, `/admin/key`, `/admin/*/credit|status`): контракт,
  которым БУДУЩАЯ КОММЕРЦИЯ (отдельный сервис) управляет движком. Гейт — `forward::control_authed`
  (control-ключ, ОТДЕЛЬНО от forwarding-admin). Все записи — через single-writer актор `AsyncBilling`
  (та же дисциплина, что reserve/settle). Движок остаётся авторитетом ЖИВОГО баланса; коммерция лишь
  создаёт аккаунты/ключи и кредитует (идемпотентно по `ref`). Полный контракт — `docs/engine/CONTROL_API.md`.
  Account pricing is updated by `/admin/account/{id}/pricing`; cursor ledger reads use `after_id` for
  the commercial pricing worker. Account reads include the coherent paid/bonus/other/unattributed
  funding summary. Ledger rows add stored immutable attribution and normalized funding allocations;
  old rows remain null/empty and are never reclassified at the HTTP boundary.
  Stage 3C adds authenticated `/admin/pricing/*` prepare/read/activate routes for immutable product
  catalogs, provider switches and account policies. Requests/ACKs preserve complete version/digest,
  capability lineage and binding identity; typed CAS errors stay distinguishable. Routes only
  expose the registry contract through `AsyncBilling` actors and cannot backfill, issue keys,
  enable strict enforcement or reorder catalog → switches → policy activation.
  Key issue/list also carries optional `spend_limit_nano`/`expires_ts` policy metadata. The
  account-scoped `/admin/account/{id}/key-id/{key_id}/policy` endpoint replaces both nullable
  guardrails; validation is at this HTTP boundary while enforcement remains in registry reservation
  transactions.
- `poller.rs` — СОБЫТИЙНЫЕ циклы: `reload_loop` (перечитать реестр; будит поллер `Notify` при
  изменении флота) + `poll_loop` (free count-tokens probe созревших подписок конкурентно, затем сон
  РОВНО до ближайшего due-времени или до `poke`). Фиксированного тика нет: reset вычисляется
  локально. Под боевым трафиком пассивные headers обычно держат `polled_ts` свежим, но завершение
  authoritative Claude turn после постановки exact spend в FIFO принудительно помечает подписку due
  и будит poller для post-turn quota pairing. `LIVENESS_INTERVAL` задаёт редкую фоновую проверку
  простаивающей, а per-sub 15-секундный debounce ограничивает forced probes. Если у Claude-sub
  отсутствует plan, тот же backend probe сначала читает официальный OAuth profile. Для inference-only
  токена с HTTP 403 разрешён только fail-closed fallback: все непустые планы активного fleet должны
  единогласно совпасть с одним `pro|max5|max20`. Результат durable сохраняется и сразу обновляет
  in-memory roster; mixed/unknown fleet остаётся без plan, UI в детекте не участвует.
  `persist_loop` — write-through персист состояния пула по событию cooling (`pool.on_change` → `Notify`)
  + редкий safety-flush; на старте `serve` восстанавливает состояние через `pool.import_state`.
  Если import карантинил implausible legacy calibration, `serve` сразу будит persist-loop, чтобы
  repaired prior-fallback не остался лишь in-memory до safety-flush.
  `poll_loop` также ведёт **durable auth-health**: probe кормит `pool.record_probe` (машина dead-детекта),
  изменившийся вердикт персистится owner-fenced (`save_sub_health`); suspect/dead probe-ятся НЕЗАВИСИМО
  от cooling (`SUSPECT_INTERVAL`/`DEAD_RESURRECT_INTERVAL`), чтобы добрать корроборацию/ресуррекцию. На
  старте `serve` сеет вердикт через `pool.import_health` (мёртвые сразу вне ротации, переживают рестарт).
- `main.rs` — clap CLI: `serve`, `sub add/add-file/list/rm/status/proxy/fleet/set-plan/detect-plan/health`
  и PostgreSQL-only read evidence `db stage8-evidence`.

**Инварианты:**
- При старте PostgreSQL authority только read-only проверяет применённую схему; DDL выполняется
  отдельным `db migrate-engine` до запуска слота blue-green.
- Новую env-переменную заводи ТОЛЬКО тут и прокидывай дальше через конфиг-структуры.
- Atomic legacy snapshot bridge config читается только здесь
  `CLAUDE_API_PRICING_BRIDGE_ENABLED`/`CLAUDE_API_PRICING_BRIDGE_SAMPLE_BP`. Default строго
  `false/0`; bool принимает только `0|1|false|true`, sample — integer `0..=10000`, несогласованные
  пары отклоняются. Enabled требует sample `1..=10000` и активирует только atomic actual-snapshot
  reserve caller Anthropic/OpenAI; policy shadow/resolver он не включает. Rollout выполняется
  отдельными наблюдаемыми config-ступенями, а не изменением безопасного default.
- Pricing shadow config читается только здесь под `CLAUDE_API_PRICING_SHADOW_*`. Default —
  disabled/0 bp; queue 256, workers 2, timeout 750ms, max age 300s, field 512 B, item 16 KiB,
  rate 20/s burst 40, PostgreSQL readers 2. Все значения strict-validated; max age всегда `<24h`.
  Enabled требует billing + PostgreSQL + fixed Anthropic/OpenAI plane. Server собирает fixed
  versioned runtime manifest, запускает отдельные read actors/worker и дренирует worker до billing
  FIFO flush.
- Тот же compile-fixed pricing manifest используется независимо от shadow enablement при claim
  PostgreSQL owner epoch, startup heartbeat и каждом регулярном heartbeat. Active strict
  dependencies, которых нет в manifest, не получают новый owner; drift после claim снимает
  readiness и fence-ит slot. Scalar-only `claim_instance` остаётся несовместимым со strict heads.
- `POST /admin/key` и reactivation через `/admin/key-id/{key_id}/status` принимают nested
  `activation_policy_ack {effective_policy_version, policy_digest}`. Для strict binding exact ACK
  обязателен; отсутствующий/stale/wrong identity даёт 409, malformed identity — 400. Disable не
  требует ACK. Секрет ключа по-прежнему выдаётся один раз и только после durable ACK check.
- `db stage8-evidence` получает тот же compile-fixed runtime manifest из `Settings`, требует явное
  frozen window/sample limits и внешний агрегат Gemini admissions, печатает read-only JSON и
  возвращает ошибку после печати при любом blocker. Команда не меняет heads, bindings или деньги.
- Redis здесь только конфигурируется; `AffinityStore` живёт в `forward`, а pool остаётся без сети.
- Управляющие эндпоинты (`/health`, `/pool`, `/capacity`, `/fleet-history`, `/settlement-health`,
  `/codex-subs`, `/gemini-subs`, `/admin/*`) — здесь; остальное → форвардинг. `/capacity`,
  `/codex-subs` и `/gemini-subs` сериализуют безопасный paid-plan identity для защищённого
  `admin.apitoken.sale/sales/calculator`; этот классификатор не является credential. `/codex-subs`
  гейтится `control_authed` и отдаёт только opaque home id плюс bounded email hint (первые четыре
  символа local-part без домена), никогда полный ChatGPT email/account id/OAuth/proxy. Окна явно
  публикуют provider measurement resolution, а `plan_cohorts` объединяет только exact paid plan +
  duration в общий native-credit capacity per home/fleet; per-home evidence и workload-dependent
  API USD не заменяются этим агрегатом. `/capacity` публикует Claude 5ч/7д и horizon money как
  decimal nanoUSD strings, per-sub remaining и authoritative conversion catalogue из `metering`:
  Standard для семи canonical-моделей, Fast только для фактически поддерживаемых Opus 5/4.8.
  Claude full-window capacity pool-ится только внутри exact plan+duration по формуле
  `10^8*Σspend/Σfraction`; другой routable plan без evidence, snapshot старше 900с или pending/
  degraded calibration delivery fail-closed для fleet remaining. Историческая capacity при этом
  не стирается. `calibration_delivery` раскрывает только bounded queue counts/integrity, без identity.
  `/capacity` также отдаёт newest-first `calibration_recent_turns` максимум из 512 immutable
  Anthropic events: opaque request ID, masked email и полный token/cost vector без prompt/credential.
  Это backend evidence операторского runner; aggregate `calibration_evidence` остаётся статистикой.
  `/overview` и новые metrics.db snapshots берут capacity-facing поля из того же exact report;
  `pool::Cap` prior/EMA остаётся routing-only. Overview добавляет canonical decimal `*_nano`, а его
  старые float USD поля остаются только display compatibility.
  `/fleet-history` читает историю metrics.db
  (snapshots/sub_snapshots за 24h/7d/30d/90d, бакетирование до ≤ ~500 точек, опциональный
  per-sub ряд по маске email) и гейтится `control_authed`, как `/overview` с денежными
  агрегатами. `/settlement-health` — денежная диагностика settlement pipeline: counts
  settlement_outbox по state (pending/done/failed; 'processing' в схеме есть, но не пишется),
  failed всего/24ч, backlog несеттленых старше 300с, ≤10 последних failed с last_error,
  урезанным до 200 символов (settle-ошибки — invariant/SQLSTATE детали, без секретов), и лаг
  pricing-консьюмера (max(ledger.id) vs ledger_consumer_checkpoints, возраст старейшей
  неподтверждённой строки); читается через registry (`PgStore::settlement_health` / SQLite-twin
  в registry::settlement_health), server в PG напрямую не лезет. `/spend-stats` принимает
  опциональные `from`/`to` (epoch-секунды, вместе): ответ дополняется блоком `custom` за
  полуоткрытый диапазон [from, to) шириной ≤ 92 дней (мусор/from ≥ to/будущее/шире лимита — 400,
  `to` зажимается до now+1); `custom` считается на каждый запрос мимо TTL-кэша, в котором лежат
  только стандартные окна d1/d7/d30. Range-агрегации — через registry `spend_by_*_range`. `/gemini-subs` существует
  только в fixed Gemini runtime, гейтится
  `readonly_authed` и сериализует opaque ids, bounded email hint, quota/cooling, per-model
  generation health и low-cardinality failure classes плюс отдельные gaxios и Undici transport attestations и
  Antigravity version без Google subject/full email/domain, project/proxy/OAuth. Ответ также
  публикует exact nanoUSD fleet totals, paid-tier conversion catalogue из `metering::gemini` и
  canonical-model → private quota-bucket mapping; отсутствующий provider amount остаётся `null`.
- **Три класса ключей (разделение секретов):** `CLAUDE_API_KEYS` (forwarding-admin: неметеренный /v1
  + всё), `CLAUDE_API_CONTROL_KEY` (control-плоскость `/admin/*`: аккаунты/деньги, для коммерции),
  `CLAUDE_API_PANEL_KEY` (read-only дашборды `/capacity`,`/metrics`). Гейты: `authed` (admin) ⊂
  `control_authed` (admin|control) ⊂ `readonly_authed` (admin|control|panel).
- `/health` без авторизации (голый liveness); `/pool` — `authed`; `/capacity`,`/metrics` —
  `readonly_authed`; `/fleet-history`, `/settlement-health` и `/admin/*` — `control_authed`.
- Fixed OpenAI `/ready` дополнительно проверяет provider snapshot: любой transport требует хотя бы
  один live+authenticated home. Одна рабочая подписка остаётся реальной ёмкостью и не превращается
  в 503 из-за размера пула; оба blue-green поколения читают один sealed roster, поэтому паритет
  authenticated-home set при cutover обеспечен конструкцией, без минимального soak-интервала.
- `/metrics` публикует privacy-safe affinity counters, включая soft cache-root hits/writes,
  fixed-cardinality strict admission/rejection counters для Anthropic/OpenAI/Gemini и fleet-only
  Anthropic exact-capacity/coverage/delivery gauges. Raw client IDs, prompt content, account IDs,
  model IDs и subscription IDs в Redis/метрики не попадают.
- Stage 9 runtime delivery не активирует production bindings и не применяет account assignments.
  Production Stage 5/6 data application, Stage 8 evidence и strict activation остаются
  заблокированы до reviewed B2B/service/OpenKeys assignment matrix.
- **loopback-доверие — только явный opt-in** `CLAUDE_API_TRUST_LOOPBACK=1` + реальный loopback-bind
  (иначе за реверс-прокси аноним получил бы админ-доступ).
- Shutdown OpenAI сначала ждёт detached Codex stream/history/settlement tasks (нативный провайдер
  не держит child-процессов — abort-сигнал рвёт upstream read на deadline); только после этого
  billing FIFO-flush может завершить процесс.
- Shutdown Gemini сначала закрывает admission и ждёт detached SSE drain; на deadline abort-сигнал
  прерывает upstream read, task settle-ит последний usage snapshot и пересекает semaphore barrier.
  Billing FIFO-flush разрешён только после этого. Gemini health/preflight/network живут в `forward`,
  а env/upstream pin и startup-fixed service composition — только здесь. Production unit обязан
  argv-level pin-ить Antigravity version + Cloud Code host + Node binary/version/SHA после shared
  EnvironmentFile.
- Shutdown Claude после stream drain вызывает общий billing FIFO barrier: pending calibration head
  повторяется до outbox reconcile; процесс не объявляет flush успешным, пока exact evidence остаётся
  неприменённым.

**Проверка:** `cargo build -p claude-api`; `cargo run -p claude-api -- serve`.
