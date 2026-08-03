# Модель веток claude-api

**Trunk + ветка-владелец на компонент.** `master` интегрирует всё и всегда собирается; у каждого
компонента — своя долгоживущая ветка, где идёт сфокусированная работа над ним. Так и человек, и
нейросеть сразу видят, «где что делается».

## Ветки

| Ветка | Владеет | Назначение | Куда мёржится |
|---|---|---|---|
| `master` | — | Интеграция и production trigger. Всегда зелёная (`cargo build`). Изменения попадают только через `deploy/agent-merge.sh`; прямых коммитов нет. | — |
| `comp/registry` | `crates/registry` | Реестр подписок (БД, схема, CRUD, миграции). | `master` |
| `comp/pool` | `crates/pool` | Пул и ротация (выбор, cooling, состояние лимитов). | `master` |
| `comp/forward` | `crates/forward` | Форвардинг /v1/*, инжект identity, поллер, стрим. | `master` |
| `comp/server` | `crates/server` | Композиция: env-конфиг, CLI, роутер, фоновые циклы. | `master` |
| `comp/authbot` | `crates/authbot` | Пополнение пула: Telegram-бот покупки Claude/ChatGPT-подписок. | `master` |

На каждой `comp/*`-ветке лежит **`BRANCH.md`** — что она делает, границы, как собрать/проверить.
Checkout ветки → сразу видно её назначение.

## Правила

1. **Изменение компонента → task-ветка от `origin/master`** в отдельном worktree (канон
   процесса — корневой `AGENTS.md`). `comp/*` — долгоживущие ветки-владельцы для накопительной
   работы над компонентом; их синхронизация с `master` — отдельная операция вне типового цикла.
   Ветку берут в **отдельный управляемый worktree** (`deploy/agent-worktree.sh create`), а не
   переключением текущего каталога: в одном каталоге может работать другой агент, а сырой
   `git worktree add` не оставляет lifecycle-метаданных для безопасной аварийной уборки.
2. **Границы крейта соблюдаются** (см. корневой `CLAUDE.md` и `crates/<x>/CLAUDE.md`). Ветка
   `comp/pool` не должна тащить сеть; `comp/forward` — не читать env; и т.д.
3. **`master` = production trigger.** Мёрж только через `deploy/agent-merge.sh` и только когда
   изменение полностью готово к production. До gate скрипт отклоняет красный target и ребейзит
   ветку на последний закоммиченный `master`, затем параллельно запускает fail-closed локальный
   path-aware gate и trusted host-validation точного feature SHA, переиспользуя credential из
   `git credential` и не перекладывая токен/доказательство на человека. Локальные shell/whitespace
   проверки выполняются всегда; language/deployment lanes выбираются по diff, а неизвестный путь
   или изменение gate включает их все. Обычный TypeScript diff проверяется по dependency-aware
   workspace closure; shared inputs и удаления включают полный workspace, а Next.js cache
   переиспользуется между кандидатами. Pending target может перекрываться с этими
   проверками, но push разрешён только после зелёной повторной проверки под lock. Оба результата
   действуют только для неизменившегося SHA. Скрипт сериализует мёржи машинным локом, поэтому два
   production-кандидата никогда не деплоятся друг поверх друга. Тот же замороженный host-кандидат
   переиспользуется после push в `master`; затем watchdog выполняет migration-before-app и
   blue-green deploy, а итог виден в `deploy/watchdog`.
4. **Кросс-компонентная задача** (например, поменяли контракт `Sub` в registry и его потребителей):
   разбей по владельцам последовательными мёржами ИЛИ веди одной task-веткой от `origin/master`
   с явным описанием в коммите. Прямые коммиты в `master` запрещены — мёрж только через
   `deploy/agent-merge.sh`.
5. **Синхронизация:** перед работой `git fetch`. Ветки синхронизирует человек; агент не вливает
   `master` в свою ветку сам — `deploy/agent-merge.sh` ребейзит его ветку в момент мёржа.
6. **Миграция сначала:** новый append-only expand migration добавляется до зависящего от неё кода;
   историю миграций не редактировать. Полный contributor/AI workflow — `CONTRIBUTING.md`.

## Типовой цикл

Ветка не изолирует — изолирует worktree. Над репозиторием параллельно работает несколько агентов,
поэтому переключение веток в общем каталоге запрещено: оно переписывает файлы под соседом и
переносит его незакоммиченные правки на твою ветку.

```bash
worktree=$(./deploy/agent-worktree.sh create feat/forward-<task> forward-<task>)
cd "$worktree"                      # дальше работаем только здесь
# … правки строго в crates/forward …
cargo build                          # зелёно
git add crates/forward               # только свои пути, никогда git add -A
git commit                           # Conventional-заголовок + подробный body (см. AGENTS.md)
git push -u origin HEAD
./deploy/agent-merge.sh              # сериализованный мёрж в master; вручную — нельзя
```

Закончил задачу — после зелёного `deploy/watchdog` агент из основного клона запускает
`deploy/agent-worktree.sh finish <путь>`. Скрипт сам проверяет clean+merged, делает допустимый
ff-only локального `master` и убирает только выбранные worktree/ветку. Чужие worktree не трогать:
для глобального состояния используются безопасный `doctor` и dry-run `gc`, а `gc --apply` оставлен
оператору или плановому maintenance-процессу. На macOS пропущенную уборку может безопасно
подхватить постоянный LaunchAgent `DELETE_WORKTREE` (`docs/ops/DELETE_WORKTREE.md`).

## Создание веток (первичная настройка)

```bash
for c in registry pool forward server authbot; do
  git branch comp/$c master         # ответвить от master
done
# затем на каждой добавить свой BRANCH.md (см. историю коммитов)
```
