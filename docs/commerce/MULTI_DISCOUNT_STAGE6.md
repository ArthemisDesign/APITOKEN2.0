# Stage 6 — online funding normalization

Статус: engine producer, strict TypeScript transport consumer и bounded commerce orchestration
реализованы поверх уже доставленной migration `0029_pricing_release_two_phase_finalize.sql`.
Orchestration связывается с exact Stage 5 plan, вычисляет funding manifest только из фактического
full-inventory readback и затем двухфазно финализирует target/recovery releases. Наличие кода само
по себе ничего не запускает: worker начинает POST только после явного idempotent staging. Stage 6
не требует maintenance window, остановки money writers, нуля всех reservations или ручной
проверки аккаунтов и ничего не активирует.

## Source policy

Funding normalization нужна для paid/bonus attribution и реферальной математики, а не для
ограничения доступных моделей.

- точный неотозванный `signup-bonus:<subject>` сохраняется как `welcome_bonus`;
- существующая выдача сохраняет фактический номинал `$4`;
- новые выдачи после изменения контракта имеют номинал `$5`;
- весь остальной существующий остаток по решению владельца классифицируется `paid`;
- paid lot материализуется и при нулевом residual: это immutable anchor для разрешённого `$1`
  overrun у bonus-only/zero-hold request;
- bonus разрешён для любой модели, доступной B2C policy;
- reserve расходует bonus-first, затем paid;
- referral commission получает только paid-funded settlement amount.

Режим `track`, eligibility `track` и bucket `welcome_track_bonus` не создаются новым кодом.
Immutable historical rows могут сохранять старые значения только как audit evidence.

## Подготовка writers

До backfill production runtime должен:

1. сохранять v2 pricing/funding snapshot в каждой новой reservation;
2. dual-write topup, bonus, reserve, cancel, settlement и refund в aggregate и funding lots одной
   account transaction;
3. брать тот же account row/advisory lock, что и backfill;
4. после ожидания lock повторно читать funding generation;
5. уметь завершать старую reservation по её immutable legacy snapshot.

Это приложение выкатывается blue-green при старом active release и само по себе не меняет цену.

### Pre-cutover writer checkpoint

PostgreSQL writer выбирает путь только после account-local serialization:

```text
reserve/settlement: request advisory lock → funding account advisory lock
                    → reread active funding head → row locks/money writes
topup/adjust:       funding account advisory lock → reread active funding head
                    → row locks/money writes
```

Отсутствующий head означает полностью legacy transaction. Существующий head означает обязательный
dual-write: account aggregate, active generation, lots и reservation snapshot/allocation либо
commit вместе, либо полностью rollback. Это же правило закрывает гонку с normalization: writer,
который ждал её lock, перечитывает уже новый head и не может продолжить как legacy writer.

Reserve сохраняет bonus-first allocation. Overdraft разрешён только в paid и не больше старого
account floor `$1`; normalized generation обязана содержать paid lot даже с нулевым residual, и
bonus-only или нулевой hold сохраняет его как zero allocation anchor, чтобы возможный settlement
overrun не был ошибочно отнесён к bonus. Cancel возвращает весь hold по сохранённым
allocations. Settlement превращает ровно эти allocations в charged/released, обновляет lots и
пишет charge attribution в `funding_ledger_allocations_v2`. Exact terminal replay ничего повторно
не списывает и проверяет исходную immutable generation даже после последующего monotonic head
advance.

`account_topup` классифицирует положительный `signup-bonus:*` как `welcome_bonus`; остальные
credits и negative adjustments — как `paid`. Exact idempotency replay возвращает первую ledger
строку до любой повторной lot mutation. Durable outbox recovery выполняет тот же settlement path.

Real PostgreSQL evidence —
`pg::tests::pre_cutover_funding_v2_writer_postgres_matrix`: bonus-first/replay/cancel/settlement,
paid overrun, top-up/bonus/adjust, recovery после enqueue, writer после normalization wait и
проверка, что settlement не захватывает reservation row до funding-account lock.

Пока global release head отсутствует, snapshot составной: существующий immutable
`pricing_admission_snapshots` закрепляет старую активную цену, а
`funding_reservation_snapshots_v2` вместе с `funding_reservation_allocations_v2` закрепляет exact
funding generation и bonus-first lots. Это необходимо, потому что полный prepared release сам
ссылается на уже нормализованные funding generations. После Stage 9 новые запросы атомарно пишут
release-связанные таблицы migration 0023; pre-cutover rows продолжают завершаться по своему
составному snapshot и не пересчитываются.

## Online plan/apply

