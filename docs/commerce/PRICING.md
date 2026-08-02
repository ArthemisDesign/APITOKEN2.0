# Customer pricing

Целевой контракт утверждён 2026-08-02. До атомарного Stage 9 cutover production может продолжать
исполнять старый scalar/progressive путь, но его нельзя расширять: implementation обязана прийти к
контракту ниже по zero-downtime плану `docs/commerce/MULTI-DISCOUNT.md`.

## B2C

Обычный B2C платит 50% официальной стоимости любой модели из main product catalog:

```text
global discount_bps = 5000
global payable_multiplier_bp = 5000
```

Прогрессивных tiers, порогов top-up, 30-day retention и month-close pricing behavior в целевой
системе нет. Пополнение увеличивает только баланс и не меняет процент скидки.

Оператор может задать B2C provider/model override. Приоритет всегда:

1. exact model rule;
2. provider rule;
3. global 50%.

Например, Gemini 60% и отдельная Gemini image model 55% дают 55% именно image-модели и 60% всем
остальным Gemini-моделям. Скидки не суммируются.

Официальная стоимость вычисляется `crates/metering` по immutable effective-dated tariff и только
затем умножается на integer `payable_multiplier_bp`. Все суммы — nanoUSD/decimal strings; float и
JavaScript `number` для денег запрещены.

## Welcome bonus

После rollout новая eligible Google/GitHub B2C-регистрация получает ровно `$5.000000000` с
идемпотентным `signup-bonus:<commercial-user-id>`. Password, invited B2B, OpenKeys и service
аккаунты бонус не получают. Ранее выданные `$4` сохраняются без ретроактивного увеличения.

Welcome bonus может оплачивать любую разрешённую B2C-модель Anthropic/OpenAI/Gemini. Funding
расходуется bonus-first, затем paid. Номинал показывается как денежный баланс, без маркетингового
пересчёта в «официальный usage».

## Provider/model pricing authority

Pricing policy и model admission независимы. Catalog включает модель в продукт, switch может
аварийно закрыть provider, policy задаёт процент. Отсутствующий applicable rule после Stage 9 —
fail closed; scalar fallback запрещён.

Текущий Gemini tariff schedule `google/gemini-developer-api/2026-08-02` включает
`gemini-3-flash-preview`: text/image/video input `$0.50/M`, audio input `$1/M`, cached text
`$0.05/M`, cached audio `$0.10/M`, output вместе с thinking `$3/M` и Search `$14/1000 queries`.
В B2C после общей скидки 50% эффективные суммы составляют половину этих официальных ставок; новый
model ID всё равно требует явной catalog generation и не появляется в OpenKeys автоматически.

Policy versions immutable, content-addressed и доставляются catalog → switches → policy. Все
аккаунты переключаются одним active release head, а не последовательным обновлением bindings.

## B2B

B2B не наследует global B2C и её provider/model overrides. У клиента собственная immutable policy,
которая копируется из invitation snapshot и далее редактируется полным CAS replacement.

Существующий scalar `mult_bp` при миграции становится только Anthropic provider-rule:

```text
provider_id = anthropic
discount_bps = 10000 - mult_bp
```

OpenAI/Gemini не появляются у существующего B2B автоматически. Их добавляет оператор явными
provider/model rules.

## OpenKeys

Все существующие и новые OpenKeys работают 1:1: `discount_bps=0`,
`payable_multiplier_bp=10000`. Они не наследуют B2C/B2B скидки и не участвуют в referral
commission. Новая модель требует явного OpenKeys catalog enablement.

## Service

Service accounts имеют `billing_mode=meter_only`: все runtime-capable модели доступны, official
usage и tariff lineage сохраняются, но reserve/debit баланса не выполняется и нулевой баланс не
даёт 402. Ограничения конкретного домена находятся в коде этого домена, не в pricing policy.

## Referral commission

Referral eligibility больше не зависит от pricing mode. Для referred B2C settlement commerce
передаёт exact `paid_funded_nano`; bonus-funded часть, B2B, OpenKeys и service исключены. Sales
применяет существующие `commission_bps`/`sub_commission_bps` к этой integer базе.

Immutable ledger attribution должна содержать pricing release/policy/rule/tariff identities,
official и charged cost, ordered funding allocations и exact paid/bonus totals. Commerce валидирует
evidence и cursor в одной transaction; sales feed получает только подтверждённое событие.

## Zero-downtime activation

Новая policy не активируется по аккаунтам. Dual-compatible runtime и funding writers сначала
выкатываются dormant, funding нормализуется online account-local transactions, а full-inventory
shadow работает на 100% traffic. Затем Stage 9 одним CAS меняет global active release head.

Активные v2 reservations могут пересекать cutover и settle'ятся по immutable reserve snapshot.
Глобальный drain, maintenance window и canary-account rollout запрещены. Полный runbook —
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`.

## Известный временный разрыв

До завершения rollout старый код/схема могут содержать tier, retention, `track`, `$4` grant и
scalar jobs. Это migration source, а не целевой контракт. Новый код не должен добавлять к ним
функциональность. После переключения readers/writers удаляются; immutable history и уже применённые
append-only migrations не переписываются.
