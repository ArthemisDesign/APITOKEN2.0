# Live-калибровка GLM Coding Plan

`tools/glm_calibration/run_live.py` — операторский fail-closed прогон живой подписки GLM Coding
Plan (Z.ai / open.bigmodel.cn). В отличие от Claude/Gemini runner'ов, он ходит **напрямую в
провайдера**, а не через наш backend: движок, engine PostgreSQL и клиентский трафик не
затрагиваются, synthetic calibration vectors не создаются — runner только читает провайдера и
печатает evidence для оператора. Цель ровно одна подписка, зафиксированная тройкой
`--profile` (opaque label оператора) + `--base-url` (allowlist из двух хостов:
`https://api.z.ai` или `https://open.bigmodel.cn`) + ключ из env `GLM_CALIBRATION_API_KEY`.

Runner снимает `unknown`'ы из §6 `docs/engine/GLM_PROVIDER.md`: форму `usage` на
Anthropic-маршруте, реальную инкрементальность SSE, семантику единиц quota endpoint и точные
business-коды quota wall. Платная матрица — три подписочные модели (`glm-5.2`, `glm-5-turbo`,
`glm-4.7`) × (non-stream + incremental stream) на `POST {base}/api/anthropic/v1/messages` с
Bearer и флот-фингерпринтом Claude Code (`User-Agent: claude-cli/2.1.195 (external, sdk-cli)`,
`anthropic-version: 2023-06-01`, `anthropic-beta: claude-code-20250219`).

## Страховки

- Без явного `--execute` платный трафик не отправляется. Dry-run печатает machine-readable
  план (ноги, per-leg worst-case, суммарный worst-case) и — если ключ установлен — снимает
  только бесплатный read-only quota anchor. Dry-run без ключа не падает молча: план честно
  говорит `live_possible=false` и почему.
- `--budget-usd` — integer nanoUSD без float. Hard cap по умолчанию **$0.05** (масштаб
  admission micro-smoke из AGENTS.md); поднять до абсолютного потолка **$5** можно только
  явным `--i-understand`.
- Перед каждым платным запросом runner считает и печатает worst-case bound по официальному
  rate card (те же числа, что `crates/metering/src/glm.rs`): input ≤ длина промпта в байтах
  (ASCII) + 32 токена на framing, output ≤ `max_tokens` (hard limit 1024), cache — полный miss.
  Бюджетный guard проверяется до dispatch; actual, превысивший bound, останавливает прогон.
- Attribution идёт через quota endpoint `GET {base}/api/monitor/usage/quota/limit`
  (`Authorization: <key>` **без Bearer**; невалидный ключ — HTTP 200 с `code: 401` в теле).
  До ноги — before-наблюдение, после ответа — два settled-наблюдения с задержкой
  (`--quota-poll-delay`, default 5s). Дельта приписывается served-модели, только если каждый
  двинувшийся счётчик равен ровно ожидаемым целым кредитам по официальной формуле (с off-peak
  ×0.5 по расписанию UTC+8). Любое отклонение — чужой трафик, лаг учёта или неизвестные
  единицы — записывается `unattributed` и останавливает матрицу fail closed, без угадывания.
  Нулевая дельта при суб-кредитной ноге — честный `below-resolution`, не ошибка.
- Retry только read-only: quota poll повторяется до трёх раз при transport failure. Платный
  запрос после transport ambiguity НИКОГДА не повторяется автоматически: нога удерживается по
  полному worst-case bound (`held-ambiguous`) и не переотправляется даже при resume — новую
  попытку делает только новый run id.
- Typed-отказы классифицируются по business-коду: `1311` (модель не в плане) — проверенная
  недоступность capability, остальные модели продолжаются; `1308`/`1310` — quota wall:
  evidence записывается (снимает §6.6), платный трафик останавливается; `401` — ключ мёртв,
  стоп до любого следующего запроса. Остальное — fail closed.
