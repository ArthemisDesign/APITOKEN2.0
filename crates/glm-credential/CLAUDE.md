# CLAUDE.md — crates/glm-credential

Локальные границы крейта. Общие правила — корневые `AGENTS.md` и `CLAUDE.md`; контракт
credential — `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6; факты о GLM —
`docs/engine/GLM_PROVIDER.md`.

## Что это

Запечатанные AEAD-конверты статических API-ключей GLM Coding Plan (Zhipu AI / Z.ai).
Крейт **чистый**: XChaCha20-Poly1305, валидация, нормализация — и всё. Он стоит ВНЕ слоёв
`registry ← pool ← forward ← server`, как `kimi-credential`, `gemini-credential` и
`codex-credential`.

## Границы — не нарушать

- **Никакой сети и HTTP.** Крейт не ходит в `api.z.ai`/`open.bigmodel.cn` и не опрашивает
  quota endpoint. Он умеет только `seal`/`open`/`validate` над уже полученным материалом.
  Валидацию ключа probe'ом делает `crates/authbot`, runtime — `crates/forward`.
- **Никакого файлового I/O.** Права `0600`/`0700`, atomic rename, fsync и публикация
  roster — ответственность вызывающего производителя, а не этого крейта.
- **Никакого env.** Keyring приходит строкой `kid:hex[,kid:hex]` из
  `crates/server/src/config.rs`.
- **Никакого digest'а ключа.** Machine-readable subject у GLM нет (`/me` не существует);
  dedup-единица — сам ключ. Сравнение выполняет вызывающий (authbot) на открытых
  конвертах; хеширующую зависимость (blake3 и т.п.) в крейт не добавлять.
- **Зависимости минимальны.** Добавление чего-либо тяжелее текущего набора требует
  обоснования в коммите.

## Инварианты

1. **Секреты не печатаются.** `Debug` у `GlmCredential` и `CredentialKeyring` написан
   вручную и отдаёт `REDACTED` для ключа и прокси. Тест `debug_never_prints_secrets`
   фиксирует, что секреты не утекают ни в `Debug`, ни в `Display` ошибок. Производный
   `Debug` у `GlmCredential` запрещён.
2. **AAD связывает конверт с profile id И с видом credential.** Конверт нельзя перенести
   на соседний профиль; cleartext-поле `kind` — AEAD-вход и после расшифровки
   перепроверяется против содержимого. Вид один (`PlanKey`), но инвариант сохраняется для
   любых будущих видов.
3. **Ключ статический.** Refresh-семьи, expiry и `rotate()` нет и не появляется: ротация —
   это перевыпуск ключа в консоли и повторный `seal` через Auth Bot. Не добавлять
   refresh-поверхность «на будущее».
4. **Base URL — allowlist ровно двух origin'ов** (`https://api.z.ai`,
   `https://open.bigmodel.cn`), хранится в канонической форме без trailing slash; ключи
   int/CN несовместимы между площадками. Чужой хост, непустой путь, query, fragment или
   credentials в URL — отказ при `seal`/`open`. Вызов `normalize_base_url` на входе —
   обязанность вызывающего. Единственное исключение — cargo-фича
   `test-loopback-base-url`: plain-HTTP на `127.0.0.1`/`localhost`/`[::1]` для mock-апстримов
   в тестах потребителей. Она включается только через dev-dependencies (`forward`), в
   production-бинарях allowlist остаётся строгим.
5. **План декларируется оффером** и нормализуется к `lite|pro|max` (`GlmPlan::parse`);
   Team и legacy prompts-планы fail closed. Наблюдённый quota window-limit, противоречащий
   заявленному плану, — забота runtime (профиль вне ротации), не этого крейта.
6. **Window credits официально опубликованы** (docs.z.ai/devpack/overview, ревью
   2026-08-03): lite 2000/5ч + 10000/7д, pro 12000 + 60000, max 28000 + 140000 — поэтому
   `GLM_REVIEWED_PLANS`, в отличие от KIMI, не пуст. Rate-limit/concurrency различия тиров
   НЕ кодируются: они динамические и недокументированы (`unknown`). Все три модели
   (`GLM_PLAN_MODELS`) доступны на всех планах — тоже official.

## Как проверять

```bash
cargo test -p glm-credential
```

Тесты обязаны покрывать: roundtrip, перенос конверта на чужой профиль, подмену `kind`,
чтение старым ключом при онлайн-ротации keyring, неизвестный `kid`, порчу ciphertext,
allowlist base_url (оба origin'а; чужой хост/путь/credentials — отказ; каноническая форма
обязательна), нормализацию и неизвестный план, официальные window credits, границы
profile id и proxy, реконструкцию прокси-userinfo, отсутствие секретов в `Debug` и в
`Display` ошибок.
