# crates/authbot — CLAUDE.md

**Роль:** пополнение пула — Telegram-бот покупки подписок. Компонент-ПРОИЗВОДИТЕЛЬ: стоит ВНЕ
слоёв API (`registry←pool←forward←server`), ПЕРЕД реестром. Покупает у продавцов доступ и передаёт
его движку.

**Границы (жёстко):**
- Зависит от `registry` (только `authority` — запись подписки) + `tokio`, `portable_pty`, `rusqlite`,
  `reqwest` (URL validation/прочие bot API) и `serde`; Gemini Google HTTPS выполняет общий exact
  Node helper, а не reqwest/rustls.
  НЕ импортирует `pool`/`forward`/`server` и не лезет в их внутренности.
- Пополняет ИСКЛЮЧИТЕЛЬНО пул этого проекта: свой bot-токен, свой `AUTH_BOT_FLEET`.
- Своё состояние (юзеры/офферы) — в отдельной SQLite бота, НЕ в реестре движка.
- Реестр подписок — ТОЛЬКО engine PostgreSQL из root-owned `engine-postgres.env`. SQLite допустим
  только для собственного workflow-state бота; fallback реестра запрещён, без DSN бот не стартует.

**Три принципиально разных сценария передачи доступа** (`handoff_kind` выбирает ветку по продукту
оффера — это единственное место, где они расходятся):

| | Claude | ChatGPT (Codex) | Gemini via Antigravity OAuth |
|---|---|---|---|
| Результат | `sk-ant-oat01-…` | ничего, что нам можно читать | refresh/access token + Google subject/project/tier |
| Чем становится покупка | строка в реестре | каталог `CODEX_HOME` | AEAD envelope + opaque запись в `profiles.json` |
| Модуль | `setup_token.rs` | `codex_login.rs` | `gemini_oauth.rs` |
| Шаги продавца | прокси → email → `code#state` | прокси → email → одноразовый код | прокси → Gemini CLI OAuth/code → Antigravity OAuth/localhost URL |
| Шаг назад | `ho_code→ho_email→ho_proxy` | `cx_wait→cx_email→cx_proxy` | `gm_wait→gm_ready→gm_gproxy` |
| Как движок узнаёт | reload реестра | скан homes | atomic roster refresh на health-loop |

Каждый Claude/ChatGPT/Gemini-оффер сразу объясняет новичку весь будущий путь. После выплаты бот
выдаёт отдельный прокси и подробно проводит через новый профиль антидетект-браузера,
самостоятельную регистрацию и активацию нужного тарифа, затем через соответствующую авторизацию.
Gemini ждёт отдельного подтверждения «Аккаунт готов» и только после него показывает ссылки. На всех шагах подчёркнуто:
до прокси аккаунт не открывать, профиль/IP не менять, пароли, cookie и платёжные данные не присылать.
Если автоматическая выдача недоступна, продуктовый fallback отдельно запрашивает и проверяет прокси,
не отправляя новичка дальше с неполной инструкцией.
На любом шаге продавец может вернуться **ровно на один шаг назад** — кнопкой `↩️`, словом `назад`
или `/cancel`: это один механизм с тремя входами, а не три разных. Шаг повторяем, поэтому до ввода
прокси можно дойти нажатием несколько раз. Возврат с шага, где одноразовая ссылка или код уже
выданы, требует явного подтверждения, гасит старую capability и выдаёт новое поколение. Прокси
покупателя и живой IPRoyal lease заменить так нельзя: у них шага «ввод прокси» в истории продавца
нет, и `hproxy_order` ни на одном пути отката не обнуляется.
В быстрой админской клавиатуре доступны Claude, ChatGPT Plus/Pro и выбранные Google AI планы.

**Batch-покупки:** команда `/batch` или кнопка `🧺 Batch-покупка` запускает покупку от 2 до 100
одинаковых подписок с одной общей выплатой. Для каждой позиции хранится отдельный прокси, а
продавец получает позиции строго последовательно: следующая открывается только после успешной
передачи предыдущей. Перед созданием batch админ выбирает источник прокси — свои прокси (по одному
на позицию) или прокси продавца. Этот выбор одинаково разводит Claude, ChatGPT и Google AI/Gemini
handoff; состояние batch и незавершённый мастер переживают рестарт authbot.

