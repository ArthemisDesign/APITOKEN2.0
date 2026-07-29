# crates/authbot — CLAUDE.md

**Роль:** пополнение пула — Telegram-бот покупки подписок. Компонент-ПРОИЗВОДИТЕЛЬ: стоит ВНЕ
слоёв API (`registry←pool←forward←server`), ПЕРЕД реестром. Покупает у продавцов доступ и передаёт
его движку.

**Границы (жёстко):**
- Зависит от `registry` (только `authority` — запись подписки) + `tokio`, `portable_pty`, `rusqlite`,
  `reqwest`/`serde` (проверка и публикация Gemini credential).
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
| Шаги продавца | ссылка → `code#state` | ссылка + одноразовый код | свой OAuth client (id+secret) + прокси → hosted Google OAuth callback |
| Как движок узнаёт | reload реестра | скан homes | atomic roster refresh на health-loop |

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
1. Каждый продавец присылает СВОЙ Google Cloud OAuth **Web** client (client_id + client_secret),
   созданный в его собственном проекте, — бот собирает его в хендоффе и seal-ит в state-bound PKCE
   payload и в credential. Это размазывает флот по множеству OAuth-клиентов, чтобы пул нельзя было
   отозвать одним действием. Операторский `AUTH_BOT_GEMINI_CLIENT_ID/SECRET` остаётся только как
   fallback. Всегда hosted callback, `state` + PKCE. User-Agent всегда truthful. Продавец обязан
   добавить наш redirect URI (`…/oauth/callback`) в свой OAuth-клиент.
2. OAuth code/tokens никогда не идут через Telegram. Короткоживущий proxy в SQLite только как
   XChaCha20-Poly1305 envelope, привязанный AAD к одноразовому state; callback claim одноразовый.
3. До публикации проверяются verified userinfo и `loadCodeAssist`; принимаются только известные
   Google AI Pro/Ultra, Code Assist Standard/Enterprise и Workspace AI Ultra. Free, Plus,
   несовместимые Workspace и unknown future paid tiers fail-closed.
4. Google subject — quota identity: дубликаты запрещены даже при другом project/file. Email,
   subject, project, tier, OAuth secret/token и authenticated proxy живут только внутри AEAD.
5. Credential envelopes и `profiles.json` — `0600`, каталоги — `0700`, symlink/alternate path
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
- `AUTH_BOT_GEMINI_CLIENT_ID`, `AUTH_BOT_GEMINI_CLIENT_SECRET`,
  `AUTH_BOT_GEMINI_REDIRECT_URI`, `AUTH_BOT_GEMINI_OAUTH_BIND` — hosted OAuth callback + fallback
  operator client (продавцы обычно присылают свой client id/secret через бота).
- `AUTH_BOT_GEMINI_CREDENTIAL_KEYS`, `AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID` — общий с runtime
  AEAD keyring и активный ключ публикации/rotation.
- `AUTH_BOT_IPROYAL_KEY` — авто-выпуск прокси (пусто = ручной ввод).

**Деплой:** watchdog собирает бот вместе с движком и кладёт протестированный бинарь в immutable
engine release; `claude-authbot.service` запускает `/srv/claude-api/releases/current/authbot`.
Изменённый бинарь перезапускается после promotion. На startup потерянный in-memory Claude child
восстанавливается из persisted `ho_code` в `ho_email`; продавец присылает email и получает свежий flow.

**Проверка:** `cargo test -p authbot`. Живой прогон Telegram/OAuth/Google API — только на сервере.
