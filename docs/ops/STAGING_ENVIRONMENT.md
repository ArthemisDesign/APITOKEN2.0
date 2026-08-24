# Тестовый стенд (staging) — план внедрения

> **Статус: IMPLEMENTATION PLAN, фазы 1–7 на master.** Утверждён владельцем 2026-08-22
> (интервью, раздел 11.3). Fail-closed admission живой. Twin v1 mock-first: stores, private
> Caddy, safe sinks, trusted policy; operator env placeholders оставляют реальные application
> binaries выключенными. Фаза 8 — OWNER GATE. Раздел 9 — нормативный. Definition of Done
> раздела 12 закрывает приёмку полного живого контура, не одного только git-flow.
> Составлен 2026-08-16 на основе `CONTRIBUTING.md`, `docs/ops/INFRASTRUCTURE.md`,
> `docs/ops/DEPLOYMENT.md`, `docs/ops/MONITORING.md`, `deploy/README.md`. Каждая фаза
> внедряется отдельными коммитами; каждый коммит, который меняет поведение в других
> документах, обновляет их в том же коммите ("documentation is a living contract").
> `AGENTS.md` / `BRANCHES.md` / `CONTRIBUTING.md` не меняются этим документом — только
> в коммите соответствующей фазы.
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
> Compose, Control API, smoke/mock, часть Rust). Перед реализацией документ больше нельзя
> читать как «поднять вторую линию того же watchdog». Добавлен раздел 9 «Границы доверия
> и протокол допуска». Исправлены блокеры: цикл `deploy/watchdog`; запрет candidate
> root-installer на прод-хосте; caller-bound reporting; неизменяемая единица promotion и
> инвалидация после rebase/hotfix; агрегированный `staging.slice`, Docker/disk/network
> isolation; сначала параметризация production-hardcoded контура, затем вторая линия;
> enforcement только после dry-run и drill.
>
> **v8 (2026-08-22):** владелец закрыл оставшиеся развилки интервью (раздел 11.3). Статус
> сменён на IMPLEMENTATION PLAN без переписывания разделов 1–9. Сняты противоречия:
> network namespace **вместо** host-loopback `+10000`; бюджет `staging.slice`
> MemoryMax=32G / CPUQuota=400% / loopback 80G; Docker — rootless у `deploy-stage`;
> первый код — только extract `contour-config`; infra-proof — расширение
> `deploy/host-image-gate.sh`. Ресурсные числа v4 (8G / 2 CPU / 50G) **заменены**.
>
> **v9 (2026-08-23):** фазы 1–7 и fail-closed admission на `master`. Lock §11.3 не менялся.
> Twin остаётся mock-first до заполнения operator env. Фаза 8 не начата.
>
> **План исполнения для агента:** [`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md).
> **Стартовый промпт новой сессии:** [`docs/ops/STAGING_AGENT_PROMPT.md`](STAGING_AGENT_PROMPT.md)
> (сначала `/goal`, затем план исполнения).
> Этот файл — архитектура, инварианты и решения владельца. Пошаговая работа, статусы фаз,
> запреты и журнал SHA живут в плане исполнения. Агент не начинает код «по этому документу»,
> минуя план исполнения. Агент обновляет план исполнения в том же коммите, что и работу.

---

## 1. Резюме

Сейчас есть ровно один стенд — производственный. Локальные тесты, smoke-прогоны с mock-upstream и
exact-SHA валидация кандидата дают сильный, но не полный фильтр: систему целиком (engine + commerce +
worker + router + Caddy + БД) в связке, под нагрузкой и с реальными сценариями деградации проверять
негде. При частых деплоях часть регрессий переживает все гейты и проявляется на клиентах.

Вводится **второй стенд (stage) — контур-близнец прода**, размещённый **на том же VPS,
что и прод**, в **реально изолированном** пространстве имён: свой ОС-пользователь, свои корни
каталогов (`/opt/apitoken-staging`, `/srv/claude-api-staging`, …), свой network namespace
(внутри — те же номера портов, что у прода; на хосте видны только veth IP), свой
PostgreSQL-контейнер, свой набор systemd-юнитов, агрегированный `staging.slice`
(MemoryMax=32G, CPUQuota=400%), enforceable disk quota (80G loopback) и rootless Docker
у `deploy-stage`. Та же сборка из того же SHA и те же шаблоны приложения, но **не**
«тот же watchdog-конвейер с четырьмя env-переменными»: production-контроллеры сейчас
жёстко зашиты под прод-инвентарь, и вторая линия появляется только после явного `contour-config`.
Секреты, БД и mock-апстрим — свои. Сетевой доступ закрыт: клиенты не достигают стенд;
стенд не достигает прод-loopback, Unix-сокетов, Mailcow/support/payments-test и живых
payment/mail/OAuth/provider endpoint. Живой провайдерский lane — только явная фаза 8.

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

Инварианты раздела 9 закрыты в v7 и уточнены в v8. Этот документ — implementation plan.
Порядок работы агента, чеклисты и журнал SHA — [`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md).
Приёмка живого контура — только по разделу 12. Буквальная реализация редакций v1–v6
создала бы две наиболее опасные регрессии: циклический production admission через
`deploy/watchdog` и выполнение неутверждённого infrastructure candidate как root на
production-host. Эти редакции не реализовывать.

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