Planner строит content-addressed plan по всему inventory. Ручной resolution/reviewer artifact не
используется. Для каждого аккаунта plan содержит source-state/ledger digests, точные target lots и
структурные blockers.

Engine producer предоставляет только account-local операции под control key:

```text
GET  /admin/pricing/v2/funding/{account_id}/normalization
POST /admin/pricing/v2/funding/{account_id}/normalization
     {expected_source_state_digest, expected_normalization_digest}
```

`GET` работает в `REPEATABLE READ READ ONLY` и возвращает `ready|blocked|normalized`, canonical
`sha256:v2` source/target identities, exact lots и typed blockers. `POST` выполняется в
`SERIALIZABLE`, сначала берёт тот же funding-account advisory lock, затем полностью перестраивает
plan. Ответ `stored|unchanged|stale|blocked|conflict` не допускает применения отредактированного или
устаревшего JSON. SQLite отвечает fail closed: live authority этого перехода только PostgreSQL.
`packages/contracts` валидирует полный strict wire shape и canonical digests, а единственные typed
вызовы находятся в `packages/engine-client`; transport consumer сам по себе не запускает backfill.

При наличии согласованных legacy `funding_buckets` exact historical `welcome_track_bonus`
переносится в provider-independent `welcome_bonus`, а все остальные buckets схлопываются в `paid`.
Если legacy buckets отсутствуют, planner восстанавливает welcome по immutable
`signup-bonus:*` top-up и `balance_after_nano`; удалённые retention-ом charge rows учитываются как
точные отрицательные gaps между сохранившимися money rows. Без welcome evidence весь aggregate
становится paid. В каждом варианте создаётся нулевой paid anchor.

Apply идёт bounded batches. Каждая account-local `SERIALIZABLE` transaction:

1. берёт account money lock;
2. перечитывает aggregate, ledger, reservations и existing lots;
3. проверяет expected source digest;
4. вычисляет точный неиспользованный welcome остаток;
5. относит residual balance/reserved/spent к paid;
6. проверяет суммы и overflow;
7. атомарно пишет lots и funding generation.

Другие аккаунты не блокируются. Запрос текущего аккаунта может кратко ждать его money lock, после
чего целиком выполняется по состоянию до или после normalization.

Apply resumable и idempotent: exact account replay не создаёт дубликаты. Stale account
перепланируется без отката уже завершённых аккаунтов. Глобальный partial-result допустим во время
backfill, потому что active release остаётся legacy; Stage 9 требует 100% readiness.

## Bounded orchestration

Двухфазный `stageFundingNormalizationJobV2` принимает только exact `plan_digest` полностью ACKed
Stage 5 run в состоянии `materializing`, у которого target/recovery skeletons ещё имеют nullable
final identities, и идемпотентно создаёт один
`pricing_release_control_jobs_v2` с `job_kind=normalize_funding`. Payload identity связывает
immutable Stage 5 run/plan, оба skeleton plan digest, engine/service inventories и funding-plan
digest, но не содержит ещё не существующий final funding manifest. `planned`, blocked/failed,
already-finalized или изменившийся Stage 5 run отклоняется до создания job. Replay уже
подтверждённого `prepared` run возвращает тот же job ID. Worker не ищет release для автоматического
запуска: отсутствие явно staged job означает отсутствие любых normalization POST.

На каждом resumable slice worker:

1. восстанавливает только истёкшие parent/account leases;
2. дважды исчерпывает engine cursor с bounded page size и отклоняет duplicate/regressing cursor;
3. сравнивает stable identity `(account_id,status,multiplier_bp)`. Live
   `balance/reserved/spent` и funding head намеренно не входят в coverage digest: money writers
   продолжают работать и сериализуются с apply account-local lock'ом;
4. отдельно сверяет canonical service inventory. Все service accounts исключаются из funding
   queue, потому что их release assignment — `meter_only` без funding generation;
5. получает и применяет не больше configured account batch. Перед каждым POST выполняется новый
   GET, и POST получает только exact digests из этого ответа;
6. делает финальный полный inventory scan и повторную coverage-проверку перед confirmation.

`active_legacy_reservation` возвращает только свой account в `retry`. Остальные typed blockers
сохраняются как `blocker` с exact `source`, `source_state_digest` и `blockers[]`; `conflict` также
остаётся fail closed. `stale` перепланируется. `normalized`, `stored` и `unchanged` фиксируются как
`ready`. Expired leases не теряют plan identity, а bounded retry не откатывает уже ready accounts.

Parent становится `confirmed` только при одновременном выполнении всех условий:

