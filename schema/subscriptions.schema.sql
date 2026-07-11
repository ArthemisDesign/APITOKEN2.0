-- Реестр пула подписок (Claude-аккаунты). Источник истины — SQLite, идентификатор = email.
-- Подписке для форвардинг-прокси нужны только OAuth-токен + прокси (+ статус/флот).
-- Создаётся автоматически (src/db.rs); историческая БД дополняется мягкой миграцией колонок.
CREATE TABLE IF NOT EXISTS subs(
  email       TEXT PRIMARY KEY,               -- аккаунт-подписка
  token       TEXT,                           -- OAuth Bearer подписки, inline (СЕКРЕТ)
  token_file  TEXT,                           -- ИЛИ путь к файлу с токеном (СЕКРЕТ)
  proxy       TEXT,                           -- http://user:pass@ip:port (СЕКРЕТ); "" = напрямую
  plan        TEXT DEFAULT '',                -- pro | max5 | max20 (детект из /api/oauth/profile)
  status      TEXT DEFAULT 'active',          -- active | paused | disabled
  fleet       TEXT DEFAULT 'prod',            -- пул берёт только подписки своего флота (SUBS_FLEET)
  added_ts    INTEGER,
  added       TEXT);
