---
status: актуален
verified: 2026-07-08
---

# Авторизация Claude-профилей (подключение новой подписки)

Как **подключить новую авторизацию** (Claude-аккаунт/подписку) для флота, как
**перелогинить** протухший профиль и как развести трафик, чтобы агентские вызовы
`claude` не жгли пользовательскую квоту.

> ⚠️ **Без секретов.** Здесь только МЕТОД. Токены живут в keychain / в
> `<profile>/.claude.json` и НЕ коммитятся. Реестр аккаунтов с реальными email'ами
> держится **машинно-локально** в `~/Desktop/apps/CLAUDE_PROFILES.md` (на MacBook
> пользователя) — это source of truth по тому, какой аккаунт где. В репозиторий
> почты не выносим.

---

## 1. Зачем разделение профилей

На машине два (и более) Claude Code-профиля, изолированных каталогом
`CLAUDE_CONFIG_DIR`:

| Кто вызывает | Профиль | Механизм |
|---|---|---|
| **Пользователь в чате** (интерактивный Claude Code) | `~/.claude/` (его Pro/Max) | автоматически — процесс наследует |
| **Агент/пайплайн/скрипт**, шеллящий `claude` | отдельный профиль (другой план) | `CLAUDE_CONFIG_DIR=$AGENT_CLAUDE_CONFIG_DIR` на child-процессе |

Правило: **трафик пользователя → его план; автоматический трафик (тесты,
пайплайны, суб-агенты, верификаторы, генерация) → агентский профиль.** Если
ошибиться — у пользователя деградирует интерактивный чат (rate-limit). Разделение
держится на ОТДЕЛЬНОЙ переменной `AGENT_CLAUDE_CONFIG_DIR`, а не на per-команда
флаге.

> ⚠️ Схема с `AGENT_CLAUDE_CONFIG_DIR`/крейтом `llm_client` — из дев-репо `AGENTS`
> (машина разработчика); в ЭТОМ репозитории её нет. Здесь роутинг профилей делает
> `scripts/subscription.sh`: реестр подписок `subscriptions.json` → `activate`
> проецирует активную в плоские файлы `active_profile`/`active_proxy`, которые
> читают движок (`host_profile()`) и `box_docker.sh` (сидит `.credentials.json`
> в коробку). См. `docs/RUNTIME_FILES.md` §2 и §4.

---

## 2. Текущий flow OAuth (claude CLI v2.1.x и новее) — ВАЖНО

Flow **изменился**. Старого localhost-листенера, который ловил OAuth-callback сам,
**больше нет**. Теперь:

1. CLI печатает OAuth-URL;
2. пользователь логинится в браузере;
3. браузер оказывается на `platform.claude.com/oauth/code/callback?code=XYZ&state=ABC`;
4. CLI просит вставить в приглашение `>` строку **`<code>#<state>`** (оба куска
   через `#`). Только код без `#state` — **молча не срабатывает**.

Используем подкоманду **`claude auth login`**, НЕ слэш-команду `claude /login`
(она только в TUI).

---

## 3. Подключить НОВЫЙ профиль (человек за своим терминалом)

Самый простой путь — интерактивно, FIFO-пляска не нужна (она для агента, см. §6):

```bash
NICK="friend2"                      # короткий слаг → каталог ~/.claude-friend2
EMAIL="<account-email>"             # email их Pro/Max плана
DIR="$HOME/.claude-$NICK"

# 1) каталог + скелет настроек (пропускает первый вопрос про тему)
mkdir -p "$DIR"
echo '{"theme":"dark-daltonized"}' > "$DIR/settings.json"

# 2) интерактивный вход (откроет браузер сам)
CLAUDE_CONFIG_DIR="$DIR" ~/.local/bin/claude auth login --claudeai --email "$EMAIL"
#   → залогиниться нужным аккаунтом в браузере
#   → вставить в приглашение `>` строку  XYZ#ABC  (code#state с callback-URL)

# 3) проверить, что профиль реально живой (дешёвый ping Haiku):
CLAUDE_CONFIG_DIR="$DIR" ~/.local/bin/claude -p "Reply with exactly: ok" \
    --model claude-haiku-4-5-20251001
# → ok
```

