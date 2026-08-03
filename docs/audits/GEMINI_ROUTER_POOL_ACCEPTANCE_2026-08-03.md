# Gemini pool through unified router — production acceptance, 2026-08-03

## Итог

Published text path Gemini принят для клиентского трафика через unified router в проверенном
объёме: два здоровых subscription-профиля обслужили non-stream, incremental SSE, sticky session и
повторяемый cache payload; нездоровый профиль не попал в untargeted dispatch; локальной очереди
исполнения и потерянных settlement events не обнаружено. Fault rotation, высокая параллельность,
disconnect и FIFO дополнительно закрыты детерминированными тестами.

Это не безусловный GA всей Gemini capability matrix. Live-прогон остановился fail-closed на
обнаруженном расхождении тарификации inline audio. Опубликованные модели после исправления
локально отклоняют такой input до provider dispatch. Dormant `gemini-3-flash-preview` и его точный
PCM WAV fallback требуют отдельного Pro+Ultra exact-SHA gate и не считаются опубликованными.

## Exact runtime и бюджет

- `14953d7f50f360578b1b74818bf6801a91ab4fab` — router pool verification, сохранение sticky и
  calibration headers, немедленный бесплатный quota probe после durable exact-target event.
- `5ba257bd95b5dd3571213eecb89e7dc801a6fb30` — fail-closed защита опубликованных моделей от
  нетарифицируемого inline audio.
- Оба SHA влиты в `master`, прошли exact-candidate gate и получили зелёный `deploy/watchdog`.
- Billable traffic шёл через stable unified router `127.0.0.1:8802`; capacity, profile state и
  immutable evidence читались с Gemini plane `127.0.0.1:8794` на том же production host.
- Жёсткий лимит прогона: `$30.00`. Совокупный API-equivalent расход всех controlled turns:
  `3,177,060 nanoUSD` (`$0.00317706`), остаток лимита: `$29.99682294`. В основной versioned report
  вошли `2,917,160 nanoUSD`; ещё `259,900 nanoUSD` относятся к ограниченным подготовительным и
  post-run sticky checks. Бесплатные `countTokens` и fail-closed audio smokes расход не увеличили.

## Live evidence

Проверялся опубликованный `gemini-2.5-flash-lite`. В пуле было три opaque-профиля:

| Профиль | План | Результат |
| --- | --- | --- |
| `gemini_oauth_000001` | Google AI Pro | authenticated, healthy, обслужил controlled turns |
| `gemini_oauth_000003` | Google AI Ultra | authenticated, healthy, обслужил controlled turns |
| `gemini_oauth_000002` | Google AI Pro | unauthenticated/cooling, корректно исключён из dispatch |

На обоих здоровых профилях подтверждены real output, terminal finish, terminal usage и точное
совпадение response usage с immutable turn. Non-stream generation прошёл на Pro и Ultra.
Incremental SSE вернул по два candidate frames на каждом профиле, то есть это не буферизованный
однокадровый ответ.

Cache payload содержал `12,340` fresh input tokens при первом cold turn. Повтор идентичного payload
показал `8,172` cache-read и `4,168` fresh tokens, а последующие reads сохранили тот же warm split.
Две подписки были прогреты одним cache root, после чего warm placement переиспользовался.

Одна native session два последовательных раза осталась на `gemini_oauth_000001`. Наблюдаемые
end-to-end latency были `3,349 ms` и `3,543 ms`. Это подтверждает отсутствие явной локальной
задержки/ребинда в этом smoke, но не является нагрузочным SLA: время включает inference Google, а
live-выборка содержит только два sticky turn.

Exact-target бесплатный `countTokens` на нездоровый профиль вернул `503`; следующий untargeted
`countTokens` сразу прошёл через здоровую часть пула. После turns backend показывал
`pending_events=0`, `dropped_events=0`, `persistence_ok=true`, `inflight=0` и
`usage_metadata_missing=0`. Это подтверждает live exclusion и чистый FIFO tail, но не подменяет
fault-injection тесты ротации.

## Детерминированная проверка нагрузки и отказов

Rust suites покрыли свойства, которые небезопасно или бессмысленно воспроизводить платным
production flood:

- sticky affinity не меняет профиль при `1,000` параллельных посторонних запросах и при `1,000`
  уже занятых leases на preferred profile;
- `10,000` immediate leases выдаются без semaphore, execution queue или искусственного provider
  reset; inflight используется только как least-loaded signal для новой непривязанной сессии;
- новый shared cache root прогревает две подписки, затем предпочитает уже тёплую копию;
- `401/403`, `429` с `RetryInfo`, transport errors и `5xx` охлаждают только надлежащий
  profile/model scope и вращают запрос на допустимый профиль;
- после первого SSE byte retry запрещён; client disconnect дренирует terminal usage и выполняет
  settlement, не оставляя деньги или inflight зависшими;
- durable spend event, settlement, exact-target probe wake и quota observation сохраняют FIFO;
- unified router сохраняет native session/calibration headers и транзитивно закрывает plane
  connection при disconnect клиента.

Для exact candidate выполнены `cargo test -p forward gemini::api::tests`,
`cargo test -p forward gemini::pool::tests`, `cargo test -p claude-router`,
`cargo test -p claude-api`, Python-тесты live runner и полный locked workspace gate. Итог полного
Rust workspace: `1,316 passed`, один Redis-only тест был штатно ignored в этом прогоне; отдельный
shared-affinity Redis proof после этого добавлен в обязательный gate на текущем `master`.

## Audio attribution miss и remediation

Controlled Pro audio turn вернул реальный output, но provider usage сообщил `58` generic input
tokens и `0` audio input tokens. Бесплатный `countTokens` на том же классе input вернул только
aggregate без modality split. Поэтому официальный более дорогой audio SKU нельзя было точно
отделить от text SKU; продолжать audio matrix или угадывать стоимость было запрещено.

На `5ba257bd95b5dd3571213eecb89e7dc801a6fb30` опубликованные модели стали отклонять inline audio до
provider dispatch для generation и обеих форм `countTokens`. Post-deploy smoke дал:

- через unified router: generation `400`, `countTokens` `400`, обычный text `countTokens` — `200`
  и `totalTokens=6`;
- на loopback Gemini plane оба audio-запроса вернули `400` с
  `x-apitoken-execution-state: not_started`;
- для уникальных calibration request IDs появилось `0` immutable events;
- после smoke: `pending_events=0`, `dropped_events=0`, `persistence_ok=true`, `inflight=0`,
  `usage_metadata_missing=0`.

Inline images остаются разрешены. Отдельный dormant Flash Preview путь допускает только строгий
integral-duration PCM WAV fallback по документированному Google правилу `32 tokens/sec`; он не
входит в этот acceptance и остаётся закрытым для production catalog до собственного live gate.

## Границы вывода

- Live evidence относится к `gemini-2.5-flash-lite`, двум здоровым subscription plans и
  перечисленным text/SSE/cache/sticky сценариям, а не ко всем моделям и capabilities каталога.
- Реальная ротация доказана для исключения одного cooling-профиля бесплатным запросом; полный
  `401/429/5xx`, burst и high-inflight matrix доказан детерминированно, без платного production
  flood.
- Audio на опубликованных моделях безопасно fail-closed, но пока функционально недоступен.
- Необходимый следующий отдельный gate — fresh exact-SHA Pro+Ultra прогон dormant Flash Preview
  PCM accounting; до него расширять GA-формулировку нельзя.
