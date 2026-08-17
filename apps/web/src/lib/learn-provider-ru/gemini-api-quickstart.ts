import type { LocalizedContent } from "../learn";
import { sourceBlock } from "./shared";

export const content: LocalizedContent = {
    title: "Gemini API Quickstart: первый запрос за минуты",
    h1: "Быстрый старт Gemini API: первый запрос через curl и Google GenAI SDK",
    description: "Gemini API quickstart: первый запрос через apiToken.sale — нативный generateContent с curl или Google GenAI SDK, аутентификация x-goog-api-key, SSE-стриминг и явные model ID.",
    keywords: ["gemini api quickstart", "быстрый старт gemini api", "gemini api curl пример", "google genai sdk base url", "gemini generatecontent api", "gemini api python пример", "gemini api стриминг", "x-goog-api-key header", "как вызвать gemini api", "gemini api javascript sdk"],
    dek: "Этот quickstart по Gemini API доводит до первого рабочего запроса за минуты: один curl на нативный маршрут generateContent, затем тот же вызов из официального Google GenAI SDK на Python или JavaScript. Меняются только base URL и заголовок с ключом — формы запросов, стриминг и метаданные usage остаются в точности такими, как их описывает Google.",
    sections: [
      { h2: "Один endpoint, нативный протокол Gemini", blocks: [
        { type: "p", text: "Чтобы сделать первый запрос к Gemini API через apiToken.sale, оставьте протокол Google как в документации и поменяйте только два значения: base URL становится https://router.apitoken.sale, а ключ — ваш ключ apiToken.sale в заголовке x-goog-api-key. Каждый запрос и ответ сохраняет нативную форму generateContent, поэтому документация Google, примеры SDK и любой ваш существующий код под Gemini работают без изменений." },
        { type: "p", text: "Один ключ и один предоплатный баланс покрывают всех поддерживаемых провайдеров — Gemini наряду с Claude, GPT и Kimi. Использование Gemini тарифицируется по официальным токен-ставкам Google, а перед списанием с баланса применяется фиксированная скидка 50%. Никакого проекта Google Cloud или платёжного аккаунта с вашей стороны не требуется." },
      ] },
      { h2: "Создайте ключ и посмотрите свой каталог", blocks: [
        { type: "steps", items: [
          "Создайте бесплатный аккаунт apiToken.sale и откройте дашборд — без согласований и waitlist.",
          "Сгенерируйте один API key. Он выглядит как sk-pool-… и одинаково работает для Gemini, Claude, GPT и Kimi.",
          "Пополните баланс на любую целую сумму в долларах картой или криптой; предоплатный баланс не сгорает.",
          "Экспортируйте ключ как APITOKEN_API_KEY и запросите список моделей, которые реально доступны вашему ключу:",
        ] },
        sourceBlock("gemini-api-quickstart", 1, 1),
        { type: "p", text: "Выберите из ответа явный model ID. gemini-3.6-flash — правильный дефолт для первого текстового запроса: встроенного дефолта клиентской библиотеки может не оказаться в каталоге шлюза, а router обслуживает только те ID, которые перечисляет." },
        { type: "note", text: "Новые аккаунты, созданные через Google или GitHub, получают $5 бонусных средств платформы — они действуют на поддерживаемых моделях Claude, GPT, Gemini и Kimi; аккаунты с email и паролем бонус не получают." },
      ] },
      { h2: "Первый запрос: generateContent через curl", blocks: [
        sourceBlock("gemini-api-quickstart", 2, 0),
        { type: "p", text: "Ответ — стандартная форма Google: читайте candidates[0].content.parts и склейте текстовые части. В том же JSON приходит usageMetadata со счётчиками токенов prompt, candidate и total, так что код учёта токенов и расходов работает с самого первого вызова." },
        { type: "p", text: "Перед отправкой большого промпта вызовите :countTokens на том же пути модели. Он возвращает число токенов, ничего не генерируя, — бесплатная оценка входа до того, как вы потратите деньги на генерацию." },
      ] },
      { h2: "Стриминг токенов через streamGenerateContent", blocks: [
        sourceBlock("gemini-api-quickstart", 3, 0),
        { type: "p", text: "Параметр ?alt=sse переводит ответ в server-sent events: каждое событие — один инкрементальный чанк в той же структуре candidate, а финальное событие несёт суммарную usageMetadata. В SDK тот же маршрут вызывается через generate_content_stream в Python и generateContentStream в JavaScript." },
        { type: "p", text: "Стримьте всё, что видит пользователь, чтобы первые токены отрисовывались сразу. Для пакетных задач, где важен только итоговый текст, обычный generateContent проще парсить и повторять." },
      ] },
      { h2: "Официальные SDK: Python и JavaScript", blocks: [
        sourceBlock("gemini-api-quickstart", 4, 0),
        sourceBlock("gemini-api-quickstart", 4, 1),
        { type: "list", items: [
          "Передавайте голый base URL https://router.apitoken.sale; не добавляйте /v1beta в конфигурацию SDK.",
          "Передавайте конкретный model ID вроде gemini-3.6-flash — никогда не полагайтесь на дефолт клиента.",
          "Держите APITOKEN_API_KEY в переменных окружения, а не в исходном коде.",
        ] },
        { type: "note", text: "Если каждый запрос SDK возвращает 404, проверьте путь на удвоенный сегмент /v1beta/v1beta. SDK сам подставляет версию API; если в конфигурации хоста /v1beta уже указан, получается удвоенный путь." },
      ] },
      { h2: "Сколько стоят первые запросы", blocks: [
        { type: "p", text: "Вызовы Gemini рассчитываются по точным официальным ставкам Google — input, cached input и output — а сверху применяется фиксированная скидка 50%. Цены после скидки за 1M токенов для основных текстовых моделей:" },
        { type: "table", headers: ["Модель", "Input / cached / output за 1M", "Подходящая первая задача"], rows: [
          ["gemini-3.6-flash", "$0.75 / $0.075 / $3.75", "Повседневный кодинг, чат и агенты"],
          ["gemini-3.1-flash-lite", "$0.125 / $0.0125 / $0.75", "Классификация, извлечение, роутинг"],
          ["gemini-2.5-flash-lite", "$0.05 / $0.005 / $0.20", "Самый дешёвый текст в больших объёмах"],
          ["gemini-3.1-pro-preview", "$1 / $0.10 / $6", "Самое сложное рассуждение и ревью"],
        ] },
        { type: "p", text: "Gemini 3.1 Flash Image (Nano Banana 2) работает на том же маршруте с тем же ключом; сгенерированное изображение — отдельная ценовая нога, описанная в гайде по изображениям. Расход по каждому запросу и применённая скидка видны в дашборде после каждого вызова." },
        { type: "link", text: "Полный прайс Gemini, включая длинный контекст и генерацию изображений", href: "/docs/learn/gemini-api-pricing" },
        { type: "link", text: "Все поддерживаемые model ID и цены", href: "/models" },
      ] },
      { h2: "Разбор проблем первого ответа", blocks: [
        { type: "table", headers: ["Статус", "Вероятная причина", "Решение"], rows: [
          ["401", "Отсутствует или неверен x-goog-api-key", "Проверьте значение ключа и точное имя заголовка"],
          ["404", "Удвоенный /v1beta или model ID не из каталога", "Передавайте голый host; выберите ID из GET /v1beta/models"],
          ["402", "Предоплатный баланс исчерпан", "Пополните баланс на любую целую сумму в долларах в дашборде"],
        ] },
        { type: "p", text: "Не отправляйте Authorization: Bearer или Anthropic-заголовок x-api-key на нативных маршрутах Gemini — x-goog-api-key единственный credential, который они принимают. Поскольку формат на проводе не меняется, возврат на собственный endpoint Google позже сводится к изменению base URL в одну строку." },
        { type: "link", text: "Как выбрать между Pro, Flash и Flash-Lite", href: "/docs/learn/gemini-pro-vs-flash-vs-flash-lite" },
        { type: "link", text: "Генерация изображений с Nano Banana 2", href: "/docs/learn/nano-banana-2-api-guide" },
      ] },
    ],
    faq: [
      { q: "Работает ли официальный Google GenAI SDK с apiToken.sale?", a: "Да. Установите HttpOptions(base_url) в Python или httpOptions.baseUrl в JavaScript на https://router.apitoken.sale и передайте ключ apiToken.sale; формы запросов и ответов остаются нативными." },
      { q: "Какой заголовок аутентифицирует запросы к Gemini API?", a: "x-goog-api-key с вашим ключом sk-pool. Нативные маршруты Gemini не принимают Authorization: Bearer и Anthropic-заголовок x-api-key." },
      { q: "Как стримить вывод Gemini?", a: "Вызовите /v1beta/models/{model}:streamGenerateContent?alt=sse с x-goog-api-key или используйте метод SDK generate_content_stream / generateContentStream. Финальное SSE-событие несёт суммарную usageMetadata." },
      { q: "Почему удвоенный /v1beta возвращает 404?", a: "Google SDK сам добавляет версию API к настроенному хосту. Укажите только голый host, чтобы в итоговом запросе был ровно один сегмент /v1beta." },
      { q: "Какую модель Gemini вызвать первой?", a: "Начните с gemini-3.6-flash для обычного текста и кодинга. Массовую классификацию перенесите на модель Flash-Lite, а самое сложное рассуждение — на gemini-3.1-pro-preview." },
      { q: "Бесплатен ли вызов countTokens?", a: "Да. Вызов :countTokens на пути модели возвращает число токенов без генерации, так что размер входа можно оценить до оплаты генерации." },
    ],
  };