Фаза 1 реализовала production-форму этого контракта: `deploy/contour-production.json` — один
immutable inventory, `deploy/contour-config.schema.json` — закрытая схема,
`deploy/contour-config.py` / `deploy/contour-config.sh` — fail-closed validator и loader.
Production watchdog, его root-bridges и application controllers читают значения оттуда.
Golden snapshot фиксирует прежние production-значения, а synthetic second-contour fixture уже
проверяет запрет пересечений. Stage-конфиг, stage users/processes и admission в фазе 1 не созданы.

Уже сегодня почти вся host-specific конфигурация вынесена в `/etc/apitoken/*` и
`/srv/claude-api/data/*` — этот же принцип переносится на стенд (со своими путями).
Совместное размещение гарантирует нулевой дрифт инструментов (одни и те же
Node/Rust/Postgres/Caddy на одной машине).

Различия стенда от прода (полный перечень — раздел 7.4) должны быть явными и минимальными.

### 5.2 Размещение на общем хосте и изоляция контуров

| Параметр | Прод (как есть) | Стенд (цель) |
|---|---|---|
| Хост | `84.32.48.2`, 8 ядер/16 потоков, 96 GB | **тот же VPS.** Совместный failure domain принят владельцем осознанно |
| ОС-пользователь | `deploy`, root-бриджи, `apitoken-ci`, `observe` | `deploy-stage` + `stage-ci` (рантайм/тесты); `observe-stage` (агент, read-only + туннель на veth); `stage-ctl` (forced-command: attest/sync/emergency-stop/reseed). Стенд-процессы не работают от прод-пользователей и наоборот. Агент **не** получает shell `deploy` |
| Каталоги | `/opt/apitoken/releases`, `/srv/claude-api/releases`, `/var/lib/apitoken/...` | свои корни: `/opt/apitoken-staging`, `/srv/claude-api-staging`, `/var/lib/apitoken-staging` — ни одного общего пути с продом; все три — bind-mount с одного 80G loopback |
| Порты | 5433, 8790–8806, 3000/3001, 6379/6380, sales/openkeys/admin/devbot | **внутри netns — те же номера, что у прода** (5433, 8787/8788, 8790–8806, 3000/3001, 6379/6380, …). На хосте процессы стенда **не** слушают `127.0.0.1`. Скрейп и `ssh -L` идут на veth IP. Таблица veth фиксируется в `docs/ops/INFRASTRUCTURE.md` при provisioning. Host-loopback `+10000` **не** используется |
| systemd-юниты | `apitoken-*`, `claude-api-*` | те же шаблоны с суффиксом `-stage`, ставит **trusted master-sourced renderer** с whitelist имён, путей и портов — не candidate installer. Прод-watchdog не видит stage-инстансы |
| PostgreSQL | контейнер `apitoken-postgres`, `127.0.0.1:5433`, прод-БД | контейнер `apitoken-postgres-stage` в staging netns / rootless Docker, внутри `:5433`, свои volume и роли (см. 5.4). На хосте порт 5434 не публикуется |
| Ресурсы | весь хост; per-unit `MemoryMax` (Anthropic/OpenAI/router 8G, Gemini 16G, KIMI 2G; parent slices 12G/24G) | **агрегированный `staging.slice`**: `MemoryMax=32G`, `MemoryHigh=28G`, `CPUQuota=400%`, `TasksMax` (не `TaskMax`) как aggregate bound, `IOWeight` ниже production. Все stage services, builders, validators и generators входят в этот slice. Per-unit `MemoryMax` копирует прод; **стена — slice**. Gemini A/B на прод-капах может OOM-red soak — это принятый false-red, не инцидент прода |
| Docker | Compose project names/ports/volumes прода; oneshot wrappers; контейнеры **не** наследуют cgroup oneshot-unit; disposable `docker run` без CPU/RAM/PID limits | **rootless Docker** пользователя `deploy-stage`; native `mem_limit`/`cpus`/`pids_limit`; `cgroup_parent=staging.slice`; отдельные volumes. `deploy-stage` **не** читает `/var/run/docker.sock`. Production socket остаётся у `deploy` |
| Диск | общий filesystem, ext4 RAID, без project quota | один loopback **80 GB**, mount + bind на три staging-корня. Retention KEEP=3 релиза на корень. Large-payload canary требует ≥16G свободно под router spool. Emergency GC до ENOSPC. Remount `/` с quota **запрещён** |
| Сеть | общий network namespace; `RestrictAddressFamilies` в units | отдельный network namespace/veth: deny production loopback, Unix sockets, Mailcow (`13306` и mail-порты), support `:3010`, payments-test `:5440`/`:3900`; egress allowlist: mock, stage DB/Redis, GitHub/reporting proxy. Payment/OAuth/mail vendor egress **нет**. Тесты отрицательной доступности merge-blocking |
| Caddy | global `/etc/caddy/Caddyfile`, production keys, reload/restart | отдельный unprivileged stage Caddy process с собственными config/data/admin ports. Stage **не** перезагружает global Caddy и не использует production admin endpoints |
| Мониторинг | общий host-network stack | production Prometheus скрейпит trusted static stage targets; candidate dashboards/rules валидируются отдельно и попадают в общий stack только после production promotion. Stage labels и cardinality budgets обязательны |
| Публичные маршруты | все продуктовые vhost'ы | **нет ни одного.** Caddy стенда слушает только свой namespace/loopback; клиенты недостижимы по конструкции |
| Доступ оператора/агентов | SSH `observe` для агента; `deploy` — watchdog identity, не agent login | Агент: `ssh observe-stage@host` (status/logs/ready + `permitopen` только на staging veth). Человек: `ssh -L` как `deploy`. Write-path: `deploy/promotion-attest.sh` / `deploy/stage-sync.sh` через forced-command `stage-ctl`, только после явной команды человека в том разговоре. Shell `deploy` агенту запрещён |
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
   routes, internal origins, Mailcow, support и payments-test.
