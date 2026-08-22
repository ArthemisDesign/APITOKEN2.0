# Тестовый стенд (staging) — план внедрения

> **Статус: PROPOSAL. План, а не инструкция и не implementation contract.** Составлен 2026-08-16
> на основе анализа текущей модели доставки (`CONTRIBUTING.md`, `docs/ops/INFRASTRUCTURE.md`,
> `docs/ops/DEPLOYMENT.md`, `docs/ops/MONITORING.md`, `deploy/README.md`). Ничего из описанного
> пока НЕ реализовано: ни стенда, ни ветки `stage`, ни новых скриптов. Каждая фаза ниже внедряется
> отдельными коммитами, и каждый коммит, который меняет поведение, описанное в других документах
> (веточная модель, слияние, рунбуки), обновляет эти документы в том же коммите
> ("documentation is a living contract").
>
> **v2 (2026-08-16):** по решению владельца стенд размещается **на том же VPS, что и прод**
> (совместный failure domain принят осознанно). Раздел 5.2 переписан под co-located-вариант:
> изоляция достигается пространством имён (пользователь, каталоги, порты, БД, systemd-юниты)
> и жёсткими ресурсными лимитами, а не отдельной машиной. Прочие разделы (git-модель, конвейер,
> гейт деградации) не зависят от размещения и остались без изменений.
>
> **v3 (2026-08-16):** ревизия по целям владельца: (а) весь основной поток разработки идёт через
> стенд, хотфиксы — в обход; (б) прямая доставка в `master` запрещена без исключений, включая
> «мелочи» (байпас-порог из открытого вопроса №5 отменён, см. раздел 6.5); (в) флот подписок
> один, управляется только прод-контуром, стенд (и хотфикс-кандидаты) получают доступ к его
> емкости через контролируемый shadow-read режим, а не через владение
> (новый подраздел 5.3.1 и решения фазы 0).
>
> **v4 (2026-08-16):** все открытые вопросы закрыты решениями владельца (раздел 11 переписан
> из «открытых вопросов» в «решения фазы 0»): доступ — только SSH-туннель; данные — протекающий
> стенд + reseed по команде; флот — фазы 1–5 только shadow-read, решение о live-endpoint после
> фазы 5; песочница подписок — в фазе 7. Ресурсный бюджет и портовая таблица подтверждены
> живым снятием состояния с прод-хоста (утилизация RAM ~11/96 GB, диск 326/894 GB,
> предложенные порты стенда свободны — см. 11.1).
>
> **v5 (2026-08-16):** исправлена механика запрета прямой доставки в `master` (раздел 6.5).
> Прежняя редакция опиралась на механизм защиты веток на стороне GitHub-хостинга, который для
> этого репозитория недоступен, а `deploy/watchdog-lib.test.sh` запрещает на него полагаться.
> Раздел переписан под трёхслойную защиту, реализуемую собственной инфраструктурой:
> клиентский хук (A) + выделенный merge-кредишенс, отделённый от повседневного
> `git credential` (B) + fail-closed проверка легитимности кандидата в хост-watchdog с алертом
> на прямой пуш (C).
>
> **v6 (2026-08-16):** добавлен подраздел 6.6 «Как этим пользоваться — простым языком» —
> пошаговый рабочий процесс для обычной разработки, хотфикса и запрета прямого пуша,
> без деталей реализации.
>
> **v7 (2026-08-22):** план сверен с фактическим кодом по снимку
> `apisnapshot-ops-20260820-1714` (147 файлов: watchdog, merge/recovery, systemd, sudoers,
> Compose, Control API, smoke/mock, часть Rust). Статус остаётся `PROPOSAL`. Перед
> реализацией документ больше нельзя читать как «поднять вторую линию того же watchdog».
> Добавлен раздел 9 «Границы доверия и протокол допуска». Исправлены блокеры: цикл
> `deploy/watchdog`; запрет candidate root-installer на прод-хосте; caller-bound reporting;
> неизменяемая единица promotion и инвалидация после rebase/hotfix; агрегированный
> `staging.slice`, Docker/disk/network isolation; сначала параметризация production-hardcoded
> контура, затем вторая линия; enforcement только после dry-run и drill. Уточнено, что уже
> есть в проде (exact-SHA marker, immutable artifacts, expand-only миграции, binary/slot
> rollback, окно A/B, hardening юнитов) и чего в плане не хватало.

---

## 1. Резюме

Сейчас есть ровно один стенд — производственный. Локальные тесты, smoke-прогоны с mock-upstream и
exact-SHA валидация кандидата дают сильный, но не полный фильтр: систему целиком (engine + commerce +
worker + router + Caddy + БД) в связке, под нагрузкой и с реальными сценариями деградации проверять
негде. При частых деплоях часть регрессий переживает все гейты и проявляется на клиентах.

Предлагается ввести **второй стенд (stage) — контур-близнец прода**, размещённый **на том же VPS,
что и прод**, но в **реально изолированном** пространстве имён: свой ОС-пользователь, свои корни
каталогов (`/opt/apitoken-staging`, `/srv/claude-api-staging`, …), свой диапазон портов, свой
PostgreSQL-контейнер, свой набор systemd-юнитов, агрегированный `staging.slice`, enforceable
disk quota и отдельный network namespace. Та же сборка из того же SHA и те же шаблоны приложения,
но **не** «тот же watchdog-конвейер с четырьмя env-переменными»: production-контроллеры сейчас
жёстко зашиты под прод-инвентарь, и вторая линия появляется только после явного `contour-config`.
Секреты, БД и (по умолчанию mock-) апстрим — свои. Сетевой доступ закрыт: клиенты не достигают
стенд, стенд не достигает прод-loopback, прод-сокетов и живых payment/mail/provider endpoint
без явно включённого бюджетного lane.

Разработка идёт **stage → prod**: ветка `stage` является триггером стенда. В `master` код
попадает только как **fast-forward того же проверенного SHA** после stage-deployment,
degradation gate, human approval и host-owned `promotion/eligible`. **Хотфиксы** имеют отдельный
быстрый путь сразу в `master`, но только с host-owned attestation `mode=hotfix` и обязательным
последующим сведением `stage` с `master`. Имя ветки `hotfix/*` и commit trailer сами по себе
не являются авторизацией.

Ключевое отличие от обычного "заведи второй сервер": стенд получает **гейт обнаружения
деградации** — постоянную синтетическую нагрузку с профилем прод-трафика и A/B-сравнение синего и
зелёного слотов, плюс пороговые правила по метрикам. Регрессия ловится до прода не эпизодической
проверкой "вручную посмотрели", а автоматическим red-статусом, который блокирует promotion.
Пороги гейта — trusted policy последнего production-approved контроллера, а не версия,
привезённая проверяемым кандидатом.

Документ **нельзя** переводить из `PROPOSAL` в обязательный implementation contract, пока
не закрыты инварианты раздела 9 и Definition of Done раздела 12. Буквальная реализация
редакций v1–v6 создала бы две наиболее опасные регрессии: циклический production admission
через `deploy/watchdog` и выполнение неутверждённого infrastructure candidate как root на
production-host.

---

## 2. Текущая модель доставки (как есть)

Факты, на которых стоит план (детали — в указанных документах; сверка с кодом — снимок
2026-08-20):

1. **Trunk-based.** `master` — единственный триггер производства. В него нельзя коммитить напрямую;
   единственный штатный путь — `git push` + `deploy/agent-merge.sh`, который берёт merge-lock,
   перебазирует ветку, гоняет path-aware гейт, ждёт зелёный `deploy/watchdog`.
2. **Хост-watchdog на проде.** Поллит tip `master` каждые 5 секунд **без** предварительного
   зелёного собственного статуса. В начале цикла публикует `deploy/watchdog=pending`,
   валидирует точный SHA (path-aware: TypeScript/DB лайны, Rust лайны, операционные
   регрессионные сьюты), перед выкаткой снимает валидированные бэкапы БД, применяет
   forward-only миграции, затем blue-green: engine-слоты Anthropic 8787/8788, OpenAI 8793/8797,
   Gemini 8795/8799, API 3000/3001, router 8800/8801; финальная проверка — синтетические пробы
   (`/ready`, contract-пробы OpenAI/Gemini). `deploy/watchdog=success` появляется **только
   после** production deployment и verification. Это уже ломает формулировку «выкатывать только
   при уже зелёном `deploy/watchdog`».
3. **Exact-SHA и неизменяемые артефакты уже реализованы.** После тестов watchdog фиксирует
   полный commit SHA, Git tree, digest migration manifest, версию и digest validation
   policy/plan, выбранные TypeScript/Rust lanes, digest собранных TypeScript bundles и digest
   engine/authbot/router binaries (`deploy/watchdog.sh`). Candidate-каталог затем
   `root:root` без write-битов; перед использованием marker, SHA, tree и digests сверяются
   повторно. Production продвигает уже проверенные артефакты без пересборки
   (`CONTRIBUTING.md`). Недостающая часть для стенда — не новая система артефактов с нуля,
   а связывание этого marker со stage-deployment, degradation result, human approval и
   production promotion.
4. **Кандидат-валидация до мержа.** `deploy/agent-merge.sh` просит прод-хост протестировать точный
   SHA фиче-ветки в изолированном окружении (свои порты, своя БД, свой Cargo target), до попадания
   в `master`. Это де-факто "тест на проде в песочнице", а не отдельный стенд. У скрипта одна
   переменная `AGENT_MERGE_REQUIRED_CONTEXT` (по умолчанию `deploy/watchdog`) сразу для трёх
   ролей: baseline текущего target, precondition кандидата и post-push ожидание. Простая
   подстановка `deploy/stage` эти роли не разделяет.
5. **Миграции expand-only и rollback сильнее, чем следовало из ранних редакций плана.**
   Отдельный expand-only migration commit, ожидание зелёных `deploy/migration` и
   `deploy/watchdog`, затем dependent application commit; старое приложение совместимо с
   расширенной схемой; cleanup контракта — поздний релиз. `deploy/rollback.sh` явно не
   меняет базу: автоматический rollback — это **binary/slot switchback**, не rollback БД.
   Release, ломающий предыдущий binary, не является rollout-safe (`docs/ops/DEPLOYMENT.md`).
6. **Окно для A/B технически уже есть.** API и engine controllers сначала поднимают target
   slot, ждут readiness и Caddy inclusion, и лишь затем переводят old slot в pre-drain.
   Полностью переписывать blue-green не требуется; нужна явная state machine и безопасная
   семантика тестового трафика.
