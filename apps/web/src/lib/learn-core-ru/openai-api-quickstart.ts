import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "OpenAI-совместимый API: быстрый старт — GPT-5.6 на одном ключе",
    h1: "OpenAI-совместимый API: Responses и Chat Completions",
    description: "Запускайте модели GPT-5.6 на apiToken.sale через OpenAI-совместимый API — Responses и Chat Completions со SSE-стримингом, один ключ sk-pool и баланс, общий с Claude, с единой скидкой 50%.",
    keywords: ["openai совместимый api", "gpt-5.6 api", "responses api", "chat completions свой base url", "openai sdk base_url", "gpt api ключ", "цена gpt-5.6"],
    dek: "Ваш ключ sk-pool работает не только с Claude. Тот же ключ и предоплаченный баланс открывают линейку GPT-5 через OpenAI-совместимый эндпоинт — стандартные вызовы Responses и Chat Completions, официальные SDK OpenAI, SSE-стриминг и та же единая скидка 50%.",
    sections: [
      { h2: "Три шага до первого вызова GPT", blocks: [
        { type: "steps", items: [
          "Создайте бесплатный аккаунт и выпустите один API-ключ (вида sk-pool-…) — он уже покрывает и модели Claude.",
          "Направьте клиент на https://router.apitoken.sale/v1 и используйте Authorization: Bearer — не x-api-key: тот заголовок относится к Anthropic-поверхности.",
          "Проверьте доступные модели через GET https://router.apitoken.sale/v1/models — единый каталог разделяет ID по провайдерам (anthropic/*, openai/*, google/*) — затем отправьте запрос Responses.",
        ] },
        { type: "code", code: `curl https://router.apitoken.sale/v1/responses \\\n  -H "Authorization: Bearer $APITOKEN_API_KEY" \\\n  -H "Content-Type: application/json" \\\n  -d '{\n    "model": "gpt-5.6-sol",\n    "input": "Reply with exactly: connected"\n  }'` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы для поддерживаемых Claude, GPT, Gemini и Kimi; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Официальный OpenAI SDK", blocks: [
        { type: "p", text: "Официальные SDK работают без изменений — меняются только base_url и ключ. В production храните ключ в серверной переменной окружения." },
        { type: "code", code: `import os\nfrom openai import OpenAI\n\nclient = OpenAI(\n    api_key=os.environ["APITOKEN_API_KEY"],\n    base_url="https://router.apitoken.sale/v1",\n)\n\nresponse = client.responses.create(\n    model="gpt-5.6-sol",\n    input="Reply with exactly: connected",\n)\nprint(response.output_text)` },
        { type: "p", text: "Chat Completions работает на том же хосте, если ваш клиент ожидает его — ID модели и ключ те же." },
        { type: "code", code: `completion = client.chat.completions.create(\n    model="gpt-5.6-sol",\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(completion.choices[0].message.content)` },
      ] },
      { h2: "Какие модели GPT доступны", blocks: [
        { type: "p", text: "Набор моделей закреплён и тарифицирован в движке; GET https://router.apitoken.sale/v1/models — всегда актуальный ответ. Сегодня линейка включает три уровня GPT-5.6 и две модели прошлого поколения:" },
        { type: "table", headers: ["ID модели", "Уровень", "Офиц. вход / выход ($ за 1M)", "Кэш входа"], rows: [
          ["gpt-5.6-sol (псевдоним: gpt-5.6)", "Флагман", "$5 / $30", "$0.50"],
          ["gpt-5.6-terra", "Сбалансированная", "$2 / $12", "$0.20"],
          ["gpt-5.6-luna", "Быстрая", "$0.20 / $1.20", "$0.02"],
          ["gpt-5.5", "Флагман прошлого поколения", "$5 / $30", "$0.50"],
          ["gpt-5.4", "Сбалансированная прошлого поколения", "$2.50 / $15", "$0.25"],
        ] },
        { type: "list", items: [
          "Усилие рассуждений настраивается на запрос — от none до xhigh у всех моделей, плюс max в линейке GPT-5.6.",
          "Все модели принимают текст и изображения на входе и стримят по SSE в Responses и Chat Completions.",
          "Запросы свыше 272K входных токенов тарифицируются по ставкам OpenAI для длинного контекста: ×2 вход и ×1,5 выход за весь запрос.",
          "Ваша скидка B2C действует здесь так же, как и для Claude, — один баланс, одна ставка, 50% от официального расхода.",
        ] },
        { type: "link", text: "Полные характеристики моделей и цены со скидкой", href: "/models" },
      ] },
      { h2: "Что эндпоинт покрывает, а что нет", blocks: [
        { type: "p", text: "Это независимый OpenAI-совместимый сервис, а не OpenAI Platform. Он обслуживает каталог, streaming Responses и Chat Completions, а также отдельные routes генерации и редактирования GPT Image 2. Endpoints для audio, files, realtime, assistants, batch и fine-tuning недоступны." },
        { type: "note", text: "Ошибки приходят в конверте OpenAI — {\"error\":{\"message\",\"type\",\"param\",\"code\"}}. 401 означает неверный ключ или заголовок (нужен Bearer, не x-api-key), 402 — предоплаченный баланс нужно пополнить, 404 — ID модели не включён: проверьте GET https://router.apitoken.sale/v1/models." },
      ] },
    ],
    faq: [
      { q: "Тот же ключ работает не только с GPT?", a: "Да. Один sk-pool ключ и баланс также покрывают поддерживаемые Claude, Gemini и Kimi; используйте протокол и заголовок авторизации нужного провайдера." },
      { q: "Какой заголовок авторизации у OpenAI-совместимого эндпоинта?", a: "Authorization: Bearer sk-pool-…. Заголовок x-api-key — только для Anthropic-поверхности; с ним OpenAI-эндпоинт вернёт 401." },
      { q: "Responses или Chat Completions?", a: "Обе службы доступны со SSE-стримингом. Для нового кода и официальных SDK берите Responses; Chat Completions подходит клиентам и фреймворкам, ожидающим классическую форму." },
      { q: "Как тарифицируется использование GPT?", a: "За токены по официальным ставкам OpenAI — включая кэш входа и длинный контекст, — затем ваша единая скидка B2C 50% вычитается перед списанием с предоплаченного баланса, ровно как у Claude." },
    ],
  };