9. `stage-emergency-stop` останавливает весь `staging.slice` и освобождает ресурсы без
   изменения production state. Production-контур вызывает его автоматически, если
   host `MemAvailable < 12G` или production SLO red и доля staging CPU/RAM выше
   документированного порога.

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
  2. **Бюджетный live-endpoint (фаза 8, строго опционально).** Если mock-реализма мало,
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

- **Песочница (опционально, фаза 8)** — из прежнего плана остаётся как третий, независимый
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

- Свой PostgreSQL: контейнер `apitoken-postgres-stage` в staging netns / rootless Docker,
  внутри `:5433`, свои volume и роли. БД: `commerce`, `claude_engine`, `sales`, `openkeys`
  (CRM не нужен — не его контур). Content-studio в v1 нет. Прод-контейнер
  (`apitoken-postgres` на хосте `:5433`) стенд не трогает никогда — ни соединениями, ни
  перезапусками.
- Redis стенда — отдельные инстансы внутри netns на прод-номерах `:6379`/`:6380`. В v1
  они входят в twin (affinity/history). Disposable Redis для CI остаётся отдельным.
- Наполнение: **сгенерированный seed** (новый скрипт, см. фазу 3): аккаунты и ключи, балансы,
  заказы в sandbox-статусах, referral-строки, mock-подписки, тарифные и provider-строки, чтобы
  повторять форму прод-данных без реальных PII. Схема — всегда из тех же миграций, что и прод:
  расхождение схем само по себе является сигналом.
- Прод-дам из прод-бэкапа **не** используется. Позже (фаза 8) — опционально скрабленный
  анонимизированный дамп, если реализма seed'а окажется мало. Это отдельное решение
  с собственным скраб-конвейером (PII, токены, почты, суммы), не раньше закрытия стандартных фаз.

### 5.5 Commerce-безопасные заглушки

Стенд должен «проживать» все те же воркфлоу без внешних эффектов:

- Платежи — **только локальная заглушка**. Стенд не ходит к Platega/Cryptomus и не получает
  sandbox-ключи, пока владелец отдельно не откроет это решение.
- Почта — sink (Mailhog/локальный SMTP / лог); ни одно реальное письмо и ни один webhook
  наружу не уходит.
- OAuth Google/GitHub — **локальные заглушки**, не stage-приложения у вендора.
- Devbot стенда — запись в лог, без Telegram-токена и без `api.telegram.org`.
- Authbot стенда — mock/UI only: без живых OAuth, без покупки подписок, без чтения прод-секретов.
- Внешний uptime-workflow (`.github/workflows/production-uptime.yml`) на стенд не распространяется.

Paired A/B и isolated mutating scenarios (раздел 8.1) доказывают zero external side effects:
локальные payment/webhook/mail sinks, разные synthetic accounts и idempotency namespaces.

### 5.6 Состав twin v1 (до enforcement)

Входят и должны существовать до фазы 6: Anthropic, OpenAI/Codex, Gemini, KIMI, unified router,
commerce API + worker, stage Postgres (`commerce`+`claude_engine`+`sales`+`openkeys`), stage Redis,
unprivileged stage Caddy, mock upstream + load generator, sales API+web, OpenKeys, admin,
mock/UI authbot, log-sink devbot.

