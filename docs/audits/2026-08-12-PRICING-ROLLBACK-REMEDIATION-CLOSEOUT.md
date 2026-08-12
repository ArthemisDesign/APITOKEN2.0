# Pricing rollback remediation closeout — 2026-08-12

## Вывод и граница этого документа

Все runtime-, money-, contract-, partner- и operator-UI дефекты, обнаруженные после pricing
rollback 2026-08-10, исправлены отдельными production-пакетами и повторно проверены на текущем
production. Этот документ — контрольная точка, чтобы последующие агенты не восстанавливали историю
аудита повторным чтением всего репозитория.

Аудит ещё не имеет финального статуса `complete`: удаление retired pricing schema намеренно
заблокировано 30-дневным retention до `2026-09-09 10:00:00 UTC`, а один отрицательный аккаунт —
явный бонусный долг, судьба которого требует решения владельца денег. Пополнение payout hot wallet
и внешняя failure-domain инфраструктура также не являются изменениями исходного кода.

Текущий production `master`/watchdog SHA на момент последней проверки:
`255f1d6b5a5d3206996d83254b8418c9ec7c2b98`, `deploy/watchdog=GREEN`.

## Finding → production evidence

| Исходная находка | Исправляющие production SHA | Текущее доказательство |
|---|---|---|
| Sales producer/consumer не соглашались о scalar usage row | `b8c428e7`, затем durability/fencing `76c3f3a9`–`dacf5da2` | usage cursor продолжает двигаться; официальный preflight: все три Sales stable watermarks покрыты, ошибок sync/parser после активации нет |
| Pricing worker мог подтвердить чужую lease или obsolete desired | `ac9ef787`, `c328ac5b` | stale-confirmed `0`; default/provider/status drift `0`; real-PostgreSQL interleaving и lease CAS закреплены тестами |
| Параллельные settlements могли пробить общий `−$1` floor | `8ead1e44`, `bce0e67b`, `115b9cb3`, `1b3910d9`, `6717ce8d`, `b1411966` | shared account-row serialization и explicit uncollected evidence; charge mismatch `0`, balance divergence `0`; единственная строка ниже floor доказана как adjustment debt, не settlement leak |
| OpenKeys literal не проходил CHECK, readiness ничего не доказывал | `aba91366`, `b77c6faf` | authenticated engine/catalog readiness; pricing drift `0`; официальный preflight сообщает OpenKeys health clear |
| B2B admin-credit мог восприниматься как реальные деньги/commission basis | `a526ffc7` | admin-credit классифицирован как gift/bonus, scalar free-first; Sales basis содержит только collected customer-funded nanoUSD |
| Два реально активных B2B mapping оставались pending | `28826e1e`, `c9c13be0` | status drift `0`; pricing preflight покрывает 177 mapped accounts/cursors |
| Provider overrides отсутствовали в commerce, default+overrides писались частично | `a3317a83`, `78620080` | atomic bundle writer, exact recovery contract; provider drift `0` |
| Refund после confirmed credit невозможно было завершить | `60150fca` | fenced `engine_adjustments` worker и idempotent engine debit; queue health clear |
| Refund/dispute не уменьшал комиссию и payout мог пересечь stale source head | `94aab427`–`5a6bfca8` | immutable signed adjustments, debt/payable, funding/reversal cursors and source-head fencing; все четыре accounting completeness series `0`, partner debt `0`, payout batches active/unsafe `0` |
| Partial rollback commerce/engine был HTTP-несовместим | `597d1f93` | immutable release capability contract и PID/exact-release mixed-version gate; preflight подтверждает current/previous/recorded/active rollback floors |
| Multiplier/provider contracts расходились (`100000`, `zhipu`/`glm`, weak CHECK) | `a8082de0`, `c2f0b238` | обе PostgreSQL schema и writers имеют `0..10000` и canonical provider set; pricing authority reconciliation up |
| SQLite/PostgreSQL overdraft semantics расходились | `0ab6d8aa` | одна registry-константа и одинаковый account-wide reserve contract, pinned backend tests |
| Authorization делал два pricing round-trip | `1ee750d3` | один joined snapshot, bounded provider rows, no TTL/cache |
| 477 historical usage rows не имели доказуемого provider | `78620080`, monitoring `d6714685` | все 477 terminal version-2 gaps; `window=1h` равно `0`; ни одной строки не было выдуманно relabelled |
| Monitoring смотрел на retired queues и не видел cross-context drift | `e413d885`, `d6714685`, `46950491` | fixed series для pricing, OpenKeys, provider, Sales cursors/accounting/debt; Prometheus rules health OK; текущих partner alerts нет |
| B2C admin скрывал восемь сохранённых `4000 bp` как `5000` | `c2f0b238`, UI `46950491` | production содержит 8 строк `4000`; admin artifact показывает persisted scalar (`−60%`) и не подставляет default |
| B2B/admin transfer UI был неровным и не объяснял условия | `48faef13`, `dacf5da2` | B2B default/provider bundle, invitations and conversion workflows разделены и согласованы; production admin build GREEN |
| Пустой payout wallet выглядел как настроенный движок | producer `729e5e5e`, consumer `9b78d299`, bounded diagnostics `255f1d6b` | production chain proof: address `0x82F76Fc837f53F02b6daD3D0a2DdF0d20e4B80a1`, `0 USDT`, `0 BNB`, `0.00001 BNB` bound/transfer; `/partners` показывает явный readiness verdict и requirements |