`/jobs` доступна администратору и одобренному продавцу. Карточка batch показывает выполненные,
оставшиеся и текущую позицию. Processing-batch можно немедленно поставить на паузу: завершённые
позиции сохраняются, текущая незавершённая возвращается в `pending`, все её OAuth/device
capability инвалидируются, а seller lock освобождается для одного single-offer. Новый batch при
наличии paused-batch запрещён. Resume разрешён только после завершения single и создаёт новую exact
generation для той же позиции. Админ может двухшагово удалить batch из рабочей очереди; это
soft-delete в статус `cancelled`, поэтому tx/progress остаются в SQLite для аудита. Batch в
неопределённой фазе `paying` удалить нельзя до payment review.

**Изоляция сделок продавца:** `seller_jobs` хранит ровно одну активную работу на продавца — либо
конкретный single-offer, либо конкретные `batch_id + item_no`. Контекст резервируется атомарно уже
при принятии сделки, до blockchain-вызова, и привязывается к Claude/ChatGPT/Gemini handoff (Gemini
также сохраняет его в PKCE-сессии). Каждая активация позиции получает одноразовый generation token:
успешная авторизация может завершить только exact source/id/item/generation с совпадающим типом
продукта; наличие рядом другого активного batch само по себе никогда не двигает его курсор. Пока
работа не завершена, принять или оплатить конкурирующую single-/batch-сделку нельзя. Админ видит
очередь через `/jobs` или `📋 Активные сделки`; для исправления ошибочной отметки старой версии у
текущего batch есть двухшаговая кнопка возврата ровно на предыдущую позицию. Продавцовский аналог —
шаг назад внутри передачи доступа: он идёт через тот же generation guard, ротирует токен (поэтому
любой поздний callback проваливается fail-closed) и по условию `phase='processing'` не может
тронуть работу в фазе `paying`. Предикат исходного шага живёт в том же SQL-statement, что и guard,
поэтому двойное нажатие уводит ровно на один шаг, а не на два. Неопределённые single-
и batch-выплаты остаются locked до явного admin review после проверки chain. В `/jobs` админ может
двухшагово удалить exact single-offer поколения accepted/processing: текущий handoff отменяется,
seller lock освобождается, response становится `cancelled`, а исходные response/job-фазы и автор
удаления сохраняются в `offer_archive_events`. Фаза `paying` удалению не подлежит.

**Инварианты Codex-ветки (критично):**
1. **Логин — только официальный клиент в PTY; секреты не логируем и не пересылаем.** Бот никогда
   не видит пароль и второй фактор. После device-флоу бот ОДИН РАЗ читает `auth.json` staging-
   каталога, запечатывает OAuth-материал в AEAD envelope (`codex-credential`) в roster движка и
   полностью удаляет staging — открытого токена больше нигде нет. Тип аккаунта до этого проверяется
   строкой `codex login status`, план — claim `chatgpt_plan_type` id_token (free отклоняется).
2. **Незавершённая покупка не оставляет следов.** Истёк код, отказ, не тот тип аккаунта → staging
   удаляется; в roster профиль попадает только после успешного seal. Шаг назад с `cx_wait` тоже
   сносит staging вместе с дочерним процессом. Ожидание device-флоу — явное состояние `cx_wait`, а
   не пустой `want`: после рестарта оно восстанавливается в `cx_email`, потому что дочерний процесс
   рестарт не переживает, а его одноразовый код истекает без присмотра.
3. **Логин уходит через тот же прокси, что и будущий трафик аккаунта** — иначе покупка и
   эксплуатация выглядят как два разных пользователя.
4. **Прокси — секрет:** существует только внутри envelope, никогда не печатается ни в лог, ни в чат.
5. **Roster публикуется атомарно** (tmp+rename, credential 0600, каталог 0700): движок никогда не
   читает половину файла и подхватывает профиль ближайшим health-тиком без рестарта.
6. Бот НЕ правит `config.env`, не рестартит движок и не ходит под root: `AUTH_BOT_CODEX_ROSTER_DIR`
   + keyring — вся его часть контракта.

