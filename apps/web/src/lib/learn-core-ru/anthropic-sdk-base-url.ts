import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Anthropic SDK с кастомным base URL",
    h1: "Направьте Anthropic SDK на apiToken.sale",
    description: "Используйте официальные Anthropic SDK для Python и TypeScript с apiToken.sale, задав base_url на router.apitoken.sale. Тот же SDK, тот же код, ниже цена за токен.",
    keywords: ["anthropic sdk base url", "anthropic python sdk кастомный endpoint", "claude sdk base url", "anthropic typescript sdk", "claude api sdk", "anthropic_base_url переменная окружения", "claude api кастомный endpoint", "anthropic sdk прокси", "@anthropic-ai/sdk baseurl", "claude api gateway url"],
    dek: "Каждый официальный Anthropic SDK принимает кастомный base URL, поэтому переход на apiToken.sale — это изменение одного аргумента. Идентификаторы моделей, код сообщений и логика стриминга остаются ровно теми же — меняются только эндпоинт и цена за токен.",
    sections: [
      { h2: "Один аргумент переключает эндпоинт", blocks: [
        { type: "p", text: "Оба официальных SDK Anthropic — Python и TypeScript — позволяют переопределить корень API при создании клиента. Укажите https://router.apitoken.sale, и каждый запрос, который ваш код уже отправляет, будет обслуживаться шлюзом apiToken.sale вместо api.anthropic.com. Больше в кодовой базе ничего не меняется: тот же пакет anthropic, тот же Messages API, те же идентификаторы моделей вроде claude-opus-4-8, те же объекты ответов." },
        { type: "p", text: "Меняется биллинг. Каждый вызов тарифицируется по официальным токен-расценкам Anthropic, из суммы вычитается ваша фиксированная скидка 50%, а итог списывается с предоплатного баланса, который вы пополняете на целое число долларов. Никакой подписки и платы за место — дни простоя ничего не стоят." },
      ] },
      { h2: "Python: base_url в клиенте", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
        { type: "p", text: "Асинхронный клиент принимает тот же именованный аргумент: AsyncAnthropic(base_url=..., api_key=...). Стриминг через client.messages.stream, tool use, системные промпты и промпт-кеширование работают по тому же соединению — отдельный эндпоинт для них настраивать не нужно." },
        { type: "note", text: "Передавайте голый корень, без пути. SDK сам добавляет /v1/messages, поэтому base_url=\".../v1\" приведёт к запросам на /v1/v1/messages и ошибке 404. То же правило действует для TypeScript SDK." },
      ] },
      { h2: "TypeScript: baseURL в клиенте", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "p", text: "Пакет @anthropic-ai/sdk сам отправляет заголовки x-api-key и anthropic-version — ровно так же, как при работе с официальным эндпоинтом. Ретраи, таймауты и классы ошибок (APIError, RateLimitError и остальные) ведут себя идентично, поэтому существующая обработка ошибок продолжает работать." },
      ] },
      { h2: "В общем коде используйте переменные окружения", blocks: [
        { type: "p", text: "Оба SDK читают ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY из окружения, если аргументы конструктора не заданы. Тогда переключение становится деталью деплоя, а не изменением кода — удобно, когда один репозиторий работает с разными эндпоинтами в разработке и в продакшене." },
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# your code now constructs Anthropic() with no arguments` },
        { type: "p", text: "Инструменты поверх SDK наследуют те же переменные. Например, Claude Code напрямую учитывает ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY, а фреймворки вроде LangChain или LiteLLM пробрасывают это окружение своему Anthropic-клиенту внутри. Явные аргументы конструктора важнее переменных окружения, если заданы оба, поэтому разовый оверрайд в скрипте никогда не протечёт в конфигурацию деплоя." },
      ] },
      { h2: "Что проходит через шлюз без изменений", blocks: [
        { type: "list", items: [
          "Весь Messages API: POST /v1/messages с тем же JSON запроса и ответа.",
          "SSE-стриминг — инкрементальные чанки приходят ровно как с api.anthropic.com.",
          "Tool use и function calling, включая многоходовые циклы tool_result.",
          "Системные промпты, vision-входы и промпт-кеширование с брейкпоинтами cache_control.",
          "Объект usage в каждом ответе — ваш код учёта токенов и расходов продолжает работать.",
          "Идентификаторы моделей: claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 и остальной поддерживаемый каталог.",
        ] },
        { type: "p", text: "Один ключ покрывает все поддерживаемые модели — Claude наряду с GPT, Gemini и Kimi, — поэтому в мультипровайдерном проекте остаются одни креды и один баланс. Расход по каждому запросу и применённая скидка видны в панели после каждого вызова." },
        { type: "link", text: "Поддерживаемые идентификаторы моделей и цены по каждой", href: "/models" },
        { type: "link", text: "Оцените месячный расход в калькуляторе стоимости", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "Чеклист первого запроса и частые ошибки", blocks: [
        { type: "steps", items: [
          "Создайте бесплатный аккаунт, откройте панель и сгенерируйте ключ — он выглядит как sk-pool-… и работает с поддерживаемыми моделями Claude, GPT, Gemini и Kimi.",
          "Задайте base_url / baseURL на https://router.apitoken.sale в коде или экспортируйте ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY.",
          "Один раз запустите сниппет на Python или TypeScript выше и убедитесь, что получаете обычный ответ Anthropic message.",
          "Откройте панель и проверьте, что запрос появился с токен-usage, стоимостью и скидкой.",
        ] },
        { type: "table", headers: ["Статус", "Значение", "Решение"], rows: [
          ["401 Unauthorized", "Отсутствует или неверен x-api-key, либо неправильный base URL", "Перепроверьте ключ и что URL — голый корень"],
          ["400 Bad Request", "Некорректное тело запроса", "Проверьте идентификатор модели и что задан max_tokens"],
          ["402 Payment Required", "Недостаточно средств на предоплатном балансе", "Пополните на любое целое число долларов в панели"],
          ["429 Too Many Requests", "Параллелизм выше текущего лимита", "Учитывайте Retry-After и снизьте параллелизм"],
        ] },
        { type: "p", text: "Поскольку SDK, формат протокола и таксономия ошибок идентичны на обоих эндпоинтах, переключение обратимо в любой момент: верните base_url на api.anthropic.com (или удалите оверрайд) — и тот же код снова общается с Anthropic напрямую. Многие команды на неделю миграции держат оба клиента рядом и направляют небольшой процент трафика на новый эндпоинт перед полным переключением." },
        { type: "note", text: "Существующие интеграции на прежнем хосте https://api.apitoken.sale продолжают работать. Единый роутер router.apitoken.sale — рекомендуемый эндпоинт для новых настроек, потому что один base URL обслуживает всех четырёх провайдеров." },
        { type: "note", text: "Новые аккаунты, созданные через Google или GitHub, получают приветственный бонус $5 на баланс платформы — действует на поддерживаемые модели Claude, GPT, Gemini и Kimi; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Можно ли и дальше пользоваться официальным Anthropic SDK?", a: "Да. Задайте base_url (Python) или baseURL (TypeScript) на https://router.apitoken.sale, и всё остальное — импорты, идентификаторы моделей, стриминг, обработка ошибок — останется прежним." },
      { q: "Меняются ли идентификаторы моделей при смене base URL?", a: "Нет. Используйте те же идентификаторы, что и в официальном API, например claude-opus-4-8, claude-sonnet-5 и claude-haiku-4-5." },
      { q: "Должен ли base URL заканчиваться на /v1?", a: "Нет. SDK сам добавляет /v1/messages к переданному корню, поэтому завершающий /v1 ломает путь. Передавайте ровно https://router.apitoken.sale." },
      { q: "Работают ли стриминг и tool use через кастомный base URL?", a: "Да. Шлюз обслуживает стандартный Anthropic Messages API, поэтому SSE-стриминг, вызовы инструментов, системные промпты и промпт-кеширование ведут себя ровно как с api.anthropic.com." },
      { q: "Как позже вернуться на Anthropic?", a: "Уберите аргумент base_url / baseURL или снимите ANTHROPIC_BASE_URL. SDK вернётся к умолчанию https://api.anthropic.com — других изменений в коде не потребуется." },
    ],
  };
