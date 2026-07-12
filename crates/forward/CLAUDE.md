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

**Cache-first роутинг сессии:** `session_key(headers, body)` = хэш кэшируемого префикса (клиентский
ключ + `system` + `messages[0]`) — стабильный id диалога (большой статический префикс живёт в
prompt-cache и не меняется от хода к ходу). Считается ДО инжекта identity (по исходному контенту).
Первая попытка цикла = `pool.route(session)` → пин на тёплый дом / capacity-weighted placement /
спилл при кратком давлении (вся логика в pool). Ретраи после 429/5xx → load-based `pool.pick` (дом
уже в `tried`, привязка сессии цела). Не messages-запрос → `session=None`, сразу load-based.
In-flight держится всю жизнь стрима: успех → `mark_healthy`, `end_stream` из tee-метеринга (`meter.rs`)
снимает слот на завершении/обрыве; 4xx → `mark_ok`.

**Ротация/лимиты (устойчивость пула):**
- **Пассивный сбор:** на КАЖДОМ ответе апстрима вытаскиваем unified-ratelimit (`limits_from_headers`)
  → `pool.set_util`. Так util/reset всегда свежи из боевого трафика; активный `poll_sub` (server)
  добивает лишь простаивающие подписки (обновлённый `polled_ts` сам это гейтит). Экономит квоту.
- **429 → правильное окно:** `cool_secs_429` = `Retry-After` (авторитетно) → окно-виновник
  (`util7d≥0.98` → до `reset7d`, иначе до `reset5h`) → дефолт. Не студим на 5h, если выбит 7d.
- Битый прокси → `mark_cooling(10)` (cooling + −1 in-flight). Каждый `mark_used` в цикле ротации
  парен с `mark_ok`/`mark_cooling`.

**Антифингерпринт флота (`persona_ua`):** флот из 100 байт-в-байт одинаковых UA — сам по себе
отпечаток. `persona_ua(cfg, email)` даёт **стабильный во времени** для подписки, но **различный между
подписками** UA: пул задан списком (`user_agents` len>1) → пиним по hash(email); иначе варьируем
patch-версию базового UA на `ua_spread`. Клиентский `user-agent` НЕ пробрасываем (в `skip_req_header`)
— отпечаток наш. Тот же UA идёт и в `poll_sub`/`detect_plan` (здоровье персоны = тот же отпечаток,
что и бой). Identity/beta/anthropic-version НЕ варьируем — они корректностные (нет ground-truth на
правдоподобные альтернативы). Env: `CLAUDE_API_UA` (один или список), `CLAUDE_API_UA_SPREAD`.

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
