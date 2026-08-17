import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude API в VS Code (Cline, Continue)",
    h1: "Используйте Claude API в VS Code",
    description: "Запускайте Claude в VS Code через Cline или Continue с ключом apiToken.sale. Задайте base URL Anthropic на router.apitoken.sale и платите за токены со скидкой.",
    keywords: ["claude api vs code", "cline claude api", "continue claude api", "claude в vscode", "vscode anthropic api ключ"],
    dek: "Бесплатные VS Code-агенты вроде Cline и Continue принимают любой Anthropic-совместимый эндпоинт, поэтому вы можете кодить с Claude прямо в VS Code на балансе со скидкой.",
    sections: [
      { h2: "Cline", blocks: [
        { type: "code", code: `# Cline → Settings\nAPI Provider : Anthropic\nBase URL     : https://router.apitoken.sale\nAPI Key      : sk-pool-•••\nModel        : claude-opus-4-8` },
      ] },
      { h2: "Continue", blocks: [
        { type: "code", code: `// ~/.continue/config.json\n{\n  "models": [{\n    "title": "Claude via apiToken.sale",\n    "provider": "anthropic",\n    "apiBase": "https://router.apitoken.sale",\n    "apiKey": "sk-pool-•••",\n    "model": "claude-opus-4-8"\n  }]\n}` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Какое расширение выбрать и решение проблем", blocks: [
        { type: "p", text: "Cline — отличный выбор по умолчанию для автономных правок; Continue легче и хорош для инлайн-чата и автодополнений. Оба бесплатны и используют ваш предоплаченный баланс." },
        { type: "list", items: [
          "401 Unauthorized: неверный ключ API или base URL.",
          "Модель не найдена: используйте актуальный идентификатор, например claude-sonnet-5 или claude-opus-4-8.",
          "Медленно или 429: снизьте параллелизм и учитывайте Retry-After.",
        ] },
      ] },
    ],
    faq: [
      { q: "Какие расширения VS Code подходят?", a: "С ключом apiToken.sale работает любое расширение, поддерживающее Anthropic-совместимый эндпоинт, включая Cline и Continue." },
      { q: "Нужно ли платное расширение?", a: "Нет. Cline и Continue бесплатны; вы платите только за использование Claude API из вашего предоплаченного баланса." },
    ],
  };