7. **Тесты.** `cargo test` (в т.ч. обязательные money-тесты), pnpm build/typecheck/test,
   интеграционные тесты против disposable PostgreSQL, smoke-сценарии с mock upstream
   (`tests/rotation_fanout_smoke.sh`, `tests/universal_chat_smoke.sh`, `tests/mock_upstream.py`
   с реалистичными unified-ratelimit заголовками и thinking-блоками).
8. **Фронтенд-превью.** Для `apps/web` есть независимый канал: ветки `preview/*` дают Vercel
   pre-production деплой с человеческим ревью до мержа. Для backend/engine/инфраструктуры
   эквивалента нет.
9. **Мониторинг.** Полный стек (Prometheus/Grafana/Loki/Alertmanager + экспортеры) только на проде,
   с `network_mode: host`. Candidate monitoring installer меняет общий stack через root
   infrastructure transaction. Внешний uptime-детектор — GitHub Actions. Метрики, дашборды и
   правила — из репозитория (`observability/`).
10. **Провайдеры.** Флот живых подписок (Claude Max/Pro, Codex, Gemini). Live-гейты новых моделей и
    canary на транзитных loopback-портах выполняются **на прод-хосте**.
11. **Hardening application units уже есть.** Типовые units: `NoNewPrivileges=true`,
    `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`, ограниченные
    `ReadWritePaths` и per-unit `MemoryMax`. Для стенда это база, не «начать с нуля»:
    нужно stage-specific усиление (aggregate slice, network namespace, destination
    filtering, Docker cgroup parent, отдельные overrides).
12. **Infrastructure lane прода исполняет candidate как root.** Root-owned
    `deploy/watchdog-infrastructure.sh` после проверки tested SHA/tree запускает
    `"$CANDIDATE/deploy/install-watchdog.sh"` и `"$CANDIDATE/deploy/install-caddy.sh"`.
    Для production это осознанно: candidate уже tip `master`. Для stage это **нельзя
    зеркалить**: неутверждённый candidate не должен исполнять host-global installer на
    production-host.
13. **Reporting.** Root-owned `deploy/watchdog-github.sh` принимает любой context
    `^deploy/[a-z][a-z0-9-]*$`; Unix-user `deploy` может вызывать helper с произвольным
    context. Отдельный GitHub token для стенда не обязателен, но **тот же широкий
    caller contract выдавать `deploy-stage` нельзя**.
14. **Watchdog production-hardcoded.** `deploy/watchdog.sh` и соседние controllers
    фиксируют `/opt/apitoken`, `BRANCH=master`, `/var/lib/apitoken`, users, caches,
    root helpers, release roots, locks, production state, unit names и ports. Вторая
    линия — это сначала immutable `contour-config` со schema validation и запретом
    пересечения stage/prod inventory, а не `sed` и не четыре env-переменные.

## 3. Недостатки текущей модели

Отсортированы по важности; каждый пункт — конкретный класс риска, который стенд закрывает.

- **Один контур без "среды сборки системы".** Engine, commerce, worker, router, Caddy, БД впервые
  встречаются вместе только в проде. Кандидат-валидация на проде изолирует один SHA, но не
  проверяет систему под нагрузкой и не даёт наблюдать её после выкатки. Ошибки взаимодействия
  компонентов — самая частая причина деградации, которую юнит-тесты не видят.
- **Частые деплои × серийный конвейер = почти нет окна наблюдения.** Каждый merge в `master` — это
  полный прод-цикл, а следующий merge встаёт следом. Между "выкатили" и "выкатили следующее"
  нет обязательного периода соака/наблюдения. Регрессия с отложенным проявлением (медленный рост
  ошибок, деградация под нагрузкой) смешивается со следующим деплоем, и причину инцидента трудно
  атрибутировать.
- **Деградацию не с чем сравнить.** Финальный пост-деплой гейт (`docs/ops/DEPLOYMENT.md`) проверяет
  "живо и отвечает контракту", но не сравнивает метрики до/после выкатки: латентности, долю
  4xx/5xx, поведение ротации, использование слотов. Медленная деградация проходит зелёной.
- **Смоуки завязаны на mock upstream.** Реальное поведение подписок (паттерны 429, пределы окон,
  «util5h»-динамика, SSE-особенности) проверяется только на живом проде после выкатки, либо
  дорогими калибровками по `docs/ops/*_CALIBRATION.md`.
- **Forward-only миграции на проде.** Схему нельзя откатить бинарным rollback — только новым
  корректирующим коммитом. На стенде цена неудачной миграции — ноль для клиентов, и отрабатывается
  совместимость **N-1 binary с post-migration schema**. Автоматический rollback стенда остаётся
  binary/slot switchback и **не** заявляется как rollback БД.
- **Инфраструктурные изменения едут сразу в прод.** `deploy/`, `systemd/`, `observability/`
  обновляются прод-watchdog'ом автоматически, включая candidate-controlled root installers.
  Стенд это **не** повторяет на общем VPS: host-global candidate lane проверяется в песочнице
  (ephemeral VM / systemd-nspawn / отдельный test host), а на production-host применяется
  только обычным production-watchdog после promotion.
- **Предпросмотра бэкенда для человека нет.** Фронтенд имеет Vercel Preview c человеческим ревью,
  бэкенд/движок/инфраструктура — нет. При разборе инцидентов негде воспроизвести точный
  прод-релиз отдельно от клиентского трафика (среда для воспроизведения = прод-хост).
- **Live-гейты провайдеров выполняются на прод-хосте.** Транзитные canary-порты, fenced-корни
  и live-запуски (GPT Image и аналоги) живут на боевом сервере; изоляция — на уровне процесса
  и каталога, а не контура.
- **Прод-хост перегружен обязанностями.** Гейт + кандидат-валидация + канарейки + прод-трафик на
  одной машине: тестовая активность может влиять на боевую (и наоборот). Совмещённый стенд
  добавляет на этот хост ещё один контур, поэтому весь раздел 5.2 построен вокруг одного
  требования: **стенд обязан деградировать сам, но никогда — прод**. Per-unit `MemoryMax`
  этого не обеспечивает: нужен агрегированный slice, Docker cgroup parent, disk quota и
  network namespace.

Что уже хорошо и что план сознательно **не ломает**: изоляция через worktree, сериализованный
мерж с lock, exact-SHA неизменяемые релизы, blue-green, forward-only дисциплина миграций,
принцип «всё из репозитория, секреты только на хосте», hardening application units. Стенд
переиспользует эти механизмы, а не изобретает параллельную систему артефактов. Переиспользование
**не** значит «запустить второй экземпляр текущего production-hardcoded watchdog».

## 4. Цели и не-цели

**Цели:**

- G1. Существует стенд, поведенчески близкий к проду (та же сборка из того же SHA, те же
  application-юниты в stage-именах, тот же порядок тесты→миграции→blue-green), на котором
  разработка валидируется до прода. Контур-specific inventory (user, roots, ports, locks,
  Compose projects, reporting helper) задаётся явным `contour-config`, а не копией прод-путей.
- G2. Поток доставки: task-ветка → `stage` → авто-деплой на стенд → automatic degradation +
  human approval + host-owned `promotion/eligible` → fast-forward того же SHA в `master` → прод.
- G3. Есть быстрый путь хотфикса в `master` в обход стенда, с host-owned attestation
  `mode=hotfix` и обязательным последующим схождением `stage` с `master`.
- G4. Клиенты взаимодействуют только с продом. Стенд не имеет публичных маршрутов; доступа к нему
  у клиентов нет по конструкции (нет публичного DNS, firewall, tunnel-only).
- G5. Регрессия/деградация, прошедшая юнит-тесты, ловится на стенде автоматически
  (базлайны метрик, A/B слотов, пороги trusted policy) и блокирует promotion.
- G6. Отказ стенда никогда не блокирует хотфикс-путь в прод (прод остаётся автономным).
- G7. Прямая доставка в `master` без trusted attestation запрещена без исключений: ни ручных
  пушей, ни «мелких» байпасов. Штатный путь — через скрипты мержа. Реалистичная гарантия
  без server-side GitHub branch protection: SHA без `promotion/eligible` или hotfix-attestation
  **не выкатывается** production-watchdog. Физическую невозможность записать ref `master`
  собственная инфраструктура честно не обещает (см. 6.5).
- G8. Флот подписок провайдеров существует в одном экземпляре, и управлять им (пополнение,
  ротация, лизинги, health) может только прод-контур. Стенд и хотфикс-кандидаты используют
  емкость флота только через контролируемый прод-интерфейс (shadow-read, см. 5.3.1), никогда —
  через прямое владение токенами, полный `CONTROL_KEY` или запись в прод-БД движка.

**Не-цели (сознательно исключено):**

- Не вводится полноценное канареечное развертывание прода (X% трафика на новый релиз). Сначала —
  стенд с A/B слотов; canary-сложность вернётся позже, если понадобится.
- Не строится DR-резерв: стенд живёт на том же хосте, что и прод, и в принципе не переживает
  отказ хоста. Авария хоста роняет оба контура — это принятый компромисс совместного размещения.
- Не переносится биллинг/деньги: стенд работает на своей БД с синтетическими деньгами.
- Не роем chaos engineering как отдельную дисциплину — только целевые сценарии деградации
  в нагрузочном генераторе.
- Не меняются межконтекстные контракты (Control API, sales feed) expand-only правилами без
  отдельного producer-first коммита: стенд их потребляет в том же виде, что и прод. Узкий
  stage-telemetry scope, если он потребуется, — расширение контракта, не ломка.
- Не выполняется candidate-версия host-global/root инсталляторов (`install-watchdog.sh`,
  `install-caddy.sh`, sudoers, Docker daemon, firewall, общий monitoring stack) на
  production-host из ветки `stage`.

## 5. Целевая архитектура: два контура-близнеца

### 5.1 Базовый принцип

Один репозиторий, два контура-близнеца на одном хосте. **Application-сборка берётся из
репозитория идентично**: Cargo-workspace и pnpm-workspace собираются из одного SHA.
`deploy/`, `systemd/`, `observability/` остаются источником правды, но **применяются по
разным lanes** (раздел 9): stage-runtime-safe — на общем host; host-global — только после
promotion обычным production-watchdog, а до этого — в песочнице.

Контур-specific задаётся **immutable `contour-config`** со schema validation. Он покрывает:

- user/group;
- branch/context/environment;
- state/release/data/cache roots;
- locks;
- units;
- all ports/origins;
- DB/Redis Compose projects;
- enabled/disabled lanes;
- reporting helper;
- resource/network namespace.

Конфиг запрещает пересечение stage/prod inventory. Текстовая подстановка `sed` по production
scripts запрещена как способ получить вторую линию.

Уже сегодня почти вся host-specific конфигурация вынесена в `/etc/apitoken/*` и
`/srv/claude-api/data/*` — этот же принцип переносится на стенд (со своими путями).
Совместное размещение гарантирует нулевой дрифт инструментов (одни и те же
Node/Rust/Postgres/Caddy на одной машине).