- Checkpoint после каждой ноги (атомарная запись). `--resume <checkpoint>` продолжает тот же
  run id без повторения completed ног; несовпадение profile/base-url/budget/models/max_tokens
  с checkpoint'ом — fail closed. Свежий прогон отказывается затирать чужой checkpoint.
- Ключ читается только из env и нигде не материализуется: не в argv, не в отчёте, не в
  checkpoint, не в stdout/stderr; в typed error details ключ вырезается. Отчёт адресует цель
  только по opaque `--profile` + `--base-url`.

## Команда

```bash
# 1. План + бесплатный quota anchor (платного трафика нет):
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai

# 2. Платная матрица (6 ног, суммарный worst-case ~$0.002 при дефолтах):
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai \
  --execute --budget-usd 0.05 \
  --report /tmp/glm-calibration-report.json

# 3. Прерванный прогон — продолжение без повторения ног:
GLM_CALIBRATION_API_KEY=... python3 tools/glm_calibration/run_live.py \
  --profile glm-pro-1 --base-url https://api.z.ai \
  --execute --budget-usd 0.05 \
  --resume /tmp/glm-calibration-report.json.checkpoint.json
```

Во время прогона на подписке не должно быть другого трафика: sequential execution и один
profile — единственная гарантия attribution; чужое движение квоты конвертируется в
`unattributed` и останавливает матрицу. Чтобы снять §6.3 (единицы quota endpoint), нужна нога
с дельтой ≥ 1 кредита: `--max-tokens 1024 --i-understand --budget-usd` выше дефолта. Quota wall
специально не провоцируется; если стена встретилась сама, её business-код окажется в отчёте.

## Офлайн-проверка runner

```bash
python3 -m unittest tools.glm_calibration.test_run_live
# или
python3 -m pytest tools/glm_calibration/ -q
```

Тесты покрывают budget guard и hard caps, run-id resume без повторения ног, запрет повтора
платного запроса после transport ambiguity, secret containment (ключ не в отчёте/checkpoint/
stdout и вырезается из error details), attribution на фоне чужого трафика → `unattributed`,
dry-run без платного трафика, парсер quota (HTTP 200 + `code: 401`), полноту отчёта и паритет
денег/кредитов/off-peak с `crates/metering/src/glm.rs`.

## Как читать результат

Отчёт (`glm-live-calibration/v1`) — machine-readable JSON:

- `legs[]` — per-leg exact spend: `api_nanousd` по rate card из авторитетного `usage`,
  preflight `worst_case_nanousd`, `usage_observed_keys` (реальные имена полей — evidence для
  §6.1), before/after quota-наблюдения, `quota_deltas`, `attribution`
  (`attributed`/`below-resolution`/`unattributed`), для stream — `stream_evidence` (число
  кадров, text-delta, first-to-last ms, признак инкрементальности).
- `coverage` — статус каждой модели × {non_stream, stream}
  (`ok`/`unavailable`/`held-ambiguous`/`failed`/`not-run`).
- `unavailable_capabilities` — проверенная недоступность (например `1311`), а не нулевой расход.
- `unattributed_deltas` — сырые дельты, которые runner отказался угадывать.
- `unknowns` — явный статус по §6: `usage_form`, `sse_incrementality`, `quota_units`,
  `quota_wall_codes` — каждый `resolved`/`unresolved` с detail. `unresolved` — не сбой runner'а,
  а честное «не доказано в этом прогоне» (например wall не встретилась, а дельты были ниже
  разрешения провайдера).
- `complete: false` + `failure` — прогон остановился fail closed; частичный отчёт и checkpoint
  сохранены, продолжение — через `--resume`. Формально complete требует `ok`/`unavailable` по
  всем ногам; `held-ambiguous` нога навсегда держит прогон incomplete — это намеренно.

`spent_nanousd` — стоимость запросов прогона в official API replacement cost, не списание с
квоты подписки и не цена плана. `held_nanousd` — консервативный hold по worst-case bound для
ног с недоказанным исходом.
