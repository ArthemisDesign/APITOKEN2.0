import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Ключ Claude API для Cursor",
    h1: "Используйте ключ Claude API в Cursor",
    description: "Подключите Cursor к Claude с ключом apiToken.sale: задайте base URL Anthropic на router.apitoken.sale, вставьте ключ, выберите модель и кодьте с единой скидкой 50%.",
    keywords: ["ключ claude api для cursor", "cursor claude api", "cursor anthropic ключ", "использовать claude в cursor", "cursor без cursor pro"],
    dek: "Cursor позволяет подключить собственный ключ Anthropic, а значит, вы можете запускать Claude в Cursor на предоплаченном балансе со скидкой вместо встроенного тарифа.",
    sections: [
      { h2: "Настройка в три шага", blocks: [
        { type: "steps", items: [
          "Откройте Cursor → Settings → Models → Anthropic API.",
          "Задайте base URL на https://router.apitoken.sale и вставьте ваш ключ sk-pool-•••.",
          "Выберите модель, например claude-opus-4-8, и начинайте кодить.",
        ] },
      ] },
      { h2: "Конфигурация", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Решение проблем", blocks: [
        { type: "list", items: [
          "Cursor игнорирует ключ: убедитесь, что вы редактировали провайдера Anthropic, а не OpenAI.",
          "Модель не найдена: задайте актуальный идентификатор, например claude-opus-4-8.",
          "401: перепроверьте base URL и что ключ вставлен целиком.",
        ] },
        { type: "p", text: "После подключения все поддерживаемые модели Claude доступны на одном ключе и балансе." },
      ] },
      { h2: "Ключ Claude API в Cursor для любого языка", blocks: [
        { type: "p", text: "Ключ не привязан к языку — Cursor использует его для Python, JavaScript, TypeScript, Go, Rust и любого другого проекта, на Windows, macOS и Linux. Вы настраиваете провайдера модели, а не язык." },
      ] },
    ],
    faq: [
      { q: "Можно ли использовать свой ключ Claude в Cursor?", a: "Да. Провайдер Anthropic в Cursor принимает кастомный base URL и ключ, поэтому можно направить его на apiToken.sale." },
      { q: "Нужен ли всё ещё Cursor Pro?", a: "Вы можете запускать Claude через собственный ключ API и баланс; функции, требующие собственного тарифа Cursor, — это отдельная от провайдера модели вещь." },
      { q: "Работает ли ключ Claude API в Cursor на Windows и Mac?", a: "Да — настройка провайдера Anthropic одинакова на Windows, macOS и Linux." },
    ],
  };
