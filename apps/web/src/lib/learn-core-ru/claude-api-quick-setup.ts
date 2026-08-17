import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Быстрый старт Claude API: от ключа до первого вызова за минуты",
    h1: "Быстрый старт Claude API: настройка и первый вызов",
    description: "Быстрый старт Claude API: создайте один ключ, направьте любой Anthropic-совместимый клиент на router.apitoken.sale и отправьте первый запрос /v1/messages через curl, Python, TypeScript или IDE.",
    keywords: ["claude api быстрый старт", "claude api настройка", "claude api первый запрос", "anthropic messages api", "claude api base url", "claude api curl пример", "claude api ключ", "как подключить claude api", "claude api quickstart", "claude api туториал", "купить доступ к claude api"],
    dek: "Этот быстрый старт Claude API проведёт вас от свежего аккаунта до завершённого вызова /v1/messages за несколько минут. Нужно ровно три вещи: один ключ sk-pool, base URL router.apitoken.sale и два HTTP-заголовка. Всё остальное — стандартный Anthropic Messages API, поэтому тот же код без изменений работает и против официального эндпоинта.",
    sections: [
      { h2: "Что на самом деле нужно для быстрого старта Claude API", blocks: [
        { type: "p", text: "Рабочая настройка Claude API — это не установка SDK и не неделя онбординга, а один HTTP POST с двумя заголовками. Зарегистрируйтесь, сгенерируйте ключ и отправьте запрос messages — первый 2xx обычно приходит быстрее, чем остынет кофе, который вы заварили, читая эту страницу. Эндпоинт говорит ровно на протоколе Anthropic Messages, поэтому каждый туториал, SDK и coding agent, написанный под Claude, уже знает, как с ним общаться." },
        { type: "list", items: [
          "Бесплатный аккаунт — без одобрения, без вейтлиста и без аккаунта Anthropic.",
          "Один API-ключ (выглядит как sk-pool-…), который работает со всеми поддерживаемыми моделями: Claude, GPT, Gemini и Kimi.",
          "Base URL https://router.apitoken.sale — единый эндпоинт для новых интеграций.",
          "Два заголовка в каждом запросе: x-api-key с вашим ключом и anthropic-version: 2023-06-01.",
        ] },
      ] },
      { h2: "Создайте ключ и выберите эндпоинт", blocks: [
        { type: "steps", items: [
          "Зарегистрируйтесь через Google, GitHub или email и откройте панель — очереди на проверку нет.",
          "Сгенерируйте ключ. Он показывается один раз — храните его в переменной окружения, а не в исходном коде.",
          "Укажите в клиенте base URL https://router.apitoken.sale и убедитесь, что запросы уходят на POST /v1/messages.",
        ] },
        { type: "code", code: `Base URL:  https://router.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
        { type: "p", text: "Ключ работает уже со следующего запроса — задержки на активацию нет. Если баланс пуст, сначала пополните его: пополнение принимает любую сумму в целых долларах, так что одного доллара достаточно, чтобы проверить весь пайплайн от начала до конца." },
      ] },
      { h2: "Отправьте первый запрос через curl", blocks: [
        { type: "p", text: "Прежде чем подключать что-то к приложению, проверьте путь минимальным вызовом. max_tokens обязателен в Messages API — его отсутствие самая частая ошибка первого вызова." },
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "p", text: "Успешный ответ — JSON-объект, у которого поле content — массив блоков; для обычного ответа это один блок типа text. На этапе настройки стоит читать два поля в каждом вызове: stop_reason показывает, завершила ли модель ответ (end_turn) или уперлась в ваш лимит max_tokens, а usage сообщает точные input_tokens и output_tokens, за которые вы заплатили. Если content вернулся пустым со stop_reason: max_tokens, поднимите лимит, а не повторяйте тот же запрос." },
      ] },
      { h2: "Тот же вызов из Python или TypeScript", blocks: [
        { type: "p", text: "Официальные SDK Anthropic принимают кастомный base URL, поэтому переход от curl к настоящему коду — переопределение в одну строку. Идентификаторы моделей, формат сообщений, системные промпты и tool use ведут себя ровно так же, как против api.anthropic.com." },
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(msg.content[0].text)` },
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "link", text: "Полный разбор SDK: anthropic-sdk-base-url", href: "/docs/learn/anthropic-sdk-base-url" },
      ] },
      { h2: "Включите стриминг до того, как строить UI", blocks: [
        { type: "p", text: "Всё, чего ждёт человек — чат, автодополнение кода, агентный цикл с видимым прогрессом — должно стримиться. Добавьте \"stream\": true в то же тело запроса, и ответ станет Server-Sent Events: конверт message_start, последовательность событий content_block_delta с фрагментами текста и message_stop. Клиент собирает фрагменты сам; в остальном запрос не меняется." },
        { type: "code", code: `curl -N https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "stream": true,\n    "messages": [{"role":"user","content":"Count to five."}]\n  }'` },
        { type: "note", text: "Две ловушки стриминга: без -N (или режима без буферизации в вашем HTTP-клиенте) curl буферизует всё тело SSE и выглядит в точности как нестриминговый вызов; а итоговый usage приходит в финальном событии message_delta, а не в JSON-теле — читайте его там, если считаете расход по каждому запросу." },
      ] },
      { h2: "Направьте IDE или coding agent на тот же ключ", blocks: [
        { type: "p", text: "Поскольку эндпоинт идентичен по протоколу, любой инструмент с настройкой провайдера Anthropic заработает после изменения двух полей. В Cursor, например: Settings → Models → Anthropic API — укажите base URL, вставьте ключ и выберите актуальный идентификатор модели." },
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "p", text: "Те же два поля покрывают расширения VS Code вроде Cline и Continue, а также терминальные агенты, которые читают ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY из окружения. Один ключ, один предоплаченный баланс, все инструменты." },
        { type: "link", text: "Отдельный гайд: claude-api-key-for-cursor", href: "/docs/learn/claude-api-key-for-cursor" },
        { type: "link", text: "Актуальный список моделей и цены по каждой", href: "/models" },
      ] },
      { h2: "Ошибки первого вызова: расшифровка", blocks: [
        { type: "p", text: "Почти каждый неудачный первый вызов — это один из четырёх статусов. Читайте и тело ответа: ошибки приходят в конверте Anthropic с сообщением, которое называет проблемное поле." },
        { type: "table", headers: ["Статус", "Что это значит", "Как исправить"], rows: [
          ["400 Bad Request", "Некорректное тело запроса — обычно отсутствует max_tokens или неизвестный идентификатор модели", "Задайте max_tokens; используйте актуальный идентификатор, например claude-opus-4-8"],
          ["401 Unauthorized", "Отсутствует или неверен x-api-key, либо запрос ушёл не на тот base URL", "Проверьте, что ключ вставлен целиком, а base URL — https://router.apitoken.sale"],
          ["402 / недостаточно баланса", "Предоплаченного баланса не хватает на запрос", "Пополните на любую сумму в целых долларах и повторите"],
          ["429 Too Many Requests", "Упёрлись в лимит параллельности или частоты", "Соблюдайте заголовок Retry-After и снизьте параллелизм"],
        ] },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Какой base URL использовать для быстрого старта Claude API?", a: "Используйте https://router.apitoken.sale с любым Anthropic-совместимым инструментом и отправляйте запросы на /v1/messages. Существующие интеграции на прежнем хосте https://api.apitoken.sale продолжают работать — единый роутер просто рекомендуемый эндпоинт для новых настроек." },
      { q: "Какой заголовок авторизации требует Claude API?", a: "Отправляйте x-api-key с вашим ключом и anthropic-version: 2023-06-01 — ровно как в официальном Anthropic API. Не используйте Authorization: Bearer на этой поверхности — этот заголовок относится к OpenAI-совместимой линии." },
      { q: "Нужен ли аккаунт Anthropic или привязанная карта?", a: "Аккаунт Anthropic не нужен — вы регистрируетесь через Google, GitHub или email и получаете собственный ключ sk-pool. Баланс предоплаченный: пополняете на любую сумму в целых долларах, и он расходуется только при выполнении запросов." },
      { q: "Как дешевле всего проверить, что настройка работает?", a: "Пополните минимальную сумму в целых долларах и отправьте один запрос с max_tokens: 1 — успешный 2xx подтверждает авторизацию, эндпоинт и биллинг одним вызовом. Новые аккаунты через Google или GitHub также начинают с бонусных $5 платформы, которых может хватить на весь тест." },
      { q: "Почему первый вызов возвращает 400, хотя ключ верный?", a: "Почти всегда дело в отсутствующем поле max_tokens или идентификаторе модели, который не включён — Messages API отклоняет запросы без max_tokens. Используйте актуальный идентификатор, например claude-opus-4-8, и задайте явный лимит токенов." },
      { q: "Можно ли использовать тот же ключ для стриминга и tool use?", a: "Да. Стриминг — это флаг \"stream\": true в том же запросе, а tool use следует стандартной схеме Anthropic — отдельный ключ, тариф или эндпоинт не нужны." },
    ],
  };
