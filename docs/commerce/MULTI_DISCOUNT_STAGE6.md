# Stage 6 — online funding normalization

Статус: целевой контракт. Stage 6 не требует maintenance window, остановки money writers, нуля всех
reservations или ручной проверки аккаунтов.

## Source policy

Funding normalization нужна для paid/bonus attribution и реферальной математики, а не для
ограничения доступных моделей.

- точный неотозванный `signup-bonus:<subject>` сохраняется как `welcome_bonus`;
- существующая выдача сохраняет фактический номинал `$4`;
- новые выдачи после изменения контракта имеют номинал `$5`;
- весь остальной существующий остаток по решению владельца классифицируется `paid`;
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
