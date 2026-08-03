# OpenKeys — предоплаченные ключи без регистрации

`openkeys.apitoken.sale` — витрина для ключей, которые продаются готовыми (FunPay и подобные
площадки). Покупателю не нужны регистрация, почта и карта: он получает ключ и личную ссылку на
страницу расхода.

Ключевое отличие от конкурентов: **номинал задаётся в долларах прайса выбранного API**,
а не во внутренних «токенах». Админ выпускает отдельные партии Claude или GPT; тип определяет
Base URL, инструкцию и подписи USAGE, не меняя общий контракт движка и формат `sk-pool`.

## Состав

| Компонент | Что это |
|---|---|
| `apps/openkeys` | Next.js на порту 3410: Claude/GPT-доки, `/profile/<token>`, USAGE и админка `/admin` |
| `packages/openkeys-db` | Своя PostgreSQL-схема (`openkeys_batches`, `openkeys_keys`) и раннер миграций |
| `deploy/openkeys-deploy.sh` | Выкат: промоушен релиза, миграции, атомарный симлинк, readiness-gate, откат |
| `systemd/apitoken-openkeys.service` | Юнит сервиса |

Границы контекста: OpenKeys **не** трогает commerce и sales. С движком общается только через
Control API из `docs/engine/CONTROL_API.md` — как и весь остальной коммерческий слой.

## Модель данных

Один проданный ключ = один аккаунт движка. Так баланс принадлежит ровно этому ключу, и страница
расхода может показывать остаток, ничего не зная о пользователе.

`openkeys_batches.api_type` различает `anthropic` и `openai` только как историческую/витринную
метку. Исторические строки с `NULL` интерпретируются как `anthropic`; поле не ограничивает модели,
не выбирает pricing rule и не меняет универсальный доступ одного ключа. Целевой product catalog
может явно добавить Gemini без использования `api_type` как pricing/admission authority.

Полный секрет `sk-pool-…` хранится на складе только в AES-256-GCM шифротексте и стирается после
выдачи или снятия. Для истории остаются `engine_account_id`, `engine_key_id`, маска и `view_token` —
случайный 128-битный идентификатор публичной страницы расхода.

Страница расхода форматирует название модели по canonical family и engine `provider`: только
Anthropic получает префикс `Claude`, GPT остаётся GPT, Gemini — Gemini, а неизвестные будущие
семейства показываются нейтрально. Fallback не имеет права приписывать `Claude` чужому провайдеру.

Новый выпуск всегда имеет `pricing_contract=official_1_to_1`, `mult_bp=10000`: ключ с номиналом
$50 получает ровно $50 engine balance, а $1 полной официальной стоимости модели списывает $1.
До общего Stage 9 cutover historical `pricing_contract=legacy` остаётся migration source. Target
release переводит все существующие и новые OpenKeys на 1:1; прошлые ledger rows не пересчитываются.

## Переменные окружения (`/etc/apitoken/openkeys.env`, root-only, 0600)

| Переменная | Назначение |
|---|---|
| `OPENKEYS_DATABASE_URL` | DSN к своей БД openkeys |
| `ENGINE_CONTROL_KEY` | Control API движка. Только server-side, в браузер не уходит |
| `ENGINE_BASE_URL` | По умолчанию `http://127.0.0.1:8790` — стабильный loopback-origin, не слот |
| `ENGINE_PUBLIC_BASE_URL` | По умолчанию `https://api.apitoken.sale`, используется для `/balance` при поиске по ключу |
| `ENGINE_OPENAI_PUBLIC_BASE_URL` | По умолчанию `https://openai.api.apitoken.sale`, резервная проверка GPT-ключа через `/balance` |
| `OPENKEYS_ADMIN_USER` | Логин основной учётки админки |
| `OPENKEYS_ADMIN_PASSWORD` | Её пароль |
| `OPENKEYS_ADMIN_ACCOUNTS` | Дополнительные учётки как `user:password`, через запятую или перевод строки |
| `OPENKEYS_SESSION_SECRET` | Секрет подписи сессионной куки, минимум 32 символа |
| `OPENKEYS_SECRET_KEY` | 32-байтный ключ AES в hex (64 символа); резервируется отдельно от дампа БД |
| `OPENKEYS_SECRET_KEYS` | Keyring для ротации: `kid:64-hex,kid2:64-hex`; старые ключи сохраняются до re-encryption |
| `OPENKEYS_SECRET_ACTIVE_KID` | KID ключа из keyring, которым шифруются новые складские секреты |
| `OPENKEYS_PUBLIC_BASE_URL` | Базовый адрес для ссылок вида `/u/<token>` |
| `OPENKEYS_SESSION_TTL_SECONDS` | Время жизни сессии админки, по умолчанию 12 часов |

Учётки админки действуют **только на этом домене**: кука подписывается отдельным секретом и
ставится на `openkeys.apitoken.sale`, никакой связи с `admin.partners.*` или панелью нет. В куке
лежит имя вошедшего, оно же попадает в `created_by` партии — видно, кто выпустил ключи. Удаление
учётки из env немедленно инвалидирует её сессии: имя проверяется по списку на каждом запросе.

Экономика нового выпуска не настраивается через env или запрос: номинал зачисляется в engine
ровно 1:1 и сохраняется как `official_1_to_1`. Старые `legacy`-ключи могут читаться с historical
multiplier только до Stage 9. После global cutover live reserve любого OpenKeys использует
canonical 1:1; legacy contract нельзя выбрать для новой партии и нельзя переносить в target release.

