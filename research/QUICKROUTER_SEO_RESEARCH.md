# QuickRouter.ai — глубокий SEO-ресерч и сравнение с apitoken.sale (2026-07-28)

## Кто они

**QuickRouter.AI (快游API)** — китайский «API 中转站» (relay): единый Base URL / ключ для 400+ моделей
(OpenAI, Claude, Gemini, DeepSeek, Grok…), оплата в юанях, «国内直连» (прямой доступ из Китая без VPN).
Весь сайт **только zh_CN** — их SEO нацелен на китайский рынок (Baidu + Google zh). Мы с ними почти
не пересекаемся в выдаче (у нас EN/RU + learn на zh/ko), но их тактики переносимы.

## Их индекс: ~603 URL

`robots.txt` → `sitemap-index.xml` → 2 сайтмапа:
- **quickrouter.ai — 144 URL**: `/blog` 36, `/errors` 31, `/tutorials` 23, `/questions` 16,
  `/ai-quant` 12, `/models` 7, `/use-cases` 6, `/tools` 5, `/compare` 5, + `/chat`, `/openclaw`.
- **doc.quickrouter.ai — 459 URL**: каждый endpoint/пример API — отдельная HTML-страница
  (`claude-native-pdf.html`, `chat-function-call.html`, …).

Наш живой sitemap: **225 URL** (60 /ru, 48 /docs, 46 /zh, 46 /ko, 6 /models, 2 /blog, 1 /tools, core).

## Технический срез (их)

- Главная: title «大模型 API 中转站｜OpenAI/Claude/Gemini/DeepSeek/Grok 国内直连», meta keywords
  (для Baidu это ещё сигнал), canonical, OG zh_CN, JSON-LD: WebSite + Organization +
  **SoftwareApplication** + FAQPage.
- Шаблонные страницы: BreadcrumbList + FAQPage почти на каждой; полный SSR-текст (1300–4000 «токенов»);
  14–18 уникальных внутренних ссылок на страницу.
- Блог-статьи: Article schema с datePublished, ~3000+ слов, свежайшие темы (Claude Fable 5,
  GPT-5.5, Gemini 3 Pro, Kimi K2.7 — обзор под каждый релиз модели), blog changefreq=daily.
- НЕТ: hreflang/локалей (только zh), llms.txt (404), RSS не замечен, md-гейтвея нет.

## Чем они ЛУЧШЕ нас (по убыванию ценности)

### 1. Программатик-кластер /errors — 31 страница «инструмент × ошибка»
`/errors/claude-code/api-error-429`, `/errors/cursor/unable-to-reach-model-provider`,
`/errors/codex/config-toml-error`, `/errors/trae/...`, `/errors/cherry-studio/...`, `/errors/opencode/...`
Ловят запросы по ТОЧНОМУ тексту ошибки. Пользователь со сломанным Claude Code/Cursor — самый горячий лид.

**ПОПРАВКА 2026-07-28 (после публикации первой версии этого дока):** у нас errors УЖЕ ЕСТЬ —
в тот же день на ветке `feat/error-seo` вышли `/docs/errors` + `/ru/docs/errors` (коммиты
94ff1d7 / 66589ee, в проде, в sitemap): один большой справочник ~3800 слов со ВСЕМИ кодами
(тексты ошибок verbatim), TechArticle+FAQPage, MD-двойник, шорт-линки `/e/<code>`; своя research —
`research/` в коммите 30b0490, где выбор «одна страница вне learn-кластера» сделан осознанно,
ставка — на незанятую RU-нишу. Остаточный гэп против quickrouter: (а) у них 31 ОТДЕЛЬНЫЙ URL —
title каждой страницы = точная формулировка запроса, у нас title один на все коды; (б) их кластер
СКОУПЛЕН ПО ИНСТРУМЕНТАМ (cursor/codex/trae/cherry-studio/opencode) — запросы вида «cursor provider
returned 401» наш справочник по тайтлу не ловит.

### 2. Скорость контента: обзор под каждый релиз модели (36 постов блога)
claude-fable-5-review, claude-sonnet-5-review, gpt-5.5-review, gemini-3-pro-review, grok-4-5,
kimi-k2-7, glm-5-2… Каждый релиз модели = всплеск поиска, они его снимают в первые дни.
У нас /blog есть (бэкенд-CMS, Article schema, citation) — но в sitemap всего 2 поста.
Инфраструктура готова, контента нет.

