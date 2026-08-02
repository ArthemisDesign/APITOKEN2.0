# Live-калибровка Gemini-пула

`tools/gemini_calibration/run_live.py` проводит ограниченный нагрузочный прогон native Gemini через
реальные платные Code Assist подписки. Скрипт не выводит ёмкость из размера prompt и не доверяет
клиентскому списанию: единственный источник стоимости — immutable Google turn events в backend
`/gemini-subs`, а единственный источник движения окна — точные provider quota snapshots.

## Что покрывает прогон

Для всех моделей из backend `conversion_models` runner проверяет native non-stream и SSE, все
опубликованные thinking levels, fresh text, повторяемый text-cache payload, audio и повторяемый
audio payload, function-declaration tool prompt и Google Search. Для моделей с tiered pricing
отдельный payload должен пересечь опубликованный long-context threshold по фактическому
`countTokens`; для image-модели отдельно выполняются 1K, 2K и 4K. Бесплатный
`countTokens` является preflight каждого платного запроса. HTTP 400/403/404 и успешный turn без
ожидаемого token class записываются в `unavailable_capabilities`, а не считаются нулевым расходом.

Backend estimator остаётся workload-dependent: он оценивает API-долларовый эквивалент фактически
наблюдаемой смеси. Plan является частью identity; 5-часовое и недельное окна независимы. Bounds
учитывают decimal resolution обоих provider snapshots. Неразличимое движение не превращается в
`$0`, а finite high отсутствует, если движение не превосходит измерительную неопределённость.

## Страховки

- Без `--execute` runner печатает детерминированный dry-run plan и не открывает backend/API.
- `--budget-usd` парсится в integer nanoUSD и не может превышать суммарные `$40` за запуск.
- До первого turn обязательны `calibration_authority_available=true`, пустая очередь, нулевой
  `dropped_events`, healthy persistence и известный paid plan каждого выбранного профиля.
- Admin-only `x-apitoken-calibration-profile` содержит полный opaque Gemini profile ID. Backend
  выбирает ровно его, не spill/rebind-ится и не обходит auth death, cooling или provider zero.
  Заголовок никогда не передаётся Google.
- Перед dispatch `countTokens` и официальный effective-dated rate card строят worst-case bound.
  Code Assist добавляет provider-owned instructions, которых нет в результате `countTokens`; live
  cache-leg уже доказал такой hidden input. Поэтому каждый generation leg резервирует полный
  официальный input-context limit модели. Это единственный hard ceiling и для обычного скрытого
  prompt, и для отсутствующего в `countTokens` отдельного `toolUsePromptTokenCount`, а не
  предположение о размере клиентского payload или JSON declaration.
  Search, тарифицируемый один раз за grounded prompt, резервирует один официальный SKU. Для Gemini 3
  Google тарифицирует каждую внутреннюю query, но не публикует hard fanout ceiling: такой Search
  runner записывает как unavailable и не отправляет платный запрос. Image использует принудительно
  ограниченные 1K/2K/4K SKU (1 120/1 680/2 520 токенов) плюс requested text-output ceiling.
- Платный запрос после transport ambiguity не повторяется. GET и `countTokens` можно безопасно
  повторить, но generation имеет ровно одну попытку.
- Для каждого платного turn runner заранее создаёт canonical UUIDv4 и передаёт его admin-only
  заголовком `x-apitoken-calibration-request-id` вместе с exact profile target. В immutable backend
  evidence должен появиться именно этот request id с ожидаемыми profile/model и полным token/
  API-cost vector. Rebind, нарушенная сумма cost legs, pending FIFO или actual выше preflight bound
  останавливают прогон fail closed; параллельный customer traffic не участвует в атрибуции.
- Cache payload содержит уникальный `run_id`; write/read пары байт-в-байт одинаковы, но другой запуск
  не может принять старую cache warmth за свою.
- Между turn выдерживается 16 секунд, чтобы backend quota poll успел связать durable spend с новой
  fraction. Quota-only движение, повторившееся без spend, уходит в `unattributed_fraction_units`.
- API/control keys загружаются только локально или внутри production shell и не входят в report.

## Production-команда

Запуск разрешён только после зелёного production deploy exact runtime SHA:

```bash
python3 tools/gemini_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --report /tmp/gemini-calibration-report.json
```

Production SSH path читает `/gemini-subs` через стабильную Gemini-плоскость `127.0.0.1:8794` и
отправляет generation туда же с remote-only forwarding-admin key. Секрет не возвращается через SSH.

## Офлайн-проверка

```bash
python3 -m unittest tools.gemini_calibration.test_run_live
```

Тесты покрывают dry-run, жёсткий `$40` guard, authority/FIFO baseline, exact attribution на фоне
нескольких новых событий, cost-vector integrity, long-context/search/image bounds, полную матрицу
capabilities и byte-identical cache/audio replay.

## Результат

Report `gemini-live-calibration/v1` сохраняет точный total и расход по opaque profile в nanoUSD,
полный token/API-cost vector каждого turn, 5h/7d fraction delta, недоступные capabilities, profile
quota walls, before/after identity каждого окна, финальный backend snapshot и
`model_profitability`, отсортированный по API nanoUSD на 1% соответствующего 5h/7d окна отдельно
для каждого paid plan, model и token class. Смена reset identity никогда не считается model-specific
fraction delta.

Рейтинг допустимо использовать для коммерческого выбора только у строк с положительным различимым
quota delta, если между before/after на том же профиле не появилось чужого immutable turn. Runner
помечает такой interval `profitability_eligible=false` и исключает его из рейтинга. Отсутствующая
строка означает недостаточную provider resolution/изоляцию, а не нулевую ценность модели.
