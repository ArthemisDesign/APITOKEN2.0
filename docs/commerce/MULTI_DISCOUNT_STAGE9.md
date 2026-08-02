# Stage 9 — атомарный full-inventory cutover

Stage 9 переводит всех клиентов одновременно без canary и без остановки production. Единственная
live mutation — compare-and-set одного global active pricing release head.

## Preconditions

- deployed runtime на обоих blue-green слотах поддерживает target и recovery release schema;
- старый несовместимый binary исключён из rollback floor;
- Stage 5 target/recovery manifests materialized и имеют exact ACK;
- Stage 6 завершён для 100% inventory;
- Stage 7 подтверждает canonical OpenKeys 1:1;
- Stage 8 full-inventory evidence fresh и `passed=true`;
- shadow evaluation покрывает 100% поддержанных запросов;
- нет legacy-format reservations/outbox rows;
- нет pending/processing/retry/dead pricing control jobs;
- каждый active/disabled account имеет ровно одну B2C/B2B/OpenKeys/service assignment;
- account creation/activation использует общий release control-plane lock.

Active v2 reservations могут существовать: их immutable snapshot позволяет безопасно пересечь
cutover. Ноль всех reservations не является precondition.

## Apply

Operator передаёт exact target release digest, Stage 8 evidence digest и reason. Engine открывает
короткую `SERIALIZABLE` transaction под release advisory lock, повторяет все freshness/coverage
checks и CAS-продвигает одну head row на target generation.

Apply не обновляет account bindings, balances, reservations или ledger rows по одному. После
commit новый reserve любого аккаунта читает target release. Reservation, созданная до commit,
settle'ится по сохранённому прежнему snapshot.

Exact replay возвращает `unchanged`. Stale evidence, inventory drift, неподдержанный runtime,
неполная funding generation или CAS mismatch отклоняются до mutation. Отказ не требует выключать
traffic: старый release продолжает обслуживаться.

## Recovery

Recovery release готовится до apply и имеет следующую monotonic generation. При автоматическом
post-activation blocker выполняется forward CAS на recovery head; это не возврат к старому binary и
не удаление target artifacts.

Recovery trigger'ы включают системный рост pricing/admission failures, funding invariant failures,
settlement backlog и расхождение active release readback. Единичный provider outage обрабатывается
provider master-switch и сам по себе не откатывает pricing release.

## Post-activation evidence

Сразу после CAS проверяются exact active release digest, B2C 50%/override test vectors, один B2B,
один OpenKeys 1:1, service с нулевым балансом, welcome/referral attribution и cross-cutover
settlement. Финальный exact SHA должен получить зелёный `deploy/watchdog`.

Maintenance window, global drain, canary selection artifact и ручное утверждение денежных
allocations в этом runbook отсутствуют.
