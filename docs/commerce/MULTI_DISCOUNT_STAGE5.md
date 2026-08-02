# Stage 5 — materialization целевого pricing release

Статус: целевой двухфазный контракт; OpenKeys authoritative cursor producer и admin-managed service
inventory producer реализованы отдельными producer-first checkpoint. Compile-fixed runtime manifest
сохраняет dormant capability generation 3 и публикует additive generation 4 с
`gemini-3-flash-preview`: exact Anthropic/OpenAI/Gemini model identity разрешена
для последующей подготовки, но ни один catalog/switch/release head этим не активируется. Migration
`0029_pricing_release_two_phase_finalize.sql` должна быть GREEN до нового materializer consumer.
Stage 5 готовит immutable source/ownership/policy authority, но не угадывает меняющиеся funding
identities и не меняет live traffic.

## Входные inventories

Planner получает свежие authoritative inventories всех bounded contexts:

- commerce B2C и B2B с полными engine account IDs;
- B2B current scalar discount и active invitation snapshots;
- каждый OpenKeys account из exact `/api/internal/pricing/v2/inventory`, включая disabled,
  removed и ранее считавшиеся legacy; все страницы обязаны иметь один full-manifest digest;
- каждый service account с purpose/responsible metadata;
- полный engine inventory для проверки покрытия.

Service authority заполняется только через `PUT /admin/service-account-inventory/{service_id}`.
Mutation делает два совпадающих полных engine scans, берёт status из engine, отклоняет commerce и
OpenKeys ownership и пишет monotonic per-service version/content digest по exact CAS. Простое
отсутствие account в commerce/OpenKeys не превращает его в service автоматически: metadata должны
быть явно зарегистрированы, а Stage 5 проверяет весь complement. GET этого admin endpoint возвращает
canonical aggregate inventory digest; mutation не создаёт policy/release и не меняет live traffic.

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

Внутренний engine provider ID Gemini — `google`. Frozen capability generation 3 сохраняет исходные
восемь тарифно закреплённых Gemini-моделей; target generation 4 добавляет
`gemini-3-flash-preview`. OpenKeys target по-прежнему намеренно сохраняет Anthropic/OpenAI набор:
Gemini появится там только отдельной явной OpenKeys catalog generation и всё равно будет 1:1.
Capability publication не является таким enablement.

Planner резервирует target generation и recovery generation следующего monotonic номера и строит
immutable source/policy/assignment plan для обеих. На этой фазе balance assignments намеренно имеют
`funding_generation=NULL`, а `funding_manifest_digest`, `engine_release_digest` и итоговые
target/recovery release digests отсутствуют. Их нельзя честно вычислить заранее: account-local
normalization включает live `balance_nano`, `reserved_nano`, `spent_nano` и lots, пока money writers
продолжают работать. Финальные release manifests строятся только из Stage 6 readback evidence.

## Dry run

Dry run работает в read-only repeatable snapshot и выводит:

- source/inventory digests;
- полное покрытие account classes;
- immutable policy identities;
- зарезервированные target/recovery generations и отсутствие преждевременных release digests;
- typed blockers и exact writes plan.

Dry run ничего не пишет и не требует reviewer field. Любое изменение inventory делает результат
stale; JSON нельзя редактировать вручную или применять по digest после изменения source state.

## Materialize

Apply работает в `SERIALIZABLE` transaction, повторно строит тот же план и принимает exact expected
source/plan digest. Он materialize'ит immutable capability/catalog/switch/policy rows, Stage 5 run,
release-plan skeletons и полные assignments. У balance assignments funding identity остаётся
nullable; engine release и Stage 6 parent job в этом independently delivered checkpoint не
создаются. Только после GREEN Stage 5 source/policy materialization отдельный consumer может
запустить Stage 6 по exact plan digest. Active pricing release head не двигается.

Same-version/same-digest replay возвращает `unchanged`. Same-version/different-digest, неполное
покрытие inventory, stale source, policy collision или unsupported runtime capability отклоняются
до commit. B2C/B2B/service/OpenKeys target готовится целиком; partial apply по классам запрещён.

## Evidence для следующих этапов

До Stage 6/8 сохраняются restricted operational artifacts:

- exact inventories;
- dry-run report и plan digest;
- target/recovery plan skeletons, а после Stage 6 — финализированные release manifests;
- durable ACK всех prepared identities.

Migration `packages/db/migrations/0028_pricing_stage5_evidence.sql` заранее создаёт пустое
хранилище этих доказательств: `pricing_stage5_runs_v2` удерживает exact inventory/plan artifacts и
обе пары scan digests, `pricing_stage5_blockers_v2` — типизированные расхождения, а
`pricing_stage5_prepare_acks_v2` — только успешные prepare+readback identities. DB constraint не
даёт принять нестабильные engine/OpenKeys scans или ACK с отличающимся readback digest. Наличие
таблиц не запускает planner, не создаёт release/control job и не двигает head.

Migration `packages/db/migrations/0029_pricing_release_two_phase_finalize.sql` разрешает честное
двухфазное состояние: Stage 5 run и release plans могут хранить nullable final identities, а
balance assignment — nullable funding generation. Guard triggers сохраняют source/policy plan
immutable, разрешают только переход funding generation `NULL → positive`, запрещают замену уже
установленной identity и не дают перевести release в `prepared`, пока assignment graph не полный и
не совпадает один-к-одному с ready Stage 6 rows. После engine prepare/readback обе release identity
и assignments замораживаются. Миграция ничего не запускает и не касается live money rows.

Stage 5 не меняет цены, балансы, ключи и доступ. Live behavior меняется только single-head CAS на
Stage 9 по `docs/commerce/MULTI_DISCOUNT_STAGE9.md`.
