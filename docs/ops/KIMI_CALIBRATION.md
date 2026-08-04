# Live-калибровка KIMI-плоскости

`tools/kimi_calibration/run_live.py` — операторский fail-closed прогон KIMI subscription-плоскости
(backend-only, default-off). Runner не выводит ёмкость из размера prompt и не доверяет клиентскому
списанию. Источники истины — admin-only `GET /kimi-subs` (control key в `x-api-key`): delivery
диагностика (`pending_events`/`dropped_events`/`persistence_ok`), immutable
`calibration_recent_turns` (единственный API-dollar authority, деньги — decimal strings,
неизвестное — `null`, никогда `0`) и per-window quota observations, идентифицируемые точным
`duration_secs` (18000 — rolling 5h, 604800 — weekly; duration — данные, а не зашитый список).
Aggregate `calibration`-строка профиля остаётся статистикой estimator и для атрибуции отдельного
запроса не используется. Профили фигурируют только по opaque roster id (1..128, ASCII alnum plus
`-`/`_`); `subject_id`/email/phone в контракте не появляются и в report не попадают.

Endpoint `/kimi-subs` и admin-only calibration request headers приходят отдельным engine-изменением;
runner написан против зафиксированного контракта, а офлайн-тесты мокают endpoint, поэтому его
существование для проверки runner не требуется.

## Что покрывает прогон

Матрица строится из `--models` (по умолчанию документированный served set: `kimi-k3`,
`kimi-k2.7-code`, `kimi-k2.7-code-highspeed`, `kimi-k2.6`) × context mode × reasoning effort,
который плоскость принимает. Запрос идёт точным подписочным alias (`kimi-for-coding`,
`kimi-for-coding-highspeed`, `k3-256k`, `k3`) на Anthropic-compatible `POST /v1/messages`; served
model берётся только из immutable event. `k3`: 256k всегда, 1m — только когда paid plan профиля
входит в reviewed список `--one-m-plans` (список может быть пуст — тогда 1m legs записываются в
`unavailable_capabilities` с доказательством, а не пропускаются молча), efforts `low`/`high`/`max`
плюс `off`. Coding-семейство: `high` (Thinking ON) плюс `off`. `kimi-k2.6` не имеет собственного
alias и покрывается через документированный thinking-off re-route (`k3` и `k2.7-code` served'ятся
как `kimi-k2.6`); дубликаты leg'ов dedup'ятся по точной тройке alias/context/effort.

## Страховки

- Без `--execute` runner печатает детерминированный машиночитаемый dry-run plan
  (`kimi-live-calibration-plan/v1` с per-leg `upper_bound_nanousd`) и завершается с кодом 0.
  Ни один live-запрос без `--execute` не отправляется.
- `--budget-usd` парсится строгим decimal-парсером (без float/exponent) в integer nanoUSD,
  default `0.0001`; значения выше hard cap `0.0001` USD (= `100_000` nanoUSD) — CLI-ошибка.
  Бюджет единый aggregate на весь запуск.
- Перед каждым платным запросом строится worst-case upper bound по official rate card served
  модели: полный accepted input context alias'а по miss-ставке (cache write == miss, hit дешевле)
  плюс весь запрошенный max output. Billing follows the served model: для thinking-off legs в
  candidates входит и `kimi-k2.6`, берётся per-class максимум. `charge` проверяет
  `actual <= upper_bound` и никогда не пересекает aggregate cap.
- Платные turns идут только на явный `--profile <opaque id>`; формат id валидируется. Если
  endpoint показывает профиль dead (`live=false`), не authenticated или cooling (любой non-null
  `cooling.auth_until`/`transport_until`/`quota_until` в будущем) — стоп до траты.
- Baseline health до каждого paid leg и после атрибуции: `enabled=true`,
  `calibration_authority_available=true`, `delivery.pending_events=0`,
  `delivery.dropped_events=0`, `delivery.persistence_ok=true`,
  `calibration_recent_turn_limit >= 512`. Любое нарушение — fail-closed стоп.
- Per-paid-turn runner чекает canonical UUIDv4, проверяет его отсутствие в before-снапшоте и
  шлёт admin-only заголовками `x-apitoken-calibration-profile` (полный opaque id, exact target,
  engine отказывает без spill/rebind) и `x-apitoken-calibration-request-id`. Атрибуция: poll
  `/kimi-subs`, пока не появится ровно один новый turn event с этим exact request id, profile id
  и served model; `0` после таймаута или `>1` — fail closed. Параллельные чужие events по id
  игнорируются; duplicate same-id events — fail closed; rebind (mismatch profile/served) — стоп.
- После durable event и `pending_events==0` runner ждёт post-turn window observation с
  `observed_at >= completed_at` и только потом считает per-window fraction/native delta.
  Unresolved snapshot — не ноль: leg исключается из profitability. Смена `resets_at` identity —
  `reset-crossed`, никакого delta.