Нет `ok` → токен не сохранился → повтори логин.

---

## 4. Перелогинить СУЩЕСТВУЮЩИЙ профиль (токен протух → 401)

То же самое, но в тот же каталог; шаг создания пропускаем. **Никакие code/plist/
zshrc менять не нужно** — каталог тот же, все ссылки на него остаются валидны:

```bash
CLAUDE_CONFIG_DIR="$HOME/.claude-<nick>" ~/.local/bin/claude auth login \
    --claudeai --email "<account-email>"
# затем тот же Haiku-ping для проверки
```

---

## 5. Маршрутизация после подключения

На флоте (этот репозиторий) профиль подключается через реестр подписок:

```bash
# Добавить подписку (интерактивно: OAuth-URL → вставить code#state) и сделать активной:
scripts/subscription.sh add <email> <proxy>
scripts/subscription.sh activate <email>    # пишет проекции active_profile/active_proxy
scripts/subscription.sh list                # что подключено и что активно
```

Движок и `box_docker.sh` подхватывают активный профиль сами (через
`active_profile`/`active_proxy`) — код не меняем, рестарт не обязателен
(коробки сидятся кредами на следующем ходе).

На дев-машине (репо `AGENTS`, вне этого репозитория) — переменная
`AGENT_CLAUDE_CONFIG_DIR` в `~/.zshrc`, её читает тамошний `llm_client`.

### 5.1. Откуда физически берутся подписки — покупка у продавцов (ОТДЕЛЬНЫЙ репозиторий)

Ручной OAuth-логин (§3/§4) — не единственный источник. Основной канал пополнения
пула — **покупка готовых Claude-подписок у продавцов через Telegram-бота**. Этот
бот живёт **вне данного репозитория**, в отдельном GitHub-репо:

> **`https://github.com/Q666Q666Q/CLAUDE_SETUP_TOKEN_BOT`** — Rust-бот, каталог
> исходников `rust/src/`. На сервере деплоится в
> `/srv/agents/AgentsCloningFork/tools/auth_token_bot/` (там же лежит собранный
> бинарь `auth_token_bot_rs`), крутится как systemd-юнит **`claude-auth-bot.service`**
> (`journalctl -u claude-auth-bot -f` — логи, `systemctl status claude-auth-bot` —
> статус). Общий с этим репо только шаблон юнита и зашифрованный env
> (`tools/auth_token_bot/{claude-auth-bot.service,auth_bot.env.gpg}` в ЭТОМ
> репозитории — секреты/деплой-обвязка, а не код бота).

**Флоу (снаружи для продавца — обычные офферы, «токен»/«setup-token» не
упоминаются):**
1. Новый человек жмёт «✅ Стать продавцом» → заявка админу → одобрение.
2. Админ создаёт оффер (название → цена → **выбор стенда: DevStand / Test+Main**)
   → рассылка одобренным продавцам.
3. Продавец принимает → подтверждает BEP-20-адрес → админ подтверждает выплату →
   бот сам шлёт USDT (BEP-20, ончейн через `alloy`).
4. После оплаты продавец передаёт доступ к Claude-аккаунту: прокси → email →
   `claude setup-token` (PTY) → бот вытаскивает 1-летний токен.
5. **Мост в этот репозиторий:** бот вызывает `scripts/subscription.sh
   register-token <email> <token_file> [proxy] [fleet] [seller] [seller_id]
   [seller_nick]` — так купленная подписка попадает в реестр (`subscriptions.db`,
   см. `docs/RUNTIME_FILES.md`) СРАЗУ в нужный флот (`dev`=DevStand,
   `prod`=Test+Main) и с атрибуцией продавца (username + TG id + никнейм).

