# Stage 7 — OpenKeys canonical 1:1

Stage 7 закрывает все пути выпуска OpenKeys с ценой, отличной от официальной 1:1, и готовит всё
существующее inventory к общему Stage 9 cutover.

## New issuance

Каждый новый batch/key имеет contract `official_1_to_1`, `discount_bps=0` и
`payable_multiplier_bp=10000`. Request/env поля multiplier, discount или pricing override
отклоняются до database/engine write. Face-value credit остаётся точным integer nanoUSD.

Issuance использует текущий OpenKeys product catalog. Новая Anthropic/OpenAI/Gemini модель
появляется только после явной catalog generation; наличие модели в engine capability не включает
её автоматически.

До выдачи usable secret приложение обязано получить exact prepared/active policy ACK, повторно
прочитать binding и сохранить matching OpenKeys row. Lost-process compensation отключает
незавершённый engine account.

## Existing inventory

Все существующие OpenKeys, включая ранее считавшиеся legacy, получают target canonical 1:1 policy.
Их прошлые ledger rows и списания не переписываются. Текущий live reserve остаётся на старом active
release до Stage 9; затем весь inventory одновременно начинает списываться 1:1.

Stage 7 dry run сверяет OpenKeys DB inventory с engine accounts, Stage 5 plan и canonical policy
digest. Missing/duplicate/source collision или любой discount в target policy блокирует complete
apply до первой записи.

Apply идемпотентно materialize'ит exact target bindings и подтверждает readback. Он не двигает
global active release head, не меняет balance/key/status и не выполняет отдельный OpenKeys cutover.

## Durable shadow rollout lane (migration 0035)

Pre-cutover policy alignment OpenKeys-инвентаря (включая replacement-locked legacy) выполняется
только durable lane поверх пустых parent/child таблиц `pricing_shadow_rollouts_v2` /
`pricing_shadow_policy_jobs_v2`. Это заменяет упразднённый OpenKeys-local backfill с ручной
assignment matrix: его модуль, CLI и тесты удалены, потому что generic prepare/activate для
replacement-locked bindings принципиально возвращают `423 locked`, а ручная matrix не является
durable authority.

**Scope lane — только OpenKeys.** Commerce B2C/B2B и service lineages выравниваются их managed
policy writers (catalog/switch convergence + managed policy update создают gen-aligned версии на
существующей lineage), потому что engine никогда не принимает другой policy identity для аккаунта
с существующей lineage (`policy_identity_matches`), а `meter_only` service-семантика вообще не
выражается v1 shadow policy. Rollout по-прежнему валидирует full Stage 5 inventory fail-closed,
но jobs создаются только для OpenKeys assignments.

**Producer.** Единственный способ создать rollout — AdminGuard-protected
`POST /v1/admin/pricing-shadow-rollout-v2/stage` в `apps/api` с UUID `idempotency_key`, exact
`stage5_run_id`, meaningful `reason` и verified actor из заголовка `x-admin-actor` (actor в JSON
body запрещён). `packages/db/src/pricing-shadow-rollout-jobs-v2.ts` одной `SERIALIZABLE`
транзакцией под advisory lock читает exact prepared Stage 5 run (status `prepared`), prepared
target/recovery release plans и делает fresh engine inventory scan: любой drift digest'ов,
collision или missing owner — fail closed до первой записи. Rollout пинит target/recovery
generation+digest, catalog/switch generations+digests, engine inventory, assignment/policy manifest
и canonical `sha256:v2` rollout digest; per-account jobs несут release-policy identity, exact
effective version/content digest, nullable expected active (только для детерминированно выводимых
locked legacy bindings), request digest и полное byte-exact request payload. Идемпотентность по
`idempotency_key` и `rollout_digest`: exact replay возвращает существующий rollout без записи.

**Locked-OpenKeys путь.** Job для replacement-locked legacy аккаунта (`owner_context=openkeys`,
`pricing_contract=legacy` в exact Stage 5 inventory) содержит только payload
`locked_openkeys_transition`: successor строится детерминированно (+1 exact version, тот же
immutable policy identity, managed provider-only 1:1 rules, без replacement lock), а
expected active — exact legacy policy version 1 с digest в домене `multi-discount-stage5`.
Worker доставляет его исключительно через
`POST /admin/pricing/policy/{account_id}/locked-openkeys-transition` после fresh readback:
расхождение active policy с durable expectation, потерянный replacement lock или typed отказ
(400/409/423) — terminal `blocked` с `last_error`.

**Generic путь (canonical OpenKeys).** Для уже канонических OpenKeys-аккаунтов (`official_1_to_1`)
job несёт payload `policy_shadow`: successor строится на СУЩЕСТВУЮЩЕЙ engine lineage аккаунта
(тот же policy identity, следующая monotonic version, exact current active как expected active),
с правилами, сконвертированными из release policy, и pins точного Stage 5 catalog/switch.
Worker читает engine state, подтверждает уже exact
policy одним readback без mutation, иначе делает prepare → exact readback → activate с CAS
expectation из fresh state. Любой version conflict, digest mismatch, newer engine policy или typed
rejection — `blocked`; transient transport — bounded `retry` с lease; expired lease reclaim'ится,
последняя попытка уходит в `dead`.

**ACK evidence и terminal state.** Каждый confirmed job хранит canonical `sha256:v2` ACK digest и
полный ACK payload (engine ACK либо exact readback evidence). Когда все jobs терминальны, rollout
атомарно становится `confirmed` (все confirmed), `blocked` или `dead`. Read-only статус —
`GET /v1/admin/pricing-shadow-rollout-v2`: bounded snapshot субъектов только как `sha256:v2`
digest'ы, без raw account identities. Startup, migration, polling и read endpoint не создают
rollout/job. Lane не меняет live цену, funding authority, release head, balances или OpenKeys rows.
Worker bounds: `PRICING_SHADOW_ROLLOUT_POLL_MS=5000` (`1000..60000`),
`PRICING_SHADOW_ROLLOUT_LEASE_MS=300000` (`30000..3600000`),
`PRICING_SHADOW_ROLLOUT_RETRY_MS=15000` (`1000..3600000`),
`PRICING_SHADOW_ROLLOUT_MAX_ATTEMPTS=10` (`1..100`),
`PRICING_SHADOW_ROLLOUT_BATCH_SIZE=25` (`1..500`); дефолты production-safe и валидируются на
старте worker.

## Invariants

- В target release нет source-specific discounted legacy policy.
- Existing и new OpenKeys имеют одну экономику 1:1.
- OpenKeys не наследует global B2C/provider/model discounts.
- OpenKeys usage не участвует в referral commission.
- Ни admin API, ни batch issuance не принимают multiplier field.
- Live change происходит только общим CAS из `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