A/B soak: две слота Anthropic + OpenAI + Gemini + KIMI + router + API. Остальные — один инстанс.

Не входят, пока владелец отдельно не решит: content-studio, CRM, отдельные systemd-плоскости
Suno/Tripo. GLM покрывается процессом Anthropic.

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
                                       deploy/stage-sync.sh — только по команде оператора
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
  admission — только в фазе 7 после обоих drills.

### 6.4 Что меняется в правилах для агентов (фаза реализации git-модели)

- `BRANCHES.md`: добавить строку `stage` (владелец, триггер стенда, serial batch, правила
  схождения) и раздел hotfix (критерии обхода, host-owned attestation, документирование).
- `AGENTS.md`: в фазе 2 — `observe-stage` и `stage-ctl` (агент не получает shell `deploy`).
  Поток «merge в stage → freeze → attest по команде оператора → eligible → FF того же SHA
  в master» и fail-closed admission — только в фазе 7, после drills. Запрет `git push -f`
  в `stage`. Скрипты `deploy/agent-merge-stage.sh` / `deploy/stage-sync.sh` /
  `deploy/promotion-attest.sh`. Пошаговый чеклист этих правок —
  [`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md) фазы 2 и 7.
- `CONTRIBUTING.md`: описание двухступенчатой доставки, разделение информационных и
  авторизующих статусов, `promotion/eligible` как admission, критерии hotfix — в фазе 7.
- Индексы `docs/README.md` и карта в `AGENTS.md` — по факту добавления новых документов.
- Fail-closed enforcement документов и кода включается только когда dry-run и drill уже
  доказаны (раздел 10). Иначе обычные production merges встанут до готовности контура.

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

**v8: слой B в v1 не внедряется.** Повседневный `git credential` остаётся как сейчас.
Отдельный merge-PAT — опциональная гигиена позже, не условие плана. Честная гарантия —
слой C (нет attestation → нет production cutover). Учётка с repository write по-прежнему
может записать ref `master`; собственная инфраструктура это не прячет.

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
- В фазах 3–6 слой C работает observe-only / dry-run: alert и quarantine-маркер без
  блокировки обычных production merges. Fail-closed enforcement — фаза 7 после обоих drills.

**Promotion-прекондишн (клиентская сторона слоя C).** `deploy/agent-merge.sh` в
`AGENT_MERGE_TARGET=master` перед пушем проверяет GREEN `deploy/stage` на той же identity,
если это не `--hotfix`. `--hotfix` пропускает клиентскую stage-проверку и **не** доказывает
host-owned hotfix record. RED GitHub `promotion/eligible` тоже отказывает. `stage-ctl attest`
публикует зеркало `promotion/eligible`. Проверка только `deploy/stage` без host-owned
`promotion/eligible.json` по-прежнему недостаточна на хосте: production-watchdog fail-closed.

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
типовых ситуациях после внедрения стенда **и** после включения enforcement (фаза 7).
До фазы 7 прод по-прежнему принимает `master` как сейчас; стенд в это время observe-only.

**Обычная разработка (фича, исправление, документация):**

1. Агент создаёт себе изолированную копию репозитория (worktree) и ветку `feat/что-то` —
   одной командой `deploy/agent-worktree.sh create`. Работает только в ней, не трогая общий
   каталог.
2. Пишет код, прогоняет локальные тесты, обновляет документацию в том же коммите.
3. Публикует результат одной командой: `git push -u origin HEAD` + `./deploy/agent-merge-stage.sh`.
   Это отправляет изменение в ветку `stage` и автоматически раскатывает его на **тестовый
   стенд** — отдельную копию системы, куда клиенты не имеют доступа.
4. На стенде всё само прогоняется: тесты, миграции, проверка «ничего не сломалось»
   (degrade-гейт). Docs-only / test-only: окно A/B = 0, human approval остаётся.
   Runtime SHA: soak **60 минут**. Агент получает зелёный или красный статус.
5. Стенд в этот момент **заморожен**: в очереди один SHA. Когда soak пройден,
   **оператор (человек)** в том разговоре называет SHA и говорит агенту выпустить
   attestation (`deploy/promotion-attest.sh` → forced-command `stage-ctl`) и затем
   `deploy/agent-merge.sh`. **Тот же SHA** (не «содержимое stage после rebase») уходит в
   `master` и на боевой сервер. `agent-merge.sh` отказывает в пуше `master`, пока
   `deploy/stage` не GREEN, кроме `--hotfix`. Агент не аттестует SHA по стоячему правилу.
6. После успешной выкладки агент убирает за собой — удаляет свою рабочую копию одной командой
   `deploy/agent-worktree.sh finish`.

**Аварийный хотфикс (прод упал, нужно срочно чинить):**

1. Агент делает ветку `hotfix/имя` от свежего `master`, чинит, прогоняет тесты.
2. Публикует сразу в `master` той же командой `deploy/agent-merge.sh --hotfix` — **минуя стенд**, потому
   что ждать некогда. Но все обязательные проверки (локальный гейт, кандидат-валидация) и
   отдельная host-owned hotfix-attestation **сохраняются** — хотфикс не значит «без проверок»
   и не значит «ветка с нужным именем». `--hotfix` без host-owned записи admission всё равно
   отвергнет.
3. Когда прод ожил, оператор **отдельной командой** велит агенту запустить
   `deploy/stage-sync.sh` (тот же `stage-ctl`). Автосинхронизации нет. Непродвинутые
   стенд-изменения после хотфикса недействительны и проходят стенд заново.

**Чего делать нельзя ни в каком случае:** напрямую `git push origin master` как рабочий путь.
Боевой сервер **не примет** такой код к выкладке без attestation и поднимет тревогу.
Собственные скрипты не обещают, что GitHub физически отвергнет запись в `master`.

**Зачем всё это:** разработка и эксперименты идут на стенде, не рискуя клиентами; в прод
попадает только тот SHA, который уже проверен на копии системы; при аварии есть быстрый, но
всё равно контролируемый путь.

## 7. Конвейер доставки на стенде

### 7.1 Стенд-watchdog

На том же хосте работает **вторая линия watchdog-комплекта только после параметризации**.
Первый код — не запуск второго экземпляра с иной конфигурацией поверх текущего
production-hardcoded кода.

Сначала (фаза 1) refactor в явный immutable `contour-config` (раздел 5.1) **без**
пользователя `deploy-stage` и без второго контейнера. Затем (фаза 2+) stage-линия:

- поллится ветка `stage` (тот же read-only деплой-ключ репозитория);
- свой state-root (`/var/lib/apitoken-staging/watchdog`), свои lock-файлы, свои quarantine-маркеры;
  стенд-конвейер никогда не пишет в прод-статусы и прод-quarantine, и наоборот;
- работает с пониженным приоритетом и внутри `staging.slice`;
- тот же path-aware отбор лайнов, те же изолированные disposable БД/Cargo-таргеты, но внутри
  стенд-пространства; disposable Docker получает CPU/RAM/PID limits и `cgroup_parent`;
- миграции — только на стенд-БД внутри netns; blue-green — на стенд-слотах с прод-номерами портов внутри netns (раздел 5.2);
- **application lane** на общем host: binaries, stage units уже установленные trusted
  renderer'ом, stage DB/Redis, stage-only Caddy, mock/load generator, stage dashboards/data
  sources;
- **host-global lane не исполняется из stage-кандидата на production-host:**
  `install-watchdog.sh`, sudoers, общие systemd definitions/controllers, production Caddy,
  Docker daemon, firewall/sysctl/packages, общий Prometheus/Loki/Grafana/Alertmanager.
  Candidate infrastructure changes проходят sandboxed infra-validation расширением
  `deploy/host-image-gate.sh` (уже существующий disposable Ubuntu 24.04). Новый VM-farm
  и systemd-nspawn в v1 не вводятся. На production-host эти изменения применяет только
  production-watchdog после promotion;
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
движок в mock-режиме, почта-заглушка, платежи-заглушка. `CONTROL_KEY` стенда — отдельный и
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
5. Платежи/почта/OAuth — только локальные заглушки; вендорам стенд не пишет.
6. Прод-специфичные live-гейты и внешний uptime-детектор не активны.
7. Бэкапы — локальные, упрощённая ротация.
8. Роллбек при red degrade-гейта — автоматический binary/slot switchback (на проде —
   по решению оператора). Не заявляется как DB rollback.
9. Ресурсы — агрегированный `staging.slice` MemoryMax=32G / CPUQuota=400%, rootless
   Docker внутри slice, enforceable disk quota 80G loopback, заниженные приоритеты.
10. Совместное размещение: общий хост, ОС с продом — изоляция пользователями, каталогами,
    network namespace (внутри — прод-номера портов), юнитами `*-stage`, slice, 80G
    loopback, по инвариантам раздела 5.2. Host-loopback `+10000` не используется.
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

Продолжительность окна сравнения для **runtime SHA — 60 минут**. Docs-only / test-only:
окно A/B = 0 (deploy на стенд + human approval без soak). Warm-up и cool-down обязательны
для runtime. A/B одновременно: Anthropic + OpenAI + Gemini + KIMI + router + commerce API.
Sales, OpenKeys, admin, worker, mock-authbot, log-sink devbot — один инстанс, probe без A/B.
После окна — обычный cutover; старый слот уходит по штатному drain. Red gate — automatic
binary switchback. Large-payload canary на стенде — полный production-набор тел
(8/32/64/128/256 MiB) на inactive router с production MemoryMax 8G и 16G spool floor.

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

PromQL-пороги калибруются по живым рядам. В репозитории уже есть
`observability/prometheus/prometheus.yml`, rules и Grafana dashboards; production Prometheus
скрепит staging veth с `env=staging`. Конкретные числа порогов фиксируются при калибровке
фазы degrade-gate, не раньше.

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
- `unix_user=deploy` (admission identity; helper пишет файл от root/deploy, не interactive shell);
- `github_actor` и названный `commit_sha` из команды оператора (аудит, не admission);
- `tree_sha`;
- artifact digests (binaries, TS bundles, migration manifest);
- validation policy digest и degradation policy digest;
- contour id (`stage` / `prod`);
- issued_at, expires_at (TTL **24h**);
- reason (обязателен для hotfix; для promotion агент копирует формулировку оператора);
- binding на текущий exact candidate marker / state-root.

Attestation выпускает host-owned `apitoken-stage-ctl attest` по вызову
`deploy/promotion-attest.sh` с ноутбука агента **только после явной команды оператора**
в том разговоре на названный SHA. Не stage-watchdog и не проверяемый candidate.
SSH-пользователь `stage-ctl` — ForceCommand, без shell. Пользователь `deploy` интерактивно
агентом не используется.

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
- Docker: rootless daemon `deploy-stage`, native limits, `cgroup_parent=staging.slice`,
  отдельные volumes, нет production Docker socket;
- enforceable disk quota 80 GB loopback и emergency GC;
- network namespace/veth с deny production loopback/Unix sockets, Mailcow/support/payments-test
  и egress allowlist без payment/OAuth vendor;
- отрицательные isolation tests как merge-blocking для изоляционного кода.

### 9.7 Разделение application и host-global infrastructure lanes

См. 7.1 и таблицу 9.1. Host-global infra-validation — отдельная параллельная фаза/контур,
не смешивается с co-located application stage.

---

## 10. Фазы внедрения

Каждая фаза — отдельные коммиты и зелёные статусы; документы обновляются в тех же коммитах.
Оценки — грубые (человеко-дни оператора + доля агентской работы).

Исполняемый порядок для агента (чеклисты, запреты до фазы, журнал SHA) —
[`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md). Этот раздел задаёт состав фаз;
план исполнения задаёт, что агент делает сейчас. При расхождении по архитектуре и
lock §11.3 побеждает этот файл; при расхождении «что делать сейчас» агент останавливается
и чинит оба документа в одном коммите, а lock §11.3 без команды владельца не меняет.