**Инварианты Gemini-ветки (критично):**
1. Новый handoff — две отдельные client-bound OAuth-транзакции, а не конвертация токена. Сначала
   публичный installed-app client официального Gemini CLI с redirect
   `https://codeassist.google.com/authcode` подтверждает verified Google identity; его токены
   никогда не публикуются и его изменчивый Code Assist ответ не используется для admission. Затем
   новый `state` + PKCE S256 использует
   публичный Antigravity client и фиксированный redirect
   `http://localhost:51121/oauth-callback`. Google subject, canonical proxy и seller-job generation
   обязаны совпасть; legacy proof переносится только внутри state-bound AEAD второй фазы. Продавец
   не создаёт OAuth client и не включает private API в своём проекте.
2. Token exchange, userinfo, Antigravity `loadCodeAssist` и onboarding идут через тот же source
   `node_transport.cjs`, что runtime: SHA-pinned `/usr/bin/node` v24.18.0 Linux/x64, per-account
   authenticated CONNECT и `env_clear`. Legacy-фаза сохраняет client-bound form-order
   `google-auth-library` 10.9.0 и token/userinfo identity; финальная identity pinned к Antigravity
   2.2.1: control plane
   использует `antigravity/hub/2.2.1 darwin/arm64`, onboarding добавляет
   `google-api-nodejs-client/10.3.0`, token exchange — `Go-http-client/2.0`; userinfo идёт через
   attested Node-internal Undici
   dispatcher (его headers, pooling и ClientHello нельзя подменять gaxios-профилем). Proxy/bearer/form
   существуют в zeroizing IPC buffers; Rust TLS и ambient proxy не участвуют. `loadCodeAssist`
   передаёт `ideType=ANTIGRAVITY`, а onboarding — Antigravity ide name/version metadata.
3. OAuth codes/tokens никогда не идут через Telegram. На legacy-фазе продавец копирует показанный
   Google одноразовый Gemini CLI code в no-store HTTPS-форму. На Antigravity-фазе localhost может
   не открыться; продавец копирует полный URL из адресной строки в отдельную форму. Parser проверяет
   exact HTTP localhost:51121 path, callback state и отсутствие credentials/fragment/OAuth error.
   Короткоживущий
   proxy в SQLite только как XChaCha20-Poly1305 envelope, привязанный AAD к одноразовому state;
   form/callback claim одноразовый.
4. Legacy-фаза проверяет verified userinfo и до второго consent выполняет duplicate/proxy
   preflight. Отсутствие проекта/tier на legacy Code Assist surface не доказывает ни совместимость,
   ни несовместимость аккаунта, поэтому authoritative tier/project admission выполняется только
   после свежего Antigravity consent. Принимаются только известные
   Google AI Pro/Ultra, Code Assist Standard/Enterprise и Workspace AI Ultra. Free, Plus,
   несовместимые Workspace и unknown future paid tiers fail-closed. Меню создания оффера показывает
   только Google AI Pro/Ultra; организационные tier продолжают распознаваться для совместимости
   старых callback и фактической проверки плана после OAuth.
   После финального tier check выполняется ровно один non-streaming
   `gemini-2.5-flash-lite:generateContent` через production-pinned sandbox host и runtime headers;
   нужны 2xx, wrapped candidate и ненулевая authoritative `usageMetadata`. 503, malformed response,
   missing usage или ambiguous transport не публикуют credential и не завершают выплату; paid
   generation автоматически не повторяется. `countTokens`, quota и `loadCodeAssist` не являются
   acceptance.