**Персист бота-продавца** — своя SQLite `auth_bot.db` (в `AUTH_BOT_STATE_DIR`,
по умолчанию `~/.config/agents/auth_bot_state/`): таблицы `bot_users` (продавцы/
админы) и `bot_offers`/`bot_responses` (офферы и отклики). Никаких JSON-файлов —
исторические `users.json`/`offers.json` мигрируются в БД один раз при первом
старте нового бинаря и переименовываются в `.migrated`. Эту же БД читает
`RUST/tools/stats_web` (реестр сделок «Покупки через бота»), поэтому схему
(`bot_users`/`bot_offers`/`bot_responses`) нельзя менять несовместимо без правки
обеих сторон.

**Если нужно изменить логику бота-продавца** — правки делаются в репозитории
`CLAUDE_SETUP_TOKEN_BOT` (не здесь), затем: `cargo build --release` → скопировать
`target/release/auth_token_bot` поверх `auth_token_bot_rs` на сервере →
`systemctl restart claude-auth-bot`. Перед заменой бинаря на проде — бэкап
текущего (`cp auth_token_bot_rs auth_token_bot_rs.bak-$(date +%s)`) и
`offers.json`/`users.json` (если ещё не мигрированы), это живой платёжный бот.

### Чего НЕЛЬЗЯ
- **Не `export CLAUDE_CONFIG_DIR=…` глобально** — перехватит и интерактивный чат
  пользователя. Для агентского трафика — только `AGENT_CLAUDE_CONFIG_DIR`.
- **Не копировать `.claude.json` (auth-секцию) между машинами** — OAuth-токены
  бывают device-bound. Всегда OAuth заново на целевой машине.
- **Не подменять логин аккаунта на `ANTHROPIC_API_KEY`**, если только явно не нужен
  metered-биллинг по API (это в обход Pro/Max — платишь per-token).
- **Не трогать `~/.claude/`** (основной пользователя) воркером/перелогином — Claude
  Code управляет им динамически.

---

## 6. Неинтерактивный драйв (агент гонит логин из скрипта/по ssh)

Если логин драйвит агент (нет человека у `>`-приглашения) — кормим `code#state` в
процесс через FIFO, открытый в режиме `<>` (read+write, иначе `open()` блокируется):

```bash
mkfifo /tmp/claude-auth-fifo
chmod 600 /tmp/claude-auth-fifo
: > /tmp/claude-auth.log

nohup env CLAUDE_CONFIG_DIR=~/.claude-<nick> \
    ~/.local/bin/claude auth login --claudeai --email "<account-email>" \
    <> /tmp/claude-auth-fifo > /tmp/claude-auth.log 2>&1 &
disown
sleep 4

# вытащить URL и открыть в браузере ПОЛЬЗОВАТЕЛЯ (не на headless-хосте!)
URL=$(grep -o 'https://claude\.com/cai/oauth/authorize[^ ]*' /tmp/claude-auth.log | head -1)
/usr/bin/open "$URL"          # на Linux: xdg-open

# пользователь логинится → сообщает code+state с callback-URL → отдаём в FIFO:
echo "${CODE}#${STATE}" > /tmp/claude-auth-fifo
sleep 3
grep -q 'Login successful' /tmp/claude-auth.log && echo OK
```

---

## 7. Вариант: профиль на Linux-сервере / удалённом headless-хосте

Прод-хаб — Linux-сервер (`root@YOUR_SERVER`, Ubuntu 24.04), флот под systemd.

1. **`claude` CLI там может быть не установлен:**
   ```bash
   ssh root@YOUR_SERVER 'curl -fsSL https://claude.ai/install.sh | bash'
   # бинарь → ~/.local/bin/claude; добавить ~/.local/bin в PATH
   ```
