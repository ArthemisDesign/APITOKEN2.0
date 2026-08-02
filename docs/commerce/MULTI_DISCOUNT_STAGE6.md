# Stage 6 — online funding normalization

Статус: engine producer реализован; orchestration consumer подключается отдельным producer-first
checkpoint после зелёного `deploy/watchdog`. Stage 6 не требует maintenance window, остановки money
writers, нуля всех reservations или ручной проверки аккаунтов.

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

## In-flight contract

Ноль всех reservations не требуется. До Stage 8 должны естественно завершиться только
legacy-format reservations/outbox rows, созданные до dual-compatible runtime. Новые запросы
продолжают поступать и уже несут v2 snapshot, поэтому могут пересечь Stage 9 без пересчёта цены или
funding allocation.

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

## Completion evidence

Stage 6 завершён, когда каждый account target release имеет exact funding generation, все новые
writers dual-compatible, legacy-format inflight count равен нулю и full replay возвращает только
`unchanged`. Evidence digest входит в Stage 8 и target release.
