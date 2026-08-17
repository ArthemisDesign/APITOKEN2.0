import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Стриминг Claude API (SSE): токены по мере генерации",
    h1: "Стриминг Claude API: SSE-ответы токен за токеном",
    description: "Как работает стриминг Claude API на apiToken.sale: stream:true, последовательность SSE-событий Anthropic, хелперы SDK, финальный подсчёт токенов и почему тарификация совпадает с обычными запросами.",
    keywords: ["claude api стриминг", "claude sse", "потоковые ответы claude", "anthropic streaming api", "claude api server-sent events", "stream true claude messages api", "anthropic sdk стриминг", "claude streaming python", "claude api ответы в реальном времени", "claude api stream пример"],
    dek: "Стриминг Claude API отправляет каждый токен по server-sent events сразу после генерации, не заставляя ждать всё сообщение целиком. На apiToken.sale это стандартный SSE-формат Anthropic на том же эндпоинте, с оплатой за токены ровно как у обычного запроса. Разбираем запрос, последовательность событий и сбои, которые важны в проде.",
    sections: [
      { h2: "Включаем стриминг флагом stream:true", blocks: [
        { type: "p", text: "Стриминг в Claude API — это один флаг, а не новый эндпоинт. Отправьте POST на https://router.apitoken.sale/v1/messages с ключом в заголовке x-api-key, заголовком anthropic-version: 2023-06-01 и «stream»: true в теле — шлюз ответит стандартным потоком server-sent events Anthropic вместо единого JSON-документа. Форма запроса, ID моделей и заголовки те же, что ждёт api.anthropic.com, поэтому любой клиент, который уже говорит на Messages API, стримит без изменений." },
        { type: "code", code: `curl -N https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-sonnet-5",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "Флаг -N отключает буферизацию вывода curl — самая частая причина, по которой первый тест стриминга выглядит сломанным: ответ приходит нормально, но curl держит его до закрытия соединения. Без буферизации вы видите, как события text_delta прилетают в реальном времени. Ответ имеет Content-Type: text/event-stream и остаётся открытым, пока модель не закончит." },
        { type: "steps", items: [
          "Сгенерируйте ключ в дашборде — он выглядит как sk-pool-••• и работает со всеми поддерживаемыми моделями Claude.",
          "Запустите curl выше с флагом -N и смотрите, как события приходят инкрементально.",
          "Убедитесь, что поток открывается message_start, несёт чанки content_block_delta и закрывается message_delta, а затем message_stop.",
          "Откройте экран использования в дашборде и сверьте входные и выходные токены запроса с тем, что сообщил поток.",
        ] },
      ] },
      { h2: "Читайте последовательность SSE-событий, а не сырой текст", blocks: [
        { type: "p", text: "Поток Anthropic — это типизированная последовательность событий, и самописные клиенты ломаются именно на попытке читать его как сырой текстовый фид. Каждое событие приходит строкой event: с именем плюс строкой data: с JSON. Минимальный поток для короткого ответа выглядит так:" },
        { type: "code", code: `event: message_start\ndata: {"type":"message_start","message":{"id":"msg_01...","role":"assistant","model":"claude-sonnet-5","usage":{"input_tokens":12,"output_tokens":1}}}\n\nevent: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\nevent: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}\n\nevent: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\nevent: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":9}}\n\nevent: message_stop\ndata: {"type":"message_stop"}` },
        { type: "table", headers: ["Событие", "Что оно несёт"], rows: [
          ["message_start", "Оболочка сообщения: id, role, model и usage.input_tokens для промпта"],
          ["content_block_start / content_block_stop", "Границы каждого блока вывода — текста или tool_use — по заданному индексу"],
          ["content_block_delta", "Инкрементальный text_delta для текстовых блоков, фрагменты input_json_delta для вызовов инструментов"],
          ["ping", "Keepalive между блоками; можно игнорировать, но не принимайте его за ошибку"],
          ["message_delta", "stop_reason и накопленный usage.output_tokens — авторитетный счёт выходных токенов"],
          ["message_stop", "Конец потока; после него соединение закрывается"],
        ] },
        { type: "p", text: "Из последовательности следуют два правила учёта. Первое: input_tokens фиксируется в message_start, а output_tokens накапливается и становится финальным только в message_delta, который несёт stop_reason, — поэтому записывайте usage из завершающих событий, а не пересчитывая дельты самостоятельно. Второе: генерация может содержать несколько блоков контента (текст вперемешку с tool_use), у каждого свой index, поэтому накапливайте дельты по индексу, а не склеивайте всё в одну строку. Аргументы инструментов приходят фрагментами частичного JSON в input_json_delta: их нужно конкатенировать и распарсить один раз на content_block_stop." },
        { type: "p", text: "Чтение потока без SDK требует одной дисциплины: делите по границам протокола, а не по сетевым чанкам. Читайте тело как поток (в браузере и Node 18+ res.body — это ReadableStream), буферизуйте байты до пустой строки и считайте всё между пустыми строками одним событием. Сетевые чанки не совпадают с событиями: строка data: может прийти разрезанной на два чтения, а несколько событий — уместиться в одно. Парсите JSON только из полезной нагрузки data: и только для тех типов событий, которые обрабатываете. EventSource здесь не вариант: он умеет только GET, а Messages API требует POST." },
        { type: "note", text: "Длинные потоки могут включать события ping или надолго замолкать, пока модель думает. Настраивайте таймаут чтения на тишину между событиями, а не на общую длительность потока — жёсткий общий таймаут в 30 секунд убьёт легитимные длинные генерации." },
      ] },
      { h2: "Что меняет стриминг — и чего не меняет", blocks: [
        { type: "p", text: "Официальные SDK прячут всю механику событий. Направьте клиент на шлюз и используйте его стриминговый хелпер: события выше отдаются итератором, а финальное сообщение с авторитетным usage получается одним вызовом:" },
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\n\nwith client.messages.stream(\n    model="claude-sonnet-5",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Explain SSE in one paragraph"}],\n) as stream:\n    for text in stream.text_stream:\n        print(text, end="", flush=True)\n    final = stream.get_final_message()\n    print(final.usage)  # input_tokens + final output_tokens` },
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\n\nconst stream = client.messages.stream({\n  model: "claude-sonnet-5",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Explain SSE in one paragraph" }],\n});\nstream.on("text", (text) => process.stdout.write(text));\nconst final = await stream.finalMessage();\nconsole.log(final.usage);` },
        { type: "p", text: "Не меняются деньги: запросы со стримингом и без тарифицируются одинаково — по входным и выходным токенам, — так что стримингом вы ничего не теряете. Стрим-ответ на 500 выходных токенов стоит ровно столько же, сколько те же 500 токенов в буферизованном виде, и в разбивке использования в дашборде запрос виден с теми же строками токенов в обоих режимах. Меняется воспринимаемая задержка (первый токен приходит за долю общего времени), устойчивость длинных генераций (простаивающее молчаливое нестриминговое соединение — ровно то, что прокси и балансировщики любят обрывать по таймауту) и то, как рано может реагировать ваш код: агент способен запустить вызов инструмента в момент, когда дописана его закрывающая скобка, а не после всего ответа." },
        { type: "list", items: [
          "Чат- и кодинг-интерфейсы, где пользователь смотрит, как появляется ответ, — разница между приложением, которое ощущается мгновенным, и тем, что выглядит зависшим.",
          "Длинные генерации, чтобы отрисовывать частичный вывод и действовать по нему раньше, держа занятым каждый узел на пути.",
          "Агенты, которые останавливаются или ветвятся, как только сгенерирован завершённый вызов инструмента.",
        ] },
        { type: "p", text: "Для коротких пакетных задач — классификации, извлечения, всего объёмом в несколько сотен токенов, что никто не смотрит, — без стриминга проще ретраить и логировать, а стоимость одинакова в любом случае. Какой бы режим вы ни выбрали, помните: поток может упасть уже после 200 OK. Событие event: error или обрыв соединения до message_stop означает, что генерация не завершилась. Считайте накопленный частичный вывод недоверенным — не сохраняйте его и не подавайте на следующий шаг агентского цикла — и повторите запрос." },
        { type: "link", text: "Оцените стоимость стрим-нагрузки в калькуляторе стоимости Claude API", href: "/tools/claude-api-cost-calculator" },
        { type: "link", text: "Если потоки падают с 429 под нагрузкой, см. гайд по rate limit", href: "/docs/learn/claude-api-rate-limits" },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Поддерживает ли apiToken.sale стриминг Claude API?", a: "Да. Установите «stream»: true в POST на https://router.apitoken.sale/v1/messages с заголовками x-api-key и anthropic-version — и получите стандартный SSE-поток Anthropic: message_start, чанки content_block_delta, message_delta с финальным usage, message_stop. Работает для кодинг-агентов, IDE, стриминговых хелперов официальных Python- и TypeScript-SDK Anthropic и продакшн-вызовов." },
      { q: "Стриминг ответов Claude стоит дороже, чем обычные запросы?", a: "Нет. Запросы со стримингом и без него тарифицируются одинаково — по входным и выходным токенам, и финальные итоги usage совпадают с буферизованным ответом: читайте их из завершающего события message_delta или с экрана использования в дашборде. Стриминг меняет только то, когда токены доходят до вас, а не их стоимость." },
    ],
  };
