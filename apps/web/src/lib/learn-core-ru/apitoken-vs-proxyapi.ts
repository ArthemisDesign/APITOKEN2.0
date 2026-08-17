import type { LocalizedContent } from "../learn";

export const content: LocalizedContent = {
    title: "apiToken.sale против ProxyAPI для Claude",
    h1: "apiToken.sale против ProxyAPI",
    description: "Сравнение реселлеров Claude API: apiToken.sale предлагает нативный эндпоинт Anthropic с единой скидкой 50%, оплату картой или криптой и один ключ для всех моделей.",
    keywords: ["замена proxyapi", "apitoken vs proxyapi", "реселлер claude api", "proxyapi claude", "claude api без proxyapi"],
    dek: "Оба позволяют обращаться к Claude без аккаунта Anthropic. Разница — в способе оплаты, размере экономии и в том, насколько эндпоинт действительно нативен к Anthropic.",
    sections: [
      { h2: "Нативный эндпоинт Anthropic", blocks: [
        { type: "p", text: "apiToken.sale отдаёт стандартный Anthropic Messages API на https://router.apitoken.sale, поэтому Claude Code, Cursor и SDK Anthropic работают без изменений — между вами и Claude нет слоя-адаптера." },
      ] },
      { h2: "Скидка, а не наценка", blocks: [
        { type: "list", items: [
          "Единая B2C-скидка 50% от официального расхода Claude.",
          "Один предоплаченный ключ и баланс для Opus, Sonnet и Haiku.",
          "Пополнения картой или криптовалютой, которые не сгорают.",
        ] },
        { type: "note", text: "Новые аккаунты через Google или GitHub получают приветственный бонус $5 на баланс платформы; аккаунтам по email и паролю бонус не начисляется." },
      ] },
      { h2: "Кому что подходит", blocks: [
        { type: "list", items: [
          "apiToken.sale — нативный эндпоинт Anthropic с единой скидкой, общим лимитом расходов ключа и необязательной датой истечения.",
          "Универсальный реселлер — может подойти, если вы уже используете его других провайдеров.",
          "Оба убирают барьер аккаунта Anthropic; разница — в цене и в том, насколько нативен доступ к Claude.",
        ] },
      ] },
    ],
    faq: [
      { q: "Дешевле ли apiToken.sale обычного реселлера?", a: "Он применяет единую скидку 50% к официальному расходу Claude, а не добавляет наценку поверх прайса." },
      { q: "Будут ли работать мои инструменты Anthropic?", a: "Да — это нативный Anthropic Messages API, поэтому Claude Code, Cursor и SDK нужно лишь сменить base URL." },
    ],
  };
