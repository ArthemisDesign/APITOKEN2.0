# CRM_PARSING_SPEC.md — контракт парсинга: конверт контакта + AI-инструкция классификатора

Это ЕДИНСТВЕННЫЙ контракт между любыми парсинг-системами (нашими и партнёрскими) и AI-CRM.
Парсеры могут быть сколь угодно разными по природе (Telegram-группы, Gmail-переписки, LinkedIn,
формы, чужие базы) — на выходе ВСЕГДА конверт из этого файла. Устройство самой CRM — `CRM_AI.md`.

## 1. Конверт `apitoken.crm/contact@v1`

Батч = JSON-массив конвертов или JSONL (конверт на строку). Отправка:

```
POST https://crm.panel.apitoken.sale/v1/ingest/contacts
x-crm-ingest-key: <CRM_INGEST_KEY>
content-type: application/json

{ "run": {"parser": "tg-groups-scraper", "run_id": "2026-07-20-uk-saas-01"},
  "contacts": [ <конверт>, … ] }
```

Один конверт:

```json
{
  "envelope": "apitoken.crm/contact@v1",
  "source": {
    "parser": "tg-groups-scraper",
    "run_id": "2026-07-20-uk-saas-01",
    "parsed_at": "2026-07-20T12:34:56Z",
    "origin": { "kind": "telegram_group", "ref": "t.me/…", "query": "как искали" }
  },
  "identity": {
    "name": "Ivan Petrov",
    "channels": [
      { "type": "telegram", "value": "@ivan_petrov" },
      { "type": "gmail",    "value": "ivan@gmail.com" }
    ]
  },
  "raw": { "bio": "…", "messages_sample": ["…"], "profile": { "любые": "сырые данные" } },
  "ai": {
    "model": "claude-sonnet-5",
    "classified_at": "2026-07-20T12:35:10Z",
    "summary": "Кто этот человек: 2–4 предложения глубокого портрета.",
    "attributes": {
      "role":            { "value": "cto",            "confidence": 0.9,  "evidence": "в био: 'CTO @ …'" },
      "geo_country":     { "value": "UK",             "confidence": 0.7,  "evidence": "часовой пояс + группа UK Devs" },
      "pain_points":     { "value": ["api cost"],     "confidence": 0.8,  "evidence": "жаловался на цены в сообщении от 12.07" },
      "buying_intent":   { "value": 0.65,             "confidence": 0.6,  "evidence": "спрашивал про альтернативы" }
    },
    "hypotheses": ["возможно ищет white-label решение — уточнить при первом касании"]
  }
}
```

**Жёсткий минимум** (без него контакт отклоняется): поле `envelope` с точным тегом и хотя бы
один канал в `identity.channels` (`type` + непустой `value`). ВСЁ остальное — открытое:
CRM принимает любые ключи признаков и сама расширяет реестр. Если твой формат не влез в
конверт — CRM попробует AI-адаптером смапить его сама и запишет отклонение в drift-лог,
но это аварийный путь, а не норма.

Типы каналов: `telegram | gmail | email | phone | linkedin | x | github | discord | site | other`.
`gmail` — отдельно от `email` намеренно (принадлежность к экосистеме — сам по себе признак).

## 2. Правила признаков (attributes)

- Ключ — `snake_case`, англ.; значение — скаляр (строка/число/бул) или массив строк.
- На КАЖДЫЙ признак: `confidence` 0..1 и `evidence` — короткое основание (цитата, факт,
  откуда вывод). Признак без evidence — мусор; лучше не ставить.
- Не выдумывать: уверенность ниже 0.3 → признак не ставим, гипотезу — в `hypotheses`.
- **Ключей должно быть МНОГО.** Цель — глубокий портрет: норма 15–40 признаков на контакт,
  а не 3–5. Если видишь особенность, которой нет в базовой таксономии — ПРИДУМАЙ новый ключ
  по конвенции (напр. `tg_activity_level`, `crypto_native`, `hiring_now`) — CRM подхватит
  его в реестр автоматически.

### Базовая таксономия (ориентир, НЕ ограничение)