2. **systemd НЕ читает `~/.zshrc`/логин-профиль.** Если пайплайн крутится под
   systemd-юнитом — `AGENT_CLAUDE_CONFIG_DIR` надо ПРОДУБЛИРОВАТЬ через
   `Environment=` в юните (или `EnvironmentFile=`), иначе процесс не увидит
   переменную и тихо свалится на `~/.claude/`.
3. OAuth-драйв тот же (§6), но **URL открывать в браузере на локальной машине
   пользователя** — браузер у него, а не на сервере:
   ```bash
   URL=$(ssh root@YOUR_SERVER 'grep -o "https://claude\.com/cai/oauth/authorize[^ ]*" /tmp/claude-auth.log | head -1')
   open "$URL"   # или xdg-open на Linux-десктопе
   ssh root@YOUR_SERVER "echo '${CODE}#${STATE}' > /tmp/claude-auth-fifo"
   ```

---

## 8. Частые отказы (в 2.1.x)

- **«Redirect URI not supported by client».** Открытый URL поломан мусором (обычно
  `++` от переноса строк в markdown/TUI). Открывай через `/usr/bin/open "$URL"`,
  не copy-paste. Старые code/state мертвы — начинай заново.
- **«OAuth state invalid» / пустая callback-страница.** Процесс `claude auth login`
  убили до конца логина → PKCE-state потерян. Убей остатки, рестарт, новый URL.
- **Приглашение приняло код, ничего не печатает, висит.** Шли `code#state`, не
  только код.
- **«/login isn't available in this environment».** Запускай `claude auth login`
  (подкоманда), не `claude /login`.
- **OAuth-коды одноразовые.** Сорвался flow — убей процесс и начни с новой ссылки,
  не переиспользуй старую.

---

## 9. Свежесть токенов — воркер

OAuth = короткоживущий **access-токен** + долгоживущий **refresh-токен**. Каждый
вызов `claude` авто-обновляет access через refresh. Значит **профиль, который
получает трафик, не протухает**. Риск — только у **простаивающего** агентского
профиля: за недели бездействия refresh может протухнуть → следующий запуск упадёт
на auth-ошибке (как наш `~/.claude-test` → `401`).

Лечение — дешёвый периодический ping (launchd-agent на MacBook): раз в несколько
часов гоняет `claude -p "Reply with exactly: ok" --model claude-haiku-4-5-20251001`
по агентскому профилю (читает тот же `AGENT_CLAUDE_CONFIG_DIR`). ~2 c, копейки,
держит refresh-токен живым. Linux-сервер не нуждается — там профиль и так нагружен
пайплайнами непрерывно. Детали (пути скрипта/плиста, операции
load/unload/cadence) — в машинно-локальном `~/Desktop/apps/CLAUDE_PROFILES.md`.

---

## 10. Чеклист диагностики auth-ошибок в пайплайне

1. `echo $AGENT_CLAUDE_CONFIG_DIR` — задана в текущем shell?
2. `ls $AGENT_CLAUDE_CONFIG_DIR/.claude.json` — токен на месте?
3. `CLAUDE_CONFIG_DIR=$AGENT_CLAUDE_CONFIG_DIR claude -p "ping" --model claude-haiku-4-5-20251001`
   — аккаунт работает сам по себе?
4. (3) падает → токен протух/отозван → перелогинь (§4), позови владельца аккаунта.

---

## Инструмент-автоматика (с оговоркой)

`~/.local/bin/claude-add-profile <nick> <email>` пытается провести §3 end-to-end.
**НО** он писался под СТАРЫЙ flow (localhost-листенер) и может зависнуть на новом
`code#state`. Завис — Ctrl-C и руками по §3. Полный source-of-truth плейбук (с
реестром аккаунтов и email'ами) — `~/Desktop/apps/CLAUDE_PROFILES.md`,
машинно-локально, **не в репозитории**.
