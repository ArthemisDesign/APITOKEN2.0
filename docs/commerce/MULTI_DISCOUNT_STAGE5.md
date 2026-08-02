# Stage 5 — materialization целевого pricing release

Статус: целевой контракт; OpenKeys authoritative cursor producer реализован отдельным
producer-first checkpoint, а Stage 5 materializer consumer ещё должен быть приведён к этому
контракту до production apply. Stage 5 готовит immutable pricing authority, но не меняет live
traffic.

## Входные inventories

Planner получает свежие authoritative inventories всех bounded contexts:

- commerce B2C и B2B с полными engine account IDs;
- B2B current scalar discount и active invitation snapshots;
- каждый OpenKeys account из exact `/api/internal/pricing/v2/inventory`, включая disabled,
  removed и ранее считавшиеся legacy; все страницы обязаны иметь один full-manifest digest;
- каждый service account с purpose/responsible metadata;
- полный engine inventory для проверки покрытия.

Ручная assignment matrix больше не является authority. Все владельцы должны следовать из
authoritative inventory. Один account в двух inventories, неизвестный account, active account без
owner или отсутствующий engine account — typed blocker. Аккаунты не исключаются из target release
из-за disabled status: при последующем включении они уже должны иметь корректную policy.

## Матрица target policies

- B2C: global default `discount_bps=5000` для Anthropic, OpenAI и Gemini с возможными явными
  provider/model overrides. Exact model rule имеет приоритет над provider, provider — над global.
- Existing B2B: текущий `mult_bp` становится только provider-rule `anthropic`:
  `discount_bps=10000-mult_bp`. OpenAI/Gemini автоматически не добавляются.
- B2B invitations: независимый immutable full-policy snapshot; redemption копирует exact snapshot.
- OpenKeys: один canonical 1:1 contract (`discount_bps=0`) для всех существующих и новых accounts.
  Старые scalar discounts не переносятся в target release.
- Service: все runtime-capable модели и `billing_mode=meter_only`; balance не участвует в admission.

Planner создаёт target release manifest и заранее подготовленный recovery release следующей
monotonic generation. Оба связывают capability, main/OpenKeys catalogs, switches, policies,
assignments, funding generation и minimum runtime capability одним canonical SHA-256 digest.

## Dry run

Dry run работает в read-only repeatable snapshot и выводит:

- source/inventory digests;
- полное покрытие account classes;
- immutable policy identities;
- target/recovery release digests;
- typed blockers и exact writes plan.

Dry run ничего не пишет и не требует reviewer field. Любое изменение inventory делает результат
stale; JSON нельзя редактировать вручную или применять по digest после изменения source state.

## Materialize

Apply работает в `SERIALIZABLE` transaction, повторно строит тот же план и принимает exact expected
source/plan digest. Он materialize'ит immutable capability/catalog/switch/policy/release rows и
durable delivery jobs, но не двигает active pricing release head.

Same-version/same-digest replay возвращает `unchanged`. Same-version/different-digest, неполное
покрытие inventory, stale source, policy collision или unsupported runtime capability отклоняются
до commit. B2C/B2B/service/OpenKeys target готовится целиком; partial apply по классам запрещён.

## Evidence для следующих этапов

До Stage 6/8 сохраняются restricted operational artifacts:

- exact inventories;
- dry-run report и plan digest;
- target/recovery release manifests;
- durable ACK всех prepared identities.

Stage 5 не меняет цены, балансы, ключи и доступ. Live behavior меняется только single-head CAS на
Stage 9 по `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