Различия стенда от прода (полный перечень — раздел 7.4) должны быть явными и минимальными.

### 5.2 Размещение на общем хосте и изоляция контуров

| Параметр | Прод (как есть) | Стенд (цель) |
|---|---|---|
| Хост | `84.32.48.2`, 8 ядер/16 потоков, 96 GB | **тот же VPS.** Совместный failure domain принят владельцем осознанно |
| ОС-пользователь | `deploy`, root-бриджи, `apitoken-ci` | отдельный `deploy-stage` (+ `stage-ci` для тестовых процессов); стенд-процессы не работают от прод-пользователей и наоборот |
| Каталоги | `/opt/apitoken/releases`, `/srv/claude-api/releases`, `/var/lib/apitoken/...` | свои корни: `/opt/apitoken-staging`, `/srv/claude-api-staging`, `/var/lib/apitoken-staging` — ни одного общего пути с продом |
| Порты | 5433, 8790–8805, 3000/3001, 6379/6380 | свой диапазон (по умолчанию «+10000»: движок 18790/18787/18788, OpenAI 18792/18793/18797, Gemini 18794/18795/18799, KIMI 18803–18805, router 18800–18802, API 13000/13001, commerce 18791; PostgreSQL **5434**, Redis стенда 16379/16380; mock upstream — отдельный порт). Точная таблица фиксируется в `docs/ops/INFRASTRUCTURE.md` в фазе 1. Разные loopback-порты **не** являются network isolation |
| systemd-юниты | `apitoken-*`, `claude-api-*` | те же шаблоны с суффиксом `-stage`, ставит **trusted master-sourced renderer** с whitelist имён, путей и портов — не candidate installer. Прод-watchdog не видит stage-инстансы |
| PostgreSQL | контейнер `apitoken-postgres`, `127.0.0.1:5433`, прод-БД | отдельный контейнер `apitoken-postgres-stage`, `127.0.0.1:5434`, свои volume и роли (см. 5.4) |
| Ресурсы | весь хост; per-unit `MemoryMax` (слот провайдера 2G, router 512M) | **агрегированный `staging.slice`**: `MemoryMax=8G`, `MemoryHigh` ниже 8G, `CPUQuota=200%`, `TasksMax` (не `TaskMax`) как aggregate bound, `IOWeight` ниже production. Все stage services, builders, validators и generators входят в этот slice. Per-unit лимиты остаются, но одних их недостаточно: полный blue-green набор плоскостей превышает 8 GB ещё до API/worker/PostgreSQL/Redis/load generator |
| Docker | Compose project names/ports/volumes прода; oneshot wrappers; контейнеры **не** наследуют cgroup oneshot-unit; disposable `docker run` без CPU/RAM/PID limits | отдельные Compose project names; Docker-native `mem_limit`/`cpus`/`pids_limit`; `cgroup_parent` под `staging.slice` либо отдельный rootless runtime; отдельные volumes; `deploy-stage` **не** получает production Docker socket, если тот управляет всеми host containers |
| Диск | общий filesystem | enforceable quota ≤ 50 GB (отдельный filesystem/LV, XFS/ext4 project quota или loopback 50 GB). Retention артефактов не заменяет quota. Thresholds и emergency GC до ENOSPC |
| Сеть | общий network namespace; `RestrictAddressFamilies` в units; нет `PrivateNetwork` / IP policy для будущего stage | отдельный network namespace/veth: deny к production loopback ranges и Unix sockets; egress allowlist только mock, stage DB/Redis, GitHub/reporting proxy и явно разрешённые sandbox endpoints; контролируемый proxy для shadow telemetry; тесты отрицательной доступности |
| Caddy | global `/etc/caddy/Caddyfile`, production keys, reload/restart | отдельный unprivileged stage Caddy process с собственными config/data/admin ports. Stage **не** перезагружает global Caddy и не использует production admin endpoints |
| Мониторинг | общий host-network stack | production Prometheus скрейпит trusted static stage targets; candidate dashboards/rules валидируются отдельно и попадают в общий stack только после production promotion. Stage labels и cardinality budgets обязательны |
| Публичные маршруты | все продуктовые vhost'ы | **нет ни одного.** Caddy стенда слушает только свой namespace/loopback; клиенты недостижимы по конструкции |
| Доступ оператора/агентов | SSH `observe` для агента; `deploy` — watchdog identity, не agent login | SSH + проброс портов (`ssh -L`), интерфейсы стенда (Grafana, панель) через тот же туннель |
| Firewall | UFW deny-inbound, SSH/HTTP/HTTPS | без изменений публичного inbound; внутренний deny stage→prod добавляется namespace/policy, не новыми входящими правилами |
| DNS | `*.apitoken.sale` | отсутствует (режим туннеля; публичное имя для стенда не заводится) |
| Секреты | `/etc/apitoken/*`, `/srv/claude-api/data/*` | собственные файлы в **стенд-корнях** (`/etc/apitoken-staging`, `/srv/claude-api-staging/data`), mode 0600, чтение только для стенд-пользователей; **ни один прод-секрет не копируется и не становится читаемым для стенда** |

#### Инварианты совместного размещения (не обсуждаются)

1. Ни один stage-процесс не запускается от прод-пользователей; ни один прод-процесс — от
   стенд-пользователей.
2. Никаких пересечений корней релизов, БД, Redis, дампов, секретов и lock-файлов между контурами.
   Пути стенда всегда и только с суффиксом `-staging`.
3. Ресурсные лимиты стенда настраиваются до запуска первого stage-юнита. Бюджет — агрегированный
   `staging.slice` плюс Docker `cgroup_parent`. Превышение давит стенд (OOM/сброс его процессов),
   а не прод. Production SLO остаётся в заданном bounded-impact диапазоне при fork bomb,
   memory pressure и burst load.
4. Stage-watchdog работает с приоритетом кандидат-валидаторов (прецедент на проде уже есть —
   low-priority host workers), со своими lock'ами и state-каталогами; он никогда не трогает
   прод-lock, прод-релизы и прод-статусы и не исполняет candidate host-global installers.
5. Стенд-миграции и стенд-операции никогда не касаются прод-контейнера PostgreSQL
   (`apitoken-postgres` остаётся неприкосновенным); стендовый контейнер можно пересоздавать
   свободно.
6. Бэкап-таймер прода не захватывает стенд-БД (имена дюмпов разные, каталоги разные); при
   желании — отдельный дюмп-таймер стенда, но без влияния на прод-цепочку.
7. Stage user/process не читает production secrets, env, candidate cache или GitHub credential.
8. Stage namespace не подключается к production PostgreSQL, Redis, Control API mutation
   routes и internal origins.
9. `stage-emergency-stop` останавливает весь `staging.slice` и освобождает ресурсы без
   изменения production state.

#### Что даёт совместное размещение (в сравнении с отдельным VPS)

- Ноль затрат на второй сервер; нулевой дрифт инструментов и ОС-слоя;
- переиспользование уже оттестированного SHA: промоушен-коммит, который стенд уже собрал и
  провалидировал, прод-конвейер берёт из существующего root-owned кандидат-кэша (та же
  механика exact-SHA marker) вместо повторной сборки — **если** SHA/tree/digests совпали и
  attestation жива;
- общий наблюдательный стек: Prometheus/Grafana прод-хоста дополнительно скрейпит стенд
  с меткой `env=staging`, отдельный стенд-дашборд, алерты стенда — низким severity или только в
  стенд-отчёт (раздел 8), чтобы не шуметь в прод-мониторинге. Candidate `observability/` на
  этот стек не применяется.

#### Что берём на себя (осознанные компромиссы)

- Общий failure domain: хост, питание, диск, Docker — одна точка отказа для обоих контуров.
- Ресурсная конкуренция (CPU билдов, RAM, диск, iops) между контурами — управляется
  агрегированным slice, Docker limits, disk quota и приоритетами, но не устраняется полностью.
- Ошибка оператора в одном контуре потенциально ближе к другому (правильные пути/порты —
  вопрос дисциплины, суффиксов, schema validation `contour-config` и отрицательных isolation
  tests). Таблица портов и путей из этого документа — обязательная часть provisioning'а фазы 1.

Стенд не имеет CORS-пересечений с боевым фронтом (`NEXT_PUBLIC_BACKEND_URL` прод-сайта указывает
на прод, и не меняется). «Клиенты не могут попасть на стенд» обеспечивается на уровне сети,
а не только конфигурации приложения.

### 5.3 Подписки провайдеров на стенде

Это самое чувствительное место (живые подписки стоят денег и банятся при неаккуратных тестах).
Исходное требование владельца: **флот подписок один, управляет им master, но стенд и
хотфикс-кандидаты должны мочь его использовать.** Прямая трактовка «стенд читает прод-флот»
небезопасна и отклонена — причины и рабочая альтернатива ниже.

#### 5.3.1 Почему прямое разделение флота невозможно, и что вместо этого

Флот подписок — это не статичный список токенов, а stateful-система:

