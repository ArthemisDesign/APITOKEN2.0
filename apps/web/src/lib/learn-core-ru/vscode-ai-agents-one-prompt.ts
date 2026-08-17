import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Бесплатные ИИ-агенты в VS Code на Claude",
    h1: "Запускайте бесплатные ИИ-агенты VS Code на Claude",
    description: "Настройте бесплатные VS Code-агенты вроде Cline и Roo Code с ключом Claude от apiToken.sale — без Cursor Pro. Один эндпоинт, все модели Claude, со скидкой.",
    keywords: ["бесплатный ии-агент vscode", "cline roo code claude", "claude агент vscode", "бесплатная замена cursor", "claude vscode без cursor"],
    dek: "Чтобы получить агентное программирование, Cursor Pro не нужен. Бесплатные VS Code-агенты принимают любой Anthropic-совместимый ключ, поэтому Claude работает в VS Code на балансе со скидкой.",
    sections: [
      { h2: "Направьте агента на Claude", blocks: [
        { type: "steps", items: [
          "Установите бесплатное расширение-агент, например Cline или Roo Code.",
          "Выберите Anthropic в качестве провайдера API.",
          "Задайте base URL на https://router.apitoken.sale, вставьте ваш ключ sk-pool-••• и выберите модель, например claude-sonnet-5.",
        ] },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Подбирайте модель под задачу", blocks: [
        { type: "list", items: [
          "claude-sonnet-5 — выбор по умолчанию для повседневного кодинга и агентных циклов.",
          "claude-opus-4-8 — сложные рефакторинги, архитектура и длинные сессии.",
          "claude-haiku-4-5 — быстрые дешёвые правки и высокочастотные шаги.",
        ] },
        { type: "p", text: "Поскольку один ключ покрывает все модели, вы можете переключаться под задачу прямо в расширении, не меняя аккаунт или биллинг." },
      ] },
    ],
    faq: [
      { q: "Нужен ли Cursor Pro для ИИ-кодинга?", a: "Нет. Бесплатные VS Code-агенты вроде Cline и Roo Code работают с ключом Claude от apiToken.sale." },
      { q: "Какую модель выбрать?", a: "claude-sonnet-5 для повседневного кодинга; claude-opus-4-8 для сложных задач." },
    ],
  };
