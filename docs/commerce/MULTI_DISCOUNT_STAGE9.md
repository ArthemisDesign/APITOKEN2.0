# Stage 9 — атомарный full-inventory cutover

Stage 9 переводит всех клиентов одновременно без canary и без остановки production. Единственная
live mutation — compare-and-set одного global active pricing release head.

Commerce migration `0031_pricing_activation_evidence_capture.sql` — отдельный expand-only
checkpoint до consumer-кода. Она добавляет nullable-хранение source engine evidence digest/exact
capture time, immutable activation request и полного validated engine receipt. Миграция не делает
backfill, не создаёт activation job и не вызывает engine CAS; старый runtime продолжает работать,
а dependent consumer доставляется только после GREEN `deploy/migration` и `deploy/watchdog` этого
schema SHA.

После GREEN schema SHA durable consumer добавляет strict contracts, единственный typed engine
transport и worker lifecycle `pending → processing → retry|dead|confirmed`. Explicit staging
сохраняет immutable request до сети и принимает только persisted `passed=true`/zero-blocker
evidence с prepared target/recovery engine digests. Перед первой сетевой доставкой worker в свежем
`SERIALIZABLE` snapshot повторно исчерпывает engine и OpenKeys inventory, сверяет commerce/service
ownership, status, B2B scalar authority и OpenKeys 1:1, а после cutover требует exact paired
assignment extension, policy и active funding generation для каждого нового account. Наружу
выходят только digest'ы субъектов. После того как exact request впервые выдан transport lane,
timeout, crash или lost ACK повторяет только это сохранённое тело без TTL/mutable-authority
preflight: CAS уже мог примениться, поэтому новая интерпретация retry была бы небезопасна. Успех
атомарно сохраняет complete validated ACK и canonical request/receipt result digest.
Recovery expectation не реконструируется: она читается только из полного durable cutover receipt.
Consumer не создаёт job автоматически. Пока Stage 8 collector не заполнил nullable source digest и
capture time из миграции 0031, staging fail-closed и production CAS невозможен.

## Preconditions

- deployed runtime на обоих blue-green слотах поддерживает target и recovery release schema, а
  каждый live claim привязан к своему exact owner epoch;
- старый несовместимый binary исключён из rollback floor;
- Stage 5 target/recovery manifests materialized и имеют exact ACK;
- Stage 6 завершён для 100% inventory;
- Stage 7 подтверждает canonical OpenKeys 1:1;
- Stage 8 combined schema-v2 evidence persisted, unexpired и `passed=true`; его source engine
  evidence прошло canonical digest и 120-second age checks, а engine/OpenKeys были исчерпывающе
  просканированы дважды и не изменились между проходами;
- sales v2 runtime/consumer отдельно подтверждает commission только с `paid_funded_nano` и
  исключение welcome bonus; `sales_contract_digest` в Stage 8 сам по себе это не доказывает;
- shadow evaluation покрывает 100% поддержанных запросов;
- legacy-format reservations/outbox rows учтены как audit count и продолжают завершаться по
  reserve-time snapshot; отсутствие таких строк не является precondition;
- нет pending/processing/retry/dead pricing control jobs;
- каждый active/disabled account имеет ровно одну B2C/B2B/OpenKeys/service assignment;
- account creation/activation использует общий release control-plane lock.

Active v2 и legacy-format reservations могут существовать: каждый формат settle'ится по своей
immutable reserve-time identity. Ноль reservations или искусственная пауза traffic не являются
precondition.

## Apply

Protected control-plane передаёт exact target/recovery engine release digests, combined Stage 8
evidence identity, source engine capture time/subdigests, complete expected head, operator и reason.
До вызова engine CAS он требует exact immutable commerce row с `passed=true`, проверяет
`valid_until` и непосредственно перед первой delivery заново сверяет commerce/service/OpenKeys и
engine authority с target/recovery. Этот preflight не блокирует traffic или money writers: live
balances исключены из inventory identity и отдельно проверяются через active funding
generation/head/aggregates. Engine
открывает короткую `SERIALIZABLE` transaction под release advisory lock, повторяет engine-side
freshness/coverage checks: immutable pair/link и active catalog/switch lineage, base inventory,
funding manifest/parity, exact runtime-floor digest и owner-epoch claim каждого live instance. Затем
он CAS-продвигает одну head row на target generation. Evidence/audit/head либо commit'ятся вместе,
либо целиком откатываются.

Apply не обновляет account bindings, balances, reservations или ledger rows по одному. После
commit новый reserve любого аккаунта читает target release. Reservation, созданная до commit,
settle'ится по сохранённому прежнему snapshot.

Аккаунт, созданный после cutover, до выдачи usable key получает append-only assignment extension,
привязанную к exact текущему head и его prepared recovery. Исходный full-inventory manifest не
переписывается; exact extension pair добавляется одной транзакцией под тем же control-plane lock.

Exact replay сверяется с durable activation audit и возвращает `unchanged`, даже если ACK был
потерян, а исходный TTL затем истёк. Retry не перечитывает mutable authority, потому что отличить
lost ACK от уже применённого CAS невозможно; он отправляет byte-for-byte тот же durable request.
Stale evidence, inventory drift, неподдержанный runtime или
унаследованный от чужого owner epoch claim, неполная funding generation либо CAS mismatch
отклоняются до mutation. Отказ не требует выключать
traffic: старый release продолжает обслуживаться.

## Recovery

Recovery release готовится до apply и имеет следующую monotonic generation. При автоматическом
post-activation blocker выполняется forward CAS на recovery head; это не возврат к старому binary и
не удаление target artifacts.

Recovery принимает только complete exact target head из cutover receipt. Новые после cutover
accounts не переписывают base manifest: перед forward CAS engine требует их атомарные
target/recovery assignment extensions и проверяет их active funding heads. Поэтому recovery
остаётся одним head write и не превращается в N-account rollback. Fresh engine evidence в этом
состоянии сохраняет base inventory digest, но считает каждый новый account покрытым только exact
paired extension; recovery не ограничена TTL исходного cutover evidence.

Recovery trigger'ы включают системный рост pricing/admission failures, funding invariant failures,
settlement backlog и расхождение active release readback. Единичный provider outage обрабатывается
provider master-switch и сам по себе не откатывает pricing release.

## Post-activation evidence

Сразу после CAS проверяются exact active release digest, B2C 50%/override test vectors, один B2B,
один OpenKeys 1:1, service с нулевым балансом, welcome/referral attribution и cross-cutover
settlement. Финальный exact SHA должен получить зелёный `deploy/watchdog`.

Maintenance window, global drain, canary selection artifact и ручное утверждение денежных
allocations в этом runbook отсутствуют.