5. Google subject — quota identity: два РАЗНЫХ subject не могут делить профиль, а один subject
   всегда занимает ровно один профиль. Legacy preflight распознаёт уже опубликованный Antigravity
   subject ДО проверки изменчивого tier display и второго consent, поэтому повтор уже подключённого
   аккаунта возвращает exact duplicate outcome, а не ложное «подписка не найдена», и не аннулирует
   живой refresh-token. Существующий
   legacy-профиль может мигрировать в Antigravity только с тем же subject/proxy; id профиля, roster и
   IPRoyal lifecycle сохраняются. In-flight Antigravity callback старой версии остаётся совместимым
   и при exact same subject/proxy может атомарно заменить материал на месте, потому что его consent
   уже мог аннулировать старый token. Смена proxy и обратный переход на legacy fail-closed. Ссылка авторизации
   всегда несёт `prompt=select_account consent`: одного `consent` мало — он подтверждает уже
   залогиненный аккаунт без экрана выбора, и продавец, делающий позиции batch подряд в одном
   профиле браузера, молча переподтверждает предыдущий аккаунт и убивает его токен. Email, subject,
   project, tier, OAuth secret/token и authenticated proxy живут только внутри AEAD.
   Если Google одновременно возвращает `paidTier` и `currentTier`, принимается exact reviewed
   соответствие из любого поля. Точный reviewed tier ID — authority и переживает изменение display
   name; exact имя другого известного плана конфликтует и fail-closed. Неизвестный ID сам по себе и
   знакомые подстроки не дают доступ; exact standalone name остаётся legacy evidence. Два
   противоречащих известных тарифа тоже fail-closed. Перед каждым `unsupported_plan` журнал получает
   только bounded shape-классы: наличие project/paid/current, число allowed tiers и
   `known_id`/`known_name`/`name_drift` без raw tier, project или identity.
6. Credential envelopes и `profiles.json` — `0600`, каталоги — `0700`, symlink/alternate path
   запрещены. Новая публикация пишет сначала envelope, затем atomic roster rename+fsync. Миграция
   сохраняет opaque profile id, roster и существующий IPRoyal lifecycle, атомарно заменяя только
   envelope. После generation acceptance и ожидания publication-lock exact seller-job generation
   проверяется повторно непосредственно перед записью; SQLite и roster не образуют общей транзакции,
   поэтому это минимизирует неизбежное cross-store окно. Startup rewrap переводит старые envelopes
   на active kid, сохраняя online key rotation.
   Ручная смена egress выполняется только локальными operator-командами `gemini-proxy-stage`,
   `gemini-proxy-commit` и `gemini-proxy-rollback` при остановленном Auth Bot: proxy читается из
   stdin, старый envelope остаётся зашифрованным rollback, а runtime подхватывает atomic replace без
   рестарта. Telegram, argv и вывод команды proxy не содержат. Stage не принимает proxy другого
   профиля и сбрасывает IPRoyal order в `0`, потому что внешний proxy бот продлевать не может.
7. После неуспешного OAuth retry сохраняет exact egress для buyer/IPRoyal и seller-proxy. Любая
   ошибка второй фазы начинает новую двухфазную generation; legacy token/project нигде не остаются.
   `transport_unavailable`, control-plane `temporary_upstream` и final
   `generation_unavailable` — разные outcomes, поэтому исправный proxy больше не обвиняется
   сообщением за Google HTTP/malformed response или generation 503. В
   seller-proxy работе команда `повторить` создаёт новую PKCE generation с сохранённым proxy, а новое
   proxy-сообщение явно заменяет его. До инструкции по созданию аккаунта выполняется только локальная
   канонизация URL: speculative CONNECT запрещён, потому что residential gateway может ответить
   transient 403 на сам probe при полностью рабочем allocation. Реальный OAuth transport сериализован
   внутри authbot и различает bounded CONNECT-классы `proxy_auth`, `proxy_throttle`,
   `proxy_rejected`, `proxy_upstream`, `proxy_connect`, `proxy_eof`, `proxy_protocol` и
   `proxy_timeout`. Безопасные pre-target отказы token exchange автоматически повторяются свежим
   helper на 0/2/7/17/37 секунде; после получения токена idempotent userinfo/Code Assist операции
   используют то же bounded recovery. Ambiguous post-send timeout/network никогда не переигрывает
   одноразовый authorization code. Transport-журнал содержит только номер попытки и bounded-класс,
   никогда не URL/credentials прокси. `назад` с `gm_wait` восстанавливает egress из запечатанной
   PKCE-транзакции, а не спрашивает прокси заново (`start_gemini_oauth` стирает `users.hproxy`,
   поэтому другой копии нет); пока callback уже обрабатывает код, откат отказывает, а не гоняется с
   обменом. Закреплённый прокси откат не стирает никогда — прежний `/cancel` делал это безусловно и
   этим намертво запирал работу с прокси покупателя.
