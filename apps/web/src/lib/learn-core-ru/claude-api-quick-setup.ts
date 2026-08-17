import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Настройка Claude API за две минуты",
    h1: "Настройте Claude API за две минуты",
    description: "Быстрый старт Claude API за две минуты: создайте ключ, укажите base URL router.apitoken.sale и отправьте первый запрос /v1/messages через curl, Python или IDE.",
    keywords: ["claude api быстрый старт", "claude api настройка", "claude api первый запрос", "anthropic messages api", "claude api base url"],
    dek: "Это самый быстрый путь от нуля до рабочего вызова Claude API. Всё ниже использует стандартный Anthropic Messages API, поэтому встраивается прямо в ваш существующий код.",
    sections: [
      { h2: "1. Создайте ключ", blocks: [ { type: "p", text: "Зарегистрируйтесь, откройте панель и сгенерируйте ключ. Он выглядит как sk-pool-… и работает со всеми поддерживаемыми моделями." } ] },
      { h2: "2. Укажите эндпоинт", blocks: [
        { type: "p", text: "Направьте любой Anthropic-совместимый клиент на шлюз:" },
        { type: "code", code: `Base URL:  https://router.apitoken.sale\nEndpoint:  POST /v1/messages\nHeaders:   x-api-key: sk-pool-•••\n           anthropic-version: 2023-06-01` },
      ] },
      { h2: "3. Отправьте первый запрос", blocks: [
        { type: "code", code: `curl https://router.apitoken.sale/v1/messages \\\n  -H "x-api-key: sk-pool-•••" \\\n  -H "anthropic-version: 2023-06-01" \\\n  -H "content-type: application/json" \\\n  -d '{\n    "model": "claude-opus-4-8",\n    "max_tokens": 1024,\n    "messages": [{"role":"user","content":"Hello"}]\n  }'` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Частые ошибки первого вызова", blocks: [
        { type: "list", items: [
          "401 Unauthorized — отсутствует или неверен x-api-key, либо неправильный base URL.",
          "400 Bad Request — проверьте идентификатор модели и что задан max_tokens.",
          "429 Too Many Requests — учитывайте Retry-After и снизьте параллелизм.",
          "402 / недостаточно баланса — пополните на любое целое число долларов.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какой base URL использовать?", a: "Используйте https://router.apitoken.sale с любым Anthropic-совместимым инструментом и отправляйте запросы на /v1/messages. Существующие интеграции на прежнем хосте https://api.apitoken.sale продолжают работать — единый роутер просто рекомендуемый эндпоинт для новых настроек." },
      { q: "Какой заголовок авторизации нужен?", a: "Отправляйте x-api-key с вашим ключом и anthropic-version — ровно как в официальном Anthropic API." },
    ],
  };
