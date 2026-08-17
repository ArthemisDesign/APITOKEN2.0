import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Официальные SDK Anthropic с кастомным base URL",
    h1: "Направьте SDK Anthropic на apiToken.sale",
    description: "Используйте официальные SDK Anthropic для Python и TypeScript с apiToken.sale, задав base_url на router.apitoken.sale. Тот же SDK, тот же код, ниже цена за токен.",
    keywords: ["anthropic sdk base url", "anthropic python sdk кастомный endpoint", "claude sdk base url", "claude api sdk", "anthropic typescript sdk"],
    dek: "Официальные SDK Anthropic позволяют переопределить base URL, поэтому переход на apiToken.sale — это изменение в одну строку: идентификаторы моделей и код работы с сообщениями остаются ровно теми же.",
    sections: [
      { h2: "Python", blocks: [
        { type: "code", code: `from anthropic import Anthropic\n\nclient = Anthropic(\n    base_url="https://router.apitoken.sale",\n    api_key="sk-pool-•••",\n)\nmsg = client.messages.create(\n    model="claude-opus-4-8",\n    max_tokens=1024,\n    messages=[{"role": "user", "content": "Hello"}],\n)` },
      ] },
      { h2: "TypeScript", blocks: [
        { type: "code", code: `import Anthropic from "@anthropic-ai/sdk";\n\nconst client = new Anthropic({\n  baseURL: "https://router.apitoken.sale",\n  apiKey: "sk-pool-•••",\n});\nconst msg = await client.messages.create({\n  model: "claude-opus-4-8",\n  max_tokens: 1024,\n  messages: [{ role: "user", content: "Hello" }],\n});` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Проверьте, что переключение сработало", blocks: [
        { type: "p", text: "После смены base URL сделайте один запрос и убедитесь, что получаете обычный ответ Anthropic. Стриминг, использование инструментов и системные промпты ведут себя точно так же, как с api.anthropic.com — изменился только биллинговый эндпоинт." },
        { type: "list", items: [
          "Ошибка 401 означает неверный ключ или base URL — перепроверьте оба.",
          "Оставляйте те же идентификаторы моделей; код вокруг сообщений менять не нужно.",
          "Смотрите расход по каждому запросу в панели, чтобы подтвердить траты и вашу скидку.",
        ] },
      ] },
    ],
    faq: [
      { q: "Можно ли и дальше пользоваться официальным SDK Anthropic?", a: "Да. Задайте base_url (Python) или baseURL (TypeScript) на apiToken.sale, и всё остальное остаётся прежним." },
      { q: "Меняются ли идентификаторы моделей?", a: "Нет. Используйте те же идентификаторы, например claude-opus-4-8 и claude-sonnet-5." },
    ],
  };