8. **Реконструкция прокси из сообщения продавца обратима.** `ip:port:user:pass` режется ровно на
   четыре поля (пароль может содержать `:`), а userinfo процент-кодируется в unreserved-набор,
   потому что канонизация ниже по стеку ДЕКОДИРУЕТ процент-последовательности: без кодирования
   литеральный `%41` в пароле превращается в `A`, а `/`, `?`, `#` рвут разбор authority. Любая
   потеря здесь уходит в CONNECT как чужой пароль и возвращается классом `proxy_auth`, который
   неотличим от мёртвого прокси без ручного расследования. Форма `ip:port` остаётся валидной
   (авторизация по IP), но продавцу явно сообщается, что логин и пароль не распознаны. Отвергнутый
   ввод логируется только бесключевым отпечатком (форма, валидность хоста/порта, длины полей).
**Секреты:** `AUTH_BOT_TOKEN`, ключ BSC-выплат, Claude/Gemini credentials и прокси — только в
`authbot.env` или закрытых runtime-файлах (вне репо). Не коммитить, не печатать.

**Env:**
- `AUTH_BOT_TOKEN`, `AUTH_BOT_ADMIN`, `AUTH_BOT_FLEET`, `CLAUDE_API_DATABASE_URL` — база.
- `AUTH_BOT_CLAUDE_BIN` (приоритет) / `CLAUDE_BIN` — claude CLI для Claude-ветки. Production unit
  задаёт первый и bind-mount'ит официальный install read-only в `/run/claude-authbot/claude`, не
  открывая остальной home; legacy `CLAUDE_BIN` из `authbot.env` не может переопределить unit path.
- `AUTH_BOT_CLAUDE_CONFIG_DIR` — writable-корень изолированных Claude-сессий (деф
  `/srv/claude-api/data/authbot`); токены и состояние не должны лежать в home.
- `AUTH_BOT_CODEX_BIN` — пиннованный codex CLI (деф `/srv/claude-api/data/codex/bin/codex`).
- `AUTH_BOT_CODEX_HOMES_DIR` — staging-каталог device-флоу (скрытые каталоги логина; НЕ пул).
- `AUTH_BOT_CODEX_ROSTER_DIR` — корень `credentials/` + `profiles.json` движка (деф
  `/srv/claude-api/data/codex`); движковый `CLAUDE_API_CODEX_PROFILES_FILE` должен указывать на
  `<этот каталог>/profiles.json`.
- `AUTH_BOT_CODEX_CREDENTIAL_KEYS`, `AUTH_BOT_CODEX_CREDENTIAL_ACTIVE_KID` — общий с runtime
  AEAD keyring и активный ключ публикации/rotation (`CLAUDE_API_CODEX_CREDENTIAL_KEYS` у движка).
- `AUTH_BOT_GEMINI_DIR` — корень `credentials/` + `profiles.json` (деф
  `/srv/claude-api/data/gemini`); движковый `CLAUDE_API_GEMINI_PROFILES_FILE` должен указывать на
  `<этот каталог>/profiles.json`.
- `AUTH_BOT_GEMINI_REDIRECT_URI`, `AUTH_BOT_GEMINI_OAUTH_BIND` — публичная HTTPS-форма приёма
  одноразового кода (`…/oauth/callback`) + её loopback bind. Legacy-название redirect сохранено для
  совместимости env; Google получает фиксированный localhost redirect Antigravity.
- `AUTH_BOT_GEMINI_CREDENTIAL_KEYS`, `AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID` — общий с runtime
  AEAD keyring и активный ключ публикации/rotation.
- `AUTH_BOT_IPROYAL_KEY` — авто-выпуск прокси (пусто = ручной ввод).

Фоновый lifecycle-контроль обновляет срок прокси и при необходимости продлевает тот же IPRoyal
allocation, но не отправляет периодические отчёты «Контроль прокси» в Telegram.

**Деплой:** watchdog собирает бот вместе с движком и кладёт протестированный бинарь в immutable
engine release; `claude-authbot.service` запускает `/srv/claude-api/releases/current/authbot`.
Изменённый бинарь перезапускается после promotion. На startup потерянный in-memory Claude child
восстанавливается из persisted `ho_code` в `ho_email`, а прерванное ожидание ChatGPT — из `cx_wait`
в `cx_email`; продавец присылает email и получает свежий flow.

**Проверка:** `cargo test -p authbot`. Живой прогон Telegram/OAuth/Google API — только на сервере.
