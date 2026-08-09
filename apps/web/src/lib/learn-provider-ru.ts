import { learnProviderEn } from "./learn-provider-en";
import type { LearnBlock, LocalizedContent } from "./learn";

function sourceBlock(slug: string, sectionIndex: number, blockIndex: number): LearnBlock {
  const article = learnProviderEn.find((entry) => entry.slug === slug);
  if (!article) throw new Error("Unknown provider guide: " + slug);
  const block = article.sections[sectionIndex]?.blocks[blockIndex];
  if (!block) throw new Error("Missing provider guide block: " + slug + "/" + sectionIndex + "/" + blockIndex);
  return block;
}

export const learnProviderRu: Record<string, LocalizedContent> = {
  "how-to-buy-gpt-api-key": {
    title: "Как купить API-ключ GPT",
    h1: "Как купить API-ключ GPT",
    description: "Купите API-ключ GPT с предоплаченным балансом, оплатой картой или криптовалютой и OpenAI-совместимым endpoint для GPT-5.6, GPT-5.5 и GPT Image 2 со скидкой 50%.",
    keywords: ["купить api ключ gpt", "api ключ gpt", "купить ключ openai api", "gpt-5.6 api", "openai совместимый api", "gpt api с предоплатой"],
    dek: "Один ключ apiToken.sale открывает каталог GPT без отдельного аккаунта OpenAI Platform. Пополните баланс, укажите OpenAI-совместимый endpoint и платите на 50% меньше официальной стоимости каждого запроса.",
    sections: [
      { h2: "Получите ключ GPT за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите ключ в дашборде.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой — без пакетов и ежемесячных обязательств.",
          "Укажите base URL https://router.apitoken.sale/v1, используйте Authorization: Bearer и выберите модель из GET /v1/models.",
        ] },
        sourceBlock("how-to-buy-gpt-api-key", 0, 1),
      ] },
      { h2: "Что входит в доступ", blocks: [
        { type: "list", items: [
          "Responses и Chat Completions с инкрементальным SSE-стримингом.",
          "GPT-5.6 Sol, Terra и Luna, предыдущие GPT и отдельные маршруты GPT Image 2.",
          "Тот же ключ и баланс работают с поддерживаемыми Claude, Gemini и Kimi.",
          "Плоская B2C-скидка 50% от официальной стоимости каждого запроса.",
        ] },
        { type: "note", text: "Храните ключ в серверной переменной окружения. GPT использует Authorization: Bearer; x-api-key и x-goog-api-key относятся к протоколам Anthropic и Gemini." },
      ] },
    ],
    faq: [
      { q: "Нужен ли аккаунт OpenAI?", a: "Нет. Ключ, баланс и биллинг находятся в apiToken.sale; клиенту нужен только custom base URL и Bearer-ключ." },
      { q: "Один ключ работает с GPT и Claude?", a: "Да. Один sk-pool ключ и баланс покрывают всех поддерживаемых провайдеров; меняются только endpoint и заголовок авторизации." },
      { q: "Это OpenAI Platform?", a: "Нет. Это независимый OpenAI-совместимый шлюз со своим аккаунтом, предоплаченным балансом и каталогом моделей." },
    ],
  },
  "gpt-api-pricing": {
    title: "Цены GPT API: как считается стоимость",
    h1: "Цены GPT API: input, кэш, output и длинный контекст",
    description: "Разбор цен GPT-5.6 Sol, Terra и Luna: входные, кэшированные и выходные токены, cache write, long context и плоская скидка apiToken.sale 50%.",
    keywords: ["цены gpt api", "цена gpt-5.6", "стоимость gpt api", "цена токенов gpt", "gpt-5.6 sol цена", "дешевый gpt api"],
    dek: "Стоимость GPT — сумма точных токенных компонентов, а не цена запроса. На официальный расход влияют tier модели, кэш и длина input; затем apiToken.sale вычитает 50%.",
    sections: [
      { h2: "Текущие тарифы GPT-5.6", blocks: [
        { type: "table", headers: ["Модель", "Официально: input / cache / output", "После скидки 50%"], rows: [
          ["gpt-5.6-sol", "$5 / $0.50 / $30", "$2.50 / $0.25 / $15"],
          ["gpt-5.6-terra", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gpt-5.6-luna", "$0.20 / $0.02 / $1.20", "$0.10 / $0.01 / $0.60"],
        ] },
        { type: "p", text: "Все ставки указаны за 1 млн токенов. gpt-5.6 — alias модели gpt-5.6-sol с тем же тарифом, а не отдельная ценовая позиция." },
      ] },
      { h2: "Cache write и long context", blocks: [
        { type: "list", items: [
          "Запись кэша GPT-5.6 стоит 125% обычного input, а чтение кэша — 10%.",
          "После 272K входных токенов весь запрос получает множители 2× для input и 1,5× для output.",
          "Reasoning-токены входят в output и не тарифицируются повторно отдельной строкой.",
          "Дашборд сохраняет фактический usage и точное списание после скидки.",
        ] },
        { type: "note", text: "Переход на более дешёвый tier часто экономит больше сокращения промпта: Terra стоит 40% цены Sol, а Luna — 4%. Маршрутизируйте задачи по сложности." },
      ] },
    ],
    faq: [
      { q: "Сколько стоит GPT-5.6 за 1 млн токенов?", a: "Официально Sol стоит $5 input/$30 output, Terra — $2/$12, Luna — $0.20/$1.20. apiToken.sale применяет скидку 50% к каждому компоненту." },
      { q: "Что считается cached input?", a: "Повторяющиеся префиксы промпта, которые провайдер отдал из кэша. Один токен не оплачивается одновременно как cached и fresh input." },
      { q: "Когда включается long-context тариф?", a: "Когда input превышает 272K токенов. Весь запрос получает 2× input и 1,5× output до применения скидки." },
    ],
  },
  "gpt-5-6-sol-vs-terra-vs-luna": {
    title: "GPT-5.6 Sol, Terra и Luna: сравнение",
    h1: "Сравнение GPT-5.6 Sol, Terra и Luna",
    description: "Сравните GPT-5.6 Sol, Terra и Luna по цене, reasoning effort, контексту и задачам, чтобы выбрать GPT-модель для кода и production.",
    keywords: ["gpt-5.6 sol или terra", "gpt-5.6 terra или luna", "лучшая модель gpt-5.6", "модели gpt-5.6", "сравнение gpt-5.6", "gpt для программирования"],
    dek: "У семейства GPT-5.6 общий контекст 400K, output до 128K и полный диапазон reasoning effort. Практическая разница — сколько качества и скорости вы покупаете за токен.",
    sections: [
      { h2: "Выбор по задаче", blocks: [
        { type: "table", headers: ["Tier", "Для чего подходит", "Официально input / output"], rows: [
          ["Sol", "Сложный reasoning, долгие агенты, трудный code review", "$5 / $30"],
          ["Terra", "Повседневный код, production-чат, сбалансированные агенты", "$2 / $12"],
          ["Luna", "Классификация, извлечение, роутинг и простые массовые задачи", "$0.20 / $1.20"],
        ] },
        { type: "p", text: "Terra — безопасный default: те же controls и context, что у Sol, за 40% цены. Переходите на Sol, когда eval показывает разницу в качестве, а предсказуемый bulk отправляйте в Luna." },
      ] },
      { h2: "Что у моделей одинаковое", blocks: [
        { type: "list", items: [
          "Контекст 400K и output до 128K.",
          "Текст и изображения на входе, текст на выходе.",
          "Responses и Chat Completions с SSE-стримингом.",
          "Reasoning effort от none до max в линейке GPT-5.6.",
          "Один endpoint, ключ и баланс для переключения модели по задаче.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая GPT-5.6 лучше для программирования?", a: "Начните с Terra. Sol используйте для самой сложной архитектуры и агентов, Luna — для дешёвых детерминированных подзадач." },
      { q: "Нужны разные endpoints для Sol, Terra и Luna?", a: "Нет. Все три работают через один OpenAI-совместимый base URL и ключ; меняется только model ID." },
      { q: "Terra поддерживает max reasoning effort?", a: "Да. У Sol, Terra и Luna один диапазон GPT-5.6, включая max." },
    ],
  },
  "gpt-image-2-api-guide": {
    title: "GPT Image 2 API: генерация и редактирование",
    h1: "Генерация и редактирование изображений через GPT Image 2 API",
    description: "Используйте GPT Image 2 для генерации и редактирования изображений: endpoints, model ID, лимит референсов, токенные цены и скидка apiToken.sale 50%.",
    keywords: ["gpt image 2 api", "gpt-image-2", "api генерации изображений openai", "gpt image редактирование", "цена gpt image", "image generation api"],
    dek: "GPT Image 2 использует отдельные image routes, но тот же ключ и баланс, что GPT для текста. Создавайте изображения по промпту или редактируйте до пяти PNG-референсов без отдельного тарифа.",
    sections: [
      { h2: "Запрос на генерацию", blocks: [
        sourceBlock("gpt-image-2-api-guide", 0, 0),
        { type: "p", text: "Для редактирования отправьте multipart/form-data на /v1/images/edits с той же моделью и максимум пятью PNG. Текущая поверхность возвращает один PNG без стриминга." },
      ] },
      { h2: "Как считается стоимость изображения", blocks: [
        { type: "table", headers: ["Компонент", "Официально за 1 млн", "Цена здесь"], rows: [
          ["Текстовый input", "$5", "$2.50"],
          ["Image input", "$8", "$4"],
          ["Image output", "$30", "$15"],
        ] },
        { type: "list", items: [
          "Кэшированный текстовый и image input стоит 25% обычного тарифа.",
          "gpt-image-2 — alias immutable-снимка gpt-image-2-2026-04-21.",
          "Image usage списывается с того же баланса, что запросы GPT, Claude и Gemini.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какой endpoint использует GPT Image 2?", a: "POST /v1/images/generations для нового изображения и POST /v1/images/edits для редактирования на OpenAI-совместимом base URL." },
      { q: "GPT Image 2 умеет редактировать изображение?", a: "Да. Маршрут edits принимает до пяти PNG-референсов в multipart/form-data." },
      { q: "Нужны отдельный ключ и баланс?", a: "Нет. Используются тот же Bearer-ключ и предоплаченный баланс, что для остальных моделей." },
    ],
  },
  "how-to-buy-gemini-api-key": {
    title: "Как купить API-ключ Gemini",
    h1: "Как купить API-ключ Gemini",
    description: "Купите API-ключ Gemini с предоплаченным балансом, оплатой картой или криптовалютой, нативными Gemini endpoints и одним аккаунтом для Gemini, GPT, Claude и Kimi со скидкой 50%.",
    keywords: ["купить api ключ gemini", "api ключ gemini", "google gemini api", "gemini api с предоплатой", "оплата gemini api", "дешевый gemini api"],
    dek: "Ключ apiToken.sale даёт доступ к нативному Gemini API без отдельного Google Cloud billing. Один раз пополните баланс, передавайте ключ как x-goog-api-key и используйте его со всеми поддерживаемыми провайдерами.",
    sections: [
      { h2: "Получите ключ Gemini за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите sk-pool ключ в дашборде.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой; баланс не сгорает.",
          "Укажите Gemini base URL https://router.apitoken.sale, отправляйте x-goog-api-key и выберите модель из GET /v1beta/models.",
        ] },
        sourceBlock("how-to-buy-gemini-api-key", 0, 1),
      ] },
      { h2: "Какие возможности доступны", blocks: [
        { type: "list", items: [
          "Текстовые Pro, Flash и Flash-Lite через нативный протокол Gemini.",
          "Gemini 3.1 Flash Image (Nano Banana 2) для генерации изображений.",
          "generateContent, streamGenerateContent и countTokens с Google-совместимыми схемами.",
          "Плоская B2C-скидка 50% и тот же ключ/баланс для GPT, Claude и Kimi.",
        ] },
        { type: "note", text: "В Google SDK указывайте голый host. SDK сам добавляет /v1beta; двойной префикс приводит к 404." },
      ] },
    ],
    faq: [
      { q: "Нужен Google Cloud project?", a: "Нет. Gateway-аккаунтом и биллингом управляет apiToken.sale; клиенту нужны только custom base URL и sk-pool ключ." },
      { q: "Какой заголовок авторизует Gemini?", a: "x-goog-api-key. Не используйте Anthropic x-api-key или OpenAI Authorization: Bearer на нативных Gemini routes." },
      { q: "Один ключ может вызывать GPT и Gemini?", a: "Да. Ключ и баланс общие; для каждого провайдера меняются endpoint, протокол и model ID." },
    ],
  },
  "gemini-api-quickstart": {
    title: "Gemini API Quickstart",
    h1: "Быстрый старт Gemini API: curl и Google GenAI SDK",
    description: "Первый запрос к Gemini через apiToken.sale с curl или Google GenAI SDK: нативный generateContent, x-goog-api-key и явный model ID.",
    keywords: ["gemini api quickstart", "инструкция gemini api", "google genai sdk base url", "gemini generatecontent", "gemini api curl", "пример gemini api"],
    dek: "Шлюз сохраняет нативный протокол Google Gemini. Замените base URL и API key, оставьте схемы generateContent и официального SDK и всегда выбирайте модель явно.",
    sections: [
      { h2: "Первый запрос через curl", blocks: [
        sourceBlock("gemini-api-quickstart", 0, 0),
        { type: "p", text: "Для инкрементального ответа вызовите streamGenerateContent?alt=sse. На том же model path доступен countTokens для бесплатной оценки input до генерации." },
      ] },
      { h2: "Официальный Python SDK", blocks: [
        sourceBlock("gemini-api-quickstart", 1, 0),
        { type: "list", items: [
          "Передавайте только голый base URL, без /v1beta в конфигурации SDK.",
          "Всегда задавайте конкретный model ID: автоматического default клиента может не быть в каталоге gateway.",
          "Храните APITOKEN_API_KEY в переменной окружения, а не в исходном коде.",
        ] },
      ] },
    ],
    faq: [
      { q: "Работает официальный Google GenAI SDK?", a: "Да. Укажите HttpOptions(base_url) как https://router.apitoken.sale и передайте ключ apiToken.sale; формы запросов и ответов остаются нативными." },
      { q: "Как стримить ответ Gemini?", a: "Используйте /v1beta/models/{model}:streamGenerateContent?alt=sse с x-goog-api-key или соответствующий streaming-метод SDK." },
      { q: "Почему двойной /v1beta даёт 404?", a: "Google SDK добавляет версию API сам. Укажите только голый host, чтобы в итоговом URL был один /v1beta." },
    ],
  },
  "gemini-api-pricing": {
    title: "Цены Gemini API: как считается стоимость",
    h1: "Цены Gemini API: Pro, Flash, Flash-Lite и изображения",
    description: "Сравните цены Gemini Pro, Flash, Flash-Lite и Nano Banana 2: cached input, long context, image output и плоская скидка apiToken.sale 50%.",
    keywords: ["цены gemini api", "стоимость gemini api", "цена токенов gemini", "цена gemini flash", "цена gemini pro", "дешевый gemini api"],
    dek: "Стоимость Gemini зависит от tier модели, кэшированного input, типа output и — для Pro — длины контекста. Gateway рассчитывает официальные компоненты точно и применяет скидку 50%.",
    sections: [
      { h2: "Тарифы основных текстовых моделей", blocks: [
        { type: "table", headers: ["Модель", "Официально: input / cache / output", "После скидки 50%"], rows: [
          ["gemini-3.1-pro-preview", "$2 / $0.20 / $12", "$1 / $0.10 / $6"],
          ["gemini-3.6-flash", "$1.50 / $0.15 / $7.50", "$0.75 / $0.075 / $3.75"],
          ["gemini-3.1-flash-lite", "$0.25 / $0.025 / $1.50", "$0.125 / $0.0125 / $0.75"],
          ["gemini-2.5-flash-lite", "$0.10 / $0.01 / $0.40", "$0.05 / $0.005 / $0.20"],
        ] },
        { type: "p", text: "Все значения указаны за 1 млн токенов. Cached input — самостоятельный usage-компонент провайдера; один токен не добавляется одновременно в fresh input." },
      ] },
      { h2: "Long context и изображения", blocks: [
        { type: "list", items: [
          "У Gemini 3.1 Pro Preview после 200K input весь запрос стоит $4 input и $18 output за 1 млн.",
          "Gemini 3.1 Flash Image тарифицирует текстовый output по $3, а image output — по $60 за 1 млн image-токенов.",
          "Cached input Flash Image стоит как обычный input: скидки текстовых моделей у него нет.",
          "B2C-скидка 50% применяется после точного расчёта официальных компонентов.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая модель Gemini самая дешёвая?", a: "Среди опубликованных текстовых tiers Gemini 2.5 Flash-Lite стоит официально $0.10 input/$0.40 output, здесь — $0.05/$0.20 после скидки." },
      { q: "Когда действует long-context тариф Gemini?", a: "Для Gemini 3.1 Pro Preview после 200K входных токенов. Повышенные ставки применяются ко всему запросу." },
      { q: "Как считается image output?", a: "Gemini 3.1 Flash Image стоит $60 за 1 млн image-output токенов официально и $30 после скидки 50%." },
    ],
  },
  "gemini-pro-vs-flash-vs-flash-lite": {
    title: "Gemini Pro, Flash и Flash-Lite: сравнение",
    h1: "Сравнение Gemini Pro, Flash и Flash-Lite",
    description: "Сравните Gemini Pro, Flash и Flash-Lite по цене, контексту, reasoning и задачам, чтобы выбрать модель для кода, агентов и массового API.",
    keywords: ["gemini pro или flash", "gemini flash или flash lite", "лучшая модель gemini", "сравнение моделей gemini", "gemini для программирования", "gemini 3.6 flash"],
    dek: "Выбирайте tier как маршрут: Pro — для самого сложного reasoning, Flash — coding default, Flash-Lite — дешёвые массовые шаги. Один ключ работает со всеми тремя.",
    sections: [
      { h2: "Выбор по задаче", blocks: [
        { type: "table", headers: ["Tier", "Для чего подходит", "Рекомендуемый ID"], rows: [
          ["Pro", "Сложный reasoning, планирование, глубокий анализ кода и документов", "gemini-3.1-pro-preview"],
          ["Flash", "Повседневный код, multimodal-агенты и production", "gemini-3.6-flash"],
          ["Flash-Lite", "Классификация, извлечение, роутинг и pre-processing", "gemini-3.1-flash-lite"],
          ["Image", "Генерация и редактирование изображений", "gemini-3.1-flash-image"],
        ] },
        { type: "p", text: "Gemini 3.6 Flash — лучший старт для новых текстовых задач. Только самые сложные запросы поднимайте до Pro, а предсказуемый bulk опускайте до Flash-Lite." },
      ] },
      { h2: "Компромисс контекста и цены", blocks: [
        { type: "list", items: [
          "Текущие текстовые модели дают контекст 1M и output до 64K.",
          "У Pro есть long-context premium после 200K input; Flash и Flash-Lite сохраняют плоские ставки.",
          "Cached input текстовых моделей обычно стоит 10% fresh input.",
          "Перед большими запросами используйте countTokens и маршрутизируйте по eval, а не по названию модели.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какую Gemini выбрать для программирования?", a: "Начните с Gemini 3.6 Flash. Сложную архитектуру и review отправляйте в 3.1 Pro Preview, дешёвые предсказуемые шаги — в Flash-Lite." },
      { q: "У Flash-Lite меньше контекст?", a: "Нет. Опубликованные text Flash-Lite сохраняют контекст 1M; их преимущество — цена и задержка на простых задачах." },
      { q: "Для смены tier нужен новый ключ?", a: "Нет. Оставьте тот же base URL и x-goog-api-key, измените только model ID." },
    ],
  },
  "nano-banana-2-api-guide": {
    title: "Nano Banana 2 API: инструкция",
    h1: "Генерация изображений через Nano Banana 2 API",
    description: "Используйте Gemini 3.1 Flash Image (Nano Banana 2) через нативный Gemini API: model ID, generateContent, цена image output и скидка 50%.",
    keywords: ["nano banana 2 api", "gemini 3.1 flash image api", "gemini генерация изображений", "nano banana api ключ", "цена gemini image", "google image api"],
    dek: "Nano Banana 2 — публичное имя Gemini 3.1 Flash Image. Модель работает через нативный generateContent, принимает multimodal input и возвращает изображения с того же баланса, что текстовые модели.",
    sections: [
      { h2: "Используйте точный model ID", blocks: [
        sourceBlock("nano-banana-2-api-guide", 0, 0),
        { type: "p", text: "Разбирайте response parts по MIME type: текстовые части содержат комментарий, image parts — сгенерированный файл. В API используйте gemini-3.1-flash-image, а не маркетинговое имя." },
      ] },
      { h2: "Лимиты и цены", blocks: [
        { type: "list", items: [
          "Контекст 128K и output до 32K — меньше, чем у текстовой Flash-линейки.",
          "Официально text input/output стоят $0.50/$3 за 1 млн, image output — $60.",
          "После скидки apiToken.sale это $0.25/$1.50 и $30 за image output.",
          "Cached input этой image-модели остаётся по полной ставке $0.50.",
        ] },
        { type: "note", text: "Для чисто текстового ответа выбирайте text Flash. Flash Image нужен, когда response должен содержать отрисованное изображение." },
      ] },
    ],
    faq: [
      { q: "Какой model ID у Nano Banana 2?", a: "gemini-3.1-flash-image на нативном маршруте Gemini generateContent." },
      { q: "Сколько стоит image output Nano Banana 2?", a: "$60 за 1 млн image-output токенов официально и $30 после скидки apiToken.sale 50%." },
      { q: "Нужен отдельный image API key?", a: "Нет. Используйте тот же sk-pool ключ в x-goog-api-key и общий баланс." },
    ],
  },
  "how-to-buy-kimi-api-key": {
    title: "Как купить API-ключ Kimi",
    h1: "Как купить API-ключ Kimi",
    description: "Купите предоплаченный API-ключ для Kimi K3 и Kimi for Coding, используйте Anthropic Messages или OpenAI-совместимые клиенты и платите на 50% меньше официальной стоимости.",
    keywords: ["купить api ключ kimi", "api ключ kimi", "kimi k3 api", "kimi for coding api", "moonshot kimi api", "kimi api с предоплатой"],
    dek: "Kimi доступен в собственном namespace на едином router. Используйте нативный Anthropic Messages route или OpenAI-совместимый клиент, а usage списывается с общего баланса Claude, GPT и Gemini.",
    sections: [
      { h2: "Получите доступ за три шага", blocks: [
        { type: "steps", items: [
          "Создайте аккаунт apiToken.sale и выпустите sk-pool ключ.",
          "Пополните баланс на любую целую сумму в долларах картой или криптовалютой — отдельный Kimi-план вам не нужен.",
          "Откройте GET https://router.apitoken.sale/v1/models и выберите kimi/* ID из живого каталога вашего ключа.",
        ] },
        sourceBlock("how-to-buy-kimi-api-key", 0, 1),
      ] },
      { h2: "Чем отличается маршрут Kimi", blocks: [
        { type: "list", items: [
          "Kimi — отдельный provider namespace, но не четвёртый wire format: используйте POST /v1/messages с x-api-key либо единый OpenAI-совместимый route /v1.",
          "Публичные IDs — aliases kimi/k3 и kimi/kimi-for-coding, а не внутренние тарифные названия.",
          "У K3 есть варианты контекста 256K и 1M, у Kimi for Coding — обычный и High Speed aliases.",
          "Ответ /v1/models — источник истины: доступность зависит от capacity провайдера и policy ключа.",
        ] },
      ] },
    ],
    faq: [
      { q: "Для Kimi нужен отдельный API-ключ?", a: "Нет. Тот же sk-pool ключ и баланс работают с Kimi и другими поддерживаемыми провайдерами." },
      { q: "Какой endpoint использует Kimi?", a: "Для Anthropic Messages — https://router.apitoken.sale/v1/messages; для OpenAI-совместимого клиента — Chat Completions на /v1. Оба принимают публичные kimi/* IDs." },
      { q: "Зачем сначала проверять /v1/models?", a: "Каталог scoped к ключу и показывает только модели, которые сейчас можно маршрутизировать и тарифицировать." },
    ],
  },
  "kimi-api-quickstart": {
    title: "Kimi API Quickstart",
    h1: "Быстрый старт Kimi API с Anthropic SDK",
    description: "Вызывайте Kimi K3 и Kimi for Coding через apiToken.sale с Anthropic Messages API, x-api-key, namespaced model IDs, streaming и общим балансом.",
    keywords: ["kimi api quickstart", "инструкция kimi api", "kimi anthropic api", "пример kimi k3 api", "kimi for coding api", "kimi api curl"],
    dek: "Kimi говорит на Anthropic Messages через единый router. Существующему Anthropic-клиенту нужны только custom base URL, ключ apiToken.sale и явный kimi/* model ID.",
    sections: [
      { h2: "Первый запрос через curl", blocks: [
        sourceBlock("kimi-api-quickstart", 0, 0),
        { type: "p", text: "Установите stream: true для инкрементального SSE. Terminal usage сохраняет Anthropic-форму, поэтому существующий parser usage продолжит работать." },
      ] },
      { h2: "Anthropic Python SDK", blocks: [
        sourceBlock("kimi-api-quickstart", 1, 0),
        { type: "note", text: "Не подставляйте Open Platform ID вроде kimi-k2.7-code. Публичный router принимает subscription aliases из GET /v1/models. OpenAI-совместимые клиенты вызывают те же Kimi aliases через единый route /v1." },
      ] },
    ],
    faq: [
      { q: "Можно использовать Anthropic SDK с Kimi?", a: "Да. Укажите base_url https://router.apitoken.sale и выберите kimi/* model ID из scoped-каталога." },
      { q: "Kimi поддерживает streaming?", a: "Да. Установите stream: true и обрабатывайте обычные инкрементальные Anthropic SSE events." },
      { q: "С какого model ID начать?", a: "kimi/kimi-for-coding — coding default; kimi/k3-256k — K3 reasoning без полного контекста 1M." },
    ],
  },
  "kimi-api-pricing": {
    title: "Цены Kimi API: как считается стоимость",
    h1: "Цены Kimi API: cache hit, miss, output и скорость",
    description: "Разбор цен Kimi K3, Kimi for Coding и High Speed: cache-hit, cache-miss, output, mapping aliases и скидка apiToken.sale 50%.",
    keywords: ["цены kimi api", "цена kimi k3", "цена kimi for coding", "стоимость токенов kimi", "цена kimi k2.7 code", "дешевый kimi api"],
    dek: "Kimi публикует отдельные ставки cache hit, cache miss и output. apiToken.sale тарифицирует фактически обслужившую модель, не смешивает usage-компоненты и применяет скидку 50%.",
    sections: [
      { h2: "Официальные ставки за публичными aliases", blocks: [
        { type: "table", headers: ["Публичный alias", "Официально hit / miss / output", "После скидки 50%"], rows: [
          ["kimi/k3 · k3-256k · k3[1m]", "$0.30 / $3 / $15", "$0.15 / $1.50 / $7.50"],
          ["kimi/kimi-for-coding", "$0.19 / $0.95 / $4", "$0.095 / $0.475 / $2"],
          ["kimi/kimi-for-coding-highspeed", "$0.38 / $1.90 / $8", "$0.19 / $0.95 / $4"],
        ] },
        { type: "p", text: "Все цены указаны за 1 млн токенов. Кэширование автоматическое. Отдельной цены cache write нет, поэтому новый cached-токен считается miss, а не бесплатным четвёртым компонентом." },
      ] },
      { h2: "Как контролировать расходы", blocks: [
        { type: "list", items: [
          "Kimi for Coding — самый экономичный общий coding-вариант.",
          "High Speed берите, только когда меньшая задержка оправдывает удвоенные токенные ставки.",
          "Используйте k3-256k вместо 1M-варианта, когда большой контекст не нужен.",
          "Задайте lifetime spending limit ключа и смотрите settled usage в дашборде.",
        ] },
        { type: "note", text: "Reasoning-токены входят в output и оплачиваются по output rate, а не второй отдельной строкой." },
      ] },
    ],
    faq: [
      { q: "Сколько стоит Kimi for Coding?", a: "Официально $0.19 за 1 млн cache-hit, $0.95 за cache-miss и $4 за output; apiToken.sale списывает половину." },
      { q: "Зачем разные цены cache hit и miss?", a: "Kimi автоматически кэширует повторный контекст. Terminal usage показывает, какие input-токены пришли из кэша, и каждый компонент получает свою ставку." },
      { q: "High Speed дороже?", a: "Да. Его cache-hit, cache-miss и output ставки ровно вдвое выше базового Kimi for Coding." },
    ],
  },
  "kimi-k3-vs-kimi-for-coding": {
    title: "Kimi K3 и Kimi for Coding: сравнение",
    h1: "Сравнение Kimi K3 и Kimi for Coding",
    description: "Сравните Kimi K3, K3 256K, Kimi for Coding и High Speed по контексту, reasoning, задержке и цене для кода и агентов.",
    keywords: ["kimi k3 или kimi for coding", "kimi k3 api", "kimi k2.7 code", "лучшая kimi для кода", "сравнение моделей kimi", "kimi highspeed"],
    dek: "K3 — семейство для reasoning и длинного контекста; Kimi for Coding — экономичная coding-линейка. High Speed покупает скорость за двойную ставку, а aliases K3 выбирают 256K или 1M.",
    sections: [
      { h2: "Карта семейства", blocks: [
        { type: "table", headers: ["Публичный ID", "Контекст", "Для чего подходит"], rows: [
          ["kimi/kimi-for-coding", "256K", "Повседневный код и экономичные agent loops"],
          ["kimi/kimi-for-coding-highspeed", "256K", "Latency-sensitive код, где скорость окупается"],
          ["kimi/k3-256k", "256K", "K3 reasoning без полного context mode"],
          ["kimi/k3 · kimi/k3[1m]", "1M", "Большие кодовые базы, документы и сложный reasoning"],
        ] },
        { type: "p", text: "k3[1m] — compatibility spelling режима K3 1M, а не отдельная модель. Router нормализует его в настоящий wire model k3." },
      ] },
      { h2: "Reasoning и маршрутизация", blocks: [
        { type: "list", items: [
          "K3 поддерживает low, high и max reasoning effort; default — high.",
          "Kimi for Coding и High Speed работают с включённым thinking.",
          "Доступность моделей берётся из scoped /v1/models перед фиксацией alias.",
          "Практический router отправляет обычный код в Kimi for Coding, а большие и сложные задачи — в K3.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какая Kimi лучше для программирования?", a: "Kimi for Coding — экономичный default. K3 выбирайте для сложного reasoning и длинного контекста, High Speed — только когда задержка важнее двойной цены." },
      { q: "k3 и k3[1m] — разные модели?", a: "Нет. Это один режим K3 1M; запись со скобками — compatibility alias." },
      { q: "Можно вызвать внутренний official model ID?", a: "Нет. Используйте публичные subscription aliases из router catalog, а не тарифные IDs вроде kimi-k2.7-code." },
    ],
  },
  "kimi-api-for-opencode": {
    title: "Как использовать Kimi API в OpenCode",
    h1: "Запускаем Kimi K3 и Kimi for Coding в OpenCode",
    description: "Подключите OpenCode к Kimi через apiToken.sale: router plugin, живой каталог, явные kimi/* IDs, streaming и один предоплаченный ключ.",
    keywords: ["kimi opencode", "kimi api opencode", "kimi k3 opencode", "настройка kimi for coding", "opencode custom provider", "kimi coding agent"],
    dek: "OpenCode умеет явно обращаться к namespace Kimi и читает живой каталог router. Это безопасный coding-agent вариант для переключения между K3 и Kimi for Coding без ручного списка лимитов.",
    sections: [
      { h2: "Установите и проверьте", blocks: [
        { type: "steps", items: [
          "Запустите installer apiToken.sale для OpenCode: он добавит router plugin и сохранит backup существующего config.",
          "Перезапустите OpenCode, чтобы plugin получил scoped-каталог моделей.",
          "Выполните один однозначный prompt с явной namespaced-моделью.",
        ] },
        sourceBlock("kimi-api-for-opencode", 0, 1),
      ] },
      { h2: "Безопасный выбор Kimi-модели", blocks: [
        { type: "list", items: [
          "apitoken/kimi/kimi-for-coding — экономичный coding default.",
          "apitoken/kimi/kimi-for-coding-highspeed — меньшая задержка за двойную токенную ставку.",
          "apitoken/kimi/k3-256k — K3 reasoning в меньшем context mode.",
          "apitoken/kimi/k3 — K3 с контекстом 1M, если он есть в каталоге.",
        ] },
        { type: "note", text: "Claude Code и Kimi Code тоже поддерживают Kimi, но настраиваются иначе: Claude Code требует закрепить каждый model tier, а Kimi Code — явный OpenAI-совместимый provider block." },
      ] },
    ],
    faq: [
      { q: "OpenCode поддерживает Kimi?", a: "Да. Router plugin apiToken.sale регистрирует живой Kimi namespace, а модель выбирается как apitoken/kimi/{model}." },
      { q: "Зачем plugin вместо статического списка?", a: "Он синхронизирует IDs, лимиты и доступность со scoped-каталогом ключа, поэтому retired или недоступные aliases не остаются в config." },
      { q: "Claude Code тоже работает с Kimi?", a: "Да, с другой настройкой. Направьте Claude Code на Anthropic endpoint и закрепите main, Opus, Sonnet, Haiku и subagent model variables на одном Kimi alias." },
    ],
  },
  "kimi-api-for-claude-code": {
    title: "Как использовать Kimi K3 в Claude Code",
    h1: "Kimi K3 и Kimi for Coding в Claude Code",
    description: "Настройте Claude Code для Kimi K3 или Kimi for Coding через apiToken.sale: закрепите все model tiers, сохраните контекст 1M и проверьте endpoint.",
    keywords: ["kimi claude code", "kimi k3 claude code", "kimi for coding claude code", "claude code custom model", "claude code kimi api", "k3 1m claude code"],
    dek: "Claude Code уже говорит на Anthropic Messages, поэтому может запускать Kimi напрямую. Надёжная настройка закрепляет каждый внутренний model tier на одном Kimi alias — иначе основная сессия работает, а subagents падают на унаследованной Claude-модели.",
    sections: [
      { h2: "Закрепите подключение и все model tiers", blocks: [
        sourceBlock("kimi-api-for-claude-code", 0, 0),
        { type: "p", text: "На Anthropic route используйте bare subscription alias. Для 256K-модели вроде k3-256k или kimi-for-coding оставьте tier pins, но уберите две переменные контекста 1M." },
      ] },
      { h2: "Проверяйте маршрут, а не самопрезентацию модели", blocks: [
        { type: "list", items: [
          "Откройте /status и убедитесь, что Anthropic base URL указывает на apiToken.sale.",
          "Не спрашивайте модель, кто она: system prompt Claude Code может заставить любой backend назвать себя Claude.",
          "Не отключайте thinking — это может изменить фактически обслуживающую Kimi-модель.",
          "Перед долгим закреплением alias проверьте GET /v1/models.",
        ] },
      ] },
    ],
    faq: [
      { q: "Claude Code поддерживает Kimi K3?", a: "Да. Укажите https://router.apitoken.sale и закрепите каждый model tier Claude Code на допущенном subscription alias Kimi." },
      { q: "Зачем закреплять все model variables Claude Code?", a: "Claude Code отдельно выбирает модели для основной сессии, tiers и subagents. Незакреплённый tier может унаследовать Claude ID и упасть только при запуске фонового пути." },
      { q: "Как сохранить полный контекст K3 1M в Claude Code?", a: "Используйте k3 или k3[1m] и установите CLAUDE_CODE_MAX_CONTEXT_TOKENS и CLAUDE_CODE_AUTO_COMPACT_WINDOW в 1048576." },
    ],
  },
  "kimi-api-for-kimi-code": {
    title: "Как использовать apiToken.sale в Kimi Code",
    h1: "Kimi, Claude, GPT и Gemini в Kimi Code",
    description: "Подключите Kimi Code к apiToken.sale через OpenAI-совместимый provider config, объявите namespaced-модель и защитите API-ключ в config.toml.",
    keywords: ["kimi code api", "kimi code custom provider", "kimi code config toml", "kimi code api ключ", "kimi code k3", "kimi code openai compatible"],
    dek: "Kimi Code принимает custom OpenAI-совместимый provider, поэтому одна запись apiToken.sale достигает единого каталога. Каждую модель нужно объявить отдельно с настоящим namespace и проверенным размером контекста.",
    sections: [
      { h2: "Установите CLI и объявите provider", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 0, 0),
        { type: "note", text: "Не запускайте /login: он привяжет CLI к Kimi membership. Custom provider credentials Kimi Code хранит только в config.toml, поэтому файл содержит ключ в открытом виде и должен быть защищён." },
      ] },
      { h2: "Запустите, проверьте и добавьте модели", blocks: [
        sourceBlock("kimi-api-for-kimi-code", 1, 0),
        { type: "list", items: [
          "/status должен показывать https://router.apitoken.sale/v1 как base URL провайдера.",
          "Поле model использует namespace единого каталога: например kimi/k3, openai/gpt-5.6-terra или google/gemini-3.6-flash.",
          "Объявляйте каждую дополнительную модель в config.toml с проверенным max_context_size — по нему Kimi Code решает, когда сжимать контекст.",
        ] },
      ] },
    ],
    faq: [
      { q: "Kimi Code работает с ключом apiToken.sale?", a: "Да. Добавьте OpenAI-совместимый provider с base_url https://router.apitoken.sale/v1 и сохраните ключ в config.toml Kimi Code." },
      { q: "Kimi Code может запускать не только Kimi?", a: "Да. Та же запись provider достигает единого каталога; объявите каждую Claude, GPT, Gemini или Kimi модель с namespaced ID и правильным лимитом контекста." },
      { q: "Зачем нужен chmod 600?", a: "Kimi Code не читает custom-provider credentials из shell. Сырой API-ключ лежит в config.toml, поэтому файл должен читаться только вашим аккаунтом." },
    ],
  },
};
