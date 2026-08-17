import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Доступ к Claude Sonnet API",
    h1: "Claude Sonnet через API",
    description: "Используйте Claude Sonnet 5 и Sonnet 4.6 через apiToken.sale — модель по умолчанию для ежедневного кодинга и агентов, с единой скидкой 50% от официальных цен API.",
    keywords: ["claude sonnet api", "claude sonnet 5 api", "sonnet api ключ", "цена claude sonnet", "лучшая модель claude для кода"],
    dek: "Sonnet — рабочая лошадка: достаточно быстрая для интерактивного кодинга и достаточно умная для реальных агентных сценариев. apiToken.sale отдаёт Sonnet 5 и Sonnet 4.6 на одном балансе со скидкой.",
    sections: [
      { h2: "Модель на каждый день", blocks: [
        { type: "p", text: "Для большинства задач кодинга и агентов Sonnet — правильный выбор по умолчанию: удачный баланс качества, скорости и стоимости. Opus оставляйте для по-настоящему сложных проблем." },
      ] },
      { h2: "Заметка о ценах Sonnet", blocks: [
        { type: "p", text: "Claude Sonnet 5 (claude-sonnet-5) идёт с вводными официальными тарифами, и движок всегда применяет текущую действующую ставку до вашей скидки. Sonnet 4.6 остаётся доступен на том же ключе." },
        { type: "table", headers: ["Модель", "Официально вход / выход ($ за 1 млн)", "Здесь (−50%)"], rows: [
          ["Claude Sonnet 5", "$3 / $15", "$1.50 / $7.50"],
          ["Claude Sonnet 4.6", "$3 / $15", "$1.50 / $7.50"],
        ] },
        { type: "link", text: "Подробные цены Claude Sonnet 5 (кэш, контекст, FAQ)", href: "/models/claude-sonnet-5" },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
    ],
    faq: [
      { q: "Какие модели Sonnet можно использовать?", a: "Claude Sonnet 5 (claude-sonnet-5) и Claude Sonnet 4.6 — на том же балансе, что Opus и Haiku." },
      { q: "Хорош ли Sonnet для кодинга?", a: "Да — Sonnet рекомендуется по умолчанию для повседневного кодинга и агентных сценариев." },
    ],
  };