- final engine identity digest и service inventory digest совпадают с immutable target plan;
- queue содержит ровно все balance accounts и ни одного service/missing/extra account;
- каждая строка `ready`, имеет positive funding generation и
  `applied_funding_digest=target_funding_digest` без blockers;
- canonical funding manifest вычислен из exact ready queue и ещё не конфликтует с уже
  финализированной identity.

Новый account после этого evidence инвалидирует следующий Stage 8 inventory digest; Stage 9 ещё
раз проверяет полное покрытие под global release lock непосредственно перед single-head CAS.
Первая короткая `SERIALIZABLE` confirmation transaction заполняет каждую balance assignment только
переходом `funding_generation: NULL → positive`, сохраняет одинаковый canonical
`funding_manifest_digest` и копирует exact funding evidence в заранее подготовленный recovery plan.
Любая replacement, missing/extra assignment или несовпадение ready queue откатывает всю локальную
финализацию. После commit consumer строит exact target/recovery engine releases и recovery link,
выполняет три prepare+readback без удержания PostgreSQL transaction, а затем второй короткой
`SERIALIZABLE` transaction сохраняет три ACK, `engine_release_digest`, target/recovery digests и
статус `prepared`. Retryable transport/DB failure оставляет parent неподтверждённым и допускает
idempotent retry; immutable readback drift или изменившийся local bundle останавливается fail
closed. DB guards запрещают более ранний `prepared` и замораживают assignments после него. Runner
не создаёт activation job/Stage 8 evidence, не двигает release head и не меняет balances или
pricing policy.

## Staging и status

CLI принимает только `DATABASE_URL` и exact Stage 5 plan digest; engine credentials ему не нужны,
потому что все account-local и release prepare выполняет уже запущенный worker. `status` ничего не
пишет и показывает lineage, состояния обоих plans/job, attempts/last error, разбиение queue и обе
final identities.
`stage` идемпотентно создаёт job и затем печатает тот же status snapshot:

```bash
pnpm --filter @claude-api/db pricing:stage6-v2 status sha256:v2:<exact-stage5-plan-digest>
pnpm --filter @claude-api/db pricing:stage6-v2 stage sha256:v2:<exact-stage5-plan-digest>
```

Это package entrypoint для защищённого production control-plane, а не разрешение запускать команду
по SSH. До появления штатной host/admin operation production Stage 5/6 остаются невыполненными;
наличие CLI или локального integration evidence не заменяет production `confirmed` status.

Bounded параметры worker имеют безопасные defaults и жёсткие пределы:

```text
FUNDING_NORMALIZATION_POLL_MS=5000
FUNDING_NORMALIZATION_BATCH_SIZE=25              # 1..500 account operations per slice
FUNDING_NORMALIZATION_INVENTORY_PAGE_SIZE=500    # 1..500 rows per cursor page
FUNDING_NORMALIZATION_LEASE_MS=300000             # 30s..1h, heartbeat на cursor pages
FUNDING_NORMALIZATION_RETRY_MS=15000              # 1s..1h
```

## In-flight contract

Ноль reservations не требуется. Legacy-format reservations/outbox rows, созданные до
dual-compatible runtime, продолжают естественно завершаться до или после Stage 9 по своей
reserve-time identity. Новые запросы продолжают поступать и уже несут v2 snapshot; оба формата
пересекают Stage 9 без пересчёта цены или funding allocation.

## Blockers

Ручной финансовой ревизии нет, но автоматическая арифметика остаётся fail closed. Stage 6 не может
объявить account ready при:

- несовпадении aggregate и суммы lots;
- negative/overflow, нарушающем money invariants;
- конфликтующем idempotency reference;
- незавершённой legacy reservation, для которой нет честного snapshot;
- изменении account state после построения expected digest.

Такой blocker исправляется кодом или повторным планом на свежем state; production traffic ради него
не останавливается.

Durable queue хранит точные `source_state_digest`, `source`, `blockers[]` и nullable target identity.
Для `ready` target generation/digest и exact applied digest остаются обязательными. Это позволяет
сохранять честный blocked-план без placeholder-значений; наличие expand-схемы само по себе не
запускает backfill.

## Completion evidence

Stage 6 завершён, когда confirmed parent доказывает exact full-inventory funding manifest, каждый
balance account target и recovery plan имеет exact immutable funding generation, final manifest
атомарно сохранён, все новые writers dual-compatible, legacy-format inflight count наблюдается и
format-aware settlement доказан, а full replay возвращает только `unchanged`. После engine prepare/readback evidence входит в Stage 8 и
обе финальные release identities. Наличие runner-кода без staged/confirmed production job
завершением не считается.
