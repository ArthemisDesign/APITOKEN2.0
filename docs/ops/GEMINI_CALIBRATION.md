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
После уже оплаченного generation runner также проверяет само native-тело ответа: публичный
`modelVersion`, видимый non-thought text (либо обязательный `functionCall`/`inlineData` для
соответствующего control), `finishReason`, terminal `usageMetadata` и точное совпадение response
token vector с immutable event. SSE считается incremental только при нескольких candidate frames;
один буферизованный кадр не проходит gate.

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
- `429`/`503` останавливает только целевой профиль, а не оставшуюся матрицу здоровых профилей,
  исключительно когда stable Gemini plane вернул авторитетный
  `x-apitoken-execution-state: not_started`. `RetryInfo` и sanitized body сами по себе не являются
  доказательством. Такой partial report можно продолжить через
  `--resume-report`: runner сохраняет тот же `run_id`, общий budget, cache lineage и exact spend,
  пропускает уже завершённые/доказанно недоступные legs и не добавляет новые профили или модели.
  Generic 5xx без not-started proof, SSH/HTTP transport ambiguity и любой иной непроверенный failure помечаются
  `resume_safe=false`; такой платный leg повторять запрещено.
- Для каждого платного turn runner заранее создаёт canonical UUIDv4 и передаёт его admin-only
  заголовком `x-apitoken-calibration-request-id` вместе с exact profile target. В immutable backend
  evidence должен появиться именно этот request id с ожидаемыми profile/model и полным token/
  API-cost vector. Rebind, нарушенная сумма cost legs, pending FIFO или actual выше preflight bound
  останавливают прогон fail closed; параллельный customer traffic не участвует в атрибуции.
- Успешный HTTP-код сам по себе не является evidence. Ответ декодируется в памяти без сохранения
  сгенерированного текста в report. Private/отсутствующий `modelVersion`, thoughts-only output,
  malformed JSON/SSE, отсутствие terminal usage/finish, single-frame SSE, не вызванный forced tool
  или расхождение response usage с immutable turn являются terminal coverage failure. Runner уже
  учитывает подтверждённый расход, не повторяет запрос и прекращает оставшуюся платную матрицу.
  Для явных `low`/`medium`/`high` thinking levels immutable usage обязан содержать ненулевые
  thinking output tokens. `minimal` допускает нулевой счётчик: этот уровень использует dynamic
  thinking и успешный ответ с полным public identity/output/terminal usage proof не становится
  coverage miss только из-за отсутствия отдельного thinking token class.
  HTTP 400/403/404 required capability ведёт к тому же fail-closed исходу. Единственное
  неблокирующее `unavailable_capabilities` — заранее пропущенный без generation Search с
  документированно неограниченным per-query fanout (`blocking=false`,
  `skipped_before_dispatch=true`).
- Cache payload содержит уникальный `run_id`; write/read пары байт-в-байт одинаковы, но другой запуск
  не может принять старую cache warmth за свою.
- После turn runner выдерживает минимум 16 секунд и затем опрашивает backend до quota snapshot с
  `quota_updated_at >= immutable completed_at`. До такого post-turn snapshot fraction delta не
  считается model/token-class evidence и не попадает в profitability. Quota-only движение,
  повторившееся без spend, уходит в `unattributed_fraction_units`.
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

После явного временного provider stop тот же прогон продолжается без повторного расхода:

```bash
python3 tools/gemini_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --resume-report /tmp/gemini-calibration-report.json \
  --report /tmp/gemini-calibration-report.json
```

`--budget-usd` при resume — исходный aggregate cap, а не добавочный бюджет; значение обязано точно
совпадать с checkpoint. `complete=false`, `resume_safe=true`,
`resume_proof=x-apitoken-execution-state:not_started` и `pending_legs` явно показывают, что
ещё осталось после cooling. `resume_safe=false` означает терминальный ручной разбор без повторения
платного запроса. Узкое versioned-исключение существует только для уже завершённого `minimal` turn,
который старый runner ошибочно остановил исключительно из-за нулевого thinking token count: новый
runner сохраняет exact spend и доказанный record, снимает только этот obsolete coverage miss и
продолжает pending legs без replay. Для этого обязаны точно совпасть public `modelVersion`, реальный
visible output, terminal finish/usage, response/event usage parity, non-stream identity и
единственный blocking miss; любое отличие оставляет report терминальным. Смена paid plan или
effective tariff schedule между попытками также прекращает resume: evidence разных денежных
identity в один прогон не объединяется.

Production SSH path читает `/gemini-subs` через стабильную Gemini-плоскость `127.0.0.1:8794` и
отправляет generation туда же с remote-only forwarding-admin key. Секрет не возвращается через SSH.
Для controlled gate dormant-модели оба чтения можно направить в заранее поднятый непубличный
exact-SHA canary на том же host через `--production-ssh-target <user@host>` и
`--production-api-port <loopback-port>`. Target и порт валидируются до SSH; default остаётся
`apitokensale:8794`. Canary обязан использовать production PostgreSQL authority, billing и
immutable calibration evidence: изолированный процесс без них не является публикационным
доказательством.

## Офлайн-проверка

```bash
python3 -m unittest tools.gemini_calibration.test_run_live
```

Тесты покрывают dry-run, жёсткий `$40` guard, authority/FIFO baseline, exact attribution на фоне
нескольких новых событий, cost-vector integrity, long-context/search/image bounds, полную матрицу
capabilities, byte-identical cache/audio replay, forced tool call, public model identity, реальный
non-thought output, terminal response/event usage parity, incremental SSE и fail-closed resume с
точным восстановлением spend, включая non-replay reclassification доказанного `minimal` turn с
нулевым thinking token count и fail-closed отклонение подменённого evidence.

## Результат

Report `gemini-live-calibration/v2` сохраняет точный total и расход по opaque profile в nanoUSD,
полный token/API-cost vector каждого turn, 5h/7d fraction delta, недоступные capabilities, profile
quota walls, before/after identity каждого окна, финальный backend snapshot и
`model_profitability`, отсортированный по API nanoUSD на 1% соответствующего 5h/7d окна отдельно
для каждого paid plan, model и token class. Смена reset identity никогда не считается model-specific
fraction delta. Каждый успешный record дополнительно содержит только sanitized `response_evidence`
(счётчики frames/output/control, публичный model id и booleans terminal/incremental/usage parity),
но не текст ответа. `blocking_unavailable_capabilities` делает terminal publication miss явным;
report может иметь `complete=true` только без таких miss и без незавершённых legs.

Рейтинг допустимо использовать для коммерческого выбора только у строк с положительным различимым
quota delta, если между before/after на том же профиле не появилось чужого immutable turn. Runner
помечает такой interval `profitability_eligible=false` и исключает его из рейтинга. Отсутствующая
строка означает недостаточную provider resolution/изоляцию, а не нулевую ценность модели.