## Последний production closeout snapshot

Read-only проверки на `9b78d299`/`255f1d6b` подтвердили:

- pricing default/provider/status drift — `0/0/0`, reconciliation scopes — `1/1`;
- stale-confirmed pricing jobs — `0`, provider charge mismatch — `0` для всех пяти providers;
- OpenKeys pricing drift — `0`;
- Sales funding cursor/head — `13/13`, reversal — `0/0`;
- partner accounting incomplete (`usage_funding`, `commission_funding`,
  `reversal_adjustments`, `payout_boundary`) — все `0`, partner debt — `0`;
- unresolved provider: historical all-time `477`, последний час `0`;
- firing alert один: `EngineAccountsBelowFloor=1`, и он соответствует документированному
  bonus-revoke adjustment debt `−$2.52259`, а не новой settlement-утечке;
- payout chain authority доступна и canonical USDT contract proof проходит, но wallet пуст.

Официальный `deploy/pricing-retirement-preflight.sh --report` на exact processed SHA прошёл:

- source manifest/baseline/Drizzle/runtime reader guard;
- rollback-floor contract;
- 177 mapped pricing watermarks и все три Sales watermarks;
- live queues, money invariants, pricing authority, Sales и OpenKeys health;
- immutable evidence для 31 engine tables (`198757` rows, `154804224` bytes) и 43 commerce
  tables (`85275` rows, `104783872` bytes), без content/physical drift;
- dependency graphs: engine — ровно один разрешённый FK, `0` views, 52 functions, 36 triggers;
  commerce — `0` external FK/views, 7 functions, 7 triggers.

Вердикт закономерно `NOT AUTHORIZED`: retention epoch `1788948000` ещё не наступил; exact-SHA
fresh dumps создаются watchdog только у destructive migration boundary.

## Реальный остаток и точный порядок закрытия

### 1. Retired schema — после 2026-09-09 10:00 UTC

Следовать только `docs/ops/PRICING_RETIREMENT.md`, тремя независимо зелёными изменениями:

1. commerce migration-only `0048_retire_pricing_schema.sql`;
2. после её production GREEN — engine migration-only `0049_retire_pricing_schema.sql`;
3. после обеих — schema/code/document cleanup и final post-drop matrix.

Нельзя заранее создавать destructive migrations, запускать final preflight вручную, использовать
`CASCADE`/`IF EXISTS`, объединять обе базы в один change или считать diagnostic report
авторизацией. Watchdog сам создаёт и валидирует свежие exact-SHA dumps, выполняет admission и
post-drop proof.

### 2. Bonus-revoke debt — требуется решение владельца

Один engine account имеет `−$2.52259` из-за записанного `bonus-revoke` adjustment `−$4`. Это
консервация денег, не дефект floor. Допустимы только три явных бизнес-решения: оставить как долг,
взыскать либо простить отдельной auditable adjustment-операцией. Не переписывать ledger и не
маскировать alert без такого решения.

### 3. Payout wallet — внешнее пополнение

До ближайшей выплаты перевести на показанный в админке адрес:

- canonical BSC USDT не меньше актуального `eligible` total;
- BNB не меньше `gasCostPerTransferWei × sendable rows`, с разумным операционным запасом.

Админка и backend после этого должны независимо показать sufficiency; backend повторяет proof под
money/send locks непосредственно перед signing. Исходный audit не предоставляет полномочий
переводить on-chain средства, поэтому агент не финансирует кошелёк самостоятельно.

### 4. Failure-domain hardening — внешняя конфигурация

Внутренний Prometheus/Alertmanager, public blackbox probes, hourly PostgreSQL dumps и daily encrypted
Borg off-host repository работают. Полный host/power/network outage всё ещё требует внешнего
dead-man/uptime monitor, а Cherry backup volume — независимой второй backup location. Конкретный
provider/account/credentials должны быть предоставлены оператором; без них репозиторий не может
создать независимый failure domain.

## Критерий финального `complete`

Pricing rollback audit закрывается только когда одновременно:

1. оба retired-schema contraction и cleanup SHA production-GREEN с post-drop proof;
2. bonus debt получил зафиксированное бизнес-решение либо владелец явно утвердил его как retained debt;
3. payout wallet покрывает актуальный eligible USDT и BNB gas requirement до payout window;
4. внешний dead-man и независимая вторая backup location имеют живое delivery/restore evidence.

До этого этот документ — доказанный remediation closeout и resumable checkpoint, но не заявление,
что внешние и retention-bound пункты уже выполнены.
