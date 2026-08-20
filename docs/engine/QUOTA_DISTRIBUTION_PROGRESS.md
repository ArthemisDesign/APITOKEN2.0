# Ход работ: внедрение QUOTA_DISTRIBUTION_ANALYSIS (R2, R7, R1 v1 + измерение шаг 0)

> Рабочий журнал задачи. Ветка: `feat/quota-5h7h-split-impl`, worktree `~/wt/quota-5h7h-split-impl`.
> Исходный документ-план: `docs/engine/QUOTA_DISTRIBUTION_ANALYSIS.md` (принятый план, 2026-08-20).
> Порядок — строго по §6 плана: шаг 0 (наблюдаемость, без изменения селекторов) → R2 → R7 (Kimi+GLM парой) → R1 v1 (Codex).
> Каждая правка — отдельный коммит с тестами рядом с существующими. Кэш-экономика (пин Claude,
> sticky Kimi/GLM, preferred Codex/Gemini) не трогается ни одной правкой.

## 2026-08-27 — старт

- Создан worktree через `deploy/agent-worktree.sh create feat/quota-5h7h-split-impl` от свежего
  `origin/master` (e405bc78). Имя `docs/quota-distribution-analysis` было занято удалённой веткой
  прошлого анализа — выбрано уникальное.
- Прочитаны: сам план, `AGENTS.md`, `CLAUDE.md`, `crates/pool/CLAUDE.md`, `crates/forward/CLAUDE.md`.
- Изучен текущий код всех пяти плоскостей (состояние до правок зафиксировано ниже, чтобы дальше
  журнал читался без перечитывания кода).

### Зафиксированное состояние кода (база для правок)

**Claude `crates/pool/src/lib.rs`:**
- `placement_free` (строка ~684): `min(cap5·(1−util5), cap7·(1−util7))` — абсолютный USD, без
  штрафа за util7d. Используется в `place_best` и в тёплом выборе cache-root
  (`peek_affinity_home_with_warm` ~строки 959–994).
- `select_best_with_policy`: 7d сравнивается раньше 5h (инвариант, покрыт `conserves_weekly_budget`
  — только для `pick`-пути).
- Счётчики `route_pin/route_spill/route_place/route_rebind` уже есть (`RouteStats`) — это и есть
  метрики pin/spill/rebind для контроля «кэш не изменился» из §6.

**Kimi `crates/forward/src/kimi/`:** `Candidate` несёт только свёртку `used_fraction_units`
(max по окнам) + `quota_age_secs`; оба окна целиком лежат в `ProfileHealth.quota_windows`
(`KimiQuotaWindowStatus`: duration_secs, used/limit, used_fraction_units, resets_at, observed_at),
но в `RuntimeProfile::candidate()` теряются. Селектор `select`: freshness → inflight →
steering(≥50% от свёртки) + курсор. Sticky выигрывает безоговорочно, пока элигибелен.
Софт-резерва нет.

**GLM `crates/forward/src/glm/`:** зеркало Kimi (ранг дословно тот же), отличия: таксономия
`Ineligible` (AccountDead/AccountSuspect/ModelIneligible/TransportWedged/QuotaWall), поля окон
`Option`al (`GlmQuotaWindowStatus`), нет environmental-escape в селекторе.

**Codex `crates/forward/src/codex/mod.rs`:**
- `WindowReserve { base5h, base7d, jitter }`, `caps(home_id)` — джиттерованные потолки,
  `window_cap` — выбор потолка по длительности окна (>24h → 7d-резерв), `reserve_blocked` —
  чистая функция, возвращает earliest reset среди окон за порогом.
- `CodexRateLimitWindow` несёт `used_fraction_units`, `resets_at`, `window_duration_mins` — всё
  нужное для pace-aware правки уже есть.
- Пиковый escape-hatch до `WindowReserve::FULL` уже есть в `select_home`.

**Server `crates/server/src/poller.rs` + `metrics_store.rs`:**
- `metrics_loop` раз в 60с пишет `snapshots` (агрегат `/overview`) и `sub_snapshots`
  (per-sub Claude: cap5h/cap7d/util5h/util7d). Это готовый фундамент шага 0 для Claude.
- Статусы других плоскостей доступны: `codex.operational_status().await` (per-home
  `rate_limits: Option<CodexRateLimits>`), `kimi.operational_status()` (per-profile
  `quota_windows: Vec<KimiQuotaWindowStatus>`), `glm.operational_status()` (аналогично),
  `gemini.operational_status().await` (per-profile `quotas: Vec<GeminiQuotaBucketStatus>`).
- AppState имеет `app.codex`, `app.kimi`, `app.glm`, `app.gemini` как `Option<Arc<…>>`.

### План шага 0 (уточнён по факту чтения кода)

- Новая таблица `provider_sub_snapshots(plane, sub_id, ts, used5h REAL, used7d REAL,
  reset5h_in INTEGER, reset7d_in INTEGER)` в metrics.db (отдельная от money-БД, локальная;
  id профилей — opaque roster id, документированно безопасны для метрик).
