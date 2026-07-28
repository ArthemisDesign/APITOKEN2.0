# dev.to cross-post — инструкция и журнал

> Канал off-page SEO/GEO: репаблиш статей learn-кластера на dev.to (DR ~90) с
> `canonical_url` на оригинал. Стратегический контекст — `research/GEO_GITHUB_STRATEGY.md`
> (упоминания бренда коррелируют с AI-видимостью 0.664; comparative-контент даёт 2.4×
> брендовых упоминаний). Этот файл — операционка: как публиковать и что уже опубликовано.
> **Журнал внизу обновляй при каждой публикации.**

## Как это работает

- Скрипт `devto-crosspost.mjs` берёт контент с живого markdown-гейтвея
  `https://apitoken.sale/md/docs/learn/<slug>` — публикуется ровно то, что в проде.
- Срезает YAML front matter и H1 (dev.to сам рендерит заголовок), абсолютизирует
  относительные ссылки (`/models` → `https://apitoken.sale/models`), добавляет футер
  «Originally published at …».
- Ставит `canonical_url` на оригинал: дубль-контент штрафа нет, поисковый вес
  консолидируется на apitoken.sale, dev.to показывает «Originally published at» под шапкой.
- Публикация через Forem API v1 (`POST https://dev.to/api/articles`), ретрай на 429.

## Публикация

```bash
cd apps/web
node scripts/devto-crosspost.mjs <slug> [<slug>...] [--dry-run]
```

- Ключ: env `DEVTO_API_KEY` или `~/.config/apitoken/devto.env` (лежит на маке юзера).
  Аккаунт: `api_token_46fac5c7112fe23`. Перевыпуск ключа: dev.to Settings → Extensions.
- Слаги — из `apps/web/src/lib/learn.ts` (EN-версии; локализованные не постим).
- Сначала `--dry-run`, глазами проверить title/description/начало body.
- После публикации открыть URL и проверить: таблицы отрендерены, ссылки ведут на
  apitoken.sale, есть «Originally published at».

## Правила канала

1. **Каденция 2–3 статьи в неделю, максимум.** Весь кластер разом — спам-сигнал для
   dev.to и Google. Rate limit dev.to: ~1 пост / 5 мин (скрипт ретраит сам).
2. **Что постить в первую очередь** (по убыванию SEO/GEO-ценности):
   - comparative (`apitoken-vs-*`, `*-vs-*`) — 2.4× брендовых упоминаний в LLM;
   - статьи с цифрами/таблицами цен — «статистика» (+31% GEO-видимости, Принстон KDD'24);
   - интеграционные с копируемым конфигом (`claude-api-key-for-cursor`, `claude-api-aider`,
     `claude-api-litellm`) — разработчик копирует пример вместе с base_url.
3. **Теги**: ровно 4, lowercase, без дефисов. Карта per-slug — в `TAGS` внутри скрипта;
   для нового слага добавь запись туда (дефолт `ai, claude, api, llm`).
4. **Не редактировать текст под dev.to вручную** — источник правды только гейтвей.
   Правки контента делаются в `learn.ts` (и попадают и на сайт, и в будущие кросс-посты).
5. Механику пула/подписок/ротации в постах и комментариях НЕ упоминать — публично мы
   «Anthropic-compatible API provider» (риск-рамка из GEO_GITHUB_STRATEGY.md §3).

## Что это даёт (честно) и что замерять

Canonical — это не классический бэклинк, а сигнал атрибуции; ссылки в теле поста на
dev.to — nofollow. Ценность канала в другом: индексируемая поверхность на DR90-домене,
который активно цитируют AI-движки; брендовые упоминания (главный коррелят AI-видимости);
шанс dev.to-версии ранжироваться по long-tail запросам, где наш домен пока слаб.
Эффект накопительный — работает регулярность, а не разовый вброс.

Замер раз в 2 недели: referral с dev.to в аналитике сайта; позиции dev.to-постов по своим
запросам; ручные вопросы к ChatGPT/Perplexity («cheapest claude api», «openrouter
alternative for claude») — попали ли наши посты в цитаты.

## Журнал публикаций

| Дата | Slug | dev.to URL |
|---|---|---|
| 2026-07-28 | cheapest-claude-api | https://dev.to/api_token_46fac5c7112fe23/cheapest-claude-api-up-to-80-discount-2ne8 |
| 2026-07-28 | apitoken-vs-openrouter | https://dev.to/api_token_46fac5c7112fe23/apitokensale-vs-openrouter-for-claude-3gmj |
| 2026-07-28 | claude-code-without-subscription | https://dev.to/api_token_46fac5c7112fe23/use-claude-code-without-a-subscription-1i6h |

Очередь кандидатов: `claude-api-key-for-cursor`, `apitoken-vs-anthropic-direct`,
`claude-api-prompt-caching`, `claude-api-litellm`, `claude-api-aider`,
`claude-code-api-key`, `claude-api-pricing-explained`.
