-- Реестр пула подписок (Claude-аккаунты). Общий SQLite = источник истины;
-- subscriptions.json — зеркало/фолбэк. Идентификатор подписки = email.
CREATE TABLE IF NOT EXISTS subs(
  email       TEXT PRIMARY KEY,               -- аккаунт-подписка
  kind        TEXT NOT NULL DEFAULT 'token',  -- token (OAuth) | login
  profile_dir TEXT NOT NULL DEFAULT '',       -- каталог профиля = CLAUDE_CONFIG_DIR (~/.claude-<slug>)
  token_file  TEXT NOT NULL DEFAULT '',       -- <profile_dir>/oauth_token (СЕКРЕТ)
  proxy       TEXT NOT NULL DEFAULT '',       -- http://user:pass@ip:port (СЕКРЕТ)
  plan        TEXT NOT NULL DEFAULT '',       -- pro | max5 | max20
  status      TEXT NOT NULL DEFAULT 'active', -- active | paused | disabled | pending
  source      TEXT NOT NULL DEFAULT '',       -- auth_bot | manual
  fleet       TEXT NOT NULL DEFAULT 'prod',   -- dev | prod (пул берёт только свой флот)
  seller      TEXT NOT NULL DEFAULT '',
  seller_id   TEXT NOT NULL DEFAULT '',
  seller_nick TEXT NOT NULL DEFAULT '',
  refresher   INTEGER NOT NULL DEFAULT 1,     -- держать токен живым фоном
  added_ts    INTEGER NOT NULL DEFAULT 0,
  added       TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS subs_meta(k TEXT PRIMARY KEY, v TEXT NOT NULL DEFAULT '');  -- 'active' = email активной
-- Телеметрия лимитов (без секретов): история опроса rate-limit заголовков.
CREATE TABLE IF NOT EXISTS subs_poll_log(
  ts INTEGER, email TEXT, plan TEXT, util5h REAL, util7d REAL,
  status TEXT, reset5h INTEGER, reset7d INTEGER, cooling INTEGER);