### 3. Вертикальная ниша /ai-quant — 12 страниц + квант-туториалы
Туториалы «подключи наш API к X»: TradingAgents(-CN), FinGPT, FinRL, Qlib, ai-hedge-fund,
vibe-trading, alpaca-trading-agent… Захватили целую нишу (AI-квант-трейдинг), где юзерам нужен
именно их продукт, а конкуренции в выдаче почти нет. У нас вертикалей нет вообще.

### 4. Бесплатные тулзы-магниты /tools — 5 штук
openai-base-url-tester, api-price-calculator, **claude-code-config-generator**,
**cursor-config-generator**. Генераторы конфигов = утилита + прямой онбординг на их Base URL.
У нас 1 тулза (cost calculator).

### 5. doc.quickrouter.ai — 459 индексируемых страниц документации
Каждый вызов/объект/пример — отдельный URL: длинный хвост запросов «claude api pdf example»,
«function call stream» и т.п. Наш /docs — одностраничный портал = 1 URL в индексе.

### 6. /questions — 16 страниц под вопросные запросы
Title = дословный вопрос («Claude API 国内怎么用？»). У нас часть закрыта learn-кластером,
но их URL/тайтлы бьют точнее в формулировку запроса и живут на верхнем уровне, не под /docs/learn/.

### 7. Структура «инструмент-центричная», шире нашей
errors/tutorials/use-cases размечены по инструментам: claude-code, cursor, codex, opencode,
cherry-studio, trae, cline. У нас 6 int-* (нет trae, cherry studio, opencode, codex).

## Чем МЫ лучше их

- **i18n**: 4 локали learn + hreflang + x-default; у них zh-only, hreflang нет.
- **AI-краулинг (GEO)**: llms.txt ×8, /md-гейтвей, RSS, Content-Signal, allowlist 27 AI-ботов;
  у них llms.txt = 404.
- Богаче schema-покрытие типов (Service+OfferCatalog, WebApplication, CollectionPage, TechArticle,
  генерируемые OG-картинки, SEO-инварианты в тестах, IndexNow-скрипт).
- Compare-кластер у нас крупнее (10 против их 5), но спрятан под /docs/learn/.

## Наши баги, найденные при аудите (apps/web)

1. `/ru` рендерит английский JSON-LD (те же @id, inLanguage:en) — app/ru/page.tsx:13.
2. `/ru/docs` без hreflang, og:locale=en_US, не в sitemap.
3. `/blog/[slug]` без hreflang при наличии locale у поста.
4. zh/ko-хабы learn ссылаются на английский /docs (learn-article.tsx:43-45).
5. `alternates` страниц затирают RSS-альтернейт из layout.
6. twitter.site/creator не заданы; lastModified 14 core-страниц — константа 2026-07-16.
7. indexnow.mjs / google-sitemap.mjs не подключены к деплою (ручной запуск).

## Что перенять (приоритезировано)

1. **Достроить errors-канал**: справочник `/docs/errors` уже в проде (см. поправку выше);
   осталась tool-скоупная часть — отдельные страницы «инструмент × ошибка» (Cursor × {provider 401,
   rate limit, unable to reach}, Codex × {config.toml, auth.json}, Cline/Zed), чтобы ловить запросы
   с названием инструмента в формулировке, которые общий тайтл справочника не покрывает.
2. **Разогнать /blog**: обзор + прайс-гайд под КАЖДЫЙ релиз Claude-модели в день релиза
   (инфраструктура уже есть).
3. **Тулзы**: Claude Code config generator (env/settings.json под наш base URL), base-url/key tester,
   Cursor config generator.
4. **Вертикальный кластер** «подключи Claude к X»: n8n, LangChain, LiteLLM, LibreChat, agent-фреймворки
   (синергия с GEO-GitHub стратегией P0).
5. Разбить /docs на многостраничную индексируемую документацию (по примеру их doc-поддомена).
6. Починить пункты 1–7 из списка багов.

## Источники

- https://quickrouter.ai/ (+ sitemap.xml, sitemap-index.xml, robots.txt, шаблоны страниц — скачаны в /tmp/qr-seo)
- https://doc.quickrouter.ai/sitemap.xml
- https://routerhubs.com/ — независимый мониторинг 中转站
- https://toolsify.ai/cn/ai/quickrouter-ai, https://aiproducthub.cn/sites/quickrouter-api.html — каталоги
- https://developer.volcengine.com/articles/7630771531187552298 — сторонний туториал по их подключению
