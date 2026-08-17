import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale против OpenRouter для Claude",
    h1: "apiToken.sale против OpenRouter для Claude",
    description: "Выбираете шлюз для Claude? Сравните apiToken.sale и OpenRouter: нативный эндпоинт Anthropic со скидкой на предоплате против мультипровайдерного маршрутизатора.",
    keywords: ["замена openrouter", "apitoken vs openrouter", "claude api шлюз", "openrouter claude", "лучший шлюз claude api"],
    dek: "Оба позволяют обращаться к Claude без аккаунта Anthropic, но устроены по-разному. Если Claude — ваша основная модель, нативный эндпоинт Anthropic всё упрощает.",
    sections: [
      { h2: "Нативный эндпоинт Anthropic", blocks: [
        { type: "p", text: "apiToken.sale отдаёт стандартный Anthropic Messages API на https://router.apitoken.sale, поэтому Claude Code, Cursor и SDK Anthropic работают без адаптеров. Вы не маршрутизируете через универсальную мультипровайдерную абстракцию." },
      ] },
      { h2: "Скидка на предоплате, а не наценка", blocks: [
        { type: "list", items: [
          "Единая B2C-скидка 50% от официального расхода Claude.",
          "Один ключ и баланс для Opus, Sonnet и Haiku.",
          "Пополнения картой или криптой, которые не сгорают.",
        ] },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Когда что выбрать", blocks: [
        { type: "list", items: [
          "apiToken.sale — Claude ваша основная модель и вам нужен нативный эндпоинт Anthropic со скидкой.",
          "OpenRouter — вам нужно маршрутизировать между многими провайдерами за одной абстракцией.",
          "Оба позволяют стартовать без аккаунта Anthropic; но только apiToken.sale напрямую снижает расход на Claude.",
        ] },
      ] },
    ],
    faq: [
      { q: "Зачем выбирать Claude-нативный шлюз?", a: "Если Claude ваша основная модель, нативный эндпоинт Anthropic означает, что ваши существующие инструменты и SDK Anthropic работают без изменений." },
      { q: "Делает ли apiToken.sale наценку?", a: "Нет — вместо наценки применяется скидка к официальному расходу Claude." },
    ],
  };
