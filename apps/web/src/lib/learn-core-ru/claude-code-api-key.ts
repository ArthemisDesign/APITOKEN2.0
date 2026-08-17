import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Настройка Claude Code с ключом API",
    h1: "Используйте Claude Code с ключом apiToken.sale",
    description: "Настройте Claude Code с ключом apiToken.sale в двух переменных окружения и запускайте любую модель Claude на предоплаченном балансе с единой скидкой 50%.",
    keywords: ["claude code api ключ", "claude code настройка", "claude code anthropic base url", "claude code кастомный ключ", "claude code дешевле"],
    dek: "Claude Code читает две переменные окружения. Направьте их на apiToken.sale — и вы сохраняете все функции, оплачивая работу из предоплаченного баланса со скидкой.",
    sections: [
      { h2: "Две переменные", blocks: [
        { type: "code", code: `export ANTHROPIC_BASE_URL=https://router.apitoken.sale\nexport ANTHROPIC_API_KEY=sk-pool-•••\n\n# then just run\nclaude` },
        { type: "p", text: "Это вся настройка. Используйте claude-opus-4-8 для сложной работы и claude-sonnet-5 для повседневного кодинга." },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Проверьте и выберите модель", blocks: [
        { type: "p", text: "Сначала запустите небольшой промпт, чтобы убедиться, что ключ работает, затем задайте модель по умолчанию. Если Claude Code сообщает об ошибке авторизации, перепроверьте обе переменные окружения и перезапустите шелл, чтобы они экспортировались." },
        { type: "list", items: [
          "Повседневный кодинг: claude-sonnet-5.",
          "Сложные рефакторинги и длинные сессии: claude-opus-4-8.",
          "Смотрите расход токенов по каждому запросу в панели, чтобы отслеживать траты.",
        ] },
      ] },
    ],
    faq: [
      { q: "Как направить Claude Code на apiToken.sale?", a: "Задайте ANTHROPIC_BASE_URL и ANTHROPIC_API_KEY на ваш эндпоинт и ключ apiToken.sale, затем запустите claude." },
      { q: "Сохраняются ли все функции Claude Code?", a: "Да — меняется только биллинг: с подписки на предоплаченное использование со скидкой." },
    ],
  };