- окна лимитов 5h/7d живут в головах провайдеров и в PG-авторити движка
  (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`);
- в `crates/pool` держится in-memory состояние ротации: лизинги, health-статусы, «rebind»-связи
  клиент↔подписка, счётчики 429;
- `CLAUDE_API_UPSTREAM` — глобальная настройка инстанса движка (`crates/server/src/config.rs`),
  per-account маршрутизации апстрима в коде нет: один инстанс либо ходит в реальный
  `api.anthropic.com`, либо целиком в mock.

Следствие: второй движок, ходящий теми же токенами в тех же провайдеров, неизбежно (а) пишет
два независимых представления о состоянии подписок (окна, health, лизинги) — движок потеряет
единый авторити; (б) сжигает реальные лимиты прод-флота тестовым трафиком; (в) своими
429-волнами и неаккуратными паттернами ухудшает health и рискует баном прод-подписок.

**Принятая модель: один флот — один владелец, остальные — клиенты по HTTP.**

- Управляют флотом только прод-инстансы (`claude-api`, authbot, прод-registry) — единственный
  писатель в авторити, единственный держатель in-memory пула. Это инвариант уровня G8.
- Стенд и хотфикс-кандидаты **не получают живых токенов, не получают полный `CONTROL_KEY` и
  не пишут в прод-БД движка**.
  Доступ к емкости флота они получают двумя способами, оба опциональны и включаются
  отдельным решением оператора:

  1. **Shadow-read состояния флота (не раньше готовности telemetry-контракта, рекомендуется).**
     Прод-движок уже отдаёт часть телеметрии через Control API. Низкопривилегированная
     проверка (`CLAUDE_API_PANEL_KEY` / `readonly_authed`) есть для `/metrics`, `/capacity`,
     `/gemini-subs`, `/admin-events`. Другие fleet projections (`/codex-subs`, `/kimi-subs`,
     `/glm-subs` и operational/money views) требуют полный `control_authed`. Полный Control API
     тем же control credential защищает мутационные money/account endpoints.
     Перед включением shadow-read составляется точный endpoint inventory, и выбирается один
     вариант:
     - перевести только sanitised fleet projections на `readonly_authed` (expand-only,
       producer-first);
     - ввести отдельный `CLAUDE_API_STAGE_TELEMETRY_KEY` с узким route scope;
     - **предпочтительно** — однонаправленный telemetry exporter/proxy, который отдаёт стенду
       только агрегаты и не маршрутизирует произвольный URL.
     Ответы исключают email hints, account identifiers и поля, не нужные для калибровки mock.
  2. **Бюджетный live-endpoint (фаза 7, строго опционально).** Если mock-реализма мало,
     стенд может ходить в **прод-флот как обычный API-клиент**: на проде заводится отдельный
     engine-аккаунт `stage-live` с собственным ключом `sk-pool-…` и жёстким nanoUSD-капом
     (переиспользуется дисциплина капов из `docs/ops/*_CALIBRATION.md`, стартовый бюджет —
     центы). Трафик стенда идёт через тот же публичный/внутренний endpoint прода, что и
     трафик любого клиента: ротацией, лимитами и health флота по-прежнему управляет только
     прод-движок. Это единственный безопасный смысл фразы «стенд использует флот»:
     как потребитель сервиса, не как совладелец. Lane явно включается; без него stage не
     обращается к реальным provider endpoints. Хотфикс-кандидаты используют тот же
     endpoint в своих live-пробах, если оператор его включил.

- Чего в модели сознательно **нет**: расшаривания `subscriptions.db`/PG-авторити между
  контурами, копирования живых токенов в стенд-секреты, read-replica пула, выдачи стенду
  полного `CONTROL_KEY`. Любое из этого — нарушение G8 и блокер мержа.

- **Песочница (опционально, фаза 7)** — из прежнего плана остаётся как третий, независимый
  вариант: отдельный минимальный набор реальных подписок (низкие тарифы, выделенные
  аккаунты, НЕ из прод-флота), свой `authbot` на стенде. Дешевле и безопаснее, чем
  live-endpoint, но требует отдельных денег и обслуживания. Если live-endpoint включён,
  песочница скорее не нужна.

- Прод-флот никогда не используется стендом на запись, стенд-флот (песочница) никогда не
  используется продом — это инвариант уровня секретов и уровня правил.

#### 5.3.2 Дефолтный режим стенда

- **Дефолт — mock upstream.** Движок стенда стартует с `CLAUDE_API_UPSTREAM` =
  mock-сервер, поднятый на стенде из тех же механик, что `tests/mock_upstream.py`, расширенный
  сценариями: окна 5h/7d, 429-волны, «мёртвая подписка», замедление стримов, обрыв SSE. Подписки
  в БД стенда — синтетические, с фейковыми токенами. Никаких живых токенов.
- Сценарии mock-апстрима калибруются по shadow-read телеметрии прод-флота (5.3.1), когда она
  включена: это закрывает главный недостаток mock-режима — расхождение профиля с реальностью.

### 5.4 Данные стенда

- Свой PostgreSQL: отдельный контейнер `apitoken-postgres-stage` на `127.0.0.1:5434` того же
  хоста, свои volume и роли. Свои БД: `commerce`, `claude_engine`, `sales`, `openkeys`
  (CRM не нужен — не его контур). Прод-контейнер (`apitoken-postgres`, 5433) стенд не трогает
  никогда — ни соединениями, ни перезапусками.
- Redis стенда (позже, для fidelity affinity/history) — отдельные инстансы на своих портах
  (16379/16380); на первых фазах движок в mock-режиме работает без них (тесты со
  `CLAUDE_API_TEST_REDIS_URL` проходят в стенд-лайне со своим disposable Redis).
- Наполнение: **сгенерированный seed** (новый скрипт, см. фазу 3): аккаунты и ключи, балансы,
  заказы в sandbox-статусах, referral-строки, mock-подписки, тарифные и provider-строки, чтобы
  повторять форму прод-данных без реальных PII. Схема — всегда из тех же миграций, что и прод:
  расхождение схем само по себе является сигналом.
- Прод-дам из прод-бэкапа **не** используется. Позже (фаза 7) — опционально скрабленный
  анонимизированный дамп, если реализма seed'а окажется мало. Это отдельное решение
  с собственным скраб-конвейером (PII, токены, почты, суммы), не раньше закрытия стандартных фаз.

### 5.5 Commerce-безопасные заглушки

Стенд должен «проживать» все те же воркфлоу без внешних эффектов:

- Платежи — sandbox-ключи провайдеров (Platega/Cryptomus sandbox), либо локальная заглушка, если
  провайдер не даёт sandbox.
- Почта — sink (Mailhog/локальный SMTP), либо devbot-стиль запись в лог; ни одно реальное письмо
  и ни один webhook наружу не уходит.
- OAuth Google/GitHub — отдельные stage-приложения разработчика.
- Внешний uptime-workflow (`.github/workflows/production-uptime.yml`) на стенд не распространяется.

Paired A/B и isolated mutating scenarios (раздел 8.1) доказывают zero external side effects:
локальные payment/webhook/mail sinks, разные synthetic accounts и idempotency namespaces.

## 6. Git-модель: `master` + `stage` + hotfix

### 6.1 Ветки

- `master` — без изменений: интеграция и прод-триггер, только через `deploy/agent-merge.sh`.
- **`stage` (новая)** — интеграция и стенд-триггер. Защищена от прямых пущей той же дисциплиной
  скриптов, что и master. В первой версии одновременно валидируется **один serial promotion
  batch**. Несколько непредпродвинутых кандидатов и «rebase-поток stage целиком» из редакций
  v1–v6 снимаются: rebase меняет commit SHA и инвалидирует marker, degradation result и
  approval.
- Task-ветки (`feat/*`, `fix/*`, `docs/*`) — от `origin/master`, обычный поток, без изменений.
- **`hotfix/*`** (новая семантика) — от последнего зелёного `master`; короткоживущая ветка
  аварийного исправления. Имя ветки — сигнал процессу, не авторизация.

### 6.2 Поток разработки (stage → prod)

```text
origin/master ─┬─ feat/foo ─────...─────▶ (1) merge в stage ──▶ стенд: тесты, миграции, blue-green
               │                                        │       + degrade-гейт + human approval
               │                                        │       + host-owned promotion/eligible
               │                                        └─────▶ (2) fast-forward того же SHA в master
               │                                                    └─▶ прод: обычный watchdog-цикл
               └─ feat/bar ─────...──┐                             по promotion/eligible, не по
                                     └── ждёт своей очереди        будущему deploy/watchdog
```

1. **Merge в `stage`.** Тот же `deploy/agent-merge.sh` с явно заданными переменными, но **не**
   одной подстановкой `AGENT_MERGE_REQUIRED_CONTEXT=deploy/stage`. Нужно разделить как минимум:

   ```text
   TARGET_BASELINE_CONTEXT=deploy/watchdog   # зелёный прод на текущем master до speculative validation
   CANDIDATE_PRECONDITION_CONTEXT=deploy/stage
   POST_PUSH_CONTEXT=deploy/stage            # после push в stage ждать stage-deploy, не prod
   ```

   Отдельный lock (`AGENT_MERGE_LOCK=.../stage-merge.lock`), отдельная validation-environment
   (`AGENT_MERGE_VALIDATION_ENVIRONMENT=staging-candidate-validation`). Удобно оформить как
   тонкую обёртку `deploy/agent-merge-stage.sh`. Простая замена единственного context на
   `deploy/stage` заставила бы проверять старый `master` по stage-статусу и после push в
   `master` ждать `deploy/stage` вместо production deployment.

2. Стенд-watchdog (раздел 7) тестирует точный SHA, мигрирует stage-БД, выкатывает blue-green
   application lane и публикует информационные статусы `stage/deployed` и компонентные
   `deploy/stage-*`. Эти статусы **не** являются production admission.

3. После `stage/deployed` работает trusted degrade-гейт (`stage/degradation`) и (для изменений,
   затрагивающих клиентский фронтенд) обычное Vercel-превью с ревью. **Утверждение promotion —
   операторское**: агент не продвигает stage без явной команды. Approval привязан к
   `{commit_sha, tree_sha, artifact_digests, policy_digest}` (`promotion/approved`).
   Host-owned helper собирает итоговую `promotion/eligible`.

4. **Promotion — serial batch, неизменяемая идентичность.**

   1. в stage одновременно валидируется один batch;
   2. после deployment stage замораживается на soak/approval;
   3. `master` fast-forward’ится на **тот же SHA** (не rebase-поток «содержимого stage»);
   4. production использует тот же tested candidate marker;
   5. движение `stage` или `master` автоматически отзывает approval.

   Если история требует rebase/cherry-pick, promotion helper создаёт новую attestation только
   после: точного tree equality, повторной exact-SHA trusted validation, повторного stage
   deployment/degrade gate и нового approval. Даже при том же итоговом tree равенство
   доказывается явно.

5. **Схождение.** После успешного promotion `stage` fast-forward к новому `master`.
   Единственный разрешённый способ — скрипт `deploy/stage-sync.sh` под тем же merge-lock.
   Прямые `git push -f` ветки `stage` агенту запрещены. Recovery после failed
   promotion/stage-sync имеет формализованный lock order и не оставляет stale approval.

### 6.3 Хотфикс (в обход стенда)

```text
origin/master ─▶ hotfix/x ──▶ merge сразу в master (deploy/agent-merge.sh, обычный гейт)
                                   │
                                   └─▶ host-owned attestation mode=hotfix
                                       (operator identity, exact SHA/tree, причина, TTL)
                                       production-watchdog принимает SHA по ней,
                                       не по имени ветки и не по trailer
                                       deploy/stage-sync.sh подтягивает hotfix в stage
```

- Хотфикс — это merge в `master` без предварительной валидации на стенде, но **не без гейтов
  вообще**: локальный gate + кандидат-валидация остаются обязательными. Обходится только
  стадийный цикл (стенд + утверждение). Production admission даёт **host-owned attestation
  с `mode=hotfix`**, а не строка `hotfix/` в имени ветки и не commit trailer: оба можно
  воспроизвести в обходном commit.
- Правило миграций не меняется: хотфикс с миграцией — два коммита, migration-first.
- После hotfix все прежние approvals для непредпродвинутых stage-кандидатов
  недействительны. Stage-sync переводит `stage` на новый `master`; прежний batch, если он
  не был продвинут, проходит заново (новая identity, новый gate, новый approval).
- Стенд, будучи впереди, никогда не блокирует хотфикс — прод-контур автономен (G6).
  Direct-push detector в observe-only фазах только alert/quarantine dry-run; fail-closed
  admission — только в фазе 6 после drill.

### 6.4 Что меняется в правилах для агентов (фаза реализации git-модели)

- `BRANCHES.md`: добавить строку `stage` (владелец, триггер стенда, serial batch, правила
  схождения) и раздел hotfix (критерии обхода, host-owned attestation, документирование).
- `AGENTS.md`: поток «merge в stage → freeze → approval → eligible → FF того же SHA в
  master», запрет `git push -f` в `stage`, обязанность использовать
  `deploy/agent-merge-stage.sh` / `deploy/stage-sync.sh`.
- `CONTRIBUTING.md`: описание двухступенчатой доставки, разделение информационных и
  авторизующих статусов, `promotion/eligible` как admission, критерии hotfix.
- Индексы `docs/README.md` и карта в `AGENTS.md` — по факту добавления новых документов.
- Эти правки **не** входят в фазу 2 observe-only: enforcement документов и кода включается
  только когда dry-run и drill уже доказаны (раздел 10). Иначе обычные production merges
  встанут до готовности контура.

### 6.5 Запрет прямой доставки в `master` — техническое закрепление

Правило «в `master` только через `deploy/agent-merge.sh`» действует и сегодня, но обеспечено
только дисциплиной и клиентским хуком `.claude/hooks/guard-git.sh`, который **не запускается** в
OpenCode и других агентах и легко обходится человеком в терминале. Защита веток на
стороне GitHub-хостинга для этого репозитория недоступна, а регрессионный тест
`deploy/watchdog-lib.test.sh` явно запрещает на неё полагаться.

Защита строится **собственной инфраструктурой** тремя слоями. Слои A и B остаются полезны.
Честная гарантия без server-side branch protection / pre-receive / fork-only:

> SHA, не имеющий доверенной promotion/hotfix attestation, не будет развёрнут
> production-watchdog.

Нельзя честно гарантировать:

> никто физически не сможет записать SHA в `master`.

Архив сверки не содержит GitHub role/permission matrix; утверждение, какие учётки способны
писать в `master`, пока не доказано.

**Слой A. Клиентский хук (уже есть).** `guard-git.sh` денайтит `git push … master` и wildcard-add
в Claude Code. Остаётся как удобный ранний сигнал (UX guard), но не считается серверным
барьером — это совпадает с самим хуком.

**Слой B. Отделение merge-кредишенса (гигиена, не ACL).** Сегодня пуш в `master` и пуш в
фиче-ветку выполняет одна учётка: `agent-merge.sh` берёт `GITHUB_TOKEN` либо обычный
`git credential fill`, push идёт обычным remote. Отдельный PAT/machine user улучшает
credential hygiene, но **сам по себе не создаёт branch-level ACL**. При отсутствии
server-side branch rules учётка с repository write технически может записать ref `master`;
отсутствие merge token в её credential helper не отнимает существующее GitHub permission.

Вводится разделение как гигиена:

- Заводится **отдельный merge-кредишенс**. Им подписан пуш `HEAD:master` в
  `deploy/agent-merge.sh`.
- Этот кредишенс **не кладётся** в повседневный `git credential` разработчиков и агентов.
  `agent-merge.sh` читает его через выделенный helper/env (`AGENT_MERGE_PUSH_TOKEN`).
- Обычные учётки сохраняют пуш в фиче-ветки. Прямой пуш в `master` перестаёт быть штатным
  путём для повседневной учётки, но слой B **не** формулируется как «физически невозможно».

**Слой C. Fail-closed watchdog на хосте (главный).** Хост-watchdog — последняя линия.
Даже удачный прямой пуш в `master` **не выкатывается** без attestation:

- Production-watchdog до выкатки проверяет host-owned `promotion/eligible` (обычный поток)
  либо `mode=hotfix` attestation. Он **не** проверяет собственный будущий
  `deploy/watchdog=success` — этот статус появляется только после deployment (цикл из
  редакций v5–v6).
- Кандидат без attestation считается нелегитимным: watchdog не выкатывает, quarantine,
  audit event «прямой пуш в master мимо гейта» с SHA и автором пуша (через GitHub API).
- Имя `hotfix/*` и trailer не дают admission.
- Fail-closed: при любой неопределённости (не смог проверить attestation, API недоступен)
  watchdog **не выкатывает**.
- В фазах 2–5 слой C работает observe-only / dry-run: alert и quarantine-маркер без
  блокировки обычных production merges. Fail-closed enforcement — фаза 6 после drill.

**Promotion-прекондишн (клиентская сторона слоя C).** `deploy/agent-merge.sh` в
`AGENT_MERGE_TARGET=master` перед пушем проверяет `promotion/eligible` на той же identity,
если это не hotfix с живой hotfix-attestation. Проверка `deploy/stage` как единственного
зелёного context недостаточна и создаёт цикл со статусом production-watchdog.

**Оставшиеся правила.**

3. **Без байпаса для «мелочей».** Исходный открытый вопрос №5 закрыт отказом: цена прохождения
   мелочи через стенд минимальна (лайны path-aware, docs-only дифф не собирает Rust/TS), а
   прецедент исключения размывает сам инвариант. Единственный обход стенда — явный `hotfix/*`
   с host-owned attestation и документированной причиной (раздел 6.3).
4. **Хотфикс не отменяет гейтов.** Хотфикс обходит только стадийный цикл (стенд + человеческое
   утверждение); локальный gate, кандидат-валидация и hotfix-attestation обязательны, а
   пуш в `master` выполняет всё та же merge-учётка через `agent-merge.sh`. Прямого
   `git push origin master` как штатного пути не существует.

**Свод по потокам:**

- *Тестовая разработка (stage → master):* в `master` попадает тот же SHA через
  `agent-merge.sh` после `promotion/eligible`. Прямой пуш не является штатным путём
  (A+B); без attestation не выкатывается (C).
- *Хотфикс:* тоже **не** прямой пуш. `hotfix/*` пушится фиче-веткой, затем
  `agent-merge.sh` + host-owned `mode=hotfix`. От стенда хотфикс освобождён, от гейтов,
  merge-учётки и attestation — нет.

### 6.6 Как этим пользоваться — простым языком

Этот подраздел — для человека и агента, которым не нужны детали реализации. Что делать в
типовых ситуациях после внедрения стенда **и** после включения enforcement (фаза 6).
До фазы 6 прод по-прежнему принимает `master` как сейчас; стенд в это время observe-only.

**Обычная разработка (фича, исправление, документация):**

1. Агент создаёт себе изолированную копию репозитория (worktree) и ветку `feat/что-то` —
   одной командой `deploy/agent-worktree.sh create`. Работает только в ней, не трогая общий
   каталог.
2. Пишет код, прогоняет локальные тесты, обновляет документацию в том же коммите.
3. Публикует результат одной командой: `git push -u origin HEAD` + `./deploy/agent-merge-stage.sh`.
   Это отправляет изменение в ветку `stage` и автоматически раскатывает его на **тестовый
   стенд** — отдельную копию системы, куда клиенты не имеют доступа.
4. На стенде всё само прогоняется: тесты, миграции, проверка «ничего не сломалось»
   (degrade-гейт). Агент получает зелёный или красный статус.
5. Стенд в этот момент **заморожен**: в очереди один batch. Когда soak пройден,
   **оператор (человек)** даёт команду «выкатить в прод». Агент запускает
   `deploy/agent-merge.sh` — **тот же SHA** (не «содержимое stage после rebase») уходит в
   `master` и на боевой сервер.
6. После успешной выкладки агент убирает за собой — удаляет свою рабочую копию одной командой
   `deploy/agent-worktree.sh finish`.

**Аварийный хотфикс (прод упал, нужно срочно чинить):**

1. Агент делает ветку `hotfix/имя` от свежего `master`, чинит, прогоняет тесты.
2. Публикует сразу в `master` той же командой `deploy/agent-merge.sh` — **минуя стенд**, потому
   что ждать некогда. Но все обязательные проверки (локальный гейт, кандидат-валидация) и
   отдельная host-owned hotfix-attestation **сохраняются** — хотфикс не значит «без проверок»
   и не значит «ветка с нужным именем».
3. Как только прод ожил, стенд автоматически подтягивает то же исправление
   (`deploy/stage-sync.sh`). Непродвинутые стенд-изменения после хотфикса проверяются заново.

**Чего делать нельзя ни в каком случае:** напрямую `git push origin master` как рабочий путь.
Боевой сервер **не примет** такой код к выкладке без attestation и поднимет тревогу.
Собственные скрипты не обещают, что GitHub физически отвергнет запись в `master`.

**Зачем всё это:** разработка и эксперименты идут на стенде, не рискуя клиентами; в прод
попадает только тот SHA, который уже проверен на копии системы; при аварии есть быстрый, но
всё равно контролируемый путь.

## 7. Конвейер доставки на стенде

### 7.1 Стенд-watchdog

На том же хосте работает **вторая линия watchdog-комплекта только после параметризации**.
Фаза 2 — не запуск второго экземпляра с иной конфигурацией поверх текущего
production-hardcoded кода.

Сначала refactor в явный immutable `contour-config` (раздел 5.1). Затем stage-линия:

- поллится ветка `stage` (тот же read-only деплой-ключ репозитория);
- свой state-root (`/var/lib/apitoken-staging/watchdog`), свои lock-файлы, свои quarantine-маркеры;
  стенд-конвейер никогда не пишет в прод-статусы и прод-quarantine, и наоборот;
- работает с пониженным приоритетом и внутри `staging.slice`;
- тот же path-aware отбор лайнов, те же изолированные disposable БД/Cargo-таргеты, но внутри
  стенд-пространства; disposable Docker получает CPU/RAM/PID limits и `cgroup_parent`;
- миграции — только на стенд-БД (5434); blue-green — на стенд-слотах (порты «+10000» из 5.2);
- **application lane** на общем host: binaries, stage units уже установленные trusted
  renderer'ом, stage DB/Redis, stage-only Caddy, mock/load generator, stage dashboards/data
  sources;
- **host-global lane не исполняется из stage-кандидата на production-host:**
  `install-watchdog.sh`, sudoers, общие systemd definitions/controllers, production Caddy,
  Docker daemon, firewall/sysctl/packages, общий Prometheus/Loki/Grafana/Alertmanager.
  Candidate infrastructure changes проходят sandboxed infra-validation (параллельный контур:
  ephemeral VM / systemd-nspawn / отдельный test host) и применяются к production-host
  исключительно обычным production-watchdog после promotion;
- статусы GitHub — через **caller-bound** reporting helper, не через расширение того же
  широкого контракта новыми именами:

  ```text
  watchdog-github-prod   — caller deploy; contexts deploy/watchdog, deploy/tests, ...
  watchdog-github-stage  — caller deploy-stage; contexts deploy/stage, deploy/stage-*, stage/*
  promotion-attest       — caller/operator policy; создаёт host-owned attestation
  ```

  Один root-owned GitHub credential допустим. Helper определяет допустимые contexts по
  Unix user; stage caller не публикует production contexts; произвольные environments
  запрещены; status связан с текущим exact candidate marker; свободный SHA от caller без
  проверки state-root не принимается. Отдельного root-кредишенса для стенда не заводится;
- прод-специфичные одноразовые гейты (gpt-image-*, live-калибровочные контроллеры, devbot
  прод-топики) у стенд-линии **не активны** (полный список выключений фиксируется в `deploy/README.md`
  при реализации);
- бэкап-политика стенда минимальна (перед миграциями — дампы stage-БД в стенд-каталог; Borg
  off-host — только если оператор решит, что стенд этого заслуживает). Прод-цепочка
  (`claude-api-backup.timer`, ежечасные дюмпы) стенд-БД не захватывает.

Схема «две линии одного комплекта на одном хосте» имеет прецедент в low-priority
candidate-validators. Стенд-линия — постоянный участник с собственным неймспейсом **после**
contour-config, а не зеркало текущих абсолютных путей.

Возможные переиспользуемые уже сейчас куски (проверено по коду): параметризация
`AGENT_MERGE_TARGET` / `AGENT_MERGE_LOCK` в `deploy/agent-merge.sh` (но не единственный
`AGENT_MERGE_REQUIRED_CONTEXT` в трёх ролях), машинерия `.github`-статусов
(`deploy/watchdog-github.sh`) как база для **сужения** caller contract, exact-SHA marker.

### 7.2 Порядок деплоя на стенде

Зеркально проду для **application lane**: тесты → дампы/бэкапы → миграции под локом →
неактивный слот (миграции движка транзакционные, `/ready`) → health-gate → окно A/B
(между admission нового slot и pre-drain старого) → degrade-гейт (раздел 8) → cutover →
пост-деплой пробы → информационные статусы. Перед A/B — проверка совместимости N-1
binary с post-migration schema. Провал любой стадии — red stage-status, quarantine-SHA.
Роллбек на стенде — тот же `rollback.sh` как **binary/slot switchback** + автоматический
откат при red degrade-гейта. Это не rollback БД.

### 7.3 Вёрстка секретов стенда

Все env-файлы стенда заводит оператор один раз (фаза 1), в репозиторий не попадают.
Набор — зеркало продового, но с заменёнными значениями: свои пароли БД, свои контрольные ключи,
движок в mock-режиме, почта-заглушка, платежи-sandbox. `CONTROL_KEY` стенда — отдельный и
не равен прод-ключу. Прод-`CONTROL_KEY` стенду не выдаётся.

### 7.4 Полный список сознательных отличий стенда от прода

Этот список — часть документа-контракта, всё остальное поведение application lane совпадает:

1. Триггер — ветка `stage`; информационные статусы — `stage/deployed`, `deploy/stage-*`,
   `stage/degradation`; авторизующие — `promotion/approved`, `promotion/eligible`;
   environment — `staging-environment`. `deploy/watchdog` остаётся результатом production
   deployment.
2. Нет публичных маршрутов, только loopback/туннель + network namespace.
3. Движок по умолчанию на mock upstream; прод-подписки стенду не принадлежат и не
   адресуются им напрямую; доступ к емкости флота — только по модели 5.3.1 (sanitised
   shadow-read и/или бюджетный live-endpoint как API-клиент). Полный `CONTROL_KEY` не
   выдаётся.
4. БД — своя, seed-данные; прод-дампы не используются (опционально скраб в фазе 7).
5. Платежи/почта/OAuth — sandbox/заглушки.
6. Прод-специфичные live-гейты и внешний uptime-детектор не активны.
7. Бэкапы — локальные, упрощённая ротация.
8. Роллбек при red degrade-гейта — автоматический binary/slot switchback (на проде —
   по решению оператора). Не заявляется как DB rollback.
9. Ресурсы — агрегированный `staging.slice` ≤ 8 GB / ≤ 2 CPU, Docker внутри slice,
   enforceable disk quota ≤ 50 GB, заниженные приоритеты.
10. Совместное размещение: общий хост, ОС и Docker с продом — изоляция пользователями,
    каталогами, портами «+10000», юнитами `*-stage`, PostgreSQL 5434, slice, namespace,
    quota, по инвариантам раздела 5.2.
11. Candidate host-global installers на production-host не исполняются.
12. Stage Caddy — отдельный unprivileged process; global Caddy не reload.
13. Gate policy — trusted production-approved, не candidate-owned.

## 8. Обнаружение деградации (ядро ценности стенда)

### 8.1 Постоянная синтетическая нагрузка с профилем прод-трафика

- Новый генератор нагрузки (скрипт в репо, напр. `deploy/stage-load/`): работает на стенде
  постоянно (низкая интенсивность) и умеет burst-режим на время последеплойного окна.
  Генератор входит в `staging.slice`.
- Сценарии покрывают профиль прод-трафика, видимый в метриках: messages stream/non-stream
  (в т.ч. thinking), роутер `/v1/messages` и универсальный чат, responses API, `/balance`,
  ошибочные пути (невалидный ключ, исчерпанный баланс, отмена стрима на середине),
  «спящие» доли трафика по провайдерам.
- **Не дублировать произвольные stateful requests в оба слота.** «Одинаковый поток»
  безопасен только для read-only или специально спроектированных synthetic scenarios.
  Mutating flow дважды меняет баланс/заказ, дедуплицируется одним idempotency key, конкурирует
  за одну stage DB/Redis или даёт недетерминированный model output. Сценарии разделены:

  - **paired read/protocol probes** — один и тот же вход, прямое сравнение
    latency/status/protocol;
  - **isolated mutation probes** — разные synthetic accounts, order IDs и idempotency
    namespaces для blue/green;
  - **outcome-class comparison** — для генеративных ответов сравнивать контракт, usage,
    TTFT, error class и completion, а не текст побитово;
  - **zero external side effects** — локальные payment/webhook/mail sinks.

- Интенсивность калибруется так, чтобы НЕ выжигать лимиты mock-пула, а лишь создавать
  измеримый поток метрик. Burst не обязан равняться пику прода — важен стабильный, сравнимый
  профиль.

### 8.2 A/B между синим и зелёным слотами

Blue-green механика уже умеет держать два слота двигателя/API. На стенде окно сравнения
встраивается **между admission нового slot и pre-drain старого** (существующие controllers:
`deploy/api-bluegreen.sh`, `deploy/engine-bluegreen.sh`). Полностью переписывать blue-green
не требуется; нужна явная state machine.

Метрики снимаются по слотам раздельно, direct per-slot scraping. Labels:
`{env, contour, slot, release_sha, scenario}`.

Продолжительность окна сравнения настраивается (дефолт-предложение: 15 минут burst; для изменений
движка/маршрутизации — до часа под постоянной нагрузкой). Warm-up и cool-down обязательны.
После окна — обычный cutover; старый слот уходит по штатному drain. Red gate — automatic
binary switchback.

### 8.3 Золотые метрики и пороги (degradation gate)

Стенд-watchdog после деплоя вызывает `deploy/stage-degrade-gate.sh`. Скрипт и пороги лежат в
репозитории, но **исполняемая policy — trusted**: fixed root-owned validation policy, digest
которой входит в tested marker (как уже делает production watchdog для validation policy).
Candidate-version policy сначала проходит старую policy и отдельные контрольные инъекции.
Кандидат не может одновременно внести регрессию и ослабить порог, которым сам проверяется.

Сравнение с базлайном (параллельный старый слот — раздел 8.2, либо тот же слот до выкатки
с baseline TTL). Золотые метрики:

- латентность запросов p50/p95, время до первого байта стрима;
- доля клиентских 4xx и серверных 5xx, `customer_http_error`-события;
- ротация: `429`-ретраи, «rebind»-события, усреднённое число попыток на запрос;
- служебное здоровье: заполнение billing-очереди, латентность PG-команд, inflight, утечки
  лизингов/резервов;
- ошибки роутера (fallback-rate, admission failures) и stage-Caddy upstream health.

Правила гейта (fail-closed):

- регрессия пороговой величины = red `stage/degradation` + авто switchback + запрет promotion;
- движение в пределах допуска — желтая зона (запись в отчёт, не блокирует);
- деградация, причинно объяснимая сценарием самого деплоя — оператор может разблокировать с
  письменной причиной (аналог `--fix-red`), что создаёт **новое** approval, а не переиспользует
  старое;
- отсутствует метрика / stale series / недостаточный sample size / изменившееся имя
  label/metric / Prometheus недоступен / host saturation, делающая сравнение недостоверным
  → red, не false green.

Одновременно нужны: warm-up/cool-down, minimum request count, absolute SLO, relative delta
blue vs green, baseline TTL, confidence/noise rule, policy digest в attestation.

PromQL-пороги калибруются по живым рядам; в снимке 2026-08-20 полных scrape/rule files и
Grafana dashboards не было (только `observability/compose.yaml`). Конкретные числа порогов
не фиксируются в этом proposal до появления этих рядов.

Промоушен-прекондишн на клиенте: `agent-merge.sh` в target=master проверяет
`promotion/eligible` на той же identity; для hotfix — живую hotfix-attestation.
Информационный `deploy/stage` / `stage/degradation` входит в eligible, но сам по себе
admission не даёт.

### 8.4 Регулярные проверки, которые стенд берёт на себя

- Миграции: commerce-миграции прогоняются на stage-БД *до* попадания в прод; отдельно —
  N-1 binary против post-migration schema.
- E2E-протоколы поверх живого стенда: ChatGPT-протокол, стриминговые сценарии —
  аналоги `tests/universal_chat_smoke.sh`, но по реально развернутой системе.
- Репетиции роллбека: автоматический binary/slot switchback стенда — регулярный
  (еженедельный) тренировочный сценарий.
- Деградационные инъекции (фаза 4): генератор умеет сценарии падений (убить слот, затормозить
  PG, обрубить поток SSE), чтобы правила гейта проверялись регулярно и не «протухали».
  Control injections обязательны до enforcement.

## 9. Границы доверия и протокол допуска

Нормативный раздел. Реализация без него запрещена. Он отвечает на вопросы рецензии кода
и снимает противоречия редакций v1–v6.

### 9.1 Кто может исполнять root-код

| Код | Кто исполняет на production-host | Когда |
|---|---|---|
| Production host-global installers (`install-watchdog.sh`, `install-caddy.sh`, sudoers, controllers, global Caddy, monitoring stack) | root helper production-watchdog | только для SHA, который уже tip `master` и имеет trusted validation marker |
| Stage application units | trusted **master-sourced** renderer с whitelist имён, путей и портов | provisioning и обновление шаблонов с master, не из candidate `stage` |
| Stage application binaries / slot cutover | `deploy-stage` / stage-watchdog внутри `staging.slice` и network namespace | SHA ветки `stage` после trusted validation этой линии |
| Candidate host-global из ветки `stage` | **никто на production-host** | только sandboxed infra-validation вне прод-инвентаря |

Отдельные users, каталоги, порты и per-unit `MemoryMax` не исправляют риск: root-код
кандидата может менять global systemd, sudoers, Caddy, Docker, firewall, monitoring,
файлы production-контура и сам механизм изоляции. Это прямое нарушение границы
«стенд не влияет на прод».

### 9.2 Какие статусы информационные, а какие авторизующие

```text
stage/deployed          — информационный: точный SHA успешно развёрнут на stage
stage/degradation       — информационный: A/B и абсолютные SLO пройдены
deploy/stage, deploy/stage-*  — информационные компонентные контексты stage-линии
promotion/approved      — авторизующий вход: подписанное операторское решение
promotion/eligible      — авторизующий итог: host-owned attestation
hotfix attestation      — авторизующий: host-owned, mode=hotfix
deploy/watchdog         — результат production deployment, не precondition самого себя
deploy/tests            — production test lane, production caller only
```

Production-watchdog до выкатки проверяет `promotion/eligible` (или hotfix attestation),
а не собственный будущий статус.

`deploy-stage` технически не может опубликовать `deploy/watchdog` или иной production
deployment status.

### 9.3 Формат promotion/hotfix attestation

Host-owned запись, не GitHub commit status как единственный источник истины. Mutable GitHub
status остаётся удобным зеркалом, не admission.

Минимум полей:

- `mode`: `promotion` | `hotfix`;
- operator identity;
- `commit_sha`, `tree_sha`;
- artifact digests (binaries, TS bundles, migration manifest);
- validation policy digest и degradation policy digest;
- contour id (`stage` / `prod`);
- issued_at, expires_at (TTL);
- reason (обязателен для hotfix);
- binding на текущий exact candidate marker / state-root.

Attestation выпускает `promotion-attest`, не stage-watchdog и не проверяемый candidate.

### 9.4 Точная единица identity

Единица promotion — `{commit_sha, tree_sha, artifact_digests, policy_digest}`, а не
«ветка `stage`», не «содержимое после rebase» и не «тот же tree без доказательства».

`master` fast-forward’ится на тот же `commit_sha`. Production использует тот же tested
candidate, без пересборки, если marker совпал.

### 9.5 Правила invalidation

Следующее автоматически отзывает `promotion/approved` и `promotion/eligible`:

- любой rebase, cherry-pick или новый commit на `stage`;
- движение `master` (в том числе hotfix);
- смена tree, artifact digest или policy digest;
- истечение TTL;
- `stage-emergency-stop` и quarantine SHA;
- failed promotion / несогласованный stage-sync.

После hotfix все прежние approvals непредпродвинутых stage-кандидатов недействительны.
Новая attestation — только после tree equality (если применимо), повторной trusted
validation, повторного stage deploy/degrade и нового approval.

### 9.6 Контуры network / cgroup / Docker isolation

На co-located host обязательны одновременно, не по отдельности:

- `staging.slice` с агрегированными Memory/CPU/Tasks/IO;
- все stage processes и generators внутри slice;
- Docker: отдельные project names, native limits, `cgroup_parent` (или rootless), отдельные
  volumes, нет production Docker socket у `deploy-stage`;
- enforceable disk quota ≤ 50 GB и emergency GC;
- network namespace/veth с deny production loopback/Unix sockets и egress allowlist;
- отрицательные isolation tests как merge-blocking для изоляционного кода.

### 9.7 Разделение application и host-global infrastructure lanes

См. 7.1 и таблицу 9.1. Host-global infra-validation — отдельная параллельная фаза/контур,
не смешивается с co-located application stage.

---

## 10. Фазы внедрения

Каждая фаза — отдельные коммиты и зелёные статусы; документы обновляются в тех же коммитах.
Оценки — грубые (человеко-дни оператора + доля агентской работы).

Порядок фаз **изменён относительно v1–v6**. Прежняя редакция вводила fail-closed production
enforcement уже в фазе 2, хотя stage data, components и degradation gate появлялись позже.
Это заблокировало бы обычные production merges до готовности контура.

- **Фаза 0. Решения — ВЫПОЛНЕНА (v4, 2026-08-16).** Все вопросы закрыты владельцем
  (раздел 11.1): бюджет ≤ 8 GB / ≤ 2 ядер / ≤ 50 GB, доступ только SSH-туннель, протекающий
  стенд, песочница в фазе 7, флот — shadow-read до решения после degradation gate. Ресурсный
  бюджет и портовая таблица подтверждены живым снятием состояния прод-хоста. Закупать нечего.
  v7 не переоткрывает эти решения; она уточняет, **как** их технически соблюсти.
- **Фаза 1 — trusted contour foundation (1–2 д).** Inventory/schema для prod и stage;
  `staging.slice`, disk quota, network namespace; users, roots, ports, Compose projects;
  stage-specific reporting helper (caller-bound); отрицательные isolation tests; **никаких
  candidate root installers**. На `84.32.48.2` без остановки прод-контура: пользователь
  `deploy-stage`, стенд-корни, секреты сгенерированы заново, контейнер
  `apitoken-postgres-stage` на `127.0.0.1:5434`. UFW публичный inbound не меняется.
  `docs/ops/INFRASTRUCTURE.md` получает секцию «Staging contour (co-located with
  production)». Trusted master-sourced renderer stage units — whitelist.
- **Фаза 2 — parameterization и observe-only stage watchdog (2–4 д).** Refactor controller
  configuration в `contour-config`; stage poll/deploy в mock mode; статусы публикуются, но
  **не** являются production precondition; direct-push detector только alert/quarantine
  dry-run. Скрипты `deploy/agent-merge-stage.sh`, `deploy/stage-sync.sh` (+ регрессионные
  сьюты) в режиме, который не ломает текущий `AGENT_MERGE_REQUIRED_CONTEXT` для прода.
  Обновление `BRANCHES.md` / `AGENTS.md` / `CONTRIBUTING.md` на этом шаге описывает
  observe-only контур, не fail-closed admission.
- **Фаза 3 — data/components и safe sinks (3–5 д).** Seed/reseed/snapshot/GC; stage
  DB/Redis; API/engine/router/commerce; mock/payment/mail/webhook sinks; unprivileged stage
  Caddy. Движок в mock-режиме (улучшенный mock upstream с деградационными сценариями).
  Подключение к существующему мониторинг-стеку только как trusted static scrape
  `env=staging`; candidate dashboards/rules в общий stack не попадают. Каждый контекст —
  своими лайнами/юнитами `*-stage`.
- **Фаза 4 — trusted degradation gate (2–4 д).** A/B state machine; direct per-slot
  scraping; fail-closed metrics semantics; control injections; automatic binary switchback.
  Генератор нагрузки, paired vs isolated scenarios. Приёмка фазы: искусственная регрессия
  ловит гейт; missing/stale/renamed metric даёт red; candidate-ослабленная policy не
  проходит старую policy.
- **Фаза 5 — promotion attestation и dry-run enforcement (1–2 д).** Human approval record;
  exact identity binding; master-watchdog проверяет attestation, но сначала только логирует
  расхождения; hotfix drill и approval invalidation. Ещё не блокирует обычный прод.
- **Фаза 6 — enforcement (1–2 д).** Обязательный stage→prod flow; fail-closed production
  admission; emergency stop/break-glass runbook; полный drill зафиксирован в аудите.
  С этого момента поток stage→prod обязателен для всего, кроме hotfix с attestation.
  Рунбуки: `docs/ops/DEPLOYMENT.md` и новый runbook стенда.
- **Параллельно, не смешивая с application stage — host-global infra-validation.**
  Candidate `deploy/`, `systemd/`, `observability/` прогоняются в ephemeral VM /
  systemd-nspawn / отдельном test host. На production-host эти изменения по-прежнему
  применяет только production-watchdog после promotion.
- **Фаза 7. Опциональное усиление (по решению оператора).** Песочница реальных подписок
  и/или бюджетный live-endpoint (решение **после** фазы 4/5, когда измерено, какие
  регрессии mock не ловит); скрабленный анонимизированный дамп; контроль конфигурационного
  дрифта контуров. Перенос канареечных live-гейтов провайдеров с прод-контура на
  стенд-контур — только если live-lane явно включён и не обходит G8.

## 11. Риски и решения фазы 0

Риски:

- **Общий failure domain.** Хост, питание, диск, Docker и UFW общие: аппаратная авария или
  ошибка на уровне хоста роняют оба контура сразу (в варианте с отдельным VPS прод был бы
  изолирован). Принято владельцем осознанно; стенд по определению не DR-резерв.
- **Blast radius стенда на прод.** Билды стенда, burst нагрузки и стенд-миграции могут
  конкурировать за CPU/RAM/iops/диск. Per-unit лимиты недостаточны. Митигируется инвариантами
  5.2 / 9.6 (aggregate slice, Docker cgroup parent, disk quota, namespace, заниженные
  приоритеты) и мониторингом бюджета стенда как прод-предупреждения.
- **Candidate root на прод-хосте.** Буквальное зеркало infrastructure lane со stage-кандидата
  даёт неутверждённому SHA root на production-host. Митигируется разделом 9.1: на co-located
  host candidate не исполняет класс B.
- **Цикл `deploy/watchdog`.** Выкатка, которая требует уже зелёного собственного статуса,
  не стартует. Митигируется разделением информационных и авторизующих статусов (9.2).
- **Подделка production-status.** Широкий `watchdog-github` в руках `deploy-stage` ломает
  последнюю границу. Митигируется caller-bound helper (7.1, 9.2).
- **Ошибка оператора между контурами.** Перепутанные порт/путь/юнит на общем хосте бьют
  дальше, чем на разных машинах. Митигируется жёсткими суффиксами, schema validation
  `contour-config`, отказом стенд-скриптов принимать прод-пути и отрицательными isolation
  tests.
- **Двойной контур = двойное обслуживание.** Митигируется: всё из репозитория, параметризованный
  watchdog-комплект, все секреты контур-специфичны; дрифт контролируется в фазе 7. Всё же
  стенд потребует бюджета внимания при обновлениях конвейера.
- **Стоимость.** Серверного бюджета больше нет; остаются (в фазе 7) отдельные подписки
  песочницы — фиксируются отдельной строкой.
- **Стенд даёт ложное спокойствие.** Реализм mock-апстрима ограничен; живые паттерны провайдеров
  стенд увидит только с песочницей или live-endpoint. Поэтому правила: живой провайдерский GA —
  по-прежнему через существующую дисциплину (`docs/engine/PROVIDER_ONBOARDING.md`, калибровки),
  стенд её дополняет, а не заменяет.
- **`stage`-ветка и force-операции.** Сведение stage к master по сути force. Ограничено одним
  скриптом под локом; stale approval снимается; аварийные русла описаны в 6.3.
- **Задержка доставки.** Двухступенчатый цикл удлиняет путь в прод. Для аварий — хотфикс-поток
  (раздел 6.3); байпаса для «мелочей» нет (6.5). Docs-only/тестовые диффы проходят стенд дёшево
  за счёт path-aware лайнов. Serial batch (один за раз) — сознательная цена identity binding.
- **Стенд как клиент прод-флота (риск v3).** Если включён бюджетный live-endpoint (5.3.1),
  стенд-трафик конкурирует с клиентским за реальные окна подписок. Митигируется nanoUSD-капом
  аккаунта `stage-live`, отдельным ключом (отзываемым независимо) и правилом: live-прогоны
  стенда — редкие и по расписанию, постоянная нагрузка — только на mock.
- **Неполный Control API для shadow-read.** Выдача полного `CONTROL_KEY` стенду закрывает
  мутационные endpoints. Митигируется inventory + unidirectional exporter (5.3.1) до фазы
  shadow-read.
- **Ложный green degrade-гейта.** Missing/stale/renamed metric или candidate-ослабленный
  порог. Митигируется fail-closed семантикой и trusted policy (8.3).

### 11.1 Решения фазы 0 (v4, утверждено владельцем 2026-08-16)

Бывшие «открытые вопросы» закрыты; формулировки ниже — обязательная часть контракта.
v7 уточняет реализацию, не отменяет решения.

1. **Ресурсный бюджет стенда — утверждён.** Суммарно MemoryMax ≤ 8 GB и CPUQuota ≤ 200%
   (2 ядра) на **весь** stage-контур через `staging.slice` (все `*-stage` юниты, builders,
   validators, generators, Docker с `cgroup_parent`). Per-unit `TasksMax` дополняет
   агрегированный `TasksMax` slice; директива называется `TasksMax`, не `TaskMax`. Диск
   стенда ≤ 50 GB (`/opt/apitoken-staging` + `/srv/claude-api-staging` +
   `/var/lib/apitoken-staging`) **enforceable quota**, не декларация. Билд-процессы стенда —
   `Nice`/`IOSchedulingClass` ниже прод-класса. Прод держит весь остальной запас. Бюджет
   подтверждён живым снятием состояния хоста (2026-08-16, SSH `deploy@84.32.48.2`, только
   чтение): RAM занято ~11 из 96 GB (available ~81 GB), диск 326/894 GB (40%), прод-релизы
   занимают ~34+12+40 GB, свободной ёмкости с запасом хватает под бюджет стенда даже в час
   пик билдов. Снимок 2026-08-20 не содержал live `systemd-cgls` / quotas / listeners;
   перед implementation sign-off ресурсные числа и mounts переснимаются.
2. **Доступ — только SSH-туннель.** Публичное DNS-имя стенду не заводится; интерфейсы стенда
   (Grafana с меткой `env=staging`, панель, API) смотрятся через `ssh -L`. UFW и Caddy-маршруты
   прода не меняются; G4 обеспечивается сетью, а не конфигурацией приложения.
3. **Песочница подписок — в фазе 7.** Набор: один минимальный Claude-тариф + один Codex +
   один Gemini на выделенных аккаунтах (НЕ из прод-флота), свой `authbot` на стенде. Если к
   фазе 7 будет включён live-endpoint (п.6) и его реализма хватает — песочницу не заводим,
   решение фиксируется отдельным коммитом в этом документе.
4. **Данные стенда — «протекающий» стенд.** Seed наполняется один раз (фаза 3), далее данные
   живут своей жизнью (история заказов/ключей/балансов накапливается, как в проде) — это
   ловит класс багов «проявляется на накопленном состоянии». Периодический reseed — по явной
   команде оператора (например, после смены формы seed или аварии стенд-БД).
5. ~~Порог «мелочи» для бай-паса стенда~~ — **закрыт отказом (v3):** байпаса нет, обход стенда
   только через `hotfix/*` с host-owned attestation и документированной причиной (разделы 6.3,
   6.5).
6. **Доступ к емкости флота — до фазы 4/5 только mock; shadow-read — после telemetry
   inventory.** Mock-апстрим стенда калибруется по read-only агрегатам прод-флота, когда
   exporter/узкий ключ готов (5.3.1). Решение о бюджетном live-endpoint (аккаунт
   `stage-live` с nanoUSD-капом, 5.3.1 вариант 2) принимается **после фазы 4**, когда
   измерено, какие регрессии mock не ловит. Полный `CONTROL_KEY` стенду не выдаётся.
7. **Хотфикс-кандидаты и флот.** Кандидат-валидация хотфиксов остаётся на mock-апстриме —
   этого достаточно, т.к. хотфикс всё равно проходит полный прод-гейт. Если когда-либо
   понадобятся live-проверки хотфикса (фикс ротации под реальными 429), они идут через тот же
   бюджетный live-endpoint, что и стенд, — отдельной механики не вводится.

Портовая таблица стенда («+10000») проверена против живых слушающих портов хоста
(2026-08-16): 5434, 13000/13001, 16379/16380, 18787–18805 — все свободны; конфликтов с продом
(5433, 6379/6380, 8787–8806) нет. Точная таблица фиксируется в `docs/ops/INFRASTRUCTURE.md`
в фазе 1. Порты не заменяют network namespace.

### 11.2 Что ещё отсутствует для implementation sign-off

Для правки этого proposal снимка 2026-08-20 достаточно. Перед implementation sign-off
полезны и пока отсутствуют:

- фактическая GitHub role/credential/settings matrix;
- live snapshot `systemd-cgls`, Docker daemon config, groups, listeners, mounts/quotas и
  current overrides;
- полные Prometheus scrape/rule files и Grafana dashboards;
- фактические migration SQL directories в том же архиве (в репозитории они есть; сверка
  плана их не подменяла);
- 3–5 incident/postmortem examples для калибровки degradation scenarios.

Отсутствие этих материалов не мешает держать архитектурные инварианты раздела 9, но мешает
подтвердить ресурсные числа, GitHub permissions и конкретные PromQL thresholds.

## 12. Критерии успеха (definition of done)

Implementation нельзя считать принятой, пока не пройдены исходные цели и следующие проверки:

1. Новый production SHA развёртывается без циклической зависимости от собственного
   `deploy/watchdog`.
2. Прямой пуш в `master` без attestation не развёртывается и создаёт quarantine/audit event.
3. Поддельный `hotfix/*` trailer/branch name не даёт production admission.
4. `deploy-stage` технически не может опубликовать `deploy/watchdog` или production
   deployment status.
5. Stage candidate не может исполнить произвольный root-код на production-host.
6. Stage user/process не может прочитать production secrets, env, candidate cache или
   GitHub credential.
7. Stage namespace не может подключиться к production PostgreSQL, Redis, Control API
   mutation routes и internal origins.
8. Stage не может обращаться к реальным payment/mail/provider endpoints без явно включённого
   budgeted lane.
9. CPU/RAM/PID budget сохраняется при fork bomb, memory pressure и burst load; production
   SLO остаётся в заданном bounded-impact диапазоне.
10. Docker containers входят в stage budget; Docker socket не даёт управления production
    containers.
11. Заполнение stage disk до quota не заполняет production filesystem; emergency GC/stop
    срабатывает.
12. Любой rebase, hotfix или движение stage SHA инвалидирует degradation result и human
    approval.
13. Promotion подтверждает exact SHA/tree/artifact digests и policy digest.
14. Old slot/binary проходит проверку на post-migration schema; rollback меняет только
    binary/slot и не заявляется как DB rollback.
15. Missing/stale/renamed metric даёт red, а не false green.
16. Candidate не может ослабить gate policy, которым сам проверяется.
17. Paired read-only и isolated mutating A/B scenarios доказано не создают внешних эффектов.
18. Emergency `stage-emergency-stop` останавливает весь stage slice и освобождает ресурсы
    без изменения production state.
19. Полный normal flow и hotfix flow проходят без ручных Git mutations.
20. Recovery после failed promotion/stage-sync имеет формализованный lock order и не оставляет
    stale approval.
21. Внесённая в код деградация (латенси/ошибки/дед-подписка) ловится гейтом стенда до прода —
    подтверждено контрольной инъекцией, результат зафиксирован в аудите.
22. У прод-клиентов нет ни одного сетевого пути до стенда (проверяемые UFW/Caddy-пробы
    снаружи: стенд недостижим).
23. Новая команда работает на агентском потоке так же детерминированно, как текущая:
    скрипты покрыты `.test.sh`-сьютами, документы обновлены, merge только через скрипты.

## 13. Связанные документы

- `CONTRIBUTING.md`, `BRANCHES.md`, `AGENTS.md` — текущая модель и правила (обновляются в
  фазах 2 и 6 по мере observe-only → enforcement).
- `docs/ops/INFRASTRUCTURE.md`, `docs/ops/DEPLOYMENT.md`, `deploy/README.md` — прод-топология
  и конвейер, которые стенд зеркалирует в application lane.
- `docs/ops/MONITORING.md` — прод-метрики, список «золотых» метрик гейта растёт из него.
- `tests/mock_upstream.py`, `tests/rotation_fanout_smoke.sh`, `tests/universal_chat_smoke.sh` —
  база для mock-режима стенда и E2E-протоколов.
- `docs/engine/PROVIDER_ONBOARDING.md`, `docs/ops/*_CALIBRATION.md` — дисциплина live-проверок,
  которую стенд дополняет, но не отменяет.
- `docs/engine/CONTROL_API.md` — граница shadow-read; полный control credential стенду не
  выдаётся.
- `docs/ops/INCIDENT_POSTMORTEMS.md` — калибровка degradation scenarios на реальных инцидентах.
- `deploy/rollback.sh` — binary/slot switchback, БД не меняет.
