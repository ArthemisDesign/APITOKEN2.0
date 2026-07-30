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

| | Claude | ChatGPT (Codex) | Gemini Code Assist OAuth |
|---|---|---|---|
| Результат | `sk-ant-oat01-…` | ничего, что нам можно читать | refresh/access token + Google subject/project/tier |
| Чем становится покупка | строка в реестре | каталог `CODEX_HOME` | AEAD envelope + opaque запись в `profiles.json` |
| Модуль | `setup_token.rs` | `codex_login.rs` | `gemini_oauth.rs` |
| Шаги продавца | ссылка → `code#state` | ссылка + одноразовый код | прокси → официальный Gemini CLI OAuth → одноразовый код в HTTPS-форму |
| Как движок узнаёт | reload реестра | скан homes | atomic roster refresh на health-loop |

Каждый Claude/ChatGPT/Gemini-оффер сразу объясняет новичку весь будущий путь. После выплаты бот
выдаёт отдельный прокси и подробно проводит через новый профиль антидетект-браузера,
самостоятельную регистрацию и активацию нужного тарифа, затем через соответствующую авторизацию.
Gemini ждёт отдельного подтверждения «Аккаунт готов» и только после него показывает ссылки. На всех шагах подчёркнуто:
до прокси аккаунт не открывать, профиль/IP не менять, пароли, cookie и платёжные данные не присылать.
Если автоматическая выдача недоступна, продуктовый fallback отдельно запрашивает и проверяет прокси,
не отправляя новичка дальше с неполной инструкцией.
В быстрой админской клавиатуре доступны Claude, ChatGPT Plus/Pro и выбранные Google AI планы.

**Инварианты Codex-ветки (критично):**
1. **Auth store не читаем, не логируем, не пересылаем.** Единственное, что бот берёт из профиля, —
   строка `codex login status` (нужно убедиться, что это ChatGPT-подписка, а не вход по API-ключу).
2. **Незавершённая покупка не оставляет следов.** Истёк код, отказ, не тот тип аккаунта → каталог
   удаляется. Каталог без `auth.json` движок в пул не берёт, но мусор не копим.
3. **Логин уходит через тот же прокси, что и будущий трафик аккаунта** — иначе покупка и
   эксплуатация выглядят как два разных пользователя.
4. **Прокси — секрет:** `proxy.url` пишется 0600, каталог 0700, значение никогда не печатается
   ни в лог, ни в чат. Движок откажется брать home с world-readable `proxy.url`.
5. Бот НЕ правит `config.env`, не рестартит движок и не ходит под root: каталог в
   `AUTH_BOT_CODEX_HOMES_DIR` — вся его часть контракта.

**Инварианты Gemini-ветки (критично):**
1. OAuth использует публичный installed-application client id/secret из официального Gemini CLI и
   его фиксированный redirect `https://codeassist.google.com/authcode`. Продавец не создаёт Cloud
   OAuth-клиент и не включает private API в своём проекте. Всегда `state` + PKCE S256; а
   client id/secret и redirect, использованные при старте, seal-ятся
   вместе с транзакцией, чтобы token exchange не мог сменить identity.
2. Token exchange, userinfo, `loadCodeAssist`, onboarding и operation poll идут через тот же source
   `node_transport.cjs`, что runtime: SHA-pinned `/usr/bin/node` v24.18.0 Linux/x64, per-account
   authenticated CONNECT и `env_clear`. Token/Code Assist повторяют gaxios/google-auth-library
   10.9.0; userinfo отдельно повторяет официальный global fetch через attested Node-internal Undici
   dispatcher (его headers, pooling и ClientHello нельзя подменять gaxios-профилем). Proxy/bearer/form
   существуют в zeroizing IPC buffers; Rust TLS и ambient proxy не участвуют. `loadCodeAssist`/
   onboard bodies повторяют Gemini CLI 0.53.0 без custom `client-metadata` header или выдуманного mode.
3. OAuth code/tokens никогда не идут через Telegram. Google показывает одноразовый code на своей
   Code Assist странице; продавец POST-ит его через no-store HTTPS-форму Auth Bot. Короткоживущий
   proxy в SQLite только как XChaCha20-Poly1305 envelope, привязанный AAD к одноразовому state;
   form/callback claim одноразовый.
4. До публикации проверяются verified userinfo и `loadCodeAssist`; принимаются только известные
   Google AI Pro/Ultra, Code Assist Standard/Enterprise и Workspace AI Ultra. Free, Plus,
   несовместимые Workspace и unknown future paid tiers fail-closed. Меню создания оффера показывает
   только Google AI Pro/Ultra; организационные tier продолжают распознаваться для совместимости
   старых callback и фактической проверки плана после OAuth.
5. Google subject — quota identity: дубликаты запрещены даже при другом project/file. Email,
   subject, project, tier, OAuth secret/token и authenticated proxy живут только внутри AEAD.
6. Credential envelopes и `profiles.json` — `0600`, каталоги — `0700`, symlink/alternate path
   запрещены. Сначала envelope, затем atomic roster rename+fsync. Startup rewrap переводит старые
   envelopes на active kid, сохраняя online key rotation.
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
- `AUTH_BOT_CODEX_HOMES_DIR` — каталог покупок; ДОЛЖЕН совпадать с движковым
  `CLAUDE_API_CODEX_HOMES_DIR`, иначе купленный аккаунт никто не подхватит.
- `AUTH_BOT_GEMINI_DIR` — корень `credentials/` + `profiles.json` (деф
  `/srv/claude-api/data/gemini`); движковый `CLAUDE_API_GEMINI_PROFILES_FILE` должен указывать на
  `<этот каталог>/profiles.json`.
- `AUTH_BOT_GEMINI_REDIRECT_URI`, `AUTH_BOT_GEMINI_OAUTH_BIND` — публичная HTTPS-форма приёма
  одноразового кода (`…/oauth/callback`) + её loopback bind. Legacy-название redirect сохранено для
  совместимости env; Google получает фиксированный redirect официального Gemini CLI.
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