Порядок фаз **изменён относительно v1–v6**. Прежняя редакция вводила fail-closed production
enforcement уже в фазе 2, хотя stage data, components и degradation gate появлялись позже.
Это заблокировало бы обычные production merges до готовности контура.

- **Фаза 0. Решения — ВЫПОЛНЕНА (v4 + v8, 2026-08-16/22).** Доступ только SSH-туннель,
  протекающий стенд, песочница в фазе 7, mock-first флот, без байпаса «мелочей». Ресурсный
  бюджет v8: MemoryMax=32G, CPUQuota=400%, loopback 80G (числа v4 8G/2CPU/50G **сняты**).
  Полный lock — раздел 11.3.
- **Фаза 1 — `contour-config` extract only.** Один production merge: watchdog/controllers
  читают immutable prod contour-config. Поведение `master` не меняется. Нет `deploy-stage`,
  нет второго контейнера, нет enforcement. Это **первый** код, не inventory.
- **Фаза 2 — trusted contour foundation.** Inventory/schema stage; `staging.slice` 32G/400%;
  80G loopback; netns+veth; users `deploy-stage` / `stage-ci` / `observe-stage` / `stage-ctl`;
  rootless Docker; caller-bound reporting helper; отрицательные isolation tests включая
  Mailcow/support/payments-test; **никаких candidate root installers**. Postgres-stage и
  секреты — в staging netns. UFW публичный inbound не меняется.
  `docs/ops/INFRASTRUCTURE.md` получает секцию staging. Trusted master-sourced renderer —
  whitelist. `AGENTS.md` в этом коммите добавляет `observe-stage` и запрет shell `deploy`.