| Группа | Примеры ключей |
|---|---|
| Идентичность | `full_name_confident`, `gender_guess`, `age_range`, `languages` |
| География | `geo_country`, `geo_city`, `timezone_guess`, `relocation_signals` |
| Роль/карьера | `role`, `seniority`, `occupation`, `is_decision_maker`, `career_stage` |
| Компания | `company_name`, `company_size_guess`, `industry`, `b2b_or_b2c`, `company_stage` |
| Технологии | `tech_stack`, `uses_ai_tools`, `ai_provider_current`, `dev_or_nondev` |
| Деньги | `budget_signals`, `price_sensitivity`, `pays_for_saas`, `crypto_native` |
| Намерение | `buying_intent`, `pain_points`, `looking_for`, `objections_heard` |
| Активность | `tg_activity_level`, `posting_frequency`, `community_roles`, `last_seen_active` |
| Влияние | `audience_size`, `is_influencer`, `network_quality`, `referral_potential` |
| Стиль | `communication_style`, `preferred_language`, `formality`, `responds_to_cold` |
| Интересы | `interests`, `content_topics`, `communities` |
| Риски | `risk_flags` (спамер/бот/токсичность/конкурент), `bot_likelihood` |

## 3. СИСТЕМНАЯ ИНСТРУКЦИЯ ДЛЯ AI-КЛАССИФИКАТОРА ПАРСЕРА

Вставляется как system-промпт в нейронку парсера (модель — через наш движок
`https://api.apitoken.sale/v1/messages`, ключ «CRM & Parsing»). Текст готов к использованию:

```
Ты — классификатор потенциальных клиентов в конвейере парсинга apitoken.sale (продажа доступа
к Claude API с большой скидкой; ЦА — разработчики, фаундеры, агентства, AI-энтузиасты, ресейлеры).

Тебе дают СЫРЫЕ данные об одном человеке (профиль, био, сообщения, метаданные — состав любой).
Твоя задача — построить максимально ГЛУБОКИЙ портрет: кто этот человек, чем живёт, на что у
него боль и деньги, как и о чём с ним говорить.

Верни СТРОГО один JSON-объект без пояснений вокруг, формата:
{
  "summary": "<2–4 предложения: кто это, чем занимается, почему (не)интересен нам>",
  "attributes": { "<snake_case_ключ>": {"value": <скаляр|массив строк>,
                   "confidence": <0..1>, "evidence": "<основание: цитата/факт>"}, … },
  "hypotheses": ["<осторожные догадки, недотянувшие до признака>", …]
}

Правила:
1. Признаков должно быть МНОГО — выжми всё: идентичность, география, языки, роль, seniority,
   индустрия, компания и её размер, стек и AI-инструменты, деньги и ценочувствительность,
   намерение купить, боли, активность, влияние/аудитория, стиль общения, интересы, риски
   (бот/спамер/конкурент). Норма — 15–40 признаков.
2. КАЖДЫЙ признак — с confidence и evidence. Нет основания — не ставь признак; догадку
   положи в hypotheses. Ниже 0.3 уверенности — только hypotheses.
3. Известные ключи используй из базовой таксономии (role, geo_country, pain_points,
   buying_intent, tech_stack, …). Видишь важную особенность без готового ключа — ПРИДУМАЙ
   новый snake_case-ключ, не теряй информацию.
4. Значения нормализуй: страны — ISO-код (UK, DE), языки — ISO (en, ru), роли — нижний
   регистр ("cto", "founder", "indie_hacker"), числовые сигналы (buying_intent,
   bot_likelihood) — числом 0..1.
5. Ничего не выдумывай про личные данные: имя/контакты бери только из входных данных.
6. Пиши evidence на языке источника, summary — на русском.
```

## 4. Чек-лист автора нового парсера

1. Парсишь источник → собираешь сырьё (`raw`) и каналы связи.
2. Гонишь сырьё через классификатор (промпт выше, инструкция §3) → `ai`-блок.
3. Складываешь конверты §1 (валидируй минимум: envelope-тег + канал).
4. Шлёшь батчами ≤500 контактов в `/v1/ingest/contacts` с `run_id` (идемпотентно: повтор
   батча не плодит дубликаты — контакты сливаются по каналам).
5. Смотришь ответ: `accepted / repaired / rejected` + drift. Если drift не пуст — твой формат
   отклонился от спеки: почини парсер или предложи расширение спеки (PR в этот файл).
```
