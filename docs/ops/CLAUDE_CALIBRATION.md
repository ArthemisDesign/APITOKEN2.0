# Live-калибровка Claude-пула

`tools/claude_calibration/run_live.py` — операторский нагрузочный прогон реального Claude-пула.
Он проверяет все опубликованные Claude model ID и для каждого реально поддерживаемого tier выполняет
полную матрицу fresh input/output, cache write/read 5m, cache write/read 1h и Web Search. Fast Mode
отправляется с обязательным beta `fast-mode-2026-02-01` только для Opus 5 и Opus 4.8; официальный
conversion-каталог не изображает Fast у остальных моделей. Затем при наличии бюджета runner
добирает измеримый signal стандартными 1h cache-write запросами Fable 5. Итоговый JSON содержит
exact spend, движение 5ч/7д quota, реально недоступные capability и рейтинг модель × tier по
наблюдённому API-dollar equivalent на 1% 5-часового окна.

Это не frontend-тест и не расчёт по размеру prompt. Источники истины — backend `/capacity`,
immutable `calibration_evidence` и provider quota snapshots.

## Страховки

- Без явного `--execute` скрипт завершится до любого live-запроса.
- `--budget-usd` принимает не больше `40`; деньги переводятся в integer nanoUSD без float.
- Перед каждым generation выполняется бесплатный `/v1/messages/count_tokens`. Anthropic отклоняет
  server-side Web Search в этом endpoint, поэтому runner убирает только эту tool-схему из preflight,
  а затем консервативно добавляет к возвращённому input полную длину её compact JSON в UTF-8 байтах
  (число непустых токенов не может превышать число байт). Worst-case по-прежнему отдельно включает
  полный cache miss нужного TTL, весь `max_tokens` output и все разрешённые Web Search calls.
- Guard проверяет свободный бюджет **каждой** healthy-подписки, а не только ожидаемого sticky home:
  неожиданный affinity rebind не может перелить запрос на уже исчерпавший тестовый лимит аккаунт.
- После generation следующий запрос запрещён, пока exact token vector не появился ровно в одном
  backend evidence aggregate. Конкурентный трафик в другой model/tier строке не мешает; совпавший
  конкурентный aggregate считается неоднозначностью и останавливает прогон.
- Rebind, cooling/dead, degraded/pending delivery, движение aggregate назад или actual cost выше
  preflight bound останавливают конкретную небезопасную ветку fail closed. Provider quota wall
  выводит только соответствующую подписку из оставшейся нагрузки, не лишая данных другие профили.
  Ожидаемый 400/403/429 первого Fast-запроса сохраняется как `unavailable_capabilities` и не
  повторяется для остальных token legs той же model/profile пары. Отсутствие требуемого token class
  не прерывает оставшуюся матрицу, но помечает leg как `coverage_ok=false`, а весь отчёт как incomplete.
- Между turn одной подписки выдерживается 16 секунд — больше 15-секундного backend probe debounce.
  Это даёт post-turn poll шанс связать exact spend с новой quota fraction.
- API key и panel/control key читаются только из env/remote shell и в отчёт не попадают. Email уже
  приходит из `/capacity` в bounded mask без домена.
- Production-режим отправляет generation через SSH прямо в стабильный loopback router с
  forwarding-admin key, который раскрывается только внутри remote shell. Admin-only заголовок
  адресует bounded четырёхсимвольный profile hint, вырезается до upstream и fail-closed при
  коллизии. Поэтому normal customer routing/reserve не мешает измерить конкретную подписку, а
  калибровочный запрос никогда не spill/rebind-ится на соседнюю.

## Production-команда

Control snapshot и live generation безопасно выполняются на production host: remote shell загружает
`/srv/claude-api/data/server.env`; panel key используется только для JSON `/capacity`, а
forwarding-admin key — только для loopback `/v1/*`. Ни один секрет через SSH не печатается и в
локальный процесс не возвращается. `APITOKEN_API_KEY` нужен только для legacy/public режима без
`--production-api-over-ssh`.

```bash
python3 tools/claude_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --report /tmp/claude-calibration-report.json
```

Перед запуском обязательны зелёный production deploy и baseline:

- `calibration_delivery.pending_events=0`;
- `calibration_delivery.dropped_events=0`;
- `calibration_delivery.persistence_ok=true`;
- хотя бы одна `per_sub` строка `routable=true`, без `dead`/`cooling`.

У каждой routable строки должен быть непустой paid plan. Backend сначала использует OAuth profile;
для inference-only токена с 403 допускается только единогласный plan остальных подписок этого fleet.
Если fleet смешанный или полностью не размечен, нагрузку не запускают до появления authoritative plan.

В production SSH-режиме runner создаёт отдельный `x-session-id` на каждую healthy подписку и
проверяет первым микрозапросом, что admin-only exact target совпал с backend attribution. Target
обходит только мягкий routing reserve; hard 100% provider cap, cooling и auth-dead остаются
непроходимыми. В legacy/public режиме runner по-прежнему пытается разложить новые sessions обычным
capacity placement. Если все homes получить не удалось, нагрузочная матрица не начинается.

## Офлайн-проверка runner

```bash
python3 -m unittest tools.claude_calibration.test_run_live
```

Тесты покрывают budget/rebind guard, exact attribution на фоне конкурентного evidence, fail-closed
неоднозначность, все Claude token classes, tariff alias/ceiling, полный coverage plan и сортировку
наблюдаемой profitability.

## Как читать результат

`spent_nano_per_profile` — только стоимость запросов этого запуска в official API equivalent.
Она не равна списанию клиентского баланса: скидка/multiplier клиента здесь намеренно не участвует.

Каждая `records[]` строка содержит:

- requested/served model, tier и token leg;
- exact usage и `actual_nano` из backend evidence;
- count-tokens worst-case `upper_bound_nano`;
- observed `fraction_delta_5h` и `fraction_delta_7d` до/после turn.

`model_profitability` сортируется по `api_nano_per_1pct_5h` убыванию. `null` означает не нулевую
выгодность, а отсутствие различимого движения quota: провайдерская fraction грубее этого сегмента.
`unavailable_capabilities` отделяет проверенную недоступность Fast на конкретном профиле от нулевого
расхода, а `profile_stops` фиксирует настоящий provider quota wall/cooling без spill на соседа.
Для коммерческого вывода сравниваются только строки с положительным наблюдённым delta и достаточным
числом turn; full-window `final_capacity.window_totals` остаётся pooled realized-workload оценкой,
а не универсальным номиналом подписки для любой будущей смеси.