- **Фаза 3 — observe-only stage watchdog.** Stage poll/deploy в mock mode; статусы
  публикуются, но **не** являются production precondition; direct-push detector только
  alert/quarantine dry-run. Скрипты `deploy/agent-merge-stage.sh`, `deploy/stage-sync.sh`,
  `deploy/promotion-attest.sh` (+ регрессионные сьюты) не ломают текущий
  `AGENT_MERGE_REQUIRED_CONTEXT` для прода. Документы описывают observe-only контур, не
  fail-closed admission.
- **Фаза 4 — data/components и safe sinks.** Seed/reseed/snapshot/GC; состав twin из 5.6;
  mock/payment/mail/webhook **stubs**; unprivileged stage Caddy; mock-authbot; log-sink
  devbot. Движок в mock-режиме. Prometheus скрепит staging veth, `env=staging`.
- **Фаза 5 — trusted degradation gate.** A/B state machine на наборе из 5.6; 60 min soak
  для runtime; full large-payload canary на inactive router; fail-closed metrics; control
  injections; automatic binary switchback. Приёмка: искусственная регрессия ловит гейт;
  missing/stale/renamed metric даёт red; candidate-ослабленная policy не проходит.
  Shadow-read telemetry **не раньше** этой фазы.
- **Фаза 6 — promotion attestation, drills, dry-run.** Human approval record; 24h TTL;
  `unix_user=deploy` + `github_actor`; master-watchdog проверяет attestation, сначала
  только логирует. Обязательны injected-fault drill **и** hotfix drill, запись в
  `docs/audits/`. Ещё не блокирует обычный прод.