Issuance использует `GET /admin/pricing/v2/provisioning-context` как линейную границу cutover. Пока
context равен `null`, сохраняется прежний catalog/switch policy-before-credit путь. После cutover
аккаунт сначала получает ровно face value без usable secret, затем writer account-local normalizes
funding, готовит canonical release-v2 OpenKeys policy с одной global rule
`discount_bps=0/payable_multiplier_bp=10000`, записывает exact active/recovery assignment extension,
читает его обратно и повторно читает свежий context. Только после совпавшего readback выпускается
`sk-pool` secret. Активный target всегда использует evidence-selected recovery из context; активный
recovery получает один member. Любой rejected ACK, funding blocker, readback mismatch или head drift
оставляет secret невыпущенным, а существующая issuance compensation отключает незавершённый engine
account. Поэтому OpenKeys не зависит от commerce activation tables и не требует остановки batch
issuance во время global CAS.

Stage 7 target materialization запускается только явной командой с exact Stage 5 plan/release
digest. Сначала обязателен `dry_run`, затем idempotent `apply`. Команда включает disabled accounts,
готовит canonical 1:1 bindings и не меняет OpenKeys rows, balances, key status или history. Она не
активирует live цену: всё inventory переключается одним Stage 9 release-head CAS. Полный протокол —
`docs/commerce/MULTI_DISCOUNT_STAGE7.md`.

### Authoritative pricing inventory v2

OpenKeys публикует additive read-only producer для Stage 5 v2 materializer только на
loopback/internal поверхности:

```text
GET /api/internal/pricing/v2/inventory?after_account_id=<id>&limit=500
x-openkeys-control-key: <ENGINE_CONTROL_KEY>
```

Ответ `{inventory:{inventory_digest,accounts[],next_after_account_id}}` strict-валидируется
`packages/contracts`. Cursor идёт по `account_id` в byte order, page size ограничен `1..500`, а
`inventory_digest=sha256:v2` описывает полный manifest, не одну страницу. Каждая account identity
содержит только `account_id`, durable source-row `source_id`, lifecycle
`active|disabled|removed`, source `pricing_contract`, historical `source_multiplier_bp` и
собственный content digest.

Producer включает все durable `openkeys_keys`, в том числе disabled, removed и legacy, а также
созданные engine accounts из issuance journal, которые ещё не представлены key row. Незавершённый
journal account считается active, compensated — disabled; появление итогового key row заменяет
journal identity в следующем snapshot без дублирования account и меняет manifest/content digest.
Он не возвращает key secret, ciphertext, view token, live balance или персональные данные продавца.
`packages/db` Stage 5 v2 consumer обязан исчерпать cursor, требовать один digest на всех страницах
и повторить полный scan;
изменение inventory между страницами делает результат stale. Endpoint ничего не materialize'ит,
не меняет OpenKeys/engine rows и не активирует pricing. Consumer подключён отдельным checkpoint
только после GREEN exact producer SHA и также не меняет OpenKeys rows.

## Административные интерфейсы

Собственная `/admin` построена вокруг партий, а не номиналов. Список партий имеет серверный поиск
по метке/ID и пагинацию; содержимое выбранной партии раскрывается прямо под её строкой, а повторный
клик его скрывает. Только для выбранной партии загружаются и расшифровываются складские секреты
(не больше 100), отдельно видны
готовые к продаже ключи и история выдачи. Новая партия в UI требует метку, чтобы продавец не
терялся среди большого числа выпусков; исторические партии без метки остаются видимыми.

Единая `admin.apitoken.sale` показывает все OpenKeys-ключи отдельным разделом: маску, обязательную
колонку метки/партии, продавца, live-расход/остаток и обратимое отключение. Фильтры работают по
партии, статусу и использованию. Браузер ходит только на same-origin `/openkeys-admin/*`; после
managed-admin auth Caddy проксирует запрос в `/api/internal/admin/*`, добавляет проверенного actor
и server-side credential. Публичный `openkeys.apitoken.sale/api/internal/*` закрыт ответом `404`,
а полный ключ и AES-GCM шифротекст внутренний API не возвращает.

## Первый запуск на сервере

```bash
# 1. Создать базу и записать env (root)
sudo -u postgres createdb openkeys
install -o root -g root -m 0600 /dev/null /etc/apitoken/openkeys.env
# заполнить переменными из таблицы выше

# 2. Раскатить обновлённые юниты, sudoers и контроллеры
sudo bash deploy/install-watchdog.sh

# 3. Дальше выкат автоматический: watchdog увидит изменения в apps/openkeys
#    или packages/openkeys-db и вызовет openkeys-deploy.sh
```

Пароль, секрет сессии и `OPENKEYS_SECRET_KEY` в репозиторий не коммитим — только в
`/etc/apitoken/openkeys.env`. Без отдельной защищённой резервной копии `OPENKEYS_SECRET_KEY`
дамп PostgreSQL не позволяет восстановить ещё не выданные складские секреты.

## Что проверяет watchdog

`wd_path_is_openkeys` относит к контексту `apps/openkeys/*`, `packages/openkeys-db/*`,
`packages/engine-client/*`, `packages/contracts/*` и корневые манифесты. На каждый кандидат
миграции openkeys прогоняются против отдельной одноразовой PostgreSQL (`watchdog-test-db
openkeys-dsn`), и только потом идёт выкат с readiness-gate на
`http://127.0.0.1:3410/api/ready`. Readiness проверяет конфигурацию, PostgreSQL и Control API
движка, не раскрывая наружу причину отказа. База `openkeys` входит в регулярный и обязательный
pre-deploy backup вместе с остальными PostgreSQL-контекстами.

GitHub-контекст называется `deploy/openkeys`; собственный baseline лежит в
`$STATE_ROOT/openkeys.sha`, поэтому изменения только в OpenKeys не трогают ни движок, ни backend.
