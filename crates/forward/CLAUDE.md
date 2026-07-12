# crates/forward — CLAUDE.md

**Роль:** прозрачный форвардинг `/v1/*` на api.anthropic.com (Шаг B) + поллер лимитов.
Сердце «неотличимости от оригинального API».

**Владелец-ветка:** `comp/forward`.

**Границы (жёстко):**
- Зависит от `pool`, `registry`, `metering`, `axum`, `reqwest`, `serde_json`, `futures-util`, `bytes`.
- НЕ читает env и НЕ содержит CLI/управляющих роутов (`/health`, `/pool`, `/balance`) — это `server`.
- Конфиг получает готовым: [`ProxyConfig`] наполняет `server::config`; биллинг — `Option<Arc<Billing>>` в `AppState`.

**Биллинг (tee-метеринг, `meter.rs`):** авторизация по балансу (`authorize`): метерный ключ из
`api_keys` → проверка баланса (≤0 → 402), иначе Admin (env-ключ/localhost) без тарификации. На
УСПЕШНЫЙ ответ метерного ключа тело оборачивается в `TeeMeter` — клиент получает байты без
задержки, а на завершении стрима парсим usage (`metering::usage_from_sse`/`_response_json`) →
`cost_with_multiplier` → `Billing::deduct`. 4xx/ошибки/ротация НЕ тарифицируются.

**Что внутри:** `ProxyConfig`, `AppState`, `Clients` (кэш http-клиентов по прокси),
`limits_from_headers`/`Limits` (unified-ratelimit из ответа), `poll_sub` (активный опрос idle),
`detect_plan` (тариф из /api/oauth/profile), `forward` (axum-хендлер), `authed`.

**Sticky-роутинг (cache-affinity):** `session_key(headers, body)` = хэш(клиентский ключ + `messages[0]`)
— стабильный id диалога (первое сообщение не меняется от хода к ходу). Считается ДО инжекта identity
(по исходному контенту клиента). Первая попытка цикла = `pool.pick_sticky(session)` → «домашняя»
подписка сессии (весь диалог на одном аккаунте: prompt-cache hit + паттерн одного юзера). Дом
недоступен/уже пробован → `pick_sticky` вернёт None, падаем на load-based `pool.pick`. Ретраи (после
429) всегда load-based (дом уже в `tried`). Не messages-запрос → `session=None`, сразу load-based.

**Ротация/лимиты (устойчивость пула):**
- **Пассивный сбор:** на КАЖДОМ ответе апстрима вытаскиваем unified-ratelimit (`limits_from_headers`)
  → `pool.set_util`. Так util/reset всегда свежи из боевого трафика; активный `poll_sub` (server)
  добивает лишь простаивающие подписки (обновлённый `polled_ts` сам это гейтит). Экономит квоту.
- **429 → правильное окно:** `cool_secs_429` = `Retry-After` (авторитетно) → окно-виновник
  (`util7d≥0.98` → до `reset7d`, иначе до `reset5h`) → дефолт. Не студим на 5h, если выбит 7d.
- Битый прокси → `mark_cooling(10)` (cooling + −1 in-flight). Каждый `mark_used` в цикле ротации
  парен с `mark_ok`/`mark_cooling`.

**Инварианты прозрачности (критично — не ломать):**
1. Ответ апстрима отдаётся клиенту **байт-в-байт** (включая SSE-стрим). Не буферизировать,
   не переписывать тело.
2. Под капотом: инжект Claude Code identity ПЕРВЫМ system-блоком + `anthropic-beta: oauth-…` +
   `Bearer` подписки. Клиентский `system` сохраняется вторым блоком. Без identity Anthropic не
   пускает OAuth-токены подписок — но клиент об этом знать не должен.
3. **Ротация только ДО начала стрима:** решение по статусу (429/401/403/5xx → cooling + следующая
   подписка) принимается до отдачи тела. Как только начали стримить — не переключаемся.
4. Клиентские ошибки запроса (400/404/422 …) пробрасываются как есть, БЕЗ ротации.
5. Заголовки авторизации клиента (`x-api-key`/`authorization`) НЕ уходят апстриму — заменяются
   на Bearer подписки. Токены не логировать.

**Тюнинг под живой Anthropic** (identity/beta/UA/version) — через поля `ProxyConfig`, которые
`server` берёт из env. Значения по умолчанию — в `config.rs`.

**Проверка:** `cargo build -p forward`; полный smoke — через бинарь против мок-апстрима.