- **Фаза 7 — enforcement.** Только после обоих drills. Обязательный stage→prod flow;
  fail-closed production admission; `stage-emergency-stop` + auto-stop при
  `MemAvailable < 12G` или production SLO red. Стенд down **не** блокирует hotfix.
  Рунбуки: `docs/ops/DEPLOYMENT.md` и runbook стенда.
- **Параллельно, не смешивая с application stage — host-global infra-validation.**
  Candidate `deploy/` / `systemd/` / `observability/` — расширение
  `deploy/host-image-gate.sh`. На production-host их применяет только production-watchdog
  после promotion.
- **Фаза 8. Опциональное усиление (по решению оператора).** Бывшая «фаза 7»: песочница
  реальных подписок и/или бюджетный live-endpoint (решение **после** degrade-gate);
  скрабленный дамп; дрифт контуров. Payment/OAuth вендоры по-прежнему закрыты, пока
  владелец отдельно не откроет.

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
v8 **заменяет** ресурсные числа и модель портов v4; прочие решения v3–v4 (туннель,
протекающий стенд, нет байпаса мелочей, mock-first флот) остаются.

1. **Ресурсный бюджет стенда — утверждён (v8 заменяет v4).** `staging.slice`:
   MemoryMax=32G, MemoryHigh=28G, CPUQuota=400% (4 ядра), `TasksMax` (не `TaskMax`).
   Per-unit `MemoryMax` копирует прод (Gemini 16G, Anthropic/OpenAI/router 8G, KIMI 2G);
   стена — slice. Диск — loopback **80 GB** на три корня, KEEP=3, canary требует ≥16G
   свободно под spool. Rootless Docker `deploy-stage` с `cgroup_parent=staging.slice`.
   Билды стенда — `Nice`/`IOSchedulingClass` ниже прод-класса. Снимок хоста 2026-08-22
   (SSH `deploy@84.32.48.2`, только чтение): RAM used ~10/93 GiB, MemAvailable ~82–87 GiB;
   диск 371/879 GB (45%); `/var/lib/apitoken` 66G; staging users/roots **отсутствуют**;
   Docker cgroup v2, user `deploy` в группе `docker`; на хосте уже Mailcow, support,
   payments-test — isolation tests обязаны их отрицать. ext4 `/` без project quota —
   поэтому loopback, не remount `/`.
2. **Доступ — только SSH-туннель, без публичного DNS.** Агент расследует стенд через
   `observe-stage` (read-only + `permitopen` на veth). Человек может `ssh -L` как `deploy`.
   Grafana/Prometheus прода скребят veth, `env=staging`. UFW и Caddy-маршруты прода не
   меняются; G4 обеспечивается сетью.
3. **Песочница подписок — опционально после enforcement (фаза 8).** Набор: один минимальный
   Claude-тариф + один Codex + один Gemini на выделенных аккаунтах (НЕ из прод-флота), свой
   authbot на стенде. Если к фазе 8 будет включён live-endpoint (п.6) и его реализма хватает —
   песочницу не заводим. v1 authbot — mock/UI only.
4. **Данные стенда — «протекающий» стенд.** Seed наполняется один раз (фаза 4), далее данные
   живут своей жизнью. Периодический reseed — только через `stage-ctl reseed` после явной
   команды оператора.
5. ~~Порог «мелочи» для бай-паса стенда~~ — **закрыт отказом (v3):** байпаса нет, обход стенда
   только через `hotfix/*` с host-owned attestation и документированной причиной (разделы 6.3,
   6.5).
6. **Доступ к емкости флота — mock до degrade-gate (фаза 5); shadow-read не раньше.**
   Решение о бюджетном live-endpoint (`stage-live`, 5.3.1 вариант 2) — **после фазы 5**,
   в опциональной фазе 8. Полный `CONTROL_KEY` стенду не выдаётся.
