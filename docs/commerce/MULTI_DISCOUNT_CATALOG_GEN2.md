# Multi-discount catalog generation 2 — frozen inert artifact

Status as of 2026-08-02: generation 2 was delivered as dormant code and its dry run was green, but
its durable heads were never activated. It contains Anthropic/OpenAI only and predates the approved
Gemini/global-50%/single-release contract. **Do not activate generation 2.**

The immutable constants and digests remain recorded for audit and exact readback:

- capability generation `2`, schema version `1`, content digest
  `sha256:v1:9b23acd863d22abe2a6ed12096a4bb68a07b8d5c196351f1a15d38f11029bcd0`;
- `main` product catalog generation `2`, content digest
  `sha256:v1:807fbe80c12a03e773e2f5067bc04a66b5a41e42d4bfdc8f85fe5656a5013616`;
- `openkeys` product catalog generation `2`, content digest
  `sha256:v1:3b019fc3cfd619b5d4a81451aceafebf0c40de3b8c2cc150aa5b7a28b0102760`;
- provider-switch generation `2`, content digest
  `sha256:v1:ddbe078beec31d4f8b77e027ff3e9dad5477be6d10dafd4c99956abd9a74febd`.

Its twelve-model payload contains seven Anthropic models (`claude-fable-5`,
`claude-haiku-4-5`, `claude-opus-4-7`, `claude-opus-4-8`, `claude-opus-5`,
`claude-sonnet-4-6`, `claude-sonnet-5`) and five OpenAI models from generation 1. It contains no
Gemini entries and therefore cannot be the target release foundation.

## Replacement rule

Implementation must create a new monotonic capability/catalog/switch generation; it must never
rewrite these generation-2 constants or reuse their generation/digests. The replacement:

- includes reviewed Anthropic/OpenAI/Gemini capabilities;
- builds separate explicit `main` and `openkeys` product catalogs;
- binds global B2C 50% plus provider/model overrides;
- binds canonical OpenKeys 1:1 and service all-model `meter_only` behavior;
- is referenced by prepared target and recovery pricing releases;
- remains dormant until the full-inventory Stage 9 head CAS.

The old `pricing:catalog-gen2 apply` command must be disabled or made fail closed with a
`superseded_generation` result before the final rollout. It is not a supported production recovery
step.

Activation and rollback for the replacement are specified in
`docs/commerce/MULTI_DISCOUNT_STAGE9.md`. A rollback binary must understand the replacement release
schema; reactivating generation 1 or 2 is not rollback.
