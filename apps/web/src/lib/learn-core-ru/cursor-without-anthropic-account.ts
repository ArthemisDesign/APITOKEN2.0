import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "Claude в Cursor без аккаунта Anthropic",
    h1: "Запустите Claude в Cursor без аккаунта Anthropic",
    description: "Нет аккаунта Anthropic? Используйте Claude в Cursor с ключом apiToken.sale. Мгновенный доступ, оплата картой или криптой и единая скидка 50% от официальных цен API.",
    keywords: ["cursor без аккаунта anthropic", "claude cursor без anthropic", "cursor ключ claude api", "использовать claude без аккаунта anthropic"],
    dek: "Если вы не можете или не хотите создавать аккаунт Anthropic, apiToken.sale выдаёт собственный ключ, который Cursor принимает как провайдера Anthropic.",
    sections: [
      { h2: "Почему это работает", blocks: [
        { type: "p", text: "Cursor общается с Anthropic Messages API. apiToken.sale отдаёт ровно этот API, поэтому Cursor не видит разницы — он просто использует ваш ключ и base URL." },
      ] },
      { h2: "Настройте", blocks: [
        { type: "code", code: `# Cursor → Settings → Models → Anthropic API\nBase URL : https://router.apitoken.sale\nAPI key  : sk-pool-•••\nModel    : claude-opus-4-8` },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Что вы сохраняете", blocks: [
        { type: "list", items: [
          "Всю линейку Claude — Opus, Sonnet и Haiku — на одном ключе.",
          "Стандартное поведение Anthropic: стриминг, использование инструментов, системные промпты.",
          "Необязательный общий лимит расходов за всё время и дата истечения для каждого ключа, плюс потокенный расход в панели.",
        ] },
        { type: "p", text: "В том, как вы пользуетесь Cursor, ничего не меняется; вы просто берёте ключ из apiToken.sale, а не из Anthropic." },
      ] },
    ],
    faq: [
      { q: "Нужен ли для этого аккаунт Anthropic?", a: "Нет. apiToken.sale предоставляет ключ и баланс, поэтому аккаунт Anthropic не требуется." },
      { q: "Это официальный Anthropic API?", a: "Cursor использует стандартный Anthropic Messages API; apiToken.sale отдаёт тот же самый API со скидкой." },
    ],
  };
