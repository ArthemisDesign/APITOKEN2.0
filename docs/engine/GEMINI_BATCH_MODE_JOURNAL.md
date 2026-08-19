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
