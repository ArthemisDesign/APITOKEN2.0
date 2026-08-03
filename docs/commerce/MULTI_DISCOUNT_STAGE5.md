# Stage 5 — materialization целевого pricing release

Статус: двухфазный consumer реализован в
`packages/db/src/pricing-stage5-materializer-v2{,-store}.ts` после отдельных GREEN producer и
migration checkpoint. OpenKeys authoritative cursor, admin-managed service inventory, compile-fixed
runtime capability generations 3/4 и migration `0029_pricing_release_two_phase_finalize.sql` уже
являются его deployed prerequisites. Stage 5 готовит immutable source/ownership/policy authority,
но не угадывает меняющиеся funding identities и не меняет live traffic.

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
canonical aggregate inventory digest. До cutover (`provisioning-context=null`) mutation не создаёт
release-v2 artifacts. После cutover она до записи inventory готовит rule-free service policy с
`billing_mode=meter_only`, без product/catalog/switch pins, и exact active/recovery assignment
extension с purpose/responsible; prepare ACK, GET readback и свежий context обязаны совпасть.
Mutation не создаёт engine account, release или activation и не двигает global head.
Exact replay существующей service identity остаётся `unchanged`; смена metadata уже immutable
active assignment требует следующей release generation и не переписывается на месте.

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
восемь тарифно закреплённых Gemini-моделей. Additive generation 4 добавила
`gemini-3-flash-preview`, но не стала target после failed production gate. OpenKeys target
по-прежнему намеренно сохраняет Anthropic/OpenAI набор:
Gemini появится там только отдельной явной OpenKeys catalog generation и всё равно будет 1:1.
Capability publication не является таким enablement. После failed production generation gate
generation 4 остаётся immutable rejected artifact: Stage 5 не должен materialize или финализировать
target/recovery plan на её digest. До новой additive capability используется generation 3.

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

Dry run ничего не пишет и не требует reviewer field. Любое изменение стабильной inventory identity
делает результат stale; JSON нельзя редактировать вручную. Движущиеся деньги намеренно не входят в
plan digest: apply сохраняет свой свежий полный snapshot как evidence, а replay того же immutable
плана не подменяет уже записанное evidence более поздними balance/reserved/spent значениями.

Штатная команда запускается только из защищённого runtime-окружения с `DATABASE_URL`,
`ENGINE_BASE_URL`, `ENGINE_CONTROL_KEY`; OpenKeys читается напрямую на loopback
`OPENKEYS_INTERNAL_BASE_URL` (по умолчанию `http://127.0.0.1:3410`) с отдельным
`OPENKEYS_CONTROL_KEY` либо тем же server credential:

```bash
pnpm --filter @claude-api/db pricing:stage5-v2 dry_run
pnpm --filter @claude-api/db pricing:stage5-v2 apply sha256:v2:<exact-dry-run-plan>
```

Engine cursor исчерпывается дважды. Стабильность Stage 5 identity включает `account_id`, status и
legacy scalar multiplier, но намеренно не включает меняющиеся `balance/reserved/spent` и funding
head: полные денежные snapshots сохраняются как evidence, а их финальная identity принадлежит
Stage 6. OpenKeys cursor также исчерпывается дважды и обязан вернуть один неизменный full-manifest
digest на всех страницах обоих проходов.

## Materialize

Apply работает в `SERIALIZABLE` transaction, повторно строит тот же план и принимает exact expected
source/plan digest. Он materialize'ит immutable capability/catalog/switch/policy rows, Stage 5 run,
release-plan skeletons и полные assignments. У balance assignments funding identity остаётся
nullable; engine release и Stage 6 parent job в этом independently delivered checkpoint не
создаются. Только после GREEN Stage 5 source/policy materialization отдельный consumer может
запустить Stage 6 по exact plan digest. Active pricing release head не двигается.

Local plan сначала фиксируется под advisory lock с повторной проверкой commerce/service snapshot;
та же проверка обязательна перед сохранением terminal blocker evidence. Затем consumer делает
только dormant engine prepare для main/OpenKeys catalog generation 3, provider switches generation
3 и каждой v2 policy, немедленно читает exact version обратно и фиксирует ACK лишь для
`stored|unchanged` с совпавшим digest. Materializer строит capability projection, оба каталога,
switches, customer и service policies только на generation 3. Rejected generation 4 остаётся
compile-fixed immutable history, не входит ни в один Stage 5 target/recovery artifact и не получает
фиктивный capability ACK. Target/recovery release prepare, recovery link и control job до Stage 6
отсутствуют.

Same-version/same-digest replay возвращает `unchanged`. Same-version/different-digest, неполное
покрытие inventory, stale source, policy collision или unsupported runtime capability отклоняются
до commit. B2C/B2B/service/OpenKeys target готовится целиком; partial apply по классам запрещён.
Стабильный план с ownership blockers может сохранить только terminal `blocked` run и typed blocker
rows; catalog/policy/release skeleton и remote prepare при этом не создаются. Нестабильные парные
сканы не сохраняются как ложное evidence и требуют нового полного прохода.

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