7. **Хотфикс-кандидаты и флот.** Кандидат-валидация хотфиксов остаётся на mock-апстриме —
   этого достаточно, т.к. хотфикс всё равно проходит полный прод-гейт. Если когда-либо
   понадобятся live-проверки хотфикса (фикс ротации под реальными 429), они идут через тот же
   бюджетный live-endpoint, что и стенд, — отдельной механики не вводится.

Host-loopback `+10000` **не** является моделью v8: процессы стенда слушают прод-номера
**внутри netns**. На хосте 2026-08-22 порты 5434 / 13000 / 16379 / 18787–18805 свободны и
остаются свободными. Точная таблица veth IP фиксируется в `docs/ops/INFRASTRUCTURE.md`
в фазе 2.

### 11.2 Sign-off gaps v7 — статус на v8

- GitHub role/credential matrix: **закрыто решением** — повседневный `git credential`,
  отдельный merge-PAT в v1 нет; admission = слой C.
- Live snapshot 2026-08-22 снят (11.1): listeners, Docker, RAM/диск, отсутствие staging
  артефактов, Mailcow/support/payments-test.
- Prometheus scrape/rules и Grafana dashboards **есть в репозитории**
  (`observability/`). Числа PromQL порогов — калибровка фазы 5, не блокер плана.
- Migration SQL — в репозитории; план их не подменяет.
- Incident postmortems — `docs/ops/INCIDENT_POSTMORTEMS.md`; конкретные injection
  scenarios выбираются в фазе 5.

### 11.3 Locked decisions (интервью владельца 2026-08-22)

Поздний ответ перекрывает более ранний в том же интервью. Это нормативный перечень v8.

| Тема | Решение |
|---|---|
| Бюджет | `staging.slice` MemoryMax=32G, MemoryHigh=28G, CPUQuota=400%. Per-unit caps как у прода; стена — slice |
| Диск | 80G loopback, KEEP=3, canary spool floor 16G |
| Сеть | Real netns + veth; внутри прод-порты; не host `+10000` |
| Docker | Rootless у `deploy-stage`; production socket только у `deploy` |
| Соседи на VPS | Mailcow, support, payments-test остаются; isolation tests deny их порты и контейнеры |
| Twin v1 | Состав 5.6. Нет content-studio, CRM, Suno/Tripo units |
| Authbot / Devbot | Mock/UI only; log sink; без живых токенов и Telegram |
| A/B | Engine (вкл. KIMI) + router + API вместе, 60 min runtime soak. Sales/OpenKeys/admin/worker — single |
| Docs/test-only | Через `stage`, A/B=0, human approval обязателен |
| Canary | Полный production large-payload на inactive router |
| Git queue | Serial freeze, один SHA |
| Attestation | Скрипт после явной команды оператора; `unix_user=deploy`; TTL 24h; аудит `github_actor`+SHA |
| SSH агента | `observe-stage` read-only+туннель; write через `stage-ctl` ForceCommand; не shell `deploy` |
| stage-sync | Только после явной команды оператора |
| Первый код | `contour-config` extract only |
| Infra-proof | Расширить `host-image-gate`; не VM-farm |
| Fail-closed admission | После injected-fault **и** hotfix drill |
| Layer B PAT | Не в v1 |
| Shadow-read | После mock twin + degrade gate (фаза 5+) |
| Payment/OAuth | Никогда к вендорам с этого стенда, пока отдельное решение |
| Emergency stop | Скрипт + auto при MemAvailable < 12G или production SLO red |
| Крупный payload / Gemini A/B | Может OOM-red soak о 32G стене — принятый false-red |

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
   mutation routes, internal origins, Mailcow, support и payments-test.
8. Stage не ходит к payment/mail/OAuth вендорам. К реальным provider endpoints — только
   с явно включённым budgeted lane фазы 8.
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

- [`docs/ops/STAGING_AGENT_PLAN.md`](STAGING_AGENT_PLAN.md) — обязательный план исполнения:
  порядок фаз, стоячие запреты, чеклисты, журнал SHA. Агент следует ему и обновляет его
  в том же коммите, что и работу. Архитектура и lock остаются в этом файле.
- [`docs/ops/STAGING_AGENT_PROMPT.md`](STAGING_AGENT_PROMPT.md) — стартовый промпт для новой
  сессии исполнителя: сначала `/goal`, затем план исполнения.
- `CONTRIBUTING.md`, `BRANCHES.md`, `AGENTS.md` — текущая модель и правила (обновляются в
  фазах 2 и 7 по мере observe-only → enforcement).
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
- `docs/ops/HOST_IMAGE_GATE.md` — disposable Ubuntu proof for candidate host-global installers.
- `deploy/rollback.sh` — binary/slot switchback, БД не меняет.
