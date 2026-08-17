import type { LocalizedContent } from "../learn";
import { BASE, OPENAI_BASE, KEY } from "../learn-shared";

export const content: LocalizedContent = {
    title: "Claude API через LiteLLM",
    h1: "Используйте Claude API через LiteLLM",
    description: "Используйте Claude API через LiteLLM с apiToken.sale: оставьте префикс anthropic/, задайте api_base на router.apitoken.sale в litellm.completion() или конфиге прокси и платите за токены на 50% меньше.",
    keywords: ["claude api litellm", "litellm anthropic", "litellm claude", "litellm api_base anthropic", "litellm proxy claude", "litellm claude api key", "litellm anthropic base url", "litellm custom anthropic endpoint", "claude api через litellm прокси", "дешёвый claude api litellm"],
    dek: "Подключение Claude API через LiteLLM к apiToken.sale сводится к одному параметру: LiteLLM нативно говорит протокол Anthropic Messages, поэтому вы оставляете префикс anthropic/ у модели и переопределяете только api_base. Тот же формат запросов и ответов, но на 50% дешевле за токен — и при вызове litellm.completion() из скрипта, и когда LiteLLM-прокси стоит перед всем вашим стеком.",
    sections: [
      { h2: "Направьте litellm.completion() на эндпоинт со скидкой", blocks: [
        { type: "p", text: "LiteLLM уже реализует Anthropic Messages API, поэтому маршрутизация Claude через apiToken.sale — это один дополнительный аргумент: оставьте префикс anthropic/ у модели, задайте api_base на шлюз и передайте предоплаченный ключ. Запросы и ответы сохраняют стандартный формат Anthropic — меняются только эндпоинт и цена за токен: расходы на Claude фиксированно на 50% ниже прайса." },
        { type: "code", code: `import litellm\n\nresponse = litellm.completion(\n    model="anthropic/claude-opus-4-8",\n    api_base="${BASE}",\n    api_key="${KEY}",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n    stream=True,\n)\nfor chunk in response:\n    print(chunk.choices[0].delta.content or "", end="")` },
        { type: "p", text: "Здесь работают три вещи. Префикс anthropic/ выбирает Anthropic-провайдера LiteLLM, поэтому max_tokens, temperature, tools и стриминг мапятся на Messages API ровно так же, как в апстриме, — а max_tokens в этом API обязателен, так что задавайте его явно, а не полагайтесь на дефолты. api_base переопределяет, куда уходят запросы, для каждого вызова. А api_key — это ваш ключ шлюза: один и тот же sk-pool-… работает со всеми поддерживаемыми моделями Claude, поэтому переход между claude-opus-4-8, claude-sonnet-5 и claude-haiku-4-5 — это смена строки, а не новая интеграция." },
        { type: "note", text: "На практике кусаются две ловушки. Никогда не убирайте префикс anthropic/: голый claude-opus-4-8 заставляет LiteLLM угадывать провайдера, и при неверной догадке уйдёт не тот протокол или отклонится ключ. И читайте ключ из окружения (api_key=os.environ[\"APITOKEN_KEY\"]), а не вставляйте его в ноутбуки и конфиги, которые окажутся в git." },
      ] },
      { h2: "Один LiteLLM-прокси для всех сервисов, которым нужен Claude", blocks: [
        { type: "p", text: "Прямые вызовы нормальны для одиночного скрипта. Когда Claude нужен нескольким сервисам, ноутбукам и кодинг-агентам, запустите LiteLLM как прокси: один YAML-файл хранит эндпоинт и ключ, каждый клиент общается с прокси через OpenAI-совместимый интерфейс LiteLLM, а апстрим-трафик остаётся на протоколе Anthropic." },
        { type: "code", code: `# config.yaml\nmodel_list:\n  - model_name: claude-opus-4-8\n    litellm_params:\n      model: anthropic/claude-opus-4-8\n      api_base: ${BASE}\n      api_key: ${KEY}\n  - model_name: claude-haiku-4-5\n    litellm_params:\n      model: anthropic/claude-haiku-4-5\n      api_base: ${BASE}\n      api_key: ${KEY}\nrouter_settings:\n  fallbacks:\n    - claude-opus-4-8:\n        - claude-haiku-4-5` },
        { type: "steps", items: [
          `Установите proxy-экстра и сохраните YAML выше как config.yaml: pip install "litellm[proxy]".`,
          "Запустите шлюз: litellm --config config.yaml --port 4000.",
          `Направьте любой OpenAI-совместимый клиент на http://localhost:4000 с model="claude-opus-4-8" — прокси превратит вызов в запрос Anthropic Messages на ${BASE}.`,
          "Следите за расходом в панели apiToken.sale: использование записывается по каждому ключу с детализацией до токенов, так что один ключ прокси даёт единую строку расходов по всем сервисам за ним.",
        ] },
        { type: "p", text: "Блок router_settings оправдывает свои две строки: если claude-opus-4-8 падает с ошибкой или недоступен, LiteLLM повторяет запрос на claude-haiku-4-5, а не отдаёт сбой клиенту. Для долгоживущих агентов, которые держат сессию часами, этот фолбэк — разница между тихим ретраем и мёртвым процессом." },
      ] },
      { h2: "Стриминг, вызов инструментов и промпт-кеширование переживают переход", blocks: [
        { type: "p", text: "Возможности, которые обычно ломаются за транслирующим слоем, здесь продолжают работать: шлюз отдаёт нативный Anthropic Messages API, а не перекодирует ваш трафик в другой протокол. Всё, что LiteLLM умеет выразить в терминах Anthropic, доезжает до модели без изменений." },
        { type: "list", items: [
          "Стриминг: stream=True отдаёт те же инкрементальные server-sent events, так что потокенные UI и агенты ведут себя идентично.",
          "Вызов инструментов: tools, tool_choice и round-trip с tool_result мапятся на стандартные блоки Messages — агентам с function calling не нужны доработки.",
          "Промпт-кеширование: брейкпоинты cache_control работают, как описано в апстрим-документации, а чтения из кеша тарифицируются по кеш-ставкам со страниц моделей.",
        ] },
        { type: "p", text: "Это важнее всего для инструментов, построенных поверх LiteLLM, а не для самого LiteLLM: многие кодинг-агенты и фреймворки гоняют свой Anthropic-трафик через него и наследуют эндпоинт со скидкой из той же конфигурации — без правок собственного кода." },
      ] },
      { h2: "Добавьте GPT, Gemini и Kimi в тот же model_list", blocks: [
        { type: "p", text: "Ключ шлюза мультипровайдерный, поэтому настроенный вами прокси — не только для Claude. Добавьте по записи на каждую полосу провайдера — и все модели будут тратить один и тот же предоплаченный баланс: ни второго аккаунта, ни второго ключа для ротации." },
        { type: "code", code: `# additional model_list entries\n  - model_name: gpt-5.6-terra\n    litellm_params:\n      model: openai/gpt-5.6-terra        # OpenAI-compatible lane\n      api_base: ${OPENAI_BASE}\n      api_key: ${KEY}\n  - model_name: gemini-3.6-flash\n    litellm_params:\n      model: gemini/gemini-3.6-flash     # native Gemini lane\n      api_base: ${BASE}\n      api_key: ${KEY}` },
        { type: "p", text: "Модели Kimi ездят по тем же двум полосам — Anthropic Messages или универсальный OpenAI-совместимый эндпоинт, — так что одно развёртывание LiteLLM может обслуживать поддерживаемые модели Claude, GPT, Gemini и Kimi одновременно. Каждый провайдер сохраняет протокол, который LiteLLM для него уже говорит; новым здесь оказываются только base URL и ключ." },
      ] },
      { h2: "Что меняется при переходе — и что остаётся идентичным", blocks: [
        { type: "p", text: "Смена эндпоинта намеренно скучна, и стоит точно обозначить, какие части стека это замечают, а какие — нет." },
        { type: "table", headers: ["Слой", "Что вы задаёте", "Что происходит"], rows: [
          ["ID моделей", "anthropic/claude-opus-4-8, anthropic/claude-sonnet-5, anthropic/claude-haiku-4-5", "Те же ID, что в апстриме; префикс выбирает протокол Anthropic"],
          ["Эндпоинт", BASE, "Нативный Anthropic Messages API, а не трансляция в формат OpenAI"],
          ["Возможности", "Стриминг, вызов инструментов, промпт-кеширование", "Ведут себя так же, как с официальным эндпоинтом"],
          ["Цена", "На 50% ниже прайса за токен", "Действует для каждой поддерживаемой модели Claude на том же предоплаченном балансе"],
          ["Учёт", "Один ключ sk-pool-…", "Расход по ключу с детализацией до токенов в панели"],
        ] },
      ] },
      { h2: "Спланируйте бюджет трафика до масштабирования", blocks: [
        { type: "p", text: "Биллинг предоплаченный: вы пополняете баланс, и каждый запрос списывает свою точную стоимость в токенах — модели Claude по ставке со скидкой. Никакого месячного обязательства, которое нужно оценивать заранее, поэтому помодельный трекинг расходов в LiteLLM — приятное дополнение, а не инструмент выживания: авторитетные цифры живут в панели apiToken.sale с разбивкой по ключам и детализацией до токенов." },
        { type: "p", text: "Прежде чем направлять весь флот на прокси, прогоните через один ключ репрезентативный день трафика и считайте реальное потребление с панели; экстраполируйте от реальных токенов, а не от арифметики прайс-листа. Калькулятор стоимости по ссылке ниже делает ту же математику заранее, если вы знаете примерный состав запросов." },
        { type: "note", text: "Новые аккаунты, созданные через Google или GitHub, начинают с бонуса $5 на балансе платформы — он действует на поддерживаемые модели Claude, GPT, Gemini и Kimi; аккаунты с email и паролем бонус не получают." },
        { type: "link", text: "Цены по моделям, включая кеш-ставки", href: "/models" },
        { type: "link", text: "Оцените стоимость вашего LiteLLM-трафика в бесплатном калькуляторе", href: "/tools/claude-api-cost-calculator" },
      ] },
    ],
    faq: [
      { q: "Как задать кастомный base URL для Anthropic в LiteLLM?", a: "Передайте api_base напрямую в litellm.completion() или задайте его в litellm_params в model_list прокси. LiteLLM будет отправлять запросы в формате Anthropic Messages на этот эндпоинт — для apiToken.sale это https://router.apitoken.sale." },
      { q: "Нужно ли сохранять префикс anthropic/ у модели при маршрутизации Claude через шлюз?", a: "Да. Используйте anthropic/claude-opus-4-8 (или любую поддерживаемую модель), чтобы LiteLLM применил протокол Anthropic; меняются только эндпоинт и ключ, а без префикса LiteLLM начнёт угадывать провайдера." },
      { q: "Работает ли стриминг LiteLLM с кастомным api_base?", a: "Да. stream=True возвращает те же инкрементальные события Anthropic через шлюз, так что потокенный рендеринг и агентские циклы ведут себя ровно как с официальным эндпоинтом." },
      { q: "Может ли один LiteLLM-прокси обслуживать Claude, GPT и Gemini одновременно?", a: "Да. Один ключ apiToken.sale покрывает поддерживаемые модели Claude, GPT, Gemini и Kimi; добавьте каждого провайдера отдельной записью в model_list — модели anthropic/ и gemini/ на https://router.apitoken.sale, модели openai/ на https://router.apitoken.sale/v1." },
      { q: "Как настроить фолбэк между моделями Claude в LiteLLM?", a: "Используйте router_settings.fallbacks в конфиге прокси, связав основное развёртывание с резервным — например, claude-opus-4-8 с claude-haiku-4-5. Обе записи указывают на тот же шлюз и ключ, так что ретрай остаётся на балансе со скидкой." },
    ],
  };
