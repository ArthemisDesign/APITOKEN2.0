import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Используйте Claude API с LangChain",
    h1: "Используйте Claude API с LangChain",
    description: "Подключите LangChain к Claude через apiToken.sale: направьте ChatAnthropic на router.apitoken.sale, оставьте те же ID моделей и платите за токены на 50% меньше.",
    keywords: ["claude api langchain", "langchain anthropic", "langchain claude", "chatanthropic base url", "langchain claude api key", "langchain anthropic_api_url", "langchain claude api ключ", "langgraph claude api", "chatanthropic streaming", "langchain claude дешевле"],
    dek: "Claude API работает с LangChain из коробки, и ChatAnthropic принимает кастомный URL API — ваши цепочки и агенты заработают на Claude через apiToken.sale после правки в две строки. Тот же пакет langchain-anthropic, те же ID моделей, тот же стриминг и вызов инструментов; меняются только эндпоинт и цена за токен.",
    sections: [
      { h2: "Направьте ChatAnthropic на router.apitoken.sale", blocks: [
        { type: "p", text: "Интеграция Anthropic в LangChain принимает кастомный URL API, поэтому подключение Claude API к LangChain через apiToken.sale — это ровно два аргумента конструктора: anthropic_api_url и anthropic_api_key. Промпты, парсеры вывода, колбэки и логика ретраев в существующих цепочках остаются нетронутыми." },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nllm = ChatAnthropic(\n    model="claude-opus-4-8",\n    anthropic_api_url="https://router.apitoken.sale",\n    anthropic_api_key="sk-pool-•••",\n)\nprint(llm.invoke("Hello").content)` },
        { type: "note", text: "Передавайте корень роутера ровно как показано: без завершающего слэша и без суффикса /v1. Вложенный Anthropic-клиент сам добавляет /v1/messages, и удвоенный путь — самая частая причина 404 при в остальном верной настройке." },
        { type: "p", text: "Один аргумент стоит задать явно — max_tokens. По умолчанию ChatAnthropic ограничивает ответ 1024 токенами и молча обрезает длинные ответы; увеличьте лимит для цепочек суммаризации или генерации кода. Параметры семплирования вроде temperature и top_p проходят без изменений, как и системные промпты и стоп-последовательности." },
        { type: "note", text: "Новые аккаунты, созданные через Google или GitHub, получают приветственный бонус $5 на баланс платформы — он действует на поддерживаемых моделях Claude, GPT, Gemini и Kimi; аккаунты через email/пароль бонус не получают." },
      ] },
      { h2: "Задайте один раз через переменные окружения", blocks: [
        { type: "p", text: "Если кодовую базу вы делите с теми, кто сидит на официальном эндпоинте, — или работаете в ноутбуках, где править исходники неудобно, — обойдитесь вообще без аргументов конструктора. ChatAnthropic читает оба значения из окружения, так что зачекиненный проект не требует ни одной правки кода." },
        { type: "code", code: `export ANTHROPIC_API_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••` },
        { type: "steps", items: [
          "Установите пакет интеграции: pip install -U langchain-anthropic. Поддержку Anthropic LangChain поставляет именно там, а не в langchain-core.",
          "Сгенерируйте ключ в дашборде apiToken.sale — он начинается с sk-pool- и работает с поддерживаемыми моделями Claude, GPT, Gemini и Kimi.",
          "Экспортируйте ANTHROPIC_API_URL и ANTHROPIC_API_KEY, как показано выше (или положите их в .env-файл, который подхватывает ваш раннер).",
          "Создайте ChatAnthropic(model=\"claude-sonnet-5\") без других аргументов и выполните один invoke(), чтобы убедиться в нормальном ответе.",
        ] },
        { type: "p", text: "Явные аргументы конструктора побеждают переменные окружения, поэтому локальный оверрайд никогда не протечёт в общую конфигурацию. Подход через env также держит ключ вне истории git — обращайтесь со sk-pool-… как с любым секретом: .env не коммитим, а в CI значение приходит из хранилища секретов." },
      ] },
      { h2: "Стриминг, вызов инструментов и LangGraph остаются на месте", blocks: [
        { type: "p", text: "Шлюз отдаёт стандартный Anthropic Messages API, и LangChain общается с ним через официальный клиент. Всё, что построено на этом протоколе, — SSE-стриминг, блоки tool use, структурированный вывод — ведёт себя ровно так же, как против api.anthropic.com. Сюда входят и with_structured_output(), который LangChain реализует поверх вызова инструментов, и .astream_events() для потокенных колбэков в асинхронных приложениях." },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\nfrom langchain_core.tools import tool\n\n@tool\ndef get_weather(city: str) -> str:\n    """Return the current weather for a city."""\n    return f"Sunny in {city}"\n\nllm = ChatAnthropic(model="claude-sonnet-5")  # env vars supply URL and key\nllm_with_tools = llm.bind_tools([get_weather])\n\nfor chunk in llm_with_tools.stream("What is the weather in Paris?"):\n    print(chunk.content, end="")` },
        { type: "p", text: "LangGraph-агенты наследуют ту же настройку: узел графа просто вызывает чат-модель. Направьте модель на роутер один раз — и каждый агент, супервизор и субграф, построенные на ней, последуют за ней. Никакой LangGraph-специфичной конфигурации переделывать не нужно." },
        { type: "note", text: "Учёт токенов тоже продолжает работать: каждый AIMessage по-прежнему несёт usage_metadata с количеством входных и выходных токенов, потому что шлюз возвращает стандартный объект usage от Anthropic. Трейсы LangSmith и кастомные колбэки, читающие usage_metadata, правок не требуют." },
      ] },
      { h2: "Что меняется, а что нет", blocks: [
        { type: "p", text: "Перед миграцией продакшн-приложения полезно увидеть всю дельту в одном месте. Коротко: ваш код, ваши модели и ваши возможности LangChain остаются на месте — движущихся частей только три: эндпоинт, ключ и цена за токен." },
        { type: "table", headers: ["Вопрос", "Через apiToken.sale"], rows: [
          ["ID моделей", "Без изменений — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 и остальной каталог"],
          ["Протокол", "Без изменений — Anthropic Messages API через официальный клиент"],
          ["Стриминг и вызов инструментов", "Без изменений — SSE-чанки и блоки tool use как обычно"],
          ["Цепочки, агенты, LangGraph", "Без изменений — правок кода, кроме URL и ключа, нет"],
          ["Цена за токен", "На 50% меньше на тех же моделях"],
          ["API-ключ", "Один ключ sk-pool-… для поддерживаемых моделей Claude, GPT, Gemini и Kimi"],
          ["Биллинг", "Предоплаченный баланс с детализацией расхода и токенов по каждому ключу в дашборде"],
        ] },
        { type: "link", text: "Посмотрите полный список поддерживаемых моделей Claude и цены", href: "/models" },
      ] },
      { h2: "Выберите правильную модель Claude для каждого узла", blocks: [
        { type: "p", text: "Раз смена модели — это правка одного аргумента, воспринимайте выбор модели как решение по узлам, а не глобальное. Цепочке-роутеру, которая классифицирует намерение, не нужен тот же уровень, что узлу, пишущему финальный ответ." },
        { type: "list", items: [
          "claude-haiku-4-5 — быстрый и недорогой уровень: классификация, маршрутизация, извлечение и другие высокообъёмные шаги.",
          "claude-sonnet-5 — сбалансированный дефолт для большинства продакшн-цепочек, RAG-пайплайнов и кодовых агентов.",
          "claude-opus-4-8 — верхний уровень рассуждений; приберегите его для сложного анализа, длинных документов и шагов планирования агентов.",
        ] },
        { type: "code", code: `from langchain_anthropic import ChatAnthropic\n\nfast = ChatAnthropic(model="claude-haiku-4-5")      # routing, extraction\nbalanced = ChatAnthropic(model="claude-sonnet-5")   # default nodes\ndeep = ChatAnthropic(model="claude-opus-4-8")       # planning, hard analysis\n\nrouter_chain = router_prompt | fast\nanswer_chain = answer_prompt | balanced | StrOutputParser()` },
        { type: "p", text: "Все три экземпляра делят один URL и ключ из окружения, и каждый вызов списывается с единого предоплаченного баланса. Это делает эксперименты с уровнями дешёвыми: поменяйте строку модели, прогоните оценочный набор, оставьте победителя." },
        { type: "note", text: "Прототипируйте на Sonnet, затем понижайте простые узлы до Haiku и повышайте до Opus только сложные. При предоплатном помтокенном биллинге смешанная цепочка стоит заметно дешевле, чем весь прогон на флагмане." },
        { type: "link", text: "Оцените цепочку со смешанными моделями в калькуляторе стоимости", href: "/tools/claude-api-cost-calculator" },
      ] },
      { h2: "Диагностика подключения", blocks: [
        { type: "p", text: "Меняются только эндпоинт и ключ, поэтому почти любой сбой — одна из трёх ошибок конфигурации, а не проблема LangChain. Пройдите их по порядку, прежде чем трогать код цепочек." },
        { type: "list", items: [
          "401 Unauthorized — ключ отсутствует или введён с опечаткой, либо переменная окружения не дошла до процесса. Распечатайте os.environ в том же интерпретаторе, чтобы убедиться, и помните: аргументы конструктора перекрывают env.",
          "404 Not Found — в URL затесался лишний /v1 или завершающий путь. Используйте чистый корень роутера https://router.apitoken.sale.",
          "Model not found — сверьте ID с каталогом на /models; здесь используются те же ID, что публикует Anthropic.",
        ] },
        { type: "p", text: "Если непонятно, виноват шлюз или ваша цепочка, верните URL на официальный эндпоинт на один прогон. Одинаковое поведение означает баг в цепочке; различие сужает поиск до конфигурации." },
        { type: "note", text: "Для временных 429 или 5xx не нужна собственная логика: ChatAnthropic по умолчанию повторяет запрос дважды с backoff'ом (настраивается через max_retries). Долгоживущим агентам всё же стоит задавать явный таймаут в секундах, а не полагаться на дефолт клиента." },
      ] },
    ],
    faq: [
      { q: "Работает ли LangChain с кастомным эндпоинтом Claude API?", a: "Да. ChatAnthropic принимает anthropic_api_url (или переменную окружения ANTHROPIC_API_URL), поэтому его можно направить на https://router.apitoken.sale и оставить всё остальное — пакет, ID моделей, код цепочек — без изменений." },
      { q: "Как задать базовый URL Anthropic для LangChain, не меняя код?", a: "Экспортируйте ANTHROPIC_API_URL=https://router.apitoken.sale и ANTHROPIC_API_KEY=sk-pool-… перед запуском скрипта. ChatAnthropic подхватывает оба автоматически, поэтому общие репозитории не требуют правок вообще." },
      { q: "Продолжают ли работать стриминг и вызов инструментов через apiToken.sale?", a: "Да. Шлюз отдаёт стандартный Anthropic Messages API, поэтому .stream(), bind_tools(), структурированный вывод и LangGraph-агенты ведут себя ровно как с официальным эндпоинтом." },
      { q: "Какие модели Claude можно вызывать из LangChain?", a: "Все поддерживаемые модели Claude — claude-opus-4-8, claude-sonnet-5, claude-haiku-4-5 и другие — на одном ключе и предоплаченном балансе, на 50% дешевле за токен." },
      { q: "Можно ли использовать ChatOpenAI вместо ChatAnthropic для Claude?", a: "Да. Роутер также предоставляет OpenAI-совместимую линию на https://router.apitoken.sale/v1, так что ChatOpenAI(base_url=\"https://router.apitoken.sale/v1\", api_key=\"sk-pool-•••\") достаёт те же модели Claude тем же ключом — удобно, когда фреймворк говорит только по протоколу OpenAI." },
      { q: "Нужен ли отдельный ключ для GPT, Gemini или Kimi в LangChain?", a: "Нет. Тот же ключ sk-pool-… работает с поддерживаемыми моделями Claude, GPT, Gemini и Kimi, поэтому мультипровайдерное приложение на LangChain может делить один ключ и один предоплаченный баланс." },
    ],
  };
