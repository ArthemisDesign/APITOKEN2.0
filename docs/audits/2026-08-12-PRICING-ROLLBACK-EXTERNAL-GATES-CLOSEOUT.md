# Pricing rollback external gates closeout — 2026-08-12

## Назначение

Это append-only продолжение
`docs/audits/2026-08-12-PRICING-ROLLBACK-REMEDIATION-CLOSEOUT.md`. Оно фиксирует живые
read-only доказательства, собранные после production SHA `ff521c1d`, и закрывает внешний
dead-man/uptime пункт. Следующий агент начинает отсюда и не повторяет широкий аудит репозитория.

Последний доставленный SHA этого продолжения: `9574aa9d833d6064877ec34e5074646902f175ef`,
`deploy/watchdog=GREEN`.

## Bonus-revoke debt — точная причинность

В engine ровно один аккаунт ниже общего settlement floor:

- account: `acct_d83edbc93d17247d216ce019`;
- balance: `−2522590000 nanoUSD` (`−$2.52259`);
- reserved/uncollected: `0/0`;
- spent: `2522590000 nanoUSD`.

Неизменяемый ledger полностью объясняет результат:

1. `2026-07-22 15:57:48 UTC` — signup bonus `+$4`, ref
   `signup-bonus:18a95747-89e3-42fc-a81a-1d2e2dac549b`;
2. 12 charge rows списали ровно `$2.52259`, оставив `$1.47741`;
3. `2026-07-22 16:17:56 UTC` — полный отзыв исходного бонуса `−$4`, ref
   `bonus-revoke:18a95747-89e3-42fc-a81a-1d2e2dac549b`;
4. итоговый balance `−$2.52259` арифметически равен потраченному до отзыва бонусу.

Это записанный долг после административного отзыва, а не settlement race, uncollected overage или
пробой floor. Код и ledger не изменялись. Остаётся только решение владельца денег: retained debt,
взыскание или отдельная auditable forgiveness adjustment.

## Payout wallet — точное требование

Read-only Sales endpoint и независимый chain proof показали:

- период `2026-07-P2` закрыт; окно было `2026-08-08..2026-08-11 UTC`;
- сейчас eligible rows `0`, потому что окно закрыто;
- следующий sendable candidate: один партнёр, `3468673624 nanoUSD` (`3.468673624 USDT`);
- payout hot wallet: `0x82F76Fc837f53F02b6daD3D0a2DdF0d20e4B80a1`;
- баланс кошелька: `0 USDT`, `0 BNB`;
- gas bound: `10000000000000 wei` (`0.00001 BNB`) на перевод.

Перед следующим окном оператор должен заново прочитать актуальный due list, затем внести не меньше
его USDT total и `gas bound × sendable rows`, лучше с операционным BNB-запасом. Значения выше —
доказанный snapshot, не разрешение отправлять устаревшую сумму.

## Backup evidence

Hourly `claude-api-backup.timer` и daily `borgmatic.timer` активны/enabled, последние service result
равны `success`. Все пять local custom-format dumps присутствовали и были моложе 22 минут в момент
проверки: `commerce`, `claude_engine`, `sales`, `openkeys`, `apitoken_crm`.

Последний off-host encrypted Borg archive:

- archive: `apitokensale-2026-08-12T00:22:38`;
- repository: Cherry Borg path из `docs/ops/INFRASTRUCTURE.md`;
- `claude_engine.dump`: `67977659` bytes;
- `commerce.dump`: `18735607` bytes;
- `apitoken_crm.dump`: `22828446` bytes;
- `sales.dump`: `285553` bytes;
- `openkeys.dump`: `95597` bytes.

Таким образом, ежедневный off-host архив доказан содержимым, а не только существованием archive
name. Cherry volume всё ещё связан с lifecycle того же инфраструктурного провайдера; независимая
вторая backup location остаётся внешним гейтом.

## Off-host uptime — закрыт

Production SHA `b4f3f032` добавил GitHub-hosted five-minute probes, incident reconciliation и
mocked regression suite; `3f5ec2d2` устранил live-observed issue-list cache ambiguity; `9574aa9d`
закрепил reopen singleton incident.

Живое delivery evidence:

- synthetic failure run `31580220909` открыл reserved incident issue `#1`;
- healthy run `31580962722` проверил все восемь public contracts и закрыл issue с recovery evidence;
- повторный synthetic failure run `31581301887` переоткрыл тот же issue `#1`;
- healthy run `31581340887` снова закрыл его с recovery comment;
- issue `https://github.com/3xcalibur-tech/Claude_API/issues/1` имеет финальный
  `state=closed`, `state_reason=completed`, два recovery comments.

Это доказывает отдельный host/network/power failure domain и полный
`detect → deliver → deduplicate/reopen → recover → close` цикл. GitHub failure остаётся общим риском
самого внешнего провайдера; второй монитор может быть defense in depth, но больше не является
обязательным пунктом этого audit closeout.

## Retention gate

Production processed SHA на начале этой проверки был `ff521c1d`. Retired schema admission остаётся
запрещён до `2026-09-09 10:00:00 UTC`: последний authoritative retired timestamp
`2026-08-10 09:26:32 UTC` плюс 30 полных дней, округлённый консервативно. Final admission заново
вычисляет максимум; более новая строка сдвинет срок.

После границы выполнять только три стадии из `docs/ops/PRICING_RETIREMENT.md`: commerce contraction,
engine contraction, затем schema/code/docs cleanup. Не создавать migration заранее и не обходить
watchdog admission.

## Точный остаток

1. После `2026-09-09 10:00 UTC` — три independently GREEN retired-schema стадии.
2. Решение владельца по `−$2.52259`: retained debt, collect или auditable forgive.
3. Внешнее пополнение payout wallet перед окном; текущий доказанный минимум — `3.468673624 USDT`
   и `0.00001 BNB`, но сумма должна быть перечитана непосредственно перед funding.
4. Независимая вторая backup location и restore evidence; нужны provider/account/credentials.

Новых незакрытых дефектов приложения из исходного pricing rollback аудита нет. Финальный статус
цели всё ещё не `complete`, потому что четыре пункта выше требуют времени, денег, бизнес-решения или
внешней инфраструктуры.
