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

## Invariants

- В target release нет source-specific discounted legacy policy.
- Existing и new OpenKeys имеют одну экономику 1:1.
- OpenKeys не наследует global B2C/provider/model discounts.
- OpenKeys usage не участвует в referral commission.
- Ни admin API, ни batch issuance не принимают multiplier field.
- Live change происходит только общим CAS из `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
