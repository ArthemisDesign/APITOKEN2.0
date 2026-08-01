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
| Шаги продавца | ссылка → `code#state` | ссылка + одноразовый код | прокси → Antigravity OAuth → полный localhost callback URL в HTTPS-форму |
| Как движок узнаёт | reload реестра | скан homes | atomic roster refresh на health-loop |

Каждый Claude/ChatGPT/Gemini-оффер сразу объясняет новичку весь будущий путь. После выплаты бот
выдаёт отдельный прокси и подробно проводит через новый профиль антидетект-браузера,
самостоятельную регистрацию и активацию нужного тарифа, затем через соответствующую авторизацию.
Gemini ждёт отдельного подтверждения «Аккаунт готов» и только после него показывает ссылки. На всех шагах подчёркнуто:
до прокси аккаунт не открывать, профиль/IP не менять, пароли, cookie и платёжные данные не присылать.
Если автоматическая выдача недоступна, продуктовый fallback отдельно запрашивает и проверяет прокси,
не отправляя новичка дальше с неполной инструкцией.
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
текущего batch есть двухшаговая кнопка возврата ровно на предыдущую позицию. Неопределённые single-
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
   удаляется; в roster профиль попадает только после успешного seal.
3. **Логин уходит через тот же прокси, что и будущий трафик аккаунта** — иначе покупка и
   эксплуатация выглядят как два разных пользователя.
4. **Прокси — секрет:** существует только внутри envelope, никогда не печатается ни в лог, ни в чат.
5. **Roster публикуется атомарно** (tmp+rename, credential 0600, каталог 0700): движок никогда не
   читает половину файла и подхватывает профиль ближайшим health-тиком без рестарта.
6. Бот НЕ правит `config.env`, не рестартит движок и не ходит под root: `AUTH_BOT_CODEX_ROSTER_DIR`
   + keyring — вся его часть контракта.

**Инварианты Gemini-ветки (критично):**
1. OAuth использует публичный installed-application client id/secret Antigravity и фиксированный
   redirect `http://localhost:51121/oauth-callback`. Продавец не создаёт Cloud
   OAuth-клиент и не включает private API в своём проекте. Всегда `state` + PKCE S256; а
   client id/secret и redirect, использованные при старте, seal-ятся
   вместе с транзакцией, чтобы token exchange не мог сменить identity.
2. Token exchange, userinfo, `loadCodeAssist` и повторяемый до `done` onboarding идут через тот же source
   `node_transport.cjs`, что runtime: SHA-pinned `/usr/bin/node` v24.18.0 Linux/x64, per-account
   authenticated CONNECT и `env_clear`. HTTP identity pinned к Antigravity 2.2.1: control plane
   использует `antigravity/hub/2.2.1 darwin/arm64`, onboarding добавляет
   `google-api-nodejs-client/10.3.0`, token exchange — `Go-http-client/2.0`; userinfo идёт через
   attested Node-internal Undici
   dispatcher (его headers, pooling и ClientHello нельзя подменять gaxios-профилем). Proxy/bearer/form
   существуют в zeroizing IPC buffers; Rust TLS и ambient proxy не участвуют. `loadCodeAssist`
   передаёт `ideType=ANTIGRAVITY`, а onboarding — Antigravity ide name/version metadata.
3. OAuth code/tokens никогда не идут через Telegram. После Google redirect страница localhost может
   не открыться; продавец копирует полный URL из адресной строки и POST-ит его через no-store
   HTTPS-форму Auth Bot. Parser проверяет exact HTTP localhost:51121 path, callback state и
   отсутствие credentials/fragment/OAuth error. Короткоживущий
   proxy в SQLite только как XChaCha20-Poly1305 envelope, привязанный AAD к одноразовому state;
   form/callback claim одноразовый.
4. До публикации проверяются verified userinfo и `loadCodeAssist`; принимаются только известные
   Google AI Pro/Ultra, Code Assist Standard/Enterprise и Workspace AI Ultra. Free, Plus,
   несовместимые Workspace и unknown future paid tiers fail-closed. Меню создания оффера показывает
   только Google AI Pro/Ultra; организационные tier продолжают распознаваться для совместимости
   старых callback и фактической проверки плана после OAuth.
5. Google subject — quota identity: дубликаты запрещены даже при другом project/file. Единственное
   исключение — односторонний переход уже опубликованного `LegacyGeminiCli` credential на
   `Antigravity` для того же subject и того же канонического proxy. Antigravity→Antigravity,
   обратный переход и смена proxy fail-closed. Email, subject, project, tier, OAuth secret/token и
   authenticated proxy живут только внутри AEAD.
6. Credential envelopes и `profiles.json` — `0600`, каталоги — `0700`, symlink/alternate path
   запрещены. Новая публикация пишет сначала envelope, затем atomic roster rename+fsync. Миграция
   сохраняет opaque profile id, roster и существующий IPRoyal lifecycle, атомарно заменяя только
   envelope. Startup rewrap переводит старые envelopes на active kid, сохраняя online key rotation.
7. После неуспешного OAuth retry сохраняет exact egress для buyer/IPRoyal и seller-proxy. В
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
   никогда не URL/credentials прокси.
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
восстанавливается из persisted `ho_code` в `ho_email`; продавец присылает email и получает свежий flow.

**Проверка:** `cargo test -p authbot`. Живой прогон Telegram/OAuth/Google API — только на сервере.