- Запись в `metrics_loop` из уже читаемых operational-статусов: Codex (primary/secondary),
  Kimi/GLM (5h-окно ~18 000с и weekly ~604 800с из `quota_windows`, по близости duration),
  Gemini (min `remaining_fraction` по бакетам → max used; reset из строки не парсим — NULL).
- Claude route_stats (pin/spill/place/rebind) — уже публикуются через `RouteStats`; оставляем
  как есть, ссылка в плане.
- Синхронность weekly-reset и «5h пустой при 7d на резерве» вычисляются SQL-запросами поверх
  новой таблицы — рунбук добавим в docs в коммите шага 0.

(дальше журнал пополняется по мере коммитов)

## Шаг 0 — наблюдаемость распределения квот (без правок селекторов)

**Коммит:** `feat(server): per-профильные снапшоты квот Codex/Kimi/GLM/Gemini в metrics.db`.

Что сделано:

- `metrics_store`: новая таблица `provider_sub_snapshots(plane, sub_id, ts, used5h, used7d,
  reset5h_in, reset7d_in)` (expand-only `CREATE TABLE IF NOT EXISTS`, prune вместе с остальными
  рядами по retention) + `ProviderQuotaPoint`/`insert_provider_sub_snapshots`.
- Новый модуль `server/src/provider_quota_points.rs` — чистые функции сбора из уже кэшированных
  `operational_status()` плоскостей (ни одного сетевого вызова):
  - **Codex**: primary → used5h, secondary → used7d, resets_at обоих окон → секунды до reset;
  - **Kimi/GLM**: окна разделяются по ближайшей длительности к опорным константам
    (`KIMI_ROLLING_WINDOW_SECS=18 000` / `KIMI_WEEKLY_WINDOW_SECS=604 800`, GLM-аналоги) с
    допуском ±10%; у GLM доля окна `Option`al — нет доли, есть только факт наблюдения;
  - **Gemini**: `used5h = max(1 − remaining_fraction)` по бакетам (худший остаток), `used7d`/reset
    остаются NULL — per-модельный каталог Google не сводится к одной паре окон, ничего не выдумываем.
- `poller::metrics_loop` пишет точки каждые 60с вместе с существующими снапшотами.

Что это даёт из §6 шага 0 (и как читать поверх metrics.db, все запросы — SQLite):
- гистограммы распределения: `SELECT plane, ROUND(used7d,1), COUNT(*) FROM
  provider_sub_snapshots WHERE ts>=:t GROUP BY 1,2` (аналогично used5h; Claude — из `sub_snapshots`);
- синхронность weekly-reset по флоту: разброс `reset7d_in` в одном ts;
- доля «5h пустой при 7d на резерве» (теряемая ёмкость): `used5h < 0.5 AND used7d > 0.90`
  (0.90/0.97 — типичные джиттерованные потолки Codex 10%/3%);
- доли pin/spill/rebind и cache-hit — уже публикуются: `Pool::route_stats()` (RouteStats) → `/metrics`;
  частота провайдерских 429 — существующие счётчики `upstream_429` в `forward::Metrics`.
Проверки: `cargo build -p claude-api`, `cargo test -p claude-api provider_quota_points` (2 теста:
раздельные окна + допуск длительности/кламп прошедшего reset), `cargo test -p claude-api
metrics_store` (6 существующих тестов зелёные).

## R2 — Claude: 7d учитывается в placement новых сессий

**Коммит:** `fix(pool): нормированный placement-скоринг новых сессий с учётом 7d (R2)`.

Проблема П2 подтверждена кодом: `placement_free = min(free5h, free7d)` в абсолютных USD
максимизировал свежий 5h даже при выжженной неделе — новые сессии концентрировались на
«5h-свежих, 7d-выжженных» домах и дожигали их же неделю. `conserves_weekly_budget` покрывал
только `pick`/`select_best`, где 7d и так решает первым ключом.

Реализация (ровно по плану §4 R2):
- `placement_free` заменён на `placement_score = min(free5/cap5, free7/cap7) − λ·util7`,
  λ = 0.5 (середина принятого диапазона 0.5–1.0). Нормировка на cap обязательна — абсолютный
  штраф λ·cap7 ~$750–1500 забил бы free5 ~$50.
- Применён в обоих местах, где работал `placement_free`: `place_best` и
  `peek_affinity_home_with_warm` (выбор тёплого cache-root) — иначе cache-root продолжал бы
  налипать на 7d-выжженные дома.
- Пин/спилл продолжений не затронуты: `route_affinity` для продолжений по-прежнему использует
  жёсткий 100% потолок, без скоринга.

Тесты рядом с `conserves_weekly_budget` (crates/pool/src/lib.rs):
- `placement_conserves_weekly_budget_for_new_sessions` — 5h-свежий/7d-выжженный дом проигрывает
  новое размещение;
- `placement_still_respects_the_tight_5h_window` — 5h-дефицит решает через min;
- `placement_score_is_normalized_across_plans` — равные доли при разных планах (Max20 vs Pro) дают
  равный скоринг (старый абсолютный вариант всегда выигрывал у Max20).

Обновлён `crates/pool/CLAUDE.md` (живой контракт: описание place_best и placement_score).
Проверки: `cargo build -p pool`, `cargo test -p pool` — 59 тестов зелёные (включая 3 новых).
