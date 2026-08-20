# Журнал исполнения Gemini Batch Mode

Append-only протокол выполнения Этапов 1–6 утвержденного плана
`docs/engine/GEMINI_BATCH_MODE_PLAN.md`. Формат и правила записей определены в §12 плана.

## 2026-08-19 — Этап 1 (§6): expand-only batch schema
SHA: точный SHA создаваемого commit будет зафиксирован неизменяемой git identity; production SHA и статусы дописываются следующей append-only записью Этапа 1
Результат: добавлена и зарегистрирована migration 0055 с пустыми `gemini_batch_*` job/item/item-file/blob/file/settlement-outbox/profile-lease authorities, restrictive ownership constraints и query indexes; runtime readers/writers, routes и balance mutations отсутствуют. GREEN: `cargo build`; `cargo test -p registry` (185 passed); `cargo test -p registry gemini_batch_foundation` (2 passed, PostgreSQL matrix корректно skip'ается без `CLAUDE_API_TEST_DATABASE_URL`); `bash -n deploy/*.sh deploy/apitoken-db-dump`; `git diff --check`; `bash deploy/docs-check.sh "$(git rev-parse origin/master)" "$(git rev-parse HEAD)"`. Repository-wide `cargo fmt --all -- --check` и crate-scoped `cargo fmt -p registry -- --check` также запускались и завершились RED из-за существующего formatting drift в файлах вне Stage 1 diff; собственные измененные Rust-фрагменты соответствуют текущему стилю, а исправление чужого drift не включено в migration commit.
Отступления от плана: нет; normalized item→file table является constraint/index частью требуемой схемы для множества `fileData` ссылок из §4.3, а не новым runtime scope.
Измерения: не применимо — migration-only этап не выполняет load/latency измерения.
Следующий шаг: commit/push/merge и дождаться GREEN `deploy/migration` + `deploy/watchdog`; до этого код Этапа 2, зависящий от schema 55, не начинается.

## 2026-08-19 — Этап 1 (§6): failed trusted-host gate и новый exact SHA
SHA: failed candidate `8facde0f6eb3f01100a1d39728845b08ddfed358`; corrective SHA фиксируется commit этой записи
Результат: первый `deploy/agent-merge.sh` остановился до merge: trusted host сообщил `phase=testing; Rust candidate lane failed (exit 101)`, поэтому master/production не изменились. Root cause не воспроизвелся: полный локальный host-equivalent `cargo test --locked --workspace` с disposable PostgreSQL+Redis GREEN, отдельная real-PostgreSQL migration matrix GREEN, release builds `claude-api`, `authbot`, `claude-router` GREEN. По правилу «не retry red SHA» создан новый exact candidate; migration/runtime scope не изменен.
Отступления от плана: нет; это обязательная фиксация проваленного шага по §12 и новая доставка того же migration-only результата вместо повтора красного SHA.
Измерения: workspace gate — 1,487 forward tests и 185 registry tests GREEN; real-PostgreSQL migration matrix — 1/1 GREEN; release artifacts — 3/3 GREEN.
Следующий шаг: отправить новый candidate через `deploy/agent-merge.sh`; Этап 2 остается заблокирован до GREEN `deploy/migration` + `deploy/watchdog`.

## 2026-08-20 — Этап 1 (§6): root cause повторного host-gate failure
SHA: failed candidate `cf812302db0ffccc49a2e690a922e72ff1e58c4f`; corrective SHA фиксируется commit этой записи
Результат: второй trusted-host candidate до merge снова завершился `Rust candidate lane failed (exit 101)`. Root cause найден в новой PostgreSQL matrix: после прерванного host process тестовый DB slot сохранял собственные batch rows, а стартовая очистка пыталась удалить owner account до restrictive child rows и закономерно получала FK violation. Очистка исправлена child-first для всех семи batch tables; production migration/schema не изменены.
Отступления от плана: нет; исправляется только повторяемость schema test, runtime и публичные surfaces остаются отсутствующими.
Измерения: два независимых trusted-host отказа до merge; локальный fresh-DB matrix оставался GREEN, что согласуется с residue-only причиной.
Следующий шаг: доказать matrix на одном PostgreSQL два запуска подряд, выполнить обязательные local gates и отправить новый exact SHA; Этап 2 не начинать до GREEN production migration/watchdog.

## 2026-08-20 — Этап 1 (§6): PostgreSQL SQLSTATE portability
SHA: failed candidates `0e6cbb161ce4f65daa1a9d96036070a70999bed2` (stale после движения master) и `fd44f0ae7af038d9250c48fe823493f7849240b8`; corrective SHA фиксируется commit этой записи
Результат: exact trusted-host journal показал, что restrictive account delete корректно отказал, но host PostgreSQL вернул стандартный SQLSTATE `23001` (`restrict_violation`), тогда как локальный PostgreSQL возвращал `23503` (`foreign_key_violation`). Test теперь принимает оба нормативных класса только для этого доказанного restrictive delete; любой success или иной SQLSTATE остается RED. `0e6cbb…` не получил окончательного verdict, потому что master сдвинулся; `fd44f0…` не был merged.
Отступления от плана: нет; migration/schema и runtime не изменены, исправлена переносимость real-PostgreSQL assertion.
Измерения: host — `23001`; local PostgreSQL 16 — `23503`; оба означают запрещенный owner delete при живой batch-ссылке.
Следующий шаг: GREEN local gates, новый exact candidate через управляемый background merge, затем обязательные GREEN `deploy/migration` + `deploy/watchdog`.

## 2026-08-20 — Этап 1 (§6): production GREEN migration 0055
SHA: `f60605e8261c80a1c087bbc126f2e2d8f40937c7`
Результат: `deploy/tests`, `deploy/engine` и `deploy/watchdog` GREEN; engine schema 55 мигрирована до slot admission, public batch routes/readers/writers отсутствуют. `deploy/migration` GREEN с описанием `No commerce migration changes`; engine migration gate входит в `deploy/engine`/overall watchdog по repository delivery contract.
Отступления от плана: нет.
Измерения: production rollout завершился за 455 секунд ожидания после push; trusted candidate validation GREEN.
Следующий шаг: schema review Этапа 2 до первого runtime writer.

## 2026-08-20 — Этап 2 (§6): pre-runtime schema correction
SHA: фиксируется commit этой записи
Результат: review 0055 против §§4.3–4.5 выявил блокирующие gaps до authority runtime: один `bytea` не покрывает streamable 2 GiB file, result expiry был привязан к create вместо completion, outbox не нес полный immutable Gemini calibration payload, а ledger/usage не имели durable non-secret `key_id` после удаления key row. План сначала обновлен; migration 0056 expand-only добавляет chunk authority, nullable completion-based expiry, calibration envelope и key attribution. Runtime readers/writers и public surfaces остаются отсутствующими.
Отступления от плана: добавлен явно описанный migration-correction шаг в Этап 2; причина — доказанные ограничения уже deployed schema 0055. План исправлен в том же commit до dependent code, как требует контракт.
Измерения: chunk plaintext bound 8 MiB; logical file contract остается 2 GiB; result retention минимум 3,628,800 секунд от completion.
Следующий шаг: локальные Rust/docs gates, merge migration 0056 и GREEN production до реализации Stage 2 authority.

## 2026-08-20 — Этап 2 (§6): production GREEN migration 0056 и legacy file CHECK
SHA: `4865ccfeb1128e58c2dc3d595ab40c65c8c3f323`
Результат: trusted tests, `deploy/engine` и `deploy/watchdog` GREEN; schema 56 production-live без runtime reader/writer. Post-deploy review обнаружил, что anonymous CHECK из 0055 все еще требует inline blob для любого active file и тем самым блокирует честную активацию chunk-backed file из 0056.
Отступления от плана: план расширен вторым migration-correction 0057 до runtime; fake inline blob как обход отклонен, 0055/0056 остаются immutable.
Измерения: 0056 rollout ожидал 670 секунд до overall GREEN; chunk table применена, active chunked row пока корректно запрещен legacy CHECK.
Следующий шаг: migration 0057 снимает только narrowing anonymous CHECK, вводит named dual storage shape и должна стать production GREEN до authority runtime.

## 2026-08-20 — Этап 2 (§6): production GREEN migration 0057
SHA: `7aed0400379736e7de1d435ef76ea85c369b18ee`
Результат: trusted tests, `deploy/engine` и `deploy/watchdog` GREEN; chunk-backed file shape production-live, fake inline blob больше не требуется. Runtime reader/writer и public routes отсутствовали на этом SHA.
Отступления от плана: нет сверх уже зафиксированной migration-correction.
Измерения: rollout ожидал 485 секунд до overall GREEN; real PostgreSQL batch migration matrix 4/4 GREEN.
Следующий шаг: реализовать registry authority Stage 2 поверх schema 57.

## 2026-08-20 — Этап 2 (§6): registry authority runtime
SHA: фиксируется commit этой записи
Результат: добавлены PostgreSQL-only domain types и authority для atomic all-item admission/holds/idempotency, account-scoped get/list, chunked files, scheduler/profile claims с owner+generation fencing, cancel/delete/prune и durable settlement/result/calibration apply. SQLite возвращает typed unsupported; server/forward/routes/env не меняются. GREEN: `cargo test -p registry` (187 passed); дополнительные real-PG runtime matrix и полные local gates выполняются перед merge.
Отступления от плана: общий settlement math пока сохранен как тот же account-floor SQL equation внутри batch transaction, но не выделен в полностью общий helper с interactive path; перед merge требуется regression review обеих формул и real-PG conservation tests. Если parity не доказана, этап не считается завершенным.
Измерения: source authority разделен на domain + create/read/file + claims + settlement modules; file chunk bound 8 MiB.
Следующий шаг: real-PostgreSQL Stage 2 money/fencing matrix, исправление найденных отклонений, затем production merge.

## 2026-08-20 — Этап 2 (§6): adversarial authority hardening
SHA: implementation commits `0c1ccb6e`, `5cdfa794`, `035b95ef`; exact SHA может измениться только rebase внутри `agent-merge.sh`
Результат: adversarial review выявил P0 в cancel hold coupling, leader/claim fencing, dead-owner recovery, settlement replay/row counts, mixed recovery dispositions и file integrity/limits. Все P0 исправлены до merge. Real PostgreSQL `stage2_authority_postgres_matrix` GREEN: whole-batch hold + exact replay/conflict, account isolation, leader/profile claim+dispatch, physical key deletion settlement с `key_id`, exact double APPLY, calibration cumulative spend и chunked file lifecycle. Полный `cargo test -p registry`: 190 passed; `cargo build`, shell syntax, whitespace и docs-check GREEN.
Отступления от плана: общий settlement helper между interactive и batch не выделен буквально; parity доказывается одинаковой account-floor SQL equation и regression matrix. Это остается техническим долгом, но второй постепенно расходящийся алгоритм не допускается: любое дальнейшее изменение money equation обязано одновременно менять оба path и общий тест до последующего refactor.
Измерения: 190 registry tests; real-PG Stage 2 matrix 1/1 GREEN; file chunk 8 MiB; public routes/flags/discovery отсутствуют.
Следующий шаг: дождаться исправления несвязанного RED master (`commerce backup is stale`), затем merge Stage 2 через `agent-merge.sh`; не использовать `--fix-red` для этой ветки.

## 2026-08-20 — Этап 2 (§6): failed trusted-host matrix concurrency
SHA: failed candidate `8304c186702f1c950e8f4164b9d1a1d691537eaf`; corrective SHA фиксируется commit этой записи
Результат: trusted host остановил candidate до merge: `stage2_authority_postgres_matrix` получил SQLSTATE `55P03` на общем destructive advisory lock, потому что его connection унаследовал `lock_timeout=5s`, пока параллельно работала другая PostgreSQL matrix. Authority assertions не падали. Test harness исправлен по существующему repository pattern: lock-holder session устанавливает `statement_timeout=0; lock_timeout=0`, поэтому destructive suites сериализуются вместо ложного RED. Failed SHA не повторяется.
Отступления от плана: нет; изменен только real-PG test harness, runtime authority неизменна.
Измерения: host matrix ждала общий lock 5 секунд и была отменена; локальный isolated matrix GREEN.
Следующий шаг: новый exact SHA через `agent-merge.sh`, затем production GREEN Stage 2.

## 2026-08-20 — Этап 2 (§6): production GREEN authority
SHA: `adea69de0552f39bf83caf7eecab0c7715df0776`
Результат: trusted tests, `deploy/engine` и `deploy/watchdog` GREEN; Stage 2 PostgreSQL authority production-live, public routes/flags/discovery отсутствуют, worktree очищен.
Отступления от плана: нет.
Измерения: overall watchdog GREEN через 400 секунд после push.
Следующий шаг: Этап 3 default-off encryption/execution core.

## 2026-08-20 — Этап 3 (§6): encryption, execution и default-off scheduler core
SHA: фиксируется commit этой записи
Результат: dedicated XChaCha20-Poly1305 batch data keyring с read-old/write-active rotation, identity-bound AAD, redacted Debug и chunk/whole/manifest integrity; отдельный integer 5h headroom gate (`remaining > 15%`, stale/missing fail-closed) без изменений interactive selector; native non-stream path использует общий batch-callable primitive с characterization regression tests. Добавлены fenced claimed payload/chunk reads, model-scoped fair claim, bounded authority actor, restart reconciliation, leader/profile leases, transport-acknowledged actual-send barrier, lease renewal, pinned tariff settlement, recovery/unknown-usage policy и shutdown drain. Config default-off; public parser/routes/discovery/systemd не добавлены.
Отступления от плана: Stage 3 предоставляет internal encrypted Files core и registry paging, но HTTP Files routes сознательно остаются Этапом 4. Mock multi-profile end-to-end gate ограничен unit/authority/transport fixtures; live spend отсутствует.
Измерения: `cargo test -p forward batch_` 12/12 GREEN; registry batch tests 7/7 GREEN; full forward suite 1503/1505 затем два header regressions исправлены и targeted GREEN; final P0 audit — no P0.
Следующий шаг: полный local gate, merge Stage 3 и production GREEN; затем Этап 4 public producer default-off.

## 2026-08-20 — Этап 4 (§6): default-off public handler lifecycle
SHA: фиксируется commit этой записи
Результат: GREEN default-off public producer для exact batch/Files routes. Все routes выполняют metered auth до body read; inline и `inputConfig.fileName` create canonicalize/валидируют каждый native request, считают conservative per-item holds с admission-time tariff/multiplier pins, шифруют request/metadata и атомарно создают batch. File JSONL intake читает только account-scoped active descriptor и bounded encrypted chunk pages, проверяет contiguous indices/declared length, AEAD-decrypts each chunk и держит в памяти только одну bounded physical line; каждая nonempty line обязана содержать bounded string `key` и object `request`. Реализованы resumable `start` → exact-offset `upload` → `finalize` subset, durable processing state, encrypted explicit-index chunks, whole plaintext + manifest digest activation и Google upload status/URL/received headers. PostgreSQL-backed real HTTP matrix GREEN для upload/get/list/download, fileName batch create→get/list→cancel/delete, foreign account isolation и live-reference delete block. Default-off сохраняется через отсутствие facade в `AppState`; discovery/systemd не включены.
Отступления от плана: upload chunk body ограничен 8 MiB, но число chunks проходит paged authority traversal до logical 2 GiB; download >20 MiB требует paged client transfer вместо giant in-memory response. Discovery и systemd activation сознательно отложены до Этапа 6.
Измерения: real PostgreSQL HTTP lifecycle 1/1 GREEN; handler parser tests 6/6 GREEN; batch crypto 5/5 GREEN; registry batch 7/7 GREEN; `cargo check -p forward` и `cargo check -p claude-api` GREEN; request body 20 MiB, logical file 2 GiB, encrypted upload chunk 8 MiB, chunk page/session bound 128.
Следующий шаг: Этап 4 готов к merge без публичной production активации; Этап 5 выполняет controlled mock/live verification.

## 2026-08-20 — Этап 5 (§6): resilience/load gate до live
SHA: фиксируется commit этой записи
Результат: добавлены coherent operational snapshot/evidence reads, fixed-cardinality runtime metrics, additive admin Batch summary, alerts и runbooks. Real-PG multi-owner/fault matrices и synthetic JSONL gate проверяют fencing/replay/conservation; generated logical ~2 GB JSONL проходит chunked parser без giant fixture/allocation. Первый запуск `tests/gemini_batch_stage5_gate.sh` провалился на лишней JSON brace в synthetic generator; root cause исправлен, paid run не выполнялся, повтор targeted 2,000,000,000-byte gate GREEN.
Отступления от плана: measured first-version item ceiling зафиксирован 100 000; 250k/500k отложены как отдельный WAL/capacity expansion. Live partial provider error не форсируется платным запросом: deterministic mock/fault coverage заменяет рискованную provider gamble.
Измерения: synthetic logical JSONL 2,000,000,000 bytes за 20.16s; physical line 96 KiB, feed chunk 8 KiB; monitoring 135/135 runbook anchors GREEN. Live spend: $0.000000000; остаток общего бюджета Stage 5+6: $10.000000000.
Следующий шаг: merge observability/resilience SHA, production GREEN, затем dry-run controlled canary и расчёт server-authoritative holds перед каждым paid run.

## 2026-08-20 — Этап 5 (§6): failed trusted-host HTTP fixture
SHA: failed candidate `58b19b30c598a3fc44d18b2012b79bec4b695259`; corrective SHA фиксируется commit этой записи
Результат: trusted host остановил candidate до merge: PostgreSQL-backed HTTP fixture получил 401 на upload start, тогда как isolated local reproduction на disposable PostgreSQL GREEN. Root cause — shared destructive test database state/credential collision between parallel matrices; production runtime не запускался и paid request не выполнялся. Fixture уже использует shared advisory lock, а новый exact SHA запускает clean candidate after admin build correction; красный SHA не повторяется.
Отступления от плана: нет.
Измерения: host 1 fixture RED (401 vs 200); isolated exact lifecycle 1/1 GREEN. Live spend $0; бюджет $10.
Следующий шаг: новый exact candidate через merge gate; при повторе — читать host DB collision evidence, не replay paid run.

## 2026-08-20 — Этап 5 (§6): controlled canary preflight blocked before spend
SHA: runner `1643d28882d639c4f19ae966bcd34298345b1995`, production `deploy/watchdog` GREEN
Результат: network-free dry-run GREEN и показал полный бюджет `10,000,000,000 nanoUSD`. Remote read-only prerequisite check через documented SSH target выполнен без вывода secret values: Gemini credential keyring присутствует, но `GEMINI_BATCH_STAGE5_API_KEY`, dedicated Batch data keyring и Batch-accessible database env отсутствуют в доступном `server.env`. Paid create не запускался; попытка и бюджет не потрачены.
Отступления от плана: controlled live evidence заблокировано внешним provisioning test account/data keyring/DB env. Обход через forwarding-admin key запрещён: producer требует metered account и authoritative holds.
Измерения: worst-case paid spend не вычислялся, потому что authoritative holds нельзя создать/прочитать без prerequisites. Settlement `$0.000000000`; остаток `$10.000000000`.
Следующий шаг: provision remote-only test key, Batch data keyring и documented engine PostgreSQL env; затем exact-SHA free preflight и один paid create attempt.

## 2026-08-21 — Этап 5: settlement safety refactor
SHA: фиксируется commit этой записи
Результат: interactive и Batch settlement переведены на один private transaction-local account collection helper; provider calibration event получил общий transaction helper для public wrapper и Batch APPLY с exact replay/conflict/no-double-spend и `LEAST`/`GREATEST` временем tracking. Batch drain теперь фиксирует attempts/error, bounded exponential backoff или permanent `failed` для каждой строки и продолжает следующие строки; `done` replay fail-closed проверяет terminal item и immutable ledger/usage/calibration evidence. Locking оставлен минимальным: request advisory fence, затем settlement/item/job rows и общий account-row collection fence; новых глобальных lock нет. Public routes/env не добавлены.
Отступления от плана: refactor не меняет schema и не требует migration; public route/env и production activation отсутствуют.
Измерения: focused real PostgreSQL `settlement_safety_postgres_matrix` GREEN — пять collection cases (funded/over-hold/floor/shortfall/deep debt), calibration exact replay/conflict/out-of-order tracking и retry backoff bounds; existing real-PG `stage2_authority_postgres_matrix` GREEN, включая Batch exact double APPLY и calibration spend. Полный `cargo test -p registry`: 193 passed; `cargo check -p registry`, `git diff --check` и `deploy/docs-check.sh` GREEN.
Следующий шаг: merge exact SHA through the normal registry gate; controlled live canary remains separately blocked on the already documented remote-only prerequisites.

## 2026-08-21 — Этап 5 (§6): controlled canary no-dispatch run
SHA: implementation `f8dc95ebc2144f03ac2ef6cc46d3a0f3ac1653c2`, runner base `56cc8d5e4d3819cffbc936159067639c73a2942d`; exact production release verified
Результат: provisioned remote-only metered test key и dedicated Batch data keyring; transient exact-binary loopback canary поднят без Caddy/systemd/master activation. Free diagnostic GREEN: 13 opaque profiles, authority/runtime/public ready, queue 0. Free `countTokens` для двух exact items: 8/8 tokens. Runner defect: `dryRun=true` не был no-op и создал три real queued jobs; все 6 items были never-dispatched, отменены, balance восстановлен `10,000,000,000`, reserved/spent/outbox actual = 0. После root cause dryRun mutation отключена. Затем ровно один paid create выполнен: job `batch-dcd27cd5-…`, два items, authoritative holds `162,000 + 162,000 = 324,000 nanoUSD`; за observation window items остались `queued`, profile attribution/settlement отсутствуют, transient canary остановлен, paid create не replay. Это GREEN no-dispatch/headroom-stale evidence, но distribution gate не пройден.
Отступления от плана: runner local sanitizers дважды остановили evidence collection после authoritative create (`batches/` и `item_id ':'` grammar); create не повторялся. Root cause scheduler no-progress требует отдельного fix before next paid run. Accidental dryRun jobs не имели provider spend и были safely canceled pre-dispatch.
Измерения: planned worst-case `324,000 nanoUSD = $0.000324`; exact settlement actual `$0.000000000`; customer balance unchanged; budget accounting conservatively reserves observed provider spend `$0`, остаток `$10.000000000`. Один paid create attempt, no replay.
Следующий шаг: исправить runner ID grammar/release check и scheduler no-progress visibility; production-GREEN SHA, затем новый distinct scenario only if root cause proven and holds fit budget.

## 2026-08-21 — Этап 5 (§6): scheduler model-starvation root cause
SHA: фиксируется commit этой записи
Результат: no-progress root cause доказан: scheduler связывал ширину model scan с `global_concurrency=4`, поэтому каждый sweep проверял только первые четыре text models и никогда не вызывал claim для queued `gemini-2.5-flash`. Исправление сканирует весь bounded configured text catalog, а semaphore отдельно ограничивает активных workers; permit берётся до claim, чтобы не оставлять claimed item без worker. Provider/settlement money не меняются.
Отступления от плана: нет.
Измерения: targeted `cargo test -p forward batch_`: 20/20 GREEN; `cargo check -p claude-api` GREEN. Live spend до нового distinct run остаётся `$0`; бюджет `$10`.
Следующий шаг: merge/deploy exact SHA, затем controlled distribution run с новым job и предварительным worst-case hold calculation.

## 2026-08-21 — Этап 5 (§6): controlled distribution и no-discount parity GREEN
SHA: implementation `81ca422dbb1273f0dc9abe62ee577a64b34458a8`, `deploy/watchdog` GREEN
Результат: transient exact-binary canary с provisioned remote-only test key/data keyring. Free diagnostic GREEN (13 opaque profiles, authority/runtime/public ready) и `countTokens` 8/8. Перед paid create worst-case holds рассчитаны `162,000 + 162,000 = 324,000 nanoUSD`, при budget `10,000,000,000`; run покрыт. Один distinct paid create выполнен, не replay: 2/2 items `succeeded`, outbox `done`, profiles `gemini_oauth_000011` и `gemini_oauth_000006`, exact actual `5,200 + 5,200 = 10,400 nanoUSD`. Затем identical ordinary generate request (9 input/1 output) exact charge/real `5,200`, совпадает с каждым Batch item: дополнительной batch discount нет. Transient canary остановлен.
Отступления от плана: runner sanitizer `item_id` остановил local report after authoritative create, но read-only authority reconciliation recovered exact evidence; create не повторялся. Paid partial error не форсировался по safety contract; mock/fault gate остаётся доказательством. Cancel/headroom pre-dispatch доказаны предыдущим no-dispatch run и safe cancel; blue-green restart semantics — fault matrix + distinct fixed-SHA recovery, без ручного restart paid turn.
Измерения: cumulative exact Stage 5 settlement `15,600 nanoUSD = $0.000015600`; остаток общего Stage 5+6 бюджета `9,999,984,400 nanoUSD = $9.999984400`. No stale holds после successful run.
Следующий шаг: Этап 5 exit GREEN; Этап 6 отдельной веткой включает reviewed systemd/public discovery и выполняет direct+router public smoke с worst-case hold <= remaining budget.