- Платный запрос после transport ambiguity НИКОГДА не повторяется автоматически (ровно одна
  попытка). Единственное safe-stop доказательство — `x-apitoken-execution-state: not_started` на
  synthetic pre-delivery ошибках; только такие 429/503 останавливают прогон как explicit
  transient stop. Read-only операции (GET `/kimi-subs`, discovery) повторяются bounded (3 попытки).
- Уникальный run id `kimi-cal-<ts>-<uuid8>`. Report не содержит секретов и raw prompts (только
  bounded `prompt_sha256_12`), только opaque profile id.
- API/control keys читаются только из env или раскрываются внутри remote shell production
  SSH-режима; в argv, report и test fixtures они не попадают.
- HTTP 400/403/404 required capability и нарушенная cost-vector сумма — fail-closed стоп с
  записью в `unavailable_capabilities` (`blocking=true`); недоступные capabilities записываются
  с доказательством, а не пропускаются молча.

## Production-команда

Платный трафик ЗАПРЕЩЁН без явного human authorization. Живая подписка Kimi Code — обязательный
prerequisite (сейчас это human-blocked dependency), равно как и engine-изменение с `/kimi-subs`
и calibration headers. Сначала всегда dry-run (ничего не отправляет, exit 0):

```bash
python3 tools/kimi_calibration/run_live.py \
  --profile <opaque-profile-id> \
  --models kimi-k3 kimi-k2.7-code kimi-k2.7-code-highspeed kimi-k2.6
```

Затем, только после явного человеческого разрешения, зелёного deploy exact runtime SHA и
preflight checklist ниже:

```bash
python3 tools/kimi_calibration/run_live.py \
  --execute \
  --profile <opaque-profile-id> \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 0.0001 \
  --report /tmp/kimi-calibration-report.json
```

`$0.0001` — жёсткий default ceiling aggregate бюджета на весь запуск: больше через CLI передать
нельзя. Production SSH path читает `/kimi-subs` и отправляет paid turns в loopback стабильного
origin KIMI-плоскости (`127.0.0.1:8803`); forwarding-admin key раскрывается только внутри remote
shell (`${CLAUDE_API_KEYS%%,*}`), control key — `$CLAUDE_API_CONTROL_KEY`, ни один секрет через SSH
не возвращается. 1m legs требуют reviewed plan: `--one-m-plans Allegretto Allegro Vivace`
(по умолчанию список пуст, и 1m записывается unavailable).

Preflight checklist (всё обязательно):

- зелёный production deploy exact runtime SHA, `/kimi-subs` отвечает control key;
- `enabled=true`, `calibration_authority_available=true`;
- `delivery.pending_events=0`, `delivery.dropped_events=0`, `delivery.persistence_ok=true`;
- `calibration_recent_turn_limit >= 512`;
- ≥1 профиль с `live=true`, `authenticated=true`, без cooling в будущем, с непустым paid plan;
- точный opaque profile id для `--profile` из `profiles[].id`.

## Офлайн-проверка

```bash
python3 -m unittest tools.kimi_calibration.test_run_live
```

Тесты (unittest + mock, engine не поднимается) покрывают dry-run по умолчанию без единого
запроса, hard cap `$0.0001` и строгий decimal-парсер, aggregate guard на последовательных legs,
upper-bound математику по каждой served модели (включая k3→k2.6 re-route pricing), exact
request-id атрибуцию на фоне чужих событий, fail-closed duplicate/rebind/baseline/cooling/dead,
unresolved quota (не ноль, исключён из profitability), reset-crossed, отсутствие paid retry
после ambiguous transport, read-only retries, отсутствие секретов в report/argv, запись
incomplete report при failure и end-to-end `Runner.execute_leg` на FakeApi/FakeSubs.

## Как читать результат

Report `kimi-live-calibration/v1` (default `/tmp/kimi-calibration-report.json`) пишется даже при
failure с `complete=false` и частичными records: `run_id`, `budget_nanousd_total` /
`spent_nanousd_total` (strings), `profile`, `plan`, `models`, `records[]` (leg,
requested/served model, `context_mode`, `reasoning_effort`, exact `request_id`,
`upper_bound_nanousd`/`actual_nanousd` strings, полный usage и api_cost vectors strings,
per-window `status` (`resolved`/`unresolved`/`reset-crossed`) с fraction/native delta или
`null`), `unavailable_capabilities`, `stops`, `coverage` (expected/completed/pending legs),
`model_profitability`, `final_observations`.

Выводы о profitability/remaining capacity допустимы только для legs с положительным различимым
quota delta и без чужого immutable turn на том же профиле внутри interval: такие legs помечены
`profitability_eligible=true`, остальные из рейтинга исключены. `model_profitability` отсортирован
по API nanoUSD на 1% окна (точный `window_duration_secs`) убыванию отдельно для каждой связки
plan × served model × context × effort. Отсутствующая строка означает недостаточную provider
resolution/изоляцию, а не нулевую ценность модели. `complete=true` возможен только без blocking
unavailable и без pending legs; любой partial report — повод для ручного разбора, повторять
платный запрос после transport ambiguity запрещено всегда.
