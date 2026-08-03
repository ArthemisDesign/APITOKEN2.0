# apiToken.sale OpenCode router plugin

Канонический config-plugin для OpenCode. На старте он получает персональный `/v1/models`,
переводит authoritative limits/capabilities и актуальные цены в штатную model schema OpenCode и
добавляет Fast entries с исходным model ID только при опубликованном `priority` tier. Modalities,
attachments, tool calling, structured output, reasoning и variants берутся из
`apitoken.capabilities`; эвристик по `owned_by` или подстроке в model ID нет. Router-owned presets
не добавляются в OpenCode provider list: у них динамическая модель и переменная цена.

Установить plugin можно копированием `apitoken-router.js` в
`~/.config/opencode/plugin/apitoken-router.js` (или в auto-load каталог
`~/.config/opencode/plugins/`). Клиентам сайт предлагает one-click installer
`https://raw.githubusercontent.com/apitokensale-admin/apitoken.sale/main/opencode/install.sh`,
который скачивает опубликованную копию этого файла из репозитория
`apitokensale-admin/apitoken.sale` (`opencode/apitoken-router.js`) и добавляет provider
`apitoken` в `~/.config/opencode/opencode.jsonc`. При изменении плагина опубликованную
копию нужно обновить в том же релизе. Provider `apitoken` в `opencode.jsonc` должен
использовать `@ai-sdk/openai-compatible`, `https://router.apitoken.sale/v1` и literal
`sk-pool-…` key либо стандартный OpenCode placeholder `{env:NAME}`.

Plugin рекламирует всем моделям только text output. Это намеренное ограничение OpenCode 1.18.11:
его `@ai-sdk/openai-compatible` 2.0.41 не декодирует нативный Gemini `inlineData` и не принимает
OpenRouter image metadata в Chat message. Нативная генерация картинок в gateway не отключена —
`google/gemini-3.1-flash-image` нужно вызывать через Gemini
`generateContent`/`streamGenerateContent`, где изображение возвращается в
`candidates[].content.parts[].inlineData`.

При временной недоступности каталога plugin может восстановить только capability metadata из
локального last-good cache schema v2 (`catalog-v2.json`). Снимок AES-256-GCM зашифрован и привязан
к точным credential/base URL,
имеет режим `0600`, 15-минутный freshness TTL и предельный stale age 7 дней. Cached-модели явно
помечаются `[stale metadata; pricing unavailable]`; `cost` не кэшируется и до следующего успешного
live discovery в OpenCode не показывается. Другой ключ, другой URL, истёкший, повреждённый или
неизвестной версии cache отклоняется; старый v1 не переиспользуется, потому что в нём нет
authoritative modality/control полей.

Проверка:

```bash
pnpm --filter @claude-api/opencode-router-plugin test
```

Plugin-файл намеренно имеет ровно один ESM export — default factory. OpenCode 1.18.11 пытается
загрузить каждый export модуля как plugin factory, поэтому даже test-only named export ломает весь
provider на старте; export shape закреплён unit-тестом и реальным `opencode models apitoken` smoke.
