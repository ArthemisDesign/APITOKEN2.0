# crates/metering — CLAUDE.md

**Роль:** точный подсчёт токенов → USD-эквивалент. Гарантия: **ни один токен не мимо счёта.**

**Границы (жёстко):**
- Чистая математика/парсинг. Зависимость только `serde_json`. НИКАКОЙ сети/БД/env/HTTP.
- Все токеновые корзины Anthropic учитываются РАЗДЕЛЬНО (у них разная цена): input, output,
  cache_read, cache_write_5m, cache_write_1h, web_search. Добавляешь корзину — добавь и в `Usage`,
  `cost_nanodollars`, `total_tokens`, и тесты.
- Считаем в ЦЕЛЫХ нанодолларах (1 USD = 1e9 нано; $/Mtoken × 1000 = нано/токен — целое).
  Никаких f64 в подсчёте денег.
- Gemini catalog тоже живёт только здесь: paid-tier effective-dated ставки, uncached/audio/cached
  input, candidate+thinking output, диагностический tool-prompt subset, long-context и Search.
  Отсутствующий `toolUsePromptTokenCount` не вычитается из authoritative `promptTokenCount` и не
  придумывается: subset не тарифицируется второй раз. Gemini 2.5 Search считается per grounded
  prompt, Gemini 3 — per query. Новую модель/ценовую эпоху добавлять только с официальной ссылкой и
  exact-rate тестом; отдельно тарифицируемый server tool нельзя пропустить бесплатно.
- Codex catalog и ChatGPT Fast credit multiplier тоже живут только здесь. Fast — tier существующей
  модели, не отдельный model id: GPT-5.6/5.5 = 2.5x, GPT-5.4 = 2x. Изменять только по опубликованной
  OpenAI таблице с exact-multiplier тестом.
- Versioned model/tariff identity — только capability, не product access. Exact canonical map,
  alias generation, immutable schedule ID/epoch и typed reserve modifiers живут здесь; доступ всё
  равно требует отдельного product catalog и account policy. Unknown/historical ID не превращается
  в выдуманную canonical identity, а legacy conservative pricing остаётся отдельным контрактом.

**Инварианты (проверять тестами):**
- 1M токенов любой корзины = точная официальная ставка (тест `prices_exact_per_million`).
- Стрим: input/cache из `message_start`, output из ПОСЛЕДНЕГО `message_delta` (кумулятивный).
- Gemini SSE также использует последний полный кумулятивный `usageMetadata`; split/malformed frames
  не паникуют и не затирают последний валидный snapshot.
- Алиас и конкретные варианты одной Codex-модели обязаны возвращать одинаковый Fast-множитель.
- `gpt-5.6` и `gpt-5.6-sol` обязаны иметь одну canonical/tariff identity; новая price epoch меняет
  schedule ID, но не alias generation. Codex capability также фиксирует audited max-output limit,
  чтобы dormant snapshot builder мог отвергнуть drift runtime-конфига без второй reserve-формулы.
  Long-context и Fast/geo modifiers фиксируются отдельно.
- Битый ввод → `Usage::default()` (нули), НИКОГДА не паникует.
- i128 — переполнения исключены даже на млрд токенов.

**Как использует `forward`:** тий (tee) ответа апстрима → на завершении `usage_from_sse` (стрим)
или `usage_from_response_json` (не-стрим) → `cost_with_multiplier` → списание с баланса ключа.
Метерить ТОЛЬКО успешный ответ (429/ротация не тарифицируются).

**Проверка:** `cargo test -p metering` (должны пройти ВСЕ — это про деньги).
