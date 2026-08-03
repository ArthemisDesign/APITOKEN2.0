# GLM (Zhipu AI / Z.ai) — provider capability manifest

Статус интеграции: **research-скелет, default-off backend preview, ни одного live-доказательства**.
Дата ревью источников — **2026-08-03**.

Документ создан по `docs/engine/PROVIDER_ONBOARDING.md` §3.3 и является capability manifest
плоскости GLM. Каждое утверждение помечено по иерархии evidence из §3.1: `official`, `live`,
`oss-hypothesis`, `decision`, `unknown`, `not-applicable`. Механическая карта правок —
`docs/engine/PROVIDER_WIRING_CHECKLIST.md`; образец реализации — `docs/engine/KIMI_PROVIDER.md`
(KIMI — ближайший аналог: китайский subscription-провайдер с Anthropic-совместимым транспортом).

## 0. Область и намеренные ограничения

Плоскость GLM строится **только как backend**: engine runtime, метеринг, калибровка, Auth Bot
и внутренний live-runner. Провайдер **не публикуется** в публичный каталог, `/v1/models` роутера,
commerce/OpenKeys прайсинг, сайт и клиентскую документацию — решение зеркалит KIMI §0:
интеграция нужна для расширения пула ёмкости и калибровочных прогонов, публикация — отдельное
решение владельца.

- `decision` Провайдер остаётся за выключенным switch'ем; ни одна публичная поверхность не
  получает строку GLM. Чеклист «новый провайдер» из `docs/CHANGE_CHECKLISTS.md` применяется
  частично; публикационные пункты помечаются неприменимыми с этой причиной.
- `decision` Модель бизнеса реселлеров (аудит запроса): реселлер покупает GLM Coding Plan,
  получает plan-bound API key из консоли и продаёт Anthropic-/OpenAI-совместимый доступ поверх
  prompt-квоты подписки. Это ровно модель нашего Claude-пула; GLM добавляется как ещё одна
  липкая масштабируемая subscription-плоскость рядом с Claude/Codex/Gemini/KIMI.

Пока публикации нет, GA-критерий §1 `PROVIDER_ONBOARDING.md` **не заявляется**. Терминальное
состояние — verified **preview**: всё, что не требует живой подписки, доказано на mock-гейтах;
живые гейты перечислены в §6 и ждут собственную подписку.

## 1. Product / plan

`official` Zhipu AI (Z.ai / open.bigmodel.cn) разводит независимые системы доступа:

| Плоскость | Назначение | Base URL | Биллинг |
|---|---|---|---|
| Z.ai Open Platform | pay-per-token developer API | `https://api.z.ai/api/paas/v4` (int.), `https://open.bigmodel.cn/api/paas/v4` (CN) | по токенам |
| GLM Coding Plan (подписка) | subscription coding plan | `https://api.z.ai/api/coding/paas/v4` (int.), `https://open.bigmodel.cn/api/coding/paas/v4` (CN) | из квоты подписки |
| Z.ai web/app chat | потребительский чат | — | подписка, **API не даёт** |

`official` Coding Plan отдаётся и через Anthropic-совместимый endpoint
(`https://api.z.ai/api/anthropic`), что подтверждено интеграциями Claude Code и issue
`anomalyco/opencode#2431` («exposed only through the Anthropic-compatible endpoint»).

`decision` Наш провайдер — **только GLM Coding Plan**. Open Platform используется исключительно
как authority официального прайсинга (replacement cost), но не как источник ёмкости.

### 1.1 Тарифные планы

`unknown` Точный набор планов, цены и квоты не зафиксированы нормативной страницей на момент
скелета. По агрегаторам — лестница от ~$3/мес с prompt-окнами (порядка 120 промптов / 5ч на
старте, рост по тирам). Требуется deep research provider-owned страниц; цена подписки в расчётах
не участвует (см. KIMI §1.1), но **окна и лимиты квоты — load-bearing для калибровки**.

## 2. Credential

`oss-hypothesis` В отличие от KIMI (OAuth device flow), GLM Coding Plan работает по
**статическому API-ключу**, выданному в консоли подписки (`https://z.ai/manage-apikey` /
bigmodel.cn console). Refresh-семьи нет; ротация = перевыпуск ключа в консоли.

`decision` Это упрощает credential-контур до уровня «seal/validate/publish» без single-flight
refresh, но переносит вес на валидацию ключа при onboarding: identity/quota probe обязан
доказать принадлежность ключа именно Coding Plan, а не Open Platform (иначе продавец слил бы
pay-per-token ключ с реальным балансом — деньги на счёте продавца, а не квота).

`unknown` Есть ли у Coding Plan machine-readable identity endpoint (`/me`-эквивалент) и
quota endpoint (`/usages`-эквивалент). Исследуется; до доказательства — fail closed.

## 3. Model admission

`unknown` Точный список подписочных моделей и их соответствие официальному rate card.
По публичным источникам на плане доступны GLM-4.7/GLM-4.6/GLM-4.5 семейства; маппинг на
официальные модели Open Platform (`glm-4.7`, `glm-4.6`, `glm-4.5`, `glm-4.5-air`, …) и
контекстные окна фиксируется deep research'ем.

| Подписочная модель | Официальная модель (rate card) | Контекст | Тир | Non-stream | Incremental stream | Usage | Quota | Решение |
|---|---|---|---|---|---|---|---|---|
| _pending research_ | | | | `unknown` | `unknown` | `unknown` | `unknown` | preview, за switch |

## 4. Wire

| Операция | URL | Заголовки | Тело | Framing | Usage | Ошибки |
|---|---|---|---|---|---|---|
| Generation (Anthropic) | `POST https://api.z.ai/api/anthropic/v1/messages` | `unknown` (Bearer vs x-api-key) | Anthropic Messages | SSE | `unknown` | `unknown` |
| Generation (OpenAI) | `POST https://api.z.ai/api/coding/paas/v4/chat/completions` | `Authorization: Bearer` | OpenAI Chat | SSE | `unknown` | `unknown` |
| Identity / Quota | `unknown` | | | | | |

`decision` Как и у KIMI, Anthropic-совместимый транспорт позволяет переиспользовать нативный
Anthropic-путь движка без трансляционного слоя масштаба `gemini/`.

## 5. Money / quota

`unknown` Официальный rate card Open Platform (GLM-4.7/4.6/4.5, cache hit/miss, output) —
берётся только с provider-owned страницы с датой ревью (агрегаторы противоречат друг другу).

`unknown` Форма нативной квоты Coding Plan: prompt-окна (количество промптов на окно) против
токенных окон; точные длительности; поведение reset; наличие machine-readable quota endpoint.
Это определяет ledger-модель калибровки (§10.1 onboarding): prompt-окно с известным `limit` —
Claude-подобная форма с точным `native_limit_units` (как у KIMI §5.3).

## 6. Что остаётся недоказанным

Всё, кроме решений архитектуры (`decision`). Каждый `unknown` fail closed и снимается
контролируемым research'ем (нормативные страницы) или live-прогоном на собственной подписке.

## 7. Состояние доставки

| Этап | Артефакт | Состояние |
|---|---|---|
| research / capability manifest | этот файл | скелет, research в работе |

Очередь и прогресс — `research/GLM_PLANE_PROGRESS.md`.

## 8. Источники

- `https://github.com/anomalyco/opencode/issues/2431` (coding plan — Anthropic-compatible endpoint)
- `https://docs.cline.bot/provider-config/zai` (subscription coding plans, prompt-quota)
- Deep research provider-owned страниц — в работе, будет дописано с датами ревью.
