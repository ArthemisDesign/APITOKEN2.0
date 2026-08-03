# apiToken.sale OpenCode router plugin

Канонический config-plugin для OpenCode. На старте он получает персональный `/v1/models`,
переводит authoritative limits/capabilities и актуальные цены в штатную model schema OpenCode и
добавляет GPT Fast entries с исходным model ID.

Установить plugin можно копированием `apitoken-router.js` в
`~/.config/opencode/plugin/apitoken-router.js`. Provider `apitoken` в `opencode.jsonc` должен
использовать `@ai-sdk/openai-compatible`, `https://router.apitoken.sale/v1` и literal
`sk-pool-…` key либо стандартный OpenCode placeholder `{env:NAME}`.

При временной недоступности каталога plugin может восстановить только capability metadata из
локального last-good cache. Снимок AES-256-GCM зашифрован и привязан к точным credential/base URL,
имеет режим `0600`, 15-минутный freshness TTL и предельный stale age 7 дней. Cached-модели явно
помечаются `[stale metadata; pricing unavailable]`; `cost` не кэшируется и до следующего успешного
live discovery в OpenCode не показывается. Другой ключ, другой URL, истёкший, повреждённый или
неизвестной версии cache отклоняется.

Проверка:

```bash
pnpm --filter @claude-api/opencode-router-plugin test
```

Plugin-файл намеренно имеет ровно один ESM export — default factory. OpenCode 1.18.11 пытается
загрузить каждый export модуля как plugin factory, поэтому даже test-only named export ломает весь
provider на старте; export shape закреплён unit-тестом и реальным `opencode models apitoken` smoke.
